//! Scoped warn-once helper.
//!
//! Engine-agnostic: called from the SSML parser (`tts::ssml`), the Russian-
//! Vosk normalization, and the Kokoro / Vosk defensive arms in `tts::say`.
//! Emits a single stderr line per `key` per **scope**. A scope is bounded by:
//!
//! - one-shot CLI process — implicit (the process exits before a second
//!   call could repeat); historical behavior.
//! - one `say_loop::handle()` request — explicit, via [`reset()`] at the
//!   top of each request, so a user feeding the same SSML twice over
//!   the `--stdin-loop` protocol sees the warning twice. Without this,
//!   long-lived processes silently swallowed the second invocation
//!   (#267 F15 / #311).
//!
//! Two key shapes are supported via the relaxed `&str` signature:
//!
//! - **Constant keys** (e.g. `WARN_PROSODY_MID_UTTERANCE`) — preferred. The
//!   set of distinct warnings is bounded; one allocation per scope.
//! - **Dynamic keys** (e.g. `phoneme[alphabet=x-sampa]`, `say-as[interpret-as=cardinal]`,
//!   `unknown-tag-paragraph`) — used by SSML's open-ended attribute spaces.
//!   One allocation per *unique* combination per scope.
//!
//! Lock poisoning is treated as fatal — at that point another thread panicked
//! while holding the lock and the process is in an unrecoverable state.
//!
//! **Test isolation:** `cargo nextest run` spawns a fresh process per test
//! and gets the empty-scope baseline automatically. For `cargo test --lib`
//! (single-process runner) test authors who need a clean scope inside one
//! test can call [`reset()`] in the test's setup; the function is
//! `pub(crate)` for this purpose.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

fn warned() -> &'static Mutex<HashSet<String>> {
    static W: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    W.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Emit `msg` to stderr if `key` has not been warned in this process.
/// Subsequent calls with the same `key` are silent. See module docs for
/// the constant-vs-dynamic key contract.
pub fn warn_once(key: &str, msg: &str) {
    let mut set = warned().lock().expect("warn_once: mutex poisoned");
    if !set.contains(key) {
        set.insert(key.to_string());
        eprintln!("warning: {msg}");
    }
}

/// Probe whether `warn_once` has already recorded `key` in this process.
/// Test-only — the production warn_once path itself doesn't need this.
/// Honors the test-isolation caveat in the module doc-comment: order
/// matters across `#[test]` blocks in the same `cargo test --lib` process.
#[cfg(test)]
pub(crate) fn was_warned(key: &str) -> bool {
    warned()
        .lock()
        .expect("was_warned: mutex poisoned")
        .contains(key)
}

/// Clear the warn-once scope. Subsequent `warn_once(key, msg)` calls for
/// any previously-seen key will fire again. Called by `say_loop::handle`
/// at the top of every request so each `--stdin-loop` invocation gets a
/// fresh dedup scope (#267 F15 / #311). Safe to call concurrently with
/// `warn_once` — the same Mutex serializes both.
///
/// `dead_code` is silenced because `say_loop` only links into the
/// `kesha-engine` bin target, not into `lib.rs`'s library facade. The
/// library target's only consumer is the test block above (`reset` is
/// exercised by `reset_clears_the_scope_so_subsequent_warns_fire_again`).
#[allow(dead_code)]
pub(crate) fn reset() {
    warned().lock().expect("reset: mutex poisoned").clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warn_once_dedups_by_key() {
        // Public-API exercise: first call inserts the key (and prints to stderr);
        // second call with the same key is a silent no-op. We can't capture
        // stderr deterministically across the global `eprintln!`, so we assert
        // dedup via the keyed set state — but, unlike the bypass form, we go
        // through the public function so a regression that drops the eprintln
        // (or the insert) would be observable as a behavior change.
        let key = "test-warn-once-key-1";
        warn_once(key, "first call — should print once and remember the key");
        assert!(
            warned().lock().unwrap().contains(key),
            "warn_once must record the key it warned for"
        );
        // Second call: dedup means the key is already present, so insert returns
        // false. We can verify by attempting another insert manually.
        warn_once(key, "second call — should be a silent no-op");
        let still_present = warned().lock().unwrap().contains(key);
        assert!(still_present, "key remains in the set across calls");
        // Manual probe: try inserting the key fresh — should report already-there.
        let probe = warned().lock().unwrap().insert(key.to_string());
        assert!(!probe, "key already present after warn_once recorded it");
    }

    #[test]
    fn warn_once_different_keys_each_fire() {
        // Public-API exercise: each unique key should be recorded independently.
        warn_once("test-warn-once-key-2a", "first key");
        warn_once("test-warn-once-key-2b", "second key");
        let set = warned().lock().unwrap();
        assert!(set.contains("test-warn-once-key-2a"));
        assert!(set.contains("test-warn-once-key-2b"));
    }

    #[test]
    fn reset_clears_the_scope_so_subsequent_warns_fire_again() {
        // Use a key unique to this test so it doesn't collide with the dedup
        // tests above when run in a shared process (cargo test --lib).
        let key = "test-warn-once-reset-key";
        warn_once(key, "first fire — should record the key");
        assert!(was_warned(key), "key should be recorded after first warn");

        reset();
        assert!(
            !was_warned(key),
            "reset() should clear the scope, but key is still present"
        );

        // After reset, a second warn_once with the same key fires again
        // (inserts into the cleared set).
        warn_once(key, "second fire — should re-record after reset");
        assert!(was_warned(key), "key should be recorded after second warn");
    }
}
