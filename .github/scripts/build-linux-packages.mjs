#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { LINUX_PACKAGE_RELEASE, validateLinuxPackageVersion } from "./linux-package-names.mjs";

const OUT_DIR = "dist/linux-packages";
const pkg = JSON.parse(readFileSync("package.json", "utf8"));
const version = pkg.version;

validateLinuxPackageVersion(version);

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    stdio: "inherit",
    env: { ...process.env, ...options.env },
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} exited ${result.status}`);
  }
}

rmSync(OUT_DIR, { recursive: true, force: true });
mkdirSync(OUT_DIR, { recursive: true });

const binary = join(OUT_DIR, "kesha");
run("bun", [
  "build",
  "--compile",
  "--target=bun-linux-x64",
  `--outfile=${binary}`,
  "./bin/kesha.js",
]);

const nfpmEnv = {
  KESHA_VERSION: version,
  KESHA_PACKAGE_RELEASE: LINUX_PACKAGE_RELEASE,
};
for (const packager of ["deb", "rpm"]) {
  run("nfpm", [
    "package",
    "--config",
    "packaging/nfpm.yaml",
    "--packager",
    packager,
    "--target",
    OUT_DIR,
  ], { env: nfpmEnv });
}
