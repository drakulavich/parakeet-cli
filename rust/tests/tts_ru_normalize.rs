//! Issue #232 — Russian acronym auto-expansion + <say-as> integration.
//!
//! Asserts byte-length deltas through the full Vosk synth pipeline so a
//! regression in the new tts::ru layer (or in how SayOptions threads
//! `expand_abbrev`) shows up as a hard test failure rather than a
//! subjective audio change.
//!
//! Session strategy: most tests drive the engine via `--stdin-loop` so a
//! single Vosk model load (~1-2 s) is shared across requests. One test
//! keeps the cold `tts::say()` path for regression coverage of the
//! direct-call stack.

#![cfg(feature = "tts")]

mod common;

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use kesha_engine::tts::{self, EngineChoice, OutputFormat, SayOptions};

// =============================================================================
// Shared helpers
// =============================================================================

/// Cold synthesis via `tts::say()` — kept for direct-call regression coverage.
fn synth_cold(text: &str, ssml: bool, expand_abbrev: bool, model_dir: &PathBuf) -> Vec<u8> {
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
        expand_abbrev,
    })
    .expect("synth ok")
}

// =============================================================================
// stdin-loop engine wrapper
// =============================================================================

/// Thin wrapper around a `kesha-engine say --stdin-loop` subprocess.
///
/// One `LoopEngine` holds a single Vosk model load, amortising the ~1-2 s
/// cold-start across multiple `synth` calls.
struct LoopEngine {
    child: std::process::Child,
    /// Wrapped in `Option` so `Drop` can `take()` and explicitly drop the
    /// write end of the stdin pipe BEFORE `child.wait()` — otherwise the
    /// engine sits in `read_line` waiting for EOF that never arrives, and
    /// the test deadlocks at end-of-scope.
    stdin: Option<std::process::ChildStdin>,
    stdout: std::process::ChildStdout,
    /// Path to a tempfile where the child's stderr is captured.
    /// Used by `into_stderr_log()` to verify warn-once dedup. (#237)
    stderr_path: std::path::PathBuf,
}

impl LoopEngine {
    /// Spawn the engine subprocess. Returns `None` when the vosk-ru models
    /// are not installed (same skip gate as the cold-path tests).
    fn spawn() -> Option<Self> {
        common::vosk_ru_cache_dir_or_skip()?;
        // Capture stderr to a tempfile so tests can assert warn-once dedup. (#237)
        let stderr_path = std::env::temp_dir().join(format!(
            "kesha-loop-test-{}-{}.stderr.log",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        ));
        let stderr_file = std::fs::File::create(&stderr_path).expect("create stderr log");
        let mut child = Command::new(common::engine_bin())
            .args(["say", "--voice", "ru-vosk-m02", "--stdin-loop"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(stderr_file))
            .spawn()
            .expect("spawn kesha-engine --stdin-loop");
        let stdin = child.stdin.take().expect("child stdin");
        let stdout = child.stdout.take().expect("child stdout");
        Some(Self {
            child,
            stdin: Some(stdin),
            stdout,
            stderr_path,
        })
    }

    /// Consume the engine, close stdin (→ engine exits), wait for the child,
    /// read the captured stderr log, and return its contents.
    ///
    /// Call this INSTEAD of letting the engine drop naturally when a test
    /// wants to inspect stderr — it reads the file before Drop removes it.
    fn into_stderr_log(mut self) -> String {
        // Close stdin → engine sees EOF and exits cleanly.
        drop(self.stdin.take());
        let _ = self.child.wait();
        std::fs::read_to_string(&self.stderr_path).unwrap_or_default()
    }

    /// Synthesise `text` and return the raw audio bytes (WAV).
    ///
    /// Wire protocol (from `say_loop.rs`):
    /// - request:  `<JSON>\n`
    /// - response: `<status:u8><id:u32 LE><len:u32 LE><payload:[u8; len]>`
    ///   - status 0 = ok (WAV bytes), status 1 = error (UTF-8 message)
    fn synth(&mut self, text: &str, ssml: bool, expand_abbrev: bool) -> Vec<u8> {
        let req = serde_json::json!({
            "id": 1,
            "text": text,
            "voice": "ru-vosk-m02",
            "format": "wav",
            "ssml": ssml,
            "expand_abbrev": expand_abbrev,
        });
        let mut line = req.to_string();
        line.push('\n');
        let stdin = self
            .stdin
            .as_mut()
            .expect("stdin held while LoopEngine is alive");
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

impl Drop for LoopEngine {
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

/// Auto-expanding "ФСБ" (3 all-consonant letters → "эф эс бэ")
/// must produce noticeably more audio than passing "ФСБ" straight to Vosk
/// without expansion. Threshold: ≥1.3× by byte count.
///
/// This test exercises the cold `tts::say()` path directly — kept as
/// regression coverage for the direct-call stack (no subprocess).
///
/// Note: ВОЗ is no longer used here because the vowel-cluster rule (#232)
/// passes it through as a word (alternating C-V-C reads fine as "воз").
/// ФСБ has no vowels → always spells.
#[test]
fn auto_expand_plain_fsb_is_longer_than_noexpand() {
    let Some(base) = common::vosk_ru_cache_dir_or_skip() else {
        eprintln!(
            "skipping auto_expand_plain_fsb_is_longer_than_noexpand: vosk-ru models not found"
        );
        return;
    };
    let model_dir = base.join("models/vosk-ru");

    let expanded = synth_cold(
        "ФСБ", /*ssml=*/ false, /*expand_abbrev=*/ true, &model_dir,
    );
    let plain = synth_cold(
        "ФСБ", /*ssml=*/ false, /*expand_abbrev=*/ false, &model_dir,
    );

    let ratio = expanded.len() as f64 / plain.len() as f64;
    assert!(
        ratio > 1.3,
        "expanded={} plain={} ratio={:.2} (expected >1.3×)",
        expanded.len(),
        plain.len(),
        ratio,
    );
}

/// Warm-session batch: two ratio checks under a single Vosk model load via
/// `kesha-engine say --stdin-loop`. Spawns one `LoopEngine` and runs:
///
/// 1. `<say-as interpret-as="characters">ФСБ</say-as>` must spell out
///    letters identically to auto-expand: within ±10% by byte length.
/// 2. With `expand_abbrev=false`, uppercase "ВОЗ" and lowercase "воз" must
///    produce audio within ±30% (they read the same phonetically to Vosk).
#[test]
fn warm_session_say_as_and_baseline_checks() {
    let mut eng = match LoopEngine::spawn() {
        Some(e) => e,
        None => {
            eprintln!("skipping warm_session_say_as_and_baseline_checks: vosk-ru models not found");
            return;
        }
    };

    // --- check 1: <say-as characters> parity with auto-expand ---
    let auto_fsb = eng.synth("ФСБ", false, true);
    let ssml_fsb = eng.synth(
        r#"<speak><say-as interpret-as="characters">ФСБ</say-as></speak>"#,
        true,
        false, // <say-as> wins regardless of expand_abbrev flag
    );
    let ratio1 = ssml_fsb.len() as f64 / auto_fsb.len() as f64;
    assert!(
        (0.9..=1.1).contains(&ratio1),
        "say-as/auto-expand parity: auto_fsb={} ssml_fsb={} ratio={:.2} (expected 0.9..=1.1)",
        auto_fsb.len(),
        ssml_fsb.len(),
        ratio1,
    );

    // --- check 2: no-expand ВОЗ vs воз baseline ---
    let upper_voz = eng.synth("ВОЗ", false, false);
    let lower_voz = eng.synth("воз", false, false);
    let ratio2 = upper_voz.len() as f64 / lower_voz.len() as f64;
    assert!(
        (0.7..=1.3).contains(&ratio2),
        "ВОЗ/воз baseline: upper={} lower={} ratio={:.2} (expected 0.7..=1.3)",
        upper_voz.len(),
        lower_voz.len(),
        ratio2,
    );
}

/// Per-#233 spike result: vosk-tts-rs 0.9-multi honours `+vowel` markers
/// when they shift stress AWAY from the model's default first-syllable
/// position. Markers that AGREE with the default are no-ops.
///
/// Ratios chosen from the spike data:
/// - дом+а (genitive shift до-МА́): +3072 bytes vs baseline (~5.7%)
/// - д+ома (agrees with default ДО́ма): byte-identical to baseline
/// - <emphasis level="none">дом+а</emphasis>: strip `+`, matches baseline
///
/// Tolerance widened slightly to keep the test robust against minor model
/// updates: ≥+2KB for the shift, ±5% for the no-op cases.
#[test]
fn emphasis_marker_shifts_stress() {
    let mut eng = match LoopEngine::spawn() {
        Some(e) => e,
        None => {
            eprintln!("skipping emphasis_marker_shifts_stress: vosk-ru models not staged");
            return;
        }
    };

    let baseline = eng.synth("дома", false, false);

    let stressed_last = eng.synth(r#"<speak><emphasis>дом+а</emphasis></speak>"#, true, false);
    assert!(
        stressed_last.len() > baseline.len() + 2000,
        "дом+а={} baseline={} (expected >baseline+2KB)",
        stressed_last.len(),
        baseline.len(),
    );

    let agrees_with_default =
        eng.synth(r#"<speak><emphasis>д+ома</emphasis></speak>"#, true, false);
    let r1 = agrees_with_default.len() as f64 / baseline.len() as f64;
    assert!(
        (0.95..=1.05).contains(&r1),
        "д+ома={} baseline={} ratio={:.2} (expected 0.95..=1.05)",
        agrees_with_default.len(),
        baseline.len(),
        r1,
    );

    let suppressed = eng.synth(
        r#"<speak><emphasis level="none">дом+а</emphasis></speak>"#,
        true,
        false,
    );
    let r2 = suppressed.len() as f64 / baseline.len() as f64;
    assert!(
        (0.95..=1.05).contains(&r2),
        "suppressed={} baseline={} ratio={:.2} (expected 0.95..=1.05)",
        suppressed.len(),
        baseline.len(),
        r2,
    );
}

/// Issue #237, updated by #267 F15 / #311 — verify that
/// `warn_once("emphasis-no-plus", ...)` fires ONCE PER REQUEST in the
/// stdin-loop daemon. The original #237 spec was "one stderr line total
/// across all calls" because the warn-once HashSet was process-scoped;
/// that turned out to silently swallow the second + third occurrence of
/// the same bug, which is the opposite of what the user wants when they
/// keep sending bad SSML over a long-lived `--stdin-loop` session
/// (#311). The fix in `say_loop::handle` calls `tts::warn::reset` at
/// the top of each request, so each call gets a fresh dedup scope.
#[test]
fn emphasis_warn_fires_per_request_in_stdin_loop() {
    let mut eng = match LoopEngine::spawn() {
        Some(e) => e,
        None => {
            eprintln!(
                "skipping emphasis_warn_fires_per_request_in_stdin_loop: vosk-ru models not staged"
            );
            return;
        }
    };

    // Three calls without `+` markers — each call resets the warn scope,
    // so the warning should fire three times (once per request).
    let _ = eng.synth(r#"<speak><emphasis>обычно</emphasis></speak>"#, true, false);
    let _ = eng.synth(
        r#"<speak><emphasis>другое слово</emphasis></speak>"#,
        true,
        false,
    );
    let _ = eng.synth(
        r#"<speak><emphasis>третий вход</emphasis></speak>"#,
        true,
        false,
    );

    // Consume the engine: closes stdin → engine exits → stderr file fully
    // written before we read it.
    let stderr = eng.into_stderr_log();

    let warn_count = stderr
        .lines()
        .filter(|l| l.contains("emphasis-no-plus") || l.contains("no `+` marker"))
        .count();

    assert_eq!(
        warn_count, 3,
        "expected one emphasis-no-plus warning PER request (3 total across 3 calls), got {warn_count}.\nstderr:\n{stderr}"
    );
}
