//! End-to-end regression tests for audio-container error handling on
//! `transcribe`. Lives outside `rust/src/transcribe/` so it can use the
//! checked-in fixture under `rust/tests/fixtures/silence.m4a` without
//! pulling that path into the production module's `use` graph.
//!
//! Both tests exercise `transcribe_with_options`'s `audio::ensure_audio_track`
//! early bail (v1.17.0). The garbage-input test verifies the
//! "unsupported audio format" arm; the m4a-fixture test verifies the
//! `isomp4` symphonia feature is wired so AAC-in-M4A probes succeed.

use kesha_engine::audio;

#[test]
fn ensure_audio_track_bails_on_unsupported_container() {
    let tmp = tempfile::Builder::new()
        .prefix("kesha-bad-container-")
        .suffix(".bin")
        .tempfile()
        .unwrap();
    std::fs::write(tmp.path(), b"not an audio file at all").unwrap();

    let err = audio::ensure_audio_track(tmp.path().to_str().unwrap())
        .expect_err("should bail on unsupported container");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("unsupported audio format"),
        "expected clear format error, got: {msg}"
    );
}

#[test]
fn ensure_audio_track_accepts_isomp4_aac_fixture() {
    // Regression catcher: if someone drops `isomp4` from the symphonia
    // feature list in rust/Cargo.toml again, opening this m4a will fail
    // with "unsupported audio format" and this test goes red. The
    // fixture is ~900 bytes of silence (0.5 s @ 16 kHz mono AAC) so the
    // repo footprint is negligible.
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("silence.m4a");
    assert!(fixture.exists(), "fixture missing: {}", fixture.display());

    audio::ensure_audio_track(fixture.to_str().unwrap())
        .expect("isomp4 feature should let symphonia open the m4a container");

    // `probe_duration_seconds` should also work on the same fixture.
    // ffmpeg's anullsrc tends to leave `n_frames` populated for AAC-LC,
    // so we expect Some. If the encoder ever changes and starts emitting
    // a streaming-style m4a without n_frames, the assertion can flip to
    // `is_ok()` (the test still proves isomp4 is enabled).
    let dur = audio::probe_duration_seconds(fixture.to_str().unwrap())
        .expect("probe_duration_seconds should succeed on the m4a fixture");
    assert!(
        dur.is_some(),
        "expected Some duration for the silence.m4a fixture, got None"
    );
}

#[test]
fn ensure_audio_track_accepts_uppercase_extension() {
    // F21: `build_hint` lowercases the extension before handing it to
    // symphonia. Symphonia's matching is case-insensitive today, but
    // normalising at the boundary is a defensive zero-cost guard. This
    // test pins the normalisation: copy the m4a fixture to a tempfile
    // with `.M4A` (uppercase) and verify probe still succeeds. If a
    // future refactor drops the lowercase pass and symphonia tightens
    // its matching upstream, this test goes red.
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("silence.m4a");
    let tmp = tempfile::Builder::new()
        .prefix("kesha-uppercase-ext-")
        .suffix(".M4A")
        .tempfile()
        .expect("create tempfile with uppercase extension");
    std::fs::copy(&fixture, tmp.path()).expect("copy fixture into uppercase tempfile");

    audio::ensure_audio_track(tmp.path().to_str().unwrap())
        .expect("uppercase .M4A extension should still probe successfully");
}
