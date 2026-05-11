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
        rustFeatures = if isDarwin && isAarch64
          then "coreml,tts,system_tts"
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
        ] ++ lib.optionals isDarwin [ pkgs.swift ];

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
          darwin.apple_sdk.frameworks.AVFoundation
          darwin.apple_sdk.frameworks.CoreML
          darwin.apple_sdk.frameworks.Foundation
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
          postInstall = ''
            echo "${cliPkg.keshaEngine.version}" > $out/bin/kesha-engine.version
          '';
        } // ortEnv // linuxEnv);

        # Read CLI version from package.json so the package version stays in
        # lockstep with npm publishes (CLI version, not keshaEngine.version —
        # the engine is shipped via `kesha-engine` above which has its own
        # rust/Cargo.toml version).
        cliPkg = lib.importJSON ./package.json;

        # Bun's production dependency closure for the CLI. This is a
        # fixed-output derivation: Nix's sandbox blocks network access, but
        # FODs are allowed to fetch as long as `outputHash` matches the
        # resulting tree. If `bun.lock` changes, the hash must be regenerated:
        #
        #   nix build .#kesha 2>&1 | grep -A1 'hash mismatch'
        #
        # then paste the `got:` value into `outputHash` below. `bun2nix` would
        # eliminate this manual step; left as a follow-up (issue mentioned in
        # the PR body) since nixpkgs-unstable doesn't ship it yet.
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
        # Rust engine. `parakeet` is exposed as a backward-compatible alias.
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
            for shim in kesha parakeet; do
              makeWrapper $out/lib/kesha/bin/kesha.js $out/bin/$shim \
                --prefix PATH : ${lib.makeBinPath [ pkgs.bun ]} \
                --set KESHA_ENGINE_BIN ${kesha-engine}/bin/kesha-engine
            done

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
            ''}
          '';
        } // ortEnv);
      }
    );
}
