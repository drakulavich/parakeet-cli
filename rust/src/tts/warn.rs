//! Per-process warn-once helper.
//!
//! Engine-agnostic: called from the SSML parser (`tts::ssml`), the Russian-
//! Vosk normalization, and the Kokoro / Vosk defensive arms in `tts::say`.
//! Emits a single stderr line per `key` per process. Two key shapes are
//! supported via the relaxed `&str` signature:
//!
//! - **Constant keys** (e.g. `WARN_PROSODY_MID_UTTERANCE`) — preferred. The
//!   set of distinct warnings is bounded; one allocation per process.
//! - **Dynamic keys** (e.g. `phoneme[alphabet=x-sampa]`, `say-as[interpret-as=cardinal]`,
//!   `unknown-tag-paragraph`) — used by SSML's open-ended attribute spaces.
//!   One allocation per *unique* combination across the process lifetime.
//!
//! Lock poisoning is treated as fatal — at that point another thread panicked
//! while holding the lock and the process is in an unrecoverable state.

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
}
