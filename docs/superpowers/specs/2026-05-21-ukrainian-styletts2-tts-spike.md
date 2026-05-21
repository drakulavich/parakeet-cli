# Ukrainian StyleTTS2 TTS Spike

## Decision

Add Ukrainian TTS as a narrow experimental `uk-*` voice only after we mirror a
fixed single-speaker ONNX artifact and port the Ukrainian text-preprocessing
boundary. Do not integrate the Hugging Face Space as a Python sidecar.

The first viable slice is:

- `uk-styletts2-filatov` as an explicit voice id.
- Single-speaker `patriotyk/styletts2_ukrainian_single`.
- Fixed ONNX + raw style vector in Kesha-controlled model storage.
- No multispeaker model, no M2M100 verbalizer, and no auto-routing default in
  the first PR.

## Sources Checked

- Space: <https://huggingface.co/spaces/patriotyk/styletts2-ukrainian>
  (`sha=b02909e4c9f001865bf71633d76fee7110f657a3`, MIT).
- Single-speaker model:
  <https://huggingface.co/patriotyk/styletts2_ukrainian_single>
  (`sha=2646553e1f9a8c832480e3ad5ccb6839245af584`, MIT).
- Multispeaker alias resolves to
  <https://huggingface.co/patriotyk/styletts2_ukrainian_multispeaker_hifigan>
  (`sha=dcca8ce02382d91d95123c93f8b01bc90fe17cfc`, MIT).
- Inference library: <https://github.com/patriotyk/styletts2-inference>, MIT,
  described as ONNX-compatible.

## Space Pipeline

The Space runs:

```text
text -> optional verbalizer -> stressifier -> ipa_uk.ipa()
     -> char tokenizer from config.yml -> StyleTTS2 -> 24 kHz WAV
```

Important runtime dependencies in the Space:

- `torch==2.8.0`
- `torchaudio==2.8.0`
- `styletts2_inference`
- `ipa_uk`
- `ukrainian_word_stress`
- optional M2M100/CTranslate2 verbalizer for numbers and acronyms

The Python sidecar shape is a poor Kesha fit: the throwaway local venv for the
smoke was about 900 MB, and the optional verbalizer model alone is about
1.94 GB. Kesha should stay with explicit model install, pinned hashes, and the
Rust engine runtime.

## Artifact Findings

Single-speaker model:

- `model.onnx`: 327,779,591 bytes,
  `sha256=e3dbd52d5a2372edfc20fce54ccac8ab951c95143832e409a252ae61df1a6413`
- `pytorch_model.bin`: 748,848,243 bytes
- `style.pt`: 2,204 bytes,
  `sha256=f181646626df52fdcf749e93a311686ffb2eaeae8112be0005a8d6efa7dc5cc9`

Multispeaker model:

- `pytorch_model.bin`: 766,654,558 bytes
- no ONNX artifact in the model repo

Verbalizer:

- CTranslate2 `model.bin`: 1,939,838,415 bytes
- tokenizer model: about 2.4 MB

Preprocessing packages:

- `ipa_uk`: about 15 KB of Python rules.
- `ukrainian_word_stress`: includes a roughly 12 MB `stress.trie`.

## Smoke Evidence

Smoke text:

```text
Привіт, світе. Це тест українського синтезу мовлення.
```

Preprocessing produced:

```text
Приві́т, сві́те. Це́ те́ст украї́нського си́нтезу мо́влення.
prɪˈʋʲit, ˈsʲʋʲite. ˈt͡sɛ ˈtɛst ʊkrɐˈjinʲsʲkɔɦɔ ˈsɪntezʊ ˈmɔu̯ɫenʲːɐ.
```

Token count: 69.

PyTorch path:

- Cold model load: 27.24 seconds on local CPU.
- Inference: 0.83 seconds.
- Output: 24 kHz, 4.4 seconds.
- Basic audio stats: RMS 0.0756, peak 0.4796, not silent or clipped.

Published ONNX path:

- Raw upstream ONNX failed to load in `onnxruntime 1.19.2`:

  ```text
  Invalid attribute perm {1, -1, 0}
  ```

- Failure location: `/text_encoder_1/Transpose_7`.
- Mechanical patch changed one Transpose perm from `[1, -1, 0]` to `[1, 2, 0]`.
- Patched ONNX passed `onnx.checker` and loaded in ORT.
- Patched ONNX session load: 0.507 seconds.
- Patched ONNX inference: 0.977 seconds.
- Output: 24 kHz, 4.4 seconds.
- Basic audio stats: RMS 0.0752, peak 0.4520, not silent or clipped.
- Patched ONNX SHA-256:
  `fae6e3e7a1152138214bbc314db68443d2ff2ed8c84588328f8d054f3fae2a37`.

## Implementation Shape

Do not cache or distribute upstream `style.pt` directly. Convert it once into a
raw little-endian f32 `[1, 256]` style vector so runtime code can read it without
PyTorch.

The first implementation PR should:

- Mirror a fixed ONNX and raw `style.bin` in Kesha-controlled model storage.
- Pin model hashes in `rust/src/models.rs`.
- Add a `StyleTtsUk` engine arm selected by `uk-*` voice ids.
- Add cache layout checks and `kesha install --tts` manifest entries for the
  Ukrainian bundle.
- Port or vendor the Ukrainian preprocessing path. The practical boundary is
  `stress + IPA -> tokenizer ids`; either port the rules/trie to Rust or use a
  narrow helper binary, but do not ship a general Python runtime.
- Add `uk-styletts2-filatov` as an explicit experimental voice.
- Keep numbers/acronyms out of v1 or implement a simple deterministic
  normalizer; do not pull in the M2M100 verbalizer.

Do not auto-route Ukrainian text to this voice in v1. Kesha's default TTS voice
policy requires careful male-default review, and the Space's `filatov` voice
needs a listening pass before it can be treated as a brand default.

## Acceptance Criteria For The First Code PR

- `kesha install --tts` installs the Ukrainian StyleTTS2 bundle explicitly.
- `kesha say --voice uk-styletts2-filatov "Привіт, світе." --out uk.wav`
  produces a non-empty 24 kHz WAV.
- Missing Ukrainian model files fail fast with a `kesha install --tts` hint.
- `kesha say --list-voices` includes the new explicit `uk-*` voice only when
  installed or supported by the build's voice catalogue.
- Unit tests cover voice resolution, missing cache errors, tokenizer handling,
  and preprocessing boundary behavior.
- A smoke/audio-stats check confirms the Ukrainian output is mono, non-silent,
  unclipped, and plausibly sized for the input text.
