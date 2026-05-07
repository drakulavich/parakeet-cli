import { describe, test, expect } from "bun:test";

const CWD = import.meta.dir + "/../..";

async function runCli(args: string[]): Promise<{ stdout: string; stderr: string; exitCode: number }> {
  const proc = Bun.spawn(["bun", "run", "src/cli.ts", ...args], {
    stdout: "pipe",
    stderr: "pipe",
    cwd: CWD,
  });

  const [stdout, stderr, exitCode] = await Promise.all([
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
    proc.exited,
  ]);

  return { stdout: stdout.trim(), stderr: stderr.trim(), exitCode };
}

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

  test("gibberish subcommand shows no suggestion", async () => {
    const { stdout, stderr } = await runCli(["xyzxyzxyz"]);
    const output = stdout + stderr;
    expect(output).not.toContain("Did you mean");
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
});
