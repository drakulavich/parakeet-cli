#!/usr/bin/env bun
/**
 * Smoke test: verify the installed package works end-to-end.
 * Checks that the `kesha` command is linked and functional.
 * Detailed transcription/lang-id tests live in tests/integration/.
 *
 * Usage: bun scripts/smoke-test.ts
 * Prerequisites: bun link @drakulavich/kesha-voice-kit && kesha install
 */

import { Glob } from "bun";
import { resolve } from "path";

const fixturesDir = resolve(import.meta.dir, "../tests/fixtures/benchmark");
const files = [...new Glob("*.ogg").scanSync(fixturesDir)].sort();

if (files.length === 0) {
  console.error(`ERROR: No .ogg files found in ${fixturesDir}`);
  process.exit(1);
}

console.log("Running smoke tests...\n");

let passed = 0;
let failed = 0;

function check(name: string, ok: boolean, detail = "") {
  if (ok) {
    console.log(`  PASS  ${name}`);
    passed++;
  } else {
    console.log(`  FAIL  ${name}${detail ? ` (${detail})` : ""}`);
    failed++;
  }
}

// 1. The command is available and returns version
const versionProc = Bun.spawnSync(["kesha", "--version"], { stdout: "pipe", stderr: "pipe" });
const version = versionProc.stdout.toString().trim();
check(`"kesha" command works (${version})`, versionProc.exitCode === 0 && version.length > 0);

// 2. kesha install completes (models already cached = fast)
const installProc = Bun.spawnSync(["kesha", "install"], { stdout: "pipe", stderr: "pipe" });
const installOut = installProc.stdout.toString() + installProc.stderr.toString();
check("kesha install completes", installOut.includes("installed") || installOut.includes("already") || installOut.includes("models"));

// 3. Transcription produces non-empty output
const testFile = resolve(fixturesDir, files[0]);
const transcribeProc = Bun.spawnSync(["kesha", testFile], { stdout: "pipe", stderr: "pipe" });
const transcript = transcribeProc.stdout.toString().trim();
check("transcription produces output", transcribeProc.exitCode === 0 && transcript.length > 10, transcript.slice(0, 60));

// 4. --json output is valid JSON with expected fields
const jsonProc = Bun.spawnSync(["kesha", "--json", testFile], { stdout: "pipe", stderr: "pipe" });
try {
  const parsed = JSON.parse(jsonProc.stdout.toString().trim());
  check("--json output is valid", Array.isArray(parsed) && parsed[0]?.text && parsed[0]?.lang);
} catch {
  check("--json output is valid", false, "not valid JSON");
}

// 5. Command suggestion works
const typoProc = Bun.spawnSync(["kesha", "instal"], { stdout: "pipe", stderr: "pipe" });
check("typo suggestion works", typoProc.exitCode === 1 && typoProc.stderr.toString().includes("Did you mean"));

// 6. TTS smoke (opt-in via --tts flag; requires `kesha install --tts`)
if (process.argv.includes("--tts")) {
  for (const [label, text] of [
    ["en (Kokoro)", "Hello, world"],
    ["ru (Vosk, auto-routed)", "Привет, мир"],
  ] as const) {
    const tmpWav = `/tmp/kesha-smoke-${label.split(" ")[0]}.wav`;
    const sayProc = Bun.spawnSync(["kesha", "say", text, "--out", tmpWav], {
      stdout: "pipe",
      stderr: "pipe",
    });
    const wavSize = Bun.file(tmpWav).size;
    const header =
      wavSize > 0
        ? new TextDecoder().decode(
            new Uint8Array((await Bun.file(tmpWav).arrayBuffer()).slice(0, 4)),
          )
        : "";
    check(
      `kesha say ${label} produces WAV`,
      sayProc.exitCode === 0 && wavSize > 10_000 && header === "RIFF",
      `exit=${sayProc.exitCode} header=${header} size=${wavSize}`,
    );
  }
}

const total = passed + failed;
console.log(`\n${passed}/${total} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
