import { Database } from "bun:sqlite";
import { existsSync, mkdirSync, statSync } from "fs";
import { homedir } from "os";
import { dirname, extname, join } from "path";
import { log } from "./log";
import { packageVersion } from "./package-info";

const SCHEMA_VERSION = 1;
const MAX_ERROR_MESSAGE_CHARS = 300;
const DEFAULT_RETENTION_DAYS = 90;
const APP_VERSION = readPackageVersion();

export type StatsCommandName = "transcribe" | "say";
export type StatsExportFormat = "json" | "csv";
export type StatsStageStatus = "success" | "failed";
export type StatsRunStatus = "success" | "failed";

export interface StatsArtifactInput {
  kind: "input_audio" | "output_audio";
  format?: string | null;
  sizeBytes?: number | null;
  durationMs?: number | null;
  sampleRate?: number | null;
  channels?: number | null;
}

export interface StatsRecorder {
  readonly enabled: boolean;
  recordArtifact(input: StatsArtifactInput): void;
  timeStage<T>(stage: string, fn: () => Promise<T>): Promise<T>;
  recordError(stage: string, err: unknown, errorCode?: string): void;
  finish(status: StatsRunStatus, itemCount?: number): void;
}

class NoopStatsRecorder implements StatsRecorder {
  readonly enabled = false;
  recordArtifact(): void {}
  async timeStage<T>(_stage: string, fn: () => Promise<T>): Promise<T> {
    return fn();
  }
  recordError(): void {}
  finish(): void {}
}

class SqliteStatsRecorder implements StatsRecorder {
  readonly enabled = true;
  private closed = false;

  constructor(
    private readonly db: Database,
    private readonly runId: number,
  ) {}

  recordArtifact(input: StatsArtifactInput): void {
    this.safeWrite(() => {
      this.db.query(
        `insert into artifacts
          (run_id, kind, format, size_bytes, duration_ms, sample_rate, channels)
         values (?, ?, ?, ?, ?, ?, ?)`,
      ).run(
        this.runId,
        input.kind,
        normalizeFormat(input.format),
        input.sizeBytes ?? null,
        input.durationMs ?? null,
        input.sampleRate ?? null,
        input.channels ?? null,
      );
    });
  }

  async timeStage<T>(stage: string, fn: () => Promise<T>): Promise<T> {
    const startedAt = new Date().toISOString();
    const t0 = performance.now();
    try {
      const result = await fn();
      this.recordStage(stage, startedAt, t0, "success");
      return result;
    } catch (err) {
      this.recordStage(stage, startedAt, t0, "failed");
      throw err;
    }
  }

  recordError(stage: string, err: unknown, errorCode?: string): void {
    const { errorClass, message } = sanitizeStatsError(err);
    this.safeWrite(() => {
      this.db.query(
        `insert into errors
          (run_id, stage, error_class, error_code, sanitized_message, occurred_at)
         values (?, ?, ?, ?, ?, ?)`,
      ).run(
        this.runId,
        stage,
        errorClass,
        errorCode ?? null,
        message,
        new Date().toISOString(),
      );
    });
  }

  finish(status: StatsRunStatus, itemCount = 0): void {
    if (this.closed) return;
    this.safeWrite(() => {
      this.db.query(
        "update runs set finished_at = ?, status = ?, item_count = ? where id = ?",
      ).run(new Date().toISOString(), status, itemCount, this.runId);
    });
    this.closed = true;
    this.db.close();
  }

  private recordStage(
    stage: string,
    startedAt: string,
    startedMs: number,
    status: StatsStageStatus,
  ): void {
    this.safeWrite(() => {
      this.db.query(
        `insert into stage_timings
          (run_id, stage, started_at, duration_ms, status)
         values (?, ?, ?, ?, ?)`,
      ).run(
        this.runId,
        stage,
        startedAt,
        Math.max(0, Math.round(performance.now() - startedMs)),
        status,
      );
    });
  }

  private safeWrite(fn: () => void): void {
    try {
      fn();
    } catch (err) {
      warnStatsWriteFailed(err);
    }
  }
}

let warnedStatsWriteFailed = false;

function warnStatsWriteFailed(err: unknown): void {
  if (!warnedStatsWriteFailed) {
    log.warn("warning: failed to write Kesha Stats; continuing without stats for this event");
    warnedStatsWriteFailed = true;
  }
  const message = err instanceof Error ? err.message : String(err);
  log.debug(`stats write failed: ${message}`);
}

export function createStatsRecorder(command: StatsCommandName): StatsRecorder {
  const dbPath = resolveStatsDbPath();
  if (!existsSync(dbPath)) return new NoopStatsRecorder();

  try {
    const db = openStatsDatabase(dbPath);
    if (!getStatsEnabled(db)) {
      db.close();
      return new NoopStatsRecorder();
    }
    const runId = insertRun(db, command);
    return new SqliteStatsRecorder(db, runId);
  } catch (err) {
    warnStatsWriteFailed(err);
    return new NoopStatsRecorder();
  }
}

export function enableStats(): void {
  const db = openStatsDatabase(resolveStatsDbPath());
  try {
    setSetting(db, "enabled", "1");
    applyStatsRetention(db);
  } finally {
    db.close();
  }
}

export function disableStats(): void {
  const db = openStatsDatabase(resolveStatsDbPath());
  try {
    setSetting(db, "enabled", "0");
  } finally {
    db.close();
  }
}

export interface StatsStatus {
  enabled: boolean;
  dbPath: string;
  runCount: number;
  exists: boolean;
  retentionDays: number | null;
}

export function getStatsStatus(): StatsStatus {
  const dbPath = resolveStatsDbPath();
  if (!existsSync(dbPath)) {
    return {
      enabled: false,
      dbPath,
      runCount: 0,
      exists: false,
      retentionDays: defaultRetentionDays(),
    };
  }
  const db = openStatsDatabase(dbPath);
  try {
    return {
      enabled: getStatsEnabled(db),
      dbPath,
      runCount: Number((db.query("select count(*) as n from runs").get() as { n: number }).n),
      exists: true,
      retentionDays: getStatsRetentionDays(db),
    };
  } finally {
    db.close();
  }
}

export interface StatsResetResult {
  runs: number;
  artifacts: number;
  stageTimings: number;
  errors: number;
}

export function resetStats(): StatsResetResult {
  const db = openStatsDatabase(resolveStatsDbPath());
  try {
    const artifacts = runChanges(db, "delete from artifacts");
    const stageTimings = runChanges(db, "delete from stage_timings");
    const errors = runChanges(db, "delete from errors");
    const runs = runChanges(db, "delete from runs");
    return { runs, artifacts, stageTimings, errors };
  } finally {
    db.close();
  }
}

export interface StatsVacuumResult {
  dbPath: string;
  beforeBytes: number;
  afterBytes: number;
}

export function vacuumStats(): StatsVacuumResult {
  const dbPath = resolveStatsDbPath();
  const beforeBytes = fileSize(dbPath);
  const db = openStatsDatabase(dbPath);
  try {
    db.exec("pragma wal_checkpoint(TRUNCATE)");
    db.exec("vacuum");
  } finally {
    db.close();
  }
  return {
    dbPath,
    beforeBytes,
    afterBytes: fileSize(dbPath),
  };
}

export function setStatsRetentionDays(days: number | null): void {
  if (days !== null && (!Number.isInteger(days) || days < 1)) {
    throw new Error("Stats retention must be a positive whole number of days, or 'off'");
  }
  const db = openStatsDatabase(resolveStatsDbPath());
  try {
    setSetting(db, "retention_days", days === null ? "off" : String(days));
    applyStatsRetention(db);
  } finally {
    db.close();
  }
}

export function exportStats(format: StatsExportFormat): string {
  const data = readStatsExport();
  switch (format) {
    case "json":
      return `${JSON.stringify(data, null, 2)}\n`;
    case "csv":
      return renderStatsCsv(data);
  }
}

export interface StatsWeekSummary {
  runs: number;
  successes: number;
  failures: number;
  inputFiles: number;
  inputBytes: number;
  inputDurationMs: number;
  sttTimeMs: number;
  ttsTimeMs: number;
  stageBreakdown: StatsStageSummary[];
  inputFormats: StatsCountBucket[];
  inputSizeBuckets: StatsCountBucket[];
  inputDurationBuckets: StatsCountBucket[];
  hasInputDurationData: boolean;
  slowestRuns: StatsSlowRun[];
}

export interface StatsStageSummary {
  stage: string;
  count: number;
  failed: number;
  totalMs: number;
  p50Ms: number;
  p95Ms: number;
  p99Ms: number;
}

export interface StatsCountBucket {
  label: string;
  count: number;
}

export interface StatsSlowRunStage {
  stage: string;
  durationMs: number;
  status: StatsStageStatus;
}

export interface StatsSlowRun {
  command: string;
  status: StatsRunStatus;
  durationMs: number | null;
  itemCount: number;
  stages: StatsSlowRunStage[];
}

interface StatsStageTimingRow {
  runId: number;
  stage: string;
  durationMs: number;
  status: StatsStageStatus;
}

interface StatsInputArtifactRow {
  format: string | null;
  sizeBytes: number | null;
  durationMs: number | null;
}

interface StatsRunTimingRow {
  id: number;
  command: string;
  status: StatsRunStatus;
  startedAt: string;
  finishedAt: string | null;
  itemCount: number;
}

export function getWeekSummary(now = new Date()): StatsWeekSummary {
  const dbPath = resolveStatsDbPath();
  if (!existsSync(dbPath)) return emptyWeekSummary();
  const since = new Date(now.getTime() - 7 * 24 * 60 * 60 * 1000).toISOString();
  const db = openStatsDatabase(dbPath);
  try {
    const stageRows = db.query(
      `select
        run_id as runId,
        stage,
        duration_ms as durationMs,
        status
       from stage_timings
       where run_id in (select id from runs where started_at >= ?)`,
    ).all(since) as StatsStageTimingRow[];

    const inputArtifacts = db.query(
      `select
        format,
        size_bytes as sizeBytes,
        duration_ms as durationMs
       from artifacts
       where kind = 'input_audio'
         and run_id in (select id from runs where started_at >= ?)`,
    ).all(since) as StatsInputArtifactRow[];

    const runRows = db.query(
      `select
        id,
        command,
        status,
        started_at as startedAt,
        finished_at as finishedAt,
        item_count as itemCount
       from runs
       where started_at >= ?`,
    ).all(since) as StatsRunTimingRow[];

    return {
      runs: runRows.length,
      successes: countRuns(runRows, "success"),
      failures: countRuns(runRows, "failed"),
      inputFiles: inputArtifacts.length,
      inputBytes: sumInputBytes(inputArtifacts),
      inputDurationMs: sumInputDuration(inputArtifacts),
      sttTimeMs: sumStageDuration(stageRows, "transcribe"),
      ttsTimeMs: sumStageDuration(stageRows, "tts"),
      stageBreakdown: summarizeStages(stageRows),
      inputFormats: summarizeFormats(inputArtifacts),
      inputSizeBuckets: summarizeSizeBuckets(inputArtifacts),
      inputDurationBuckets: summarizeDurationBuckets(inputArtifacts),
      hasInputDurationData: inputArtifacts.some((row) => typeof row.durationMs === "number"),
      slowestRuns: summarizeSlowestRuns(runRows, stageRows),
    };
  } finally {
    db.close();
  }
}

export interface StatsRunExportRow {
  id: number;
  command: string;
  startedAt: string;
  finishedAt: string | null;
  status: StatsRunStatus;
  appVersion: string;
  itemCount: number;
}

export interface StatsArtifactExportRow {
  id: number;
  runId: number;
  kind: string;
  format: string | null;
  sizeBytes: number | null;
  durationMs: number | null;
  sampleRate: number | null;
  channels: number | null;
}

export interface StatsStageTimingExportRow {
  id: number;
  runId: number;
  stage: string;
  startedAt: string;
  durationMs: number;
  status: StatsStageStatus;
}

export interface StatsExportData {
  schemaVersion: number;
  exportedAt: string;
  retentionDays: number | null;
  privacy: {
    contentFree: true;
    neverStored: string[];
  };
  runs: StatsRunExportRow[];
  artifacts: StatsArtifactExportRow[];
  stageTimings: StatsStageTimingExportRow[];
  errors: StatsErrorRow[];
}

export interface StatsErrorRow {
  occurredAt: string;
  command: string | null;
  stage: string | null;
  errorClass: string | null;
  errorCode: string | null;
  message: string;
}

export function getRecentErrors(limit = 20): StatsErrorRow[] {
  const dbPath = resolveStatsDbPath();
  if (!existsSync(dbPath)) return [];
  const db = openStatsDatabase(dbPath);
  try {
    const rows = db.query(
      `select
        e.occurred_at as occurredAt,
        r.command as command,
        e.stage as stage,
        e.error_class as errorClass,
        e.error_code as errorCode,
        e.sanitized_message as message
       from errors e
       left join runs r on r.id = e.run_id
       order by e.occurred_at desc
       limit ?`,
    ).all(limit) as StatsErrorRow[];
    return rows;
  } finally {
    db.close();
  }
}

export function renderWeekSummary(summary: StatsWeekSummary): string {
  const realtime = summary.inputDurationMs > 0 && summary.sttTimeMs > 0
    ? `${(summary.inputDurationMs / summary.sttTimeMs).toFixed(1)}x`
    : "n/a";
  const lines = [
    "Kesha Stats - last 7 days",
    `Runs: ${summary.runs} (${summary.successes} success, ${summary.failures} failed)`,
    `Input files: ${summary.inputFiles}`,
    `Input audio: ${humanDuration(summary.inputDurationMs)}`,
    `Input size: ${humanBytes(summary.inputBytes)}`,
    `STT time: ${humanDuration(summary.sttTimeMs)} (${realtime} realtime)`,
    `TTS time: ${humanDuration(summary.ttsTimeMs)}`,
    "",
    "Stage breakdown:",
  ];

  if (summary.stageBreakdown.length === 0) {
    lines.push("  no stage timings recorded");
  } else {
    for (const stage of summary.stageBreakdown) {
      lines.push(
        `  ${stage.stage}: count ${stage.count}, failed ${stage.failed}, total ${humanDuration(stage.totalMs)}, ` +
          `p50 ${humanDuration(stage.p50Ms)}, p95 ${humanDuration(stage.p95Ms)}, p99 ${humanDuration(stage.p99Ms)}`,
      );
    }
  }

  lines.push("", "Bottlenecks:");
  if (summary.stageBreakdown.length === 0) {
    lines.push("  no bottlenecks recorded");
  } else {
    lines.push(`  Total time: ${renderTopStages(summary.stageBreakdown, "totalMs")}`);
    lines.push(`  p95 latency: ${renderTopStages(summary.stageBreakdown, "p95Ms")}`);
  }

  lines.push("", "Input shape:");
  lines.push(`  Format: ${renderBuckets(summary.inputFormats)}`);
  lines.push(`  Size: ${renderBuckets(summary.inputSizeBuckets)}`);
  lines.push(`  Duration: ${summary.hasInputDurationData ? renderBuckets(summary.inputDurationBuckets) : "n/a"}`);

  lines.push("", "Slowest anonymous runs:");
  if (summary.slowestRuns.length === 0) {
    lines.push("  no completed runs recorded");
  } else {
    for (const run of summary.slowestRuns) {
      const stages = run.stages.length > 0
        ? run.stages.map((stage) => `${stage.stage} ${humanDuration(stage.durationMs)} ${stage.status}`).join(", ")
        : "no stages recorded";
      lines.push(
        `  ${run.command} ${run.status}, ${run.itemCount} item(s), ${humanDuration(run.durationMs ?? 0)} | ${stages}`,
      );
    }
  }

  return lines.join("\n");
}

export function renderErrors(rows: StatsErrorRow[]): string {
  if (rows.length === 0) return "Kesha Stats - no recorded errors";
  const lines = ["Kesha Stats - recent errors"];
  for (const row of rows) {
    const parts = [
      row.occurredAt,
      row.command ?? "unknown",
      row.stage ?? "unknown",
      row.errorCode ?? row.errorClass ?? "error",
    ];
    lines.push(`${parts.join(" | ")} | ${row.message}`);
  }
  return lines.join("\n");
}

export function resolveStatsDbPath(): string {
  if (process.env.KESHA_STATS_DB) return process.env.KESHA_STATS_DB;
  if (process.platform === "darwin") {
    return join(homedir(), "Library", "Application Support", "kesha", "stats.sqlite");
  }
  if (process.platform === "win32") {
    const base = process.env.APPDATA || join(homedir(), "AppData", "Roaming");
    return join(base, "kesha", "stats.sqlite");
  }
  const base = process.env.XDG_DATA_HOME || join(homedir(), ".local", "share");
  return join(base, "kesha", "stats.sqlite");
}

export function artifactFromFile(
  path: string,
  kind: StatsArtifactInput["kind"],
): StatsArtifactInput | null {
  try {
    const st = statSync(path);
    if (!st.isFile()) return null;
    return {
      kind,
      format: extname(path),
      sizeBytes: st.size,
    };
  } catch {
    return null;
  }
}

export function artifactFromBytes(
  bytes: number,
  kind: StatsArtifactInput["kind"],
  format?: string,
): StatsArtifactInput {
  return {
    kind,
    format,
    sizeBytes: bytes,
  };
}

export function sanitizeStatsError(err: unknown): { errorClass: string; message: string } {
  const errorClass = err instanceof Error ? err.name : typeof err;
  const raw = err instanceof Error ? err.message || err.name : String(err);
  let message = raw
    .split(/\r?\n/)
    .filter((line) => !/^\s*at\s+/.test(line))
    .join(" ")
    .replace(/\s+/g, " ")
    .trim();

  const home = homedir();
  const cwd = process.cwd();
  for (const path of [home, cwd]) {
    if (path) {
      message = message.replace(new RegExp(escapeRegExp(path), "g"), "<path>");
    }
  }

  message = message
    .replace(/(https?:\/\/[^\s?]+)\?[^\s]+/g, "$1?<redacted>")
    .replace(/("text"\s*:\s*)"[^"]*"/gi, '$1"<redacted>"')
    .replace(/("transcript"\s*:\s*)"[^"]*"/gi, '$1"<redacted>"')
    .replace(/("stdout"\s*:\s*)"[^"]*"/gi, '$1"<redacted>"')
    .replace(/("stderr"\s*:\s*)"[^"]*"/gi, '$1"<redacted>"')
    .replace(/\/(?:Users|home|tmp|private\/tmp|var\/folders)\/[^\s)"']+/g, "<path>")
    .replace(/[A-Za-z]:\\[^\s)"']+/g, "<path>");

  if (message.length === 0) message = errorClass || "unknown error";
  if (message.length > MAX_ERROR_MESSAGE_CHARS) {
    message = `${message.slice(0, MAX_ERROR_MESSAGE_CHARS - 3)}...`;
  }
  return { errorClass, message };
}

function openStatsDatabase(dbPath: string): Database {
  mkdirSync(dirname(dbPath), { recursive: true });
  const db = new Database(dbPath);
  try {
    db.exec("pragma journal_mode = WAL");
    db.exec("pragma busy_timeout = 1000");
    migrateStatsDatabase(db);
    return db;
  } catch (err) {
    db.close();
    throw err;
  }
}

function migrateStatsDatabase(db: Database): void {
  db.exec(`
    create table if not exists schema_migrations (
      version integer primary key,
      applied_at text not null
    );
  `);

  const currentVersion = currentSchemaVersion(db);
  if (currentVersion >= SCHEMA_VERSION) return;

  if (currentVersion < 1) {
    db.exec(`
      create table if not exists settings (
        key text primary key,
        value text not null
      );

      create table if not exists runs (
        id integer primary key autoincrement,
        command text not null,
        started_at text not null,
        finished_at text,
        status text not null,
        app_version text not null,
        item_count integer not null default 0
      );

      create table if not exists artifacts (
        id integer primary key autoincrement,
        run_id integer not null references runs(id),
        kind text not null,
        format text,
        size_bytes integer,
        duration_ms integer,
        sample_rate integer,
        channels integer
      );

      create table if not exists stage_timings (
        id integer primary key autoincrement,
        run_id integer not null references runs(id),
        stage text not null,
        started_at text not null,
        duration_ms integer not null,
        status text not null
      );

      create table if not exists errors (
        id integer primary key autoincrement,
        run_id integer references runs(id),
        stage text,
        error_class text,
        error_code text,
        sanitized_message text not null,
        occurred_at text not null
      );

      create index if not exists runs_started_at_idx on runs(started_at);
      create index if not exists stage_timings_run_id_idx on stage_timings(run_id);
      create index if not exists artifacts_run_id_idx on artifacts(run_id);
      create index if not exists errors_occurred_at_idx on errors(occurred_at);
    `);

    recordSchemaVersion(db, 1);
  }
}

function currentSchemaVersion(db: Database): number {
  const row = db.query("select coalesce(max(version), 0) as version from schema_migrations").get() as {
    version: number;
  };
  return Number(row.version ?? 0);
}

function recordSchemaVersion(db: Database, version: number): void {
  db.query(
    "insert or ignore into schema_migrations (version, applied_at) values (?, ?)",
  ).run(version, new Date().toISOString());
}

function getStatsEnabled(db: Database): boolean {
  const row = db.query("select value from settings where key = 'enabled'").get() as
    | { value: string }
    | null;
  return row?.value === "1";
}

function setSetting(db: Database, key: string, value: string): void {
  db.query(
    "insert into settings (key, value) values (?, ?) on conflict(key) do update set value = excluded.value",
  ).run(key, value);
}

function insertRun(db: Database, command: StatsCommandName): number {
  applyStatsRetention(db);
  const result = db.query(
    "insert into runs (command, started_at, status, app_version) values (?, ?, 'failed', ?) returning id",
  ).get(command, new Date().toISOString(), APP_VERSION) as { id: number };
  return Number(result.id);
}

function readStatsExport(): StatsExportData {
  const dbPath = resolveStatsDbPath();
  if (!existsSync(dbPath)) {
    return emptyStatsExport(defaultRetentionDays());
  }

  const db = openStatsDatabase(dbPath);
  try {
    applyStatsRetention(db);
    return {
      schemaVersion: SCHEMA_VERSION,
      exportedAt: new Date().toISOString(),
      retentionDays: getStatsRetentionDays(db),
      privacy: statsPrivacyContract(),
      runs: db.query(
        `select
          id,
          command,
          started_at as startedAt,
          finished_at as finishedAt,
          status,
          app_version as appVersion,
          item_count as itemCount
         from runs
         order by started_at asc, id asc`,
      ).all() as StatsRunExportRow[],
      artifacts: db.query(
        `select
          id,
          run_id as runId,
          kind,
          format,
          size_bytes as sizeBytes,
          duration_ms as durationMs,
          sample_rate as sampleRate,
          channels
         from artifacts
         order by id asc`,
      ).all() as StatsArtifactExportRow[],
      stageTimings: db.query(
        `select
          id,
          run_id as runId,
          stage,
          started_at as startedAt,
          duration_ms as durationMs,
          status
         from stage_timings
         order by started_at asc, id asc`,
      ).all() as StatsStageTimingExportRow[],
      errors: db.query(
        `select
          e.occurred_at as occurredAt,
          r.command as command,
          e.stage as stage,
          e.error_class as errorClass,
          e.error_code as errorCode,
          e.sanitized_message as message
         from errors e
         left join runs r on r.id = e.run_id
         order by e.occurred_at asc, e.id asc`,
      ).all() as StatsErrorRow[],
    };
  } finally {
    db.close();
  }
}

function emptyStatsExport(retentionDays: number | null): StatsExportData {
  return {
    schemaVersion: SCHEMA_VERSION,
    exportedAt: new Date().toISOString(),
    retentionDays,
    privacy: statsPrivacyContract(),
    runs: [],
    artifacts: [],
    stageTimings: [],
    errors: [],
  };
}

function statsPrivacyContract(): StatsExportData["privacy"] {
  return {
    contentFree: true,
    neverStored: [
      "audio bytes",
      "transcripts",
      "input text",
      "output text",
      "file names",
      "full file paths",
      "raw stdout",
      "raw stderr",
      "environment variables",
      "model files",
    ],
  };
}

function renderStatsCsv(data: StatsExportData): string {
  const header = [
    "table",
    "id",
    "run_id",
    "command",
    "kind",
    "stage",
    "status",
    "started_at",
    "finished_at",
    "occurred_at",
    "duration_ms",
    "format",
    "size_bytes",
    "sample_rate",
    "channels",
    "error_class",
    "error_code",
    "sanitized_message",
    "app_version",
    "item_count",
  ];
  const rows: Array<Record<string, string | number | null>> = [];

  for (const run of data.runs) {
    rows.push({
      table: "runs",
      id: run.id,
      command: run.command,
      status: run.status,
      started_at: run.startedAt,
      finished_at: run.finishedAt,
      app_version: run.appVersion,
      item_count: run.itemCount,
    });
  }
  for (const artifact of data.artifacts) {
    rows.push({
      table: "artifacts",
      id: artifact.id,
      run_id: artifact.runId,
      kind: artifact.kind,
      format: artifact.format,
      size_bytes: artifact.sizeBytes,
      duration_ms: artifact.durationMs,
      sample_rate: artifact.sampleRate,
      channels: artifact.channels,
    });
  }
  for (const timing of data.stageTimings) {
    rows.push({
      table: "stage_timings",
      id: timing.id,
      run_id: timing.runId,
      stage: timing.stage,
      status: timing.status,
      started_at: timing.startedAt,
      duration_ms: timing.durationMs,
    });
  }
  for (const error of data.errors) {
    rows.push({
      table: "errors",
      command: error.command,
      stage: error.stage,
      occurred_at: error.occurredAt,
      error_class: error.errorClass,
      error_code: error.errorCode,
      sanitized_message: error.message,
    });
  }

  return [
    header.join(","),
    ...rows.map((row) => header.map((name) => csvCell(row[name] ?? null)).join(",")),
  ].join("\n") + "\n";
}

function csvCell(value: string | number | null): string {
  if (value === null) return "";
  const text = String(value);
  if (!/[",\n\r]/.test(text)) return text;
  return `"${text.replace(/"/g, '""')}"`;
}

function getStatsRetentionDays(db: Database): number | null {
  const row = db.query("select value from settings where key = 'retention_days'").get() as
    | { value: string }
    | null;
  if (!row) return defaultRetentionDays();
  return parseRetentionSetting(row.value);
}

function defaultRetentionDays(): number | null {
  const raw = process.env.KESHA_STATS_RETENTION_DAYS;
  if (!raw) return DEFAULT_RETENTION_DAYS;
  return parseRetentionSetting(raw);
}

function parseRetentionSetting(raw: string): number | null {
  const normalized = raw.trim().toLowerCase();
  if (normalized === "off" || normalized === "none" || normalized === "never") return null;
  const days = Number(normalized);
  if (Number.isInteger(days) && days >= 1) return days;
  return DEFAULT_RETENTION_DAYS;
}

function applyStatsRetention(db: Database): void {
  const retentionDays = getStatsRetentionDays(db);
  if (retentionDays === null) return;
  const cutoff = new Date(Date.now() - retentionDays * 24 * 60 * 60 * 1000).toISOString();
  db.exec("begin");
  try {
    db.query("delete from artifacts where run_id in (select id from runs where started_at < ?)").run(cutoff);
    db.query("delete from stage_timings where run_id in (select id from runs where started_at < ?)").run(cutoff);
    db.query("delete from errors where run_id in (select id from runs where started_at < ?)").run(cutoff);
    db.query("delete from errors where run_id is null and occurred_at < ?").run(cutoff);
    db.query("delete from runs where started_at < ?").run(cutoff);
    db.exec("commit");
  } catch (err) {
    db.exec("rollback");
    throw err;
  }
}

function runChanges(db: Database, sql: string): number {
  const result = db.query(sql).run() as { changes?: number };
  return Number(result.changes ?? 0);
}

function fileSize(path: string): number {
  try {
    return statSync(path).size;
  } catch {
    return 0;
  }
}

function normalizeFormat(format?: string | null): string | null {
  if (!format) return null;
  return format.replace(/^\./, "").toLowerCase();
}

function emptyWeekSummary(): StatsWeekSummary {
  return {
    runs: 0,
    successes: 0,
    failures: 0,
    inputFiles: 0,
    inputBytes: 0,
    inputDurationMs: 0,
    sttTimeMs: 0,
    ttsTimeMs: 0,
    stageBreakdown: [],
    inputFormats: [],
    inputSizeBuckets: [],
    inputDurationBuckets: [],
    hasInputDurationData: false,
    slowestRuns: [],
  };
}

function summarizeStages(rows: StatsStageTimingRow[]): StatsStageSummary[] {
  const byStage = new Map<string, StatsStageTimingRow[]>();
  for (const row of rows) {
    const stage = row.stage || "unknown";
    const existing = byStage.get(stage) ?? [];
    existing.push(row);
    byStage.set(stage, existing);
  }

  return [...byStage.entries()]
    .map(([stage, stageRows]) => {
      const durations = stageRows.map((row) => Math.max(0, Number(row.durationMs ?? 0))).sort((a, b) => a - b);
      return {
        stage,
        count: stageRows.length,
        failed: stageRows.filter((row) => row.status === "failed").length,
        totalMs: durations.reduce((sum, ms) => sum + ms, 0),
        p50Ms: percentile(durations, 50),
        p95Ms: percentile(durations, 95),
        p99Ms: percentile(durations, 99),
      };
    })
    .sort((a, b) => b.totalMs - a.totalMs || a.stage.localeCompare(b.stage));
}

function sumStageDuration(rows: StatsStageTimingRow[], stage: string): number {
  return rows
    .filter((row) => row.stage === stage)
    .reduce((sum, row) => sum + Math.max(0, Number(row.durationMs ?? 0)), 0);
}

function countRuns(rows: StatsRunTimingRow[], status: StatsRunStatus): number {
  return rows.filter((row) => row.status === status).length;
}

function sumInputBytes(rows: StatsInputArtifactRow[]): number {
  return rows.reduce((sum, row) => sum + Math.max(0, Number(row.sizeBytes ?? 0)), 0);
}

function sumInputDuration(rows: StatsInputArtifactRow[]): number {
  return rows.reduce((sum, row) => sum + Math.max(0, Number(row.durationMs ?? 0)), 0);
}

function summarizeFormats(rows: StatsInputArtifactRow[]): StatsCountBucket[] {
  const buckets = new Map<string, number>();
  for (const row of rows) {
    const label = row.format || "unknown";
    buckets.set(label, (buckets.get(label) ?? 0) + 1);
  }
  return sortedBuckets(buckets);
}

function summarizeSizeBuckets(rows: StatsInputArtifactRow[]): StatsCountBucket[] {
  const buckets = new Map<string, number>();
  for (const row of rows) {
    const label = sizeBucket(row.sizeBytes);
    buckets.set(label, (buckets.get(label) ?? 0) + 1);
  }
  return orderBuckets(buckets, ["unknown", "<1 MB", "1-10 MB", "10-100 MB", "100 MB+"]);
}

function summarizeDurationBuckets(rows: StatsInputArtifactRow[]): StatsCountBucket[] {
  const buckets = new Map<string, number>();
  for (const row of rows) {
    if (typeof row.durationMs !== "number") continue;
    const label = durationBucket(row.durationMs);
    buckets.set(label, (buckets.get(label) ?? 0) + 1);
  }
  return orderBuckets(buckets, ["<1 min", "1-10 min", "10-60 min", "60 min+"]);
}

function summarizeSlowestRuns(
  runs: StatsRunTimingRow[],
  stageRows: StatsStageTimingRow[],
): StatsSlowRun[] {
  const stagesByRun = new Map<number, StatsSlowRunStage[]>();
  for (const row of stageRows) {
    const existing = stagesByRun.get(row.runId) ?? [];
    existing.push({
      stage: row.stage || "unknown",
      durationMs: Math.max(0, Number(row.durationMs ?? 0)),
      status: row.status,
    });
    stagesByRun.set(row.runId, existing);
  }

  return runs
    .filter((run) => run.finishedAt !== null)
    .map((run) => ({
      run,
      durationMs: elapsedMs(run.startedAt, run.finishedAt),
    }))
    .sort((a, b) => (b.durationMs ?? -1) - (a.durationMs ?? -1))
    .slice(0, 5)
    .map(({ run, durationMs }) => ({
      command: run.command,
      status: run.status,
      durationMs,
      itemCount: Number(run.itemCount ?? 0),
      stages: (stagesByRun.get(run.id) ?? []).sort((a, b) => b.durationMs - a.durationMs || a.stage.localeCompare(b.stage)),
    }));
}

function elapsedMs(startedAt: string, finishedAt: string | null): number | null {
  if (!finishedAt) return null;
  const started = Date.parse(startedAt);
  const finished = Date.parse(finishedAt);
  if (!Number.isFinite(started) || !Number.isFinite(finished)) return null;
  return Math.max(0, finished - started);
}

function percentile(sortedDurations: number[], p: number): number {
  if (sortedDurations.length === 0) return 0;
  const index = Math.min(sortedDurations.length - 1, Math.max(0, Math.ceil((p / 100) * sortedDurations.length) - 1));
  return sortedDurations[index];
}

function sizeBucket(sizeBytes: number | null): string {
  if (typeof sizeBytes !== "number") return "unknown";
  if (sizeBytes < 1024 * 1024) return "<1 MB";
  if (sizeBytes < 10 * 1024 * 1024) return "1-10 MB";
  if (sizeBytes < 100 * 1024 * 1024) return "10-100 MB";
  return "100 MB+";
}

function durationBucket(durationMs: number): string {
  if (durationMs < 60_000) return "<1 min";
  if (durationMs < 10 * 60_000) return "1-10 min";
  if (durationMs < 60 * 60_000) return "10-60 min";
  return "60 min+";
}

function renderTopStages(
  stages: StatsStageSummary[],
  metric: "totalMs" | "p95Ms",
): string {
  return [...stages]
    .sort((a, b) => b[metric] - a[metric] || a.stage.localeCompare(b.stage))
    .slice(0, 3)
    .map((stage) => `${stage.stage} ${humanDuration(stage[metric])}`)
    .join(", ");
}

function renderBuckets(buckets: StatsCountBucket[]): string {
  if (buckets.length === 0) return "n/a";
  return buckets.map((bucket) => `${bucket.label} ${bucket.count}`).join(", ");
}

function sortedBuckets(buckets: Map<string, number>): StatsCountBucket[] {
  return [...buckets.entries()]
    .map(([label, count]) => ({ label, count }))
    .sort((a, b) => b.count - a.count || a.label.localeCompare(b.label));
}

function orderBuckets(buckets: Map<string, number>, order: string[]): StatsCountBucket[] {
  return order
    .filter((label) => buckets.has(label))
    .map((label) => ({ label, count: buckets.get(label) ?? 0 }));
}

function humanBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let n = bytes / 1024;
  let i = 0;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n.toFixed(n >= 100 ? 0 : 1)} ${units[i]}`;
}

function humanDuration(ms: number): string {
  const seconds = Math.round(ms / 1000);
  if (seconds < 1) return `${ms}ms`;
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = seconds % 60;
  if (h > 0) return `${h}h ${m}m ${s}s`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
}

function escapeRegExp(input: string): string {
  return input.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function readPackageVersion(): string {
  return packageVersion;
}
