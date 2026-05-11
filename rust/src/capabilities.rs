use serde::Serialize;

// `transcribe::diarize` is the runtime module gated on
// `all(feature = "system_diarize", target_os = "macos")` (see
// transcribe/mod.rs). Mirror that gate here so the advertised
// capability matches the runtime: building `--features system_diarize`
// on Linux otherwise pushes the flag without an executable code path,
// and `--speakers` would advertise OK then bail out at request time.
#[cfg(all(feature = "system_diarize", target_os = "macos"))]
use crate::transcribe::TRANSCRIBE_DIARIZE_FEATURE;
use crate::transcribe::TRANSCRIBE_SEGMENTS_FEATURE;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub protocol_version: u32,
    pub backend: &'static str,
    pub features: Vec<&'static str>,
}

pub fn get_capabilities() -> Capabilities {
    #[allow(unused_mut)]
    let mut features = vec![
        "transcribe",
        TRANSCRIBE_SEGMENTS_FEATURE,
        "detect-lang",
        "vad",
    ];

    #[cfg(target_os = "macos")]
    features.push("detect-text-lang");

    #[cfg(feature = "tts")]
    features.push("tts");
    #[cfg(feature = "tts")]
    features.push("tts.ru_acronym_expansion");
    #[cfg(feature = "tts")]
    features.push("tts.en_acronym_expansion");
    #[cfg(feature = "tts")]
    features.push("tts.ru_emphasis_marker");
    #[cfg(feature = "tts")]
    features.push("tts.prosody_rate");

    #[cfg(all(feature = "system_diarize", target_os = "macos"))]
    features.push(TRANSCRIBE_DIARIZE_FEATURE);

    Capabilities {
        protocol_version: 2,
        backend: backend_name(),
        features,
    }
}

fn backend_name() -> &'static str {
    #[cfg(feature = "coreml")]
    {
        "coreml"
    }
    #[cfg(not(feature = "coreml"))]
    {
        "onnx"
    }
}
