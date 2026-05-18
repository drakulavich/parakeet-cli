import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { Database } from "bun:sqlite";
import { existsSync, mkdtempSync, rmSync } from "fs";
import { join } from "path";
import { tmpdir } from "os";
import {
  createStatsRecorder,
  disableStats,
  enableStats,
  exportStats,
  getRecentErrors,
  getStatsStatus,
  getWeekSummary,
  renderErrors,
  renderWeekSummary,
  resetStats,
  resolveStatsDbPath,
  sanitizeStatsError,
  setStatsRetentionDays,
  vacuumStats,
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
    expect(status.retentionDays).toBe(90);
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

  test("week summary renders stage percentiles, bottlenecks, buckets, and slowest runs", () => {
    enableStats();
    seedStatsRun({
      command: "transcribe",
      status: "success",
      startedAt: "2026-05-16T10:00:00.000Z",
      finishedAt: "2026-05-16T10:00:02.000Z",
      itemCount: 1,
      stages: [
        { stage: "transcribe", durationMs: 100, status: "success" },
        { stage: "lang_id_audio", durationMs: 30, status: "success" },
      ],
      inputArtifact: { format: "wav", sizeBytes: 500_000, durationMs: 45_000 },
    });
    seedStatsRun({
      command: "say",
      status: "success",
      startedAt: "2026-05-16T11:00:00.000Z",
      finishedAt: "2026-05-16T11:00:02.000Z",
      itemCount: 1,
      stages: [{ stage: "tts", durationMs: 1_500, status: "success" }],
    });
    seedStatsRun({
      command: "transcribe",
      status: "failed",
      startedAt: "2026-05-16T12:00:00.000Z",
      finishedAt: "2026-05-16T12:00:05.000Z",
      itemCount: 2,
      stages: [
        { stage: "transcribe", durationMs: 3_000, status: "failed" },
        { stage: "lang_id_text", durationMs: 100, status: "success" },
      ],
      inputArtifact: { format: "mp3", sizeBytes: 15 * 1024 * 1024, durationMs: 15 * 60_000 },
    });
    seedStatsRun({
      command: "say",
      status: "success",
      startedAt: "2026-04-01T10:00:00.000Z",
      finishedAt: "2026-04-01T10:00:20.000Z",
      itemCount: 1,
      stages: [{ stage: "tts", durationMs: 20_000, status: "success" }],
    });

    const summary = getWeekSummary(new Date("2026-05-17T00:00:00.000Z"));
    const rendered = renderWeekSummary(summary);

    expect(summary.runs).toBe(3);
    expect(summary.stageBreakdown[0]).toMatchObject({
      stage: "transcribe",
      count: 2,
      failed: 1,
      totalMs: 3_100,
      p50Ms: 100,
      p95Ms: 3_000,
      p99Ms: 3_000,
    });
    expect(rendered).toContain("Stage breakdown:");
    expect(rendered).toContain("transcribe: count 2, failed 1, total 3s, p50 100ms, p95 3s, p99 3s");
    expect(rendered).toContain("Bottlenecks:");
    expect(rendered).toContain("Total time: transcribe 3s, tts 2s");
    expect(rendered).toContain("p95 latency: transcribe 3s, tts 2s");
    expect(rendered).toContain("Input shape:");
    expect(rendered).toContain("Format: mp3 1, wav 1");
    expect(rendered).toContain("Size: <1 MB 1, 10-100 MB 1");
    expect(rendered).toContain("Duration: <1 min 1, 10-60 min 1");
    expect(rendered).toContain("Slowest anonymous runs:");
    expect(rendered).toContain("transcribe failed, 2 item(s), 5s | transcribe 3s failed");
    expect(rendered).not.toContain("secret");
  });

  test("exports content-free JSON and CSV", () => {
    enableStats();
    const secretPath = `${dir}/private-recording.wav`;
    seedStatsRun({
      command: "transcribe",
      status: "failed",
      startedAt: "2026-05-16T10:00:00.000Z",
      finishedAt: "2026-05-16T10:00:01.000Z",
      itemCount: 1,
      stages: [{ stage: "transcribe", durationMs: 100, status: "failed" }],
      inputArtifact: { format: "wav", sizeBytes: 1234, durationMs: 2000 },
      error: new Error(`${secretPath} failed with {"text":"private words"}`),
    });

    const json = exportStats("json");
    const parsed = JSON.parse(json);
    expect(parsed.privacy.contentFree).toBe(true);
    expect(parsed.privacy.neverStored).toContain("transcripts");
    expect(parsed.runs).toHaveLength(1);
    expect(parsed.artifacts[0]).toMatchObject({ kind: "input_audio", format: "wav", sizeBytes: 1234 });
    expect(json).not.toContain(secretPath);
    expect(json).not.toContain("private-recording.wav");
    expect(json).not.toContain("private words");

    const csv = exportStats("csv");
    expect(csv).toContain("table,id,run_id");
    expect(csv).toContain("runs,");
    expect(csv).toContain("artifacts,");
    expect(csv).toContain("stage_timings,");
    expect(csv).toContain("errors,");
    expect(csv).not.toContain(secretPath);
    expect(csv).not.toContain("private-recording.wav");
    expect(csv).not.toContain("private words");
  });

  test("reset deletes stats records but preserves settings", () => {
    enableStats();
    setStatsRetentionDays(30);
    seedStatsRun({
      command: "say",
      status: "success",
      startedAt: "2026-05-16T10:00:00.000Z",
      finishedAt: "2026-05-16T10:00:01.000Z",
      itemCount: 1,
      stages: [{ stage: "tts", durationMs: 100, status: "success" }],
    });

    const result = resetStats();
    const status = getStatsStatus();

    expect(result.runs).toBe(1);
    expect(result.stageTimings).toBe(1);
    expect(status.enabled).toBe(true);
    expect(status.retentionDays).toBe(30);
    expect(status.runCount).toBe(0);
  });

  test("retention prunes old runs before recording new stats", () => {
    enableStats();
    setStatsRetentionDays(7);
    seedStatsRun({
      command: "transcribe",
      status: "success",
      startedAt: "2020-01-01T00:00:00.000Z",
      finishedAt: "2020-01-01T00:00:01.000Z",
      itemCount: 1,
      stages: [{ stage: "transcribe", durationMs: 100, status: "success" }],
      inputArtifact: { format: "wav", sizeBytes: 100, durationMs: 1000 },
    });
    seedStatsRun({
      command: "say",
      status: "success",
      startedAt: new Date().toISOString(),
      finishedAt: new Date().toISOString(),
      itemCount: 1,
      stages: [{ stage: "tts", durationMs: 100, status: "success" }],
    });

    const recorder = createStatsRecorder("say");
    recorder.finish("success", 1);

    const exported = JSON.parse(exportStats("json"));
    expect(exported.runs.map((run: { command: string }) => run.command)).not.toContain("transcribe");
    expect(exported.artifacts).toHaveLength(0);
    expect(getStatsStatus().retentionDays).toBe(7);
  });

  test("vacuum returns database size information", () => {
    enableStats();
    seedStatsRun({
      command: "say",
      status: "success",
      startedAt: "2026-05-16T10:00:00.000Z",
      finishedAt: "2026-05-16T10:00:01.000Z",
      itemCount: 1,
      stages: [{ stage: "tts", durationMs: 100, status: "success" }],
    });

    const result = vacuumStats();

    expect(result.dbPath).toBe(resolveStatsDbPath());
    expect(result.beforeBytes).toBeGreaterThan(0);
    expect(result.afterBytes).toBeGreaterThan(0);
    expect(getStatsStatus().runCount).toBe(1);
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

function seedStatsRun(input: {
  command: "transcribe" | "say";
  status: "success" | "failed";
  startedAt: string;
  finishedAt: string;
  itemCount: number;
  stages: Array<{ stage: string; durationMs: number; status: "success" | "failed" }>;
  inputArtifact?: { format: string; sizeBytes: number; durationMs: number };
  error?: Error;
}): void {
  const db = new Database(resolveStatsDbPath());
  try {
    const run = db.query(
      `insert into runs
        (command, started_at, finished_at, status, app_version, item_count)
       values (?, ?, ?, ?, 'test', ?)
       returning id`,
    ).get(input.command, input.startedAt, input.finishedAt, input.status, input.itemCount) as { id: number };

    if (input.inputArtifact) {
      db.query(
        `insert into artifacts
          (run_id, kind, format, size_bytes, duration_ms, sample_rate, channels)
         values (?, 'input_audio', ?, ?, ?, null, null)`,
      ).run(run.id, input.inputArtifact.format, input.inputArtifact.sizeBytes, input.inputArtifact.durationMs);
    }

    for (const stage of input.stages) {
      db.query(
        `insert into stage_timings
          (run_id, stage, started_at, duration_ms, status)
         values (?, ?, ?, ?, ?)`,
      ).run(run.id, stage.stage, input.startedAt, stage.durationMs, stage.status);
    }

    if (input.error) {
      const { errorClass, message } = sanitizeStatsError(input.error);
      db.query(
        `insert into errors
          (run_id, stage, error_class, error_code, sanitized_message, occurred_at)
         values (?, 'transcribe', ?, 'failed', ?, ?)`,
      ).run(run.id, errorClass, message, input.startedAt);
    }
  } finally {
    db.close();
  }
}
