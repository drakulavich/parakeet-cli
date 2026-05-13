//! Debug trace (#148): stderr `[debug/engine +Nms] ...` lines when
//! `KESHA_DEBUG` is truthy. No-op otherwise. Boundary-only — never
//! per-sample, never in the hot inference loop.
//!
//! Pairs with the TS-side `log.debug()` on the CLI wrapper. Together:
//!
//! ```text
//! $ KESHA_DEBUG=1 kesha audio.ogg
//! [debug +12ms] spawn /.../kesha-engine transcribe audio.ogg
//! [debug/engine +5ms] audio::load_mono16k audio.ogg
//! [debug/engine +14ms] asr::backend=onnx
//! [debug/engine +354ms] asr::transcribe.end dt=340ms chars=42
//! [debug +365ms] exit=0 dt=352ms args=["transcribe","audio.ogg"]
//! ```
//!
//! The `+Nms` prefix is relative to the LOGGER's own start (TS process
//! start vs Rust process start) — the two axes are independent. Useful
//! to see WHEN each line fired on its own process timeline; for the
//! span between two events on the same side, the inline `dt=Nms` token
//! inside the message remains the right number.

use std::sync::OnceLock;
use std::time::Instant;

/// Values that turn `KESHA_DEBUG` OFF — empty, `"0"`, `"false"`, `"no"`,
/// `"off"`, all matched **case-insensitively** after trimming. Any other
/// non-empty value turns debug ON. Mirrored verbatim in `src/log.ts`
/// (post-#275 D9) so `KESHA_DEBUG=False` flips both sides the same
/// direction.
const KESHA_DEBUG_OFF_VALUES: &[&str] = &["", "0", "false", "no", "off"];

/// Parse a raw env-var value into the boolean debug state. Pure helper so
/// production `enabled()` and the test below stay aligned by construction.
fn debug_on_for(value: Option<&str>) -> bool {
    match value {
        None => false,
        Some(s) => {
            let normalized = s.trim().to_ascii_lowercase();
            !KESHA_DEBUG_OFF_VALUES.contains(&normalized.as_str())
        }
    }
}

fn enabled() -> bool {
    static CACHE: OnceLock<bool> = OnceLock::new();
    *CACHE.get_or_init(|| debug_on_for(std::env::var("KESHA_DEBUG").ok().as_deref()))
}

static T0: OnceLock<Instant> = OnceLock::new();

/// Engine-side process-start timestamp for the relative-ms prefix on
/// debug lines. Anchored by [`init`] at the top of `main()` so early
/// startup work (clap parsing, model-cache stat, env probes) shows up
/// in the `+Nms` prefix instead of being collapsed into "+0ms" on the
/// first `dtrace!` call (Greptile P2 on #293). NOT the same axis as
/// the TS side's `PROCESS_T0_MS` — each process logs against its own
/// start.
fn engine_t0() -> Instant {
    *T0.get_or_init(Instant::now)
}

/// Anchor the relative-ms timeline as early as possible in process life.
/// Idempotent — first call wins; later calls are a no-op (the
/// `OnceLock::get_or_init` semantic). Safe to call even when
/// `KESHA_DEBUG` is off; the only cost is one atomic `OnceLock` load.
///
/// Call this AS THE FIRST line of `main()` so the timeline starts before
/// `Cli::parse()` and any pre-dispatch work. Without it, the first
/// `dtrace!` call anchors T0 and earlier work is invisible.
pub fn init() {
    let _ = engine_t0();
}

/// Emit a stderr trace line when `KESHA_DEBUG` is on. Accepts `format_args!`
/// so call sites don't allocate when debug is off — `enabled()` is one atomic
/// load via OnceLock. Use via the `dtrace!` macro below.
pub fn trace_fmt(args: std::fmt::Arguments<'_>) {
    if enabled() {
        let t = engine_t0().elapsed().as_millis();
        eprintln!("[debug/engine +{t}ms] {args}");
    }
}

/// Convenience macro so call sites don't allocate when off.
#[macro_export]
macro_rules! dtrace {
    ($($arg:tt)*) => {
        $crate::debug::trace_fmt(format_args!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    // `enabled()` caches via OnceLock, so it can only be probed once per
    // process. Call the pure helper directly instead — it covers the same
    // parsing rule that production uses.
    use super::debug_on_for;

    #[test]
    fn off_when_unset() {
        assert!(!debug_on_for(None));
    }

    #[test]
    fn off_for_zero_false_empty() {
        assert!(!debug_on_for(Some("0")));
        assert!(!debug_on_for(Some("false")));
        assert!(!debug_on_for(Some("")));
    }

    #[test]
    fn off_for_no_and_off() {
        // Expanded grammar (#275 D9): `no` and `off` join the off-set.
        assert!(!debug_on_for(Some("no")));
        assert!(!debug_on_for(Some("off")));
    }

    #[test]
    fn off_case_insensitive() {
        // The pre-D9 Rust pattern was exact-case `"false"`, which let
        // `"False"` slip through and flipped only the engine ON. Lock
        // the case-insensitive contract in.
        assert!(!debug_on_for(Some("False")));
        assert!(!debug_on_for(Some("FALSE")));
        assert!(!debug_on_for(Some("No")));
        assert!(!debug_on_for(Some("OFF")));
    }

    #[test]
    fn off_with_surrounding_whitespace() {
        // `KESHA_DEBUG=" false "` is functionally the same intent as
        // `=false`; trim before comparing.
        assert!(!debug_on_for(Some("  false  ")));
        assert!(!debug_on_for(Some("\t0\n")));
    }

    #[test]
    fn on_for_one_true_anything() {
        assert!(debug_on_for(Some("1")));
        assert!(debug_on_for(Some("true")));
        assert!(debug_on_for(Some("anything")));
    }
}
