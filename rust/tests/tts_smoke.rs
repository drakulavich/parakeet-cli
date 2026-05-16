#![cfg(feature = "tts")]

mod common;

use std::process::Command;

#[test]
fn capabilities_advertises_tts() {
    let out = Command::new(common::engine_bin())
        .arg("--capabilities-json")
        .output()
        .expect("run");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("\"tts\""),
        "capabilities missing tts: {stdout}"
    );
}

#[test]
fn install_has_tts_flag() {
    let out = Command::new(common::engine_bin())
        .args(["install", "--help"])
        .output()
        .expect("run");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("--tts"),
        "install --help missing --tts: {stdout}"
    );
    assert!(
        stdout.contains("Chatterbox 23 languages"),
        "install --help should explain Chatterbox language bundle: {stdout}"
    );
    assert!(
        stdout.contains("one bundled download"),
        "install --help should say Chatterbox languages install together: {stdout}"
    );
}

#[test]
fn say_subcommand_exists() {
    let out = Command::new(common::engine_bin())
        .args(["say", "--help"])
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "say --help should exit 0, got {:?}\nstderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("--voice"), "help missing --voice: {stdout}");
}

#[test]
fn say_with_explicit_paths_produces_wav() {
    let Some((model, voice)) = common::kokoro_paths_or_skip() else {
        eprintln!("skipping: set KOKORO_MODEL + KOKORO_VOICE");
        return;
    };
    let out = Command::new(common::engine_bin())
        .args([
            "say",
            "Hello, world",
            "--model",
            &model,
            "--voice-file",
            &voice,
            "--lang",
            "en-us",
        ])
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "exit {:?}\nstderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(&out.stdout[..4], b"RIFF", "stdout is not a WAV");
    assert!(
        out.stdout.len() > 10_000,
        "stdout too small: {} bytes",
        out.stdout.len()
    );
}

#[test]
fn say_reads_stdin_when_no_positional() {
    let Some((model, voice)) = common::kokoro_paths_or_skip() else {
        eprintln!("skipping: set KOKORO_MODEL + KOKORO_VOICE");
        return;
    };
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new(common::engine_bin())
        .args([
            "say",
            "--model",
            &model,
            "--voice-file",
            &voice,
            "--lang",
            "en-us",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"Hello")
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(&out.stdout[..4], b"RIFF");
}

#[test]
fn empty_text_exits_2() {
    let out = Command::new(common::engine_bin())
        .args([
            "say",
            "",
            "--model",
            "/nonexistent",
            "--voice-file",
            "/nonexistent",
        ])
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(2),
        "expected exit 2 for empty text\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn missing_voice_in_cache_exits_1_with_install_hint() {
    let tmp = tempfile::tempdir().unwrap();
    let out = Command::new(common::engine_bin())
        .env("KESHA_CACHE_DIR", tmp.path())
        .args(["say", "Hi"])
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(1),
        "expected exit 1 for missing voice\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("install --tts"),
        "stderr missing install hint: {stderr}"
    );
}

#[test]
fn resolves_from_cache_when_installed() {
    let Some((model, voice)) = common::kokoro_paths_or_skip() else {
        eprintln!("skipping: set KOKORO_MODEL + KOKORO_VOICE");
        return;
    };
    // misaki-rs is embedded — no G2P model cache required post-#213.
    let tmp = tempfile::tempdir().unwrap();
    let voices_dir = tmp.path().join("models/kokoro-82m/voices");
    std::fs::create_dir_all(&voices_dir).unwrap();
    // Copy instead of symlink so the test works cross-platform (Windows symlink
    // creation requires elevated privileges and the os::unix API is not available).
    std::fs::copy(&model, tmp.path().join("models/kokoro-82m/model.onnx")).unwrap();
    // Stage as am_michael.bin since DEFAULT_VOICE_ID = "en-am_michael" (CLAUDE.md
    // "DEFAULT TTS VOICES MUST BE MALE"); the bytes come from KOKORO_VOICE which
    // run-cargo-test.sh now points at am_michael.bin.
    std::fs::copy(&voice, voices_dir.join("am_michael.bin")).unwrap();

    let out = Command::new(common::engine_bin())
        .env("KESHA_CACHE_DIR", tmp.path())
        .env("DYLD_FALLBACK_LIBRARY_PATH", "/opt/homebrew/lib")
        .args(["say", "Hello"])
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(&out.stdout[..4], b"RIFF");
}

#[test]
fn list_voices_empty_on_fresh_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let out = Command::new(common::engine_bin())
        .env("KESHA_CACHE_DIR", tmp.path())
        .args(["say", "--list-voices"])
        .output()
        .expect("run");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("install --tts"),
        "expected install hint, got: {stdout}"
    );
    assert!(
        stdout.contains("Chatterbox languages install together"),
        "expected bundled-language hint, got: {stdout}"
    );
}

#[test]
fn list_voices_shows_installed() {
    let tmp = tempfile::tempdir().unwrap();
    let voices_dir = tmp.path().join("models/kokoro-82m/voices");
    std::fs::create_dir_all(&voices_dir).unwrap();
    std::fs::write(voices_dir.join("af_heart.bin"), b"").unwrap();
    let out = Command::new(common::engine_bin())
        .env("KESHA_CACHE_DIR", tmp.path())
        .args(["say", "--list-voices"])
        .output()
        .expect("run");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("en-af_heart"),
        "expected en-af_heart, got: {stdout}"
    );
}

#[test]
fn text_too_long_exits_5() {
    let huge = "a".repeat(10_000);
    let out = Command::new(common::engine_bin())
        .args([
            "say",
            &huge,
            "--model",
            "/nonexistent",
            "--voice-file",
            "/nonexistent",
        ])
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(5),
        "expected exit 5 for too-long text\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn synthesis_failure_exits_4() {
    // Missing model file at runtime -> SynthesisFailed -> exit 4
    let voice_tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(voice_tmp.path(), vec![0u8; 510 * 256 * 4]).unwrap();
    let out = Command::new(common::engine_bin())
        .args([
            "say",
            "Hi",
            "--model",
            "/nonexistent-model",
            "--voice-file",
            voice_tmp.path().to_str().unwrap(),
        ])
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(4),
        "expected exit 4 for synthesis failure\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn say_writes_to_file_with_out_flag() {
    let Some((model, voice)) = common::kokoro_paths_or_skip() else {
        eprintln!("skipping: set KOKORO_MODEL + KOKORO_VOICE");
        return;
    };
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let out = Command::new(common::engine_bin())
        .args([
            "say",
            "Hi",
            "--model",
            &model,
            "--voice-file",
            &voice,
            "--out",
            tmp.path().to_str().unwrap(),
        ])
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // stdout should be empty when --out is set
    assert!(out.stdout.is_empty(), "stdout should be empty with --out");
    let written = std::fs::read(tmp.path()).unwrap();
    assert_eq!(&written[..4], b"RIFF");
}
