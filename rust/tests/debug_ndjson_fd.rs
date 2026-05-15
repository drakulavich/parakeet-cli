//! Integration test for the F19 NDJSON sink: `KESHA_DEBUG_FD=N` makes
//! [`kesha_engine::debug::trace_json`] write structured events to fd `N`.
//!
//! Lives as its own integration binary so the process-wide env-var +
//! `OnceLock<JSON_SINK>` state can't bleed into other test binaries.
//! Each `cargo test` integration binary runs in its own process.

#![cfg(unix)]

use std::io::{Read, Seek, SeekFrom};
use std::os::fd::IntoRawFd;

#[test]
fn json_sink_emits_ndjson_with_event_and_t_ms_to_configured_fd() {
    let mut tmp = tempfile::tempfile().expect("tempfile");
    // `try_clone()` shares the underlying kernel descriptor; `into_raw_fd()`
    // hands ownership of the duplicate to the test environment so the
    // `OnceLock`-stored `File` inside `debug.rs` can close it on Drop
    // without affecting our `tmp` reader.
    let dup = tmp.try_clone().expect("dup tempfile");
    let fd = dup.into_raw_fd();

    // Set BEFORE the first `trace_json` call — `json_sink()` reads the
    // env once via `OnceLock::get_or_init`. Subsequent changes are
    // silently ignored, which is intentional (the writer's fd identity
    // must be stable for the engine's lifetime).
    std::env::set_var("KESHA_DEBUG_FD", fd.to_string());

    kesha_engine::debug::trace_json("test.first", serde_json::json!({"x": 1, "label": "ok"}));
    kesha_engine::debug::trace_json("test.second", serde_json::json!({"y": 2}));

    tmp.seek(SeekFrom::Start(0)).expect("seek to start");
    let mut contents = String::new();
    tmp.read_to_string(&mut contents).expect("read tempfile");

    let lines: Vec<&str> = contents.trim_end_matches('\n').split('\n').collect();
    assert_eq!(lines.len(), 2, "expected 2 NDJSON lines, got: {contents:?}");

    let v1: serde_json::Value = serde_json::from_str(lines[0]).expect("line 1 valid JSON");
    assert_eq!(v1["event"], "test.first");
    assert_eq!(v1["x"], 1);
    assert_eq!(v1["label"], "ok");
    assert!(v1["t_ms"].is_u64(), "t_ms missing on line 1: {v1}");

    let v2: serde_json::Value = serde_json::from_str(lines[1]).expect("line 2 valid JSON");
    assert_eq!(v2["event"], "test.second");
    assert_eq!(v2["y"], 2);
    assert!(v2["t_ms"].is_u64(), "t_ms missing on line 2: {v2}");

    // Best-effort cleanup. `OnceLock` already captured the fd, so removing
    // the env after the fact doesn't disable the sink — but it keeps the
    // process env tidy for any subsequent `Bun.spawn`-style children.
    std::env::remove_var("KESHA_DEBUG_FD");
}
