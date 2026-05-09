# Contributing

Thanks for your interest in `@drakulavich/kesha-voice-kit`!

## Setup

```bash
git clone https://github.com/drakulavich/kesha-voice-kit.git
cd kesha-voice-kit
bun install
bun link
kesha install            # downloads the engine binary + ASR / lang-id models
kesha install --tts      # opt-in: Kokoro + Vosk-TTS (~990 MB)
kesha install --vad      # opt-in: Silero VAD model
```

The CLI is a Bun/TypeScript wrapper around `kesha-engine`, a Rust binary
downloaded from GitHub Releases at the version pinned in
`package.json#keshaEngine.version`. CLI and engine are versioned
independently — see [`CLAUDE.md`](./CLAUDE.md) "RELEASE PROCESS" for the
full split.

## Development

```bash
make test           # bun unit + integration tests
make lint           # bunx tsc --noEmit
make smoke-test     # bun link → kesha install → run against fixtures
make release        # lint + test + smoke-test
```

Rust engine work happens in `rust/`:

```bash
cd rust
cargo test --no-default-features --features onnx,tts --lib
cargo clippy --all-targets --no-default-features --features onnx,tts -- -D warnings
cargo fmt --check
```

`coreml` and `system_tts` are macOS-only features — `cargo check
--no-default-features --features coreml,tts,system_tts` runs on the
darwin-arm64 CI job.

## Project structure

```
kesha-voice-kit/
├── bin/kesha.js                # shebang entry (aliased as `parakeet` for legacy)
├── src/                        # Bun/TypeScript CLI + library
│   ├── cli.ts                  # citty argument parsing, --format, install/transcribe/status
│   ├── lib.ts                  # public API at @drakulavich/kesha-voice-kit/core
│   ├── engine.ts               # subprocess wrapper, capability cache, IPC types
│   ├── engine-install.ts       # engine binary download (uses keshaEngine.version)
│   ├── transcribe.ts           # thin forwarder to the engine; segments shape
│   ├── say.ts                  # TTS forwarder
│   ├── status.ts               # `kesha status` (cache disk usage)
│   └── log.ts                  # KESHA_DEBUG-aware logger
├── rust/                       # kesha-engine Rust binary
│   ├── Cargo.toml              # `onnx` (default) / `coreml` / `tts` / `system_tts` features
│   ├── build.rs                # Swift rpath under `coreml`; AVSpeech sidecar bake-in
│   ├── src/
│   │   ├── main.rs             # clap: transcribe / detect-lang / say / install / ...
│   │   ├── transcribe.rs       # ASR pipeline + VAD routing + timestamped segments
│   │   ├── audio.rs            # symphonia decode + rubato resample
│   │   ├── lang_id.rs          # ONNX speechbrain audio language detection
│   │   ├── text_lang.rs        # macOS NLLanguageRecognizer (macOS only)
│   │   ├── vad.rs              # Silero VAD v5 (576-sample rolling context)
│   │   ├── capabilities.rs     # `--capabilities-json` feature list
│   │   ├── tts/                # Kokoro + Vosk + AVSpeech + SSML
│   │   │   ├── kokoro.rs       # ONNX Kokoro-82M
│   │   │   ├── vosk.rs         # vosk-tts-rs wrapper
│   │   │   ├── avspeech.rs     # macOS AVSpeechSynthesizer Swift sidecar
│   │   │   ├── ssml.rs         # ssml-parser → Segment { Text, Spell, Emphasis, Break, Ipa }
│   │   │   ├── en/             # English acronym auto-expansion (#244)
│   │   │   ├── ru/             # Russian acronym auto-expansion (#232)
│   │   │   └── encode.rs       # WAV / OGG-Opus / MP3 encoder
│   │   ├── say_loop.rs         # `--stdin-loop` warm session for batch TTS
│   │   └── backend/            # transcribe backend trait + onnx + fluidaudio
│   └── tests/                  # cargo integration tests (warm --stdin-loop harness)
├── tests/{unit,integration}/   # bun:test
├── scripts/                    # benchmark.ts, smoke-test.ts
├── .github/workflows/
│   ├── ci.yml                  # PR: unit + integration + tts-e2e + type check
│   ├── rust-test.yml           # PR touching rust/: cargo test/fmt/clippy across 3 OSes
│   └── build-engine.yml        # tag push (v*, excluding -cli): build 3 binaries + draft release
├── raycast/                    # Raycast extension (separate npm tree, vendored)
├── openclaw.plugin.json        # OpenClaw manifest
├── openclaw-plugin.cjs         # OpenClaw entry
└── package.json                # @drakulavich/kesha-voice-kit
```

## Pull requests

- Branch from `main`. Don't pile unrelated changes into one PR.
- Run `make test && make lint` before pushing. For Rust changes, also `cd
  rust && cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`.
- CI must pass before merging. `main` is protected.
- Squash-merge preferred. Greptile reviews are advisory but their P1/P2
  findings should be addressed before merge.
- For active work, tag the issue with the `WIP` label so the maintainer
  sees it at a glance:
  ```bash
  gh issue edit <N> -R drakulavich/kesha-voice-kit --add-label WIP
  ```

## Code style

- TypeScript strict mode, ESNext target, Bun runs `.ts` directly.
- Bun-native APIs (`Bun.spawn`, `Bun.write`, `Bun.file`) — no Node `child_process`.
- `console.error()` for progress + errors (stderr stays diagnostic);
  `console.log()` / `process.stdout.write()` for piped output.
- Relative imports (`./engine`, not `src/engine`).
- Rust: `cargo fmt` + `cargo clippy --all-targets -- -D warnings` are
  CI-fatal. Don't suppress lints with `#[allow(dead_code)]` — see
  [`CLAUDE.md`](./CLAUDE.md) "NO SPECULATIVE FIELDS OR ENUM VARIANTS".

## Error handling

- Human-readable messages: what failed, why, what to do.
- Never swallow errors silently. Never return success on failure.
- For TTS / ASR install errors, use the bordered ASCII install hint (see
  `src/transcribe.ts` for the canonical shape).

## Tests

- Unit tests in `tests/unit/` — no external deps, run on
  Linux/Windows/macOS.
- Integration tests in `tests/integration/` — exercise the actual engine
  binary, run on macos-14 in CI.
- Rust integration tests in `rust/tests/` — `cargo test` runs them on
  Linux/Windows/macOS via the warm `--stdin-loop` harness.
- `audio-quality-check` agent runs after every commit touching
  `rust/src/tts/**` (see `.claude/agents/audio-quality-check.md`).

## CI workflows

- `ci.yml` — runs on PRs: `changes` filter → unit-tests (3 OSes) +
  integration-tests + tts-e2e + raycast-lint + pr-comment.
  `integration-tests` is skipped on `release/*` branches (release
  chicken-and-egg: pinned engine tag doesn't exist yet).
- `rust-test.yml` — runs on PRs touching `rust/**`: `cargo test/fmt/clippy`
  on 3 OSes + `cargo check --features coreml --no-default-features` on
  macos-14.
- `build-engine.yml` — runs on `v*` tag pushes (excluding `v*-cli`):
  builds 3 platform binaries, smoke-tests each with `--capabilities-json`,
  creates a draft release.
- No inline scripts > 3 lines — extract to `.github/scripts/`.

## Releases

The full release runbook lives in [`CLAUDE.md`](./CLAUDE.md) "RELEASE
PROCESS". Quick orientation:

- **Engine release** (any change under `rust/`, or bumping
  `keshaEngine.version`): bump `rust/Cargo.toml` + `rust/Cargo.lock` +
  `package.json#version` + `package.json#keshaEngine.version` in lockstep
  on a `release/X.Y.Z` branch → merge → tag `vX.Y.Z` → write release notes
  on the **draft** release before publishing → independent validation
  (download the binary, run end-to-end) → `npm publish --access public`.

- **CLI-only patch** (docs, TS fix, plugin tweak): bump only
  `package.json#version` → merge → `npm publish` → tag `vX.Y.Z-cli` (the
  `-cli` suffix excludes the tag from `build-engine.yml` so no Rust
  rebuild fires).

Tag names are one-shot — GitHub's immutable releases permanently reserve
them after publish. Broken release → bump patch and cut a new tag. Never
tag "just to test"; use `gh workflow run "🔨 Build Engine" --ref main`.

## License

By contributing, you agree that your contributions will be licensed under
the MIT License (see [`LICENSE`](./LICENSE)).
