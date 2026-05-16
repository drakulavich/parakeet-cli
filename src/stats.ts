import { Database } from "bun:sqlite";
import { existsSync, mkdirSync, readFileSync, statSync } from "fs";
import { homedir } from "os";
import { dirname, extname, join } from "path";
import { log } from "./log";

const SCHEMA_VERSION = 1;
const MAX_ERROR_MESSAGE_CHARS = 300;
const APP_VERSION = readPackageVersion();

export type StatsCommandName = "transcribe" | "say";
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
}

export function getStatsStatus(): StatsStatus {
  const dbPath = resolveStatsDbPath();
  if (!existsSync(dbPath)) {
    return { enabled: false, dbPath, runCount: 0, exists: false };
  }
  const db = openStatsDatabase(dbPath);
  try {
    return {
      enabled: getStatsEnabled(db),
      dbPath,
      runCount: Number((db.query("select count(*) as n from runs").get() as { n: number }).n),
      exists: true,
    };
  } finally {
    db.close();
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
}

export function getWeekSummary(now = new Date()): StatsWeekSummary {
  const dbPath = resolveStatsDbPath();
  if (!existsSync(dbPath)) return emptyWeekSummary();
  const since = new Date(now.getTime() - 7 * 24 * 60 * 60 * 1000).toISOString();
  const db = openStatsDatabase(dbPath);
  try {
    const runs = db.query(
      `select
        count(*) as runs,
        sum(case when status = 'success' then 1 else 0 end) as successes,
        sum(case when status = 'failed' then 1 else 0 end) as failures
       from runs
       where started_at >= ?`,
    ).get(since) as { runs: number; successes: number | null; failures: number | null };

    const artifacts = db.query(
      `select
        count(*) as inputFiles,
        coalesce(sum(size_bytes), 0) as inputBytes,
        coalesce(sum(duration_ms), 0) as inputDurationMs
       from artifacts
       where kind = 'input_audio'
         and run_id in (select id from runs where started_at >= ?)`,
    ).get(since) as { inputFiles: number; inputBytes: number; inputDurationMs: number };

    const stages = db.query(
      `select
        coalesce(sum(case when stage = 'transcribe' then duration_ms else 0 end), 0) as sttTimeMs,
        coalesce(sum(case when stage = 'tts' then duration_ms else 0 end), 0) as ttsTimeMs
       from stage_timings
       where run_id in (select id from runs where started_at >= ?)`,
    ).get(since) as { sttTimeMs: number; ttsTimeMs: number };

    return {
      runs: Number(runs.runs ?? 0),
      successes: Number(runs.successes ?? 0),
      failures: Number(runs.failures ?? 0),
      inputFiles: Number(artifacts.inputFiles ?? 0),
      inputBytes: Number(artifacts.inputBytes ?? 0),
      inputDurationMs: Number(artifacts.inputDurationMs ?? 0),
      sttTimeMs: Number(stages.sttTimeMs ?? 0),
      ttsTimeMs: Number(stages.ttsTimeMs ?? 0),
    };
  } finally {
    db.close();
  }
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
  return [
    "Kesha Stats - last 7 days",
    `Runs: ${summary.runs} (${summary.successes} success, ${summary.failures} failed)`,
    `Input files: ${summary.inputFiles}`,
    `Input audio: ${humanDuration(summary.inputDurationMs)}`,
    `Input size: ${humanBytes(summary.inputBytes)}`,
    `STT time: ${humanDuration(summary.sttTimeMs)} (${realtime} realtime)`,
    `TTS time: ${humanDuration(summary.ttsTimeMs)}`,
  ].join("\n");
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
  const result = db.query(
    "insert into runs (command, started_at, status, app_version) values (?, ?, 'failed', ?) returning id",
  ).get(command, new Date().toISOString(), APP_VERSION) as { id: number };
  return Number(result.id);
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
  };
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
  try {
    const pkg = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8")) as {
      version?: unknown;
    };
    return typeof pkg.version === "string" ? pkg.version : "unknown";
  } catch {
    return "unknown";
  }
}
