# Speaker diarization PoC for darwin-arm64 (long-meeting transcription)

**Date:** 2026-05-09
**Status:** Approved (sections 1–5, brainstormed with maintainer)
**Issue:** [#199](https://github.com/drakulavich/kesha-voice-kit/issues/199) — angle D (a fork of the angle catalog in #199 — process meeting audio of any length, focused on transcription quality + speaker labels rather than live capture)
**Engine release:** v1.12.0
**Branch:** `feat/199-darwin-diarization`

## Problem

Single-track recordings of multi-person meetings transcribe as a single flat block of text. The reader cannot tell when a speaker switched, and at meeting length (1 h+) the wall-of-text becomes hard to use even when the words themselves are correct. Two adjacent failure modes show up at the same scale:

- **A — Pipeline robustness on long files.** kesha currently holds the entire decoded WAV in RAM (`audio::load_audio` → `Vec<f32>`) and runs VAD over the whole buffer. At 16 kHz mono f32, a 1 h file is ~230 MB raw plus VAD + ASR working set; the existing pipeline survives this on a 16 GB Mac, but margins are thin and >1 h is unsupported.
- **B — Accuracy at length.** VAD chunk boundaries occasionally split mid-utterance, ASR transcribes each chunk independently, and per-chunk artifacts (repeated phrases at boundaries, dropped words at fade-outs) accumulate over a 2 h file.
- **C — Speaker confusion.** The user's stated primary pain. No "who said what" markers means meeting transcripts can't be skimmed, attributed, or followed.

Of the three, **C is the user's top complaint and the most valuable to fix in a PoC**. A is bounded by the 1 h cap (covers 90% of the user's actual meetings; >1 h streaming deferred). B is orthogonal to diarization and has its own follow-up tracking issue.

## Goal

Ship `kesha --json --timestamps --speakers meeting.m4a` on darwin-arm64 returning per-segment speaker labels, in v1.12.0.

For voice id prefix unrelated; this is a transcription feature. The emitted shape extends `TranscriptionSegment` with an optional `speaker: Option<u32>` field. `--speakers` requires machine-readable output (`--json` / `--toon` / `--format json`); plain text mode skips diarization entirely. Linux / Windows return a clear "not supported on this platform" error with a tracking-issue link.

Out of scope (this spec):

- Streaming for >1 h files. Current pipeline caps at 1 h; >1 h tracked separately.
- Speaker-name labels (`speaker: "Anton"` instead of `speaker: 0`). Cluster IDs only; persistent voice enrollment is a separate identification layer.
- Cross-file speaker stability. Cluster IDs are per-call; matching across files needs voice embeddings + a persistence store.
- Live capture (angle C of #199 catalog). Batch-only PoC.
- LLM enhancement pass — decisions / action items / summary. Granola's signature feature; users plug in whatever LLM they prefer over the JSON output.
- VAD overlap + dedup at chunk boundaries (failure mode B). Orthogonal; tracked separately so the diarization pipeline doesn't grow extra knobs while it's still PoC.
- Linux / Windows diarization. Dual-backend (FluidAudio + ONNX) deferred.
- Russian-language diarization quality (verify in spike — if FluidAudio is English-tuned, ship with `--speakers` requiring `--lang en` for v1).

## Decisions (from brainstorm)

| Question | Decision | Rationale |
|---|---|---|
| Q1 — failure mode focus | A + B + C: robustness, length-accuracy, speaker confusion. **C is the primary pain; A handled by 1 h cap; B deferred.** | A and B are layered on the same pipeline, but B's fix (overlap windows + dedup) is orthogonal to diarization and would muddy the diarization design. |
| Q2 — input shape | **Single-track mixed audio** — Zoom / Teams / iPhone Voice Memo / aggregate device. Multi-track exports (Zoom Pro per-participant) deferred. | Single-track is the harder problem; solving it covers the multi-track case as a degenerate special case (each track has one cluster). |
| Q3 — length cap | **1 h** for the PoC. >1 h streaming deferred to a follow-up. | At 16 kHz mono f32, 1 h ≈ 230 MB raw + ASR/diarize working set ≈ 1 GB peak. Tractable on 16 GB without streaming. |
| Q4 — diarization backend | **FluidAudio (Swift sidecar) only on darwin-arm64.** Linux / Windows feature-gated, returns "not supported" hint. | The Rust binding `fluidaudio-rs 0.1.0` does NOT expose diarization (only ASR + VAD); upstream Swift framework does. Cross-platform ONNX backend deferred — ship darwin v1 first, validate the contract, then layer ONNX. |
| Q5 — integration mechanism | **Swift sidecar `kesha-diarize-darwin-arm64`**, mirroring the AVSpeech sidecar. Subprocess IPC. | No Rust FFI bridging needed. Mirrors a precedented pattern (#141). Cleaner than vendoring + extending `fluidaudio-rs` (path ii) for a feature on a single platform. |

## Architecture

### Pipeline

```
audio file (≤ 1 h, single-track mixed)
   │
   ▼
1. symphonia decode → 16 kHz mono f32              (existing, rust/src/audio.rs)
   │
   ▼
2. Silero VAD                                       (existing, rust/src/vad.rs)
   → Vec<(f32, f32)> speech spans
   │
   ▼
3. Parakeet TDT ASR per VAD span                    (existing, transcribe_via_vad)
   → Vec<TranscriptionSegment { start, end, text }>
   │
   ▼
4. FluidAudio diarization (Swift sidecar)           (NEW)
   stdin: WAV path (16 kHz mono f32 IEEE_FLOAT, written to a temp file by Rust)
   stdout: { spans: [{ start, end, speaker }, ...] }
   │
   ▼
5. Merge step (pure-Rust)                           (NEW)
   project each ASR segment onto diarization timeline
   by midpoint overlap; assign Option<u32>
   → Vec<TranscriptionSegment { start, end, text, speaker }>
```

ASR and diarization run independently over the same WAV. The merge step is pure interval arithmetic — no model.

**Why this order**: ASR + diarization are decoupled passes on the same input — both read the decoded WAV from step 1, neither depends on the other's output until the merge. The merge is "for each ASR segment, find the diarize span covering its midpoint" — `O(N + M)` two-pointer walk over sorted intervals. v1 runs them sequentially (ASR completes, then diarize); concurrent invocation is a follow-up perf optimization.

**Why not "diarize first, then ASR per speaker"**: would give cleaner per-speaker transcripts but requires either re-segmenting audio at every speaker change boundary or running ASR multiple times. More complex with no PoC value.

**Why not sub-VAD speaker change detection** (Approach 2 from brainstorm): finer granularity, but FluidAudio internally does its own segmentation; we'd be fighting it. Layer on top of FluidAudio's output, not under it.

### File layout

| Path | Status | Responsibility |
|---|---|---|
| `swift/kesha-diarize/main.swift` | NEW | Tiny Swift program, links FluidAudio framework, takes WAV path on argv, emits JSON spans on stdout |
| `swift/kesha-diarize/Package.swift` | NEW | Swift Package manifest pinning FluidAudio dependency |
| `rust/build.rs` | MODIFY | Under `system_diarize` cfg, compile `swift/kesha-diarize/main.swift` → `$OUT_DIR/kesha-diarize` for `cargo run` / `cargo test` (mirrors existing AVSpeech pattern) |
| `rust/Cargo.toml` | MODIFY | Add `system_diarize` feature; default-enable for darwin-arm64 |
| `rust/src/transcribe.rs` | MODIFY | Add `Diarize` decision branch parallel to `Vad`; `transcribe_via_vad` call site invokes `diarize_audio_path` and merges into segments |
| `rust/src/transcribe/diarize.rs` | NEW | Sidecar path resolution (sibling-of-engine first, `$OUT_DIR/kesha-diarize` fallback for dev), subprocess invocation, JSON parse, merge logic. Pure-Rust unit tests for the merge. |
| `rust/src/capabilities.rs` | MODIFY | Add `pub const TRANSCRIBE_DIARIZE_FEATURE = "transcribe.diarize"`; push into `features` under `#[cfg(feature = "system_diarize")]` |
| `rust/src/main.rs` | MODIFY | Add `--speakers` flag to `transcribe` subcommand; gate on `--json`; require darwin-arm64 (return error with tracking-issue link otherwise) |
| `src/engine.ts` | MODIFY | Add `TRANSCRIBE_DIARIZE_FEATURE` const; extend `transcribeEngineWithSegments` to forward `--speakers` when option set; capability gate |
| `src/transcribe.ts` | MODIFY | `TranscribeOptions.speakers?: boolean` flows through to engine |
| `src/cli/main.ts` | MODIFY | `--speakers` flag, gated on machine-readable output (same shape as `--timestamps` gate) |
| `rust/tests/diarize_e2e.rs` | NEW | gated by `#[cfg(feature = "system_diarize")]`; synthesize 30 s 2-speaker WAV from kesha's own TTS, run end-to-end, assert ≥ 2 cluster IDs |
| `tests/integration/e2e-engine.test.ts` | MODIFY | `--speakers` round-trip; skip on non-darwin via existing `os.platform()` check |
| `.github/workflows/build-engine.yml` | MODIFY | macos-14 matrix row gets `coreml,tts,system_tts,system_diarize` features. Pre-upload smoke runs `kesha-diarize-darwin-arm64 --list-models` and asserts exit 0 |

## Output shape

`TranscriptionSegment` (extended):

```rust
#[derive(Debug, Clone, Serialize)]
pub struct TranscriptionSegment {
    pub start: f32,
    pub end: f32,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker: Option<u32>,
}
```

`speaker` is `None` when `--speakers` is absent (default), `Some(cluster_id)` when present. `cluster_id` is FluidAudio's own integer assignment, stable within one file but not across files.

JSON with `--speakers`:

```json
{
  "text": "Hello everyone. Hi Anton, how are you. Doing great, thanks.",
  "segments": [
    { "start": 0.0,  "end": 2.1,  "text": "Hello everyone.",          "speaker": 0 },
    { "start": 2.4,  "end": 4.8,  "text": "Hi Anton, how are you.",   "speaker": 1 },
    { "start": 5.0,  "end": 7.3,  "text": "Doing great, thanks.",     "speaker": 0 }
  ]
}
```

JSON without `--speakers` (existing v1.9.0 shape, unchanged):

```json
{
  "text": "...",
  "segments": [
    { "start": 0.0, "end": 2.1, "text": "Hello everyone." },
    ...
  ]
}
```

## CLI surface

```bash
kesha --json --timestamps --speakers meeting.m4a       # darwin-arm64
kesha --toon --timestamps --speakers meeting.m4a       # darwin-arm64

kesha --speakers meeting.m4a                           # exit 2: --speakers requires --json/--toon/--format json
kesha --json --speakers meeting.m4a                    # exit 2: --speakers requires --timestamps (auto-implied? see CLI rules below)
```

CLI rules:

- `--speakers` implies `--timestamps` automatically (you can't attach a speaker label without per-segment timestamps).
- `--speakers` requires machine-readable output (`--json` / `--toon` / `--format json`); fails with exit 2 otherwise. Same shape as `--timestamps`'s gate.
- On Linux / Windows engines (`transcribe.diarize` not in `--capabilities-json`), `--speakers` returns:
  ```
  Error: speaker diarization is currently darwin-arm64 only.
  Tracked at https://github.com/drakulavich/kesha-voice-kit/issues/<TBD-cross-platform-diarization>.
  ```
- Help text:
  ```
  --speakers     Include speaker labels in transcript segments. Requires
                 --json / --toon / --format json. Implies --timestamps.
                 Currently darwin-arm64 only.
  ```

## Swift sidecar protocol

**Binary name**: `kesha-diarize-darwin-arm64`. Built by `rust/build.rs` under `system_diarize`. Default-on for the darwin-arm64 release builds (`build-engine.yml` matrix row).

**Distribution**: uploaded as a separate artifact alongside `kesha-engine-darwin-arm64`. `kesha install` downloads it to `~/.cache/kesha/bin/kesha-diarize-darwin-arm64` next to the engine. Runtime path resolution in `rust/src/transcribe/diarize.rs::sidecar_path()`:

1. Sibling-of-engine first (release layout: `~/.cache/kesha/bin/kesha-diarize-darwin-arm64` next to the running engine binary).
2. `$OUT_DIR/kesha-diarize` fallback for `cargo run` / `cargo test` (development).

**IPC contract** — argv for path, JSON on stdout, errors on stderr:

```
$ ./kesha-diarize-darwin-arm64 /tmp/audio.wav
{
  "spans": [
    { "start": 0.0,   "end": 4.32,  "speaker": 0 },
    { "start": 4.32,  "end": 7.81,  "speaker": 1 },
    { "start": 7.81,  "end": 12.05, "speaker": 0 }
  ]
}
```

- Audio input: 16 kHz mono f32 IEEE_FLOAT WAV (the format `audio.rs::load_audio` already produces). Rust side writes a temp WAV to `$KESHA_CACHE_DIR/diarize-tmp/<uuid>.wav`, sidecar reads it, Rust deletes after subprocess exits. Same shim pattern as the `transcribe_samples` workaround in CLAUDE.md.
- Exit 0 on success + JSON on stdout. Non-zero exit + human-readable line on stderr on failure.
- Zero-arg `--list-models` flag: prints FluidAudio's diarization model identifier + version. Used by `kesha status` and the build-engine smoke test.
- Single-shot per call. No `--stdin-loop` warm-session for v1 — diarization is one-pass per file; cold-start overhead is bounded by spike measurement.

**FluidAudio model dependency**: TBD by spike (Section 5 step 2). If models bundled in the framework, no install changes needed. If models lazy-download to a system cache, `kesha install` triggers the download deterministically before the first user call.

## Spike findings (post-validation)

> **Required before plan-writing.** Per CLAUDE.md "VERIFY THIRD-PARTY MODEL FORMATS WITH A SPIKE". Spike artifacts at `/tmp/kesha-diarize-spike/`. Replace this section's TODOs with the actual findings before transitioning to writing-plans.

| Step | Question | Finding (2026-05-09) |
|---|---|---|
| 1 | Does `import FluidAudio` expose a callable `diarize(...)` returning `[(start, end, speakerId)]` in the latest Swift package release? | **FAIL.** SwiftPM resolves `FluidAudio` from `0.1.0`, all 257 source files compile (Magpie TTS, PocketTts, SentencePiece, etc.), but `FluidAudio.Diarizer.self` errors with `type 'FluidAudio' has no member 'Diarizer'`. The published 0.1.0 surface does not export a diarization symbol at module scope. |
| 2 | Are diarization models bundled in the .framework, or lazy-downloaded on first call? | SKIPPED — Q1 hard gate failed. |
| 3 | On a known 2-speaker 5-min sample, does it produce 2 cluster IDs with reasonable timestamps? | SKIPPED — Q1 hard gate failed. |
| 4 | Wall-clock latency on a 1 h file. | SKIPPED — Q1 hard gate failed. |
| 5 | Does it work on Russian audio, or is it English-tuned? | SKIPPED — Q1 hard gate failed. |

**Decision: pivot to ONNX (or wait for upstream).** Q1's hard-gate failure means the FluidAudio Swift sidecar approach in this design is not currently achievable against the published `from: "0.1.0"` package. Two recoveries are open:

1. **Pivot to ONNX backend** — replace the `kesha-diarize-darwin-arm64` Swift sidecar with a cross-platform ONNX pipeline (e.g. `pyannote/speaker-diarization-3.1` ONNX export, or `sherpa-onnx` diarization). This re-opens Q4 of the design questions and removes the darwin-arm64-only restriction as a side benefit. Plan rewrite required (replaces T4, T5, T10, parts of T13).
2. **Wait for upstream** — block this issue until FluidAudio cuts a release exposing diarization at module scope, then re-run the spike. Lower engineering cost but indefinite timeline.

Recommendation: **pivot to ONNX**. Aligns with the project's existing ONNX investment (`ort` is already unconditional, lang_id is ONNX, ONNX is the default ASR backend on non-darwin) and is the path the original design called out as the deferred fallback. Spike artifacts retained at `/tmp/kesha-199-evidence/T1-spike.notes` and the raw error transcript.

**Decision points exiting the spike**:

- **All five PASS** → proceed to writing-plans.
- **Step 1 fails** (Swift API not yet wired) → either wait for FluidAudio release (block PoC) or pivot to ONNX Approach 1. Re-open Q4.
- **Step 2 — lazy cache, system-managed path** → wire `kesha install --diarize` to trigger the download deterministically. Adds a model-cache surface section to the plan.
- **Step 5 — Russian quality poor** → on detected non-English audio (`detect-text-lang` after ASR), warn to stderr `"warning: speaker diarization is tuned for English audio; results on <lang> may be inaccurate"` and proceed. No hard gate — preserves the user's ability to opt in. Russian-tuned diarization tracked as follow-up.

Spike outcome must be recorded inline in this section before plan-writing begins.

## Capability JSON

```jsonc
{
  "features": [
    "transcribe",
    "transcribe.segments",
    "transcribe.diarize",          // new, darwin-arm64 only
    "detect-lang",
    "vad",
    "detect-text-lang",
    "tts",
    "tts.ru_acronym_expansion",
    "tts.en_acronym_expansion",
    "tts.ru_emphasis_marker"
  ],
  ...
}
```

Engine reports `transcribe.diarize` only when built with the `system_diarize` feature. TS CLI gates `--speakers` forwarding on this OR fail-loud with the platform-not-supported message.

## Testing

| Layer | Coverage |
|---|---|
| Rust unit | `merge_diarization_into_segments(asr_segs, diarize_spans) -> segs_with_speaker` — pure interval-overlap math, fully unit-testable with mock data. Cases: 1:1 overlap, span split across two speakers (assign to majority-overlap), no overlap (None), empty diarization output, single-speaker meeting (all `Some(0)`), 4-speaker meeting (cluster IDs 0..=3) |
| Rust integration | `rust/tests/diarize_e2e.rs` — gated by `#[cfg(feature = "system_diarize")]`; synthesize a 30 s 2-speaker WAV from kesha's own TTS (e.g., `am_michael` for spans 1+3, `bm_george` for span 2), run end-to-end via the engine binary, assert ≥ 2 distinct cluster IDs in output and ≥ 80% of the ASR-detected speech time carries a non-`None` `speaker` label. Self-fixturing — no real-meeting bytes in the repo |
| TS integration | extend `tests/integration/e2e-engine.test.ts` with a `--speakers` round-trip; skip on non-darwin via the existing `os.platform()` check |
| Capability JSON | extend `tests/integration/capabilities.test.ts` to assert `transcribe.diarize: true` on darwin-arm64, `false` elsewhere |
| Build smoke | `build-engine.yml` extended to run `kesha-diarize-darwin-arm64 --list-models` post-upload; assert exit 0. Mirrors the existing `--capabilities-json` engine smoke |
| Audio QA | a 4-file diarization corpus at `/tmp/kesha-diarize-evidence/`: 2-speaker / 4-speaker / cross-talk / Russian (if spike step 5 passes). The existing `audio-quality-check` agent doesn't validate diarization output shape, so manual listening + speaker-count verification stays a human gate before publish |

**Pre-merge gate** (CLAUDE.md):

```bash
cd rust && cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
bun test && bunx tsc --noEmit
# Plus the audio QA pass on the 4-file corpus.
```

**Independent v1.12.0 validation** (CLAUDE.md "make smoke-test ALONE DOES NOT VALIDATE"): after `gh release edit v1.12.0 --draft=false`, download both `kesha-engine-darwin-arm64` and `kesha-diarize-darwin-arm64`, exercise `--speakers` end-to-end on a known 2-speaker fixture, confirm 2 distinct cluster IDs in JSON output. Cross-platform: download `kesha-engine-linux-x64`, exercise `--speakers`, confirm the platform-not-supported error fires (tracking-issue URL present in the message).

## Release plan (engine release v1.12.0)

Per CLAUDE.md "RELEASE PROCESS — CLI AND ENGINE ARE VERSIONED INDEPENDENTLY". Engine release because `rust/` changed AND a new platform-specific sidecar binary ships:

1. Lockstep bump: `rust/Cargo.toml`, `rust/Cargo.lock`, `package.json#keshaEngine.version`, `package.json#version` → all `1.12.0`.
2. Merge to main.
3. `git tag v1.12.0 && git push origin v1.12.0` — triggers `build-engine.yml`.
4. Author release notes BEFORE publishing the draft. Highlights: speaker labels for `en-*` (and `ru-*` if spike Q5 passes); darwin-arm64 only; `--speakers` flag; new capability `transcribe.diarize`; new sidecar `kesha-diarize-darwin-arm64`.
5. `gh release edit v1.12.0 --draft=false`.
6. Independent v1.12.0 validation (above).
7. `npm publish --access public`.
8. Verify issue #199 is updated with a "ships angle D — speaker labels (darwin-arm64)" comment; close any sub-issues that map to this PoC's scope.

## Acceptance criteria

- [ ] `kesha --json --timestamps --speakers meeting.m4a` on darwin-arm64 produces JSON with `segments[].speaker` populated; ≥ 2 distinct cluster IDs on a 2-speaker fixture.
- [ ] `kesha --speakers ...` (without machine-readable output) exits 2 with the gate-violation message.
- [ ] `kesha --json --speakers meeting.m4a` (without `--timestamps`) auto-implies `--timestamps` and proceeds.
- [ ] `kesha --json --timestamps --speakers meeting.m4a` on Linux / Windows returns the platform-not-supported error with a tracking-issue link.
- [ ] All Rust unit + integration tests green; `cargo clippy --all-targets -- -D warnings` clean.
- [ ] `bun test && bunx tsc --noEmit` clean.
- [ ] `kesha-engine --capabilities-json` reports `transcribe.diarize: true` on darwin-arm64, absent elsewhere.
- [ ] `kesha-diarize-darwin-arm64 --list-models` post-upload smoke passes in `build-engine.yml`.
- [ ] CHANGELOG / README / SKILL.md / docs/tts.md examples for `--speakers` (linked from this spec on commit).
- [ ] Independent v1.12.0 validation passes — known 2-speaker fixture produces 2 cluster IDs end-to-end.
- [ ] Issue #199 updated with PoC outcome; cross-platform diarization tracking issue filed for the deferred ONNX path.

## CLAUDE.md applicability

- Engine release (touches `rust/`).
- New cargo feature `system_diarize` — must appear in every `build-engine.yml` matrix row that supports it (currently only darwin-arm64 row gets it; CI feature-matrix auditor will skip).
- Spike-mandatory before plan: per CLAUDE.md "VERIFY THIRD-PARTY MODEL FORMATS WITH A SPIKE". Section 5 above lists the five spike steps.
- `cargo clippy --all-targets -- -D warnings` mandatory.
- Integration tests skip on `release/*` branches (release chicken-and-egg gate).
- Independent v1.12.0 validation per CLAUDE.md "make smoke-test ALONE DOES NOT VALIDATE" — must download the published assets directly and exercise `--speakers` before `npm publish`.
- Sidecar IPC follows the `say-avspeech-darwin-arm64` precedent (#141).
