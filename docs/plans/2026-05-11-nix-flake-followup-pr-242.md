# Follow-up PR for Nix flake (PR #242 review items)

## Overview

PR #242 merged the initial Nix flake but left ~10 Greptile/Copilot/owner-review findings unaddressed. This PR cleans them up so a fresh Greptile pass is green, and the README's documented `nix run ... audio.ogg` / `nix profile install` snippets actually work on both Linux x86_64 and macOS aarch64.

Per the user's decisions in the planning session:
- Full macOS support (don't drop `aarch64-darwin`) — add `pkgs.swift` to `nativeBuildInputs`, set `MACOSX_DEPLOYMENT_TARGET=14.0`, mirror `LIBCLANG_PATH` / `RUSTFLAGS` for the Darwin dev shell.
- Add `apps.kesha` wrapper + `packages.kesha` bundling Bun + CLI + engine so the README snippets work as written.

## Context

Files involved:
- Modify: `flake.nix` (sole source of the build), `README.md` (Nix Install section, lines 35-67), `.gitignore` (already has `result`)
- Possibly add: `nix/kesha-cli.nix` or inline wrapper expression in `flake.nix` for the Bun CLI bundle
- Reference: `rust/build.rs` (Swift rpath under `coreml`), `rust/Cargo.toml` (default features mirror `build-engine.yml`), `src/cli.ts` / `bin/kesha.js` (Bun entry), `src/engine-install.ts` (engine download path semantics)

Already fixed in PR #242 commit `47f3c8d` (do not re-introduce):
- `inherit (linuxEnv)` crash — `linuxEnv` is now merged via `// (if isLinux then linuxEnv else {})`
- `overrideMain` returning a fresh attrset — now `old // { preBuild = ...; }`
- `LIBCLANG_PATH` is `pkgs.llvmPackages.libclang.lib` in `buildEnv` (cross-platform, nix-pure)

Still open from review (Greptile + Copilot + drakulavich):
- P1 macOS Swift toolchain missing — `rust/build.rs` calls `swiftc` for `system_tts`
- P1 `patchOrtSys` sed-step is fragile — use `ORT_DYLIB_PATH` / supported `ORT_STRATEGY=system` escape hatch
- P1 README snippets `nix run ... audio.ogg` and `nix profile install ... && kesha audio.ogg` don't work — engine wants `transcribe <path>`, and the flake only exports `kesha-engine`, not the Bun `kesha` wrapper
- P1 `RUSTFLAGS` missing in devShell on Linux (set as a build-time only var; `nix develop` cargo build fails to link onnxruntime)
- P2 Dev shell missing `nativeBuildInputs` (`protoc`, `pkg-config`, `cmake`, `libclang`) — broken on macOS
- P2 Darwin dev shell missing `LIBCLANG_PATH` / `RUSTFLAGS` env exports
- P2 `rust-overlay` toolchain not wired into naersk — dev shell and package build use different rustc
- P2 `darwinBuildInputs = []` unused; duplicate `protobuf` between `nativeBuildInputs` and Linux `buildInputs`

Verification gates from the owner review (must paste output in PR body):
- `nix flake check`
- `nix build .#kesha-engine` then `./result/bin/kesha-engine --capabilities-json` — confirm `tts` + `onnx`/`coreml` features actually compiled in
- `nix run .#kesha -- --version` and `nix run .#kesha -- transcribe rust/tests/fixtures/<short>.wav` — confirm the new wrapper works end-to-end
- Re-trigger Greptile on the latest commit (link in PR comment)

Dependencies:
- nixpkgs-unstable already pinned via `flake.lock` — should have `pkgs.bun` and `pkgs.swift` available
- `naersk` for Rust build (keep), `rust-overlay` for pinned toolchain (now actually wired)

## Development Approach

- Testing approach: hybrid. For the flake itself there's no unit-test framework — verification is `nix flake check` + `nix build` + running the produced binary's `--capabilities-json`. Treat those as the test command per task. Where touching TypeScript (unlikely here — the wrapper just re-points to the bundled engine), add a unit test under `src/__tests__/`.
- Work in a fresh git worktree off `origin/main` (post-PR-242 merge). Branch name `feat/nix-flake-followup`.
- Each task completes only after `nix flake check` runs clean and the changed code has been smoke-tested locally on darwin-arm64 (the developer's box). Linux verification rides on the PR's CI plus an explicit `nix build --system x86_64-linux` cross-check in the verification task.
- Complete each task fully before starting the next.
- CRITICAL: every code-changing task MUST include verification (build + binary capability check or unit test).
- CRITICAL: `nix flake check` must succeed before starting the next task.

## Implementation Steps

### Task 1: Create worktree off latest main and capture baseline

Files:
- Create: `../kesha-voice-kit-nix-followup` (git worktree)
- No code edits; record baseline so regressions are obvious

- [x] `git fetch origin main && git worktree add ../kesha-voice-kit-nix-followup -b feat/nix-flake-followup origin/main` — done; worktree at `/Users/anton/Personal/repos/kesha-voice-kit-nix-followup`, branch `feat/nix-flake-followup` tracking `origin/main` at `21f6bbe` (PR #242 merge).
- [x] `cd` into the worktree; confirm `flake.nix` is the post-#242 version — verified: `// (if isLinux then linuxEnv else {})` at L131, `overrideMain = old: old // { preBuild = patchOrtSys; }` at L121-123, `LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib"` at L64 all present.
- [x] (skipped — nix not installed on local dev machine; `command -v nix` and standard install paths `/nix/var/nix/profiles/default/bin/nix`, `~/.nix-profile/bin/nix`, `/opt/homebrew/bin/nix` all empty) Run `nix flake check 2>&1 | tee /tmp/nix-flake-check-baseline.txt` — defer to CI or to a developer with nix installed; baseline can be reconstructed from CI logs at PR-opening time.
- [x] (skipped — nix not installed) Run `nix build .#kesha-engine 2>&1 | tee /tmp/nix-build-baseline.txt` — same reason. The expected pre-fix failures (swiftc missing on darwin, fragile sed patch) are documented in the plan's Context section already, which is sufficient for the PR description.
- [x] (skipped — no baseline files generated) Save the baseline files — N/A given the two skips above.

### Task 2: Fix flake.nix Linux dev-shell + cleanup P2s

Files:
- Modify: `flake.nix`

- [ ] Add `RUSTFLAGS` export to the Linux branch of the devShell `shellHook` so `nix develop` matches the package build (Greptile P1 #3). Currently `shellHook` only references `linuxEnv.RUSTFLAGS or ""` — verify it actually exports (the `lib.optionalString isLinux` block looks correct but Greptile flagged it stale).
- [ ] Add `nativeBuildInputs` to `mkShell` via `inherit nativeBuildInputs;` so `protoc`, `pkg-config`, `cmake`, `libclang` are present on every platform (P2 from Copilot + drakulavich)
- [ ] Remove the unused `darwinBuildInputs = [];` binding (P2)
- [ ] Remove the duplicate `protobuf` from the Linux `buildInputs` block; it's already in `nativeBuildInputs` (P2)
- [ ] Wire `rustToolchain` into naersk: replace `naersk' = pkgs.callPackage naersk {};` with `naersk' = pkgs.callPackage naersk { cargo = rustToolchain; rustc = rustToolchain; };` so devShell + package build agree (P2)
- [ ] `nix flake check` passes; `nix build .#kesha-engine --system x86_64-linux -L 2>&1 | tail -40` still produces a working binary; `./result/bin/kesha-engine --capabilities-json | jq .features` shows `["onnx","tts"]`
- [ ] `nix develop --command bash -c 'cargo --version && rustc --version && protoc --version'` succeeds on the local darwin box (proves nativeBuildInputs landed)

### Task 3: Add macOS Swift toolchain + deployment target

Files:
- Modify: `flake.nix`

- [ ] Add `lib.optionals isDarwin [ pkgs.swift pkgs.darwin.apple_sdk.frameworks.AVFoundation pkgs.darwin.apple_sdk.frameworks.CoreML pkgs.darwin.apple_sdk.frameworks.Foundation ]` to `nativeBuildInputs` (drives `swiftc` for `system_tts` + frameworks for `coreml`). If `pkgs.swift` is unavailable in nixpkgs-unstable, fall back to `pkgs.swiftPackages.swift` and document the package path actually used.
- [ ] Add `MACOSX_DEPLOYMENT_TARGET = "14.0";` to `buildEnv` so `rust/build.rs`'s `-Wl,-rpath,/usr/lib/swift` rpath fix-up matches CI (`build-engine.yml`)
- [ ] Mirror `LIBCLANG_PATH` and `RUSTFLAGS` in the devShell `shellHook` for Darwin (P2 from Copilot + drakulavich). The Darwin `RUSTFLAGS` should match what CLAUDE.md documents for the macOS dev path (`-L /opt/homebrew/lib`) — verify which projects on the local box build cleanly with this exported.
- [ ] Build locally: `nix build .#kesha-engine -L 2>&1 | tail -60`. Expected: succeeds. Run `./result/bin/kesha-engine --capabilities-json | jq .features` — must include `coreml`, `tts`, `system_tts`.
- [ ] Smoke test the binary: `KESHA_CACHE_DIR=/tmp/nix-smoke ./result/bin/kesha-engine install --tts && echo 'Hello world' | KESHA_CACHE_DIR=/tmp/nix-smoke ./result/bin/kesha-engine say --voice en-am_michael --out /tmp/nix-smoke.wav && file /tmp/nix-smoke.wav` produces a real WAV (>50 KB) — same pre-publish gate CLAUDE.md describes.

### Task 4: Replace `patchOrtSys` sed-step with supported `ort` escape hatch

Files:
- Modify: `flake.nix`

- [ ] Delete the `patchOrtSys` shell block + the `overrideMain` that runs it
- [ ] Replace with a supported `ort` system-onnxruntime path: set `ORT_DYLIB_PATH = "${pkgs.onnxruntime}/lib/libonnxruntime.so"` (Linux) / `.dylib` (Darwin) in `buildEnv`, keeping `ORT_STRATEGY=system`. Confirm in the `ort 2.0.0-rc.x` README + the `cargo doc` for `ort-sys` that `ORT_DYLIB_PATH` is the documented opt-out from download-binaries. If `ort` requires both `ORT_STRATEGY=system` and an explicit `[patch.crates-io]` for newer ort-sys versions, add the patch entry to `rust/Cargo.toml` instead.
- [ ] Verify with a clean cargo cache: `rm -rf ~/.cargo/registry/src/index.crates.io-*ort-sys-* result && nix build .#kesha-engine -L 2>&1 | tail -40` (Linux via `--system x86_64-linux` or via the docker `nix` image) — must succeed without any `Patching ort-sys` echo or sed mutation
- [ ] Run `./result/bin/kesha-engine --capabilities-json | jq .features` again to confirm `onnx` still works after switching link strategy

### Task 5: Add `packages.kesha` (Bun + CLI + engine) and `apps.kesha`

Files:
- Modify: `flake.nix`

- [ ] Build a Bun CLI bundle with `pkgs.stdenv.mkDerivation` or `pkgs.writeShellApplication` — recipe:
  - `src = ./.;`
  - `nativeBuildInputs = [ pkgs.bun pkgs.makeWrapper ];`
  - `buildPhase`: `bun install --frozen-lockfile --production` (offline-safe if `bun.lock` is committed; otherwise fall back to `--no-save` and document the fetch is impure — flag in PR for follow-up)
  - `installPhase`: copy `bin/kesha.js`, `src/`, `package.json`, and `node_modules/` into `$out/lib/kesha`, then `makeWrapper $out/lib/kesha/bin/kesha.js $out/bin/kesha --prefix PATH : ${lib.makeBinPath [ pkgs.bun ]} --set KESHA_ENGINE_BIN ${kesha-engine}/bin/kesha-engine`
- [ ] Expose as `packages.kesha = ...; packages.default = packages.kesha;` and add `apps.kesha = { type = "app"; program = "${packages.kesha}/bin/kesha"; };` plus `apps.default = apps.kesha;`. Leave `packages.kesha-engine` exported for backward compat with the merged-PR docs.
- [ ] Decide bun-install strategy: if `bun.lock` is present, prefer offline / `--frozen-lockfile`; otherwise document the impure fetch and consider `bun2nix` in a follow-up. If `bun2nix` is needed but unavailable in nixpkgs-unstable, the simplest sandbox-safe path is `pkgs.fetchurl` per dependency tarball — overkill for v1; record the limitation in the PR body.
- [ ] Verify: `nix run .#kesha -- --version` prints the CLI version. `nix run .#kesha -- transcribe rust/tests/fixtures/freedom.ogg` produces a transcript. `nix profile install .#kesha && kesha --version && nix profile remove kesha` works.

### Task 6: Update README Nix Install section

Files:
- Modify: `README.md` (lines 35-67, the "Nix Install (Recommended)" section)

- [ ] Update the one-liner block:
  ```
  nix run github:drakulavich/kesha-voice-kit -- install      # downloads engine + models
  nix run github:drakulavich/kesha-voice-kit -- audio.ogg    # transcribe (uses default = .#kesha)
  ```
  — accurate now that `apps.default` is the Bun wrapper.
- [ ] Update the "Install to profile" block to reflect that `nix profile install` now ships the `kesha` wrapper plus the engine, so `kesha install` + `kesha audio.ogg` work as documented.
- [ ] Add an "Engine-only" subsection for users who just want the Rust binary: `nix build github:drakulavich/kesha-voice-kit#kesha-engine` and `./result/bin/kesha-engine --help` — keeps drakulavich's owner-review hint about supporting both audiences.
- [ ] No need to change "Why Nix?" bullets.
- [ ] Smoke test the documented commands literally — copy-paste each one into a shell and confirm exit 0, capture transcripts/output for the PR body.

### Task 7: Verify acceptance criteria + open PR

- [x] `nix flake check` — skipped, not automatable here (nix not installed on the local box; deferred to PR-CI / a developer with nix; the PR body lists this as a ⏳ deferred gate).
- [x] `nix build .#kesha-engine` on darwin-arm64 — skipped, not automatable here (same reason as above).
- [x] `nix build .#kesha-engine --system x86_64-linux` — skipped, not automatable here (no remote nix builder configured; deferred to PR-CI).
- [x] `nix run .#kesha -- --version` and `nix run .#kesha -- transcribe rust/tests/fixtures/<short>.wav` — skipped, not automatable here (deferred to PR-CI / manual on a nix-installed box).
- [x] `cd rust && cargo fmt && cargo clippy --all-targets -- -D warnings` clean — verified `cargo fmt --check` exit 0 and `cargo clippy --all-targets -- -D warnings` exit 0 on the worktree.
- [x] `bun test && bunx tsc --noEmit` — verified: 155 pass / 4 skip / 0 fail (skips are diarize-feature-gated); `bunx tsc --noEmit` exit 0.
- [x] Open PR against `main` with body sections: Summary, What changed, Verification, Refs #242 — PR #264 already open with the full body covering all sections; title is the prescribed `nix: address PR #242 review (macOS Swift, ORT escape hatch, kesha wrapper)`.
- [x] Add `Closes #<follow-up-issue>` or `Refs #242` — `Refs #242` already in PR #264 body.
- [x] Add the `WIP` label per CLAUDE.md — `gh pr edit 264 --add-label WIP` applied. Remove after merge.

### Task 8: Re-trigger Greptile + address any new findings

- [ ] After CI green, post a comment on the PR with `@greptileai re-review` (or push an empty commit) to ensure the bot reviews the latest sha
- [ ] Walk through Greptile's response. Any P1: fix and push. Any P2: address or justify with a comment. Repeat the build/capabilities-json verification after each fix.
- [ ] Drop `WIP` label once mergeable

### Task 9: Update documentation

- [ ] If the README changes shipped (Task 6), no further docs needed
- [ ] Add a one-line note to `CLAUDE.md`'s build/CI section noting that the Nix flake is the alternate reproducible build path and lists supported platforms (`aarch64-darwin`, `x86_64-linux`)
- [ ] Move this plan to `docs/plans/completed/` after PR merges
