import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "fs";
import { join } from "path";
import { tmpdir } from "os";
import {
  collectDoctorReport,
  formatDoctorReport,
  redactDiagnosticValue,
} from "../../src/doctor";

describe("redactDiagnosticValue", () => {
  test("redacts secret-like keys", () => {
    expect(redactDiagnosticValue("API_KEY", "secret", "/tmp/home")).toBe("[REDACTED]");
    expect(redactDiagnosticValue("GITHUB_TOKEN", "secret", "/tmp/home")).toBe("[REDACTED]");
    expect(redactDiagnosticValue("MONKEY_MODE", "banana", "/tmp/home")).toBe("banana");
  });

  test("redacts home directory paths", () => {
    expect(redactDiagnosticValue("KESHA_CACHE_DIR", "/tmp/home/.cache/kesha", "/tmp/home")).toBe("~/.cache/kesha");
    expect(redactDiagnosticValue("KESHA_CACHE_DIR", "/tmp/home", "/tmp/home")).toBe("~");
    expect(
      redactDiagnosticValue(
        "KESHA_CACHE_DIR",
        "C:\\Users\\Runner\\.cache\\kesha",
        "C:\\Users\\Runner",
      ),
    ).toBe("~/.cache/kesha");
    expect(
      redactDiagnosticValue(
        "probeError",
        "spawn /tmp/home/.cache/kesha/engine/bin/kesha-engine ENOENT",
        "/tmp/home",
      ),
    ).toBe("spawn ~/.cache/kesha/engine/bin/kesha-engine ENOENT");
  });

  test("strips credentials and query strings from URLs", () => {
    expect(
      redactDiagnosticValue(
        "KESHA_MODEL_MIRROR",
        "https://user:pass@example.com/kesha?token=abc#frag",
        "/tmp/home",
      ),
    ).toBe("https://example.com/kesha");
    expect(
      redactDiagnosticValue(
        "KESHA_MODEL_MIRROR",
        "https://user:pass@example.com/tmp/home/mirror?token=abc",
        "/tmp/home",
      ),
    ).toBe("https://example.com/~/mirror");
  });
});

describe("collectDoctorReport", () => {
  const savedEnv = {
    HOME: process.env.HOME,
    KESHA_ENGINE_BIN: process.env.KESHA_ENGINE_BIN,
    KESHA_CACHE_DIR: process.env.KESHA_CACHE_DIR,
    KESHA_MODEL_MIRROR: process.env.KESHA_MODEL_MIRROR,
    KESHA_STATS_DB: process.env.KESHA_STATS_DB,
    KESHA_DEBUG: process.env.KESHA_DEBUG,
    KESHA_DEBUG_FD: process.env.KESHA_DEBUG_FD,
  };

  function restoreEnv() {
    for (const [key, value] of Object.entries(savedEnv)) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
  }

  beforeEach(restoreEnv);
  afterEach(restoreEnv);

  test("reports missing engine without throwing", async () => {
    const dir = mkdtempSync(join(tmpdir(), "kesha-doctor-test-"));
    try {
      process.env.HOME = dir;
      process.env.KESHA_ENGINE_BIN = join(dir, "engine", "bin", "kesha-engine");
      process.env.KESHA_CACHE_DIR = join(dir, ".cache", "kesha");
      process.env.KESHA_STATS_DB = join(dir, "stats.sqlite");
      process.env.KESHA_MODEL_MIRROR = "https://user:pass@example.com/kesha?token=abc";
      process.env.KESHA_DEBUG = "1";

      mkdirSync(join(dir, ".cache", "kesha", "models", "silero-vad"), { recursive: true });
      writeFileSync(join(dir, ".cache", "kesha", "models", "silero-vad", "model.onnx"), "vad");

      const report = await collectDoctorReport({ redact: true });
      expect(report.redacted).toBe(true);
      expect(report.engine.installed).toBe(false);
      expect(report.engine.path).toBe("~/engine/bin/kesha-engine");
      expect(report.cache.path).toBe("~/.cache/kesha");
      expect(report.cache.totalBytes).toBeGreaterThan(0);
      expect(report.env.KESHA_MODEL_MIRROR).toBe("https://example.com/kesha");
      expect(report.env.KESHA_DEBUG).toBe("1");
      expect("runCount" in report.stats).toBe(true);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  test("formats a human-readable report", async () => {
    const dir = mkdtempSync(join(tmpdir(), "kesha-doctor-format-test-"));
    try {
      process.env.HOME = dir;
      process.env.KESHA_ENGINE_BIN = join(dir, "engine", "bin", "kesha-engine");
      process.env.KESHA_CACHE_DIR = join(dir, ".cache", "kesha");
      process.env.KESHA_STATS_DB = join(dir, "stats.sqlite");

      const output = formatDoctorReport(await collectDoctorReport({ redact: true }));
      expect(output).toContain("Kesha Doctor");
      expect(output).toContain("Runtime:");
      expect(output).toContain("Engine:");
      expect(output).toContain("Environment:");
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});
