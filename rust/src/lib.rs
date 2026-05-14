//! Internal library target for in-crate consumers only — **NOT a public Rust API**.
//!
//! # No library contract
//!
//! `kesha-engine` ships as two artifacts:
//!
//! 1. The `kesha-engine` binary at `src/main.rs` — the public surface.
//!    All real consumers go through this via the npm `kesha` CLI
//!    wrapper, the OpenClaw plugin, or direct subprocess invocation.
//! 2. This library target (`src/lib.rs`) — exists **only** so the
//!    integration tests at `rust/tests/*.rs` can `use kesha_engine::*`
//!    without spawning a subprocess for every assertion.
//!
//! There is **no semver promise** on any `pub` symbol in this crate.
//! There is no CHANGELOG, no deprecation cycle, no stability gate.
//! The modules below may be renamed, removed, restructured, or have
//! their function signatures changed in any release — patch, minor, or
//! major — without warning, because the only Rust callers are inside
//! this repository.
//!
//! If you find this crate on crates.io: that's a mistake (the package
//! is `publish = false`). If you need a stable Rust API to consume
//! kesha-engine programmatically, please file an issue at
//! <https://github.com/drakulavich/kesha-voice-kit/issues> — designing
//! a public API contract is a separate decision that hasn't been made.
//!
//! See #267 F17 / #313 P0 for the audit thread that produced this
//! disclaimer.

pub mod audio;
pub mod debug;
pub mod models;
pub mod util;

#[cfg(feature = "tts")]
pub mod tts;
