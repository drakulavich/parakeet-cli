//! Text-to-speech dispatch across per-engine modules.

use std::borrow::Cow;
use std::path::Path;

pub mod en;
pub mod encode;
pub mod g2p;
pub mod kokoro;
pub mod ru;
pub mod sessions;
pub mod ssml;
pub mod tokenizer;
pub mod voices;
pub mod vosk;
pub mod warn;
pub mod wav;

pub use encode::OutputFormat;

#[cfg(all(feature = "system_tts", target_os = "macos"))]
pub mod avspeech;

/// Soft limit on input text length. Rejects absurdly long inputs that would
/// spend minutes on synthesis with poor quality.
pub const MAX_TEXT_CHARS: usize = 5000;

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

#[derive(Debug, thiserror::Error)]
pub enum TtsError {
    #[error("text is empty")]
    EmptyText,
    #[error("text exceeds {max} chars ({actual})")]
    TextTooLong { max: usize, actual: usize },
    #[error("synthesis failed: {0}")]
    SynthesisFailed(String),
}

/// Which TTS engine to run. Voice ids determine this via `voices::resolve_voice`.
pub enum EngineChoice<'a> {
    /// Kokoro-82M: separate model + per-voice style embedding + rate.
    Kokoro {
        model_path: &'a Path,
        voice_path: &'a Path,
        speed: f32,
    },
    /// macOS AVSpeechSynthesizer via the Swift sidecar (#141).
    /// `voice_id` is forwarded verbatim (an Apple identifier or a language code).
    #[cfg(all(feature = "system_tts", target_os = "macos"))]
    AVSpeech { voice_id: &'a str },
    /// Vosk-TTS Russian: model dir + speaker id (G2P happens inside vosk).
    Vosk {
        model_dir: &'a Path,
        speaker_id: u32,
        /// Speaking rate (1.0 = model default); passed to vosk's `speech_rate`.
        speed: f32,
    },
}

pub struct SayOptions<'a> {
    pub text: &'a str,
    /// espeak language code, e.g. `en-us`, `ru`.
    pub lang: &'a str,
    pub engine: EngineChoice<'a>,
    /// When true, `text` is parsed as SSML (issue #122). `<break>` tags yield
    /// silence of the declared duration; unknown tags are stripped with a warning.
    pub ssml: bool,
    /// Wire format for the returned bytes. Defaults to `Wav` so existing
    /// callers (and the historical `kesha say > out.wav` flow) stay
    /// bit-exact. See #223.
    pub format: OutputFormat,
    /// Auto-expand all-uppercase acronyms before synth: Cyrillic on `ru-vosk-*`
    /// (#232), Latin on `en-*` (#244). Default `true`. `<say-as interpret-as="characters">`
    /// is always honored regardless of this flag. No effect for `macos-*` voices.
    pub expand_abbrev: bool,
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
        EngineChoice::Vosk { .. } => "vosk",
        #[cfg(all(feature = "system_tts", target_os = "macos"))]
        EngineChoice::AVSpeech { .. } => "avspeech",
    };
    crate::dtrace!(
        "tts::say engine={engine_label} lang={} ssml={} chars={len}",
        opts.lang,
        opts.ssml
    );

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

    // Auto-expand English acronyms when the voice is en-* and the user
    // hasn't passed --no-expand-abbrev. <say-as> is honored on the SSML
    // path (synth_segments_kokoro) regardless. Closes #244 (mirror of #232).
    let normalized_text: Cow<'_, str> = if opts.expand_abbrev && opts.lang.starts_with("en") {
        Cow::Owned(en::expand_text(opts.text))
    } else {
        Cow::Borrowed(opts.text)
    };
    let ipa = g2p::text_to_ipa(normalized_text.as_ref(), opts.lang)
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
    let segments = if lang.starts_with("en") {
        en::normalize_segments(segments, expand_abbrev)
    } else {
        segments
    };
    let mut sess = sessions::KokoroSession::load(model_path)
        .map_err(|e| TtsError::SynthesisFailed(e.to_string()))?;
    synth_segments_kokoro_with(&mut sess, &segments, lang, voice_path, speed, format)
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
        match seg {
            // Spell: G2P-routed (Vosk path normalizes Spell→Text upstream of synth).
            ssml::Segment::Text(t) | ssml::Segment::Spell(t) => {
                let ipa = g2p::text_to_ipa(t, lang)
                    .map_err(|e| TtsError::SynthesisFailed(format!("g2p: {e}")))?;
                let audio = sess
                    .infer_ipa(&ipa, voice_path, speed)
                    .map_err(|e| TtsError::SynthesisFailed(format!("infer: {e}")))?;
                out.extend(audio);
            }
            ssml::Segment::Ipa(ph) => {
                let audio = sess
                    .infer_ipa(ph, voice_path, speed)
                    .map_err(|e| TtsError::SynthesisFailed(format!("infer: {e}")))?;
                out.extend(audio);
            }
            ssml::Segment::Break(dur) => out.extend(silence_samples(*dur, sample_rate)),
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
                let audio = sess
                    .infer_ipa(&ipa, voice_path, speed)
                    .map_err(|e| TtsError::SynthesisFailed(format!("infer: {e}")))?;
                out.extend(audio);
            }
        }
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
    let audio = sess
        .infer_ipa(ipa, voice_path, speed)
        .map_err(|e| TtsError::SynthesisFailed(format!("infer: {e}")))?;
    if audio.is_empty() {
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
        match seg {
            ssml::Segment::Text(t) | ssml::Segment::Ipa(t) | ssml::Segment::Spell(t) => {
                // Vosk path normalizes Spell→Text upstream; arm kept for match exhaustiveness.
                let (audio, _sr) = cache
                    .infer(model_dir, t, speaker_id, speed)
                    .map_err(|e| TtsError::SynthesisFailed(format!("vosk: {e}")))?;
                out.extend(audio);
            }
            ssml::Segment::Break(dur) => out.extend(silence_samples(*dur, sample_rate)),
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
                out.extend(audio);
            }
        }
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

/// Decode WAV bytes the AVSpeech sidecar handed back to PCM, then re-encode in
/// the caller's chosen format. WAV → WAV is a no-op short-circuit so we don't
/// pay a hound round-trip for the historical default path.
#[cfg(all(feature = "system_tts", target_os = "macos"))]
fn transcode_to(wav_bytes: &[u8], format: OutputFormat) -> Result<Vec<u8>, TtsError> {
    if matches!(format, OutputFormat::Wav) {
        return Ok(wav_bytes.to_vec());
    }
    let reader = hound::WavReader::new(std::io::Cursor::new(wav_bytes))
        .map_err(|e| TtsError::SynthesisFailed(format!("avspeech wav decode: {e}")))?;
    let spec = reader.spec();
    let samples = wav_to_mono_f32(reader)
        .map_err(|e| TtsError::SynthesisFailed(format!("avspeech wav decode: {e}")))?;
    encode_or_fail(&samples, spec.sample_rate, format)
}

/// Read all samples from a WAV reader, mixing stereo to mono and converting
/// integer PCM to f32. AVSpeech emits 22.05 kHz 16-bit mono on macOS today,
/// but we keep this generic so a future sidecar change doesn't break us.
#[cfg(all(feature = "system_tts", target_os = "macos"))]
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
