use serde::Serialize;

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
