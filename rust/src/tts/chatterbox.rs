//! Chatterbox Multilingual ONNX inference.
//!
//! Pipeline:
//! reference WAV -> speech_encoder -> text token embeddings -> autoregressive
//! language_model -> conditional_decoder -> 24 kHz mono f32 PCM.

use std::path::Path;

use anyhow::{Context, Result};
use ndarray::{ArrayD, IxDyn};
use ort::session::Session;
use ort::value::Value;
use tokenizers::Tokenizer;

pub const SAMPLE_RATE: u32 = 24_000;

const EXAGGERATION_TOKEN: i64 = 6563;
const BOS_TOKEN: i64 = 255;
const EOS_TOKEN: i64 = 0;
const START_SPEECH_TOKEN: i64 = 6561;
const STOP_SPEECH_TOKEN: i64 = 6562;
const SPEECH_VOCAB_SIZE: usize = 6564;
const NUM_LAYERS: usize = 30;
const NUM_KV_HEADS: usize = 16;
const HEAD_DIM: usize = 64;
const MAX_NEW_TOKENS: usize = 4096;
const REPETITION_PENALTY: f32 = 1.2;

pub const SUPPORTED_LANGS: &[&str] = &[
    "ar", "da", "de", "el", "en", "es", "fi", "fr", "he", "hi", "it", "ja", "ko", "ms", "nl", "no",
    "pl", "pt", "ru", "sv", "sw", "tr", "zh",
];

#[derive(Clone)]
struct TensorF32 {
    shape: Vec<usize>,
    data: Vec<f32>,
}

#[derive(Clone)]
struct TensorI64 {
    shape: Vec<usize>,
    data: Vec<i64>,
}

pub struct Chatterbox {
    speech_encoder: Session,
    embed_tokens: Session,
    language_model: Session,
    conditional_decoder: Session,
    tokenizer: Tokenizer,
}

struct SpeechEncoderOutput {
    cond_emb: TensorF32,
    prompt_token: TensorI64,
    ref_xvector: TensorF32,
    prompt_feat: TensorF32,
}

impl Chatterbox {
    pub fn load(model_dir: &Path) -> Result<Self> {
        let onnx = model_dir.join("onnx");
        Ok(Self {
            speech_encoder: Session::builder()?
                .commit_from_file(onnx.join("speech_encoder.onnx"))
                .with_context(|| format!("load {}", onnx.join("speech_encoder.onnx").display()))?,
            embed_tokens: Session::builder()?
                .commit_from_file(onnx.join("embed_tokens.onnx"))
                .with_context(|| format!("load {}", onnx.join("embed_tokens.onnx").display()))?,
            language_model: Session::builder()?
                .commit_from_file(onnx.join("language_model.onnx"))
                .with_context(|| format!("load {}", onnx.join("language_model.onnx").display()))?,
            conditional_decoder: Session::builder()?
                .commit_from_file(onnx.join("conditional_decoder.onnx"))
                .with_context(|| {
                    format!("load {}", onnx.join("conditional_decoder.onnx").display())
                })?,
            tokenizer: Tokenizer::from_file(model_dir.join("tokenizer.json"))
                .map_err(|e| anyhow::anyhow!("load tokenizer.json: {e}"))?,
        })
    }

    pub fn infer(&mut self, text: &str, lang: &str, voice_path: &Path) -> Result<Vec<f32>> {
        validate_lang(lang)?;
        let reference = load_reference_wav(voice_path)?;
        let encoded = self.encode_speech(&reference)?;
        let input_ids = self.prepare_input_ids(text, lang)?;
        let position_ids = build_position_ids(&input_ids);
        let text_embeds = self.embed(&input_ids, &position_ids, 0.5)?;
        let combined = concat_embeddings(&encoded.cond_emb, &text_embeds)?;
        let speech_tokens = self.generate_speech_tokens(&combined, 0.5)?;
        let all_tokens = concat_i64(&encoded.prompt_token.data, &speech_tokens);
        self.decode_speech(
            &TensorI64 {
                shape: vec![1, all_tokens.len()],
                data: all_tokens,
            },
            &encoded.ref_xvector,
            &encoded.prompt_feat,
        )
    }

    fn encode_speech(&mut self, audio: &[f32]) -> Result<SpeechEncoderOutput> {
        let audio_values = f32_value(vec![1, audio.len()], audio.to_vec())?;
        let outputs = self
            .speech_encoder
            .run(ort::inputs!["audio_values" => audio_values])?;
        Ok(SpeechEncoderOutput {
            cond_emb: extract_f32(&outputs["audio_features"])?,
            prompt_token: extract_i64(&outputs["audio_tokens"])?,
            ref_xvector: extract_f32(&outputs["speaker_embeddings"])?,
            prompt_feat: extract_f32(&outputs["speaker_features"])?,
        })
    }

    fn prepare_input_ids(&self, text: &str, lang: &str) -> Result<Vec<i64>> {
        let tagged = format!("[{lang}]{}", text.trim());
        let encoding = self
            .tokenizer
            .encode(tagged, false)
            .map_err(|e| anyhow::anyhow!("tokenize Chatterbox text: {e}"))?;
        let mut ids = Vec::with_capacity(encoding.len() + 5);
        ids.push(EXAGGERATION_TOKEN);
        ids.push(BOS_TOKEN);
        ids.extend(encoding.get_ids().iter().map(|&id| i64::from(id)));
        ids.push(EOS_TOKEN);
        ids.push(START_SPEECH_TOKEN);
        ids.push(START_SPEECH_TOKEN);
        Ok(ids)
    }

    fn embed(
        &mut self,
        input_ids: &[i64],
        position_ids: &[i64],
        exaggeration: f32,
    ) -> Result<TensorF32> {
        let seq_len = input_ids.len();
        let ids = i64_value(vec![1, seq_len], input_ids.to_vec())?;
        let pos = i64_value(vec![1, seq_len], position_ids.to_vec())?;
        let exag = f32_value(vec![1], vec![exaggeration])?;
        let outputs = self.embed_tokens.run(ort::inputs![
            "input_ids" => ids,
            "position_ids" => pos,
            "exaggeration" => exag,
        ])?;
        extract_f32(&outputs["inputs_embeds"])
    }

    fn generate_speech_tokens(
        &mut self,
        combined_embeds: &TensorF32,
        exaggeration: f32,
    ) -> Result<Vec<i64>> {
        let seq_len = combined_embeds
            .shape
            .get(1)
            .copied()
            .context("combined embeddings must be rank-3")?;
        let mut kv_cache = empty_kv_cache()?;
        let mut attention_mask = vec![1_i64; seq_len];
        let mut generated = Vec::new();

        let mut prefill = ort::inputs![
            "inputs_embeds" => f32_value(combined_embeds.shape.clone(), combined_embeds.data.clone())?,
            "attention_mask" => i64_value(vec![1, attention_mask.len()], attention_mask.clone())?,
        ];
        push_kv_cache(&mut prefill, kv_cache);
        let logits = {
            let outputs = self.language_model.run(prefill)?;
            kv_cache = extract_kv_cache(&outputs)?;
            extract_f32(&outputs["logits"])?
        };
        let mut next_token = argmax(last_logits(&logits, seq_len)?) as i64;
        generated.push(next_token);

        for step in 0..MAX_NEW_TOKENS {
            if next_token == STOP_SPEECH_TOKEN {
                break;
            }
            let next_ids = [next_token];
            let next_pos = [step as i64];
            let next_embeds = self.embed(&next_ids, &next_pos, exaggeration)?;
            attention_mask.push(1);

            let mut feeds = ort::inputs![
                "inputs_embeds" => f32_value(next_embeds.shape, next_embeds.data)?,
                "attention_mask" => i64_value(vec![1, attention_mask.len()], attention_mask.clone())?,
            ];
            push_kv_cache(&mut feeds, kv_cache);
            let logits = {
                let outputs = self.language_model.run(feeds)?;
                kv_cache = extract_kv_cache(&outputs)?;
                extract_f32(&outputs["logits"])?
            };
            let mut step_logits = logits.data[..SPEECH_VOCAB_SIZE.min(logits.data.len())].to_vec();
            apply_repetition_penalty(&mut step_logits, &generated, REPETITION_PENALTY);
            next_token = argmax(&step_logits) as i64;
            generated.push(next_token);
        }

        Ok(generated
            .into_iter()
            .filter(|&t| t != START_SPEECH_TOKEN && t != STOP_SPEECH_TOKEN)
            .collect())
    }

    fn decode_speech(
        &mut self,
        speech_tokens: &TensorI64,
        speaker_embeddings: &TensorF32,
        speaker_features: &TensorF32,
    ) -> Result<Vec<f32>> {
        let outputs = self.conditional_decoder.run(ort::inputs![
            "speech_tokens" => i64_value(speech_tokens.shape.clone(), speech_tokens.data.clone())?,
            "speaker_embeddings" => f32_value(speaker_embeddings.shape.clone(), speaker_embeddings.data.clone())?,
            "speaker_features" => f32_value(speaker_features.shape.clone(), speaker_features.data.clone())?,
        ])?;
        Ok(extract_f32(&outputs["waveform"])?.data)
    }
}

pub fn validate_lang(lang: &str) -> Result<()> {
    if SUPPORTED_LANGS.contains(&lang) {
        return Ok(());
    }
    anyhow::bail!(
        "unsupported Chatterbox language '{lang}'. supported: {}",
        SUPPORTED_LANGS.join(", ")
    )
}

fn build_position_ids(input_ids: &[i64]) -> Vec<i64> {
    input_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| {
            if id >= START_SPEECH_TOKEN {
                0
            } else {
                i as i64 - 1
            }
        })
        .collect()
}

fn load_reference_wav(path: &Path) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)
        .with_context(|| format!("open Chatterbox reference WAV {}", path.display()))?;
    let spec = reader.spec();
    anyhow::ensure!(
        spec.sample_rate == SAMPLE_RATE,
        "Chatterbox reference WAV must be 24 kHz, got {} Hz ({})",
        spec.sample_rate,
        path.display()
    );
    anyhow::ensure!(
        spec.channels == 1,
        "Chatterbox reference WAV must be mono, got {} channels ({})",
        spec.channels,
        path.display()
    );
    match spec.sample_format {
        hound::SampleFormat::Float => Ok(reader.samples::<f32>().collect::<Result<Vec<_>, _>>()?),
        hound::SampleFormat::Int => {
            if spec.bits_per_sample <= 16 {
                Ok(reader
                    .samples::<i16>()
                    .map(|s| s.map(|v| v as f32 / 32768.0))
                    .collect::<Result<Vec<_>, _>>()?)
            } else {
                let denom = ((1_i64 << (spec.bits_per_sample - 1)) - 1) as f32;
                Ok(reader
                    .samples::<i32>()
                    .map(|s| s.map(|v| v as f32 / denom))
                    .collect::<Result<Vec<_>, _>>()?)
            }
        }
    }
}

fn f32_value(shape: Vec<usize>, data: Vec<f32>) -> Result<Value> {
    Ok(Value::from_array(ArrayD::<f32>::from_shape_vec(IxDyn(&shape), data)?)?.into())
}

fn i64_value(shape: Vec<usize>, data: Vec<i64>) -> Result<Value> {
    Ok(Value::from_array(ArrayD::<i64>::from_shape_vec(IxDyn(&shape), data)?)?.into())
}

fn extract_f32(value: &Value) -> Result<TensorF32> {
    let (shape, data) = value.try_extract_tensor::<f32>()?;
    Ok(TensorF32 {
        shape: shape.iter().map(|&d| d as usize).collect(),
        data: data.to_vec(),
    })
}

fn extract_i64(value: &Value) -> Result<TensorI64> {
    let (shape, data) = value.try_extract_tensor::<i64>()?;
    Ok(TensorI64 {
        shape: shape.iter().map(|&d| d as usize).collect(),
        data: data.to_vec(),
    })
}

fn concat_embeddings(cond: &TensorF32, text: &TensorF32) -> Result<TensorF32> {
    anyhow::ensure!(cond.shape.len() == 3, "cond embeddings must be rank-3");
    anyhow::ensure!(text.shape.len() == 3, "text embeddings must be rank-3");
    anyhow::ensure!(cond.shape[0] == 1 && text.shape[0] == 1, "batch must be 1");
    anyhow::ensure!(
        cond.shape[2] == text.shape[2],
        "embedding width mismatch: {} vs {}",
        cond.shape[2],
        text.shape[2]
    );
    let mut data = Vec::with_capacity(cond.data.len() + text.data.len());
    data.extend_from_slice(&cond.data);
    data.extend_from_slice(&text.data);
    Ok(TensorF32 {
        shape: vec![1, cond.shape[1] + text.shape[1], cond.shape[2]],
        data,
    })
}

fn concat_i64(prefix: &[i64], suffix: &[i64]) -> Vec<i64> {
    let mut out = Vec::with_capacity(prefix.len() + suffix.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(suffix);
    out
}

fn empty_kv_cache() -> Result<Vec<(String, Value)>> {
    let mut cache = Vec::with_capacity(NUM_LAYERS * 2);
    for i in 0..NUM_LAYERS {
        cache.push((
            format!("past_key_values.{i}.key"),
            f32_value(vec![1, NUM_KV_HEADS, 0, HEAD_DIM], Vec::new())?,
        ));
        cache.push((
            format!("past_key_values.{i}.value"),
            f32_value(vec![1, NUM_KV_HEADS, 0, HEAD_DIM], Vec::new())?,
        ));
    }
    Ok(cache)
}

fn push_kv_cache(
    inputs: &mut Vec<(
        std::borrow::Cow<'_, str>,
        ort::session::SessionInputValue<'_>,
    )>,
    cache: Vec<(String, Value)>,
) {
    for (name, value) in cache {
        inputs.push((name.into(), value.into()));
    }
}

fn extract_kv_cache(outputs: &ort::session::SessionOutputs<'_>) -> Result<Vec<(String, Value)>> {
    let mut cache = Vec::with_capacity(NUM_LAYERS * 2);
    for i in 0..NUM_LAYERS {
        let key_name = if outputs.contains_key(format!("present.{i}.key").as_str()) {
            format!("present.{i}.key")
        } else {
            format!("present_key_values.{i}.key")
        };
        let value_name = if outputs.contains_key(format!("present.{i}.value").as_str()) {
            format!("present.{i}.value")
        } else {
            format!("present_key_values.{i}.value")
        };
        let key = extract_f32(&outputs[key_name.as_str()])
            .with_context(|| format!("extract KV key layer {i}"))?;
        let value = extract_f32(&outputs[value_name.as_str()])
            .with_context(|| format!("extract KV value layer {i}"))?;
        cache.push((
            format!("past_key_values.{i}.key"),
            f32_value(key.shape, key.data)?,
        ));
        cache.push((
            format!("past_key_values.{i}.value"),
            f32_value(value.shape, value.data)?,
        ));
    }
    Ok(cache)
}

fn last_logits(logits: &TensorF32, seq_len: usize) -> Result<&[f32]> {
    let vocab = *logits.shape.get(2).context("logits must be rank-3")?;
    let start = (seq_len - 1)
        .checked_mul(vocab)
        .context("logits offset overflow")?;
    let end = start + SPEECH_VOCAB_SIZE.min(vocab);
    logits
        .data
        .get(start..end)
        .with_context(|| format!("logits slice out of bounds: {start}..{end}"))
}

fn apply_repetition_penalty(logits: &mut [f32], generated: &[i64], penalty: f32) {
    for &token in generated {
        let Ok(idx) = usize::try_from(token) else {
            continue;
        };
        if let Some(score) = logits.get_mut(idx) {
            if *score > 0.0 {
                *score /= penalty;
            } else {
                *score *= penalty;
            }
        }
    }
}

fn argmax(values: &[f32]) -> usize {
    values
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(idx, _)| idx)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_position_ids_zeroes_speech_tokens() {
        let ids = [
            EXAGGERATION_TOKEN,
            BOS_TOKEN,
            100,
            101,
            EOS_TOKEN,
            START_SPEECH_TOKEN,
        ];
        assert_eq!(build_position_ids(&ids), vec![0, 0, 1, 2, 3, 0]);
    }

    #[test]
    fn validates_all_supported_languages() {
        for lang in SUPPORTED_LANGS {
            validate_lang(lang).unwrap_or_else(|e| panic!("{lang}: {e}"));
        }
    }

    #[test]
    fn unsupported_languages_error_clearly() {
        let err = validate_lang("uk").unwrap_err().to_string();
        assert!(err.contains("unsupported Chatterbox language"));
    }

    #[test]
    fn repetition_penalty_penalizes_seen_tokens() {
        let mut logits = vec![2.0, -2.0, 4.0];
        apply_repetition_penalty(&mut logits, &[0, 1], 2.0);
        assert_eq!(logits, vec![1.0, -4.0, 4.0]);
    }
}
