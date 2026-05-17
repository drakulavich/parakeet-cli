import { describe, test, expect, beforeEach, afterEach } from "bun:test";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "fs";
import { join } from "path";
import { tmpdir } from "os";
import { formatStatusLine, activeModelMirror, showStatus } from "../../src/status";
import { starSeenPath } from "../../src/star";

describe("formatStatusLine", () => {
  test("formats installed component", () => {
    const line = formatStatusLine("Binary", "/path/to/bin", true);
    expect(line).toContain("Binary");
    expect(line).toContain("/path/to/bin");
    expect(line).toContain("✓");
    expect(line).not.toContain("✗");
  });

  test("formats missing component", () => {
    const line = formatStatusLine("Binary", null, false);
    expect(line).toContain("Binary");
    expect(line).toContain("✗");
    expect(line).toContain("not installed");
  });

  test("formats missing component with custom label", () => {
    const line = formatStatusLine("ffmpeg", null, false, "not found");
    expect(line).toContain("not found");
  });
});

describe("activeModelMirror (#121)", () => {
  const saved = process.env.KESHA_MODEL_MIRROR;

  beforeEach(() => {
    delete process.env.KESHA_MODEL_MIRROR;
  });
  afterEach(() => {
    if (saved === undefined) delete process.env.KESHA_MODEL_MIRROR;
    else process.env.KESHA_MODEL_MIRROR = saved;
  });

  test("null when unset", () => {
    expect(activeModelMirror()).toBeNull();
  });

  test("null when empty", () => {
    process.env.KESHA_MODEL_MIRROR = "";
    expect(activeModelMirror()).toBeNull();
  });

  test("null when whitespace-only", () => {
    process.env.KESHA_MODEL_MIRROR = "   ";
    expect(activeModelMirror()).toBeNull();
  });

  test("returns the URL when set", () => {
    process.env.KESHA_MODEL_MIRROR = "https://mirror.example.com/kesha";
    expect(activeModelMirror()).toBe("https://mirror.example.com/kesha");
  });

  test("strips trailing slashes to match the Rust side", () => {
    process.env.KESHA_MODEL_MIRROR = "https://mirror.example.com/kesha///";
    expect(activeModelMirror()).toBe("https://mirror.example.com/kesha");
  });
});

describe("showStatus", () => {
  const savedEngineBin = process.env.KESHA_ENGINE_BIN;
  const savedCacheDir = process.env.KESHA_CACHE_DIR;
  const savedHome = process.env.HOME;

  function restoreEnv() {
    if (savedEngineBin === undefined) delete process.env.KESHA_ENGINE_BIN;
    else process.env.KESHA_ENGINE_BIN = savedEngineBin;
    if (savedCacheDir === undefined) delete process.env.KESHA_CACHE_DIR;
    else process.env.KESHA_CACHE_DIR = savedCacheDir;
    if (savedHome === undefined) delete process.env.HOME;
    else process.env.HOME = savedHome;
  }

  beforeEach(restoreEnv);
  afterEach(restoreEnv);

  test("does not consume the star prompt marker slot", async () => {
    const dir = mkdtempSync(join(tmpdir(), "kesha-status-test-"));
    const binDir = join(dir, "engine", "bin");
    mkdirSync(binDir, { recursive: true });
    const binPath = join(binDir, "kesha-engine");
    writeFileSync(binPath, "not a real executable");

    process.env.KESHA_ENGINE_BIN = binPath;
    process.env.KESHA_CACHE_DIR = dir;
    process.env.HOME = dir;

    const originalLog = console.log;
    const originalError = console.error;
    console.log = () => {};
    console.error = () => {};
    try {
      await showStatus();
      expect(existsSync(starSeenPath(binPath))).toBe(false);
    } finally {
      console.log = originalLog;
      console.error = originalError;
      rmSync(dir, { recursive: true, force: true });
    }
  });

  test("does not scan or print disk usage unless requested", async () => {
    const dir = mkdtempSync(join(tmpdir(), "kesha-status-test-"));
    const binDir = join(dir, "engine", "bin");
    mkdirSync(binDir, { recursive: true });
    const binPath = join(binDir, "kesha-engine");
    writeFileSync(binPath, "not a real executable");
    mkdirSync(join(dir, "models", "parakeet-tdt-v3"), { recursive: true });
    writeFileSync(join(dir, "models", "parakeet-tdt-v3", "model.onnx"), "model");

    process.env.KESHA_ENGINE_BIN = binPath;
    process.env.KESHA_CACHE_DIR = dir;
    process.env.HOME = dir;

    const originalLog = console.log;
    const originalError = console.error;
    const lines: string[] = [];
    console.log = (msg: string) => {
      lines.push(msg);
    };
    console.error = () => {};
    try {
      await showStatus();
      expect(lines.join("\n")).not.toContain("Disk usage");

      lines.length = 0;
      await showStatus({ disk: true });
      expect(lines.join("\n")).toContain("Disk usage");
    } finally {
      console.log = originalLog;
      console.error = originalError;
      rmSync(dir, { recursive: true, force: true });
    }
  });
});
