#[cfg(all(feature = "system_diarize", target_os = "macos"))]
pub(crate) mod diarize;
mod options;

pub use options::TranscribeOptionsBuilder;

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::Path;
use std::time::Instant;

use crate::audio;
use crate::backend;
use crate::models;
use crate::vad::{VadConfig, VadDetector, SAMPLE_RATE as VAD_SAMPLE_RATE};
use crate::{dtrace, dtrace_json};

/// Capability-flag string surfaced via `--capabilities-json`. Single source of
/// truth so the engine, the TS CLI gate, and the integration tests can't drift.
pub const TRANSCRIBE_SEGMENTS_FEATURE: &str = "transcribe.segments";

/// Capability flag surfaced via `--capabilities-json` when the engine ships
/// with FluidAudio diarization. Only true on darwin-arm64 release builds
/// that include the `system_diarize` feature. Closes #199 angle D.
#[cfg_attr(not(feature = "system_diarize"), allow(dead_code))]
pub const TRANSCRIBE_DIARIZE_FEATURE: &str = "transcribe.diarize";

/// Duration at which the `Auto` VAD mode flips to VAD preprocessing.
/// Voice messages (<30 s) and short clips don't benefit; meetings and
/// lectures (>2 min) do.
const AUTO_VAD_MIN_SECONDS: f32 = 120.0;

/// File-size floor below which `Auto` mode skips the duration probe entirely.
/// Any audio <120 s at a plausible bitrate weighs well over this threshold;
/// the guard keeps the hot path cheap for voice messages and bounds MP3
/// worst-case probe cost (symphonia scans the file when a Xing header is
/// absent — can reach seconds on large CBR files).
const AUTO_VAD_MIN_FILE_SIZE: u64 = 200_000;

/// Caller-requested VAD behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadMode {
    /// Use VAD when the audio looks long enough and the model is installed,
    /// otherwise skip it silently (with a one-time stderr hint if it would
    /// have helped but the model is missing).
    Auto,
    /// Force VAD on. Errors if the model isn't installed.
    On,
    /// Force VAD off regardless of duration or install state.
    Off,
}

impl VadMode {
    /// Derive the mode from the two mutually-exclusive CLI flags. `(true, true)`
    /// should be caught by clap's `conflicts_with` before we get here; we still
    /// resolve it deterministically (prefer `On`) rather than panicking.
    pub fn from_flags(vad: bool, no_vad: bool) -> Self {
        match (vad, no_vad) {
            (true, _) => Self::On,
            (_, true) => Self::Off,
            _ => Self::Auto,
        }
    }
}

/// `Default` lives with the type it's for. `TranscribeOptions::default()`
/// uses `Auto` so it matches the historical text-only `transcribe(_, Auto)`
/// behavior.
impl Default for VadMode {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TranscriptionSegment {
    pub start: f32,
    pub end: f32,
    pub text: String,
    /// Cluster ID from speaker diarization. `None` when `--speakers` was not
    /// requested (default) or when diarization could not assign a speaker
    /// to this segment. Stable within one `--json --timestamps --speakers`
    /// invocation; not stable across files.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TranscriptionOutput {
    pub text: String,
    pub segments: Vec<TranscriptionSegment>,
}

/// Canonical input shape for [`transcribe_with_options`]. Replaced three
/// per-feature top-level wrappers (`transcribe` / `transcribe_output` /
/// `transcribe_output_with_speakers`) that grew with each new flag (#267 F5).
///
/// New flags should land here as fields, not as a new top-level wrapper.
///
/// Prefer [`TranscribeOptionsBuilder`] over direct struct construction —
/// the builder lifts the `with_speakers => with_segments` constraint into
/// the type system (F18). The runtime [`anyhow::ensure!`] guard inside
/// [`transcribe_with_options`] still fires on direct misuse.
#[derive(Debug, Clone, Copy, Default)]
pub struct TranscribeOptions {
    /// VAD preprocessing selector (Auto / On / Off).
    pub mode: VadMode,
    /// Populate `TranscriptionOutput::segments` with per-utterance
    /// `(start, end, text)` triples. Text-only callers can leave this
    /// `false` to skip the duration probe on the non-VAD plain path.
    pub with_segments: bool,
    /// Run the FluidAudio diarization post-step and label each segment
    /// with its `speaker` cluster id. Currently darwin-arm64 only;
    /// `transcribe_with_options` returns an error on other platforms.
    pub with_speakers: bool,
}

/// Pure decision function so the auto-trigger rules can be unit-tested
/// without ONNX, disk, or symphonia in the loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VadDecision {
    Vad,
    Plain,
    PlainWithHint,
}

fn decide(mode: VadMode, duration_s: Option<f32>, vad_installed: bool) -> VadDecision {
    match mode {
        VadMode::On => VadDecision::Vad,
        VadMode::Off => VadDecision::Plain,
        VadMode::Auto => match duration_s {
            Some(d) if d >= AUTO_VAD_MIN_SECONDS && vad_installed => VadDecision::Vad,
            Some(d) if d >= AUTO_VAD_MIN_SECONDS => VadDecision::PlainWithHint,
            // Unknown duration or short clip → plain, no hint.
            _ => VadDecision::Plain,
        },
    }
}

/// Canonical transcribe entry. New flags should land in [`TranscribeOptions`]
/// instead of growing a new top-level wrapper.
///
/// `opts.with_segments` gates whether the non-VAD plain path probes audio
/// duration to attach a single whole-file segment. Text-only callers skip
/// the probe because `audio::probe_duration_seconds` returns `Ok(None)`
/// for streaming Ogg/Opus without a frame count, which would turn into a
/// hard error for inputs that previously transcribed cleanly. The VAD
/// path always builds segments cheaply (per-span boundaries already in
/// hand from VAD output).
///
/// `opts.with_speakers` triggers the diarization post-step after ASR completes.
/// On darwin-arm64 with the `system_diarize` feature it invokes
/// `diarize::run` + `diarize::merge_into`; on all other platforms it returns
/// a clear error pointing at #199.
pub fn transcribe_with_options(
    audio_path: &str,
    opts: &TranscribeOptions,
) -> Result<TranscriptionOutput> {
    let TranscribeOptions {
        mode,
        with_segments: timestamps_required,
        with_speakers: speakers_required,
    } = *opts;
    // Reject the `{with_speakers: true, with_segments: false}` combination
    // explicitly. On the plain path `transcribe_plain` returns an empty
    // `segments` vec when `timestamps_required == false`, and
    // `diarize::merge_into` would then drop every speaker label silently
    // (Greptile P1 on #290). Before this guard the combination was
    // unreachable: the three legacy wrappers always paired the two flags.
    // Now that `transcribe_with_options` is public, surface the misuse.
    anyhow::ensure!(
        !speakers_required || timestamps_required,
        "TranscribeOptions::with_speakers requires with_segments=true \
         (speaker labels attach to per-utterance segments; without segments \
         there is nowhere to put them)"
    );
    // Bail cleanly on unsupported containers / video-only files BEFORE
    // paying the 25 s+ ANE cold-load tax in `ensure_asr_installed` +
    // backend construction. Symphonia's error message names the
    // container and the failure mode; without this check the user sees
    // the FluidAudio wrapper's cryptic "Swift bridge error: Transcription
    // failed" ~25 s later (v1.16.0 validation against
    // `~/Downloads/assets_demo.webm` + three Zoom m4a samples). Cheap:
    // container-header read only, no frame scan.
    audio::ensure_audio_track(audio_path)?;
    let model_dir = ensure_asr_installed()?;
    let vad_dir = models::model_dir(models::ModelKind::Vad)
        .to_string_lossy()
        .into_owned();
    let vad_installed = models::is_cached(models::ModelKind::Vad);

    // `Auto` needs a duration probe first. `On`/`Off` are deterministic.
    let duration = match mode {
        VadMode::Auto => probe_duration_if_plausible(audio_path),
        _ => None,
    };
    let decision = decide(mode, duration, vad_installed);
    dtrace!(
        "asr::mode={mode:?} duration={:?} vad_installed={vad_installed} decision={decision:?}",
        duration
    );

    // `mut` is only consumed by the diarization post-step below.
    #[cfg_attr(
        not(all(feature = "system_diarize", target_os = "macos")),
        allow(unused_mut)
    )]
    let mut output = match decision {
        VadDecision::Vad => {
            transcribe_via_vad(audio_path, &model_dir, &vad_dir, VadConfig::default())
        }
        // Pass the already-probed duration through so `resolve_segment_duration`
        // doesn't re-open the file (#248). On `On`/`Off` modes we didn't probe,
        // so it's `None` and the helper does the work.
        VadDecision::Plain => {
            transcribe_plain(audio_path, &model_dir, duration, timestamps_required)
        }
        VadDecision::PlainWithHint => {
            let secs = duration.unwrap_or(0.0);
            eprintln!(
                "hint: audio is {secs:.0}s; `kesha install --vad` would improve long-audio accuracy"
            );
            transcribe_plain(audio_path, &model_dir, duration, timestamps_required)
        }
    }?;

    // --- Speaker diarization post-step ---
    #[cfg(all(feature = "system_diarize", target_os = "macos"))]
    {
        if speakers_required {
            let model_path = resolve_diarize_model_path()
                .context("speaker diarization requires a model path")?;
            let spans = diarize::run(std::path::Path::new(audio_path), &model_path)
                .context("speaker diarization failed")?;
            output.segments = diarize::merge_into(output.segments, &spans);
        }
    }
    #[cfg(not(all(feature = "system_diarize", target_os = "macos")))]
    {
        if speakers_required {
            anyhow::bail!(
                "speaker diarization is currently darwin-arm64 only.\n\
                 Tracked at https://github.com/drakulavich/kesha-voice-kit/issues/199.",
            );
        }
    }

    Ok(output)
}

fn transcribe_plain(
    audio_path: &str,
    model_dir: &str,
    duration: Option<f32>,
    timestamps_required: bool,
) -> Result<TranscriptionOutput> {
    let t0 = Instant::now();
    let mut be = backend::create_backend(model_dir)?;
    let dt_ms = t0.elapsed().as_millis() as u64;
    dtrace!("asr::backend_loaded dt={dt_ms}ms");
    dtrace_json!("asr.backend_loaded", { "dt_ms": dt_ms });
    let t1 = Instant::now();
    let text = be.transcribe(audio_path)?;
    dtrace!(
        "asr::transcribe.end dt={}ms chars={}",
        t1.elapsed().as_millis(),
        text.chars().count()
    );
    // Skip the duration probe for text-only callers AND for blank
    // transcripts — streaming Ogg/Opus (and a few other format edge cases)
    // return `Ok(None)` from `probe_duration_seconds`, which would surface
    // as a hard error for callers that don't need segments anyway, a
    // regression vs pre-#248 behavior.
    let segments = if timestamps_required && !text.trim().is_empty() {
        let dur = resolve_segment_duration(audio_path, duration)?;
        single_segment(0.0, dur, &text)
    } else {
        vec![]
    };
    Ok(TranscriptionOutput { segments, text })
}

/// VAD-preprocessed transcription: segment the audio with Silero VAD,
/// transcribe each speech span independently, stitch with spaces.
///
/// All-silence inputs fall back to a single full-file pass (with a stderr
/// warning) so a misconfigured threshold never silently drops input.
fn transcribe_via_vad(
    audio_path: &str,
    model_dir: &str,
    vad_dir: &str,
    cfg: VadConfig,
) -> Result<TranscriptionOutput> {
    if !models::is_cached_in(models::ModelKind::Vad, std::path::Path::new(vad_dir)) {
        anyhow::bail!(
            "Error: VAD model not installed\n\n\
             Please run: kesha install --vad"
        );
    }

    let t_audio = Instant::now();
    let samples = audio::load_audio(audio_path)?;
    dtrace!(
        "vad::audio_loaded dt={}ms samples={}",
        t_audio.elapsed().as_millis(),
        samples.len()
    );

    let t_vad = Instant::now();
    let vad_path = Path::new(vad_dir).join("silero_vad.onnx");
    let mut vad = VadDetector::load(&vad_path).context("load Silero VAD")?;
    let spans = vad.detect_segments(&samples, cfg)?;
    dtrace!(
        "vad::detect dt={}ms segments={}",
        t_vad.elapsed().as_millis(),
        spans.len()
    );

    let mut be = backend::create_backend(model_dir)?;

    if spans.is_empty() {
        let min_speech_samples =
            (cfg.min_speech_ms as u64 * VAD_SAMPLE_RATE as u64 / 1000) as usize;
        // #275 D7: surface the input duration + threshold the segmenter
        // saw when it concluded "no speech". With the prior `vad::detect
        // segments=0` line, this gives the user enough to tell "the audio
        // is actually silent" from "the threshold is too aggressive".
        let total_secs = samples.len() as f32 / VAD_SAMPLE_RATE as f32;
        dtrace!(
            "vad::all_silence total_secs={:.1} min_speech_ms={} threshold={:.2}",
            total_secs,
            cfg.min_speech_ms,
            cfg.threshold
        );
        if samples.len() >= min_speech_samples {
            eprintln!(
                "warning: VAD produced no speech segments; transcribing full file (consider lowering --vad threshold or skipping --vad)"
            );
        }
        let text = be.transcribe_samples(&samples)?;
        // Reuse the duration we already computed for the dtrace above
        // (Greptile follow-up on #282 — was a redundant float divide).
        return Ok(TranscriptionOutput {
            segments: single_segment(0.0, total_secs, &text),
            text,
        });
    }

    let output_segments =
        build_vad_output_segments(&spans, &samples, VAD_SAMPLE_RATE as f32, |slice| {
            be.transcribe_samples(slice)
        });

    let text = output_segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    Ok(TranscriptionOutput {
        text,
        segments: output_segments,
    })
}

/// Build per-span [`TranscriptionSegment`]s from a sequence of VAD spans.
/// Pure function: takes the spans + a transcribe callback so unit tests can
/// drive it without spinning up an ONNX model. Empty / failed spans are
/// dropped (with a stderr warning on failure) so the output preserves both
/// the monotonic-start invariant of the input and the `end > start` shape.
fn build_vad_output_segments<F>(
    spans: &[(f32, f32)],
    samples: &[f32],
    sr: f32,
    mut transcribe_span: F,
) -> Vec<TranscriptionSegment>
where
    F: FnMut(&[f32]) -> Result<String>,
{
    let mut out = Vec::with_capacity(spans.len());
    for &(start_s, end_s) in spans {
        let start = (start_s * sr) as usize;
        let end = ((end_s * sr) as usize).min(samples.len());
        if start >= end {
            continue;
        }
        let slice = &samples[start..end];
        let t = Instant::now();
        match transcribe_span(slice) {
            Ok(text) => {
                dtrace!(
                    "vad::segment dt={}ms range={:.2}-{:.2}s chars={}",
                    t.elapsed().as_millis(),
                    start_s,
                    end_s,
                    text.chars().count()
                );
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    out.push(TranscriptionSegment {
                        start: start_s,
                        end: end_s,
                        text: trimmed.to_string(),
                        speaker: None,
                    });
                }
            }
            Err(e) => {
                // One failing segment shouldn't kill the whole transcript.
                eprintln!(
                    "warning: VAD segment {:.2}-{:.2}s failed: {e}",
                    start_s, end_s
                );
            }
        }
    }
    out
}

/// Honor a caller-supplied audio duration if present; otherwise probe the
/// file. Re-uses the duration already computed during the `Auto` mode
/// probe-and-decide step (#248) so the plain path doesn't re-open the file.
/// Split from `transcribe_plain` so the probe-vs-hint contract can be
/// unit-tested without spinning up an ASR backend.
fn resolve_segment_duration(audio_path: &str, hint: Option<f32>) -> Result<f32> {
    if let Some(d) = hint {
        return Ok(d);
    }
    audio::probe_duration_seconds(audio_path)
        .with_context(|| {
            format!("failed to probe audio duration for timestamped segments: {audio_path}")
        })?
        .with_context(|| {
            format!("audio duration is unavailable for timestamped segments: {audio_path}")
        })
}

/// Construct a one-element `Vec<TranscriptionSegment>` (or empty if `text` is
/// blank after trimming). Shared by the plain-path's whole-file segment and
/// the VAD-fallback path in [`transcribe_via_vad`].
fn single_segment(start: f32, end: f32, text: &str) -> Vec<TranscriptionSegment> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return vec![];
    }
    vec![TranscriptionSegment {
        start,
        end,
        text: trimmed.to_string(),
        speaker: None,
    }]
}

/// Probe audio duration for the `Auto` decision, gated on a cheap
/// file-size floor. Files too small to plausibly be ≥ 120 s skip the
/// probe entirely. Probe failures log via `dtrace!` and return `None`
/// — the decode path will surface the real error, if any, shortly.
fn probe_duration_if_plausible(path: &str) -> Option<f32> {
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.len() < AUTO_VAD_MIN_FILE_SIZE {
            return None;
        }
    }
    match audio::probe_duration_seconds(path) {
        Ok(d) => d,
        Err(e) => {
            dtrace!("asr::probe_failed path={path} err={e}");
            None
        }
    }
}

/// Resolve the diarization model path. Priority:
/// 1. `KESHA_DIARIZE_MODEL_PATH` env var (must point to an existing path).
/// 2. Default cache location populated by `kesha install --diarize`
///    (`~/.cache/kesha/models/diarize/SortformerNvidiaLow_v2.mlpackage`).
#[cfg(all(feature = "system_diarize", target_os = "macos"))]
fn resolve_diarize_model_path() -> Result<std::path::PathBuf> {
    if let Ok(env_path) = std::env::var("KESHA_DIARIZE_MODEL_PATH") {
        let p = std::path::PathBuf::from(env_path);
        if p.exists() {
            return Ok(p);
        }
        anyhow::bail!(
            "KESHA_DIARIZE_MODEL_PATH set but path does not exist: {}",
            p.display()
        );
    }

    let default = crate::models::model_dir(crate::models::ModelKind::Diarize);
    if crate::models::is_cached(crate::models::ModelKind::Diarize) {
        return Ok(default);
    }

    anyhow::bail!(
        "diarization model not found at {}. \
         Run `kesha install --diarize` (or set KESHA_DIARIZE_MODEL_PATH).",
        default.display()
    )
}

/// Returns the cached ASR model dir or bails with the install hint.
fn ensure_asr_installed() -> Result<String> {
    if !models::is_cached(models::ModelKind::Asr) {
        anyhow::bail!(
            "Error: No transcription models installed\n\n\
             Please run: kesha install"
        );
    }
    Ok(models::model_dir(models::ModelKind::Asr)
        .to_string_lossy()
        .into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Write a 16-bit PCM mono WAV of `seconds` silence. Symphonia's probe
    /// reads `n_frames` from the WAV data-chunk size, so these files produce
    /// real durations without needing an audio-generation crate. The actual
    /// samples are zeros — a proper file, not a spoofed header, so the
    /// file-size guard in `probe_duration_if_plausible` sees real bytes.
    fn write_silent_pcm16_wav(
        path: &std::path::Path,
        seconds: u32,
        sample_rate: u32,
    ) -> std::io::Result<()> {
        let n_samples = (seconds as u64) * (sample_rate as u64);
        let data_bytes = (n_samples * 2) as u32;
        let mut f = std::fs::File::create(path)?;
        f.write_all(b"RIFF")?;
        f.write_all(&(36u32 + data_bytes).to_le_bytes())?;
        f.write_all(b"WAVE")?;
        f.write_all(b"fmt ")?;
        f.write_all(&16u32.to_le_bytes())?;
        f.write_all(&1u16.to_le_bytes())?;
        f.write_all(&1u16.to_le_bytes())?;
        f.write_all(&sample_rate.to_le_bytes())?;
        f.write_all(&(sample_rate * 2).to_le_bytes())?;
        f.write_all(&2u16.to_le_bytes())?;
        f.write_all(&16u16.to_le_bytes())?;
        f.write_all(b"data")?;
        f.write_all(&data_bytes.to_le_bytes())?;
        let zeros = vec![0u8; (n_samples * 2) as usize];
        f.write_all(&zeros)?;
        Ok(())
    }

    #[test]
    fn auto_mode_long_wav_routes_to_vad_when_installed() {
        // End-to-end test of the auto-trigger: 121-second WAV → probe reads
        // duration → `decide()` picks VAD (when installed) or hint path.
        let tmp = tempfile::Builder::new()
            .prefix("kesha-auto-vad-long-")
            .suffix(".wav")
            .tempfile()
            .unwrap();
        write_silent_pcm16_wav(tmp.path(), 121, 16_000).unwrap();
        let duration = probe_duration_if_plausible(tmp.path().to_str().unwrap());
        let secs = duration.expect("long WAV should probe to Some duration");
        assert!((120.0..122.0).contains(&secs), "expected ~121s, got {secs}");
        assert_eq!(
            decide(VadMode::Auto, duration, true),
            VadDecision::Vad,
            "long audio + installed → Vad"
        );
        assert_eq!(
            decide(VadMode::Auto, duration, false),
            VadDecision::PlainWithHint,
            "long audio + not installed → Plain + hint"
        );
    }

    #[test]
    fn probe_skipped_for_files_below_size_guard() {
        // 1 s of 16 kHz mono 16-bit PCM = ~32 KB, well below the 200 KB guard.
        // The probe should short-circuit and return None without touching
        // symphonia at all.
        let tmp = tempfile::Builder::new()
            .prefix("kesha-auto-vad-tiny-")
            .suffix(".wav")
            .tempfile()
            .unwrap();
        write_silent_pcm16_wav(tmp.path(), 1, 16_000).unwrap();
        assert!(std::fs::metadata(tmp.path()).unwrap().len() < AUTO_VAD_MIN_FILE_SIZE);
        assert_eq!(
            probe_duration_if_plausible(tmp.path().to_str().unwrap()),
            None,
        );
    }

    #[test]
    fn probe_returns_none_for_missing_or_invalid_file() {
        // Missing file → metadata fails, probe fails, returns None (decide
        // then treats as Auto/short → Plain).
        assert_eq!(
            probe_duration_if_plausible("/nonexistent/path/to/audio.wav"),
            None
        );
    }

    #[test]
    fn resolve_segment_duration_returns_hint_without_probing() {
        // When the caller already probed (Auto mode), the helper must NOT
        // re-open the file (#248). Pass a deliberately-unparseable file
        // alongside the hint — if the probe runs, it errors; if not, the
        // hint is returned verbatim.
        let tmp = tempfile::Builder::new()
            .prefix("kesha-no-probe-")
            .suffix(".raw")
            .tempfile()
            .unwrap();
        std::fs::write(tmp.path(), b"not an audio container").unwrap();
        let dur = resolve_segment_duration(tmp.path().to_str().unwrap(), Some(7.5)).unwrap();
        assert_eq!(dur, 7.5);
    }

    #[test]
    fn resolve_segment_duration_errors_when_no_hint_and_probe_fails() {
        let tmp = tempfile::Builder::new()
            .prefix("kesha-no-duration-")
            .suffix(".raw")
            .tempfile()
            .unwrap();
        std::fs::write(tmp.path(), b"not an audio container").unwrap();

        let err = resolve_segment_duration(tmp.path().to_str().unwrap(), None)
            .expect_err("timestamped output should require a known duration when no caller value");
        assert!(
            err.to_string()
                .contains("failed to probe audio duration for timestamped segments"),
            "{err}"
        );
    }

    #[test]
    fn single_segment_trims_text_and_drops_empty() {
        assert_eq!(single_segment(0.0, 1.0, "  hi  ")[0].text, "hi");
        assert!(single_segment(0.0, 1.0, "  ").is_empty());
        assert!(single_segment(0.0, 1.0, "").is_empty());
    }

    #[test]
    fn vad_output_segments_preserve_input_ordering_and_invariants() {
        // Lock the VAD-path contract: for a sorted span list, output must
        // satisfy `end > start` per segment and `start[i+1] >= start[i]`
        // across segments (monotonic). #248.
        let spans = vec![(0.5, 1.5), (2.0, 3.5), (4.0, 5.2)];
        let samples = vec![0.0_f32; 16_000 * 6];
        let mut call = 0;
        let segs = build_vad_output_segments(&spans, &samples, 16_000.0, |_slice| {
            call += 1;
            Ok(format!("utterance {call}"))
        });
        assert_eq!(segs.len(), 3);
        for s in &segs {
            assert!(s.end > s.start, "{s:?} violates end > start");
            assert!(!s.text.is_empty());
        }
        for w in segs.windows(2) {
            assert!(
                w[1].start >= w[0].start,
                "monotonic invariant violated: {w:?}"
            );
        }
    }

    #[test]
    fn vad_output_segments_skip_empty_transcriptions() {
        let spans = vec![(0.5, 1.5), (2.0, 3.5), (4.0, 5.2)];
        let samples = vec![0.0_f32; 16_000 * 6];
        let mut call = 0;
        let segs = build_vad_output_segments(&spans, &samples, 16_000.0, |_| {
            call += 1;
            if call == 2 {
                Ok(String::new())
            } else {
                Ok("hello".to_string())
            }
        });
        assert_eq!(segs.len(), 2, "empty transcription should be skipped");
        assert_eq!(segs[0].start, 0.5);
        assert_eq!(segs[1].start, 4.0);
    }

    #[test]
    fn vad_output_segments_skip_zero_length_spans() {
        // Spans where `start_s * sr >= samples_len` should be silently dropped
        // rather than panicking on the slice bounds.
        let spans = vec![(0.5, 0.5), (10.0, 11.0)];
        let samples = vec![0.0_f32; 16_000];
        let segs = build_vad_output_segments(&spans, &samples, 16_000.0, |_| Ok("ignore".into()));
        assert_eq!(segs.len(), 0);
    }

    #[test]
    fn from_flags_maps_cli_arguments_to_modes() {
        assert_eq!(VadMode::from_flags(true, false), VadMode::On);
        assert_eq!(VadMode::from_flags(false, true), VadMode::Off);
        assert_eq!(VadMode::from_flags(false, false), VadMode::Auto);
        // Should-be-unreachable (clap rejects), but resolve deterministically.
        assert_eq!(VadMode::from_flags(true, true), VadMode::On);
    }

    #[test]
    fn on_mode_always_uses_vad_regardless_of_other_inputs() {
        assert_eq!(decide(VadMode::On, None, false), VadDecision::Vad);
        assert_eq!(decide(VadMode::On, Some(5.0), false), VadDecision::Vad);
        assert_eq!(decide(VadMode::On, Some(300.0), true), VadDecision::Vad);
    }

    #[test]
    fn off_mode_always_uses_plain_regardless_of_other_inputs() {
        assert_eq!(decide(VadMode::Off, None, true), VadDecision::Plain);
        assert_eq!(decide(VadMode::Off, Some(3600.0), true), VadDecision::Plain);
    }

    #[test]
    fn auto_short_audio_uses_plain_with_no_hint() {
        assert_eq!(decide(VadMode::Auto, Some(30.0), true), VadDecision::Plain);
        assert_eq!(decide(VadMode::Auto, Some(119.9), true), VadDecision::Plain);
    }

    #[test]
    fn auto_long_audio_with_vad_installed_routes_through_vad() {
        assert_eq!(
            decide(VadMode::Auto, Some(AUTO_VAD_MIN_SECONDS), true),
            VadDecision::Vad
        );
        assert_eq!(decide(VadMode::Auto, Some(3600.0), true), VadDecision::Vad);
    }

    #[test]
    fn auto_long_audio_without_vad_prints_hint() {
        assert_eq!(
            decide(VadMode::Auto, Some(300.0), false),
            VadDecision::PlainWithHint
        );
    }

    #[test]
    fn auto_unknown_duration_skips_trigger_silently() {
        // Unknown duration → treat as short, never surprise the user with VAD.
        assert_eq!(decide(VadMode::Auto, None, true), VadDecision::Plain);
        assert_eq!(decide(VadMode::Auto, None, false), VadDecision::Plain);
    }

    #[test]
    fn transcription_segment_speaker_field_omits_when_none() {
        let s = TranscriptionSegment {
            start: 0.0,
            end: 1.0,
            text: "hi".into(),
            speaker: None,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(
            !json.contains("\"speaker\""),
            "speaker:None should be omitted, got {json}"
        );
    }

    #[test]
    fn transcription_segment_speaker_field_serializes_when_some() {
        let s = TranscriptionSegment {
            start: 0.0,
            end: 1.0,
            text: "hi".into(),
            speaker: Some(2),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(
            json.contains("\"speaker\":2"),
            "expected speaker:2 in {json}"
        );
    }

    #[test]
    fn vad_mode_default_is_auto() {
        // `TranscribeOptions::default()` only matches the legacy text-only
        // `transcribe(_, VadMode::Auto)` behavior if VadMode's own Default
        // is Auto. Lock that in.
        assert_eq!(VadMode::default(), VadMode::Auto);
    }

    #[test]
    fn transcribe_options_default_is_text_only_auto() {
        // The struct's default must match the legacy `transcribe(_, mode)`
        // text-only path: Auto VAD, no segments, no speakers. Anyone
        // building options inline can rely on `..Default::default()` for
        // the unmentioned fields without surprise.
        let o = TranscribeOptions::default();
        assert_eq!(o.mode, VadMode::Auto);
        assert!(!o.with_segments);
        assert!(!o.with_speakers);
    }

    #[test]
    fn transcribe_with_options_rejects_speakers_without_segments() {
        // Greptile P1 on #290: `{with_speakers: true, with_segments: false}`
        // would silently drop every speaker label on the plain path. The
        // guard rejects the combination before any model lookup or audio
        // probe, so a bogus `audio_path` is fine — we never get there.
        let opts = TranscribeOptions {
            mode: VadMode::Off,
            with_segments: false,
            with_speakers: true,
        };
        let err = transcribe_with_options("/dev/null", &opts)
            .expect_err("speakers-without-segments must error");
        let msg = format!("{err}");
        assert!(
            msg.contains("with_speakers requires with_segments"),
            "error message must explain the constraint, got: {msg}"
        );
    }
}
