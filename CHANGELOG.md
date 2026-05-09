# Changelog

All notable changes to `@drakulavich/kesha-voice-kit` are documented here.
Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the
project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

CLI and engine versions are **decoupled** — see `CLAUDE.md` for details. Tags
with a `-cli` suffix are CLI-only patches that reuse the previous engine
binary.

## [Unreleased]

## [1.11.0] — 2026-05-08

Engine release. Polish on the timestamped-segments path shipped in v1.9.0, plus a high-severity Dependabot fix for the Raycast extension dependency tree.

### Added
- **`build_vad_output_segments` pure helper** in `rust/src/transcribe.rs` — extracted the VAD-loop transcription step into a closure-driven function so unit tests can assert ordering invariants (`end > start` per segment, monotonic `start[i+1] >= start[i]` across segments) without spinning up the ONNX model. Codifies what manual 45-min/817-segment testing in #247 had verified by hand. Closes [#248](https://github.com/drakulavich/kesha-voice-kit/issues/248).
- **`pub const TRANSCRIBE_SEGMENTS_FEATURE = "transcribe.segments"`** (Rust + TS) — single source of truth for the capability-flag string. Capabilities, the TS CLI gate, and integration tests all import the const.
- **`single_segment(start, end, text)` shared helper** dedupes the trim-and-construct logic between `whole_file_segment` and the VAD-fallback path.
- **`@deprecated transcribeWithSegments`** alias on `src/lib.ts` for one minor-version cycle (removed in v1.12.0). The public API renamed to `transcribeWithTimestamps` to disambiguate from the internal `transcribeWithSegments` in `src/transcribe.ts` (which respects the `timestamps` option flag).

### Changed
- **`--json` Auto path probes duration once, not twice.** `transcribe_inner` now threads its already-probed `Option<f32>` through `transcribe_plain` → `whole_file_segment`, eliminating a redundant symphonia open. Saves <10 ms on the Auto-mode `--json` request.
- **Capability cache invalidates on in-process binary swap.** `getEngineCapabilities` cache key now includes `mtimeMs`. Long-lived library callers (Granola integration, programmatic SDK use) see fresh capabilities after `kesha install` overwrites the binary mid-process.
- Renamed internal flag `whole_file_segment_required` → preserved as `timestamps_required` after a Greptile P1 caught a regression: text-only `pub fn transcribe` callers must skip the duration probe entirely (streaming Ogg/Opus return `Ok(None)` from `probe_duration_seconds`, which would have hard-errored). The gate-shape is unchanged from #247; the rename was a wash.

### Security
- **Dependabot alert [#10](https://github.com/drakulavich/kesha-voice-kit/security/dependabot/10)** — high-severity ReDoS in `minimatch matchOne()` with multiple non-adjacent GLOBSTAR segments. Resolved by `"overrides": { "minimatch": ">=9.0.7" }` in `raycast/package.json`; all 7 transitive minimatch resolutions now dedupe to 10.2.5. `npm audit` clean.

### Shipped PRs
- [#252](https://github.com/drakulavich/kesha-voice-kit/pull/252) — refactor(#248): timestamped-segments path polish
- [#253](https://github.com/drakulavich/kesha-voice-kit/pull/253) — chore(release): v1.11.0 + minimatch security fix

## [1.10.1] — 2026-05-08

CLI-only patch release. Engine binary unchanged at v1.10.0. Marker tag `v1.10.1-cli` (the `-cli` suffix excludes it from `build-engine.yml`'s tag filter — no Rust rebuild fires).

### Added
- **SKILL.md** documents timestamped transcript segments shipped in v1.9.0 ([#247](https://github.com/drakulavich/kesha-voice-kit/pull/247)). Adds `kesha --json --timestamps` example with a `jq` snippet, segment shape (`start`, `end`, `text`), and the `--json`/`--toon`/`--format json` gate.

### Shipped PRs
- [#249](https://github.com/drakulavich/kesha-voice-kit/pull/249) — docs: document timestamped skill output (Timur Khakhalev)
- [#251](https://github.com/drakulavich/kesha-voice-kit/pull/251) — chore(release): v1.10.1 (CLI-only)

## [1.10.0] — 2026-05-08

Engine release. Adds English acronym auto-expansion for Kokoro voices via three cooperating tables (letter-spell rule + stop-list + IPA lexicon).

### Added
- **English acronym auto-expansion for `en-*` (Kokoro) voices**, gated by the existing `--no-expand-abbrev` flag. Three-table mechanism:
  - **Letter-spell rule** — uppercase Latin tokens 2–5 chars get expanded letter-by-letter (`FBI` → "ef bee eye", `HTTP` → "aitch tee tee pee").
  - **`STOP_LIST`** (30 entries) — natural-English caps words pass through verbatim (NASA, NATO, AIDS, OPEC, IKEA, ASCII, NAFTA, LASER, RADAR, SCUBA + 20 emphatic length-2 caps).
  - **`IPA_LEXICON`** (19 entries) — case-sensitive token → IPA-phoneme map that bypasses G2P entirely. Covers industry-pronunciation acronyms (EPAM /ˈiːpæm/, JSON /ˈdʒeɪsən/, JPEG, GIF, SQL, ASAP, CRUD, JWT) AND mixed-case proper nouns (OAuth, Microsoft, Anthropic, Claude, Kubernetes, PostgreSQL, GraphQL, Linux, Tokio, macOS, Granola). IPA hits fire even with `--no-expand-abbrev` (intent-explicit, parallel to `<say-as>`).
- **Engine `--capabilities-json` reports `tts.en_acronym_expansion: true`** in the `features` array. The TS CLI capability gate ORs this with `tts.ru_acronym_expansion` so older engines still drop `--no-expand-abbrev` correctly.
- **`<say-as interpret-as="characters">…</say-as>`** on the Kokoro path now letter-spells via the embedded letter-name table; previously it stripped + warned. Closes [#244](https://github.com/drakulavich/kesha-voice-kit/issues/244).

### Internal
- New `rust/src/tts/en/` module (`mod.rs`, `acronym.rs`, `letter_table.rs`) mirrors the shipped Russian Vosk-TTS module.
- New `rust/tests/tts_en_normalize.rs` integration test using the warm `--stdin-loop` harness — three cases: FBI letter-spell vs raw byte-length, NASA stop-list pass-through, `<say-as>` overrides `--no-expand-abbrev`.
- audio-quality-check agent ran on a 27-phrase corpus before publish: 27/27 PASS.

### Shipped PRs
- [#250](https://github.com/drakulavich/kesha-voice-kit/pull/250) — feat(tts): English acronym auto-expansion for Kokoro

## [1.9.0] — 2026-05-07

Engine release. Adds opt-in timestamped transcript segments for machine-readable transcription output. Default text path (no flag) is unchanged.

### Added
- **`kesha --json --timestamps audio.ogg`** (and `--toon --timestamps`, `--format json --timestamps`) — returns timestamped transcript segments via `--json`/`--toon`/`--format json`. Each segment has `{ start, end, text }`. With VAD active (default for files >200 KB), per-utterance segments; without VAD, a single whole-file segment.
- **`kesha-engine transcribe --json`** — engine subcommand that always returns `{ text, segments[] }`. Gated behind a new `transcribe.segments` capability flag in `--capabilities-json`. The TS CLI checks the flag before forwarding `--timestamps`, so older engines fail loudly.
- **`transcribeWithSegments()`** programmatic API in `@drakulavich/kesha-voice-kit/core` — see #247. (Renamed to `transcribeWithTimestamps` in v1.11.0; deprecated alias kept until v1.12.0.)

### Shipped PRs
- [#247](https://github.com/drakulavich/kesha-voice-kit/pull/247) — feat: add timestamped transcript segments (Timur Khakhalev)

## [1.8.2] — 2026-05-07

### Fixed

- **Mono WAV output now plays in both ears, not just the left one** ([#245](https://github.com/drakulavich/kesha-voice-kit/issues/245)). The previous hound-based encoder wrote `WAVE_FORMAT_EXTENSIBLE` (0xFFFE) with `dwChannelMask=0x4`, which Apple's CoreAudio interpreted as "Front Left" for mono streams — Kokoro and Vosk-TTS playback ended up in the left ear only on AirPods / left speaker only on stereo. Replaced with a hand-rolled writer that emits plain `WAVE_FORMAT_IEEE_FLOAT` (0x0003) without the EXTENSIBLE extension. AVSpeech sidecar and OGG/Opus paths were not affected and remain unchanged.

## [1.8.1] — 2026-05-06

### Fixed

- **`<emphasis level="none">` no longer triggers the "non-ru-vosk" warning** on Kokoro and the defensive Vosk arm. The user explicitly opted out of stress markers via `level="none"`; emitting "stress markers are honored only on ru-vosk-* voices" was technically accurate but misleading. The `warn_once` call is now gated on `!suppress`. Closes [#238](https://github.com/drakulavich/kesha-voice-kit/issues/238).

### Added

- **End-to-end test for warn-once dedup** across multiple `<emphasis>` calls in the same engine process (`emphasis_warn_once_dedups_across_calls` in `rust/tests/tts_ru_normalize.rs`). The `LoopEngine` test wrapper now captures stderr to a tempfile via a new `into_stderr_log()` consuming method so the contract "one warning per process, not per call" is verified at the integration layer. Closes [#237](https://github.com/drakulavich/kesha-voice-kit/issues/237).

## [1.8.0] — 2026-05-05

### Added

- **SSML `<emphasis>` honored on the Russian Vosk path.** Caller-provided `+`-before-vowel markers (`<emphasis>дом+а</emphasis>`) are passed through to vosk-tts-rs, which honors them as a stress hint when they shift stress AWAY from the model's default first-syllable position. `<emphasis level="none">` suppresses inherited emphasis (strips `+`). Once-per-process stderr warning when content lacks any `+` marker. Closes [#233](https://github.com/drakulavich/kesha-voice-kit/issues/233).
- **Engine `--capabilities-json` reports `tts.ru_emphasis_marker`** in the `features` array. Lets future clients gate `<emphasis>` against older engines.
- **`<emphasis>` on non-Russian-Vosk voices (Kokoro, AVSpeech)** silently strips `+` markers before reaching G2P / Swift sidecar, with a once-per-process stderr warning. The text content otherwise synthesizes normally — no caller-visible synth failure.

### Notes

- No new CLI flag. `<emphasis>` is pure SSML, ships via `--ssml`.
- No auto-stress dictionary. Path B (engine guesses ударение without a `+`) is intentionally deferred — see issue #233 for the design rationale.
- `<prosody rate/pitch/volume>` is tracked separately in [#236](https://github.com/drakulavich/kesha-voice-kit/issues/236).

## [1.7.0] — 2026-05-05

### Added
- **Russian abbreviation auto-expansion for `ru-vosk-*` voices.** Detects 2–5 letter all-uppercase Cyrillic tokens and reads them letter-by-letter via the embedded letter-name table. The rule fires when the token cannot be pronounced as a natural Russian syllable — length ≤ 2 (ИП → "и пэ"), 0 vowels (ФСБ → "эф эс бэ"), 2+ consecutive vowels (ОАЭ → "о а э"), or 2+ consecutive consonants (США → "сэ шэ а"). Tokens with strict CVC/CVCV alternation pass through (ВОЗ → "воз", НАТО → "нато", ОПЕК → "опек"). Letter-name forms tuned to user-validated pronunciation: Ф → "эф", Ш → "шэ", Л → "эл", С → "сэ" at start / "эс" elsewhere. Stop-list for common short words (ОН, МЫ, КАК, ЧТО, …) prevents false positives. Tokens containing Ъ/Ь are passed through literally. Opt-out via `--no-expand-abbrev` flag. Closes [#232](https://github.com/drakulavich/kesha-voice-kit/issues/232).
- **SSML `<say-as interpret-as="characters">…</say-as>` honored on the Russian Vosk path.** Always wins, regardless of `--no-expand-abbrev` setting. Other `interpret-as` values (cardinal, ordinal, date, …) continue to warn and strip.
- **Engine `--capabilities-json` reports `tts.ru_acronym_expansion: true`** in the `features` array for compatibility with the TS CLI gate. The CLI uses this to conditionally forward `--no-expand-abbrev` only to engines that support it.

## [1.6.0] — 2026-04-30

Engine release. Adds OGG/Opus voice-note output, restores Windows MSVC builds via a vendored vosk-tts crate, and tightens the Opus hot path. CLI surface is unchanged — npm consumers get the new format flag automatically once the engine binary updates.

### Added
- **`kesha say --format ogg-opus`** — produces OGG/Opus voice notes (mono, 24 kHz @ 32 kbps by default) instead of WAV. The output file is the messenger-friendly format consumed by Telegram `sendVoice` and similar APIs. New flags `--bitrate` and `--sample-rate` tune the encoder; format is also inferred from `--out` extension (`.ogg` / `.opus` / `.oga`). All four engine paths (Kokoro plain/SSML, Vosk-TTS plain/SSML, AVSpeech) flow through the new encoder. WAV output remains the default and is byte-exact with the previous code path. (#224, closes #223)

### Changed
- **Vendored `vosk-tts-rs`** into `rust/vendor/vosk-tts` so Windows builds compile under MSVC again — upstream's `tonic`/`prost` chain pulled in MinGW-only deps that broke the Windows engine artifact. Behaviour and the public Rust API are unchanged. (#225, closes #216)
- npm `homepage` field now points at the project landing page (`https://drakulavich.github.io/kesha-voice-kit/`) instead of the README anchor.

### Performance
- **OGG/Opus encoder hot path:** dropped a redundant `pcm_buf.copy_from_slice` per 20 ms frame (saves N memcpys for an N-frame utterance), and right-sized the output `Vec::with_capacity` from `samples.len()` (≈6× over) to `bitrate × duration / 8 + 4 KiB`. (#226)

## [1.5.0] — 2026-04-29

First engine release since v1.4.1. Catches the binary up to the engine source
that's been sitting in `main` since #209/#211/#214. CLI 1.4.4 features
(Vosk-aware status, male English default, RU darwin auto-route) become
functional once this engine binary is installed.

### Added
- **Vosk-TTS for Russian** (multi-speaker, 5 baked-in voices: `ru-vosk-{f01,f02,f03,m01,m02}`). Uses `vosk-tts-rs` directly — BERT prosody + dictionary G2P, no espeak-ng / no separate G2P model. Default Russian voice on non-darwin platforms is now `ru-vosk-m02` (male, per the brand-voice rule); darwin keeps `Milena` for the zero-install AVSpeech path. (#214, closes #210)
- **misaki-rs G2P for English** in Kokoro — embedded lexicon + POS tagging, OOV words letter-spell. Replaces the ONNX ByT5-tiny G2P pipeline for English specifically. Russian is now handled inside Vosk-TTS. (#211)

### Changed
- **`kesha install --tts`** now downloads Kokoro + Vosk-TTS (~990 MB total) instead of Kokoro + Piper-RU + ONNX G2P. Disk savings on top of removing the FP32 G2P weights.
- **`kesha status`** reports the `vosk-ru` cache directory and the 5 Vosk speakers; Piper / G2P rows removed.
- Russian auto-routing: darwin → AVSpeech `Milena` (zero install); Linux/Windows → `ru-vosk-m02`. (#209, #214)

### Removed
- **Piper-RU** as the Russian backend. Old voice ids (`ru-denis`, `ru-irina`, etc.) no longer resolve. Migration: pass `--voice ru-vosk-m02` (default), or any of `ru-vosk-{f01,f02,f03,m01,m02}`. macOS users can also use `--voice macos-com.apple.voice.compact.ru-RU.Milena` (no model download).
- **CharsiuG2P (ONNX ByT5-tiny)** removed — the model files (`models/g2p/byt5-tiny/*`) are no longer downloaded. Existing caches are dead weight; `rm -rf ~/.cache/kesha/models/{g2p,piper-ru}` to reclaim space.

### Breaking changes
- Russian voice ids changed (`ru-denis` → `ru-vosk-m02`). The change is in source since #214; v1.5.0 is when the engine binary actually enforces it.
- `kesha install --tts` cache layout changed: `models/vosk-ru/` replaces `models/piper-ru/` and `models/g2p/`.

### Internal
- `protoc` install pulled into a reusable composite action (`.github/actions/install-protoc`) shared across `ci.yml`, `rust-test.yml`, and `build-engine.yml`.
- New CI agents: `audio-quality-check` (post-commit WAV stats sanity check) and `ci-feature-matrix-auditor` (verifies every cargo default feature appears in every build-engine matrix row).
- `rust/src/tts/kokoro.rs` — 4 pipeline bugs fixed alongside the misaki-rs swap (#211).

### Upgrade
```bash
bun add -g @drakulavich/kesha-voice-kit@latest
kesha install              # engine v1.5.0 (~22 MB)
kesha install --tts        # Kokoro + Vosk-RU (~990 MB; dedupe with prior cache happens automatically)
```

If you had `models/piper-ru/` or `models/g2p/` in your cache from a previous install, they're orphaned now — `rm -rf ~/.cache/kesha/models/{g2p,piper-ru}` to reclaim ~700 MB.

## [1.4.4] — 2026-04-29

### Changed
- Default voice for English auto-routing flipped from `en-af_heart` (female) to
  `en-am_michael` (male) to match Kesha's brand voice. Pass `--voice` to
  override. (#211)
- `kesha status` reports the `vosk-ru` cache directory and lists Vosk-TTS
  speaker ids (`ru-vosk-{f01,f02,f03,m01,m02}`) instead of the Piper layout.
  Aligns the CLI with the engine work queued for the next engine release.
  (#214)
- Russian auto-routing on darwin now picks AVSpeech `Milena` (zero install);
  Linux/Windows fall through to `ru-vosk-m02`. (#209, #214)

### Internal
- `protoc` install pulled into a reusable composite action and shared across
  `ci.yml`, `rust-test.yml`, and `build-engine.yml`.
- `actions/setup-node` bumped 4 → 6. (#215)
- Raycast extension `CHANGELOG.md` tracked in repo. (#206)

CLI-only release; engine v1.4.1 unchanged. Engine source in `main` carries the
Vosk-TTS / misaki-rs / AVSpeech-routing changes (#209, #211, #214) which will
ship with the next engine bump — Linux/Windows users hitting `ru-vosk-m02`
auto-routing today will get an "unknown voice" error until that release.

## [1.4.3] — 2026-04-24

### Changed
- README trimmed from 247 → 128 lines. Advanced sections (VAD, TTS, OpenClaw
  integration, air-gapped model mirror) moved into dedicated pages under
  `docs/` with one-line pointers from the README. (#203)

CLI-only release; engine v1.4.1 unchanged.

## [1.4.2] — 2026-04-23

### Added
- `kesha status` prints per-component disk usage (engine, ASR, lang-id, VAD,
  Kokoro, Piper, G2P) with a total + `rm -rf` cleanup hint. Missing components
  are skipped so partial installs stay tidy. (#197)

### Changed
- `package.json#description` aligned with the GitHub About blurb — now
  surfaces TTS (Kokoro + Piper + ~180 macOS system voices, SSML) and VAD
  alongside STT + language detection. (#198)

CLI-only release; engine v1.4.1 unchanged.

## [1.4.1] — 2026-04-23

### Added
- SSML `<phoneme alphabet="ipa" ph="…">` override — bypass G2P and feed IPA
  directly to Kokoro / Piper for rare words or proper nouns. (#193)
- G2P parity harness (`rust/tests/g2p_parity.rs`): 40 words × 11 languages
  locked against reference phonemes; catches tokenizer / tie-break drift that
  SHA-256 on the ONNX weights alone wouldn't notice. (#193)
- `BENCHMARK.md` G2P section — 149 ms/word measured end-to-end.

## [1.4.0] — 2026-04-23

### Added
- ONNX G2P (CharsiuG2P ByT5-tiny) shared by Kokoro and Piper. Byte-identical
  IPA vs. the Python reference on in-dictionary English. (#190)
- Smart VAD auto-engages on input ≥ 120 s when `kesha install --vad` is set;
  `--vad` / `--no-vad` override either direction. (#188)
- Manual `--vad` flag via Silero VAD v5 through `ort`. (#186)
- `NOTICES.md` bundled in the npm package (CC-BY 4.0 attribution for
  CharsiuG2P + catalog of bundled / downloaded artifacts). (#189)

### Removed
- `espeak-ng` runtime dependency — no more `brew install` / `apt install` /
  `choco install` step for TTS on any platform.

### Changed
- **Breaking**: `kesha install --tts` grew from ~390 MB to ~490 MB (FP32 G2P
  adds ~100 MB; INT8 quantization tracked as follow-up).
- Public Rust API: `kesha_engine` now exposes `pub mod models` and
  `pub mod util`.

## [1.3.0] — 2026-04-20

### Added
- macOS AVSpeechSynthesizer ships in release binaries. `kesha say --voice
  macos-*` works out of the box on darwin-arm64 with zero model download and
  ~180 system voices. `kesha install` fetches the Swift sidecar alongside the
  engine; falls back gracefully if the download 404s. (#141, #166)
- Windows TTS in release binaries (`--features coreml,tts` / `onnx,tts`
  matrix). Requires `choco install espeak-ng` at runtime. (#136, #159, #162)

### Changed
- Test-suite cleanup per Luca Rossi's contract-vs-implementation framework:
  −130 LOC of liability unit tests, +3 integration tests (net −67 LOC). (#163)

## [1.2.2] — 2026-04-20

### Changed
- `kesha install` GitHub-star prompt now fires only on first install or
  major/minor CLI bumps; patch re-installs and same-version runs stay silent.
  A `.star-seen` marker records the last prompted version. (#154)

CLI-only release; engine v1.2.0 unchanged.

## [1.2.1] — 2026-04-20

### Fixed
- `kesha install` detects a stale cached engine after a CLI upgrade and
  re-downloads automatically. Previously `--no-cache` was required across an
  engine-version bump. Closes #151. (#152)

CLI-only release; engine v1.2.0 unchanged.

## [1.2.0] — 2026-04-20

### Added
- SSML preview (`kesha say --ssml`): `<speak>` root + `<break time="…">`
  silence; unknown tags (`<emphasis>`, `<prosody>`, `<phoneme>`, `<say-as>`)
  strip with a stderr warning. `<!DOCTYPE>` rejected as XXE defense. (#140)
- Latency telemetry — `sttTimeMs` in `--json` output, `STT time: …ms` in
  `--verbose`, `TTS time: …ms` for `kesha say --verbose`. (#142, #143)
- macOS AVSpeechSynthesizer dev-build preview (`--features system_tts`);
  release binaries don't ship the sidecar yet. (#141, #144, #147)
- `--debug` flag / `KESHA_DEBUG=1` env traces engine subprocess calls to
  stderr without polluting the stdout pipe. (#149)

### Fixed
- `integration-tests` CI job installs `espeak-ng` on the macOS runner so the
  dynamic link against `libespeak-ng.1.dylib` resolves.

## [1.1.3] — 2026-04-18

First release with **bidirectional voice** — Kesha speaks back.

### Added
- `kesha say` TTS command with Kokoro-82M (English) + Piper VITS (Russian),
  auto-routed by `NLLanguageRecognizer` on input text. Opt-in via
  `kesha install --tts` (~390 MB). Output: WAV mono f32 (24 kHz Kokoro,
  22.05 kHz Piper) to stdout or `--out`. (#125, #126, #129)
- Programmatic API: `say`, `downloadTts` exported from
  `@drakulavich/kesha-voice-kit/core`.

### Fixed
- Build-engine feature matrix mirrors cargo defaults so released binaries
  include `tts`. (#133)
- `LIBCLANG_PATH` set from `llvm-config --libdir` on Linux CI runners so
  bindgen via `espeakng-sys` loads libclang correctly. (#133)

> **Release-notes note**: this release's GitHub notes body originally shipped
> empty because `gh release edit --notes` silently drops content on already
> published releases. Recovered via a direct API PATCH. See `CLAUDE.md` →
> "RELEASE PROCESS".

## [1.0.10] — 2026-04-16

### Changed
- README update for the npm package. No code changes since v1.0.9.

CLI-only release; engine v1.0.2 unchanged.

## [1.0.9] — 2026-04-16

### Added
- `--format` flag: `--format transcript` emits enriched plain text with a
  `[lang: …, confidence: …]` metadata line; `--format json` mirrors `--json`
  for symmetry. Recommended for OpenClaw `type: "cli"` audio providers.

CLI-only release; engine v1.0.2 unchanged.

## [1.0.8] — 2026-04-15

Rolls up OpenClaw-integration iterations v1.0.3–v1.0.8.

### Added
- OpenClaw `MediaUnderstandingProvider` that actually routes audio through
  the local `kesha` CLI (not the earlier stub + invented `configPatch`
  field). `autoPriority.audio: 50` selects Kesha over groq (20) when
  `tools.media.audio` is enabled.
- CLI-only marker releases via `-cli` tag suffix — excluded from
  `build-engine.yml`'s trigger filter so the Rust build is skipped.

### Changed
- Decoupled CLI and engine versioning. `src/engine-install.ts` reads
  `package.json#keshaEngine.version` (fallback: `package.json#version`) when
  deriving the GitHub release URL.
- Postinstall rewritten to probe for `bun` via pure `node:fs` instead of
  shelling out, so OpenClaw's `dangerous-exec` scanner accepts the tarball.
- `openclaw.plugin.json` cleaned up to use the real required fields (`id`,
  proper JSON Schema `configSchema`, `providers`); dropped the bogus
  `configPatch` block.

CLI-only release; engine v1.0.2 unchanged.

## [1.0.2] — 2026-04-15

Patch release. Engine v1.0.2.

## [1.0.0] — 2026-04-14

First stable release. Renamed from `@drakulavich/parakeet-cli`; the
`parakeet` command remains as a backward-compatible alias.

### Added
- Rust engine as a single binary — replaces `onnxruntime-node`, a separate
  Swift binary, and the `ffmpeg` runtime dependency.
- ~19× faster than Whisper on Apple Silicon (CoreML); ~2.5× faster on CPU
  (ONNX).
- 25 languages for speech-to-text; 107 languages for spoken language
  detection.
- OpenClaw skill: `openclaw plugins install @drakulavich/kesha-voice-kit`.
- "Did you mean?" command suggestion for typos.

### Migration from `@drakulavich/parakeet-cli`

```bash
bun remove -g @drakulavich/parakeet-cli
bun install -g @drakulavich/kesha-voice-kit
kesha install   # re-downloads engine + models
```

## [1.0.0-beta.5] — 2026-04-14

Final beta before the 1.0.0 rename / rewrite.
