{
  description = "Kesha Voice Kit - fast multilingual voice toolkit with Bun CLI and Rust engine";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    naersk = {
      url = "github:nix-community/naersk";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, flake-utils, naersk, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        inherit (pkgs) lib;

        # Get Rust toolchain from rust-overlay (declared early so naersk can pick it up)
        rustToolchain = pkgs.rust-bin.stable.latest.default;

        # Wire the pinned rust-overlay toolchain into naersk so the package build
        # and the dev shell agree on rustc / cargo.
        naersk' = pkgs.callPackage naersk {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };

        # Platform detection
        isLinux = lib.hasSuffix "linux" system;
        isDarwin = lib.hasSuffix "darwin" system;
        isAarch64 = lib.hasPrefix "aarch64" system;

        # Rust features per platform
        # Note: --no-default-features disables download-binaries from ort/ort-sys
        #
        # darwin-arm64 deliberately uses `onnx` rather than `coreml`. The
        # `coreml` feature pulls in `fluidaudio-rs`, whose build script
        # invokes `swift build` against a Package.swift that depends on
        # `github.com/FluidInference/FluidAudio.git`. Nix derivations run in
        # a sandboxed, offline environment, so the SwiftPM clone fails. The
        # canonical darwin release (`build-engine.yml`, pinned Xcode 16.2)
        # still ships the CoreML backend; this flake lane validates the
        # ONNX path + Swift toolchain + Apple SDK frameworks + the
        # `say-avspeech` sidecar postInstall on darwin.
        rustFeatures = if isDarwin && isAarch64
          then "onnx,tts,system_tts"
          else "onnx,tts";

        # Build-time dependencies (tools needed to compile).
        # `swift` drives `rust/build.rs` for the `system_tts` feature on
        # darwin and is a build-host toolchain, so it stays here. Apple SDK
        # frameworks are link-time inputs and live in `buildInputs` below,
        # where the nixpkgs Darwin linker hook picks them up via `-F`.
        nativeBuildInputs = with pkgs; [
          protobuf
          llvmPackages.libclang
          pkg-config
          cmake
          makeWrapper
        ] ++ lib.optionals isDarwin [
          # `swift` is the compiler (swiftc); `swiftpm` is the package manager
          # that exposes `swift build` / `swift run` / `swift test`. They live
          # in separate nixpkgs derivations — including only `swift` leaves
          # the swift wrapper failing at `exec: swift-build: not found` when
          # fluidaudio-rs's build.rs shells out to `swift build` to compile
          # its FluidAudioBridge Swift package.
          pkgs.swift
          pkgs.swiftpm
        ];

        # Runtime / link-time dependencies. `protobuf` is in `nativeBuildInputs`.
        buildInputs = with pkgs; [
          openssl
          opus
        ] ++ lib.optionals isLinux (with pkgs; [
          clang
          llvmPackages.llvm
          onnxruntime
          abseil-cpp
        ]) ++ lib.optionals isDarwin (with pkgs; [
          # The legacy `darwin.apple_sdk.frameworks.{AVFoundation,CoreML,Foundation}`
          # stubs were removed from nixpkgs (apple_sdk_11_0 → apple-sdk migration).
          # The modern `apple-sdk` package is the umbrella that exposes every
          # framework the system SDK ships — fluidaudio-rs's `-framework CoreML`
          # / `-framework AVFoundation` link directives resolve through it.
          # Docs: https://nixos.org/manual/nixpkgs/stable/#sec-darwin-legacy-frameworks
          apple-sdk
        ]);

        # Environment variables for build - passed directly to mkDerivation.
        # MACOSX_DEPLOYMENT_TARGET=14.0 mirrors build-engine.yml so the
        # `-Wl,-rpath,/usr/lib/swift` rpath fix-up in rust/build.rs lines up
        # with the runner SDK; harmless on Linux (ignored by ld).
        buildEnv = {
          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
          PROTOC = "${pkgs.protobuf}/bin/protoc";
          OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib";
          OPENSSL_INCLUDE_DIR = "${pkgs.openssl.dev}/include";
          SYS_OPUS = "1";
          CMAKE_POLICY_VERSION_MINIMUM = "3.5";
          MACOSX_DEPLOYMENT_TARGET = "14.0";
        };

        # ort 2.0.0-rc.12 sandboxed-build escape hatch.
        # ORT_STRATEGY=system tells ort-sys/build.rs to skip its
        # download-binaries path and link against the system onnxruntime at
        # ORT_LIB_LOCATION; ORT_DYLIB_PATH points the load-dynamic loader at
        # the same file at runtime. This replaces the previous sed-based
        # patch that mutated ort-sys's Cargo.toml inside the build sandbox.
        # Docs: https://ort.pyke.io/setup/linking#bring-your-own
        ortLibName = if isDarwin then "libonnxruntime.dylib" else "libonnxruntime.so";
        ortEnv = {
          ORT_STRATEGY = "system";
          ORT_LIB_LOCATION = "${pkgs.onnxruntime}/lib";
          ORT_DYLIB_PATH = "${pkgs.onnxruntime}/lib/${ortLibName}";
          ORT_PREFER_DYNAMIC_LINK = "1";
        };

        # Linux-specific link flags. RUSTFLAGS adds the abseil deps the
        # nixpkgs-shipped onnxruntime needs but doesn't expose via pkg-config.
        linuxEnv = lib.optionalAttrs isLinux {
          RUSTFLAGS = "-L native=${pkgs.onnxruntime}/lib -L native=${pkgs.protobuf}/lib -L native=${pkgs.abseil-cpp}/lib -l onnxruntime -l protobuf -l absl_base -l absl_log_internal_check_op -l absl_log_internal_conditions -l absl_log_internal_message -l absl_log_internal_nullguard -l absl_examine_stack -l absl_log_internal_format -l absl_log_internal_structured_proto -l absl_log_internal_log_sink_set -l absl_log_sink -l absl_log_entry -l absl_log_internal_proto -l absl_flags_internal -l absl_flags_marshalling -l absl_flags_reflection -l absl_flags_config -l absl_flags_program_name -l absl_flags_private_handle_accessor -l absl_statusor -l absl_log_initialize -l absl_die_if_null";
        };

        # darwin-specific link flags. nixpkgs auto-patches ELF RPATH on
        # Linux via fixupPhase but the macOS equivalent only rewrites
        # existing absolute install names, it does NOT add LC_RPATH
        # entries. With ORT_PREFER_DYNAMIC_LINK=1, ort embeds
        # `@rpath/libonnxruntime.<ver>.dylib` as the dylib load command,
        # which dyld then fails to resolve at runtime. Pass an explicit
        # `-rpath` linker arg so the binary carries an LC_RPATH pointing
        # at the nixpkgs onnxruntime store path.
        darwinEnv = lib.optionalAttrs isDarwin {
          RUSTFLAGS = "-C link-arg=-Wl,-rpath,${pkgs.onnxruntime}/lib";
        };

        # Naersk build for kesha-engine
        kesha-engine = naersk'.buildPackage ({
          src = ./rust;
          root = ./rust;
          inherit (buildEnv) LIBCLANG_PATH PROTOC OPENSSL_LIB_DIR OPENSSL_INCLUDE_DIR SYS_OPUS CMAKE_POLICY_VERSION_MINIMUM MACOSX_DEPLOYMENT_TARGET;
          inherit nativeBuildInputs buildInputs;
          cargoBuildOptions = old: old ++ [ "--features" rustFeatures "--no-default-features" ];
          cargoTestOptions = old: old ++ [ "--features" rustFeatures "--no-default-features" ];
          # Write the version marker `src/engine-version-marker.ts` reads. Without
          # it the TS CLI treats the Nix-built engine as version-unknown,
          # falls into the re-download branch of `downloadEngine`, and
          # EROFS-fails against the read-only `/nix/store` path. Pinned to
          # package.json#keshaEngine.version so it matches the version the
          # CLI checks for.
          #
          # On darwin-arm64 we also need to stage the `say-avspeech` Swift
          # sidecar next to the engine. `rust/build.rs` writes it to
          # `target/.../build/kesha-engine-<hash>/out/say-avspeech`; runtime
          # lookup (rust/src/tts/avspeech.rs::helper_path) tries sibling-of-exe
          # first. Without this step, `macos-*` voices fail under Nix because
          # the build-time `$OUT_DIR` no longer exists at install time.
          postInstall = ''
            echo "${cliPkg.keshaEngine.version}" > $out/bin/kesha-engine.version
          '' + lib.optionalString (isDarwin && isAarch64) ''
            sidecar=$(find . -path '*/build/*/out/say-avspeech' -type f 2>/dev/null | head -1)
            if [ -z "$sidecar" ]; then
              echo "error: say-avspeech sidecar not found — system_tts may not have compiled" >&2
              exit 1
            fi
            install -Dm755 "$sidecar" "$out/bin/say-avspeech"
          '';
        } // ortEnv // linuxEnv // darwinEnv);

        # Read CLI version from package.json so the package version stays in
        # lockstep with npm publishes (CLI version, not keshaEngine.version —
        # the engine is shipped via `kesha-engine` above which has its own
        # rust/Cargo.toml version).
        cliPkg = lib.importJSON ./package.json;

        # Bun's production dependency closure for the CLI. This is a
        # fixed-output derivation: Nix's sandbox blocks network access, but
        # FODs are allowed to fetch as long as `outputHash` matches the
        # resulting tree.
        #
        # ⚠ KNOWN BROKEN AT MERGE TIME: `outputHash = lib.fakeHash` below is the
        # canonical Nix placeholder ("tell me the real hash") — it WILL fail
        # `nix build .#kesha` with a hash-mismatch error every time until a
        # developer with `nix` installed populates the real value. This is
        # intentional: the dev workflow is
        #
        #   nix build .#kesha 2>&1 | grep -A1 'hash mismatch'
        #
        # then paste the `got:` value into `outputHash` below. PR #242 spec
        # explicitly deferred `nix build` verification to a nix-equipped
        # reviewer; CI without nix can't populate the hash either. `bun2nix`
        # would eliminate this manual step; left as a follow-up (tracked in
        # the PR body) since nixpkgs-unstable doesn't ship it yet.
        #
        # Until populated, `packages.default`, `apps.default`, and the
        # README's `nix run` / `nix profile install` snippets all fail with
        # the hash-mismatch error. Greptile flags this as P1 on every review;
        # the answer is "yes, fix at merge".
        keshaNodeModules = pkgs.stdenv.mkDerivation {
          pname = "kesha-node-modules";
          version = cliPkg.version;
          src = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [
              ./package.json
              ./bun.lock
              ./scripts/postinstall.cjs
            ];
          };
          nativeBuildInputs = with pkgs; [ bun cacert nodejs ];
          dontConfigure = true;
          buildPhase = ''
            runHook preBuild
            export HOME=$TMPDIR
            # --frozen-lockfile pins to bun.lock; --production drops devDeps;
            # --ignore-scripts skips the package.json postinstall (which is a
            # PATH-probe warning for end users, irrelevant inside the
            # sandbox).
            bun install --frozen-lockfile --production --ignore-scripts --no-progress
            runHook postBuild
          '';
          installPhase = ''
            runHook preInstall
            mv node_modules $out
            runHook postInstall
          '';
          outputHash = lib.fakeHash;
          outputHashMode = "recursive";
        };

        # Bun-based `kesha` CLI bundle. Bun executes TypeScript directly so
        # there is no transpile step — we just stage the source tree and
        # makeWrapper a shim that locks `KESHA_ENGINE_BIN` to the flake-built
        # Rust engine.
        #
        # `kesha install` reads the engine version marker written by the
        # `kesha-engine` derivation's postInstall and short-circuits the
        # binary download, going straight to model fetches under
        # `~/.cache/kesha/models/`.
        kesha = pkgs.stdenv.mkDerivation {
          pname = "kesha";
          version = cliPkg.version;
          src = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [
              ./bin
              ./src
              ./package.json
              ./tsconfig.json
              ./openclaw.plugin.json
              ./openclaw-plugin.cjs
              ./SKILL.md
              ./LICENSE
              ./NOTICES.md
              ./scripts/postinstall.cjs
            ];
          };
          nativeBuildInputs = [ pkgs.makeWrapper ];
          dontBuild = true;
          installPhase = ''
            runHook preInstall

            mkdir -p $out/lib/kesha $out/bin
            cp -r bin src scripts package.json tsconfig.json \
                  openclaw-plugin.cjs openclaw.plugin.json SKILL.md LICENSE NOTICES.md \
                  $out/lib/kesha/
            ln -s ${keshaNodeModules} $out/lib/kesha/node_modules

            # bin/kesha.js has a `#!/usr/bin/env bun` shebang and imports
            # ../src/cli.ts directly — Bun resolves the TS at runtime.
            # makeWrapper sets PATH so the shebang's `env bun` resolves to
            # the Nix-built Bun, and pins KESHA_ENGINE_BIN to the flake's
            # engine output so the CLI never falls back to the
            # ~/.cache/kesha download path.
            chmod +x $out/lib/kesha/bin/kesha.js
            makeWrapper $out/lib/kesha/bin/kesha.js $out/bin/kesha \
              --prefix PATH : ${lib.makeBinPath [ pkgs.bun ]} \
              --set KESHA_ENGINE_BIN ${kesha-engine}/bin/kesha-engine

            runHook postInstall
          '';

          meta = with lib; {
            description = "Fast multilingual voice toolkit (Bun CLI + Rust engine)";
            homepage = "https://github.com/drakulavich/kesha-voice-kit";
            license = licenses.mit;
            mainProgram = "kesha";
            platforms = [ "x86_64-linux" "aarch64-darwin" ];
          };
        };

      in
      {
        packages = {
          inherit kesha kesha-engine;
          default = kesha;
        };

        apps =
          let keshaApp = { type = "app"; program = "${kesha}/bin/kesha"; };
          in {
            kesha = keshaApp;
            default = keshaApp;
          };

        devShells.default = pkgs.mkShell ({
          inherit nativeBuildInputs;
          buildInputs = [ rustToolchain ] ++ buildInputs ++ (with pkgs; [
            cargo-make
            bun
            gnumake
          ]);
          # Export LIBCLANG_PATH everywhere so bindgen can dlopen libclang in the dev shell.
          LIBCLANG_PATH = buildEnv.LIBCLANG_PATH;
          shellHook = ''
            echo "✓ Kesha Voice Kit development environment"
            echo "  - Rust: $(rustc --version 2>/dev/null || echo 'not found')"
            echo "  - Bun: $(bun --version 2>/dev/null || echo 'not found')"
            echo "  - Protoc: $(protoc --version 2>/dev/null || echo 'not found')"
            echo "  - Features: ${rustFeatures}"
            ${lib.optionalString isLinux ''
              export RUSTFLAGS="${linuxEnv.RUSTFLAGS}"
            ''}
            ${lib.optionalString isDarwin ''
              export MACOSX_DEPLOYMENT_TARGET="14.0"
              export RUSTFLAGS="${darwinEnv.RUSTFLAGS}"
            ''}
          '';
        } // ortEnv);
      }
    );
}
