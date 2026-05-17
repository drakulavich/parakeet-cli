import { afterEach, describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "fs";
import { join } from "path";
import { tmpdir } from "os";
import { renderInstallPlan } from "../../src/install-plan";

const savedEnv = {
  HOME: process.env.HOME,
  KESHA_CACHE_DIR: process.env.KESHA_CACHE_DIR,
  KESHA_ENGINE_BIN: process.env.KESHA_ENGINE_BIN,
};

function restoreEnv() {
  for (const [key, value] of Object.entries(savedEnv)) {
    if (value === undefined) delete process.env[key];
    else process.env[key] = value;
  }
}

afterEach(restoreEnv);

describe("renderInstallPlan", () => {
  test("describes install economics without mutating local state", async () => {
    const dir = mkdtempSync(join(tmpdir(), "kesha-install-plan-test-"));
    try {
      process.env.HOME = dir;
      process.env.KESHA_CACHE_DIR = join(dir, "cache");
      process.env.KESHA_ENGINE_BIN = join(dir, "engine", "bin", "kesha-engine");

      const output = await renderInstallPlan({ tts: true });

      expect(output).toContain("Kesha install plan");
      expect(output).toContain("ASR Parakeet TDT v3");
      expect(output).toContain("Audio language ID ECAPA");
      expect(output).toContain("TTS");
      expect(output).toContain("Cold-cache download:");
      expect(output).toContain("Expected network for this run:");
      expect(output).toContain("No files are downloaded or changed by --plan.");
      expect(output).toContain("Run: kesha install --tts");
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  test("--no-cache marks components for refresh", async () => {
    const dir = mkdtempSync(join(tmpdir(), "kesha-install-plan-refresh-test-"));
    try {
      process.env.HOME = dir;
      process.env.KESHA_CACHE_DIR = join(dir, "cache");
      process.env.KESHA_ENGINE_BIN = join(dir, "engine", "bin", "kesha-engine");

      const output = await renderInstallPlan({ noCache: true, vad: true });

      expect(output).toContain("refresh");
      expect(output).toContain("VAD Silero v5");
      expect(output).toContain("Run: kesha install --no-cache --vad");
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  test("darwin sidecar cache state follows installed filenames", async () => {
    const dir = mkdtempSync(join(tmpdir(), "kesha-install-plan-sidecar-test-"));
    try {
      process.env.HOME = dir;
      process.env.KESHA_CACHE_DIR = join(dir, "cache");
      const engineDir = join(dir, "engine", "bin");
      process.env.KESHA_ENGINE_BIN = join(engineDir, "kesha-engine");
      mkdirSync(engineDir, { recursive: true });
      writeFileSync(join(engineDir, "kesha-diarize-darwin-arm64"), "sidecar");

      const output = await renderInstallPlan({ noCache: true });

      if (process.platform === "darwin" && process.arch === "arm64") {
        expect(output).toContain("Sidecar kesha-diarize-darwin-arm64");
        expect(output).toContain("refresh, GitHub release");
      } else {
        expect(output).not.toContain("Sidecar kesha-diarize-darwin-arm64");
      }
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});
