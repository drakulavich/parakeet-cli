use anyhow::Result;
use std::path::PathBuf;

use crate::{models, say_loop, tts};

pub struct SayArgs {
    pub text: Option<String>,
    pub voice: Option<String>,
    pub lang: Option<String>,
    pub out: Option<PathBuf>,
    pub rate: f32,
    pub list_voices: bool,
    pub ssml: bool,
    pub format: Option<String>,
    pub bitrate: Option<i32>,
    pub sample_rate: Option<u32>,
    pub model: Option<PathBuf>,
    pub voice_file: Option<PathBuf>,
    pub stdin_loop: bool,
    pub no_expand_abbrev: bool,
}

/// Resolve the user-supplied `--format` / `--bitrate` / `--sample-rate` /
/// `--out` combination into a single [`tts::OutputFormat`]. Mirrors the UX
/// table from #223:
///
/// 1. If `--format` is given, parse it (`wav` | `ogg-opus`).
/// 2. Otherwise, sniff the `--out` extension (`.wav` → wav, `.ogg`/`.opus`
///    → ogg-opus).
/// 3. Otherwise default to `Wav` — preserves the historical `kesha say > x`
///    behaviour where stdout was always RIFF.
///
/// `--bitrate` / `--sample-rate` only matter for opus and override the
/// defaults. When the user picked WAV but supplied either flag, we surface a
/// clear error rather than silently dropping them.
pub(crate) fn resolve_output_format(
    format: Option<&str>,
    bitrate: Option<i32>,
    sample_rate: Option<u32>,
    out: Option<&std::path::Path>,
) -> Result<tts::OutputFormat, String> {
    use std::str::FromStr;

    // #275 D10: track which arm of the resolver fired so the dtrace at
    // the bottom can surface `chosen=… source=…`. Three possible sources:
    //   "--format"  — explicit flag wins.
    //   "out-ext"   — sniffed from the `--out` extension.
    //   "default"   — fallthrough (no flag, no `--out`, or unknown ext).
    let (mut chosen, source): (tts::OutputFormat, &'static str) = match (format, out) {
        (Some(f), _) => (tts::OutputFormat::from_str(f)?, "--format"),
        (None, Some(p)) => {
            let ext_fmt = p
                .extension()
                .and_then(|e| e.to_str())
                .and_then(tts::encode::format_from_extension);
            match ext_fmt {
                Some(fmt) => (fmt, "out-ext"),
                None => (tts::OutputFormat::default(), "default"),
            }
        }
        (None, None) => (tts::OutputFormat::default(), "default"),
    };

    if let tts::OutputFormat::OggOpus {
        bitrate: ref mut br,
        sample_rate: ref mut sr,
    } = chosen
    {
        if let Some(b) = bitrate {
            *br = b;
        }
        if let Some(r) = sample_rate {
            *sr = r;
        }
    } else if matches!(chosen, tts::OutputFormat::Wav)
        && (bitrate.is_some() || sample_rate.is_some())
    {
        return Err("--bitrate / --sample-rate only apply to --format ogg-opus".to_string());
    }

    crate::dtrace!("format::resolved chosen={chosen:?} source={source}");
    Ok(chosen)
}

fn list_kokoro_voices(cache: &std::path::Path) -> Vec<String> {
    let dir = cache.join("models/kokoro-82m/voices");
    std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("bin") {
                p.file_stem().map(|s| format!("en-{}", s.to_string_lossy()))
            } else {
                None
            }
        })
        .collect()
}

fn list_vosk_ru_voices(cache: &std::path::Path) -> Vec<String> {
    // Vosk-TTS Russian is a single multi-speaker model — once installed, all
    // five baked-in speakers are available. Same gate as resolve_vosk_ru, so
    // partial installs don't advertise voices that fail at synthesis time.
    let dir = models::model_dir_at(models::ModelKind::VoskRu, cache);
    if !models::is_cached_in(models::ModelKind::VoskRu, &dir) {
        return Vec::new();
    }
    vec![
        "ru-vosk-f01".into(),
        "ru-vosk-f02".into(),
        "ru-vosk-f03".into(),
        "ru-vosk-m01".into(),
        "ru-vosk-m02".into(),
    ]
}

fn list_chatterbox_voices(cache: &std::path::Path) -> Vec<String> {
    let dir = models::model_dir_at(models::ModelKind::Chatterbox, cache);
    if !models::is_cached_in(models::ModelKind::Chatterbox, &dir) {
        return Vec::new();
    }
    tts::chatterbox::SUPPORTED_LANGS
        .iter()
        .map(|lang| format!("{lang}-{}", tts::voices::CHATTERBOX_DEFAULT_VOICE))
        .collect()
}

/// Map a TTS error to the documented exit code for `kesha say`.
/// 2 = bad input, 4 = synthesis failure, 5 = text too long.
/// (Voice-not-installed exits 1 directly from the resolver path.)
fn exit_code_for_tts_err(e: &tts::TtsError) -> i32 {
    match e {
        tts::TtsError::EmptyText => 2,
        tts::TtsError::TextTooLong { .. } => 5,
        tts::TtsError::SynthesisFailed(_) => 4,
    }
}

pub fn run(a: SayArgs) -> i32 {
    use std::io::{Read, Write};

    if a.list_voices {
        let cache = models::cache_dir();
        let mut voice_ids: Vec<String> = list_kokoro_voices(&cache)
            .into_iter()
            .chain(list_chatterbox_voices(&cache))
            .chain(list_vosk_ru_voices(&cache))
            .collect();
        // macos-* voices live in the OS, not the cache — enumerate them via
        // the AVSpeech helper (#141). Best-effort: if the helper is absent or
        // errors out, we still show Kokoro/Vosk voices.
        #[cfg(all(feature = "system_tts", target_os = "macos"))]
        voice_ids.extend(tts::avspeech::list_voices(None));
        voice_ids.sort();
        if voice_ids.is_empty() {
            println!("No voices installed. Run: kesha install --tts");
        } else {
            for id in voice_ids {
                println!("{id}");
            }
        }
        return 0;
    }

    if a.stdin_loop {
        return say_loop::run();
    }

    let text_joined = match a.text {
        Some(s) => s,
        None => {
            let mut buf = String::new();
            if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
                eprintln!("error: failed to read stdin: {e}");
                return 4;
            }
            buf.trim().to_string()
        }
    };

    // `--model` + `--voice-file` are Kokoro-specific testing overrides.
    // Pinned model/voice paths bypass the cache lookup.
    let resolved = match (a.model, a.voice_file) {
        (Some(model_path), Some(voice_path)) => tts::voices::ResolvedVoice::Kokoro {
            model_path,
            voice_path,
            espeak_lang: "en-us",
        },
        (Some(_), None) | (None, Some(_)) => {
            eprintln!("error: pass both --model and --voice-file or neither");
            return 2;
        }
        (None, None) => {
            let id = a.voice.as_deref().unwrap_or(tts::voices::DEFAULT_VOICE_ID);
            match tts::voices::resolve_voice(&models::cache_dir(), id) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("error: {e}");
                    return 1;
                }
            }
        }
    };

    let espeak_lang = a
        .lang
        .clone()
        .unwrap_or_else(|| resolved.espeak_lang().to_string());
    let engine = match &resolved {
        tts::voices::ResolvedVoice::Kokoro {
            model_path,
            voice_path,
            ..
        } => tts::EngineChoice::Kokoro {
            model_path,
            voice_path,
            speed: a.rate,
        },
        tts::voices::ResolvedVoice::Vosk {
            model_dir,
            speaker_id,
        } => tts::EngineChoice::Vosk {
            model_dir,
            speaker_id: *speaker_id,
            speed: a.rate,
        },
        tts::voices::ResolvedVoice::Chatterbox {
            model_dir,
            voice_path,
            lang: _,
        } => tts::EngineChoice::Chatterbox {
            model_dir,
            voice_path,
            lang: &espeak_lang,
        },
        #[cfg(all(feature = "system_tts", target_os = "macos"))]
        tts::voices::ResolvedVoice::AVSpeech { voice_id } => {
            tts::EngineChoice::AVSpeech { voice_id }
        }
    };

    let format = match resolve_output_format(
        a.format.as_deref(),
        a.bitrate,
        a.sample_rate,
        a.out.as_deref(),
    ) {
        Ok(f) => f,
        Err(msg) => {
            eprintln!("error: {msg}");
            return 2;
        }
    };

    let bytes = match tts::say(tts::SayOptions {
        text: &text_joined,
        lang: &espeak_lang,
        engine,
        ssml: a.ssml,
        format,
        expand_abbrev: !a.no_expand_abbrev,
    }) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("error: {e}");
            return exit_code_for_tts_err(&e);
        }
    };

    let write_result = match a.out {
        Some(p) => std::fs::write(&p, &bytes).map_err(|e| e.to_string()),
        None => std::io::stdout()
            .write_all(&bytes)
            .map_err(|e| e.to_string()),
    };
    if let Err(msg) = write_result {
        eprintln!("error: write failed: {msg}");
        return 4;
    }
    0
}
