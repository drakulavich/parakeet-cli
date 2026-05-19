<p align="center">
  <img src="docs/assets/logo.png" alt="Kesha Voice Kit" width="200">
</p>

<h1 align="center">Kesha Voice Kit</h1>

<p align="center">
  <a href="https://flakiness.io/Laputa/kesha-voice-kit"><img src="https://img.shields.io/endpoint?url=https%3A%2F%2Fflakiness.io%2Fapi%2Fbadge%3Finput%3D%257B%2522badgeToken%2522%253A%2522badge-2IKMRRqUxh9P3w8Ym3Szf0%2522%257D" alt="Tests"></a>
  <a href="https://www.npmjs.com/package/@drakulavich/kesha-voice-kit"><img src="https://img.shields.io/npm/v/@drakulavich/kesha-voice-kit" alt="npm version"></a>
  <a href="https://opensource.org/licenses/MIT"><img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="License: MIT"></a>
  <a href="https://bun.sh"><img src="https://img.shields.io/badge/runtime-Bun-f9f1e1?logo=bun" alt="Bun"></a>
</p>

<p align="center"><b>Give your local tools and LLM agents a voice.</b><br>Fast speech-to-text, text-to-speech, voice-activity detection, and language detection in one local-first CLI: Apple Silicon CoreML first, ONNX fallback on supported Linux/Windows builds.</p>

- **Transcribe locally** — 25 languages, up to ~19x faster than Whisper on Apple Silicon, ~2.5x on CPU
- **Speak back** — Kokoro (EN), Vosk-TTS (RU), macOS system voices, and SSML preview
- **Plug into agents** — ship voice workflows as CLI commands or an <a href="https://github.com/openclaw/openclaw">OpenClaw</a> skill
- **Small Rust engine** — single ~20MB binary, no ffmpeg, no Python, no native Node addons

See [Product positioning](docs/product-positioning.md) for supported workflows, non-goals, maturity labels, and the platform matrix.

<p align="center">
  <img src="./demo.gif" alt="kesha demo — English + Russian transcription with automatic language detection" width="800">
</p>

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

## Requirements

- [Bun](https://bun.sh) >= 1.3
- macOS arm64, Linux x64, or Windows x64

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

Stdout: transcript. Stderr: errors. Pipe-friendly.

For long / silence-heavy audio, install VAD (`kesha install --vad`) and run without `--no-vad`. Kesha auto-uses VAD past 120 s when installed; without VAD, very long audio falls back to fixed ASR chunks. Details: [docs/vad.md](docs/vad.md).

**Speaker diarization** (darwin-arm64, post-v1.12.0):

```bash
kesha install --diarize                        # one-time, ~245MB Sortformer model
kesha --json --vad --speakers meeting.m4a > out.json
jq '.[0].segments[] | "\(.speaker)\t\(.text)"' out.json
```

Each segment gets a `speaker` integer (cluster ID, stable within one file). Linux / Windows: `--speakers` returns a clear "currently darwin-arm64 only" error — see [#199](https://github.com/drakulavich/kesha-voice-kit/issues/199).

## Text-to-speech

Kesha speaks back via Kokoro-82M (English) and Vosk-TTS (Russian) — voice auto-picks from the text's language. On darwin-arm64, Kokoro uses FluidAudio CoreML instead of ONNX:

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

## Homebrew Install

Homebrew installs the Bun-based CLI wrapper. Engine and model downloads remain
explicit:

```bash
brew tap oven-sh/bun
brew install drakulavich/tap/kesha-voice-kit
kesha install
kesha audio.ogg
```

See [Homebrew install](docs/homebrew.md) for package scope and maintainer
validation.

## Linux Packages

Stable engine releases also publish `.deb` and `.rpm` packages for Linux x64.
They install the standalone CLI wrapper; engine and model downloads remain explicit:

```bash
kesha install
kesha audio.ogg
```

See [Linux packages](docs/linux-packages.md) for install commands and package
scope.

## Docker

Linux x64 CLI image, published to GHCR:

```bash
docker run --rm \
  -v kesha-cache:/cache/kesha \
  -v "$PWD:/work" -w /work \
  ghcr.io/drakulavich/kesha-voice-kit:latest install

docker run --rm \
  -v kesha-cache:/cache/kesha \
  -v "$PWD:/work" -w /work \
  ghcr.io/drakulavich/kesha-voice-kit:latest audio.ogg
```

The image keeps model downloads and the engine cache under `/cache/kesha`.
Mount that path as a named volume so `kesha install`, TTS models, VAD, and future
runs reuse the same cache. `compose.yml` provides the same layout:

```bash
docker compose run --rm kesha install
docker compose run --rm kesha audio.ogg
```

## Nix Install

Alternative reproducible-build path on `aarch64-darwin` / `x86_64-linux`:

```bash
nix run github:drakulavich/kesha-voice-kit -- install      # downloads models (engine is bundled)
nix run github:drakulavich/kesha-voice-kit -- audio.ogg    # transcribe
```

Full recipes (one-liner, profile install, engine-only, dev shell) live in [docs/nix-install.md](docs/nix-install.md).

## Shell Completions and Manpage

The npm package includes bash, zsh, and fish completions plus `kesha(1)`.
The CLI can print the packaged files, so install paths do not depend on the
Bun global package layout:

```bash
# bash
mkdir -p ~/.local/share/bash-completion/completions
kesha completions bash > ~/.local/share/bash-completion/completions/kesha

# zsh
mkdir -p ~/.zsh/completions
kesha completions zsh > ~/.zsh/completions/_kesha
# add to ~/.zshrc once: fpath=(~/.zsh/completions $fpath); autoload -Uz compinit; compinit

# fish
mkdir -p ~/.config/fish/completions
kesha completions fish > ~/.config/fish/completions/kesha.fish

# manpage
mkdir -p ~/.local/share/man/man1
kesha manpage > ~/.local/share/man/man1/kesha.1
mandb ~/.local/share/man 2>/dev/null || true
```

## Performance

> **Up to ~19x faster than Whisper** on Apple Silicon (M2), **~2.5x faster** on CPU

Compared against Whisper `large-v3-turbo` — all engines auto-detect language.

![Benchmark: openai-whisper vs faster-whisper vs Kesha Voice Kit](docs/assets/benchmark.svg)

See [BENCHMARK.md](BENCHMARK.md) for the full per-file breakdown (Russian + English).

## What's Inside

| Model | Task | Size | Source |
|---|---|---|---|
| NVIDIA Parakeet TDT 0.6B v3 | Speech-to-text | ~2.5GB | [HuggingFace](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3) |
| SpeechBrain ECAPA-TDNN | Audio language detection | ~86MB | [HuggingFace](https://huggingface.co/speechbrain/lang-id-voxlingua107-ecapa) |
| Apple NLLanguageRecognizer | Text language detection | built-in | macOS system framework |
| Silero VAD v5 (opt-in) | Voice activity detection | ~2.3MB | [snakers4/silero-vad](https://github.com/snakers4/silero-vad) |
| Kokoro-82M / Vosk-TTS (opt-in) | Text-to-speech | ~990MB | [FluidAudio Kokoro](https://github.com/FluidInference/FluidAudio) on darwin-arm64; ONNX Kokoro elsewhere · [Vosk-TTS](https://github.com/alphacep/vosk-tts) |

All models run through `kesha-engine` — a Rust binary using [FluidAudio](https://github.com/FluidInference/FluidAudio) (CoreML) on Apple Silicon and [ort](https://github.com/pykeio/ort) (ONNX Runtime) on other platforms.

Audio decoding via [symphonia](https://github.com/pdeljanov/Symphonia) — WAV, MP3, OGG/Opus, FLAC, AAC, M4A. No ffmpeg.

## Languages

- **Speech-to-text (25):** Bulgarian, Croatian, Czech, Danish, Dutch, English, Estonian, Finnish, French, German, Greek, Hungarian, Italian, Latvian, Lithuanian, Maltese, Polish, Portuguese, Romanian, Russian, Slovak, Slovenian, Spanish, Swedish, Ukrainian.
- **Audio language detection (107):** [full list](https://huggingface.co/speechbrain/lang-id-voxlingua107-ecapa).

## Integrations

- **OpenClaw** — give your LLM agent ears. Install & config: [docs/openclaw.md](docs/openclaw.md).
- **Hermes Agent** — local STT/TTS through Hermes command providers. Setup: [docs/hermes.md](docs/hermes.md).
- **Raycast** (macOS) — transcribe selected audio & speak clipboard from the launcher. Source + install: [`raycast/`](raycast/).

## Programmatic API

```typescript
import { transcribe, downloadModel } from "@drakulavich/kesha-voice-kit/core";

await downloadModel();                       // install engine + models
const text = await transcribe("audio.ogg");  // transcribe
```

## Support diagnostics

Kesha can collect local diagnostics without downloading models or mutating cache state:

```bash
kesha doctor --json --redact
kesha support-bundle --output kesha-support.tar.gz
```

`support-bundle` creates a redacted `.tar.gz` archive for GitHub issues. It includes runtime, engine, cache, optional-component, Stats status, and known Kesha environment settings. It does not include audio, transcripts, model files, or the Stats database.

## Local Stats privacy and lifecycle

Kesha Stats is disabled by default. When you opt in with `kesha stats enable`,
Kesha writes a local SQLite database only on your machine:

```bash
kesha stats status
kesha stats week
kesha stats errors
kesha stats export --format json   # or csv
kesha stats retention 30           # default: 90 days
kesha stats retention off          # keep until reset
kesha stats reset                  # delete recorded stats rows
kesha stats vacuum                 # compact the SQLite file
```

The database stores content-free operational records only: command name
(`transcribe` or `say`), timestamps, success/failure status, app version, item
count, anonymous stage timings, input/output artifact kind, file extension,
size, optional duration/sample-rate/channel counts, and sanitized error
class/code/message.

Stats never stores audio bytes, transcripts, input text, generated speech text,
file names, full file paths, raw stdout/stderr, environment variables, model
files, API tokens, or cloud identifiers. `support-bundle` reports Stats status
only; it never includes the Stats SQLite database.

By default, Stats prunes rows older than 90 days before writing or exporting
data. Use `kesha stats retention <days>` to change the TTL or `kesha stats
retention off` to disable TTL pruning. `kesha stats reset` deletes recorded
runs, artifacts, timings, and errors while preserving settings such as enabled
state and retention.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Made with 💛🩵 and 🥤 energy under MIT License
