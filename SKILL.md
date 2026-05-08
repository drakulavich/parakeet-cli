---
name: kesha-voice-kit
description: Local multilingual voice toolkit — speech-to-text (STT), text-to-speech (TTS), and language detection. Runs entirely offline on Apple Silicon, Linux, and Windows. No API keys, no cloud. NVIDIA Parakeet TDT for STT across 25 European languages, Kokoro-82M + Vosk-TTS for TTS, plus macOS AVSpeechSynthesizer for ~180 system voices with zero install.
emoji: 🎙️

requires:
  bins: [kesha]

install:
  - kind: bash
    cmd: bun add -g "@drakulavich/kesha-voice-kit"
  - kind: bash
    cmd: kesha install
---

# kesha-voice-kit

Local voice toolkit: transcribe voice messages to text, synthesize speech, detect language of audio or text. Fully offline after `kesha install`. No API keys, no per-minute billing.

**Trigger keywords for when to use this skill:** voice message, voice memo, voice note, .ogg, .opus, .wav, .mp3, audio file, transcribe, transcription, speech-to-text, STT, text-to-speech, TTS, synthesize speech, say, telegram voice note, whatsapp voice note, ogg-opus, opus, multilingual voice, multilingual ASR, language detection, offline voice, privacy, Apple Silicon, CoreML.

## When to use

- **Voice memo arrived** (Telegram, WhatsApp, Slack, Signal .ogg/.opus/.m4a): transcribe with `kesha --json <path>` and branch on the detected language.
- **Need to reply with audio (file playback)**: synthesize with `kesha say "<text>" > reply.wav`. Auto-routes by detected language (Kokoro-82M for English, Vosk-TTS for Russian). For other languages and ~180 more voices use `--voice macos-*` on macOS (zero model download).
- **Need to send a voice note (Telegram, WhatsApp, Signal, Discord)**: synthesize directly into the messenger-native format with `kesha say "<text>" --format ogg-opus --out reply.ogg`. Default is mono 24 kHz @ 32 kbps — what Telegram `sendVoice` expects. No `ffmpeg` round-trip needed.
- **Need to detect what language a file is in** before choosing a pipeline: `kesha --json audio.ogg` returns both audio-based and text-based language detection with confidence scores.

## STT: transcribe audio

```bash
# JSON output with language detection (recommended for automation)
kesha --json voice.ogg
```

```json
[{
  "file": "voice.ogg",
  "text": "Привет, как дела?",
  "lang": "ru",
  "audioLanguage": { "code": "ru", "confidence": 0.98 },
  "textLanguage": { "code": "ru", "confidence": 0.99 }
}]
```

Use `lang` (or the more detailed `audioLanguage`/`textLanguage`) to decide how to respond.

Need timestamped transcript segments for navigation, chapters, or downstream editing:

```bash
kesha --json --timestamps voice.ogg > voice.timestamps.json
jq '.[0].segments' voice.timestamps.json
```

Each segment has `start`, `end`, and `text` fields. `--timestamps` is available for machine-readable output (`--json`, `--toon`, or `--format json`).

**Formats:** .ogg, .opus, .mp3, .m4a, .wav, .flac, .webm — decoded via symphonia, no ffmpeg required.

**Other output modes:**
- `kesha audio.ogg` — plain transcript on stdout
- `kesha --format transcript audio.ogg` — transcript + `[lang: ru, confidence: 0.99]` footer
- `kesha --json --timestamps audio.ogg` — JSON with timestamped `segments`
- `kesha --verbose audio.ogg` — human-readable with language info
- `kesha --lang en audio.ogg` — warn if detected language differs (useful sanity check)

## TTS: synthesize speech

```bash
kesha say "Hello, world" > hello.wav               # auto-routes en → Kokoro-82M
kesha say "Привет, мир" > privet.wav              # auto-routes ru → Vosk-TTS
kesha say --voice macos-de-DE "Guten Tag" > de.wav # any macOS system voice — German, French, Italian, ...
kesha say --list-voices                            # Kokoro + Vosk-TTS + ~180 macos-* voices
```

Output: WAV mono float32 by default. `--out <path>` writes to a file instead of stdout.

**Voice notes (Telegram / WhatsApp / Signal / Discord):** add `--format ogg-opus` to emit OGG/Opus directly — the format messenger APIs render as a native voice message:

```bash
kesha say "Hello there" --format ogg-opus --out reply.ogg                  # 24 kHz @ 32 kbps mono — Telegram-grade
kesha say "Привет" --voice ru-vosk-m02 --format ogg-opus --out reply.ogg   # Russian voice note
kesha say "Hi" --format ogg-opus --bitrate 16000 --out tiny.ogg            # tinier file, intelligible but lossy
```

Format is also inferred from `--out` extension (`.ogg` / `.opus` / `.oga` → OGG/Opus). `--bitrate` (6 000–510 000 bps) and `--sample-rate` (8 000 / 12 000 / 16 000 / 24 000 / 48 000 Hz) tune the encoder.

**Russian abbreviation handling** (`ru-vosk-*` only): all-uppercase Cyrillic tokens (length 2–5) auto-expand letter-by-letter when the token cannot be pronounced as a natural Russian syllable — `ФСБ объявила` → `эф-эс-бэ объявила`. Tokens with strict CVC/CVCV alternation pass through (ВОЗ → "воз", НАТО → "нато"). Disable per call with `--no-expand-abbrev`. Override per-token via SSML `<say-as interpret-as="characters">…</say-as>` (always wins, even with `--no-expand-abbrev`). Stop-list for common short words (ОН, МЫ, ВЫ, КАК, ЧТО) prevents false positives. Closes [#232](https://github.com/drakulavich/kesha-voice-kit/issues/232).

**English acronym handling** (`en-*` Kokoro-82M only). Three tables: (1) **letter-spell** for uppercase 2–5-char tokens not on the stop-list (FBI → "ef bee eye", HTTP → "aitch tee tee pee"); (2) **`STOP_LIST`** (30 entries) — natural-English caps words pass through (NASA, NATO, AIDS, OPEC, IKEA, ASCII, NAFTA, LASER, RADAR, SCUBA + 20 emphatic length-2 caps); (3) **`IPA_LEXICON`** (20 entries) — case-sensitive token → IPA-phoneme map that bypasses G2P entirely. Covers industry-pronunciation acronyms (EPAM /ˈiːpæm/, JSON /ˈdʒeɪsən/, JPEG, SQL, ASAP, GIF, CRUD, JWT, OAuth) AND mixed-case proper nouns (Anthropic /ænˈθrɒpɪk/, Microsoft, Claude, NVIDIA, Kubernetes, PostgreSQL, GraphQL, Linux, Tokio, macOS, Granola). Disable letter-spell with `--no-expand-abbrev` — IPA hits still fire (intent-explicit). Override per-token via SSML `<say-as interpret-as="characters">…</say-as>` (always wins). Closes [#244](https://github.com/drakulavich/kesha-voice-kit/issues/244).

**Russian word stress** (`ru-vosk-*` only): `<emphasis>сл+ово</emphasis>` shifts stress to the vowel marked with `+`. `<emphasis level="none">сл+ово</emphasis>` strips the `+` (cancel inherited emphasis). Other voices (`en-*`, `macos-*`) silently strip the `+` and warn once per process. Auto-stress dictionary not provided — caller writes the `+` manually. Closes [#233](https://github.com/drakulavich/kesha-voice-kit/issues/233).

## Language detection standalone

`kesha --json audio.ogg` includes both audio-based (`audioLanguage`) and text-based (`textLanguage`) detection. Use audio detection to identify the language before running language-specific logic.

## Install

```bash
bun add -g @drakulavich/kesha-voice-kit          # global CLI install
kesha install                                    # downloads engine (~350 MB)
kesha install --tts                              # adds Kokoro + Vosk-TTS RU (~990 MB more, for TTS)
```

No system deps — English G2P is embedded (`misaki-rs`); Russian G2P is bundled inside Vosk-TTS. `macos-*` voices need no install either — they use voices already on the Mac.

## Supported languages

**Speech-to-text (25):** Bulgarian, Croatian, Czech, Danish, Dutch, English, Estonian, Finnish, French, German, Greek, Hungarian, Italian, Latvian, Lithuanian, Maltese, Polish, Portuguese, Romanian, Russian, Slovak, Slovenian, Spanish, Swedish, Ukrainian.

**Text-to-speech:** English (Kokoro-82M, ~70 voices), Russian (Vosk-TTS, 5 baked-in speakers — default `ru-vosk-m02`), plus any macOS system voice via `--voice macos-*`.

## Performance

- ASR: ~19× faster than OpenAI Whisper on Apple Silicon (CoreML via FluidAudio), ~2.5× on CPU (ONNX via `ort`).
- TTS: sub-second latency for short utterances on Apple Silicon.

## Why local

No API keys to manage. No per-minute billing. Voice data never leaves the machine — important for regulated industries, personal messaging, and anything that shouldn't be in a third-party log.

## Links

- Source: https://github.com/drakulavich/kesha-voice-kit
- npm: https://www.npmjs.com/package/@drakulavich/kesha-voice-kit
- Releases: https://github.com/drakulavich/kesha-voice-kit/releases
