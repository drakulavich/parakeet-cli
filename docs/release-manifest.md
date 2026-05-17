# Release Manifest

`kesha-release-manifest.json` is packaging metadata published with every engine
release. It is a small, stable JSON contract for future package-manager channels
such as Homebrew, deb, or rpm.

The manifest does not replace the user-facing Bun install path:

```bash
bun add -g @drakulavich/kesha-voice-kit
kesha install
```

## Contents

The manifest records:

- the repository, release tag, CLI version, and engine version
- released engine binaries and macOS sidecars
- the install layout used by `kesha install`
- supported platform status for package managers
- checksum and Sigstore bundle naming conventions

`SHA256SUMS` and Sigstore bundles cover the manifest itself, so downstream
packaging can verify the metadata before consuming it.

## Validation

Run the local consistency check after changing release asset names, install
layout, or release workflow packaging:

```bash
bun run check:release-manifest
```

The check fails if manifest metadata drifts from `src/engine-install.ts` or
`.github/workflows/build-engine.yml`.
