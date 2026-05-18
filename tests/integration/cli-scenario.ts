import { existsSync, readFileSync, statSync } from "fs";
import { delimiter, dirname } from "path";

const DEFAULT_CWD = import.meta.dir + "/../..";
const DEFAULT_TIMEOUT_MS = 4_000;
const FORCE_KILL_GRACE_MS = 1_000;

export interface CliScenarioArtifactRequest {
  name?: string;
  path: string;
  text?: boolean;
}

export interface CliScenarioArtifact {
  name: string;
  path: string;
  exists: boolean;
  sizeBytes?: number;
  text?: string;
}

export interface CliScenarioEnvDiff {
  added: string[];
  changed: string[];
  overrides: Record<string, string>;
}

export interface CliScenarioResult {
  args: string[];
  command: string;
  stdout: string;
  stderr: string;
  exitCode: number;
  elapsedMs: number;
  timedOut: boolean;
  envDiff: CliScenarioEnvDiff;
  artifacts: CliScenarioArtifact[];
}

export interface CliScenarioOptions {
  cwd?: string;
  env?: Record<string, string>;
  timeoutMs?: number;
  stripAnsi?: boolean;
  trimOutput?: boolean;
  artifacts?: Array<string | CliScenarioArtifactRequest>;
  maxArtifactBytes?: number;
}

export async function runCliScenario(
  args: string[],
  opts: CliScenarioOptions = {},
): Promise<CliScenarioResult> {
  const timeoutMs = opts.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  const startedAt = performance.now();
  const envOverrides = {
    NO_COLOR: "1",
    FORCE_COLOR: "0",
    ...(opts.env ?? {}),
  };
  const proc = Bun.spawn([process.execPath, "run", "src/cli.ts", ...args], {
    stdout: "pipe",
    stderr: "pipe",
    cwd: opts.cwd ?? DEFAULT_CWD,
    env: {
      ...process.env,
      PATH: [dirname(process.execPath), process.env.PATH].filter(Boolean).join(delimiter),
      ...envOverrides,
    },
  });

  const stdoutPromise = new Response(proc.stdout).text();
  const stderrPromise = new Response(proc.stderr).text();
  let timeout: Timer | undefined;
  let forceKill: Timer | undefined;
  const timeoutPromise = new Promise<"timeout">((resolve) => {
    timeout = setTimeout(() => resolve("timeout"), timeoutMs);
  });
  const exitOrTimeout = await Promise.race([proc.exited, timeoutPromise]);
  if (timeout) clearTimeout(timeout);

  if (exitOrTimeout === "timeout") {
    proc.kill("SIGTERM");
    forceKill = setTimeout(() => proc.kill("SIGKILL"), FORCE_KILL_GRACE_MS);
  }

  const [rawStdout, rawStderr, exitCode] = await Promise.all([
    stdoutPromise,
    stderrPromise,
    proc.exited,
  ]);
  if (forceKill) clearTimeout(forceKill);

  const elapsedMs = Math.round(performance.now() - startedAt);
  const result: CliScenarioResult = {
    args,
    command: ["kesha", ...args].join(" "),
    stdout: normalizeOutput(rawStdout, opts),
    stderr: normalizeOutput(rawStderr, opts),
    exitCode,
    elapsedMs,
    timedOut: exitOrTimeout === "timeout",
    envDiff: diffEnv(envOverrides),
    artifacts: captureArtifacts(opts.artifacts ?? [], opts.maxArtifactBytes ?? 64_000),
  };

  if (result.timedOut) {
    throw new CliScenarioTimeoutError(result, timeoutMs);
  }

  return result;
}

export class CliScenarioTimeoutError extends Error {
  constructor(
    readonly result: CliScenarioResult,
    timeoutMs: number,
  ) {
    super(formatTimeout(result, timeoutMs));
    this.name = "CliScenarioTimeoutError";
  }
}

function normalizeOutput(value: string, opts: CliScenarioOptions): string {
  const stripped = opts.stripAnsi === false ? value : stripAnsi(value);
  return opts.trimOutput === false ? stripped : stripped.trim();
}

function stripAnsi(value: string): string {
  return value.replace(
    /[\u001B\u009B][[\]()#;?]*(?:(?:(?:[a-zA-Z\d]*(?:;[a-zA-Z\d]*)*)?\u0007)|(?:(?:\d{1,4}(?:;\d{0,4})*)?[\dA-PR-TZcf-nq-uy=><~]))/g,
    "",
  );
}

function diffEnv(overrides: Record<string, string>): CliScenarioEnvDiff {
  const added: string[] = [];
  const changed: string[] = [];
  for (const [key, value] of Object.entries(overrides)) {
    if (!(key in process.env)) {
      added.push(key);
    } else if (process.env[key] !== value) {
      changed.push(key);
    }
  }
  return {
    added: added.sort(),
    changed: changed.sort(),
    overrides,
  };
}

function captureArtifacts(
  artifacts: Array<string | CliScenarioArtifactRequest>,
  maxArtifactBytes: number,
): CliScenarioArtifact[] {
  return artifacts.map((entry) => {
    const request = typeof entry === "string" ? { path: entry } : entry;
    const artifact: CliScenarioArtifact = {
      name: request.name ?? request.path,
      path: request.path,
      exists: existsSync(request.path),
    };
    if (!artifact.exists) {
      return artifact;
    }

    const stats = statSync(request.path);
    artifact.sizeBytes = stats.size;
    if (request.text && stats.size <= maxArtifactBytes) {
      artifact.text = readFileSync(request.path, "utf8");
    }
    return artifact;
  });
}

function formatTimeout(result: CliScenarioResult, timeoutMs: number): string {
  return [
    `CLI timed out after ${timeoutMs}ms: ${result.command}`,
    `elapsedMs=${result.elapsedMs}`,
    `exitCode=${result.exitCode}`,
    `envAdded=${result.envDiff.added.join(",")}`,
    `envChanged=${result.envDiff.changed.join(",")}`,
    `stdout=${result.stdout}`,
    `stderr=${result.stderr}`,
  ].join("\n");
}
