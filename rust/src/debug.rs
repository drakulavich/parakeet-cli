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
//!
//! # Structured NDJSON sink (`KESHA_DEBUG_FD`, F19)
//!
//! `[debug/engine ...]` lines on stderr are great for humans but mix
//! with `eprintln!("hint: ...")` / `eprintln!("warning: ...")` progress
//! that the CLI surfaces to the user. For machine consumers (tooling,
//! CI logs, perf dashboards) we want a clean stream of structured
//! events on a dedicated channel.
//!
//! Set `KESHA_DEBUG_FD=N` to a valid file-descriptor integer that the
//! parent process opened before exec — e.g. `kesha-engine ... 3>trace.ndjson`
//! makes fd=3 a writable pipe. Each [`dtrace_json!`] call then emits one
//! JSON line:
//!
//! ```text
//! {"t_ms": 12, "event": "asr.backend_loaded", "dt_ms": 8}
//! {"t_ms": 354, "event": "asr.transcribe.end", "dt_ms": 340, "chars": 42}
//! ```
//!
//! Independent of `KESHA_DEBUG`: both can be on simultaneously (text
//! to stderr AND JSON to fd=3), or just one, or neither. The text
//! path is preserved for back-compat with `KESHA_DEBUG=1 kesha ...`
//! workflows; the JSON path is opt-in via the env var.

use std::fs::File;
use std::io::Write;
#[cfg(unix)]
use std::os::fd::FromRawFd;
use std::sync::{Mutex, OnceLock};
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

// ---------------------------------------------------------------------------
// Structured NDJSON sink — F19
// ---------------------------------------------------------------------------

/// Resolved JSON sink. `None` means `KESHA_DEBUG_FD` was unset / invalid
/// at process start; further calls to [`trace_json`] are no-ops without
/// even formatting the JSON payload.
///
/// `Mutex<File>` — not `BufWriter` — because each NDJSON line MUST hit
/// the kernel as one `write(2)` so concurrent threads can't interleave
/// half-lines into the consumer's pipe. Lines stay well under
/// `PIPE_BUF` (4096 on Linux), so a single `write_all` is one syscall
/// in practice.
static JSON_SINK: OnceLock<Option<Mutex<File>>> = OnceLock::new();

#[cfg(unix)]
fn json_sink() -> Option<&'static Mutex<File>> {
    JSON_SINK
        .get_or_init(|| {
            let raw = std::env::var("KESHA_DEBUG_FD").ok()?;
            let fd: i32 = raw.trim().parse().ok()?;
            // stdin/stdout/stderr are off-limits — they belong to the
            // text-CLI contract. Refuse them so a stray `KESHA_DEBUG_FD=2`
            // can't poison stderr with NDJSON.
            if fd < 3 {
                return None;
            }
            // SAFETY: `fd` is an integer the parent process passed via env.
            // The contract is: the parent opened `fd` before exec (e.g. via
            // shell `3>file` redirection or a `posix_spawn`-style FD action)
            // and won't close it for the engine's lifetime. If the parent
            // lied, the first `write(2)` returns EBADF and we silently drop
            // the line — no panic, no abort. The fd is owned by us from
            // here on; std drops the underlying close on `File` Drop.
            let file = unsafe { File::from_raw_fd(fd) };
            Some(Mutex::new(file))
        })
        .as_ref()
}

#[cfg(not(unix))]
fn json_sink() -> Option<&'static Mutex<File>> {
    // The fd-from-int trick is POSIX-specific. On Windows, `KESHA_DEBUG_FD`
    // is a no-op for now; users can keep relying on the stderr text path
    // via `KESHA_DEBUG=1`. Future Windows support would parse a HANDLE
    // instead — out of scope until there's a concrete user request.
    None
}

/// Returns `true` when [`trace_json`] would write to a configured sink.
///
/// Public so the [`dtrace_json!`] macro can short-circuit the
/// `serde_json::json!` allocation when the sink is inactive — matching
/// the zero-heap-allocation contract of the text-path [`dtrace!`]
/// macro (Greptile P2 on #321).
pub fn json_sink_is_active() -> bool {
    json_sink().is_some()
}

/// Emit one structured NDJSON event to the JSON sink, if configured.
///
/// `event` is the dotted event name (e.g. `"asr.backend_loaded"`).
/// `fields` MUST be a [`serde_json::Value::Object`] — a non-object
/// payload trips a `debug_assert!` in dev/test builds and is coerced
/// to an empty map in release. Two reserved keys are added by the
/// writer and override any caller-provided values of the same name:
/// `t_ms` (process-relative timestamp from [`engine_t0`]) and `event`.
///
/// No-op when `KESHA_DEBUG_FD` is unset or invalid. Call sites should
/// use the [`dtrace_json!`] macro instead — it gates `serde_json::json!`
/// construction on [`json_sink_is_active`] so disabled events pay
/// nothing.
pub fn trace_json(event: &str, fields: serde_json::Value) {
    let Some(sink) = json_sink() else {
        return;
    };
    let mut payload = match fields {
        serde_json::Value::Object(map) => map,
        other => {
            // Non-object payloads are call-site bugs (the macro grammar
            // accepts them today but the writer can't merge them with
            // `t_ms` / `event`). Catch in dev/test; degrade to empty
            // map in release so production stays panic-free.
            debug_assert!(
                false,
                "dtrace_json! expects a JSON object payload, got: {other:?}"
            );
            serde_json::Map::new()
        }
    };
    let t = engine_t0().elapsed().as_millis();
    payload.insert(
        "t_ms".into(),
        serde_json::Value::Number(serde_json::Number::from(t as u64)),
    );
    payload.insert("event".into(), serde_json::Value::String(event.into()));
    let mut line = serde_json::to_vec(&payload).unwrap_or_else(|_| {
        // Serialisation of a JSON `Map<String, Value>` is infallible in
        // practice (no float NaN/Inf paths possible from this side); the
        // fallback here keeps `trace_json` panic-free if upstream
        // serde_json ever returns Err.
        Vec::new()
    });
    if line.is_empty() {
        return;
    }
    line.push(b'\n');
    if let Ok(mut guard) = sink.lock() {
        // Best-effort: write failures (EBADF, broken pipe, full disk)
        // silently drop the line. The structured trace is observability,
        // not a contract — surfacing IO errors here would just spam stderr.
        let _ = guard.write_all(&line);
    }
}

/// Emit a structured NDJSON event when [`json_sink_is_active`].
///
/// Usage:
/// ```ignore
/// dtrace_json!("asr.backend_loaded", { "dt_ms": elapsed.as_millis() });
/// dtrace_json!("vad.detect", { "dt_ms": dt, "segments": spans.len() });
/// ```
///
/// Zero-cost when the sink is unset: the `if json_sink_is_active()`
/// gate sits in front of `serde_json::json!`, so disabled events skip
/// the heap allocation that the eager form (Greptile P2 on #321) had.
#[macro_export]
macro_rules! dtrace_json {
    ($event:expr, $fields:tt) => {
        if $crate::debug::json_sink_is_active() {
            $crate::debug::trace_json($event, ::serde_json::json!($fields))
        }
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
