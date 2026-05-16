//! TTS dispatcher: route a [`SayOptions`] request to the right per-engine
//! pipeline (Kokoro / Vosk / AVSpeech), thread SSML segmentation through it,
//! and encode the result into the caller's chosen wire format.
//!
//! Public entry points re-exported from `tts/mod.rs`:
//! - [`say`] — one-shot synth-and-encode
//! - [`synth_segments_kokoro_with`] / [`synth_segments_vosk_with`] —
//!   drive an existing engine handle from a segment list (used by the
//!   `--stdin-loop` long-lived path, #213)

use std::borrow::Cow;
use std::path::Path;
use std::time::Instant;

use super::encode::OutputFormat;
use super::{
    en, encode, g2p, kokoro, ru, sessions, ssml, EngineChoice, SayOptions, TtsError, MAX_TEXT_CHARS,
};

#[cfg(all(feature = "system_tts", target_os = "macos"))]
use super::avspeech;

/// Per-`<break>` ceiling so a hostile SSML input can't allocate gigabytes of
/// silence. 30s × 24 kHz × 4 B ≈ 2.9 MB max per tag, easily affordable.
const MAX_BREAK_SECS: f64 = 30.0;

/// Build a zero-PCM silence buffer for an SSML `<break>`, capped at
/// [`MAX_BREAK_SECS`] regardless of declared duration.
fn silence_samples(dur: std::time::Duration, sample_rate: u32) -> Vec<f32> {
    let secs = dur.as_secs_f64().min(MAX_BREAK_SECS);
    let n = (secs * sample_rate as f64).round() as usize;
    vec![0.0_f32; n]
}

/// Saturating composition of the CLI `--rate` flag with an SSML
/// `<prosody rate>` multiplier. Both factors are unit-less multipliers
/// against the engine's default rate; the result is clamped to the
/// engine-safe range so downstream `Vosk::infer` / `Kokoro::infer` never
/// see a 0× or 10× rate that would render unintelligible audio.
///
/// Range pinned to `0.5..=2.0` per the #236 spike findings: both Vosk
/// (`vosk-model-tts-ru-0.9-multi`) and Kokoro (`kokoro-82M`) honor rate
/// within ~7% of theoretical at these endpoints; past them quality
/// degrades. Single source of truth for the clamp range — change here
/// and the two engine arms in `synth_one_*` pick it up.
///
/// Emits a `warn_once` to stderr the first time a clamp diverges from
/// the raw product — without that line, an SSML `rate="300%"` capped to
/// `2.0` looks indistinguishable from a clean 2× rate (#267 F9).
fn compose_rate(cli_rate: f32, ssml_rate: f32) -> f32 {
    let raw = cli_rate * ssml_rate;
    let clamped = raw.clamp(0.5, 2.0);
    // Exact bound check, not `(raw - clamped).abs() > EPSILON`: at raw≈0.5
    // the f32 ULP (~6e-8) is below `EPSILON` (~1.2e-7), so a value one ULP
    // outside the bound would clamp silently (Greptile P2 on #287). NaN
    // is unordered against any bound → `contains` returns false → the
    // warning DOES fire ("rate NaN ... clamped to NaN"). That's
    // intentional: NaN here means an upstream bug parsed `cli_rate` or
    // `ssml_rate` as not-a-number, and surfacing it on stderr beats
    // silently propagating NaN sample-rate params downstream.
    if !(0.5..=2.0).contains(&raw) {
        crate::tts::warn::warn_once(
            "compose-rate-clamped",
            &format!(
                "rate {raw:.2} (cli={cli_rate:.2} × ssml={ssml_rate:.2}) \
                 clamped to {clamped:.2} (engine-safe range 0.5..=2.0)"
            ),
        );
    }
    clamped
}

/// Synthesize speech and return WAV bytes (mono float32; sample rate depends on engine).
///
/// Loads the ONNX session fresh on each call (~100-800ms). Fine for one-shot CLI
/// usage; callers that synthesize in a loop should hold a [`kokoro::Kokoro`] or
/// [`vosk::Vosk`] instance and drive it via its `infer` method.
pub fn say(opts: SayOptions) -> Result<Vec<u8>, TtsError> {
    if opts.text.is_empty() {
        return Err(TtsError::EmptyText);
    }
    let len = opts.text.chars().count();
    if len > MAX_TEXT_CHARS {
        return Err(TtsError::TextTooLong {
            max: MAX_TEXT_CHARS,
            actual: len,
        });
    }
    let engine_label: &str = match &opts.engine {
        EngineChoice::Kokoro { .. } => "kokoro",
        #[cfg(all(
            feature = "system_kokoro",
            target_os = "macos",
            target_arch = "aarch64"
        ))]
        EngineChoice::FluidKokoro { .. } => "fluid-kokoro",
        EngineChoice::Vosk { .. } => "vosk",
        #[cfg(all(feature = "system_tts", target_os = "macos"))]
        EngineChoice::AVSpeech { .. } => "avspeech",
    };
    crate::dtrace!(
        "tts::say engine={engine_label} lang={} ssml={} chars={len}",
        opts.lang,
        opts.ssml
    );

    #[cfg(all(
        feature = "system_kokoro",
        target_os = "macos",
        target_arch = "aarch64"
    ))]
    if let EngineChoice::FluidKokoro { voice_id, speed } = &opts.engine {
        if opts.ssml {
            return Err(TtsError::SynthesisFailed(
                "SSML is not yet supported with FluidAudio Kokoro voices".into(),
            ));
        }
        let wav_bytes = super::fluid_kokoro::synthesize(opts.text, voice_id, *speed, None)
            .map_err(|e| TtsError::SynthesisFailed(format!("fluid-kokoro: {e}")))?;
        return transcode_to(&wav_bytes, opts.format);
    }

    // AVSpeech does its own G2P + synthesis inside Swift; skip espeak G2P entirely.
    #[cfg(all(feature = "system_tts", target_os = "macos"))]
    if let EngineChoice::AVSpeech { voice_id } = &opts.engine {
        if opts.ssml {
            return Err(TtsError::SynthesisFailed(
                "SSML is not yet supported with macos-* voices (#141 follow-up)".into(),
            ));
        }
        let wav_bytes = avspeech::synthesize(opts.text, voice_id, None)
            .map_err(|e| TtsError::SynthesisFailed(format!("avspeech: {e}")))?;
        // The Swift sidecar always returns WAV. For non-WAV `--format`, decode
        // back to PCM and re-encode — cheap (a few hundred ms of audio) and
        // keeps the encoder pipeline single-pathed.
        return transcode_to(&wav_bytes, opts.format);
    }

    // Vosk-tts owns its own G2P + text normalisation; bypass our espeak/misaki path.
    if let EngineChoice::Vosk {
        model_dir,
        speaker_id,
        speed,
    } = &opts.engine
    {
        if opts.ssml {
            return synth_segments_vosk(
                opts.text,
                model_dir,
                *speaker_id,
                *speed,
                opts.format,
                opts.expand_abbrev,
            );
        }
        return say_with_vosk(
            opts.text,
            model_dir,
            *speaker_id,
            *speed,
            opts.format,
            opts.expand_abbrev,
        );
    }

    if opts.ssml {
        return say_ssml(&opts);
    }

    // English on Kokoro: route plain text through the segment pipeline so
    // IPA_LEXICON overrides (EPAM, JSON, Anthropic, Microsoft, …) emit
    // `Segment::Ipa` and bypass G2P. Letter-spell rule + STOP_LIST run inside
    // `en::normalize_segments`. Closes #244.
    if let EngineChoice::Kokoro {
        model_path,
        voice_path,
        speed,
    } = &opts.engine
    {
        if en::is_en(opts.lang) {
            return synth_segments_kokoro(
                vec![ssml::Segment::Text(opts.text.to_string())],
                opts.lang,
                model_path,
                voice_path,
                *speed,
                opts.format,
                opts.expand_abbrev,
            );
        }
    }

    // Non-English Kokoro: legacy G2P + say_with_kokoro path.
    let ipa = g2p::text_to_ipa(opts.text, opts.lang)
        .map_err(|e| TtsError::SynthesisFailed(format!("g2p: {e}")))?;
    if ipa.trim().is_empty() {
        return Err(TtsError::SynthesisFailed(
            "no phonemes produced for input (empty after G2P)".into(),
        ));
    }

    match opts.engine {
        EngineChoice::Kokoro {
            model_path,
            voice_path,
            speed,
        } => say_with_kokoro(&ipa, model_path, voice_path, speed, opts.format),
        // Vosk and AVSpeech are handled by early-returns above. Keep guard arms
        // so the match stays exhaustive when those features are enabled.
        #[cfg(all(
            feature = "system_kokoro",
            target_os = "macos",
            target_arch = "aarch64"
        ))]
        EngineChoice::FluidKokoro { .. } => unreachable!("handled by early return above"),
        EngineChoice::Vosk { .. } => unreachable!("handled by early return above"),
        #[cfg(all(feature = "system_tts", target_os = "macos"))]
        EngineChoice::AVSpeech { .. } => unreachable!("handled by early return above"),
    }
}

/// SSML path: parse, then synthesize each text segment through the engine (loaded once),
/// interleaving silence for `<break>` segments. Concatenate the f32 samples and wrap as WAV.
fn say_ssml(opts: &SayOptions) -> Result<Vec<u8>, TtsError> {
    let segments =
        ssml::parse(opts.text).map_err(|e| TtsError::SynthesisFailed(format!("ssml: {e}")))?;
    if segments.is_empty() {
        return Err(TtsError::SynthesisFailed(
            "SSML had no speakable content".into(),
        ));
    }

    match &opts.engine {
        EngineChoice::Kokoro {
            model_path,
            voice_path,
            speed,
        } => synth_segments_kokoro(
            segments,
            opts.lang,
            model_path,
            voice_path,
            *speed,
            opts.format,
            opts.expand_abbrev,
        ),
        #[cfg(all(
            feature = "system_kokoro",
            target_os = "macos",
            target_arch = "aarch64"
        ))]
        EngineChoice::FluidKokoro { .. } => {
            unreachable!("FluidAudio Kokoro + SSML rejected in say() early return")
        }
        // Vosk + SSML is handled by the early-return in say(); this arm keeps the match exhaustive.
        EngineChoice::Vosk { .. } => unreachable!("handled by early return in say()"),
        // AVSpeech + SSML is rejected up-front in `say()`; this arm keeps the match exhaustive.
        #[cfg(all(feature = "system_tts", target_os = "macos"))]
        EngineChoice::AVSpeech { .. } => {
            unreachable!("AVSpeech + SSML rejected in say() early return")
        }
    }
}

fn synth_segments_kokoro(
    segments: Vec<ssml::Segment>,
    lang: &str,
    model_path: &Path,
    voice_path: &Path,
    speed: f32,
    format: OutputFormat,
    expand_abbrev: bool,
) -> Result<Vec<u8>, TtsError> {
    // Run en::normalize_segments for en-* voices: maps Spell→Text via the
    // letter table, Text→acronym-expanded (when expand_abbrev), Emphasis→Text
    // with `+`-strip + warn-once. Mirror of synth_segments_vosk's call to
    // ru::normalize_segments. Closes #244.
    let segments = if en::is_en(lang) {
        en::normalize_segments(segments, expand_abbrev)
    } else {
        segments
    };
    let mut sess = sessions::KokoroSession::load(model_path)
        .map_err(|e| TtsError::SynthesisFailed(e.to_string()))?;
    synth_segments_kokoro_with(&mut sess, &segments, lang, voice_path, speed, format)
}

/// Synthesize a single SSML segment through Kokoro and return raw f32 samples.
/// `ProsodyRate` recursively calls this for each inner segment with the
/// multiplied+clamped rate; all other arms are leaf productions.
fn synth_one_kokoro(
    sess: &mut sessions::KokoroSession,
    seg: &ssml::Segment,
    lang: &str,
    voice_path: &Path,
    speed: f32,
) -> Result<Vec<f32>, TtsError> {
    let sample_rate = kokoro::SAMPLE_RATE;
    match seg {
        // Spell: G2P-routed (Vosk path normalizes Spell→Text upstream of synth).
        ssml::Segment::Text(t) | ssml::Segment::Spell(t) => {
            let ipa = g2p::text_to_ipa(t, lang)
                .map_err(|e| TtsError::SynthesisFailed(format!("g2p: {e}")))?;
            sess.infer_ipa(&ipa, voice_path, speed)
                .map_err(|e| TtsError::SynthesisFailed(format!("infer: {e}")))
        }
        ssml::Segment::Ipa(ph) => sess
            .infer_ipa(ph, voice_path, speed)
            .map_err(|e| TtsError::SynthesisFailed(format!("infer: {e}"))),
        ssml::Segment::Break(dur) => Ok(silence_samples(*dur, sample_rate)),
        ssml::Segment::Emphasis { content, suppress } => {
            // Defensive fallback: en::normalize_segments converts Emphasis→Text
            // upstream of synth_segments_kokoro_with's say_ssml caller
            // (synth_segments_kokoro). The arm remains for `--stdin-loop`
            // callers (#213) that bypass that wrapper and feed segments
            // directly. Mirrors synth_segments_vosk_with's Emphasis fallback.
            // Closes #238 (preserved); closes #244.
            if !suppress {
                crate::tts::warn::warn_once(
                    "emphasis-non-ru-vosk",
                    "<emphasis> stress markers are honored only on ru-vosk-* voices; \
                     stripping `+` from content for non-Vosk path",
                );
            }
            let stripped = if content.contains('+') {
                content.replace('+', "")
            } else {
                content.clone()
            };
            let ipa = g2p::text_to_ipa(&stripped, lang)
                .map_err(|e| TtsError::SynthesisFailed(format!("g2p: {e}")))?;
            sess.infer_ipa(&ipa, voice_path, speed)
                .map_err(|e| TtsError::SynthesisFailed(format!("infer: {e}")))
        }
        ssml::Segment::ProsodyRate { rate, content } => {
            let effective = compose_rate(speed, *rate);
            let mut samples = Vec::new();
            for inner in content {
                samples.extend(synth_one_kokoro(sess, inner, lang, voice_path, effective)?);
            }
            Ok(samples)
        }
    }
}

/// Drive an SSML segment list against an already-constructed Kokoro session.
/// Used by both the one-shot `tts::say()` SSML path and the long-lived
/// `--stdin-loop` (#213). Concatenates audio for `<break>` and text/IPA
/// segments, encodes once at the engine's native sample rate.
pub fn synth_segments_kokoro_with(
    sess: &mut sessions::KokoroSession,
    segments: &[ssml::Segment],
    lang: &str,
    voice_path: &Path,
    speed: f32,
    format: OutputFormat,
) -> Result<Vec<u8>, TtsError> {
    let sample_rate = kokoro::SAMPLE_RATE;
    let mut out: Vec<f32> = Vec::new();
    for seg in segments {
        out.extend(synth_one_kokoro(sess, seg, lang, voice_path, speed)?);
    }
    if out.is_empty() {
        return Err(TtsError::SynthesisFailed(
            "no audio produced from SSML input".into(),
        ));
    }
    encode_or_fail(&out, sample_rate, format)
}

fn say_with_kokoro(
    ipa: &str,
    model_path: &Path,
    voice_path: &Path,
    speed: f32,
    format: OutputFormat,
) -> Result<Vec<u8>, TtsError> {
    let mut sess = sessions::KokoroSession::load(model_path)
        .map_err(|e| TtsError::SynthesisFailed(e.to_string()))?;
    // #275 D1: boundary trace so a "no recognizable phonemes in input"
    // bail carries inputs (IPA length + voice file) and outputs (sample
    // count + wall time) instead of pointing at nothing.
    let ipa_len = ipa.chars().count();
    crate::dtrace!(
        "kokoro::infer.start ipa_len={ipa_len} voice={}",
        voice_path.display()
    );
    let t = Instant::now();
    let audio = sess
        .infer_ipa(ipa, voice_path, speed)
        .map_err(|e| TtsError::SynthesisFailed(format!("infer: {e}")))?;
    crate::dtrace!(
        "kokoro::infer.end samples={} dt={}ms",
        audio.len(),
        t.elapsed().as_millis()
    );
    if audio.is_empty() {
        crate::dtrace!(
            "kokoro::infer.empty ipa_first_20={:?}",
            ipa.chars().take(20).collect::<String>()
        );
        return Err(TtsError::SynthesisFailed(
            "no recognizable phonemes in input".into(),
        ));
    }
    encode_or_fail(&audio, kokoro::SAMPLE_RATE, format)
}

fn say_with_vosk(
    text: &str,
    model_dir: &Path,
    speaker_id: u32,
    speed: f32,
    format: OutputFormat,
    expand_abbrev: bool,
) -> Result<Vec<u8>, TtsError> {
    let normalized: Cow<'_, str> = if expand_abbrev {
        Cow::Owned(ru::expand_text(text))
    } else {
        Cow::Borrowed(text)
    };
    let mut cache = sessions::VoskCache::new();
    let (audio, sample_rate) = cache
        .infer(model_dir, normalized.as_ref(), speaker_id, speed)
        .map_err(|e| TtsError::SynthesisFailed(format!("vosk: {e}")))?;
    encode_or_fail(&audio, sample_rate, format)
}

fn synth_segments_vosk(
    text: &str,
    model_dir: &Path,
    speaker_id: u32,
    speed: f32,
    format: OutputFormat,
    expand_abbrev: bool,
) -> Result<Vec<u8>, TtsError> {
    let segments =
        ssml::parse(text).map_err(|e| TtsError::SynthesisFailed(format!("ssml: {e}")))?;
    if segments.is_empty() {
        return Err(TtsError::SynthesisFailed(
            "SSML had no speakable content".into(),
        ));
    }
    let segments = ru::normalize_segments(segments, expand_abbrev);
    let mut cache = sessions::VoskCache::new();
    synth_segments_vosk_with(&mut cache, &segments, model_dir, speaker_id, speed, format)
}

/// Synthesize a single SSML segment through Vosk and return raw f32 samples.
/// `ProsodyRate` recursively calls this for each inner segment with the
/// multiplied+clamped rate; all other arms are leaf productions.
fn synth_one_vosk(
    cache: &mut sessions::VoskCache,
    seg: &ssml::Segment,
    model_dir: &Path,
    speaker_id: u32,
    speed: f32,
    sample_rate: u32,
) -> Result<Vec<f32>, TtsError> {
    match seg {
        ssml::Segment::Text(t) | ssml::Segment::Ipa(t) | ssml::Segment::Spell(t) => {
            // Vosk path normalizes Spell→Text upstream; arm kept for match exhaustiveness.
            let (audio, _sr) = cache
                .infer(model_dir, t, speaker_id, speed)
                .map_err(|e| TtsError::SynthesisFailed(format!("vosk: {e}")))?;
            Ok(audio)
        }
        ssml::Segment::Break(dur) => Ok(silence_samples(*dur, sample_rate)),
        ssml::Segment::Emphasis { content, suppress } => {
            // Defensive fallback: ru::normalize_segments converts Emphasis→Text upstream.
            // Skip the warning when suppress=true: the caller used level="none"
            // to explicitly opt out of stress markers — the warning would be
            // misleading. Closes #238.
            if !suppress {
                crate::tts::warn::warn_once(
                    "emphasis-non-ru-vosk",
                    "<emphasis> reached the Vosk synth without ru::normalize_segments \
                     preprocessing; stripping `+` markers as a fallback",
                );
            }
            let stripped = if content.contains('+') {
                content.replace('+', "")
            } else {
                content.clone()
            };
            let (audio, _sr) = cache
                .infer(model_dir, &stripped, speaker_id, speed)
                .map_err(|e| TtsError::SynthesisFailed(format!("vosk: {e}")))?;
            Ok(audio)
        }
        ssml::Segment::ProsodyRate { rate, content } => {
            let effective = compose_rate(speed, *rate);
            let mut samples = Vec::new();
            for inner in content {
                samples.extend(synth_one_vosk(
                    cache,
                    inner,
                    model_dir,
                    speaker_id,
                    effective,
                    sample_rate,
                )?);
            }
            Ok(samples)
        }
    }
}

/// Drive an SSML segment list against a Vosk cache. Mirrors
/// [`synth_segments_kokoro_with`]. The model is loaded once via
/// `cache.sample_rate()` so a leading `<break>` can size its silence buffer
/// correctly.
pub fn synth_segments_vosk_with(
    cache: &mut sessions::VoskCache,
    segments: &[ssml::Segment],
    model_dir: &Path,
    speaker_id: u32,
    speed: f32,
    format: OutputFormat,
) -> Result<Vec<u8>, TtsError> {
    let sample_rate = cache
        .sample_rate(model_dir)
        .map_err(|e| TtsError::SynthesisFailed(format!("vosk: {e}")))?;
    let mut out: Vec<f32> = Vec::new();
    for seg in segments {
        out.extend(synth_one_vosk(
            cache,
            seg,
            model_dir,
            speaker_id,
            speed,
            sample_rate,
        )?);
    }
    if out.is_empty() {
        return Err(TtsError::SynthesisFailed(
            "no audio produced from SSML input".into(),
        ));
    }
    encode_or_fail(&out, sample_rate, format)
}

/// Common tail: PCM samples → chosen wire format. Centralised so every engine
/// path emits the same error shape when encoding fails (#223).
fn encode_or_fail(
    samples: &[f32],
    sample_rate: u32,
    format: OutputFormat,
) -> Result<Vec<u8>, TtsError> {
    encode::encode(samples, sample_rate, format)
        .map_err(|e| TtsError::SynthesisFailed(format!("encode: {e}")))
}

/// Decode WAV bytes a Swift sidecar handed back to PCM, then re-encode in the
/// caller's chosen format. WAV → WAV is a no-op short-circuit so we don't pay a
/// hound round-trip for the historical default path.
#[cfg(any(
    all(feature = "system_tts", target_os = "macos"),
    all(
        feature = "system_kokoro",
        target_os = "macos",
        target_arch = "aarch64"
    )
))]
fn transcode_to(wav_bytes: &[u8], format: OutputFormat) -> Result<Vec<u8>, TtsError> {
    if matches!(format, OutputFormat::Wav) {
        return Ok(wav_bytes.to_vec());
    }
    let reader = hound::WavReader::new(std::io::Cursor::new(wav_bytes))
        .map_err(|e| TtsError::SynthesisFailed(format!("sidecar wav decode: {e}")))?;
    let spec = reader.spec();
    let samples = wav_to_mono_f32(reader)
        .map_err(|e| TtsError::SynthesisFailed(format!("sidecar wav decode: {e}")))?;
    encode_or_fail(&samples, spec.sample_rate, format)
}

/// Read all samples from a WAV reader, mixing stereo to mono and converting
/// integer PCM to f32. AVSpeech emits 22.05 kHz 16-bit mono on macOS today,
/// but we keep this generic so a future sidecar change doesn't break us.
#[cfg(any(
    all(feature = "system_tts", target_os = "macos"),
    all(
        feature = "system_kokoro",
        target_os = "macos",
        target_arch = "aarch64"
    )
))]
fn wav_to_mono_f32<R: std::io::Read>(mut reader: hound::WavReader<R>) -> anyhow::Result<Vec<f32>> {
    let spec = reader.spec();
    let channels = spec.channels as usize;
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<Vec<f32>, _>>()?,
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / max))
                .collect::<Result<Vec<f32>, _>>()?
        }
    };
    if channels == 1 {
        return Ok(samples);
    }
    Ok(samples
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect())
}

#[cfg(test)]
mod tests {
    #[test]
    fn prosody_rate_multiplies_and_clamps() {
        let cases = [
            (1.0_f32, 1.0_f32, 1.0_f32), // identity
            (0.8, 0.75, 0.6),            // 0.8 × 0.75 = 0.6, within range
            (0.5, 0.5, 0.5),             // 0.25 → clamped up to 0.5
            (2.0, 2.0, 2.0),             // 4.0 → clamped down to 2.0
            (1.0, 0.5, 0.5),             // identity × x-slow
            (1.0, 1.5, 1.5),             // identity × x-fast
        ];
        for (cli, ssml, expected) in cases {
            let effective = super::compose_rate(cli, ssml);
            assert!(
                (effective - expected).abs() < 1e-6,
                "cli={cli}, ssml={ssml}: got {effective}, expected {expected}"
            );
        }
    }

    #[test]
    fn compose_rate_warns_once_on_clamp() {
        // F9: clamping must surface a stderr warn so a user passing
        // SSML rate="300%" learns it was capped. Subsequent clamps reuse
        // the same key — process-wide warn_once dedupes.
        let _ = super::compose_rate(2.0, 2.0); // 4.0 → 2.0 (clamp high)
        assert!(
            crate::tts::warn::was_warned("compose-rate-clamped"),
            "compose_rate must record the warn key when clamping"
        );
        // Idempotent: a second clamp doesn't change set membership.
        let _ = super::compose_rate(0.1, 0.1); // 0.01 → 0.5 (clamp low)
        assert!(crate::tts::warn::was_warned("compose-rate-clamped"));
    }

    #[test]
    fn compose_rate_in_range_does_not_warn() {
        // The `0.5..=2.0` range is honored exactly — values just inside
        // the bounds must NOT trigger the clamp warning. (Outside-the-
        // bound coverage is exercised by `compose_rate_warns_once_on_clamp`
        // above; we can't assert "warn key absent" portably because the
        // warn set persists across tests in this `cargo test --lib` proc.)
        assert!((super::compose_rate(0.5, 1.0) - 0.5).abs() < 1e-6);
        assert!((super::compose_rate(2.0, 1.0) - 2.0).abs() < 1e-6);
        assert!((super::compose_rate(1.0, 1.0) - 1.0).abs() < 1e-6);
    }
}
