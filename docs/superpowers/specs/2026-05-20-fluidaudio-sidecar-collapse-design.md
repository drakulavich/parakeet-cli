# Collapse FluidAudio sidecars into a forked `fluidaudio-rs`

- **Date:** 2026-05-20
- **Status:** Design approved; pending spec review → implementation plan
- **Scope owner:** drakulavich

## Context

The darwin-arm64 engine spawns four Swift sidecar subprocesses. Two of them —
`kesha-kokoro` (English TTS) and `kesha-diarize` (speaker diarization) — are backed by
the **FluidAudio** Swift framework, the same framework the `fluidaudio-rs` crate wraps for
our CoreML ASR backend. We therefore maintain *two hand-rolled SwiftPM packages* that
duplicate, in parallel, what the `fluidaudio-rs` crate is starting to expose natively in
Rust.

The other two sidecars — `say-avspeech` (AVSpeechSynthesizer) and `kesha-textlang`
(NLLanguageRecognizer) — are Apple OS frameworks, not FluidAudio, and **out of scope**
here (eliminating them would require Rust↔ObjC FFI, a separate effort).

Goal: route Kokoro TTS and diarization through a single forked `fluidaudio-rs`, deleting
both FluidAudio Swift sidecars, their `build.rs` blocks, and their release artifacts —
consolidating all FluidAudio Swift into the crate's one bridge.

## Goals / Non-goals

**Goals**
- Replace the `kesha-kokoro` sidecar with a native `fluidaudio-rs` Kokoro call.
- Replace the `kesha-diarize` sidecar with a native `fluidaudio-rs` diarization call **that
  preserves kesha's pinned-hash + `KESHA_MODEL_MIRROR` + explicit-install model governance.**
- Delete `swift/kesha-kokoro/`, `swift/kesha-diarize/`, their `build.rs` swift-build blocks,
  and the `kesha-kokoro-darwin-arm64` / `kesha-diarize-darwin-arm64` release artifacts.
- Upstream the additions to FluidInference/fluidaudio-rs and retire the fork once merged.

**Non-goals**
- `say-avspeech` and `kesha-textlang` sidecars (Apple frameworks; separate FFI track).
- Changing the ONNX Kokoro / Vosk-RU TTS paths used on Linux/Windows.
- Adding pinned-hash governance for the Kokoro model (it auto-downloads today; unchanged).

## Background: investigation findings (spike, 2026-05-20)

Verified against the published `fluidaudio-rs` 0.14.1 source (downloaded from crates.io),
fluidaudio-rs PR #6, and our own tree.

1. **Latest crate versions:** `misaki-rs` 0.3.0 (= our pin, no newer), `ort` 2.0.0-rc.12 (= our
   pin, still no stable 2.0), `fluidaudio-rs` **0.14.1** (we pin 0.1.0 — 13 minor versions behind).
2. **ASR API in 0.14.1 is back-compatible + adds `transcribe_samples`:** `FluidAudio::new()`,
   `init_asr()`, `transcribe_file()->{text,confidence,rtfx,...}` unchanged; `transcribe_samples(&[f32])`
   now published → the temp-WAV shim in `backend/fluidaudio.rs` can be deleted.
3. **No native Kokoro Rust API exists in 0.14.1.** `grep` of `src/`, `examples/`,
   `swift/FluidAudioBridge.swift` for `kokoro|tts|synthesi` → zero hits; the `tts = []` cargo
   feature is an empty placeholder. **PR #6** is "Update FluidAudio to v0.12.6 — Swift 6
   concurrency fixes"; its "Kokoro changes" are inside the upstream FluidAudio Swift framework,
   not new Rust bindings.
4. **Native diarization Rust API DOES exist in 0.14.1:** `init_diarization(threshold: f64)`,
   `diarize_file(path)->Vec<DiarizationSegment{ speaker_id: String, start_time, end_time,
   quality_score }>`, `is_diarization_available()`, plus `examples/diarize.rs`. **But** it takes
   no model path and auto-downloads the model from HuggingFace on first run (README: "First
   initialization downloads … ML models (~500MB)"; bridge: `AsrModels.downloadAndLoad()` /
   "auto-downloaded from HuggingFace on first use").
5. **Model-governance precedent is asymmetric:**
   - **CoreML ASR already auto-downloads** via FluidAudio (no pinned hash, no `kesha install`
     step) — documented accepted exception (`docs/.../2026-04-14-rust-engine-design.md`).
   - **Diarization today is governed by kesha:** `models.rs` `DIARIZE_FILES` pins the Sortformer
     `.mlpackage` with SHA-256, `kesha install --diarize` fetches it verified, and the
     `kesha-diarize` sidecar receives the model **path** as argv. So the stock-crate
     `diarize_file(path)` would be a **regression** (surrendering control we have today).
6. **The crate's build system is compatible with ours:** `build.rs` runs `swift build -c release`,
   links `libFluidAudioBridge.a` (static) + Foundation/AVFoundation/CoreML/Accelerate/Metal/
   MetalPerformanceShaders/swiftCore/c++ — the exact link set our `coreml` backend already needs.
   The `@_cdecl` bridge pattern (`fluidaudio_initialize_diarization`, `fluidaudio_diarize_file`)
   is a clean, repeatable template.
7. **Our Kokoro Swift** (`swift/kesha-kokoro/Sources/kesha-kokoro/main.swift`) drives
   `KokoroTtsManager(defaultVoice:)` → `initialize(preloadVoices:)` →
   `synthesize(text:voice:voiceSpeed:) -> Data` (WAV, 24 kHz mono f32), pinning FluidAudio
   **0.14.5**. The crate pins FluidAudio **0.14.1** — the fork must bump to 0.14.5.

## Design

### The fork: `drakulavich/fluidaudio-rs` (based on 0.14.x → versioned 0.14.5)

Three additive changes, each following the bridge triple
`swift/FluidAudioBridge.swift` (C-exported `@_cdecl`) → `src/ffi/bridge.rs` (`extern "C"` +
safe wrapper) → `src/lib.rs` (public method):

1. **Bump pinned FluidAudio 0.14.1 → 0.14.5** (matches our working Kokoro; crate version
   follows the underlying FluidAudio version per the crate's own AGENTS.md convention).
2. **Add Kokoro TTS binding** — port `main.swift`'s `KokoroTtsManager` calls into the bridge:
   - Swift: `fluidaudio_kokoro_init(ptr, defaultVoice)` + `fluidaudio_kokoro_synthesize(ptr,
     text, voice, speed, &out_samples, &out_len, &out_rate)`.
   - Rust: `init_kokoro() -> Result<()>`, `synthesize_kokoro(text, voice, speed) ->
     Result<KokoroAudio { samples: Vec<f32>, sample_rate: u32 }>`.
   - The bridge returns **raw f32 PCM + sample rate** (not WAV bytes) to avoid an
     encode/decode round-trip; the exact PCM-extraction path from `KokoroTtsManager`
     (`AVAudioPCMBuffer` vs the WAV `Data` it returns) is **spike #1**.
3. **Add a model-path diarization variant** — `fluidaudio_diarize_file_with_models(ptr,
   audio_path, model_dir, …)` that loads our pre-staged Sortformer model instead of
   auto-downloading. Rust: `diarize_file_with_models(audio, model_dir) ->
   Result<Vec<DiarizationSegment>>`. Our current `kesha-diarize/main.swift` already loads from
   an explicit model path (`SortformerConfig.balancedV2`), so the underlying Swift API
   supports it — confirming the binding is **spike #3**.

**Exit strategy:** open the same patch as an upstream PR to FluidInference/fluidaudio-rs;
once merged + released, switch the kesha dep back to the crates.io version and retire the fork.

### kesha-engine consumption

- `rust/Cargo.toml`: `fluidaudio-rs = { git = "https://github.com/drakulavich/fluidaudio-rs",
  rev = "<sha>" }`, enabling `diarization` + `tts` features alongside the existing ASR use.
  `system_kokoro` / `system_diarize` become thin features over the crate rather than
  swift-build gates. (Git-dependency precedent: `kesha-diarize`'s SwiftPM package already pins
  FluidAudio to a git SHA.)
- **Deleted:** `swift/kesha-kokoro/`, `swift/kesha-diarize/`; the `build.rs` blocks that
  `swift build` them (~lines 46–90 and 133–184); the subprocess code in `tts/fluid_kokoro.rs`
  and `transcribe/diarize.rs`; the `kesha-kokoro-darwin-arm64` / `kesha-diarize-darwin-arm64`
  artifact build + upload steps in `build-engine.yml`.
- **Kept:** the coverage-validation + speaker-merge logic in `transcribe/diarize.rs` (only the
  `Command`-spawn is swapped for a crate call); voice IDs/routing in `tts/voices.rs`; the
  `MIN_SAMPLES` ASR padding in `backend/fluidaudio.rs`.

### Model governance

- **Diarization stays governed:** keep `DIARIZE_FILES` pinned-SHA in `models.rs`, keep
  `kesha install --diarize` fetching the verified `.mlpackage`, feed that path to
  `diarize_file_with_models`. Pinned-hash + `KESHA_MODEL_MIRROR` + explicit-install are
  **preserved** — no surprise download, no regression.
- **Kokoro** uses `KokoroTtsManager`'s auto-downloaded model — the **status quo** (the sidecar
  already does this), documented as the same accepted exception as CoreML ASR. A pinned/
  model-path Kokoro variant is a possible follow-up, not part of this work.

### Data flow (after)

- `kesha say --voice en-am_michael` → `tts::say` → `EngineChoice::FluidKokoro` →
  `fluidaudio_rs::synthesize_kokoro(text, voice, speed)` → f32 PCM → existing `wav::encode_wav`.
  No subprocess.
- `kesha transcribe --with-speakers` → ASR → `fluidaudio_rs::diarize_file_with_models(audio,
  pinned_model_dir)` → `Vec<DiarizationSegment>` → existing coverage-validation + speaker-merge.
  No subprocess.

### Build, CI & error handling

- `build-engine.yml` darwin row keeps
  `coreml,tts,system_tts,system_kokoro,system_diarize,system_text_lang`. The crate's `build.rs`
  now performs the Swift compile/link for Kokoro + diarize in one bridge (was two SwiftPM
  builds). Swift toolchain requirement is unchanged.
- Keep the `--capabilities-json` pre-upload smoke gate; the macOS smoke must still produce
  `en-am_michael` audio and a `transcribe.diarize` capability.
- Errors: missing diarization model → existing actionable `kesha install --diarize` error (no
  auto-download); Kokoro / ASR init failure → contextful `anyhow` error matching peer modules
  (`.context(...)?`, per the `ort` style note in CLAUDE.md).

### Validation spikes (gate the implementation)

Per the repo's "verify third-party model formats with a spike" rule, three build-spikes must
pass on `macos-14` arm64 before committing the migration:

1. **Kokoro bridge** — `KokoroTtsManager` PCM extraction returns usable f32 @ 24 kHz from Rust;
   `audio-quality-check` agent compares output against the current sidecar's WAV.
2. **build.rs coexistence** — the fork's single Swift link satisfies `coreml` ASR **and**
   Kokoro **and** diarize in one binary; no duplicate-symbol / framework conflicts.
3. **Diarization model-dir loading** — the FluidAudio Swift diarizer loads our pinned
   `.mlpackage` from an explicit dir with **no** network download.

Spike artifacts live in `/tmp/<name>-spike/` and are deleted once findings are recorded.

## Sequencing

- **PR 1 (in flight, branch `chore/cargo-dep-bumps`):** bump `fluidaudio-rs 0.1 → 0.14.1`
  (crates.io) for ASR; drop the temp-WAV shim in favor of `transcribe_samples`; fold in the
  routine in-range lockfile refresh + `cargo audit` findings. Low-risk; de-risks the 0.14 ASR
  API independently before the fork lands.
- **PR 2 (this design):** switch the dep to the fork (git rev); add the Kokoro + model-path
  diarization bindings; migrate `tts/fluid_kokoro.rs` and `transcribe/diarize.rs` off the
  subprocesses; delete both SwiftPM packages, `build.rs` blocks, and release artifacts. Open
  the upstream PR to FluidInference in parallel.

## Risks & open questions

- **Fork maintenance burden** — mitigated by upstreaming and retiring the fork. Until then we
  own keeping it in sync with FluidAudio releases.
- **Git dependency build cost** — `cargo` builds the fork from source and runs `swift build`;
  CI build time on `macos-14` increases. (Already true for the sidecars today.)
- **KokoroTtsManager API stability across FluidAudio 0.14.1↔0.14.5** — resolved by spike #1.
- **Single-binary Swift linking** — resolved by spike #2; risk of duplicate FluidAudio symbols
  if any residual sidecar linking lingers (it shouldn't, since both SwiftPM packages are deleted).
- **iOS** — the crate targets macOS 14 + iOS 17; we only ship macOS arm64, so iOS is irrelevant.

## Decision log

- Scope = **FluidAudio pair only** (Kokoro + diarize); AVSpeech + text-lang excluded.
- Kokoro has no upstream Rust API → **fork** `fluidaudio-rs` to add it (vs. dropping to the
  in-Rust ONNX Kokoro path or deferring). Upstream-and-retire is the end state.
- Diarization model auto-download is unacceptable (it would regress existing governance) →
  fork adds a **model-path variant** so kesha keeps pinned-hash + mirror + explicit-install.
- Sequenced as two PRs: the safe ASR 0.14 bump first, then the fork-unify.

## References

- Upstream crate: https://github.com/FluidInference/fluidaudio-rs (PR #6: FluidAudio v0.12.6 /
  Swift 6 concurrency — *not* a Rust Kokoro API).
- Upstream framework: https://github.com/FluidInference/FluidAudio (`KokoroTtsManager`,
  `SortformerDiarizer`).
- Our files: `rust/src/backend/fluidaudio.rs`, `rust/src/tts/fluid_kokoro.rs`,
  `rust/src/transcribe/diarize.rs`, `rust/src/models.rs` (`DIARIZE_FILES`), `rust/build.rs`,
  `swift/kesha-kokoro/`, `swift/kesha-diarize/`, `.github/workflows/build-engine.yml`.
- Prior art: `docs/superpowers/specs/2026-05-09-darwin-diarization-poc-design.md`.
