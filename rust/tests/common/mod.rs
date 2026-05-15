//! Shared helpers for the integration tests under `rust/tests/*.rs`.
//!
//! Cargo compiles each `tests/*.rs` into its own test binary, and treats
//! `tests/<name>.rs` as one of those binaries — so this module lives at
//! `tests/common/mod.rs` (not `tests/common.rs`) to avoid being built as
//! a standalone test target.
//!
//! Each test file opts in via `mod common;` at its top. Not every file
//! uses every helper, so `#![allow(dead_code)]` keeps the per-binary
//! `unused` lint quiet without wrapping each helper individually.

#![allow(dead_code)]

use std::path::PathBuf;

/// Path to the freshly-built `kesha-engine` binary as embedded by cargo via
/// `env!("CARGO_BIN_EXE_kesha-engine")`. Use directly with
/// [`std::process::Command::new`] — `Command::new` accepts `&str`.
pub fn engine_bin() -> &'static str {
    env!("CARGO_BIN_EXE_kesha-engine")
}

/// `KOKORO_MODEL` + `KOKORO_VOICE` env-var skip gate.
///
/// Returns `Some((model, voice))` when both vars are set, `None` otherwise.
/// The historical pattern: tests that need a real Kokoro model + voice file
/// skip silently on CI runs that don't stage them. Callers print the skip
/// reason themselves so each test owns its own message.
pub fn kokoro_paths_or_skip() -> Option<(String, String)> {
    match (std::env::var("KOKORO_MODEL"), std::env::var("KOKORO_VOICE")) {
        (Ok(m), Ok(v)) => Some((m, v)),
        _ => None,
    }
}

/// Cache-based skip gate for Kokoro: returns the cache base
/// (`KESHA_CACHE_DIR` if set, else `~/.cache/kesha`) when both
/// `models/kokoro-82m/model.onnx` and the default male voice
/// `models/kokoro-82m/voices/am_michael.bin` are present. Returns
/// `None` otherwise.
///
/// Default voice is the male `am_michael` per CLAUDE.md
/// "DEFAULT TTS VOICES MUST BE MALE".
pub fn kokoro_cache_dir_or_skip() -> Option<PathBuf> {
    let base = cache_base();
    let model = base.join("models/kokoro-82m/model.onnx");
    let voice = base.join("models/kokoro-82m/voices/am_michael.bin");
    if model.exists() && voice.exists() {
        Some(base)
    } else {
        None
    }
}

/// Cache-based skip gate for Vosk-RU: returns the cache base
/// (`KESHA_CACHE_DIR` if set, else `~/.cache/kesha`) when all three
/// runtime files (`model.onnx`, `dictionary`, `bert/model.onnx`) are
/// present under `models/vosk-ru`. Returns `None` otherwise.
///
/// Returning the base (rather than the model dir) lets callers both
/// reuse it as `KESHA_CACHE_DIR` for child processes AND derive the
/// model dir via `.join("models/vosk-ru")`.
pub fn vosk_ru_cache_dir_or_skip() -> Option<PathBuf> {
    let base = cache_base();
    let model_dir = base.join("models/vosk-ru");
    if model_dir.join("model.onnx").exists()
        && model_dir.join("dictionary").exists()
        && model_dir.join("bert/model.onnx").exists()
    {
        Some(base)
    } else {
        None
    }
}

/// Resolve the cache base used by every cache-based skip gate.
/// `KESHA_CACHE_DIR` if set, else `$HOME/.cache/kesha`. Falls back to
/// `/tmp/.cache/kesha` if `HOME` is unset (matches the historical
/// behaviour of the per-test helpers we're replacing).
fn cache_base() -> PathBuf {
    if let Ok(dir) = std::env::var("KESHA_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".cache/kesha")
}
