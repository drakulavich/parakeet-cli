use anyhow::{Context, Result};
use serde::Serialize;
use std::path::Path;
use std::time::Instant;

use crate::audio;
use crate::backend;
use crate::dtrace;
use crate::models;
use crate::vad::{VadConfig, VadDetector, SAMPLE_RATE as VAD_SAMPLE_RATE};

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

#[derive(Debug, Clone, Serialize)]
pub struct TranscriptionSegment {
    pub start: f32,
    pub end: f32,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TranscriptionOutput {
    pub text: String,
    pub segments: Vec<TranscriptionSegment>,
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

pub fn transcribe(audio_path: &str, mode: VadMode) -> Result<String> {
    Ok(transcribe_output(audio_path, mode)?.text)
}

pub fn transcribe_output(audio_path: &str, mode: VadMode) -> Result<TranscriptionOutput> {
    let model_dir = ensure_asr_installed()?;
    let vad_dir = models::vad_model_dir();
    let vad_installed = models::is_vad_cached(&vad_dir);

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

    match decision {
        VadDecision::Vad => {
            transcribe_via_vad(audio_path, &model_dir, &vad_dir, VadConfig::default())
        }
        VadDecision::Plain => transcribe_plain(audio_path, &model_dir),
        VadDecision::PlainWithHint => {
            let secs = duration.unwrap_or(0.0);
            eprintln!(
                "hint: audio is {secs:.0}s; `kesha install --vad` would improve long-audio accuracy"
            );
            transcribe_plain(audio_path, &model_dir)
        }
    }
}

fn transcribe_plain(audio_path: &str, model_dir: &str) -> Result<TranscriptionOutput> {
    let t0 = Instant::now();
    let mut be = backend::create_backend(model_dir)?;
    dtrace!("asr::backend_loaded dt={}ms", t0.elapsed().as_millis());
    let t1 = Instant::now();
    let text = be.transcribe(audio_path)?;
    dtrace!(
        "asr::transcribe.end dt={}ms chars={}",
        t1.elapsed().as_millis(),
        text.chars().count()
    );
    Ok(TranscriptionOutput {
        segments: whole_file_segment(audio_path, &text),
        text,
    })
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
    if !models::is_vad_cached(vad_dir) {
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
    let segments = vad.detect_segments(&samples, cfg)?;
    dtrace!(
        "vad::detect dt={}ms segments={}",
        t_vad.elapsed().as_millis(),
        segments.len()
    );

    let mut be = backend::create_backend(model_dir)?;

    if segments.is_empty() {
        let min_speech_samples =
            (cfg.min_speech_ms as u64 * VAD_SAMPLE_RATE as u64 / 1000) as usize;
        if samples.len() >= min_speech_samples {
            eprintln!(
                "warning: VAD produced no speech segments; transcribing full file (consider lowering --vad threshold or skipping --vad)"
            );
        }
        let text = be.transcribe_samples(&samples)?;
        return Ok(TranscriptionOutput {
            segments: sample_span_segment(0.0, samples.len(), &text),
            text,
        });
    }

    let sr = VAD_SAMPLE_RATE as f32;
    let mut output_segments: Vec<TranscriptionSegment> = Vec::with_capacity(segments.len());
    for (start_s, end_s) in &segments {
        let start = (*start_s * sr) as usize;
        let end = ((*end_s * sr) as usize).min(samples.len());
        if start >= end {
            continue;
        }
        let slice = &samples[start..end];
        let t = Instant::now();
        match be.transcribe_samples(slice) {
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
                    output_segments.push(TranscriptionSegment {
                        start: *start_s,
                        end: *end_s,
                        text: trimmed.to_string(),
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

fn whole_file_segment(audio_path: &str, text: &str) -> Vec<TranscriptionSegment> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return vec![];
    }
    let Ok(Some(duration)) = audio::probe_duration_seconds(audio_path) else {
        return vec![];
    };
    vec![TranscriptionSegment {
        start: 0.0,
        end: duration,
        text: trimmed.to_string(),
    }]
}

fn sample_span_segment(start_s: f32, sample_count: usize, text: &str) -> Vec<TranscriptionSegment> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return vec![];
    }
    vec![TranscriptionSegment {
        start: start_s,
        end: start_s + sample_count as f32 / VAD_SAMPLE_RATE as f32,
        text: trimmed.to_string(),
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

/// Returns the cached ASR model dir or bails with the install hint.
fn ensure_asr_installed() -> Result<String> {
    let model_dir = models::asr_model_dir();
    if !models::is_asr_cached(&model_dir) {
        anyhow::bail!(
            "Error: No transcription models installed\n\n\
             Please run: kesha install"
        );
    }
    Ok(model_dir)
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
}
