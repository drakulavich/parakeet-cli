//! Text-to-speech façade.
//!
//! Per-engine pipelines live in sibling submodules (`kokoro`, `vosk`,
//! `avspeech`); shared text-processing helpers are split out by language
//! (`en`, `ru`, `g2p`, `tokenizer`). The dispatcher that routes a
//! [`SayOptions`] request across them lives in [`say`], re-exported here
//! so external callers continue to reach it as `crate::tts::say`.

use std::path::Path;

pub mod chatterbox;
pub mod en;
pub mod encode;
pub mod g2p;
pub mod kokoro;
pub mod ru;
pub mod say;
pub mod sessions;
pub mod ssml;
pub mod tokenizer;
pub mod voices;
pub mod vosk;
pub mod warn;
pub mod wav;

pub use encode::OutputFormat;
pub use say::{say, synth_segments_kokoro_with, synth_segments_vosk_with};

#[cfg(all(feature = "system_tts", target_os = "macos"))]
pub mod avspeech;

/// Soft limit on input text length. Rejects absurdly long inputs that would
/// spend minutes on synthesis with poor quality.
pub const MAX_TEXT_CHARS: usize = 5000;

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
    /// Chatterbox Multilingual ONNX: raw text + language tag + reference WAV.
    Chatterbox {
        model_dir: &'a Path,
        voice_path: &'a Path,
        lang: &'a str,
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
