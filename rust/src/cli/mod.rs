//! Per-subcommand dispatch modules for `kesha-engine`. Each `run(…)` here
//! owns the side-effecting body of one [`crate::Commands`] arm; `main.rs`
//! stays a thin parse + match table. Mirrors the TS-side `src/cli/*`
//! shape (one file per `kesha` subcommand). Issue #267 F6.

pub mod detect_lang;
pub mod detect_text_lang;
pub mod install;
pub mod record;
pub mod transcribe;

#[cfg(feature = "tts")]
pub mod say;
