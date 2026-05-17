import { describe, it, expect } from "bun:test";
import { spawn } from "bun";
import { chmodSync, mkdirSync } from "fs";

const CLI_PATH = new URL("../../bin/kesha.js", import.meta.url).pathname;

async function createFakeEngine(dir: string): Promise<string> {
  mkdirSync(dir, { recursive: true });
  const enginePath = `${dir}/kesha-engine`;
  await Bun.write(enginePath, `#!/usr/bin/env bun
const args = Bun.argv.slice(2);
if (args[0] !== "say") {
  console.error("unexpected args: " + args.join(" "));
  process.exit(2);
}
const outIndex = args.indexOf("--out");
await new Response(Bun.stdin.stream()).text();
if (outIndex >= 0) {
  await Bun.write(args[outIndex + 1], new Uint8Array([82, 73, 70, 70, 0, 0, 0, 0]));
} else {
  process.stdout.write(new Uint8Array([82, 73, 70, 70, 0, 0, 0, 0]));
}
`);
  chmodSync(enginePath, 0o755);
  return enginePath;
}

async function createFailingEngine(dir: string): Promise<string> {
  mkdirSync(dir, { recursive: true });
  const enginePath = `${dir}/kesha-engine-fail-on-use`;
  await Bun.write(enginePath, `#!/usr/bin/env bun
console.error("fake engine should not have been invoked: " + JSON.stringify(Bun.argv.slice(2)));
process.exit(99);
`);
  chmodSync(enginePath, 0o755);
  return enginePath;
}

describe("kesha say (CLI)", () => {
  it("--help exits 0 and mentions --voice", async () => {
    const proc = spawn(["bun", CLI_PATH, "say", "--help"], {
      stdout: "pipe",
      stderr: "pipe",
    });
    const exit = await proc.exited;
    expect(exit).toBe(0);
    const stdout = await new Response(proc.stdout).text();
    expect(stdout).toMatch(/--voice/);
  });

  it("shows install hint when engine not installed (empty cache)", async () => {
    const dir = `/tmp/kesha-empty-${Date.now()}-${Math.random()}`;
    const proc = spawn(["bun", CLI_PATH, "say", "Hello"], {
      env: { ...process.env, KESHA_CACHE_DIR: dir, HOME: dir },
      stdout: "pipe",
      stderr: "pipe",
    });
    const exit = await proc.exited;
    const stderr = await new Response(proc.stderr).text();
    // Engine not installed exits 1 from the TS wrapper; stderr should point at install.
    expect([1, 4]).toContain(exit);
    expect(stderr).toMatch(/install/);
  });

  it("say --out reports stderr progress while keeping stdout empty", async () => {
    const dir = `/tmp/kesha-fake-engine-${Date.now()}-${Math.random()}`;
    const enginePath = await createFakeEngine(dir);
    const outPath = `${dir}/reply.wav`;
    const proc = spawn([
      "bun",
      CLI_PATH,
      "say",
      "--voice",
      "ru-vosk-m02",
      "--out",
      outPath,
      "Привет",
    ], {
      env: {
        ...process.env,
        KESHA_CACHE_DIR: dir,
        KESHA_ENGINE_BIN: enginePath,
        HOME: dir,
      },
      stdout: "pipe",
      stderr: "pipe",
    });

    expect(await proc.exited).toBe(0);
    const stdout = await new Response(proc.stdout).text();
    const stderr = await new Response(proc.stderr).text();
    const bytes = new Uint8Array(await Bun.file(outPath).arrayBuffer());

    expect(stdout).toBe("");
    expect(stderr).toContain("Synthesizing ru-vosk-m02 ->");
    expect(stderr).toContain(outPath);
    expect(stderr).toMatch(/Saved .*reply\.wav \(\d+ms\)/);
    expect(new TextDecoder().decode(bytes.slice(0, 4))).toBe("RIFF");
  });

  it("--verbose --out does not duplicate timing output", async () => {
    const dir = `/tmp/kesha-fake-engine-${Date.now()}-${Math.random()}`;
    const enginePath = await createFakeEngine(dir);
    const outPath = `${dir}/reply.wav`;
    const proc = spawn([
      "bun",
      CLI_PATH,
      "say",
      "--voice",
      "ru-vosk-m02",
      "--out",
      outPath,
      "--verbose",
      "Привет",
    ], {
      env: {
        ...process.env,
        KESHA_CACHE_DIR: dir,
        KESHA_ENGINE_BIN: enginePath,
        HOME: dir,
      },
      stdout: "pipe",
      stderr: "pipe",
    });

    expect(await proc.exited).toBe(0);
    const stderr = await new Response(proc.stderr).text();

    expect(stderr).toMatch(/Saved .*reply\.wav \(\d+ms\)/);
    expect(stderr).not.toContain("TTS time:");
  });

  it("rejects invalid numeric flags before spawning the engine", async () => {
    const cases: Array<{ args: string[]; message: string }> = [
      { args: ["--rate", "fast", "Hello"], message: "--rate must be a finite number" },
      { args: ["--rate", "3", "Hello"], message: "--rate must be between 0.5 and 2.0" },
      { args: ["--format", "ogg-opus", "--bitrate", "wide", "Hello"], message: "--bitrate must be a finite number" },
      { args: ["--format", "ogg-opus", "--bitrate", "-1", "Hello"], message: "--bitrate must be a positive integer" },
      { args: ["--format", "ogg-opus", "--sample-rate", "44100", "Hello"], message: "--sample-rate must be one of" },
    ];

    for (const tc of cases) {
      const dir = `/tmp/kesha-fail-engine-${Date.now()}-${Math.random()}`;
      const enginePath = await createFailingEngine(dir);
      const proc = spawn(["bun", CLI_PATH, "say", "--voice", "ru-vosk-m02", ...tc.args], {
        env: {
          ...process.env,
          KESHA_CACHE_DIR: dir,
          KESHA_ENGINE_BIN: enginePath,
          HOME: dir,
        },
        stdout: "pipe",
        stderr: "pipe",
      });

      expect(await proc.exited).toBe(2);
      const stdout = await new Response(proc.stdout).text();
      const stderr = await new Response(proc.stderr).text();
      expect(stdout).toBe("");
      expect(stderr).toContain(tc.message);
      expect(stderr).not.toContain("fake engine should not have been invoked");
    }
  });
});
