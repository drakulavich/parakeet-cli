# Text-to-Speech

Kesha speaks back via Kokoro-82M (English) and Vosk-TTS (Russian). Voice is auto-picked from the input text's language — `en` routes to Kokoro, `ru` to Vosk. Pass `--voice` to override.

```bash
kesha install --tts                 # ~990MB (Kokoro + Vosk-TTS RU, opt-in)
kesha say "Hello, world" > hello.wav
kesha say "Привет, мир" > privet.wav    # auto-routes (Milena on darwin, ru-vosk-m02 elsewhere)
echo "long text" | kesha say > reply.wav
kesha say --out reply.wav "text"
kesha say --voice en-am_michael "text"    # explicit voice overrides auto-routing
kesha say --list-voices
```

Output format: WAV mono float32 (24 kHz for Kokoro, 22.05 kHz for Vosk). OGG/Opus and MP3 are tracked in follow-up issues.

Grapheme-to-phoneme:
- **English** uses [misaki-rs](https://github.com/MicheleYin/misaki-rs) — a self-contained Rust port of [hexgrad/misaki](https://github.com/hexgrad/misaki) (the G2P Kokoro was trained against). Lexicon and POS-tagger weights are embedded at compile time, no system deps. Out-of-vocabulary words spell letter-by-letter — proper-noun fallback is tracked as a follow-up.
- **Russian** is handled internally by [Vosk-TTS](https://github.com/alphacep/vosk-tts) — text normalisation, stress, palatalisation, and a BERT prosody model all run inside the bundled ONNX (no separate G2P pass, no system `espeak-ng` dependency).
- **Other languages**: not supported by the on-disk engines we ship today — tracked per-language in [#212](https://github.com/drakulavich/kesha-voice-kit/issues/212).

Default voices are **male** per CLAUDE.md "DEFAULT TTS VOICES MUST BE MALE": `am_michael` for English Kokoro, `ru-vosk-m02` for Russian Vosk on Linux/Windows. The darwin Russian fallback uses `Milena` (AVSpeech, female) for the zero-install path; pass `--voice ru-vosk-m02` to opt into Vosk on macOS too.

**Supported voices:**
- English: `en-am_michael` (default), plus any Kokoro voice you download into `~/.cache/kesha/models/kokoro-82m/voices/` (`am_*`/`bm_*` male, `af_*`/`bf_*` female).
- Russian: 5 Vosk-TTS speakers baked into the multi-speaker model — `ru-vosk-m02` (default, male), `ru-vosk-m01` (male), `ru-vosk-f01`/`f02`/`f03` (female).
- macOS system voices: `macos-<identifier-or-language>` routes to `AVSpeechSynthesizer`. Zero install, any of the 180+ voices already on your Mac.

## macOS system voices

`kesha say --voice macos-*` routes through `AVSpeechSynthesizer` on macOS, so you get voice synthesis for free — no 490 MB TTS bundle. The sidecar binary ships alongside `kesha-engine` on darwin-arm64 releases ([#141](https://github.com/drakulavich/kesha-voice-kit/issues/141)); `kesha install` places both in `~/.cache/kesha/bin/`.

```bash
kesha say --list-voices | grep ^macos-                                       # discover installed voices
kesha say --voice macos-com.apple.voice.compact.en-US.Samantha "Hello" > out.wav
kesha say --voice macos-ru-RU "Привет, мир" > hello-ru.wav                   # language-code fallback
```

Voice id format: `macos-<id>` where `<id>` is either a full Apple identifier (`com.apple.voice.compact.en-US.Samantha`) or a language code (`en-US`, `ru-RU`) — the Swift helper tries the identifier first and falls back to the language. Output is mono float32 @ 22050 Hz, structurally identical to Vosk.

Quality tradeoff is honest: macOS system voices are notification-grade. Use them when you want zero-install TTS on macOS; keep Kokoro/Vosk for anything that needs to sound good.

## English acronym auto-expansion

For `en-*` (Kokoro) voices, `kesha say` auto-expands all-uppercase Latin acronyms into a pronunciation Kokoro can render. Three cooperating tables pick the right path per token:

```bash
kesha say --voice en-am_michael 'The FBI is investigating.'
# audible: "The ef-bee-eye is investigating."

kesha say --voice en-am_michael 'EPAM partners with Anthropic.'
# audible: "EE-pam partners with an-THROP-ik."  (IPA injection bypasses G2P)

kesha say --voice en-am_michael 'Send JSON over HTTP.'
# audible: "Send JAY-son over aitch-tee-tee-pee."  (mixed: IPA + letter-spell)

kesha say --voice en-am_michael --no-expand-abbrev 'EPAM ...'
# IPA hits still fire (intent-explicit, parallel to <say-as>); letter-spell rule disabled.
```

- **Letter-spell rule** — uppercase Latin tokens 2–5 chars not on the stop-list and not in the lexicon get expanded letter-by-letter via the embedded letter-name table. Disable per call with `--no-expand-abbrev`.
- **`STOP_LIST`** (30 entries) — natural-English caps words pass through verbatim: `NASA`, `NATO`, `AIDS`, `OPEC`, `IKEA`, `ASCII`, `NAFTA`, `LASER`, `RADAR`, `SCUBA`, plus 20 emphatic length-2 caps (`OK`, `IT`, `IS`, …).
- **`IPA_LEXICON`** (19 entries) — case-sensitive token → IPA-phoneme map; hits emit a `Segment::Ipa` and bypass G2P entirely. Covers industry-pronunciation acronyms (`EPAM` /ˈiːpæm/, `JSON` /ˈdʒeɪsən/, `JPEG`, `GIF`, `SQL`, `ASAP`, `CRUD`, `JWT`, `OAuth`) AND mixed-case proper nouns (`Anthropic` /ænˈθrɒpɪk/, `Microsoft`, `Claude`, `Kubernetes`, `PostgreSQL`, `GraphQL`, `Linux`, `Tokio`, `macOS`, `Granola`). IPA hits fire even with `--no-expand-abbrev`.

`<say-as interpret-as="characters">…</say-as>` always wins — letter-spells via the embedded table regardless of `--no-expand-abbrev`. Engine reports `tts.en_acronym_expansion: true` in `--capabilities-json`. Closes [#244](https://github.com/drakulavich/kesha-voice-kit/issues/244).

## Russian abbreviation auto-expansion

For `ru-vosk-*` voices, `kesha say` detects all-uppercase Cyrillic acronyms (length 2–5) and reads them letter-by-letter when the token cannot be pronounced as a natural Russian syllable:

```bash
kesha say --voice ru-vosk-m02 'ФСБ объявила.'      # audible: "эф эс бэ объявила"
kesha say --voice ru-vosk-m02 'ВОЗ предупреждает.' # audible: "воз предупреждает" (CVC alternation passes through)
kesha say --voice ru-vosk-m02 'ОН пришёл.'         # audible: "ОН пришёл" (stop-list)
```

The rule fires when the token is length ≤ 2 (`ИП` → "и пэ"), has 0 vowels (`ФСБ` → "эф эс бэ"), or has 2+ consecutive vowels / consonants (`ОАЭ` → "о а э", `США` → "сэ шэ а"). Tokens with strict CVC/CVCV alternation pass through (`ВОЗ`, `НАТО`, `ОПЕК`). Letter-name forms tuned to user-validated Vosk pronunciation: `Ф` → "эф", `Ш` → "шэ", `Л` → "эл", `С` → "сэ" at start / "эс" elsewhere. Stop-list of ~25 common short words (`ОН`, `МЫ`, `КАК`, `ЧТО`, …) prevents false positives. Tokens containing `Ъ`/`Ь` are passed through literally.

Opt-out per call with `--no-expand-abbrev`. `<say-as interpret-as="characters">…</say-as>` always wins. Engine reports `tts.ru_acronym_expansion: true`. Closes [#232](https://github.com/drakulavich/kesha-voice-kit/issues/232).

## Russian word stress (`<emphasis>`)

For `ru-vosk-*` voices, `<emphasis>` lets you place the stress on a specific vowel by prepending `+` to it. Vosk-TTS honors the marker as a stress hint when it shifts stress AWAY from the model's default first-syllable behavior:

```bash
kesha say --voice ru-vosk-m02 --ssml \
  '<speak><emphasis>дом+а</emphasis></speak>'  # genitive до-МА́
kesha say --voice ru-vosk-m02 --ssml \
  '<speak><emphasis level="none">дом+а</emphasis></speak>'  # default ДО́ма (suppress)
```

Once-per-process stderr warning fires when `<emphasis>` content lacks any `+` marker. `<emphasis>` on Kokoro / AVSpeech voices strips `+` and warns once (Kokoro has no `+`-marker analog). Engine reports `tts.ru_emphasis_marker: true`. Closes [#233](https://github.com/drakulavich/kesha-voice-kit/issues/233).

## SSML

`kesha say --ssml` accepts a subset of [SSML](https://www.w3.org/TR/speech-synthesis11/):

```bash
kesha say --ssml '<speak>Hello <break time="500ms"/> world.</speak>'
kesha say --ssml --voice ru-vosk-m02 '<speak>Привет <break time="1s"/> мир.</speak>'
```

| Tag | Status |
|---|---|
| `<speak>` | ✅ required root |
| `<break time="Nms"\|"Ns"\|default>` | ✅ inserts silence of the given duration |
| plain text inside `<speak>` | ✅ synthesized via the selected engine |
| `<say-as interpret-as="characters">…</say-as>` | ✅ honored on `ru-vosk-*` (#232) and `en-*` (#244) — letter-spells via the embedded table; stripped with stderr warning on AVSpeech |
| `<say-as interpret-as="cardinal\|ordinal\|date\|telephone\|...">` | ⚠️ stripped with stderr warning (contained text still synthesized); separate concern |
| `<emphasis>` | ✅ honored on `ru-vosk-*` (#233) — `+vowel` markers shift stress; `level="none"` suppresses. Stripped + warned on Kokoro / AVSpeech (no `+`-marker analog) |
| `<phoneme alphabet="ipa" ph="…">` | ✅ honored on Kokoro — bypasses G2P, feeds IPA directly to inference (#193) |
| `<prosody rate/pitch/volume>` | ⚠️ stripped with stderr warning; tracked in [#236](https://github.com/drakulavich/kesha-voice-kit/issues/236) |
| `<!DOCTYPE>` | ❌ rejected (hardening against XXE) |

SSML is opt-in via the explicit `--ssml` flag — inputs that happen to contain `<angle brackets>` aren't misinterpreted as SSML.
