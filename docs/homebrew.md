# Homebrew Install

Kesha's Homebrew formula installs the Bun-based CLI wrapper. It does not
download the Rust engine or models during `brew install`; keep that explicit
with `kesha install`.

## Install

```bash
brew tap oven-sh/bun
brew install drakulavich/tap/kesha-voice-kit
kesha install
kesha audio.ogg
```

The formula depends on Bun from the official Bun tap and exposes both `kesha`
and the backward-compatible `parakeet` alias.

## Package Scope

Homebrew installs:

- the TypeScript CLI wrapper
- production Bun dependencies
- the `kesha` and `parakeet` commands

`kesha install` still downloads release assets into the Kesha cache. This keeps
the package install lightweight and preserves the no-surprise-downloads release
contract used by the Bun and Docker install paths.

## Maintainer Validation

The source formula remains in this repository and is mirrored into
`drakulavich/homebrew-tap` for users. To validate formula edits before a
release:

```bash
brew tap oven-sh/bun
brew tap-new local/tap
cp Formula/kesha-voice-kit.rb "$(brew --repository local/tap)/Formula/"
brew install local/tap/kesha-voice-kit
brew test local/tap/kesha-voice-kit
brew audit --strict --formula local/tap/kesha-voice-kit
```

The public tap itself can be validated with:

```bash
brew install drakulavich/tap/kesha-voice-kit
brew test drakulavich/tap/kesha-voice-kit
brew audit --strict --formula drakulavich/tap/kesha-voice-kit
```

Stable `vX.Y.Z` releases update the public tap through the `Homebrew Tap`
workflow. The workflow requires the `HOMEBREW_TAP_TOKEN` repository secret with
write access to `drakulavich/homebrew-tap`.
