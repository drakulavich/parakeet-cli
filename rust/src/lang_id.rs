use crate::audio;
use crate::models;
use anyhow::{Context, Result};
use serde::Serialize;

#[derive(Serialize)]
pub struct LangDetectResult {
    pub code: String,
    pub confidence: f32,
}

const MAX_SECONDS: f32 = 10.0;

pub fn detect_audio_language(audio_path: &str) -> Result<LangDetectResult> {
    audio::ensure_audio_track(audio_path)?;

    if !models::is_cached(models::ModelKind::LangId) {
        anyhow::bail!("Lang-ID model not installed. Run: kesha install");
    }

    let dir = models::model_dir(models::ModelKind::LangId);

    // Load ONNX session
    let mut session = ort::session::Session::builder()
        .context("failed to create lang-id session builder")?
        .commit_from_file(dir.join("lang-id-ecapa.onnx"))
        .context("failed to load lang-id model")?;

    // Load labels
    let labels: Vec<String> = {
        let data = std::fs::read_to_string(dir.join("labels.json"))?;
        serde_json::from_str(&data)?
    };

    // Load audio (first 10s)
    let samples = audio::load_audio_truncated(audio_path, MAX_SECONDS)?;

    // Run inference
    // Input: "waveform" [1, samples] float32
    // Output: "language_probs" [1, 107] float32
    let input_len = samples.len();
    let waveform =
        ort::value::Value::from_array(ndarray::Array2::from_shape_vec((1, input_len), samples)?)?;

    let outputs = session.run(ort::inputs!["waveform" => waveform])?;

    // Extract probs - same pattern as onnx.rs: try_extract_tensor returns (&Shape, &[T])
    let (_, probs_data) = outputs[0]
        .try_extract_tensor::<f32>()
        .context("failed to extract language_probs tensor")?;

    // Find argmax
    let mut best_idx = 0;
    let mut best_val = f32::NEG_INFINITY;
    for (i, &val) in probs_data.iter().enumerate() {
        if val > best_val {
            best_val = val;
            best_idx = i;
        }
    }

    Ok(LangDetectResult {
        code: labels.get(best_idx).cloned().unwrap_or_default(),
        confidence: best_val,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_audio_fails_before_model_lookup() {
        let err = match detect_audio_language("/nonexistent/kesha/lang-id.wav") {
            Ok(_) => panic!("missing audio should fail before lang-id model lookup"),
            Err(err) => err,
        };
        let msg = format!("{err:#}");
        assert!(
            !msg.contains("Lang-ID model not installed"),
            "input validation must happen before model lookup, got: {msg}"
        );
    }
}
