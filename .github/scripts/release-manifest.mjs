#!/usr/bin/env node
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import { linuxPackageNames } from "./linux-package-names.mjs";

const REPOSITORY = "drakulavich/kesha-voice-kit";
const MANIFEST_NAME = "kesha-release-manifest.json";

const ENGINE_ASSETS = [
  {
    id: "darwin-arm64",
    os: "darwin",
    arch: "arm64",
    status: "supported",
    engineAsset: "kesha-engine-darwin-arm64",
    install: { directory: "engine/bin", filename: "kesha-engine" },
  },
  {
    id: "linux-x64",
    os: "linux",
    arch: "x64",
    status: "supported",
    engineAsset: "kesha-engine-linux-x64",
    install: { directory: "engine/bin", filename: "kesha-engine" },
  },
  {
    id: "windows-x64",
    os: "win32",
    arch: "x64",
    status: "release-artifact-only",
    engineAsset: "kesha-engine-windows-x64.exe",
    note: "Release workflow builds this artifact, but the Bun installer currently blocks Windows x64.",
  },
];

const DARWIN_SIDECARS = [
  {
    name: "say-avspeech-darwin-arm64",
    install: { directory: "engine/bin", filename: "say-avspeech" },
  },
  {
    name: "kesha-diarize-darwin-arm64",
    install: { directory: "engine/bin", filename: "kesha-diarize-darwin-arm64" },
  },
  {
    name: "kesha-kokoro-darwin-arm64",
    install: { directory: "engine/bin", filename: "kesha-kokoro" },
  },
  {
    name: "kesha-textlang-darwin-arm64",
    install: { directory: "engine/bin", filename: "kesha-textlang" },
  },
];

function linuxPackageAssets(version) {
  const packages = linuxPackageNames(version);
  return [
    {
      name: packages.deb,
      kind: "package",
      platforms: ["linux-x64"],
      install: {
        packageManager: "apt",
        command: `sudo apt install ./${packages.deb}`,
      },
    },
    {
      name: packages.rpm,
      kind: "package",
      platforms: ["linux-x64"],
      install: {
        packageManager: "dnf",
        command: `sudo dnf install ./${packages.rpm}`,
      },
    },
  ];
}

function usage() {
  console.error(
    "usage: node .github/scripts/release-manifest.mjs [--tag vX.Y.Z] [--out path] [--check]",
  );
  process.exit(2);
}

function getArg(name) {
  const i = process.argv.indexOf(name);
  if (i === -1) return undefined;
  const value = process.argv[i + 1];
  if (!value || value.startsWith("--")) usage();
  return value;
}

function hasArg(name) {
  return process.argv.includes(name);
}

function readPackage() {
  return JSON.parse(readFileSync("package.json", "utf8"));
}

function asset(name, kind, platforms, install, checksummed = true) {
  return {
    name,
    kind,
    platforms,
    ...(install ? { install } : {}),
    checksummed,
    signatureBundle: `${name}.sigstore.json`,
  };
}

function buildManifest(tag) {
  const pkg = readPackage();
  const engineVersion = pkg.keshaEngine?.version ?? pkg.version;
  if (typeof pkg.version !== "string" || typeof engineVersion !== "string") {
    throw new Error("package.json must contain version and keshaEngine.version strings");
  }

  const sbomName = `kesha-voice-kit-${tag}.spdx.json`;
  const tagVersion = tag.slice(1);
  if (tagVersion !== pkg.version) {
    throw new Error(
      `release tag ${tag} must match package.json#version (${pkg.version}) for Linux package filenames`,
    );
  }

  const packageVersion = pkg.version;
  const assets = [
    ...ENGINE_ASSETS.map((p) => asset(p.engineAsset, "engine", [p.id], p.install)),
    ...DARWIN_SIDECARS.map((s) => asset(s.name, "sidecar", ["darwin-arm64"], s.install)),
    ...linuxPackageAssets(packageVersion).map((p) =>
      asset(p.name, p.kind, p.platforms, p.install),
    ),
    asset(sbomName, "sbom", []),
    asset(MANIFEST_NAME, "manifest", []),
    asset("SHA256SUMS", "checksum", [], undefined, false),
  ];

  return {
    schemaVersion: 1,
    repository: REPOSITORY,
    tag,
    cliVersion: pkg.version,
    engineVersion,
    packaging: {
      runtime: "bun",
      userInstall: "bun add -g @drakulavich/kesha-voice-kit",
      manifestPurpose:
        "Release metadata for package-manager channels; it does not replace the Bun-first user install path.",
    },
    verification: {
      checksumAsset: "SHA256SUMS",
      sigstoreBundleSuffix: ".sigstore.json",
    },
    platforms: ENGINE_ASSETS.map((p) => ({
      id: p.id,
      os: p.os,
      arch: p.arch,
      status: p.status,
      engineAsset: p.engineAsset,
      ...(p.note ? { note: p.note } : {}),
    })),
    assets,
  };
}

function assertIncludes(source, needle, file) {
  if (!source.includes(needle)) {
    throw new Error(`${file} is missing release manifest token: ${needle}`);
  }
}

function validateSourceConsistency(manifest) {
  const installer = readFileSync("src/engine-install.ts", "utf8");
  const workflow = readFileSync(".github/workflows/build-engine.yml", "utf8");

  for (const p of ENGINE_ASSETS) {
    if (p.status === "supported") {
      assertIncludes(installer, p.engineAsset, "src/engine-install.ts");
    }
    assertIncludes(workflow, p.engineAsset, ".github/workflows/build-engine.yml");
  }

  for (const s of DARWIN_SIDECARS) {
    assertIncludes(installer, `assetName: "${s.name}"`, "src/engine-install.ts");
    assertIncludes(installer, `fileBasename: "${s.install.filename}"`, "src/engine-install.ts");
    assertIncludes(workflow, s.name, ".github/workflows/build-engine.yml");
  }

  assertIncludes(workflow, "SHA256SUMS", ".github/workflows/build-engine.yml");
  assertIncludes(workflow, ".sigstore.json", ".github/workflows/build-engine.yml");
  assertIncludes(workflow, "build-linux-packages.mjs", ".github/workflows/build-engine.yml");
  assertIncludes(workflow, "dist/linux-packages/*.{deb,rpm}", ".github/workflows/build-engine.yml");

  const packageScript = readFileSync(".github/scripts/build-linux-packages.mjs", "utf8");
  const packageNames = readFileSync(".github/scripts/linux-package-names.mjs", "utf8");
  const nfpmConfig = readFileSync("packaging/nfpm.yaml", "utf8");
  assertIncludes(packageScript, "--target=bun-linux-x64", ".github/scripts/build-linux-packages.mjs");
  assertIncludes(packageScript, "packaging/nfpm.yaml", ".github/scripts/build-linux-packages.mjs");
  assertIncludes(packageScript, "LINUX_PACKAGE_RELEASE", ".github/scripts/build-linux-packages.mjs");
  assertIncludes(packageNames, "LINUX_PACKAGE_RELEASE", ".github/scripts/linux-package-names.mjs");
  assertIncludes(nfpmConfig, "dst: /usr/bin/kesha", "packaging/nfpm.yaml");
  assertIncludes(nfpmConfig, "dst: /usr/bin/parakeet", "packaging/nfpm.yaml");

  const names = new Set();
  for (const a of manifest.assets) {
    if (names.has(a.name)) throw new Error(`duplicate release manifest asset: ${a.name}`);
    names.add(a.name);
    if (!a.signatureBundle.endsWith(manifest.verification.sigstoreBundleSuffix)) {
      throw new Error(`bad sigstore bundle name for ${a.name}: ${a.signatureBundle}`);
    }
  }
}

const pkg = readPackage();
const defaultTag = `v${pkg.keshaEngine?.version ?? pkg.version}`;
const tag = getArg("--tag") ?? defaultTag;
if (!/^v[0-9]+\.[0-9]+\.[0-9]+$/.test(tag)) {
  throw new Error(`release manifest tag must look like vX.Y.Z, got: ${tag}`);
}

const manifest = buildManifest(tag);
validateSourceConsistency(manifest);

const out = getArg("--out");
if (out) {
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, `${JSON.stringify(manifest, null, 2)}\n`);
} else if (!hasArg("--check")) {
  process.stdout.write(`${JSON.stringify(manifest, null, 2)}\n`);
}
