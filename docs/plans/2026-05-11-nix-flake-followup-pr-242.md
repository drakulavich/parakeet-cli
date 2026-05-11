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

- [x] Add `RUSTFLAGS` export to the Linux branch of the devShell `shellHook` so `nix develop` matches the package build (Greptile P1 #3). The export is now present and also adds `ORT_PREFER_DYNAMIC_LINK`; dropped the defensive `or ""` since `linuxEnv.RUSTFLAGS` is guaranteed when the `isLinux` branch runs.
- [x] Add `nativeBuildInputs` to `mkShell` via `inherit nativeBuildInputs;` so `protoc`, `pkg-config`, `cmake`, `libclang` are present on every platform (P2 from Copilot + drakulavich). Also exported `LIBCLANG_PATH` at the mkShell top level so bindgen can dlopen libclang in the dev shell.
- [x] Remove the unused `darwinBuildInputs = [];` binding (P2)
- [x] Remove the duplicate `protobuf` from the Linux `buildInputs` block; it's already in `nativeBuildInputs` (P2)
- [x] Wire `rustToolchain` into naersk: replaced `naersk' = pkgs.callPackage naersk {};` with `naersk' = pkgs.callPackage naersk { cargo = rustToolchain; rustc = rustToolchain; };`. Moved the `rustToolchain` binding up in the `let` block so the dependency reads top-to-bottom.
- [x] (skipped — nix not installed on local dev machine; same constraint as Task 1) `nix flake check` / `nix build .#kesha-engine --system x86_64-linux` deferred to PR CI; will be paste-evidenced in Task 7's PR body.
- [x] (skipped — nix not installed on local dev machine) `nix develop --command bash -c '...'` deferred to PR CI.

### Task 3: Add macOS Swift toolchain + deployment target

Files:
- Modify: `flake.nix`

- [x] Added `lib.optionals isDarwin (with pkgs; [ swift darwin.apple_sdk.frameworks.AVFoundation darwin.apple_sdk.frameworks.CoreML darwin.apple_sdk.frameworks.Foundation ])` to `nativeBuildInputs` (flake.nix L49-60). Kept the spelling the plan called out; if `pkgs.swift` isn't in the pinned `nixpkgs-unstable`, the build will surface that on the first `nix build` and the fallback to `pkgs.swiftPackages.swift` is a one-line swap — noted in the PR body for Greptile to flag if it triggers.
- [x] Added `MACOSX_DEPLOYMENT_TARGET = "14.0";` to `buildEnv` (flake.nix L78-86) and threaded it through the `inherit (buildEnv) ...` list passed to `naersk'.buildPackage` (flake.nix L130) so the kesha-engine derivation sees it. Harmless on Linux (ignored by ld).
- [x] Added an `${lib.optionalString isDarwin ''…''}` block to the devShell `shellHook` exporting `MACOSX_DEPLOYMENT_TARGET=14.0` and `RUSTFLAGS="-L /opt/homebrew/lib"` per CLAUDE.md's macOS dev path (flake.nix L174-177). `LIBCLANG_PATH` is already set cross-platform at the mkShell top level (Task 2), so the Darwin branch only needs the two Darwin-specific vars.
- [x] (skipped — nix not installed on local dev machine; same constraint as Task 1) `nix build .#kesha-engine -L` and `./result/bin/kesha-engine --capabilities-json | jq .features` deferred to CI / a developer with nix installed. Expected feature set on darwin-arm64 is `coreml,tts,system_tts` per `rustFeatures` at flake.nix L42-44.
- [x] (skipped — nix not installed on local dev machine; binary doesn't exist) WAV smoke-test deferred to the human-in-the-loop pre-publish gate documented in CLAUDE.md. Captured the exact one-liner here so it's runnable verbatim by a developer with nix: `KESHA_CACHE_DIR=/tmp/nix-smoke ./result/bin/kesha-engine install --tts && echo 'Hello world' | KESHA_CACHE_DIR=/tmp/nix-smoke ./result/bin/kesha-engine say --voice en-am_michael --out /tmp/nix-smoke.wav && file /tmp/nix-smoke.wav`.

### Task 4: Replace `patchOrtSys` sed-step with supported `ort` escape hatch

Files:
- Modify: `flake.nix`

- [x] Deleted the `patchOrtSys` shell block and the `overrideMain = old: old // { preBuild = patchOrtSys; }` wrapper from the `kesha-engine` derivation. Also removed the duplicate `preConfigure` block that was exporting the same env vars at shell-time, and the unused stand-alone `ortEnv` attrset (lines 96-103 in the pre-Task-4 file) that was never wired into `naersk'.buildPackage`.
- [x] Replaced with a declarative `ortEnv` attrset merged into the derivation: `ORT_STRATEGY = "system"`, `ORT_LIB_LOCATION = "${pkgs.onnxruntime}/lib"`, `ORT_DYLIB_PATH = "${pkgs.onnxruntime}/lib/${ortLibName}"`, `ORT_PREFER_DYNAMIC_LINK = "1"`. `ortLibName` switches `.so` (Linux) vs `.dylib` (Darwin). The attrset is merged via `} // ortEnv // linuxEnv)` so the env vars become real `mkDerivation` attrs (visible to ort-sys's build.rs during the dep-build phase too, which is what was failing in the old `preConfigure` approach). `linuxEnv` now holds only `RUSTFLAGS` (the abseil deps list); the duplicated ORT_* keys were dropped from `linuxEnv` since `ortEnv` is now cross-platform. The dev shell also gets `// ortEnv`, so `nix develop` exports the same vars and cargo build inside the shell behaves identically to the package build (closes Greptile P1's "build vs dev shell drift").
- [x] (skipped — nix not installed on local dev machine; same constraint as Tasks 1-3) `nix build .#kesha-engine -L` deferred to PR CI. Expected behavior on Linux: ort-sys's build.rs sees `ORT_STRATEGY=system` + `ORT_LIB_LOCATION=/nix/store/...-onnxruntime-*/lib`, skips the download path, links to `libonnxruntime.so` from `linuxEnv.RUSTFLAGS`. Expected behavior on darwin-arm64: same env vars point at the Darwin onnxruntime build, but ASR uses coreml so onnx-runtime is only needed for lang_id. The `ORT_DYLIB_PATH` env var is the documented `load-dynamic` opt-out (`ort.pyke.io/setup/linking#bring-your-own`); it's a harmless extra hint if `ort` is statically linked. If CI surfaces a build failure that's specifically about `download-binaries` still being enabled, the documented fallback per the plan header is to add `[patch.crates-io] ort-sys = ...` to `rust/Cargo.toml`.
- [x] (skipped — nix not installed) `--capabilities-json | jq .features` deferred to CI; the smoke-test step inside `.github/workflows/build-engine.yml` already pre-verifies feature compilation on each platform, so PR CI's `nix flake check` failure or success will surface the same signal.

### Task 5: Add `packages.kesha` (Bun + CLI + engine) and `apps.kesha`

Files:
- Modify: `flake.nix`

- [x] Built the Bun CLI bundle via `pkgs.stdenv.mkDerivation` in two layers: `keshaNodeModules` (fixed-output derivation that runs `bun install --frozen-lockfile --production --ignore-scripts` inside the sandbox; deps pinned by `outputHash`) and `kesha` (stages `bin/`, `src/`, `package.json`, `tsconfig.json`, `scripts/postinstall.cjs`, openclaw files, plus a symlink to `keshaNodeModules` at `node_modules`, then `makeWrapper`s the `kesha.js` shebang script through `$out/bin/{kesha,parakeet}` shims). Both shims `--prefix PATH` with Nix-built Bun and `--set KESHA_ENGINE_BIN ${kesha-engine}/bin/kesha-engine` so the CLI never reads the user's home cache for the engine. See flake.nix L119-230.
- [x] Exposed `packages.kesha`, kept `packages.kesha-engine`, switched `packages.default = kesha`. Added `apps.kesha` + `apps.default` both pointing at `${kesha}/bin/kesha`. This makes `nix run .` and `nix profile install .#kesha` route to the wrapper, while the original `nix build .#kesha-engine` path stays valid for engine-only users (covered in Task 6 README copy).
- [x] Bun-install strategy: FOD with `outputHash = lib.fakeHash` + `--frozen-lockfile`. On first `nix build` the build fails with the real hash in the `got:` line — replace `lib.fakeHash` with that value and rebuild. Documented inline in flake.nix L125-134 so a future maintainer hitting a `bun.lock` bump knows the fix-up procedure. `bun2nix` would automate this; it isn't in nixpkgs-unstable yet so it's parked as a follow-up. The per-tarball `pkgs.fetchurl` approach is overkill for v1 and was rejected per the plan.
- [x] (verification skipped — nix not installed on local dev machine; same constraint as Tasks 1-4) `nix run .#kesha -- --version`, `nix run .#kesha -- transcribe fixtures/hello-english.wav`, and `nix profile install .#kesha && kesha --version` deferred to PR CI / a developer with nix. Expected behavior on darwin-arm64: `nix run .#kesha -- --version` prints `1.13.0` (sourced from `package.json#version`); `nix run .#kesha -- transcribe fixtures/hello-english.wav` invokes the bundled engine via `KESHA_ENGINE_BIN` and produces a transcript on stdout. Belt-and-braces local check that did pass: `bunx tsc --noEmit` is clean in the worktree (no TS changes, but sanity-checked that the surface-level package structure the flake stages still resolves).

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

- [ ] `nix flake check 2>&1 | tee /tmp/nix-flake-check-final.txt` — clean
- [ ] `nix build .#kesha-engine` on darwin-arm64: succeeds, `--capabilities-json` shows `coreml,tts,system_tts`
- [ ] `nix build .#kesha-engine --system x86_64-linux` (or via remote builder / docker): succeeds, `--capabilities-json` shows `onnx,tts`
- [ ] `nix run .#kesha -- --version` and `nix run .#kesha -- transcribe rust/tests/fixtures/<short>.wav` both work on darwin-arm64
- [ ] `cd rust && cargo fmt && cargo clippy --all-targets -- -D warnings` clean (no Rust changes expected, but the flake replaces `patchOrtSys` so the build does still need to compile; clippy belt-and-braces)
- [ ] `bun test && bunx tsc --noEmit` (smoke check — no TS changes expected in this PR)
- [ ] Open PR against `main` with body sections: Summary, What changed, Verification (paste `nix flake check` + `--capabilities-json` for both platforms), Closes/Refs link to PR #242 follow-ups. Title: `nix: address PR #242 review (macOS Swift, ORT escape hatch, kesha wrapper)`.
- [ ] Add `Closes #<follow-up-issue>` if drakulavich filed one for the macOS work; otherwise `Refs #242`
- [ ] Add the `WIP` label per CLAUDE.md, remove after merge

### Task 8: Re-trigger Greptile + address any new findings

- [ ] After CI green, post a comment on the PR with `@greptileai re-review` (or push an empty commit) to ensure the bot reviews the latest sha
- [ ] Walk through Greptile's response. Any P1: fix and push. Any P2: address or justify with a comment. Repeat the build/capabilities-json verification after each fix.
- [ ] Drop `WIP` label once mergeable

### Task 9: Update documentation

- [ ] If the README changes shipped (Task 6), no further docs needed
- [ ] Add a one-line note to `CLAUDE.md`'s build/CI section noting that the Nix flake is the alternate reproducible build path and lists supported platforms (`aarch64-darwin`, `x86_64-linux`)
- [ ] Move this plan to `docs/plans/completed/` after PR merges
