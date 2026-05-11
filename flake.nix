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
        # Darwin additions: swift drives `rust/build.rs` for the `system_tts`
        # feature; the Apple SDK frameworks satisfy the `coreml` link step.
        nativeBuildInputs = with pkgs; [
          protobuf
          llvmPackages.libclang
          pkg-config
          cmake
          makeWrapper
        ] ++ lib.optionals isDarwin (with pkgs; [
          swift
          darwin.apple_sdk.frameworks.AVFoundation
          darwin.apple_sdk.frameworks.CoreML
          darwin.apple_sdk.frameworks.Foundation
        ]);

        # Runtime dependencies (libraries to link against)
        # protobuf is in nativeBuildInputs already; don't duplicate it here.
        buildInputs = with pkgs; [
          openssl
          opus
        ] ++ lib.optionals isLinux (with pkgs; [
          clang
          llvmPackages.llvm
          onnxruntime
          abseil-cpp
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
        } // ortEnv // linuxEnv);

      in
      {
        packages = {
          kesha-engine = kesha-engine;
          default = kesha-engine;
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
              export RUSTFLAGS="-L /opt/homebrew/lib"
            ''}
          '';
        } // ortEnv);
      }
    );
}
