//! Kokoro voice embedding files.
//!
//! Layout: 510 rows × 256 cols, f32 little-endian, contiguous. Row index selected by
//! active token count: `min(token_count - 1, 509)`.

use std::path::Path;

pub const VOICE_ROWS: usize = 510;
/// Dimensions per row (voice embedding width).
pub const VOICE_COLS: usize = 256;
/// Expected voice file size in bytes.
pub const VOICE_FILE_BYTES: usize = VOICE_ROWS * VOICE_COLS * 4;

/// Load a Kokoro voice file into a flat Vec of [`VOICE_ROWS`] * [`VOICE_COLS`] floats.
pub fn load_voice(path: &Path) -> anyhow::Result<Vec<f32>> {
    let bytes = std::fs::read(path)?;
    if bytes.len() != VOICE_FILE_BYTES {
        anyhow::bail!(
            "voice file size {} != expected {} ({} rows × {} cols × 4 bytes)",
            bytes.len(),
            VOICE_FILE_BYTES,
            VOICE_ROWS,
            VOICE_COLS
        );
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}

/// Select the style embedding row for a given active-token count.
/// Indexes `voice[token_count]` (clamped to `VOICE_ROWS - 1`) to match
/// `kokoro-onnx` upstream — earlier code used `token_count - 1` (off-by-one),
/// see #207.
pub fn select_style(voice: &[f32], token_count: usize) -> &[f32] {
    let row = token_count.min(VOICE_ROWS - 1);
    &voice[row * VOICE_COLS..(row + 1) * VOICE_COLS]
}

/// Default voice id used when neither `--voice` nor auto-routing resolves one.
pub const DEFAULT_VOICE_ID: &str = "en-am_michael";

/// Resolved engine + paths for a given voice id.
#[derive(Debug)]
pub enum ResolvedVoice {
    Kokoro {
        model_path: std::path::PathBuf,
        voice_path: std::path::PathBuf,
        espeak_lang: &'static str,
    },
    /// Kokoro via FluidAudio CoreML sidecar on darwin-arm64.
    #[cfg(all(
        feature = "system_kokoro",
        target_os = "macos",
        target_arch = "aarch64"
    ))]
    FluidKokoro {
        voice_id: String,
        espeak_lang: &'static str,
    },
    /// Vosk-TTS multi-speaker Russian (replaces Piper-ru per spec/PR for #210).
    Vosk {
        model_dir: std::path::PathBuf,
        speaker_id: u32,
    },
    /// macOS system TTS via AVSpeechSynthesizer (#141). The voice id is
    /// whatever the user passed after the `macos-` prefix — forwarded to the
    /// Swift helper, which tries `AVSpeechSynthesisVoice(identifier:)` first
    /// and falls back to `AVSpeechSynthesisVoice(language:)`.
    #[cfg(all(feature = "system_tts", target_os = "macos"))]
    AVSpeech { voice_id: String },
}

impl ResolvedVoice {
    pub fn espeak_lang(&self) -> &'static str {
        match self {
            Self::Kokoro { espeak_lang, .. } => espeak_lang,
            #[cfg(all(
                feature = "system_kokoro",
                target_os = "macos",
                target_arch = "aarch64"
            ))]
            Self::FluidKokoro { espeak_lang, .. } => espeak_lang,
            Self::Vosk { .. } => "",
            // AVSpeech does its own G2P; the espeak language tag is unused.
            #[cfg(all(feature = "system_tts", target_os = "macos"))]
            Self::AVSpeech { .. } => "",
        }
    }
}

/// Parse a voice id like `en-am_michael` or `ru-ruslan` into engine + paths.
/// Voice id is `<lang>-<name>`; lang picks the engine and espeak language code.
/// The special `macos-*` prefix routes to AVSpeechSynthesizer on supported builds.
pub fn resolve_voice(cache_dir: &Path, voice_id: &str) -> anyhow::Result<ResolvedVoice> {
    let (lang, name) = voice_id.split_once('-').ok_or_else(|| {
        anyhow::anyhow!("voice id must be in 'lang-name' form (got '{voice_id}')")
    })?;
    match lang {
        "en" => resolve_kokoro(cache_dir, voice_id, name),
        "ru" => {
            let suffix = name.strip_prefix("vosk-").unwrap_or(name);
            resolve_vosk_ru(cache_dir, voice_id, suffix)
        }
        #[cfg(all(feature = "system_tts", target_os = "macos"))]
        "macos" => {
            if name.is_empty() {
                anyhow::bail!(
                    "'macos-' voice id requires a suffix (identifier or language code, e.g. macos-en-US)"
                );
            }
            Ok(ResolvedVoice::AVSpeech {
                voice_id: name.to_string(),
            })
        }
        #[cfg(not(all(feature = "system_tts", target_os = "macos")))]
        "macos" => anyhow::bail!(
            "'macos-*' voices require a macOS build with --features system_tts (got '{voice_id}')"
        ),
        other => {
            anyhow::bail!("language '{other}' not supported (use 'en-*', 'ru-*', or 'macos-*')")
        }
    }
}

fn resolve_kokoro(_cache_dir: &Path, voice_id: &str, name: &str) -> anyhow::Result<ResolvedVoice> {
    #[cfg(all(
        feature = "system_kokoro",
        target_os = "macos",
        target_arch = "aarch64"
    ))]
    {
        if !crate::tts::fluid_kokoro::supports_voice(name) {
            anyhow::bail!(
                "unknown FluidAudio Kokoro voice '{voice_id}'. run: kesha say --list-voices"
            );
        }
        return Ok(ResolvedVoice::FluidKokoro {
            voice_id: name.to_string(),
            espeak_lang: "en-us",
        });
    }
    #[allow(unreachable_code)]
    {
        let model_path = _cache_dir.join("models/kokoro-82m/model.onnx");
        let voice_path = _cache_dir
            .join("models/kokoro-82m/voices")
            .join(format!("{name}.bin"));
        if !voice_path.exists() {
            anyhow::bail!("voice '{voice_id}' not installed. run: kesha install --tts");
        }
        if !model_path.exists() {
            anyhow::bail!(
                "kokoro model not installed at {}. run: kesha install --tts",
                model_path.display()
            );
        }
        Ok(ResolvedVoice::Kokoro {
            model_path,
            voice_path,
            espeak_lang: "en-us",
        })
    }
}

fn resolve_vosk_ru(
    cache_dir: &Path,
    voice_id: &str,
    suffix: &str,
) -> anyhow::Result<ResolvedVoice> {
    let speaker_id: u32 = match suffix {
        "f01" => 0,
        "f02" => 1,
        "f03" => 2,
        "m01" => 3,
        "m02" => 4,
        _ => anyhow::bail!(
            "unknown Russian voice '{voice_id}'. valid: ru-vosk-f01, ru-vosk-f02, \
             ru-vosk-f03, ru-vosk-m01, ru-vosk-m02"
        ),
    };
    let model_dir = crate::models::model_dir_at(crate::models::ModelKind::VoskRu, cache_dir);
    if !crate::models::is_cached_in(crate::models::ModelKind::VoskRu, &model_dir) {
        anyhow::bail!("voice '{voice_id}' not installed. run: kesha install --tts");
    }
    Ok(ResolvedVoice::Vosk {
        model_dir,
        speaker_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_bytes(bytes: &[u8]) -> tempfile::NamedTempFile {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(bytes).unwrap();
        tmp
    }

    #[test]
    fn load_rejects_wrong_size() {
        let tmp = write_bytes(&[0u8; 100]);
        let err = load_voice(tmp.path()).unwrap_err();
        assert!(err.to_string().contains("voice file size"));
    }

    #[test]
    fn load_ok_for_correct_size() {
        let tmp = write_bytes(&vec![0u8; VOICE_FILE_BYTES]);
        let voice = load_voice(tmp.path()).unwrap();
        assert_eq!(voice.len(), VOICE_ROWS * VOICE_COLS);
    }

    #[test]
    fn select_style_clamps_high_indices() {
        let voice = vec![0.0; VOICE_ROWS * VOICE_COLS];
        let s = select_style(&voice, 10_000);
        assert_eq!(s.len(), VOICE_COLS);
    }

    #[test]
    fn select_style_handles_zero() {
        let voice = vec![0.0; VOICE_ROWS * VOICE_COLS];
        let s = select_style(&voice, 0);
        assert_eq!(s.len(), VOICE_COLS);
    }

    #[test]
    fn select_style_picks_correct_row() {
        // Row i contains value = i as f32
        let mut voice = Vec::with_capacity(VOICE_ROWS * VOICE_COLS);
        for row in 0..VOICE_ROWS {
            for _ in 0..VOICE_COLS {
                voice.push(row as f32);
            }
        }
        // token_count = 8 picks row 8 (kokoro-onnx uses voice[len(tokens)]).
        let s = select_style(&voice, 8);
        assert_eq!(s[0], 8.0);
        assert_eq!(s[VOICE_COLS - 1], 8.0);
    }

    #[cfg(not(all(
        feature = "system_kokoro",
        target_os = "macos",
        target_arch = "aarch64"
    )))]
    fn populate_cache(cache: &Path) {
        let voices = cache.join("models/kokoro-82m/voices");
        std::fs::create_dir_all(&voices).unwrap();
        std::fs::write(voices.join("am_michael.bin"), vec![0u8; VOICE_FILE_BYTES]).unwrap();
        std::fs::write(cache.join("models/kokoro-82m/model.onnx"), b"dummy").unwrap();
    }

    #[cfg(not(all(
        feature = "system_kokoro",
        target_os = "macos",
        target_arch = "aarch64"
    )))]
    #[test]
    fn resolve_installed_kokoro_voice() {
        let tmp = tempfile::tempdir().unwrap();
        populate_cache(tmp.path());
        let r = resolve_voice(tmp.path(), "en-am_michael").unwrap();
        match r {
            ResolvedVoice::Kokoro {
                voice_path,
                model_path,
                espeak_lang,
            } => {
                assert!(voice_path.ends_with("am_michael.bin"));
                assert!(model_path.ends_with("model.onnx"));
                assert_eq!(espeak_lang, "en-us");
            }
            other => panic!("expected Kokoro, got {other:?}"),
        }
    }

    #[cfg(all(
        feature = "system_kokoro",
        target_os = "macos",
        target_arch = "aarch64"
    ))]
    #[test]
    fn resolve_kokoro_voice_uses_fluid_audio_on_darwin() {
        let tmp = tempfile::tempdir().unwrap();
        let r = resolve_voice(tmp.path(), "en-am_michael").unwrap();
        match r {
            ResolvedVoice::FluidKokoro {
                voice_id,
                espeak_lang,
            } => {
                assert_eq!(voice_id, "am_michael");
                assert_eq!(espeak_lang, "en-us");
            }
            other => panic!("expected FluidKokoro, got {other:?}"),
        }
    }

    #[cfg(all(feature = "system_tts", target_os = "macos"))]
    #[test]
    fn resolve_macos_voice_returns_avspeech() {
        let tmp = tempfile::tempdir().unwrap();
        let r = resolve_voice(tmp.path(), "macos-com.apple.voice.compact.en-US.Samantha").unwrap();
        match r {
            ResolvedVoice::AVSpeech { voice_id } => {
                // Prefix stripped; the rest (including embedded dashes) passes through.
                assert_eq!(voice_id, "com.apple.voice.compact.en-US.Samantha");
            }
            other => panic!("expected AVSpeech, got {other:?}"),
        }
    }

    #[cfg(all(feature = "system_tts", target_os = "macos"))]
    #[test]
    fn resolve_macos_empty_suffix_errors() {
        // `macos-` alone would forward an empty string to the Swift helper,
        // which then fails with an unhelpful "voice not found". Reject early.
        let tmp = tempfile::tempdir().unwrap();
        let err = resolve_voice(tmp.path(), "macos-").unwrap_err().to_string();
        assert!(err.contains("requires a suffix"), "msg: {err}");
    }

    #[cfg(all(feature = "system_tts", target_os = "macos"))]
    #[test]
    fn resolve_macos_short_voice_id_works() {
        let tmp = tempfile::tempdir().unwrap();
        let r = resolve_voice(tmp.path(), "macos-en-US").unwrap();
        match r {
            ResolvedVoice::AVSpeech { voice_id } => assert_eq!(voice_id, "en-US"),
            other => panic!("expected AVSpeech, got {other:?}"),
        }
    }

    #[cfg(not(all(feature = "system_tts", target_os = "macos")))]
    #[test]
    fn resolve_macos_voice_errors_without_feature() {
        let tmp = tempfile::tempdir().unwrap();
        let err = resolve_voice(tmp.path(), "macos-en-US")
            .unwrap_err()
            .to_string();
        assert!(err.contains("system_tts"), "msg: {err}");
    }

    fn populate_vosk_ru(cache: &Path) {
        let dir = cache.join("models/vosk-ru");
        std::fs::create_dir_all(dir.join("bert")).unwrap();
        std::fs::write(dir.join("model.onnx"), b"dummy").unwrap();
        std::fs::write(dir.join("dictionary"), b"dummy").unwrap();
        std::fs::write(dir.join("config.json"), b"{}").unwrap();
        std::fs::write(dir.join("bert/model.onnx"), b"dummy").unwrap();
        std::fs::write(dir.join("bert/vocab.txt"), b"v").unwrap();
    }

    #[test]
    fn resolve_vosk_ru_default_voice() {
        let tmp = tempfile::tempdir().unwrap();
        populate_vosk_ru(tmp.path());
        let r = resolve_voice(tmp.path(), "ru-vosk-m02").unwrap();
        match r {
            ResolvedVoice::Vosk {
                model_dir,
                speaker_id,
            } => {
                assert!(model_dir.ends_with("models/vosk-ru"));
                assert_eq!(speaker_id, 4);
            }
            other => panic!("expected Vosk, got {other:?}"),
        }
    }

    #[test]
    fn resolve_vosk_ru_all_speaker_ids() {
        let tmp = tempfile::tempdir().unwrap();
        populate_vosk_ru(tmp.path());
        for (id, n) in [
            ("f01", 0u32),
            ("f02", 1),
            ("f03", 2),
            ("m01", 3),
            ("m02", 4),
        ] {
            let voice = format!("ru-vosk-{id}");
            match resolve_voice(tmp.path(), &voice).unwrap() {
                ResolvedVoice::Vosk { speaker_id, .. } => assert_eq!(speaker_id, n, "{voice}"),
                other => panic!("{voice}: expected Vosk, got {other:?}"),
            }
        }
    }

    #[test]
    fn resolve_vosk_ru_unknown_speaker_errors() {
        let tmp = tempfile::tempdir().unwrap();
        populate_vosk_ru(tmp.path());
        let err = resolve_voice(tmp.path(), "ru-vosk-zzz")
            .unwrap_err()
            .to_string();
        assert!(err.contains("vosk"), "msg: {err}");
    }

    #[test]
    fn resolve_vosk_ru_missing_model_errors_with_install_hint() {
        let tmp = tempfile::tempdir().unwrap();
        // No model on disk
        let err = resolve_voice(tmp.path(), "ru-vosk-m02")
            .unwrap_err()
            .to_string();
        assert!(err.contains("install --tts"), "msg: {err}");
    }

    #[test]
    fn resolve_missing_voice_errors_with_hint() {
        let tmp = tempfile::tempdir().unwrap();
        // Cache exists but voice does not
        let err = resolve_voice(tmp.path(), "en-am_michael").unwrap_err();
        assert!(err.to_string().contains("install --tts"), "msg: {err}");
    }

    #[test]
    fn resolve_missing_model_errors() {
        let tmp = tempfile::tempdir().unwrap();
        // Voice present but model missing
        let voices = tmp.path().join("models/kokoro-82m/voices");
        std::fs::create_dir_all(&voices).unwrap();
        std::fs::write(voices.join("am_michael.bin"), vec![0u8; VOICE_FILE_BYTES]).unwrap();
        let err = resolve_voice(tmp.path(), "en-am_michael").unwrap_err();
        assert!(err.to_string().contains("install --tts"), "msg: {err}");
    }

    #[test]
    fn resolve_bad_id_format() {
        let tmp = tempfile::tempdir().unwrap();
        let err = resolve_voice(tmp.path(), "gibberish").unwrap_err();
        assert!(err.to_string().contains("lang-name"));
    }

    #[test]
    fn resolve_unsupported_language() {
        let tmp = tempfile::tempdir().unwrap();
        let err = resolve_voice(tmp.path(), "fr-something").unwrap_err();
        assert!(err.to_string().contains("not supported"), "msg: {err}");
    }
}
