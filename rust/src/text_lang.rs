//! macOS text-language detection.
//!
//! Two macOS implementations live here:
//!
//! - **Sidecar path** (`feature = "system_text_lang"`): spawns a precompiled
//!   `kesha-textlang` Swift binary built by `rust/build.rs`. ~30-50 ms per
//!   call, no Swift JIT cost. Source at `rust/swift/kesha-textlang.swift`.
//!   Default in the release matrix; runtime resolution mirrors
//!   `tts::avspeech::helper_path` (sibling-of-exe first, then build-time
//!   `OUT_DIR` fallback).
//!
//! - **Legacy `swift -e` fallback** (no feature): shells out to `swift -e`
//!   with inline NaturalLanguage source. ~200 ms warm, up to ~35 s on cold
//!   Xcode-cache state — slow, but doesn't require `swiftc` at build time.
//!   Kept so minimal dev environments without Xcode CLT can still
//!   `cargo build --features onnx,tts` on macOS (Greptile P1 on #303).
//!
//! Non-macOS targets return an error regardless of the feature.

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct TextLangResult {
    pub code: String,
    pub confidence: f64,
}

// ─── Sidecar path (fast, feature-gated) ──────────────────────────────────

#[cfg(all(feature = "system_text_lang", target_os = "macos"))]
pub fn detect_text_language(text: &str) -> anyhow::Result<TextLangResult> {
    use std::path::PathBuf;

    /// Sibling-of-current-exe first, then build-time fallback. Matches the
    /// resolution strategy in `tts::avspeech::helper_path` and
    /// `transcribe::diarize::sidecar_path` so the three sidecars are
    /// discoverable identically — `~/.cache/kesha/engine/bin/kesha-textlang`
    /// in the release layout, `$OUT_DIR/kesha-textlang` for `cargo run`.
    fn helper_path() -> PathBuf {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                let sibling = parent.join("kesha-textlang");
                if sibling.exists() {
                    return sibling;
                }
            }
        }
        PathBuf::from(env!("KESHA_TEXTLANG_HELPER"))
    }

    detect_with_helper(text, &helper_path())
}

/// Sidecar invocation extracted from `detect_text_language` so tests can
/// inject a fake helper binary without touching the production path. Pipes
/// `text` on stdin (UTF-8, no escaping required — Swift reads bytes verbatim
/// via `readDataToEndOfFile`), reads JSON from stdout, surfaces stderr as the
/// error context on non-zero exit.
#[cfg(all(feature = "system_text_lang", target_os = "macos"))]
pub(crate) fn detect_with_helper(
    text: &str,
    helper: &std::path::Path,
) -> anyhow::Result<TextLangResult> {
    use anyhow::Context;
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new(helper)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawn {} failed", helper.display()))?;

    child
        .stdin
        .as_mut()
        .context("kesha-textlang: stdin unavailable")?
        .write_all(text.as_bytes())?;
    drop(child.stdin.take());

    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "kesha-textlang helper exited {}: {}",
            output.status,
            stderr.trim()
        );
    }
    serde_json::from_slice::<TextLangResult>(&output.stdout).with_context(|| {
        format!(
            "kesha-textlang: invalid JSON on stdout: {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

// ─── Legacy `swift -e` path (slow, no feature) ───────────────────────────

#[cfg(all(not(feature = "system_text_lang"), target_os = "macos"))]
pub fn detect_text_language(text: &str) -> anyhow::Result<TextLangResult> {
    use std::process::Command;

    // Escape text for Swift string literal.
    let escaped = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', " ");

    let swift_code = format!(
        r#"import NaturalLanguage; import Foundation; let r = NLLanguageRecognizer(); r.processString("{}"); var c = ""; var p = 0.0; if let l = r.dominantLanguage {{ c = l.rawValue; p = r.languageHypotheses(withMaximum: 1)[l] ?? 0.0 }}; let d = try! JSONSerialization.data(withJSONObject: ["code": c, "confidence": p], options: [.sortedKeys]); FileHandle.standardOutput.write(d)"#,
        escaped
    );

    let output = Command::new("swift").arg("-e").arg(&swift_code).output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("NLLanguageRecognizer failed: {}", stderr.trim());
    }

    let result: TextLangResult = serde_json::from_slice(&output.stdout)?;
    Ok(result)
}

// ─── Non-macOS ────────────────────────────────────────────────────────────

#[cfg(not(target_os = "macos"))]
pub fn detect_text_language(_text: &str) -> anyhow::Result<TextLangResult> {
    anyhow::bail!("detect-text-lang is only available on macOS");
}

// ─── Tests (sidecar path only — `swift -e` is the same code as v1.15.0) ──

#[cfg(all(test, feature = "system_text_lang", target_os = "macos"))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    /// Write a one-shot shell script that fakes the kesha-textlang contract
    /// (reads stdin, prints supplied JSON, exits with supplied code) and
    /// return its path. Same pattern as `tts::avspeech::tests::fake_helper`.
    fn fake_helper(script: &str) -> (tempfile::NamedTempFile, PathBuf) {
        let tmp = tempfile::Builder::new()
            .prefix("kesha-textlang-test-")
            .suffix(".sh")
            .tempfile()
            .unwrap();
        std::fs::write(tmp.path(), script).unwrap();
        let mut perms = std::fs::metadata(tmp.path()).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(tmp.path(), perms).unwrap();
        let path = tmp.path().to_path_buf();
        (tmp, path)
    }

    #[test]
    fn happy_path_parses_json() {
        let (_keep, helper) = fake_helper(
            "#!/bin/sh\ncat >/dev/null\nprintf '{\"code\":\"en\",\"confidence\":0.95}'\n",
        );
        let r = detect_with_helper("hello world", &helper).unwrap();
        assert_eq!(r.code, "en");
        assert!((r.confidence - 0.95).abs() < 1e-6);
    }

    #[test]
    fn forwards_stdin_byte_exact() {
        // Stdin contract: Swift's `readDataToEndOfFile` reads raw bytes. The
        // old `swift -e` impl had to Swift-escape backslash/quote/newline in
        // the input string — verify stdin pipes UTF-8 through unchanged so
        // a future regression that re-introduces escaping fails this test.
        let (_keep, helper) = fake_helper(
            "#!/bin/sh\nINPUT=$(cat)\nif [ \"$INPUT\" = \"Привет, мир!\" ]; then\n  printf '{\"code\":\"ru\",\"confidence\":0.99}'\nelse\n  printf 'wrong input: %s' \"$INPUT\" >&2\n  exit 2\nfi\n",
        );
        let r = detect_with_helper("Привет, мир!", &helper).unwrap();
        assert_eq!(r.code, "ru");
    }

    #[test]
    fn nonzero_exit_surfaces_stderr() {
        // Real sidecar exits 1 on empty stdin (see swift/kesha-textlang.swift).
        // The error chain should preserve stderr so the user can debug.
        let (_keep, helper) = fake_helper(
            "#!/bin/sh\ncat >/dev/null\nprintf 'kesha-textlang: empty stdin\\n' >&2\nexit 1\n",
        );
        let err = detect_with_helper("anything", &helper).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("kesha-textlang helper exited"), "{msg}");
        assert!(
            msg.contains("empty stdin"),
            "stderr missing from error: {msg}"
        );
    }

    #[test]
    fn malformed_json_surfaces_in_error() {
        // Defense against a future sidecar regression that prints stray
        // diagnostic text alongside the JSON — the user should see the bad
        // payload in the error, not a generic parse failure.
        let (_keep, helper) = fake_helper("#!/bin/sh\ncat >/dev/null\nprintf 'not json at all'\n");
        let err = detect_with_helper("hello", &helper).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("invalid JSON on stdout"), "{msg}");
        assert!(msg.contains("not json at all"), "raw bytes missing: {msg}");
    }
}
