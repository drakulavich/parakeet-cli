//! Speaker diarization on darwin-arm64 via the native `fluidaudio-rs`
//! `diarize_file_with_models` (FluidAudio SortformerDiarizer, pre-staged model,
//! no download). Closes #199 angle D.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use crate::{dtrace, dtrace_json};
use fluidaudio_rs::FluidAudio;

use super::TranscriptionSegment;

const MIN_DIARIZE_SEGMENT_COVERAGE: f32 = 0.95;
const MAX_DIARIZE_TAIL_GAP_SECONDS: f32 = 30.0;

// Adaptive diarization timeout: the in-process `diarize_file_with_models` call is
// blocking and un-interruptible, so `run_with_timeout` runs it on a worker thread
// and bails if it overruns. Default 90 s, scaled up by audio length / ASR segment
// count, capped at 30 min, overridable via `KESHA_DIARIZE_TIMEOUT_SECS`. (#434)
const DEFAULT_DIARIZE_TIMEOUT_SECS: u64 = 90;
const MAX_ADAPTIVE_DIARIZE_TIMEOUT_SECS: u64 = 1_800;
const DIARIZE_TIMEOUT_SECONDS_PER_AUDIO_SECOND: f32 = 0.05;
const DIARIZE_TIMEOUT_SECONDS_PER_ASR_SEGMENT: f32 = 0.10;

/// One speaker span emitted by the sidecar. Cluster IDs are stable within
/// one invocation but not across calls.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct DiarizeSpan {
    pub start: f32,
    pub end: f32,
    pub speaker: u32,
}

/// Diarize `audio_path` using the pre-staged model at `model_path` via the
/// native FluidAudio binding (`diarize_file_with_models` — no download). The
/// span list is validated against the ASR timeline before merge, so callers
/// never receive silently partial speaker labels (#397).
pub(crate) fn run(
    audio_path: &Path,
    model_path: &Path,
    asr_segments: &[TranscriptionSegment],
    duration: Option<f32>,
) -> Result<Vec<DiarizeSpan>> {
    let audio_secs = duration
        .or_else(|| max_asr_end(asr_segments))
        .unwrap_or(0.0);
    let timeout = diarize_timeout(asr_segments, duration);
    dtrace!(
        "diarize::start timeout={}s audio_secs={:.1} asr_segments={}",
        timeout.as_secs(),
        audio_secs,
        asr_segments.len()
    );
    dtrace_json!(
        "diarize.start",
        {
            "timeout_secs": timeout.as_secs(),
            "audio_secs": audio_secs,
            "asr_segments": asr_segments.len()
        }
    );

    let spans: Vec<DiarizeSpan> = run_with_timeout(audio_path, model_path, timeout, audio_secs)?;

    let coverage = validate_coverage(asr_segments, &spans)?;
    dtrace!(
        "diarize::coverage spans={} labeled={}/{} ratio={:.3} span_end={:.1}s asr_end={:.1}s",
        spans.len(),
        coverage.labeled_segments,
        coverage.total_segments,
        coverage.coverage_ratio,
        coverage.max_span_end,
        coverage.max_asr_end
    );
    dtrace_json!(
        "diarize.coverage",
        {
            "spans": spans.len(),
            "labeled_segments": coverage.labeled_segments,
            "total_segments": coverage.total_segments,
            "coverage_ratio": coverage.coverage_ratio,
            "max_span_end": coverage.max_span_end,
            "max_asr_end": coverage.max_asr_end
        }
    );
    Ok(spans)
}

/// Adaptive deadline for one diarization call. `KESHA_DIARIZE_TIMEOUT_SECS`
/// overrides everything; otherwise scale up from a 90 s floor by audio length and
/// ASR-segment count, capped at 30 min.
fn diarize_timeout(asr_segments: &[TranscriptionSegment], duration: Option<f32>) -> Duration {
    if let Some(secs) = std::env::var("KESHA_DIARIZE_TIMEOUT_SECS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|secs| *secs > 0)
    {
        return Duration::from_secs(secs);
    }

    let asr_end = max_asr_end(asr_segments);
    let audio_secs = duration.or(asr_end).unwrap_or(0.0).max(0.0);
    let by_audio = (audio_secs * DIARIZE_TIMEOUT_SECONDS_PER_AUDIO_SECOND).ceil() as u64;
    let by_segments =
        (asr_segments.len() as f32 * DIARIZE_TIMEOUT_SECONDS_PER_ASR_SEGMENT).ceil() as u64;
    let secs = DEFAULT_DIARIZE_TIMEOUT_SECS
        .max(by_audio)
        .max(by_segments)
        .min(MAX_ADAPTIVE_DIARIZE_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

/// Run the (blocking, un-interruptible) FluidAudio diarization on a worker thread
/// so the caller can enforce `timeout`. A stalled CoreML call can't be cancelled,
/// so on timeout we deliberately abandon the worker thread: diarization only runs
/// in a one-shot CLI invocation, so the process exits right after we bail and the
/// OS reclaims the thread + the stalled model. (#434)
fn run_with_timeout(
    audio_path: &Path,
    model_path: &Path,
    timeout: Duration,
    audio_secs: f32,
) -> Result<Vec<DiarizeSpan>> {
    let (tx, rx) = mpsc::channel();
    let audio_path = audio_path.to_path_buf();
    let model_path = model_path.to_path_buf();
    std::thread::spawn(move || {
        // FluidAudio is created inside the thread (it never crosses the boundary).
        // The oneshot guard silences synchronous CoreML stdout noise during the
        // call; the *asynchronous* `E5RT` teardown print (fired on a background
        // queue after the call returns) is silenced by `StdoutShield` at the CLI
        // layer, which keeps fd 1 redirected past process exit. (#259/#397/#434)
        let result =
            crate::fluid_stdout::with_silenced_stdout_oneshot(|| -> Result<Vec<DiarizeSpan>> {
                let audio = FluidAudio::new().context("failed to initialize FluidAudio bridge")?;
                let segments = audio
                    .diarize_file_with_models(&audio_path, &model_path)
                    .context("FluidAudio diarization failed")?;
                let mut seen: HashMap<String, u32> = HashMap::new();
                Ok(segments
                    .into_iter()
                    .map(|seg| DiarizeSpan {
                        start: seg.start_time,
                        end: seg.end_time,
                        speaker: speaker_id_to_index(&mut seen, &seg.speaker_id),
                    })
                    .collect())
            });
        // The receiver is gone if we already timed out; dropping `result` is fine.
        let _ = tx.send(result);
    });

    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => bail!(
            "speaker diarization timed out after {}s for {:.0}s of audio; \
             set KESHA_DIARIZE_TIMEOUT_SECS to override the adaptive limit",
            timeout.as_secs(),
            audio_secs,
        ),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            bail!("speaker diarization worker terminated unexpectedly")
        }
    }
}

/// Map FluidAudio's speaker labels to stable numeric ids by first-seen order.
/// Unlike parsing the `SPEAKER_NN` suffix, distinct labels never collide onto the
/// same id (#434) — the merge contract only needs ids stable within a single call.
fn speaker_id_to_index(seen: &mut HashMap<String, u32>, label: &str) -> u32 {
    if let Some(&idx) = seen.get(label) {
        return idx;
    }
    let idx = seen.len() as u32;
    seen.insert(label.to_string(), idx);
    idx
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DiarizeCoverage {
    pub total_segments: usize,
    pub labeled_segments: usize,
    pub coverage_ratio: f32,
    pub max_asr_end: f32,
    pub max_span_end: f32,
}

pub(crate) fn validate_coverage(
    asr_segments: &[TranscriptionSegment],
    diarize_spans: &[DiarizeSpan],
) -> Result<DiarizeCoverage> {
    if asr_segments.is_empty() {
        return Ok(DiarizeCoverage {
            total_segments: 0,
            labeled_segments: 0,
            coverage_ratio: 1.0,
            max_asr_end: 0.0,
            max_span_end: 0.0,
        });
    }

    let max_asr_end = max_asr_end(asr_segments).unwrap_or(0.0);
    let max_span_end = diarize_spans
        .iter()
        .map(|span| span.end)
        .fold(0.0_f32, f32::max);
    let labeled_segments = asr_segments
        .iter()
        .filter(|seg| {
            let midpoint = (seg.start + seg.end) / 2.0;
            diarize_spans
                .iter()
                .any(|span| span.start <= midpoint && midpoint < span.end)
        })
        .count();
    let total_segments = asr_segments.len();
    let coverage_ratio = labeled_segments as f32 / total_segments as f32;
    let coverage = DiarizeCoverage {
        total_segments,
        labeled_segments,
        coverage_ratio,
        max_asr_end,
        max_span_end,
    };

    if max_span_end + MAX_DIARIZE_TAIL_GAP_SECONDS < max_asr_end
        || coverage_ratio < MIN_DIARIZE_SEGMENT_COVERAGE
    {
        bail!(
            "speaker diarization coverage incomplete: labeled {}/{} segments ({:.1}%), \
             spans end at {:.1}s while transcript ends at {:.1}s",
            labeled_segments,
            total_segments,
            coverage_ratio * 100.0,
            max_span_end,
            max_asr_end
        );
    }

    Ok(coverage)
}

fn max_asr_end(asr_segments: &[TranscriptionSegment]) -> Option<f32> {
    asr_segments.iter().map(|seg| seg.end).reduce(f32::max)
}

/// Project each ASR segment onto the diarization timeline by midpoint
/// overlap. For each ASR segment, find the diarize span whose
/// `[start, end)` covers the ASR segment's midpoint; assign that span's
/// speaker. If no diarize span covers the midpoint, leave `speaker = None`.
pub(crate) fn merge_into(
    asr_segs: Vec<TranscriptionSegment>,
    diarize_spans: &[DiarizeSpan],
) -> Vec<TranscriptionSegment> {
    asr_segs
        .into_iter()
        .map(|mut seg| {
            let midpoint = (seg.start + seg.end) / 2.0;
            seg.speaker = diarize_spans
                .iter()
                .find(|s| s.start <= midpoint && midpoint < s.end)
                .map(|s| s.speaker);
            seg
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start: f32, end: f32, text: &str) -> TranscriptionSegment {
        TranscriptionSegment {
            start,
            end,
            text: text.into(),
            speaker: None,
        }
    }
    fn span(start: f32, end: f32, speaker: u32) -> DiarizeSpan {
        DiarizeSpan {
            start,
            end,
            speaker,
        }
    }

    #[test]
    fn one_to_one_overlap_assigns_speaker() {
        let segs = vec![seg(1.0, 3.0, "hi")];
        let spans = vec![span(0.0, 5.0, 7)];
        let out = merge_into(segs, &spans);
        assert_eq!(out[0].speaker, Some(7));
    }

    #[test]
    fn no_overlap_yields_none() {
        let segs = vec![seg(10.0, 11.0, "hi")];
        let spans = vec![span(0.0, 5.0, 0)];
        let out = merge_into(segs, &spans);
        assert_eq!(out[0].speaker, None);
    }

    #[test]
    fn span_split_assigns_via_midpoint() {
        // ASR seg 1.0-3.0, midpoint 2.0. Spans: 0..1.5 (speaker 0), 1.5..5 (speaker 1).
        // 2.0 ∈ [1.5, 5) → speaker 1.
        let segs = vec![seg(1.0, 3.0, "hi")];
        let spans = vec![span(0.0, 1.5, 0), span(1.5, 5.0, 1)];
        let out = merge_into(segs, &spans);
        assert_eq!(out[0].speaker, Some(1));
    }

    #[test]
    fn empty_diarize_spans_yield_all_none() {
        let segs = vec![seg(0.0, 1.0, "a"), seg(1.0, 2.0, "b")];
        let out = merge_into(segs, &[]);
        assert!(out.iter().all(|s| s.speaker.is_none()));
    }

    #[test]
    fn empty_asr_segs_returns_empty() {
        let out = merge_into(vec![], &[span(0.0, 5.0, 0)]);
        assert!(out.is_empty());
    }

    #[test]
    fn four_speaker_meeting_assigns_distinct_ids() {
        let segs = vec![
            seg(0.5, 1.5, "a"),
            seg(2.0, 3.0, "b"),
            seg(4.0, 5.0, "c"),
            seg(6.0, 7.0, "d"),
        ];
        let spans = vec![
            span(0.0, 1.7, 0),
            span(1.7, 3.5, 1),
            span(3.5, 5.5, 2),
            span(5.5, 8.0, 3),
        ];
        let out = merge_into(segs, &spans);
        assert_eq!(
            out.iter().map(|s| s.speaker).collect::<Vec<_>>(),
            vec![Some(0), Some(1), Some(2), Some(3)]
        );
    }

    #[test]
    fn coverage_validation_accepts_full_timeline() {
        let segs = vec![seg(0.0, 1.0, "a"), seg(1.0, 2.0, "b")];
        let spans = vec![span(0.0, 1.0, 0), span(1.0, 2.0, 1)];

        let coverage = validate_coverage(&segs, &spans).expect("full coverage should pass");

        assert_eq!(coverage.total_segments, 2);
        assert_eq!(coverage.labeled_segments, 2);
        assert_eq!(coverage.coverage_ratio, 1.0);
        assert_eq!(coverage.max_asr_end, 2.0);
        assert_eq!(coverage.max_span_end, 2.0);
    }

    #[test]
    fn coverage_validation_rejects_spans_that_end_mid_transcript() {
        let segs = vec![seg(0.0, 10.0, "a"), seg(100.0, 110.0, "b")];
        let spans = vec![span(0.0, 10.0, 0)];

        let err = validate_coverage(&segs, &spans)
            .expect_err("mid-run diarization stop should fail closed");
        let msg = format!("{err}");

        assert!(msg.contains("speaker diarization coverage incomplete"));
        assert!(msg.contains("labeled 1/2 segments"));
        assert!(msg.contains("spans end at 10.0s while transcript ends at 110.0s"));
    }

    #[test]
    fn coverage_validation_rejects_low_midpoint_coverage() {
        let segs = vec![
            seg(0.0, 1.0, "a"),
            seg(1.0, 2.0, "b"),
            seg(2.0, 3.0, "c"),
            seg(3.0, 4.0, "d"),
        ];
        let spans = vec![span(0.0, 4.0, 0), span(10.0, 20.0, 1)];
        let sparse_spans = vec![span(0.0, 1.0, 0), span(10.0, 20.0, 1)];

        validate_coverage(&segs, &spans).expect("full midpoint coverage should pass");
        let err =
            validate_coverage(&segs, &sparse_spans).expect_err("low midpoint coverage should fail");
        let msg = format!("{err}");

        assert!(msg.contains("labeled 1/4 segments"));
        assert!(msg.contains("(25.0%)"));
    }

    #[test]
    fn coverage_validation_rejects_empty_spans_when_asr_has_segments() {
        let segs = vec![seg(0.0, 1.0, "a")];

        let err = validate_coverage(&segs, &[]).expect_err("empty spans should fail");
        let msg = format!("{err}");

        assert!(msg.contains("labeled 0/1 segments"));
        assert!(msg.contains("spans end at 0.0s while transcript ends at 1.0s"));
    }

    #[test]
    fn coverage_validation_allows_empty_asr_segments() {
        let coverage =
            validate_coverage(&[], &[]).expect("no ASR segments means no missing labels");

        assert_eq!(coverage.total_segments, 0);
        assert_eq!(coverage.labeled_segments, 0);
        assert_eq!(coverage.coverage_ratio, 1.0);
    }

    #[test]
    fn speaker_ids_map_by_first_seen_order_without_collision() {
        let mut seen = HashMap::new();
        assert_eq!(speaker_id_to_index(&mut seen, "SPEAKER_00"), 0);
        assert_eq!(speaker_id_to_index(&mut seen, "SPEAKER_01"), 1);
        // Stable within a call.
        assert_eq!(speaker_id_to_index(&mut seen, "SPEAKER_00"), 0);
        // The #433 P2 bug: distinct labels sharing a numeric suffix must NOT
        // collapse onto the same id (the old `rsplit('_')` parse mapped both to 0).
        assert_eq!(speaker_id_to_index(&mut seen, "A_0"), 2);
        assert_eq!(speaker_id_to_index(&mut seen, "B_0"), 3);
    }

    #[test]
    fn adaptive_timeout_keeps_short_audio_near_current_default() {
        let _guard = EnvLockGuard::new();
        let segs = vec![seg(0.0, 1.0, "a")];

        unsafe {
            std::env::remove_var("KESHA_DIARIZE_TIMEOUT_SECS");
        }

        assert_eq!(
            diarize_timeout(&segs, Some(10.0)),
            Duration::from_secs(DEFAULT_DIARIZE_TIMEOUT_SECS)
        );
    }

    #[test]
    fn adaptive_timeout_scales_for_long_audio() {
        let _guard = EnvLockGuard::new();
        let segs: Vec<_> = (0..6_000)
            .map(|i| {
                let start = i as f32;
                seg(start, start + 0.5, "a")
            })
            .collect();

        unsafe {
            std::env::remove_var("KESHA_DIARIZE_TIMEOUT_SECS");
        }

        assert_eq!(
            diarize_timeout(&segs, Some(12_000.0)),
            Duration::from_secs(600)
        );
    }

    #[test]
    fn adaptive_timeout_is_capped() {
        let _guard = EnvLockGuard::new();
        let segs: Vec<_> = (0..100_000)
            .map(|i| {
                let start = i as f32;
                seg(start, start + 0.5, "a")
            })
            .collect();

        unsafe {
            std::env::remove_var("KESHA_DIARIZE_TIMEOUT_SECS");
        }

        assert_eq!(
            diarize_timeout(&segs, Some(100_000.0)),
            Duration::from_secs(MAX_ADAPTIVE_DIARIZE_TIMEOUT_SECS)
        );
    }

    #[test]
    fn adaptive_timeout_env_override_wins() {
        let _guard = EnvLockGuard::new();
        let segs = vec![seg(0.0, 1.0, "a")];
        let _env = EnvGuard::set("KESHA_DIARIZE_TIMEOUT_SECS", "3600");

        assert_eq!(
            diarize_timeout(&segs, Some(1.0)),
            Duration::from_secs(3_600)
        );
    }

    struct EnvLockGuard {
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvLockGuard {
        fn new() -> Self {
            static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
            Self {
                _guard: LOCK
                    .get_or_init(|| std::sync::Mutex::new(()))
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()),
            }
        }
    }

    struct EnvGuard {
        key: &'static str,
        original: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, val: &str) -> Self {
            let original = std::env::var(key).ok();
            unsafe {
                std::env::set_var(key, val);
            }
            Self { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.original {
                Some(v) => unsafe {
                    std::env::set_var(self.key, v);
                },
                None => unsafe {
                    std::env::remove_var(self.key);
                },
            }
        }
    }
}
