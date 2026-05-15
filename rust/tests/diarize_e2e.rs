//! End-to-end diarization smoke. Synthesizes a 2-speaker dialogue from kesha's
//! own TTS (3 utterances: speaker A, speaker B, speaker A again), concatenates
//! the WAVs in-process via `hound`, and runs
//! `kesha-engine transcribe --json --speakers` against the result. Asserts the
//! output JSON has segments with at least 2 distinct speaker IDs and that
//! ≥ 80% of segments carry a speaker label.
//!
//! Gated on `system_diarize` so the test compiles only on darwin-arm64 builds
//! that ship the Swift sidecar. Skips at runtime — without failing — when:
//!   * Kokoro EN models aren't installed (`kesha install --tts`),
//!   * the Sortformer model isn't installed (`kesha install --diarize`),
//!   * or the engine binary is missing.
//!
//! Closes #199.

#![cfg(feature = "system_diarize")]

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

fn say(text: &str, voice: &str, out: &Path) -> bool {
    Command::new(common::engine_bin())
        .args(["say", "--voice", voice, "--out", out.to_str().unwrap()])
        .arg(text)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Concatenate a list of WAVs (assumed same sample rate / channels / sample
/// format) into a single WAV. Avoids depending on ffmpeg.
fn concat_wavs(inputs: &[PathBuf], out: &Path) {
    let first = hound::WavReader::open(&inputs[0]).expect("read first wav");
    let spec = first.spec();
    drop(first);

    let mut writer = hound::WavWriter::create(out, spec).expect("create combined wav");
    for path in inputs {
        let mut reader = hound::WavReader::open(path).expect("read wav for concat");
        assert_eq!(reader.spec(), spec, "wav specs differ across utterances");
        match spec.sample_format {
            hound::SampleFormat::Float => {
                for s in reader.samples::<f32>() {
                    writer.write_sample(s.expect("read f32 sample")).unwrap();
                }
            }
            hound::SampleFormat::Int => {
                for s in reader.samples::<i32>() {
                    writer.write_sample(s.expect("read int sample")).unwrap();
                }
            }
        }
    }
    writer.finalize().expect("finalize combined wav");
}

#[test]
fn two_speaker_dialogue_yields_two_clusters() {
    let exe = PathBuf::from(common::engine_bin());
    if !exe.exists() {
        eprintln!("skipping: engine binary not found at {}", exe.display());
        return;
    }

    let tmp = tempfile::Builder::new()
        .prefix("kesha-diarize-e2e-")
        .tempdir()
        .unwrap();

    let p1a = tmp.path().join("p1a.wav");
    let p2 = tmp.path().join("p2.wav");
    let p1b = tmp.path().join("p1b.wav");

    // Voices chosen from the always-bundled kokoro-onnx pair (am_michael + af_heart);
    // British male voices like en-bm_george are downloadable but not part of the
    // default install, which would skip the test on a clean machine. We need two
    // sufficiently different voices for diarization to cluster them apart, and an
    // American male + American female pair satisfies that constraint cleanly.
    if !say(
        "Hello everyone, this is the first speaker beginning the call.",
        "en-am_michael",
        &p1a,
    ) || !say(
        "Hi, this is the second speaker responding now.",
        "en-af_heart",
        &p2,
    ) || !say(
        "Yes thanks for joining, the first speaker again.",
        "en-am_michael",
        &p1b,
    ) {
        eprintln!("skipping: TTS voices not installed (run `kesha install --tts`)");
        return;
    }

    let combined = tmp.path().join("dialogue.wav");
    concat_wavs(&[p1a, p2, p1b], &combined);

    // VAD is required: without it the Plain ASR path emits a single
    // whole-file segment, all ASR midpoints land inside one diarize span,
    // and the speaker-diversity assertion below collapses to one cluster.
    // VAD turns the dialogue into per-utterance segments which is the only
    // shape where speaker labels can be different across segments.
    let out = Command::new(&exe)
        .args([
            "transcribe",
            "--json",
            "--vad",
            "--speakers",
            combined.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        // Skip — don't fail — when an external prerequisite isn't installed.
        // The installer flows for each are exercised by separate unit tests.
        if stderr.contains("diarization model not found")
            || stderr.contains("kesha-diarize sidecar not found")
            || stderr.contains("VAD model")
            || stderr.contains("silero-vad")
        {
            eprintln!(
                "skipping: prerequisite missing (run `kesha install --vad --diarize`):\n{stderr}"
            );
            return;
        }
        panic!("engine transcribe --speakers failed: {stderr}");
    }

    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("engine output is not valid JSON");

    let segments = json["segments"].as_array().expect("segments is an array");
    assert!(!segments.is_empty(), "no segments produced");

    let speakers: std::collections::HashSet<u64> = segments
        .iter()
        .filter_map(|s| s["speaker"].as_u64())
        .collect();
    assert!(
        speakers.len() >= 2,
        "expected ≥ 2 distinct speakers, got {speakers:?}; segments: {segments:#?}"
    );

    let labeled = segments.iter().filter(|s| s["speaker"].is_u64()).count();
    let labeled_ratio = labeled as f32 / segments.len() as f32;
    assert!(
        labeled_ratio >= 0.80,
        "expected ≥ 80% of segments to have a speaker label, got {:.0}%",
        labeled_ratio * 100.0
    );
}
