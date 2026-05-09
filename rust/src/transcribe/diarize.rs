//! Speaker diarization on darwin-arm64 via the `kesha-diarize` Swift sidecar
//! (FluidAudio framework, SortformerDiarizer). Mirrors the AVSpeech sidecar
//! pattern (#141). Closes #199 angle D.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::TranscriptionSegment;

/// One speaker span emitted by the sidecar. Cluster IDs are stable within
/// one invocation but not across calls.
// `run` and its supporting types are wired in T7 (--speakers flag); allow
// dead_code until that wiring lands rather than add speculative call sites.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct DiarizeSpan {
    pub start: f32,
    pub end: f32,
    pub speaker: u32,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct SidecarOutput {
    spans: Vec<DiarizeSpan>,
}

/// Resolve the sidecar path. Sibling-of-engine first (release layout
/// `~/.cache/kesha/bin/kesha-diarize-darwin-arm64`), `KESHA_DIARIZE_SIDECAR`
/// fallback (set by `rust/build.rs` for `cargo run` / `cargo test`).
fn sidecar_path() -> Result<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            for name in ["kesha-diarize-darwin-arm64", "kesha-diarize"] {
                let candidate = parent.join(name);
                if candidate.exists() {
                    return Ok(candidate);
                }
            }
        }
    }
    if let Some(dev) = option_env!("KESHA_DIARIZE_SIDECAR") {
        let p = PathBuf::from(dev);
        if p.exists() {
            return Ok(p);
        }
    }
    bail!(
        "kesha-diarize sidecar not found next to the engine binary; run \
         `kesha install` to fetch it (darwin-arm64 only)"
    )
}

/// Run the sidecar against `audio_path` (16 kHz mono f32 IEEE_FLOAT WAV)
/// using the diarization model at `model_path`. Returns the parsed span list.
#[allow(dead_code)] // wired in T7 (--speakers flag)
pub(crate) fn run(audio_path: &Path, model_path: &Path) -> Result<Vec<DiarizeSpan>> {
    let sidecar = sidecar_path()?;
    let output = Command::new(&sidecar)
        .arg(audio_path)
        .arg(model_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("failed to spawn {}", sidecar.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "kesha-diarize exited {}: {}",
            output.status,
            stderr.trim()
        ));
    }
    let parsed: SidecarOutput = serde_json::from_slice(&output.stdout).with_context(|| {
        format!(
            "invalid JSON from kesha-diarize: {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })?;
    Ok(parsed.spans)
}

/// Project each ASR segment onto the diarization timeline by midpoint
/// overlap. Pure interval arithmetic; no model.
#[allow(dead_code)] // wired in T7 (--speakers flag); used in tests below
///
/// For each ASR segment, find the diarize span whose `[start, end)` covers
/// the ASR segment's midpoint; assign that span's speaker. If no diarize
/// span covers the midpoint, leave `speaker = None`.
pub(crate) fn merge_into(
    asr_segs: Vec<TranscriptionSegment>,
    diarize_spans: &[DiarizeSpan],
) -> Vec<TranscriptionSegment> {
    asr_segs
        .into_iter()
        .map(|mut seg| {
            let midpoint = (seg.start + seg.end) / 2.0;
            seg.speaker = diarize_spans
                .iter()
                .find(|s| s.start <= midpoint && midpoint < s.end)
                .map(|s| s.speaker);
            seg
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start: f32, end: f32, text: &str) -> TranscriptionSegment {
        TranscriptionSegment {
            start,
            end,
            text: text.into(),
            speaker: None,
        }
    }
    fn span(start: f32, end: f32, speaker: u32) -> DiarizeSpan {
        DiarizeSpan {
            start,
            end,
            speaker,
        }
    }

    #[test]
    fn one_to_one_overlap_assigns_speaker() {
        let segs = vec![seg(1.0, 3.0, "hi")];
        let spans = vec![span(0.0, 5.0, 7)];
        let out = merge_into(segs, &spans);
        assert_eq!(out[0].speaker, Some(7));
    }

    #[test]
    fn no_overlap_yields_none() {
        let segs = vec![seg(10.0, 11.0, "hi")];
        let spans = vec![span(0.0, 5.0, 0)];
        let out = merge_into(segs, &spans);
        assert_eq!(out[0].speaker, None);
    }

    #[test]
    fn span_split_assigns_via_midpoint() {
        // ASR seg 1.0-3.0, midpoint 2.0. Spans: 0..1.5 (speaker 0), 1.5..5 (speaker 1).
        // 2.0 ∈ [1.5, 5) → speaker 1.
        let segs = vec![seg(1.0, 3.0, "hi")];
        let spans = vec![span(0.0, 1.5, 0), span(1.5, 5.0, 1)];
        let out = merge_into(segs, &spans);
        assert_eq!(out[0].speaker, Some(1));
    }

    #[test]
    fn empty_diarize_spans_yield_all_none() {
        let segs = vec![seg(0.0, 1.0, "a"), seg(1.0, 2.0, "b")];
        let out = merge_into(segs, &[]);
        assert!(out.iter().all(|s| s.speaker.is_none()));
    }

    #[test]
    fn empty_asr_segs_returns_empty() {
        let out = merge_into(vec![], &[span(0.0, 5.0, 0)]);
        assert!(out.is_empty());
    }

    #[test]
    fn four_speaker_meeting_assigns_distinct_ids() {
        let segs = vec![
            seg(0.5, 1.5, "a"),
            seg(2.0, 3.0, "b"),
            seg(4.0, 5.0, "c"),
            seg(6.0, 7.0, "d"),
        ];
        let spans = vec![
            span(0.0, 1.7, 0),
            span(1.7, 3.5, 1),
            span(3.5, 5.5, 2),
            span(5.5, 8.0, 3),
        ];
        let out = merge_into(segs, &spans);
        assert_eq!(
            out.iter().map(|s| s.speaker).collect::<Vec<_>>(),
            vec![Some(0), Some(1), Some(2), Some(3)]
        );
    }
}
