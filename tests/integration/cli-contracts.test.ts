import { afterEach, describe, expect, test } from "bun:test";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";
import { runCliScenario, type CliScenarioOptions, type CliScenarioResult } from "./cli-scenario";

const tempDirs: string[] = [];

async function runCli(
  args: string[],
  opts: CliScenarioOptions = {},
): Promise<CliScenarioResult> {
  return runCliScenario(args, opts);
}

function makeTempDir(prefix: string): string {
  const dir = mkdtempSync(join(tmpdir(), prefix));
  tempDirs.push(dir);
  return dir;
}

function isolatedEnv(dir = makeTempDir("kesha-cli-contract-")): Record<string, string> {
  return {
    HOME: dir,
    KESHA_CACHE_DIR: join(dir, "cache"),
    KESHA_STATS_DB: join(dir, "stats.sqlite"),
  };
}

function installFakeDiarizeModel(cacheDir: string): void {
  const model = join(cacheDir, "models", "diarize", "SortformerNvidiaLow_v2.mlpackage");
  const weights = join(model, "Data", "com.apple.CoreML", "weights");
  mkdirSync(weights, { recursive: true });
  writeFileSync(join(model, "Manifest.json"), "{}");
  writeFileSync(join(model, "Data", "com.apple.CoreML", "model.mlmodel"), "model");
  writeFileSync(join(weights, "0-weight.bin"), "0");
  writeFileSync(join(weights, "1-weight.bin"), "1");
}

function createFakeEngine(dir: string): string {
  const enginePath = join(dir, "kesha-engine");
  writeFileSync(
    enginePath,
    `#!/usr/bin/env bun
const args = Bun.argv.slice(2);

if (args[0] === "--capabilities-json") {
  console.log(JSON.stringify({
    protocolVersion: 1,
    backend: "fake",
    features: ["transcribe.segments", "transcribe.diarize"],
  }));
  process.exit(0);
}

if (args[0] === "detect-lang") {
  console.log(JSON.stringify({ code: "ru", confidence: 0.99 }));
  process.exit(0);
}

if (args[0] === "detect-text-lang") {
  console.log(JSON.stringify({ code: "ru", confidence: 0.98 }));
  process.exit(0);
}

if (args[0] === "transcribe") {
  const text = args.includes("--no-vad") ? "Привет без VAD" : "Привет с воркшопа";
  if (args.includes("--speakers")) {
    console.log(JSON.stringify({
      text,
      segments: [{ start: 0, end: 1.2, text, speaker: 0 }],
    }));
  } else {
    console.log(text);
  }
  process.exit(0);
}

console.error("unexpected fake engine args: " + JSON.stringify(args));
process.exit(2);
`,
  );
  chmodSync(enginePath, 0o755);
  return enginePath;
}

function createFailingEngine(dir: string): string {
  const enginePath = join(dir, "kesha-engine-fail-on-use");
  writeFileSync(
    enginePath,
    `#!/usr/bin/env bun
console.error("fake engine should not have been invoked: " + JSON.stringify(Bun.argv.slice(2)));
process.exit(99);
`,
  );
  chmodSync(enginePath, 0o755);
  return enginePath;
}

function expectContract(
  actual: CliScenarioResult,
  expected: {
    exitCode: number;
    stdoutContains?: string[];
    stdoutNotContains?: string[];
    stderrContains?: string[];
    stderrNotContains?: string[];
    stdoutEmpty?: boolean;
    stderrEmpty?: boolean;
  },
): void {
  expect(actual.exitCode).toBe(expected.exitCode);
  if (expected.stdoutEmpty) expect(actual.stdout).toBe("");
  if (expected.stderrEmpty) expect(actual.stderr).toBe("");
  for (const needle of expected.stdoutContains ?? []) {
    expect(actual.stdout).toContain(needle);
  }
  for (const needle of expected.stdoutNotContains ?? []) {
    expect(actual.stdout).not.toContain(needle);
  }
  for (const needle of expected.stderrContains ?? []) {
    expect(actual.stderr).toContain(needle);
  }
  for (const needle of expected.stderrNotContains ?? []) {
    expect(actual.stderr).not.toContain(needle);
  }
}

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

describe("CLI contracts", () => {
  test("entrypoint help, version, and empty invocation keep stable stream contracts", async () => {
    const help = await runCli(["--help"]);
    expectContract(help, {
      exitCode: 0,
      stdoutContains: ["Kesha Voice Kit", "kesha install", "--json", "--format"],
      stderrEmpty: true,
    });

    const version = await runCli(["--version"]);
    expectContract(version, { exitCode: 0, stderrEmpty: true });
    expect(version.stdout).toMatch(/^\d+\.\d+\.\d+$/);

    const empty = await runCli([]);
    expectContract(empty, {
      exitCode: 1,
      stdoutContains: ["Usage: kesha <audio_file>", "kesha stats", "kesha support-bundle"],
      stderrEmpty: true,
    });
  });

  test("validation errors are stderr-only and exit with the documented codes", async () => {
    const cases: Array<{
      name: string;
      args: string[];
      exitCode: number;
      stderr: string[];
    }> = [
      {
        name: "json and toon mutex",
        args: ["--json", "--toon", "a.wav"],
        exitCode: 2,
        stderr: ["--json and --toon are mutually exclusive"],
      },
      {
        name: "transcript and json mutex",
        args: ["--format", "transcript", "--json", "a.wav"],
        exitCode: 2,
        stderr: ["--format transcript is mutually exclusive"],
      },
      {
        name: "timestamps require machine output",
        args: ["--timestamps", "a.wav"],
        exitCode: 2,
        stderr: ["--timestamps requires --json"],
      },
      {
        name: "speakers require machine output",
        args: ["--speakers", "a.wav"],
        exitCode: 2,
        stderr: ["--speakers requires --json"],
      },
      {
        name: "vad flags are mutually exclusive",
        args: ["--vad", "--no-vad", "a.wav"],
        exitCode: 2,
        stderr: ["--vad and --no-vad are mutually exclusive"],
      },
      {
        name: "say rejects unknown output format before synthesis",
        args: ["say", "hello", "--format", "mp3"],
        exitCode: 2,
        stderr: ["unknown --format 'mp3'"],
      },
    ];

    for (const entry of cases) {
      const run = await runCli(entry.args, { env: isolatedEnv() });
      expectContract(run, {
        exitCode: entry.exitCode,
        stdoutEmpty: true,
        stderrContains: entry.stderr,
      });
    }
  });

  test("unknown commands and missing files do not start a configured engine", async () => {
    const dir = makeTempDir("kesha-cli-contract-engine-");
    const enginePath = createFailingEngine(dir);
    const env: Record<string, string> = {
      ...isolatedEnv(dir),
      KESHA_ENGINE_BIN: enginePath,
    };

    const typo = await runCli(["instal"], { env });
    expectContract(typo, {
      exitCode: 1,
      stdoutEmpty: true,
      stderrContains: ["unknown command 'instal'", "Did you mean install?"],
      stderrNotContains: ["fake engine should not have been invoked"],
    });

    const missing = await runCli(["missing.wav"], { env });
    expectContract(missing, {
      exitCode: 1,
      stdoutEmpty: true,
      stderrContains: ["missing.wav: File not found"],
      stderrNotContains: ["fake engine should not have been invoked"],
    });
  });

  test("machine-readable missing-file failures keep JSON on stdout and diagnostics on stderr", async () => {
    const run = await runCli(["--json", "a.wav", "b.wav"], {
      env: isolatedEnv(),
    });

    expectContract(run, {
      exitCode: 1,
      stderrContains: ["a.wav: File not found", "b.wav: File not found"],
    });
    expect(JSON.parse(run.stdout)).toEqual([]);
  });

  test("machine-readable partial failures keep parseable JSON on stdout and diagnostics on stderr", async () => {
    const dir = makeTempDir("kesha-cli-contract-partial-");
    const enginePath = createFakeEngine(dir);
    const mediaPath = join(dir, "workshop.mp4");
    writeFileSync(mediaPath, "fake media");
    const env: Record<string, string> = {
      ...isolatedEnv(dir),
      KESHA_ENGINE_BIN: enginePath,
    };

    const run = await runCli(["--json", "--include-errors", mediaPath, "missing.wav"], { env });
    expectContract(run, {
      exitCode: 1,
      stderrContains: [
        `Transcribing ${mediaPath}...`,
        `Transcribed ${mediaPath}`,
        "missing.wav: File not found",
      ],
      stdoutNotContains: ["Transcribing", "Transcribed", "missing.wav: File not found"],
    });

    const parsed = JSON.parse(run.stdout);
    expect(parsed.results).toHaveLength(1);
    expect(parsed.results[0].file).toBe(mediaPath);
    expect(parsed.errors).toEqual([
      { file: "missing.wav", code: "file_not_found", message: "File not found" },
    ]);
  });

  test("successful machine-readable output keeps progress off stdout", async () => {
    const dir = makeTempDir("kesha-cli-contract-success-");
    const enginePath = createFakeEngine(dir);
    const mediaPath = join(dir, "workshop.mp4");
    writeFileSync(mediaPath, "fake media");
    const env: Record<string, string> = {
      ...isolatedEnv(dir),
      KESHA_ENGINE_BIN: enginePath,
    };
    installFakeDiarizeModel(env.KESHA_CACHE_DIR);

    const json = await runCli([mediaPath, "--json", "--speakers"], { env });
    expectContract(json, {
      exitCode: 0,
      stderrContains: [`Transcribing ${mediaPath}...`, `Transcribed ${mediaPath}`],
      stdoutNotContains: ["Transcribing", "Transcribed"],
    });
    const parsed = JSON.parse(json.stdout);
    expect(parsed).toHaveLength(1);
    expect(parsed[0]).toMatchObject({
      file: mediaPath,
      text: "Привет с воркшопа",
      lang: "ru",
      audioLanguage: { code: "ru", confidence: 0.99 },
      textLanguage: { code: "ru", confidence: 0.98 },
    });
    expect(parsed[0].segments[0]).toEqual({
      start: 0,
      end: 1.2,
      text: "Привет с воркшопа",
      speaker: 0,
    });

    const missingDiarizeEnv: Record<string, string> = {
      ...isolatedEnv(makeTempDir("kesha-cli-contract-missing-diarize-")),
      KESHA_ENGINE_BIN: enginePath,
    };
    const missingDiarize = await runCli([mediaPath, "--json", "--speakers"], { env: missingDiarizeEnv });
    expectContract(missingDiarize, {
      exitCode: 1,
      stderrContains: ["diarization model not found"],
      stderrNotContains: ["Transcribing", "Transcribed"],
      stdoutNotContains: ["Привет с воркшопа"],
    });

    const transcript = await runCli([mediaPath, "--format", "transcript"], { env });
    expectContract(transcript, {
      exitCode: 0,
      stdoutContains: ["Привет с воркшопа", "[lang: ru, confidence: 0.98]"],
      stdoutNotContains: ["Transcribing", "Transcribed"],
      stderrContains: [`Transcribing ${mediaPath}...`, `Transcribed ${mediaPath}`],
    });

    const toon = await runCli([mediaPath, "--toon"], { env });
    expectContract(toon, {
      exitCode: 0,
      stdoutNotContains: ["Transcribing", "Transcribed"],
      stderrContains: [`Transcribing ${mediaPath}...`, `Transcribed ${mediaPath}`],
    });
    const { decode: decodeToon } = await import("@toon-format/toon");
    const decoded = decodeToon(toon.stdout) as Array<Record<string, unknown>>;
    expect(decoded[0].text).toBe("Привет с воркшопа");
    expect(decoded[0].lang).toBe("ru");

    const noVadJson = await runCli([mediaPath, "--json", "--no-vad"], { env });
    expectContract(noVadJson, {
      exitCode: 0,
      stdoutNotContains: ["Transcribing", "Transcribed"],
      stderrContains: [`Transcribing ${mediaPath}...`, `Transcribed ${mediaPath}`],
    });
    expect(JSON.parse(noVadJson.stdout)[0].text).toBe("Привет без VAD");
  });

  test("diagnostic and support commands return parseable/readable contracts without leaking temp home", async () => {
    const dir = makeTempDir("kesha-cli-contract-diagnostics-");
    const enginePath = createFakeEngine(dir);
    const env: Record<string, string> = {
      ...isolatedEnv(dir),
      KESHA_ENGINE_BIN: enginePath,
    };

    const doctor = await runCli(["doctor", "--json", "--redact"], { env });
    expectContract(doctor, {
      exitCode: 0,
      stderrEmpty: true,
      stdoutNotContains: [dir],
    });
    const report = JSON.parse(doctor.stdout);
    expect(report.redacted).toBe(true);
    expect(report.package.name).toBe("@drakulavich/kesha-voice-kit");
    expect(report.engine.path).toBe("~/kesha-engine");
    expect(report.engine.capabilities.backend).toBe("fake");
    expect(report.env.KESHA_ENGINE_BIN).toBe("~/kesha-engine");
    expect(report.env.KESHA_STATS_DB).toBe("~/stats.sqlite");

    const bundlePath = join(dir, "bundle.tar.gz");
    const bundle = await runCli(["support-bundle", "--output", bundlePath], {
      env,
      artifacts: [bundlePath],
    });
    expectContract(bundle, {
      exitCode: 0,
      stdoutContains: [`Created support bundle: ${bundlePath}`, "Entries: 4", "Size:"],
      stderrEmpty: true,
    });
    expect(existsSync(bundlePath)).toBe(true);
    expect(bundle.artifacts[0]).toMatchObject({
      path: bundlePath,
      exists: true,
    });
    expect(bundle.artifacts[0]?.sizeBytes).toBeGreaterThan(0);
  });

  test("read-only planning and stats commands keep user data on stdout", async () => {
    const dir = makeTempDir("kesha-cli-contract-readonly-");
    const enginePath = createFailingEngine(dir);
    const env: Record<string, string> = {
      ...isolatedEnv(dir),
      KESHA_ENGINE_BIN: enginePath,
    };

    const plan = await runCli(["install", "--plan", "--tts"], { env });
    expect(plan.envDiff.overrides.KESHA_ENGINE_BIN).toBe(enginePath);
    expectContract(plan, {
      exitCode: 0,
      stdoutContains: ["Kesha install plan", "Expected network for this run:", "Run: kesha install --tts"],
      stderrNotContains: ["fake engine should not have been invoked"],
    });

    const status = await runCli(["stats", "status"], { env });
    expectContract(status, {
      exitCode: 0,
      stdoutContains: ["Kesha Stats: disabled", `Database: ${env.KESHA_STATS_DB}`, "Runs: 0", "Retention: 90 day(s)"],
      stderrEmpty: true,
    });

    const enabled = await runCli(["stats", "enable"], { env });
    expectContract(enabled, {
      exitCode: 0,
      stdoutContains: ["Kesha Stats enabled", `Database: ${env.KESHA_STATS_DB}`],
      stderrEmpty: true,
    });

    const missing = await runCli(["private-recording.wav"], { env });
    expect(missing.exitCode).toBe(1);

    const week = await runCli(["stats", "week"], { env });
    expectContract(week, {
      exitCode: 0,
      stdoutContains: ["Kesha Stats", "Runs: 1", "Bottlenecks:", "Slowest anonymous runs:"],
      stderrEmpty: true,
    });

    const errors = await runCli(["stats", "errors"], { env });
    expectContract(errors, {
      exitCode: 0,
      stdoutContains: ["file_not_found"],
      stdoutNotContains: ["private-recording.wav"],
      stderrEmpty: true,
    });

    const jsonExport = await runCli(["stats", "export", "--format", "json"], { env });
    expectContract(jsonExport, {
      exitCode: 0,
      stdoutContains: ['"contentFree": true', '"runs"', '"errors"'],
      stdoutNotContains: ["private-recording.wav"],
      stderrEmpty: true,
    });

    const csvExport = await runCli(["stats", "export", "--format", "csv"], { env });
    expectContract(csvExport, {
      exitCode: 0,
      stdoutContains: ["table,id,run_id", "runs,", "errors,"],
      stdoutNotContains: ["private-recording.wav"],
      stderrEmpty: true,
    });

    const retention = await runCli(["stats", "retention", "30"], { env });
    expectContract(retention, {
      exitCode: 0,
      stdoutContains: ["Kesha Stats retention set to 30 day(s)"],
      stderrEmpty: true,
    });

    const vacuum = await runCli(["stats", "vacuum"], { env });
    expectContract(vacuum, {
      exitCode: 0,
      stdoutContains: ["Kesha Stats vacuumed:", `Database: ${env.KESHA_STATS_DB}`],
      stderrEmpty: true,
    });

    const reset = await runCli(["stats", "reset"], { env });
    expectContract(reset, {
      exitCode: 0,
      stdoutContains: ["Kesha Stats reset:", "run(s)"],
      stderrEmpty: true,
    });
  });
});
