# Linux Packages

Kesha publishes `.deb` and `.rpm` packages for Linux x64 on stable engine
releases. They install a standalone Bun-compiled CLI wrapper as `kesha` and
the backward-compatible `parakeet` alias. Engine binaries and models are still
downloaded explicitly with `kesha install`.

## Debian / Ubuntu

```bash
gh release download vX.Y.Z \
  -R drakulavich/kesha-voice-kit \
  -p 'kesha-voice-kit_*_amd64.deb'
sudo apt install ./kesha-voice-kit_*_amd64.deb
kesha install
kesha audio.ogg
```

## Fedora / RHEL

```bash
gh release download vX.Y.Z \
  -R drakulavich/kesha-voice-kit \
  -p 'kesha-voice-kit-*.x86_64.rpm'
sudo dnf install ./kesha-voice-kit-*.x86_64.rpm
kesha install
kesha audio.ogg
```

## Package Scope

The Linux packages install:

- `/usr/bin/kesha`
- `/usr/bin/parakeet`
- license, notices, and README under `/usr/share/doc/kesha-voice-kit`

They depend on `ca-certificates` so `kesha install` can download release assets
and model files over HTTPS. They do not install the Rust engine or model files
during package installation.

## Maintainer Validation

```bash
go install github.com/goreleaser/nfpm/v2/cmd/nfpm@v2.43.4
node .github/scripts/build-linux-packages.mjs
sudo apt install ./dist/linux-packages/kesha-voice-kit_*_amd64.deb
kesha --version
kesha install --plan
sudo apt remove kesha-voice-kit
```
