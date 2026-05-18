# Nix Install

Alternative reproducible-build path for Kesha Voice Kit. The Bun install (`bun add -g @drakulavich/kesha-voice-kit`) remains the canonical install — npm publish + the `kesha-engine` binaries from GitHub Releases are what CI gates against. The Nix flake is a parallel artifact for users who already live in Nix.

**Prerequisites:** [Nix](https://nixos.org/download/) with flakes enabled. Supported systems: `aarch64-darwin`, `x86_64-linux`.

## One-liner run (no install)
```bash
nix run github:drakulavich/kesha-voice-kit -- install      # downloads models (engine is bundled)
nix run github:drakulavich/kesha-voice-kit -- audio.ogg    # transcribe
```

`nix run` resolves to `apps.default` (the `kesha` Bun CLI), which has the engine binary baked in via `KESHA_ENGINE_BIN`, so there's no separate engine download.

## Install to profile (persistent)
```bash
nix profile install github:drakulavich/kesha-voice-kit
kesha install       # downloads models
kesha audio.ogg     # transcript to stdout
```

`packages.default` ships the Bun CLI (`kesha`) wired to the Nix-built engine. After `nix profile install`, the `kesha` shim is on `PATH` and runs transcription, language detection, and TTS (including `macos-*` AVSpeech voices on darwin-arm64) identically to the npm install. Speaker diarization (`kesha install --diarize` / `--speakers`) is not yet wired into the Nix build — the `kesha-diarize` Swift sidecar needs network-fetched FluidAudio at build time, which the Nix sandbox forbids; use the Bun install path (`bun add -g @drakulavich/kesha-voice-kit`) on darwin-arm64 if you need that feature (tracked alongside [#199](https://github.com/drakulavich/kesha-voice-kit/issues/199)).

## Engine only (no Bun, no Node)
For users who just want the Rust binary:
```bash
nix build github:drakulavich/kesha-voice-kit#kesha-engine
./result/bin/kesha-engine --help
./result/bin/kesha-engine --capabilities-json   # see which backends compiled in
```

## Development shell
```bash
nix develop github:drakulavich/kesha-voice-kit
# Now you have: pinned rustc/cargo (via rust-overlay), bun, protoc, cmake, pkg-config, libclang
```

## Why Nix?

- Reproducible builds across Linux/macOS
- All native deps (onnxruntime, protobuf, abseil) handled automatically
- No "works on my machine" — same `flake.nix` = identical results everywhere
