import { afterEach, describe, test, expect } from "bun:test";
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "fs";
import { join } from "path";
import { tmpdir } from "os";

const CWD = import.meta.dir + "/../..";
const CLI_TIMEOUT_MS = 4_000;
const tempDirs: string[] = [];

async function runCli(
  args: string[],
  opts: { env?: Record<string, string> } = {},
): Promise<{ stdout: string; stderr: string; exitCode: number }> {
  const proc = Bun.spawn(["bun", "run", "src/cli.ts", ...args], {
    stdout: "pipe",
    stderr: "pipe",
    cwd: CWD,
    env: opts.env ? { ...process.env, ...opts.env } : process.env,
  });

  const stdoutPromise = new Response(proc.stdout).text();
  const stderrPromise = new Response(proc.stderr).text();
  let timeout: Timer | undefined;
  const timeoutPromise = new Promise<"timeout">((resolve) => {
    timeout = setTimeout(() => resolve("timeout"), CLI_TIMEOUT_MS);
  });
  const exitOrTimeout = await Promise.race([proc.exited, timeoutPromise]);
  if (timeout) clearTimeout(timeout);

  if (exitOrTimeout === "timeout") {
    proc.kill();
  }

  const [stdout, stderr, exitCode] = await Promise.all([
    stdoutPromise,
    stderrPromise,
    proc.exited,
  ]);

  if (exitOrTimeout === "timeout") {
    throw new Error(
      [
        `CLI timed out after ${CLI_TIMEOUT_MS}ms: kesha ${args.join(" ")}`,
        `exitCode=${exitCode}`,
        `stdout=${stdout.trim()}`,
        `stderr=${stderr.trim()}`,
      ].join("\n"),
    );
  }

  return { stdout: stdout.trim(), stderr: stderr.trim(), exitCode };
}

function emptyKeshaEnv(): Record<string, string> {
  const dir = mkdtempSync(join(tmpdir(), "kesha-empty-cli-"));
  tempDirs.push(dir);
  return { HOME: dir, KESHA_CACHE_DIR: dir };
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
  console.log(JSON.stringify({
    text: "Привет с воркшопа",
    segments: [{ start: 0, end: 1.2, text: "Привет с воркшопа", speaker: 0 }],
  }));
  process.exit(0);
}

console.error("unexpected fake engine args: " + JSON.stringify(args));
process.exit(2);
`,
  );
  chmodSync(enginePath, 0o755);
  return enginePath;
}

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

describe("e2e-cli", () => {
  test("--version prints version and exits 0", async () => {
    const { stdout, exitCode } = await runCli(["--version"]);
    expect(exitCode).toBe(0);
    expect(stdout).toMatch(/^\d+\.\d+\.\d+/);
  });

  test("no args prints usage and exits 1", async () => {
    const { stdout, exitCode } = await runCli([]);
    expect(exitCode).toBe(1);
    expect(stdout).toContain("Usage:");
  });

  test("--help shows description and flags", async () => {
    const { stdout, exitCode } = await runCli(["--help"]);
    expect(exitCode).toBe(0);
    expect(stdout).toContain("--json");
    expect(stdout).toContain("--verbose");
    expect(stdout).toContain("--lang");
    expect(stdout).toContain("--timestamps");
  });

  test("install --help shows --no-cache flag", async () => {
    const { stdout, exitCode } = await runCli(["install", "--help"]);
    expect(exitCode).toBe(0);
    expect(stdout).toContain("--no-cache");
  });

  test("status prints engine info and exits 0", async () => {
    const { stdout, exitCode } = await runCli(["status"]);
    expect(exitCode).toBe(0);
    expect(stdout).toContain("Engine:");
    expect(stdout).toContain("Runtime");
  });

  test("missing file prints error and exits 1", async () => {
    const { stderr, exitCode } = await runCli(["nonexistent.wav"]);
    expect(exitCode).toBe(1);
    expect(stderr.toLowerCase()).toContain("file not found");
  });

  test("missing file is reported before checking whether engine is installed", async () => {
    const { stderr, exitCode } = await runCli(["nonexistent.wav"], {
      env: emptyKeshaEnv(),
    });
    expect(exitCode).toBe(1);
    expect(stderr.toLowerCase()).toContain("file not found");
    expect(stderr).not.toContain("No transcription backend is installed");
  });

  test("multiple missing files with --json outputs empty array", async () => {
    const { stdout, stderr, exitCode } = await runCli(["--json", "a.wav", "b.wav"]);
    expect(exitCode).toBe(1);
    expect(JSON.parse(stdout)).toEqual([]);
    expect(stderr).toContain("a.wav");
  });

  test("unknown subcommand suggests closest match", async () => {
    const { stderr, exitCode } = await runCli(["instal"]);
    expect(exitCode).toBe(1);
    expect(stderr).toContain("unknown command");
    expect(stderr).toContain("Did you mean");
  });

  test("gibberish bare token fails as an unknown command without engine startup", async () => {
    const { stdout, stderr, exitCode } = await runCli(["xyzxyzxyz"]);
    const output = stdout + stderr;
    expect(exitCode).toBe(1);
    expect(stderr).toContain("unknown command 'xyzxyzxyz'");
    expect(output).not.toContain("Did you mean");
    expect(output).not.toContain("FluidAudio");
    expect(output.toLowerCase()).not.toContain("file not found");
  });

  test("--json + --toon are mutually exclusive → exit 2 (#138)", async () => {
    // Exit 2 fires before any engine spawn, so this runs without the engine
    // installed and validates the subprocess-level contract (matches the
    // existing `empty text exits 2` pattern in rust/tests/tts_smoke.rs).
    const { stderr, exitCode } = await runCli(["--json", "--toon", "a.wav"]);
    expect(exitCode).toBe(2);
    expect(stderr.toLowerCase()).toContain("mutually exclusive");
  });

  test("--timestamps without machine-readable output exits 2", async () => {
    const { stderr, exitCode } = await runCli(["--timestamps", "a.wav"]);
    expect(exitCode).toBe(2);
    expect(stderr).toContain("--timestamps requires");
  });

  test("redirected --json --speakers reports progress on stderr and keeps stdout JSON", async () => {
    const dir = mkdtempSync(join(tmpdir(), "kesha-fake-cli-"));
    tempDirs.push(dir);
    const enginePath = createFakeEngine(dir);
    const mediaPath = join(dir, "workshop.mp4");
    writeFileSync(mediaPath, "fake media");

    const { stdout, stderr, exitCode } = await runCli(
      [mediaPath, "--json", "--speakers"],
      {
        env: {
          HOME: dir,
          KESHA_CACHE_DIR: dir,
          KESHA_ENGINE_BIN: enginePath,
        },
      },
    );

    expect(exitCode).toBe(0);
    expect(stderr).toContain(`Transcribing ${mediaPath}...`);
    expect(stderr).toMatch(/Transcribed .*workshop\.mp4 \(\d+ms\)/);

    const parsed = JSON.parse(stdout);
    expect(parsed).toHaveLength(1);
    expect(parsed[0].text).toBe("Привет с воркшопа");
    expect(parsed[0].segments[0].speaker).toBe(0);
  });
});
