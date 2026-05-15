# FluidAudio KokoroAne spike

Date: 2026-05-15

## Question

Can Kesha use FluidAudio's Kokoro CoreML/ANE TTS backend on macOS instead of the current Kokoro ONNX path?

## Current Kesha behavior

Kesha's `en-*` voices currently use:

- model: `~/.cache/kesha/models/kokoro-82m/model.onnx`
- voice embedding: `~/.cache/kesha/models/kokoro-82m/voices/<voice>.bin`
- runtime: `ort::Session` in `rust/src/tts/kokoro.rs`
- default voice: `en-am_michael`
- G2P: embedded `misaki-rs`, with Kesha's acronym and SSML segment handling layered before inference

FluidAudio is used by Kesha for the macOS CoreML ASR backend, not for Kokoro TTS.

## Upstream facts checked

FluidAudio `main` has Kokoro TTS:

- `README.md` documents TTS and `KokoroAne`.
- `Documentation/TTS/KokoroAne.md` documents the 7-stage ANE/GPU Kokoro path.
- `Sources/FluidAudio/TTS/KokoroAne/KokoroAneManager.swift` exposes `KokoroAneManager`.
- `Sources/FluidAudio/TTS/TtsBackend.swift` has `case kokoroAne`.

`fluidaudio-rs` 0.14.1 is not enough to call it directly from Rust today:

- `cargo info fluidaudio-rs` reports version `0.14.1` with feature `tts = []`.
- The published Rust API mentions TTS in package docs, but `src/lib.rs` exposes ASR, streaming ASR, Qwen3 ASR, VAD, diarization, ITN, and system info only.
- The published FFI bridge has no `fluidaudio_*tts*`, `fluidaudio_*kokoro*`, or synth export.
- Therefore Kesha cannot switch to FluidAudio Kokoro by only enabling a crate feature.

## Constraints

Kesha constraints from `CLAUDE.md` still apply:

- TTS models must not auto-download during `kesha say`.
- `kesha install --tts` is the explicit install boundary.
- Default TTS voices must be male.
- Released darwin engine builds already use `coreml,tts,system_tts`; any new feature must be wired into release matrix explicitly.
- Rust tests must use `cargo nextest`.

FluidAudio KokoroAne constraints from upstream docs/code:

- macOS/iOS Apple Silicon path only.
- Output is 24 kHz mono audio.
- Current KokoroAne docs list English and Mandarin variants, but the README beta note says TTS currently supports American English only; treat Mandarin as experimental until verified in Kesha.
- English default voice upstream is `af_heart` (female), which conflicts with Kesha's male default voice rule.
- KokoroAne docs say single voice per variant (`af_heart` / `zf_001`) and no SSML/custom lexicon support in the ANE path.
- First synthesis downloads model assets into FluidAudio's cache under `~/.cache/fluidaudio/Models/kokoro/`, which conflicts with Kesha's explicit install/cache ownership unless wrapped.

## Options

### Option A: wait for upstream `fluidaudio-rs` TTS bindings

Pros:

- Lowest maintenance if upstream exposes a stable Rust API for KokoroAne.
- Avoids a Kesha-owned Swift FFI surface.

Cons:

- Not actionable in Kesha today.
- Still likely needs cache/install control, voice choice, and output contract validation.

### Option B: add a Kesha Swift sidecar for FluidAudio KokoroAne

Pros:

- Matches Kesha's existing `say-avspeech` sidecar pattern.
- Can keep Rust TTS routing stable while isolating Swift async/CoreML details.
- Gives us direct control over CLI contract: stdin text, argv voice/lang/rate, stdout WAV.

Cons:

- Needs a new sidecar binary in release artifacts.
- Needs explicit `install --tts` prefetch strategy or a fail-fast "not installed" mode to preserve no-auto-download.
- Upstream default voice is female, so this cannot replace `en-am_michael` unless we verify a male KokoroAne voice is available and loadable.

### Option C: vendor or fork `fluidaudio-rs` TTS bindings

Pros:

- Keeps Rust call sites cleaner than a subprocess sidecar.
- Could upstream later.

Cons:

- Highest integration risk.
- Kesha would own Swift FFI safety, memory ownership, async bridging, and cache behavior inside the Rust crate dependency path.

## Recommendation

Use Option B for the next implementation PR: a macOS-only `say-kokoro-ane` Swift sidecar spike behind an explicit feature/voice id, not as the default English route.

Start with a non-default voice id such as `en-kokoro-ane-af_heart` or `macos-kokoro-ane-af_heart` to make the experiment explicit and avoid silently violating the male default rule. Only promote it to default if a male FluidAudio KokoroAne voice is verified by synthesis quality and upstream asset availability.

## Implementation shape for follow-up PR

1. Add a tiny Swift sidecar target that imports FluidAudio and calls `KokoroAneManager`.
2. Make the sidecar support:
   - stdin text
   - `--voice <voice>`
   - `--lang <lang>` initially `en` only
   - `--rate <float>` if upstream speed maps cleanly
   - WAV bytes on stdout
3. Add a Rust `EngineChoice::KokoroAne` behind `system_tts` + darwin.
4. Resolve explicit `en-kokoro-ane-*` voice ids to that engine only on macOS.
5. Keep default `en` routing on existing ONNX Kokoro until male voice parity and install semantics are solved.
6. Add `kesha install --tts` preflight/prefetch behavior or a separate `kesha install --tts-kokoro-ane` flag if FluidAudio assets cannot be downloaded deterministically by Kesha.
7. Add smoke tests that compile the sidecar in CI; gate real synthesis on macOS Apple Silicon with model cache available.

## Non-goals for the follow-up

- Do not replace Kokoro ONNX globally.
- Do not auto-download FluidAudio Kokoro assets during `kesha say`.
- Do not route `--lang de`/Chatterbox languages to FluidAudio KokoroAne.
- Do not claim SSML support for KokoroAne; upstream docs say this path has no SSML.

## Open questions

- Does `FluidInference/kokoro-82m-coreml` contain a male English voice that works with the 7-stage ANE path?
- Can FluidAudio's asset downloader be pointed at Kesha's cache directory, or do we accept a FluidAudio-owned cache and make `install --tts` warm it explicitly?
- Can the sidecar disable first-run downloads and report a deterministic missing-model error?
- What is cold-start latency vs Kesha's current ONNX Kokoro on Apple Silicon?
- Does `KokoroAneManager.synthesizeDetailed` provide enough timing/debug data to replace Kesha's current `KESHA_DEBUG` signals?

## Spike conclusion

FluidAudio KokoroAne is real and attractive for macOS performance, but it is not a drop-in replacement through `fluidaudio-rs` today. The safest next PR is a macOS-only Swift sidecar experiment with explicit voice ids and explicit install semantics.
