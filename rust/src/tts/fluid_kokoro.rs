//! FluidAudio Kokoro backend — macOS arm64, behind `system_kokoro`.
//!
//! The public `fluidaudio-rs` crate does not expose Kokoro TTS yet, so Kesha
//! shells out to a small Swift sidecar that links FluidAudio directly. This
//! mirrors the AVSpeech/diarize sidecar pattern and keeps non-Darwin builds on
//! the existing ONNX Kokoro implementation.

#![cfg(all(
    feature = "system_kokoro",
    target_os = "macos",
    target_arch = "aarch64"
))]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

// FluidAudio 0.14.5 voice snapshot. Keep this list in sync with
// swift/kesha-kokoro/Package.resolved whenever the FluidAudio pin changes.
const VOICES: &[&str] = &[
    "af_alloy",
    "af_aoede",
    "af_bella",
    "af_heart",
    "af_jessica",
    "af_kore",
    "af_nicole",
    "af_nova",
    "af_river",
    "af_sarah",
    "af_sky",
    "am_adam",
    "am_echo",
    "am_eric",
    "am_fenrir",
    "am_liam",
    "am_michael",
    "am_onyx",
    "am_puck",
    "am_santa",
];

pub fn available_voice_ids() -> Vec<String> {
    VOICES.iter().map(|v| format!("en-{v}")).collect()
}

pub fn supports_voice(name: &str) -> bool {
    VOICES.contains(&name)
}

pub fn helper_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            for name in ["kesha-kokoro", "kesha-kokoro-darwin-arm64"] {
                let sibling = parent.join(name);
                if sibling.exists() {
                    return sibling;
                }
            }
        }
    }
    PathBuf::from(env!("KESHA_KOKORO_HELPER"))
}

pub fn synthesize(
    text: &str,
    voice_id: &str,
    speed: f32,
    helper: Option<&Path>,
) -> anyhow::Result<Vec<u8>> {
    if text.is_empty() {
        anyhow::bail!("fluid-kokoro: text is empty");
    }
    let bin = helper.map(PathBuf::from).unwrap_or_else(helper_path);
    let speed_arg = format!("{speed:.3}");

    let mut child = Command::new(&bin)
        .arg("--voice")
        .arg(voice_id)
        .arg("--speed")
        .arg(speed_arg)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("spawn {}: {e}", bin.display()))?;

    child
        .stdin
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("fluid-kokoro: stdin unavailable"))?
        .write_all(text.as_bytes())?;
    drop(child.stdin.take());

    let output = child.wait_with_output()?;
    if !output.status.success() {
        anyhow::bail!(
            "fluid-kokoro helper exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fake_helper(tmp: &TempDir, script: &str) -> PathBuf {
        let path = tmp.path().join("fake-fluid-kokoro.sh");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "#!/bin/sh").unwrap();
        writeln!(f, "{script}").unwrap();
        drop(f);
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        path
    }

    #[test]
    fn lists_supported_kesha_voice_ids() {
        let voices = available_voice_ids();
        assert!(voices.contains(&"en-am_michael".to_string()));
        assert!(voices.contains(&"en-af_heart".to_string()));
    }

    #[test]
    fn helper_stdout_is_returned_verbatim() {
        let tmp = TempDir::new().unwrap();
        let helper = fake_helper(&tmp, r#"cat >/dev/null; printf 'RIFFmock'"#);
        let bytes = synthesize("hello", "am_michael", 1.0, Some(&helper)).unwrap();
        assert_eq!(&bytes, b"RIFFmock");
    }

    #[test]
    fn helper_nonzero_exit_surfaces_stderr() {
        let tmp = TempDir::new().unwrap();
        let helper = fake_helper(&tmp, r#"echo 'voice not found: xyz' >&2; exit 2"#);
        let err = synthesize("hello", "xyz", 1.0, Some(&helper))
            .unwrap_err()
            .to_string();
        assert!(err.contains("voice not found"), "msg: {err}");
        assert!(err.contains("exited"), "msg: {err}");
    }
}
