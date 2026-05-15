//! End-to-end smoke for SSML `<prosody rate>` on Vosk + Kokoro paths (#236).
//! Synthesizes the same SSML at three rates and compares WAV byte sizes.
//!
//! Skips at runtime when the relevant TTS voice isn't installed. The
//! installer flow is exercised by separate unit tests.

#![cfg(feature = "tts")]

mod common;

use std::path::Path;
use std::process::Command;

fn say_ssml(voice: &str, ssml: &str, out: &Path) -> bool {
    Command::new(common::engine_bin())
        .args([
            "say",
            "--voice",
            voice,
            "--ssml",
            "--out",
            out.to_str().unwrap(),
            ssml,
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn wav_byte_len(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

#[test]
fn vosk_prosody_rate_fast_shorter_than_medium() {
    let tmp = tempfile::Builder::new()
        .prefix("kesha-prosody-")
        .tempdir()
        .unwrap();
    let medium = tmp.path().join("medium.wav");
    let fast = tmp.path().join("fast.wav");
    let medium_ssml = r#"<speak>Привет, как дела сегодня вечером.</speak>"#;
    let fast_ssml =
        r#"<speak><prosody rate="fast">Привет, как дела сегодня вечером.</prosody></speak>"#;

    if !say_ssml("ru-vosk-m02", medium_ssml, &medium) {
        eprintln!("skipping: ru-vosk-m02 not installed (run `kesha install --tts`)");
        return;
    }
    assert!(
        say_ssml("ru-vosk-m02", fast_ssml, &fast),
        "fast synth failed"
    );
    let m = wav_byte_len(&medium) as f32;
    let f = wav_byte_len(&fast) as f32;
    assert!(m > 0.0 && f > 0.0, "got empty WAV(s)");
    let ratio = f / m;
    // fast = 1.25× → ~80% the byte length of medium, with slack for header
    // overhead and synth nondeterminism.
    assert!(
        (0.7..=0.9).contains(&ratio),
        "expected fast/medium byte ratio in 0.7..=0.9, got {ratio:.3} (medium={m}, fast={f})"
    );
}

#[test]
fn kokoro_prosody_rate_slow_longer_than_medium() {
    let tmp = tempfile::Builder::new()
        .prefix("kesha-prosody-")
        .tempdir()
        .unwrap();
    let medium = tmp.path().join("medium.wav");
    let slow = tmp.path().join("slow.wav");
    let medium_ssml = r#"<speak>The quick brown fox jumps over the lazy dog.</speak>"#;
    let slow_ssml = r#"<speak><prosody rate="slow">The quick brown fox jumps over the lazy dog.</prosody></speak>"#;

    if !say_ssml("en-am_michael", medium_ssml, &medium) {
        eprintln!("skipping: en-am_michael not installed (run `kesha install --tts`)");
        return;
    }
    assert!(
        say_ssml("en-am_michael", slow_ssml, &slow),
        "slow synth failed"
    );
    let m = wav_byte_len(&medium) as f32;
    let s = wav_byte_len(&slow) as f32;
    assert!(m > 0.0 && s > 0.0, "got empty WAV(s)");
    let ratio = s / m;
    // slow = 0.75× → ~133% the byte length of medium.
    assert!(
        (1.2..=1.5).contains(&ratio),
        "expected slow/medium byte ratio in 1.2..=1.5, got {ratio:.3} (medium={m}, slow={s})"
    );
}

#[test]
fn macos_prosody_rate_ssml_rejected() {
    // AVSpeech rejects SSML wholesale (#141 follow-up), so `--ssml` on a
    // macos-* voice returns a non-zero exit and prints the rejection
    // message — that includes <prosody rate>. This test asserts the
    // current behavior so we'd notice if AVSpeech ever started accepting
    // SSML without our prosody warn+strip arm being added.
    if !cfg!(target_os = "macos") {
        return;
    }
    let tmp = tempfile::Builder::new()
        .prefix("kesha-prosody-")
        .tempdir()
        .unwrap();
    let out = tmp.path().join("macos.wav");
    let ssml = r#"<speak><prosody rate="fast">Hello there.</prosody></speak>"#;
    let result = Command::new(common::engine_bin())
        .args([
            "say",
            "--voice",
            "macos-com.apple.voice.compact.en-US.Samantha",
            "--ssml",
            "--out",
            out.to_str().unwrap(),
            ssml,
        ])
        .output()
        .unwrap();
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        if stderr.contains("not yet supported with macos-")
            || stderr.contains("AVSpeech")
            || stderr.contains("system_tts")
        {
            // Expected: SSML rejection on macOS voices.
            return;
        }
        eprintln!("skipping: macos voice not available ({stderr})");
        return;
    }
    // AVSpeech now accepts SSML — that's a behavior change. The prosody
    // warn-strip arm called out in #236 needs to ship before this test can
    // be relaxed; fail loudly so the regression isn't missed.
    panic!(
        "AVSpeech accepted SSML <prosody rate>; add the prosody warn-strip arm \
         (see #236 plan T4) before allowing this path to succeed silently"
    );
}
