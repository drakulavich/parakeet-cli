<p align="center">
  <img src="assets/logo.png" alt="Kesha Voice Kit" width="200">
</p>

<h1 align="center">Kesha Voice Kit</h1>

<p align="center">
  <a href="https://flakiness.io/Laputa/kesha-voice-kit"><img src="https://img.shields.io/endpoint?url=https%3A%2F%2Fflakiness.io%2Fapi%2Fbadge%3Finput%3D%257B%2522badgeToken%2522%253A%2522badge-2IKMRRqUxh9P3w8Ym3Szf0%2522%257D" alt="Tests"></a>
  <a href="https://www.npmjs.com/package/@drakulavich/kesha-voice-kit"><img src="https://img.shields.io/npm/v/@drakulavich/kesha-voice-kit" alt="npm version"></a>
  <a href="https://opensource.org/licenses/MIT"><img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="License: MIT"></a>
  <a href="https://bun.sh"><img src="https://img.shields.io/badge/runtime-Bun-f9f1e1?logo=bun" alt="Bun"></a>
</p>

<p align="center"><b>Open-source voice toolkit.</b> Optimized for Apple Silicon (CoreML), works on any platform (ONNX fallback).<br>A collection of small, fast, open-source audio models — packaged as CLI tools and an <a href="https://github.com/openclaw/openclaw">OpenClaw</a> skill for LLM agents.</p>

- **Speech-to-text** — 25 languages, ~15x faster than Whisper on Apple Silicon, ~2.5x on CPU
- **Text-to-speech** — Kokoro (EN) + Vosk-TTS (RU) + macOS system voices, SSML preview
- **Rust engine** — single 20MB binary, no ffmpeg, no Python, no native Node addons
- **OpenClaw-ready** — plug into your LLM agent as a voice processing skill

## Quick Start

Runtime: **[Bun](https://bun.sh)** >= 1.3.0.

Install Bun (skip if already installed) — pick one:

```bash
# Linux & macOS
curl -fsSL https://bun.sh/install | bash       # upstream installer
brew install oven-sh/bun/bun                   # Homebrew
```

```powershell
# Windows
powershell -c "irm bun.sh/install.ps1 | iex"
```

Then install Kesha:

```bash
bun add -g @drakulavich/kesha-voice-kit
kesha install       # downloads engine + models
kesha audio.ogg     # transcript to stdout
```

Air-gapped or behind a corporate mirror? See [docs/model-mirror.md](docs/model-mirror.md).

## Nix Install

Alternative reproducible-build path on `aarch64-darwin` / `x86_64-linux`:

```bash
nix run github:drakulavich/kesha-voice-kit -- install      # downloads models (engine is bundled)
nix run github:drakulavich/kesha-voice-kit -- audio.ogg    # transcribe
```

Full recipes (one-liner, profile install, engine-only, dev shell) live in [docs/nix-install.md](docs/nix-install.md).

## Speech-to-text

```bash
kesha audio.ogg                            # transcribe (plain text)
kesha --format transcript audio.ogg        # text + language/confidence
kesha --format json audio.ogg              # full JSON with lang fields
kesha --json --timestamps audio.ogg        # JSON with timestamped segments
kesha --toon audio.ogg                     # compact LLM-friendly TOON
kesha --verbose audio.ogg                  # show language detection details
kesha --lang en audio.ogg                  # warn if detected language differs
kesha status                               # show installed backend info
```

Multiple files — headers per file, like `head`:

```bash
$ kesha freedom.ogg tahiti.ogg
=== freedom.ogg ===
Свободу попугаям! Свободу!

=== tahiti.ogg ===
Таити, Таити! Не были мы ни в какой Таити! Нас и тут неплохо кормят.
```

Stdout: transcript. Stderr: errors. Pipe-friendly. Also available as `parakeet` command (backward-compatible alias).

For long / silence-heavy audio, use `--vad` (auto-on past 120 s). Details: [docs/vad.md](docs/vad.md).

**Speaker diarization** (darwin-arm64, post-v1.12.0):

```bash
kesha install --diarize                        # one-time, ~245MB Sortformer model
kesha --json --vad --speakers meeting.m4a > out.json
jq '.[0].segments[] | "\(.speaker)\t\(.text)"' out.json
```

Each segment gets a `speaker` integer (cluster ID, stable within one file). Linux / Windows: `--speakers` returns a clear "currently darwin-arm64 only" error — see [#199](https://github.com/drakulavich/kesha-voice-kit/issues/199).

## Text-to-speech

Kesha speaks back via Kokoro-82M (English) and Vosk-TTS (Russian) — voice auto-picks from the text's language:

```bash
kesha install --tts                      # ~990MB (Kokoro + Vosk-TTS RU, opt-in)
kesha say "Hello, world" > hello.wav
kesha say "Привет, мир" > privet.wav     # auto-routes (Milena on darwin, ru-vosk-m02 elsewhere)
```

**Russian abbreviations** (`ru-vosk-*`): all-uppercase Cyrillic 2-5-char tokens auto-expand letter-by-letter when not pronounceable as a Russian syllable (ФСБ → "эф-эс-бэ", ВОЗ → "воз"). Disable with `--no-expand-abbrev`. See [docs/tts.md#russian-abbreviation-auto-expansion](docs/tts.md#russian-abbreviation-auto-expansion).

**English acronyms** (`en-*`, Kokoro): three-table mechanism (letter-spell rule + STOP_LIST + IPA_LEXICON) auto-expands FBI → "ef bee eye" and gives EPAM/JSON/Anthropic the right IPA. Disable letter-spell with `--no-expand-abbrev`. See [docs/tts.md#english-acronym-auto-expansion](docs/tts.md#english-acronym-auto-expansion).

**Russian word stress** (`ru-vosk-*` voices):

```bash
# Caller provides `+` before the stressed vowel; engine passes it to Vosk
kesha say --voice ru-vosk-m02 --ssml \
  '<speak><emphasis>дом+а</emphasis></speak>'   # genitive до-МА́

# Suppress an inherited <emphasis> with level="none"
kesha say --voice ru-vosk-m02 --ssml \
  '<speak><emphasis level="none">дом+а</emphasis></speak>'   # default ДО́ма
```

Vosk-TTS 0.9-multi honors a `+` placed BEFORE the target stressed vowel — but only when the marker shifts stress AWAY from the model's default (first-syllable). `+` agreeing with the default is a no-op. See [#233](https://github.com/drakulavich/kesha-voice-kit/issues/233).

**Speech rate via SSML** (`ru-vosk-*` and `en-*` voices):

```bash
kesha say --voice ru-vosk-m02 --ssml \
  '<speak><prosody rate="slow">Привет, как дела.</prosody></speak>' --out slow.wav

kesha say --voice en-am_michael --ssml \
  '<speak><prosody rate="x-fast">Read this fast.</prosody></speak>' --out fast.wav
```

Honored when `<prosody rate>` wraps the whole utterance. Mid-utterance prosody warns and synthesizes at default rate (whole-segment-only is a v1 limitation; mid-utterance support tracked in [#236](https://github.com/drakulavich/kesha-voice-kit/issues/236)). `--rate` and `<prosody rate>` compose multiplicatively. Range clamped to 0.5×–2.0×.

macOS system voices, SSML, voice listing, and the full voice catalogue: [docs/tts.md](docs/tts.md).

## Performance

> **~15x faster than Whisper** on Apple Silicon (M3 Pro), **~2.5x faster** on CPU

Compared against Whisper `large-v3-turbo` — all engines auto-detect language.

![Benchmark: openai-whisper vs faster-whisper vs Kesha Voice Kit](assets/benchmark.svg)

See [BENCHMARK.md](BENCHMARK.md) for the full per-file breakdown (Russian + English).

## What's Inside

| Model | Task | Size | Source |
|---|---|---|---|
| NVIDIA Parakeet TDT 0.6B v3 | Speech-to-text | ~2.5GB | [HuggingFace](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3) |
| SpeechBrain ECAPA-TDNN | Audio language detection | ~86MB | [HuggingFace](https://huggingface.co/speechbrain/lang-id-voxlingua107-ecapa) |
| Apple NLLanguageRecognizer | Text language detection | built-in | macOS system framework |
| Silero VAD v5 (opt-in) | Voice activity detection | ~2.3MB | [snakers4/silero-vad](https://github.com/snakers4/silero-vad) |
| Kokoro-82M / Vosk-TTS (opt-in) | Text-to-speech | ~990MB | [Kokoro](https://huggingface.co/hexgrad/Kokoro-82M) · [Vosk-TTS](https://github.com/alphacep/vosk-tts) |

All models run through `kesha-engine` — a Rust binary using [FluidAudio](https://github.com/FluidInference/FluidAudio) (CoreML) on Apple Silicon and [ort](https://github.com/pykeio/ort) (ONNX Runtime) on other platforms.

Audio decoding via [symphonia](https://github.com/pdeljanov/Symphonia) — WAV, MP3, OGG/Opus, FLAC, AAC, M4A. No ffmpeg.

## Languages

- **Speech-to-text (25):** Bulgarian, Croatian, Czech, Danish, Dutch, English, Estonian, Finnish, French, German, Greek, Hungarian, Italian, Latvian, Lithuanian, Maltese, Polish, Portuguese, Romanian, Russian, Slovak, Slovenian, Spanish, Swedish, Ukrainian.
- **Audio language detection (107):** [full list](https://huggingface.co/speechbrain/lang-id-voxlingua107-ecapa).

## Integrations

- **OpenClaw** — give your LLM agent ears. Install & config: [docs/openclaw.md](docs/openclaw.md).
- **Raycast** (macOS) — transcribe selected audio & speak clipboard from the launcher. Source + install: [`raycast/`](raycast/).

## Programmatic API

```typescript
import { transcribe, downloadModel } from "@drakulavich/kesha-voice-kit/core";

await downloadModel();                       // install engine + models
const text = await transcribe("audio.ogg");  // transcribe
```

## Requirements

- [Bun](https://bun.sh) >= 1.3
- macOS arm64, Linux x64, or Windows x64

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Made with 💛🩵 and 🥤 energy under MIT License
