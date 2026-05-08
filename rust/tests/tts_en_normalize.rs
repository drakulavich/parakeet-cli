//! Integration test for #244 — English acronym auto-expansion via Kokoro.
//!
//! Uses the warm `--stdin-loop` subprocess to avoid model reload per case.
//! Wire protocol (from `say_loop.rs`):
//!   request:  `<JSON>\n`
//!   response: `<status:u8><id:u32 LE><len:u32 LE><payload:[u8; len]>`
//!     status 0 = ok (WAV bytes), status 1 = error (UTF-8 message)
//!
//! Mirrors rust/tests/tts_ru_normalize.rs.

#![cfg(feature = "tts")]

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

// =============================================================================
// Skip gate
// =============================================================================

/// Return the Kokoro model path if the required runtime files are present;
/// otherwise return None so callers can skip gracefully.
///
/// Strategy: use KESHA_CACHE_DIR when set (matches CI / local dev fixture
/// layout), otherwise fall back to the default `~/.cache/kesha`. This mirrors
/// what `models::cache_dir()` does in production.
fn kokoro_model_path_or_skip() -> Option<PathBuf> {
    let base = if let Ok(dir) = std::env::var("KESHA_CACHE_DIR") {
        PathBuf::from(dir)
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        PathBuf::from(home).join(".cache/kesha")
    };

    let model = base.join("models/kokoro-82m/model.onnx");
    let voice = base.join("models/kokoro-82m/voices/am_michael.bin");

    if model.exists() && voice.exists() {
        Some(base)
    } else {
        None
    }
}

// =============================================================================
// stdin-loop engine wrapper
// =============================================================================

/// Thin wrapper around a `kesha-engine say --voice en-am_michael --stdin-loop`
/// subprocess.
///
/// One `KokoroLoopEngine` holds a single Kokoro model load, amortising the
/// ~1 s cold-start across multiple `synth_bytes` calls.
struct KokoroLoopEngine {
    child: std::process::Child,
    /// Wrapped in `Option` so `Drop` can `take()` and explicitly drop the
    /// write end of the stdin pipe BEFORE `child.wait()` — otherwise the
    /// engine sits in `read_line` waiting for EOF that never arrives, and
    /// the test deadlocks at end-of-scope.
    stdin: Option<std::process::ChildStdin>,
    stdout: std::process::ChildStdout,
}

impl KokoroLoopEngine {
    /// Spawn the engine subprocess. Returns `None` when the Kokoro models are
    /// not installed (same skip gate as the other Kokoro tests).
    fn spawn() -> Option<Self> {
        kokoro_model_path_or_skip()?;
        let bin = env!("CARGO_BIN_EXE_kesha-engine");
        let mut child = Command::new(bin)
            .args(["say", "--voice", "en-am_michael", "--stdin-loop"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Discard ORT runtime noise — it can fill a 64 KB pipe we never
            // drain and deadlock the engine on its next write.
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn kesha-engine --stdin-loop");
        let stdin = child.stdin.take().expect("child stdin");
        let stdout = child.stdout.take().expect("child stdout");
        Some(Self {
            child,
            stdin: Some(stdin),
            stdout,
        })
    }

    /// Synthesise `text` and return the raw WAV bytes.
    ///
    /// `expand_abbrev`: when `true` the engine expands English acronyms to
    /// letter names before phonemising (the default). Pass `false` to suppress.
    fn synth_bytes(&mut self, text: &str, ssml: bool, expand_abbrev: bool) -> Vec<u8> {
        let req = serde_json::json!({
            "id": 1u32,
            "text": text,
            "voice": "en-am_michael",
            "format": "wav",
            "ssml": ssml,
            "expand_abbrev": expand_abbrev,
        });
        let mut line = req.to_string();
        line.push('\n');

        let stdin = self
            .stdin
            .as_mut()
            .expect("stdin held while KokoroLoopEngine is alive");
        stdin.write_all(line.as_bytes()).expect("write request");
        stdin.flush().expect("flush request");

        // --- read response header (9 bytes) ---
        let mut header = [0u8; 9];
        self.stdout
            .read_exact(&mut header)
            .expect("read response header");
        let status = header[0];
        let len = u32::from_le_bytes([header[5], header[6], header[7], header[8]]) as usize;

        // --- read payload ---
        let mut payload = vec![0u8; len];
        self.stdout
            .read_exact(&mut payload)
            .expect("read response payload");

        if status != 0 {
            panic!(
                "engine error: {}",
                std::str::from_utf8(&payload).unwrap_or("<non-utf8>")
            );
        }
        payload
    }
}

impl Drop for KokoroLoopEngine {
    fn drop(&mut self) {
        // Close the write end of stdin BEFORE waiting on the child — engine
        // sees EOF on its read_line loop and exits cleanly. If we leave the
        // ChildStdin alive (the natural field-drop order would close it
        // AFTER this Drop body returns), `child.wait()` deadlocks.
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

// =============================================================================
// Tests
// =============================================================================

/// Auto-expanding "FBI" (3 all-caps letters, not in stop-list → F-B-I) must
/// produce noticeably more audio than passing "FBI" straight to Kokoro without
/// expansion. Threshold: expanded WAV ≥ 5 KB longer (letters spoken separately
/// produce more frames than the original token).
#[test]
fn fbi_spells_letter_by_letter_via_auto_expand() {
    let mut eng = match KokoroLoopEngine::spawn() {
        Some(e) => e,
        None => {
            eprintln!(
                "skipping fbi_spells_letter_by_letter_via_auto_expand: \
                 Kokoro models not found (run `kesha install --tts`)"
            );
            return;
        }
    };

    let raw = eng.synth_bytes("FBI investigation", false, false);
    let expanded = eng.synth_bytes("FBI investigation", false, true);

    assert!(
        expanded.len() > raw.len() + 5_000,
        "expanded={} raw={}, expected expanded > raw + 5 KB \
         (expansion spells F-B-I letter by letter)",
        expanded.len(),
        raw.len()
    );
}

/// "NASA" is in the English stop-list — both expansion-on and expansion-off
/// paths should produce virtually identical audio. Byte delta must be < 5%.
#[test]
fn nasa_passes_through_via_stop_list() {
    let mut eng = match KokoroLoopEngine::spawn() {
        Some(e) => e,
        None => {
            eprintln!(
                "skipping nasa_passes_through_via_stop_list: \
                 Kokoro models not found (run `kesha install --tts`)"
            );
            return;
        }
    };

    let with_expand = eng.synth_bytes("NASA briefed Congress", false, true);
    let no_expand = eng.synth_bytes("NASA briefed Congress", false, false);

    let delta = (with_expand.len() as i64 - no_expand.len() as i64).abs();
    let threshold = (no_expand.len() / 20) as i64; // 5 %
    assert!(
        delta < threshold,
        "NASA stop-list should produce near-identical audio; \
         with_expand={} no_expand={} delta={delta} threshold={threshold}",
        with_expand.len(),
        no_expand.len(),
    );
}

/// `<say-as interpret-as="characters">` must letter-spell even when
/// `expand_abbrev=false` is set. The SSML tag overrides the CLI flag.
///
/// Strategy: the `<say-as>` output must be close (±20%) to the auto-expand
/// version (both produce "ee pee ay em"), and must differ meaningfully (>5 KB)
/// from the no-expand plain-SSML version (which lets Kokoro treat "EPAM" as a
/// single invented word token with different duration).
#[test]
fn say_as_characters_overrides_no_expand_abbrev() {
    let mut eng = match KokoroLoopEngine::spawn() {
        Some(e) => e,
        None => {
            eprintln!(
                "skipping say_as_characters_overrides_no_expand_abbrev: \
                 Kokoro models not found (run `kesha install --tts`)"
            );
            return;
        }
    };

    // Plain FBI with expand_abbrev=false: Kokoro treats it as a single
    // unknown word and invents its own pronunciation / duration.
    let raw_no_expand = eng.synth_bytes("<speak>FBI</speak>", true, false);

    // Auto-expand (expand_abbrev=true) spells F→B→I via letter table.
    let auto_expanded = eng.synth_bytes("<speak>FBI</speak>", true, true);

    // <say-as characters> with expand_abbrev=false: must still spell F-B-I
    // because the Spell segment path is not gated by expand_abbrev.
    let say_as = eng.synth_bytes(
        r#"<speak><say-as interpret-as="characters">FBI</say-as></speak>"#,
        true,
        false,
    );

    // 1. say-as must produce different audio from no-expand plain (>5 KB delta).
    let delta_from_raw = (say_as.len() as i64 - raw_no_expand.len() as i64).abs();
    assert!(
        delta_from_raw > 5_000,
        "<say-as> should produce different audio from no-expand plain; \
         say_as={} raw_no_expand={} delta={delta_from_raw}",
        say_as.len(),
        raw_no_expand.len(),
    );

    // 2. say-as must be close to auto-expand (both spell letters) — within ±20%.
    let ratio = say_as.len() as f64 / auto_expanded.len() as f64;
    assert!(
        (0.80..=1.20).contains(&ratio),
        "<say-as> should be close to auto-expand (both letter-spell); \
         say_as={} auto_expanded={} ratio={ratio:.2} (expected 0.80..=1.20)",
        say_as.len(),
        auto_expanded.len(),
    );
}
