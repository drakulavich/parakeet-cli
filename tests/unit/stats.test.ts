import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { existsSync, mkdtempSync, rmSync } from "fs";
import { join } from "path";
import { tmpdir } from "os";
import {
  createStatsRecorder,
  disableStats,
  enableStats,
  getRecentErrors,
  getStatsStatus,
  getWeekSummary,
  renderErrors,
  renderWeekSummary,
  resolveStatsDbPath,
  sanitizeStatsError,
} from "../../src/stats";

describe("stats storage", () => {
  let dir: string;
  const savedStatsDb = process.env.KESHA_STATS_DB;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "kesha-stats-test-"));
    process.env.KESHA_STATS_DB = join(dir, "stats.sqlite");
  });

  afterEach(() => {
    if (savedStatsDb === undefined) delete process.env.KESHA_STATS_DB;
    else process.env.KESHA_STATS_DB = savedStatsDb;
    rmSync(dir, { recursive: true, force: true });
  });

  test("resolveStatsDbPath respects KESHA_STATS_DB", () => {
    expect(resolveStatsDbPath()).toBe(join(dir, "stats.sqlite"));
  });

  test("status is disabled before the database exists", () => {
    const status = getStatsStatus();
    expect(status.enabled).toBe(false);
    expect(status.exists).toBe(false);
    expect(status.runCount).toBe(0);
  });

  test("enable and disable are idempotent and preserve the database", () => {
    enableStats();
    enableStats();
    expect(existsSync(resolveStatsDbPath())).toBe(true);
    expect(getStatsStatus().enabled).toBe(true);

    disableStats();
    disableStats();
    const status = getStatsStatus();
    expect(status.exists).toBe(true);
    expect(status.enabled).toBe(false);
  });

  test("recorder writes runs, stage timings, artifacts, and errors when enabled", async () => {
    enableStats();
    const recorder = createStatsRecorder("transcribe");
    expect(recorder.enabled).toBe(true);
    recorder.recordArtifact({ kind: "input_audio", format: ".ogg", sizeBytes: 1024 });
    await recorder.timeStage("transcribe", async () => "ok");
    recorder.recordError("transcribe", new Error(`${dir}/secret.ogg failed?token=abc`));
    recorder.finish("failed", 1);

    const status = getStatsStatus();
    const week = getWeekSummary();
    const errors = getRecentErrors();

    expect(status.runCount).toBe(1);
    expect(week.runs).toBe(1);
    expect(week.failures).toBe(1);
    expect(week.inputFiles).toBe(1);
    expect(week.inputBytes).toBe(1024);
    expect(week.sttTimeMs).toBeGreaterThanOrEqual(0);
    expect(errors).toHaveLength(1);
    expect(errors[0].message).not.toContain(dir);
    expect(errors[0].message).not.toContain("secret.ogg");
  });

  test("recorder is a no-op when disabled", async () => {
    const recorder = createStatsRecorder("say");
    expect(recorder.enabled).toBe(false);
    await recorder.timeStage("tts", async () => "ok");
    recorder.recordError("tts", new Error("boom"));
    recorder.finish("failed", 1);
    expect(getStatsStatus().runCount).toBe(0);
  });

  test("renderers handle empty data", () => {
    expect(renderWeekSummary(getWeekSummary())).toContain("Runs: 0");
    expect(renderErrors(getRecentErrors())).toContain("no recorded errors");
  });
});

describe("sanitizeStatsError", () => {
  test("removes paths, query strings, stack lines, and truncates long messages", () => {
    const long = "x".repeat(400);
    const err = new Error(`/Users/alice/audio/private.wav failed at https://example.com/a?token=secret {"text":"private transcript"}\n    at frame\n${long}`);
    const sanitized = sanitizeStatsError(err);
    expect(sanitized.errorClass).toBe("Error");
    expect(sanitized.message).not.toContain("/Users/alice");
    expect(sanitized.message).not.toContain("private.wav");
    expect(sanitized.message).not.toContain("token=secret");
    expect(sanitized.message).not.toContain("private transcript");
    expect(sanitized.message).not.toContain("at frame");
    expect(sanitized.message.length).toBeLessThanOrEqual(300);
  });
});
