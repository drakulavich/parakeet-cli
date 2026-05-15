//! End-to-end TTS: real model, real voice, ONNX G2P → produces real WAV bytes.
//! Gated on engine-specific env vars so default CI without models stays fast.

#![cfg(feature = "tts")]

mod common;

use std::path::Path;

use kesha_engine::tts::{self, EngineChoice, OutputFormat, SayOptions, TtsError};

#[test]
fn kokoro_hello_world_produces_wav() {
    let Some((model, voice)) = common::kokoro_paths_or_skip() else {
        eprintln!("skipping: set KOKORO_MODEL + KOKORO_VOICE");
        return;
    };
    let wav = tts::say(SayOptions {
        text: "Hello, world",
        lang: "en-us",
        engine: EngineChoice::Kokoro {
            model_path: Path::new(&model),
            voice_path: Path::new(&voice),
            speed: 1.0,
        },
        ssml: false,
        format: OutputFormat::Wav,
        expand_abbrev: true,
    })
    .unwrap();
    assert_eq!(&wav[..4], b"RIFF", "not a WAV");
    assert!(
        wav.len() > 44 + 1000 * 4,
        "audio too short: {} bytes",
        wav.len()
    );
}

#[test]
fn empty_text_errors() {
    let res = tts::say(SayOptions {
        text: "",
        lang: "en-us",
        engine: EngineChoice::Kokoro {
            model_path: Path::new("/nonexistent"),
            voice_path: Path::new("/nonexistent"),
            speed: 1.0,
        },
        ssml: false,
        format: OutputFormat::Wav,
        expand_abbrev: true,
    });
    assert!(matches!(res, Err(TtsError::EmptyText)));
}

#[test]
fn too_long_errors() {
    let huge = "a".repeat(10_000);
    let res = tts::say(SayOptions {
        text: &huge,
        lang: "en-us",
        engine: EngineChoice::Kokoro {
            model_path: Path::new("/nonexistent"),
            voice_path: Path::new("/nonexistent"),
            speed: 1.0,
        },
        ssml: false,
        format: OutputFormat::Wav,
        expand_abbrev: true,
    });
    assert!(matches!(res, Err(TtsError::TextTooLong { .. })));
}

#[test]
fn kokoro_ssml_with_break_produces_wav() {
    let Some((model, voice)) = common::kokoro_paths_or_skip() else {
        eprintln!("skipping: set KOKORO_MODEL + KOKORO_VOICE");
        return;
    };
    let wav = tts::say(SayOptions {
        text: r#"<speak>Hello <break time="300ms"/> world</speak>"#,
        lang: "en-us",
        engine: EngineChoice::Kokoro {
            model_path: Path::new(&model),
            voice_path: Path::new(&voice),
            speed: 1.0,
        },
        ssml: true,
        format: OutputFormat::Wav,
        expand_abbrev: true,
    })
    .unwrap();
    assert_eq!(&wav[..4], b"RIFF");
    // Must be at least the audio for "Hello" + 300ms of silence + "world".
    // ~300ms @ 24kHz mono f32 = 28.8 KB just in silence.
    assert!(
        wav.len() > 44 + 24_000,
        "audio too short: {} bytes",
        wav.len()
    );
}

#[test]
fn ssml_input_without_speak_root_errors() {
    let res = tts::say(SayOptions {
        text: "plain text, not SSML",
        lang: "en-us",
        engine: EngineChoice::Kokoro {
            model_path: Path::new("/nonexistent"),
            voice_path: Path::new("/nonexistent"),
            speed: 1.0,
        },
        ssml: true,
        format: OutputFormat::Wav,
        expand_abbrev: true,
    });
    assert!(matches!(res, Err(TtsError::SynthesisFailed(_))));
}
