#!/usr/bin/env bun
/**
 * Parse every `.github/workflows/*.yml` file and report syntax errors.
 *
 * Replaces the ad-hoc `python3 -c "import yaml; yaml.safe_load(...)"` invocation
 * we were running before each workflow change. Same effect — surface syntax
 * errors locally before `git push` instead of finding them in CI — but uses
 * the bun toolchain so contributors don't need a python interpreter on PATH.
 *
 * Run via `bun run check:workflows`. Exits non-zero on any parse failure;
 * stays silent on success so it composes cleanly with other pre-push checks.
 */
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { parse, YAMLParseError } from "yaml";

const dir = ".github/workflows";
const files = readdirSync(dir)
  .filter((f) => f.endsWith(".yml") || f.endsWith(".yaml"))
  .sort();

if (files.length === 0) {
  console.error(`no workflow files found in ${dir}`);
  process.exit(1);
}

let failed = 0;
for (const f of files) {
  const path = join(dir, f);
  try {
    parse(readFileSync(path, "utf8"));
  } catch (err) {
    failed += 1;
    if (err instanceof YAMLParseError) {
      // YAMLParseError gives line/col + a code; render it the way most
      // tools do so editors can jump to the offending position.
      console.error(`${path}:${err.linePos?.[0]?.line ?? "?"}:${err.linePos?.[0]?.col ?? "?"}: ${err.message}`);
    } else {
      const msg = err instanceof Error ? err.message : String(err);
      console.error(`${path}: ${msg}`);
    }
  }
}

if (failed > 0) {
  console.error(`\n${failed} workflow file(s) failed to parse.`);
  process.exit(1);
}
