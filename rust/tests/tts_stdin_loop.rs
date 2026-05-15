//! Integration tests for `kesha-engine say --stdin-loop` (issue #213).
//!
//! Frame format:
//!
//! ```text
//! <status:u8><id:u32 LE><len:u32 LE><payload:[u8; len]>
//! ```
//!
//! The interesting tests need real Kokoro model files and run when
//! `KOKORO_MODEL` + `KOKORO_VOICE` env vars are set (matching the convention
//! in `tts_smoke.rs`). The protocol-only tests (malformed JSON, empty text,
//! framing layout) run unconditionally.

#![cfg(feature = "tts")]

mod common;

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::Duration;

use kesha_engine::tts::{self, EngineChoice, OutputFormat, SayOptions};

const STATUS_OK: u8 = 0;
const STATUS_ERR: u8 = 1;

/// A response frame parsed off the engine's stdout.
struct Frame {
    status: u8,
    id: u32,
    payload: Vec<u8>,
}

/// Read exactly `n` bytes or return an Err on early EOF.
fn read_exact(r: &mut impl Read, n: usize) -> std::io::Result<Vec<u8>> {
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

fn read_frame(r: &mut impl Read) -> std::io::Result<Frame> {
    let header = read_exact(r, 9)?;
    let status = header[0];
    let id = u32::from_le_bytes([header[1], header[2], header[3], header[4]]);
    let len = u32::from_le_bytes([header[5], header[6], header[7], header[8]]) as usize;
    let payload = read_exact(r, len)?;
    Ok(Frame {
        status,
        id,
        payload,
    })
}

struct LoopChild {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
}

impl LoopChild {
    fn spawn() -> Self {
        Self::spawn_with_cache(None)
    }

    fn spawn_with_cache(cache_dir: Option<&std::path::Path>) -> Self {
        let mut cmd = Command::new(common::engine_bin());
        // Stderr is `null` so ORT runtime warnings can't fill a 64 KB pipe
        // we never drain — that would deadlock the engine on its next write
        // and the test would hang forever waiting for a frame.
        cmd.args(["say", "--stdin-loop"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(p) = cache_dir {
            cmd.env("KESHA_CACHE_DIR", p);
            // Mirror the macOS dev runtime convention from tts_smoke.rs.
            cmd.env("DYLD_FALLBACK_LIBRARY_PATH", "/opt/homebrew/lib");
        }
        let mut child = cmd.spawn().expect("spawn engine");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        LoopChild {
            child,
            stdin,
            stdout,
        }
    }

    fn send(&mut self, json_line: &str) {
        self.stdin.write_all(json_line.as_bytes()).unwrap();
        self.stdin.write_all(b"\n").unwrap();
        self.stdin.flush().unwrap();
    }

    fn recv(&mut self) -> Frame {
        read_frame(&mut self.stdout).expect("read frame")
    }

    fn close(mut self) {
        // Dropping stdin closes the pipe; the loop sees EOF and exits 0.
        drop(self.stdin);
        // Poll for up to 2 s for clean exit before killing.
        for _ in 0..20 {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------------------
// Protocol-only tests (no model required)
// ---------------------------------------------------------------------------

#[test]
fn malformed_json_returns_err_frame_with_zero_id() {
    let mut c = LoopChild::spawn();
    c.send("{not json");
    let f = c.recv();
    assert_eq!(f.status, STATUS_ERR, "expected err status for bad json");
    assert_eq!(f.id, 0, "pre-parse errors should carry id=0");
    let msg = String::from_utf8_lossy(&f.payload);
    assert!(
        msg.starts_with("json:"),
        "error payload should be tagged 'json:': {msg}"
    );
    c.close();
}

#[test]
fn empty_text_returns_err_frame_with_request_id() {
    let mut c = LoopChild::spawn();
    c.send(r#"{"id": 42, "text": "", "voice": "en-am_michael"}"#);
    let f = c.recv();
    assert_eq!(f.status, STATUS_ERR);
    assert_eq!(f.id, 42, "post-parse errors should echo the request id");
    let msg = String::from_utf8_lossy(&f.payload);
    assert!(msg.contains("text is empty"), "unexpected error: {msg}");
    c.close();
}

#[test]
fn unknown_voice_returns_err_frame_with_request_id() {
    let mut c = LoopChild::spawn();
    c.send(r#"{"id": 9, "text": "hi", "voice": "zz-not-a-voice"}"#);
    let f = c.recv();
    assert_eq!(f.status, STATUS_ERR);
    assert_eq!(f.id, 9);
    c.close();
}

// ---------------------------------------------------------------------------
// Real synthesis (gated on env vars set by run_smoke_tests / smoke harness)
// ---------------------------------------------------------------------------

fn synth_vosk_reference(text: &str, ssml: bool, model_dir: &Path) -> Vec<u8> {
    tts::say(SayOptions {
        text,
        lang: "ru",
        engine: EngineChoice::Vosk {
            model_dir,
            // speaker_id 4 = ru-vosk-m02 (male), per voices.rs ru-vosk-* mapping
            speaker_id: 4,
            speed: 1.0,
        },
        ssml,
        format: OutputFormat::Wav,
        expand_abbrev: true,
    })
    .expect("reference Vosk synth")
}

#[test]
fn loop_synthesises_kokoro_and_caches_session() {
    // The loop-mode JSON request takes voice-by-name only — no `--model`
    // override path. So the test must materialise the runtime cache layout
    // (`$KESHA_CACHE_DIR/models/kokoro-82m/{model.onnx,voices/am_michael.bin}`)
    // and point the spawned engine at it. Same approach as
    // `tts_smoke.rs::resolves_from_cache_when_installed`.
    let Some((model, voice)) = common::kokoro_paths_or_skip() else {
        eprintln!("skipping: KOKORO_MODEL + KOKORO_VOICE not set");
        return;
    };
    if !std::path::Path::new(&model).exists() || !std::path::Path::new(&voice).exists() {
        eprintln!("skipping: KOKORO_MODEL / KOKORO_VOICE files missing");
        return;
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let voices_dir = tmp.path().join("models/kokoro-82m/voices");
    std::fs::create_dir_all(&voices_dir).expect("mkdir voices");
    std::fs::copy(&model, tmp.path().join("models/kokoro-82m/model.onnx")).expect("copy model");
    std::fs::copy(&voice, voices_dir.join("am_michael.bin")).expect("copy voice");

    let mut c = LoopChild::spawn_with_cache(Some(tmp.path()));
    let req1 = r#"{"id": 1, "text": "Hello", "voice": "en-am_michael", "format": "wav"}"#;
    let req2 = r#"{"id": 2, "text": "World", "voice": "en-am_michael", "format": "wav"}"#;

    c.send(req1);
    let f1 = c.recv();
    c.send(req2);
    let f2 = c.recv();

    // Surface the engine's err message into the test failure for diagnosis,
    // since stderr is null and CI can't show what went wrong otherwise.
    let f1_msg = if f1.status == STATUS_ERR {
        String::from_utf8_lossy(&f1.payload).to_string()
    } else {
        String::new()
    };
    let f2_msg = if f2.status == STATUS_ERR {
        String::from_utf8_lossy(&f2.payload).to_string()
    } else {
        String::new()
    };
    assert_eq!(f1.status, STATUS_OK, "first request failed: {f1_msg}");
    assert_eq!(f1.id, 1);
    assert_eq!(&f1.payload[..4], b"RIFF", "first response not a WAV");

    assert_eq!(f2.status, STATUS_OK, "second request failed: {f2_msg}");
    assert_eq!(f2.id, 2);
    assert_eq!(&f2.payload[..4], b"RIFF", "second response not a WAV");

    // No timing assertion: CI noise + small input ("Hello"/"World") makes
    // warm-vs-cold ratios unreliable. Two successful frames from one process
    // is enough to verify the cached-session code path doesn't crash.
    c.close();
}

#[test]
fn loop_applies_russian_acronym_normalization_for_vosk() {
    let Some(cache_dir) = common::vosk_ru_cache_dir_or_skip() else {
        eprintln!("skipping: vosk-ru models not found");
        return;
    };
    let model_dir = cache_dir.join("models/vosk-ru");

    let mut c = LoopChild::spawn_with_cache(Some(&cache_dir));
    c.send(r#"{"id": 101, "text": "ФСБ", "voice": "ru-vosk-m02", "format": "wav"}"#);
    let plain = c.recv();
    c.send(
        r#"{"id": 102, "text": "<speak><say-as interpret-as=\"characters\">ФСБ</say-as></speak>", "voice": "ru-vosk-m02", "format": "wav", "ssml": true}"#,
    );
    let ssml = c.recv();
    c.close();

    assert_eq!(
        plain.status,
        STATUS_OK,
        "plain request failed: {}",
        String::from_utf8_lossy(&plain.payload)
    );
    assert_eq!(plain.id, 101);
    assert_eq!(&plain.payload[..4], b"RIFF", "plain response not a WAV");

    assert_eq!(
        ssml.status,
        STATUS_OK,
        "SSML request failed: {}",
        String::from_utf8_lossy(&ssml.payload)
    );
    assert_eq!(ssml.id, 102);
    assert_eq!(&ssml.payload[..4], b"RIFF", "SSML response not a WAV");

    let reference_plain = synth_vosk_reference("ФСБ", false, &model_dir);
    let reference_ssml = synth_vosk_reference(
        r#"<speak><say-as interpret-as="characters">ФСБ</say-as></speak>"#,
        true,
        &model_dir,
    );

    let plain_ratio = plain.payload.len() as f64 / reference_plain.len() as f64;
    assert!(
        (0.9..=1.1).contains(&plain_ratio),
        "stdin-loop plain={} reference={} ratio={plain_ratio:.2} (expected acronym-normalized output)",
        plain.payload.len(),
        reference_plain.len(),
    );

    let ssml_ratio = ssml.payload.len() as f64 / reference_ssml.len() as f64;
    assert!(
        (0.9..=1.1).contains(&ssml_ratio),
        "stdin-loop ssml={} reference={} ratio={ssml_ratio:.2} (expected say-as-normalized output)",
        ssml.payload.len(),
        reference_ssml.len(),
    );
}
