# sherpa-onnx Supertonic 3 TTS Spike

Date: 2026-05-15

## Goal

Evaluate whether Kesha should add a TTS backend using
[k2-fsa/sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) with
[SupertonicTTS 3](https://k2-fsa.github.io/sherpa/onnx/tts/supertonic.html).

This is a spike, not an implementation PR. The output is the recommended
integration path, known risks, and a concrete follow-up slice.

## Verdict

Proceed to a narrow implementation PR, but keep it behind an explicit
Supertonic/sherpa backend path instead of replacing Kokoro or Chatterbox.

sherpa-onnx is materially more viable than the earlier FluidAudio Kokoro path:

- The published Rust crate (`sherpa-onnx = 1.13.2`) exposes a safe offline TTS
  API and a dedicated `OfflineTtsSupertonicModelConfig`.
- The official Supertonic page documents 31 supported language codes, `--sid`
  speaker selection, and `--lang` synthesis-language selection.
- The released model archive is ONNX-based and pinned by GitHub release asset
  digest, so it matches Kesha's existing explicit-install and SHA-verified model
  policy.
- The upstream Rust example ran end-to-end locally and produced a valid WAV.

Do not make it the default backend yet. The unresolved product risk is voice
quality/default voice mapping: the model is multi-speaker, but the upstream
archive exposes numeric `sid` values rather than a documented gender/quality
catalogue. Kesha's default voice must be male, so the first implementation
should require an explicit Supertonic voice or ship only audited per-language
defaults.

## Upstream Facts Checked

Official documentation:

- sherpa-onnx supports TTS among other local speech tasks and lists Rust as a
  supported API surface.
- The SupertonicTTS page says SupertonicTTS 3 is offline, multi-speaker,
  multi-language, supports 31 languages, and is used by selecting `--sid` and
  `--lang`.
- The model archive is:
  `sherpa-onnx-supertonic-3-tts-int8-2026-05-11.tar.bz2`.
- The release asset is 128,774,318 bytes with digest
  `sha256:82fa96f91c4ef8abaae3a14a3f4153facf88bed821d1f7331cec2700f432c427`.
- The unpacked model directory is about 145 MB.

Local source inspection:

- `/tmp/sherpa-onnx-spike/rust-api-examples/examples/supertonic_tts.rs` creates
  `OfflineTts` with `OfflineTtsSupertonicModelConfig` and calls
  `generate_with_config`.
- The Rust crate exposes:
  - `OfflineTtsSupertonicModelConfig`
  - `OfflineTtsModelConfig.supertonic`
  - `OfflineTtsConfig`
  - `GenerationConfig { sid, speed, num_steps, extra }`
- The C++ implementation validates `lang` against the same 31-code allow-list
  and returns an error for unsupported languages.
- `voice.bin` is parsed as a multi-speaker style table; `sid` is clamped to the
  available speaker range, with invalid IDs falling back to `0`.
- The archive includes an MIT `LICENSE` from Supertone Inc.

End-to-end run:

```text
cd /tmp/sherpa-onnx-spike/rust-api-examples
./run-supertonic-tts.sh
```

Result:

```text
Sample rate: 44100
Num speakers: 10
Progress: 100.0%
Elapsed seconds: 2.658 s
Audio duration: 12.496 s
Real-time factor (RTF): 0.213
Saved to: ./generated-supertonic-en-rust.wav
```

The generated file was a valid mono 44.1 kHz WAV:

```text
RIFF (little-endian) data, WAVE audio, Microsoft PCM, 16 bit, mono 44100 Hz
```

Cold example build note: Cargo built the upstream example in about 28 seconds
after downloading crates; the example `target/` directory was about 459 MB.
That is acceptable for a spike but needs explicit release-size verification
before this enters the production engine.

## Supported Languages

SupertonicTTS 3 via sherpa-onnx supports these language codes:

`ar`, `bg`, `hr`, `cs`, `da`, `nl`, `en`, `et`, `fi`, `fr`, `de`, `el`, `hi`,
`hu`, `id`, `it`, `ja`, `ko`, `lv`, `lt`, `pl`, `pt`, `ro`, `ru`, `sk`, `sl`,
`es`, `sv`, `tr`, `uk`, `vi`.

Notable product implication: `uk` is supported here. If Kesha keeps Chatterbox
for the broad non-English path, Supertonic can still be useful for Ukrainian or
as an alternative multilingual backend.

## Model Files

The archive contains a single multilingual bundle:

- `duration_predictor.int8.onnx`
- `text_encoder.int8.onnx`
- `vector_estimator.int8.onnx`
- `vocoder.int8.onnx`
- `tts.json`
- `unicode_indexer.bin`
- `voice.bin`

This matters for install DX: users should not download per-language packs.
`kesha install --tts` can install one Supertonic bundle and then route supported
languages via `--lang`.

## Recommended Architecture

Add a Rust TTS backend variant named `SherpaSupertonic` or `SupertonicSherpa`.

The backend should:

1. Resolve a cached model directory, e.g.
   `$KESHA_CACHE_DIR/models/sherpa-supertonic-3/`.
2. Construct `OfflineTtsConfig` with all seven Supertonic file paths.
3. Create `OfflineTts` once per process/backend instance.
4. Convert Kesha request options into `GenerationConfig`:
   - `voice sid` -> `sid`
   - `--lang` / routing language -> `extra["lang"]`
   - `--rate` / SSML whole-utterance prosody -> `speed`
   - conservative default `num_steps = 8`, matching upstream example
5. Save or stream generated samples as Kesha's existing WAV output path expects.

Prefer the safe `sherpa-onnx` Rust crate for the first implementation. A
sidecar CLI remains a fallback if static linking or binary size becomes painful,
but starting with the Rust API gives better error handling and avoids another
process protocol.

## Voice IDs and Routing

Use explicit voice IDs for the first cut:

- `supertonic-<lang>-s<sid>` or `<lang>-supertonic-s<sid>`
- Example: `de-supertonic-s0006`

`--voice` should continue to win over `--lang`, but Supertonic needs both pieces
internally: the voice picks the engine and `sid`; the language tag goes into
`GenerationConfig.extra["lang"]`.

Default routing should remain conservative:

- `en` continues to route to Kokoro by default.
- Existing Russian zero-install macOS behavior should not change.
- Supertonic should not become an automatic default until male speaker IDs are
  audited by listening tests and recorded in the routing dictionary.
- Unsupported `--lang` values should reach the engine resolver instead of TS
  inventing a voice. sherpa-onnx already reports invalid Supertonic languages.

## Install DX

Kesha's "never auto-download" rule still applies.

Follow-up implementation should add one explicit install path:

```bash
kesha install --tts --engine supertonic
```

or, if the CLI stays single-switch:

```bash
kesha install --tts
```

with clear output that the Supertonic bundle is included. The model manifest
must pin the archive SHA-256 and unpack into the cache. `kesha say` must fail
with a direct install hint when the bundle is absent.

Recommended user-facing error:

```text
Supertonic TTS model is not installed.
Run: kesha install --tts
```

## SSML

sherpa-onnx Supertonic consumes plain text plus generation options; it does not
provide native SSML support. Kesha should keep SSML parsing outside the backend:

- `<break>` can keep using Kesha's existing segment/silence pipeline.
- Whole-utterance `<prosody rate>` can map to `GenerationConfig.speed`.
- `<say-as>`, `<phoneme>`, and Russian stress markers should be treated as
  unsupported for Supertonic unless a later spike proves a reliable text-side
  transformation.

The first implementation should warn or reject unsupported SSML features rather
than silently synthesize incorrect text.

## Risks

1. **Binary size and static linking.** `sherpa-onnx` defaults to the `static`
   feature. The upstream example built quickly, but its local `target/`
   directory was about 459 MB. The first implementation must check final
   darwin-arm64, linux-x64, and windows-x64 release sizes and startup behavior.
2. **ORT duplication.** Kesha already uses the `ort` crate for ASR/lang-id.
   sherpa-onnx brings its own ONNX Runtime integration through its C/C++ layer.
   Verify linking and deployment on all release targets.
3. **Voice catalogue.** Upstream exposes numeric `sid`; Kesha needs male default
   voices. Do not default-route languages until audited speaker IDs exist.
4. **License/distribution.** sherpa-onnx is Apache-2.0 and the unpacked
   Supertonic archive includes an MIT license. Still review attribution and
   mirror requirements before adding the model to Kesha's install manifest.
5. **Performance.** The upstream example uses `num_steps = 8`; Kesha should
   benchmark RTF for short and long text before making this the main multilingual
   path.

## Follow-up Implementation Slice

Keep the next PR intentionally small:

1. Add `sherpa-onnx` as an optional Rust dependency behind a Cargo feature.
2. Add model manifest entries for the Supertonic archive and unpacked file
   checks.
3. Add a `SherpaSupertonic` TTS backend that works for one explicit voice:
   `en-supertonic-s0006` or `de-supertonic-s0006`.
4. Wire `--voice <lang>-supertonic-sNNNN` and `--lang` through to
   `GenerationConfig.extra["lang"]`.
5. Add tests for missing-model errors, voice ID parsing, supported language
   routing, and unsupported language pass-through/error behavior.
6. Verify with:
   - `bun test`
   - `bunx tsc --noEmit`
   - `bun run check:versions`
   - `git diff --check`
   - `cd rust && cargo fmt --check`
   - `cd rust && cargo clippy --all-targets --features tts -- -D warnings`
   - `cd rust && cargo nextest run --features tts`

## Non-goals

- Replacing Kokoro defaults.
- Replacing Chatterbox work already in progress.
- Auto-downloading the model from `kesha say`.
- Claiming a default male Supertonic voice before speaker IDs are audited.
