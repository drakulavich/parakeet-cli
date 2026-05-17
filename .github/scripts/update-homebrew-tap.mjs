#!/usr/bin/env node
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

const REPOSITORY = "drakulavich/kesha-voice-kit";
const FORMULA_REL = "Formula/kesha-voice-kit.rb";

function usage() {
  console.error(
    "usage: node .github/scripts/update-homebrew-tap.mjs --tag vX.Y.Z --tap-dir path",
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

async function sha256ForUrl(url) {
  const res = await fetch(url, { redirect: "follow" });
  if (!res.ok) {
    throw new Error(`could not fetch ${url}: HTTP ${res.status}`);
  }
  const bytes = Buffer.from(await res.arrayBuffer());
  return createHash("sha256").update(bytes).digest("hex");
}

function replaceOne(source, pattern, replacement, label) {
  if (!pattern.test(source)) throw new Error(`formula is missing ${label}`);
  return source.replace(pattern, replacement);
}

const tag = getArg("--tag");
const tapDir = getArg("--tap-dir");
if (!tag || !tapDir) usage();
if (!/^v[0-9]+\.[0-9]+\.[0-9]+$/.test(tag)) {
  throw new Error(`Homebrew tap updates only support stable vX.Y.Z tags, got: ${tag}`);
}

const tarballUrl = `https://github.com/${REPOSITORY}/archive/refs/tags/${tag}.tar.gz`;
const sha256 = await sha256ForUrl(tarballUrl);
const formulaPath = join(tapDir, FORMULA_REL);
let formula = readFileSync(formulaPath, "utf8");

formula = replaceOne(
  formula,
  /^  url ".*"$/m,
  `  url "${tarballUrl}"`,
  "url",
);
formula = replaceOne(
  formula,
  /^  sha256 "[a-f0-9]{64}"$/m,
  `  sha256 "${sha256}"`,
  "sha256",
);

mkdirSync(dirname(formulaPath), { recursive: true });
writeFileSync(formulaPath, formula);
console.log(`Updated ${FORMULA_REL} for ${tag}`);
console.log(`url: ${tarballUrl}`);
console.log(`sha256: ${sha256}`);
