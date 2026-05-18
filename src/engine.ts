import { existsSync, statSync } from "fs";
import { join } from "path";
import { log } from "./log";
import { defaultEngineBinPath, keshaCacheDir } from "./paths";
import { engineAbortError, registerProcessTree } from "./process-tree";

/**
 * Capability-flag string surfaced via `kesha-engine --capabilities-json`. Single
 * source of truth so the engine, the TS CLI gate, and the integration tests
 * can't drift. Mirrors `rust/src/transcribe/mod.rs::TRANSCRIBE_SEGMENTS_FEATURE`.
 */
export const TRANSCRIBE_SEGMENTS_FEATURE = "transcribe.segments";

/**
 * Capability-flag string for speaker diarization. Engine advertises this only
 * on darwin-arm64 builds with the `system_diarize` cargo feature (#199).
 * Mirrors `rust/src/transcribe/mod.rs::TRANSCRIBE_DIARIZE_FEATURE`.
 */
export const TRANSCRIBE_DIARIZE_FEATURE = "transcribe.diarize";

export interface LangDetectResult {
  code: string;
  confidence: number;
}

export interface TranscriptionSegment {
  start: number;
  end: number;
  text: string;
  /** Speaker cluster id when `--speakers` was requested (#199). */
  speaker?: number;
}

export interface TranscriptionOutput {
  text: string;
  segments: TranscriptionSegment[];
}

/**
 * Path to the `kesha-engine` binary. Defaults to the install location under
 * the Kesha cache directory. The `KESHA_ENGINE_BIN` env var overrides — useful
 * for running against a freshly-built engine during development or in e2e tests.
 */
export function getEngineBinPath(): string {
  return process.env.KESHA_ENGINE_BIN ?? defaultEngineBinPath();
}

export function isEngineInstalled(): boolean {
  return existsSync(getEngineBinPath());
}

/** A Bun.spawn `stdio` array entry: per-fd action or inherit-by-number. */
type SpawnStdioEntry = "inherit" | "pipe" | "ignore" | number;

interface RunEngineOptions {
  signal?: AbortSignal;
}

/**
 * Upper bound on the fd number we'll forward (#323 Greptile P2).
 *
 * The `stdio` array is index-addressed, so `KESHA_DEBUG_FD=1000000` would
 * allocate a million-entry array of `"ignore"` strings before the spawn.
 * 1024 is the conservative POSIX `RLIMIT_NOFILE` default — anything
 * above it can't be open in the parent anyway, so capping is a no-op
 * for legitimate users and a DoS guard for bogus input.
 */
const MAX_FORWARDED_FD = 1024;

/**
 * Build a `stdio` array for `Bun.spawn`, forwarding `KESHA_DEBUG_FD` (#321 F19).
 *
 * The engine's NDJSON debug sink looks for `KESHA_DEBUG_FD=N` and writes
 * structured events to fd `N`. Bun.spawn closes all non-stdio fds in the
 * child by default, so a bare `KESHA_DEBUG_FD=3 kesha ...` would propagate
 * the env var but the kernel-level fd 3 wouldn't reach the engine — the
 * sink would silently no-op.
 *
 * This helper forwards the parent's fd N to the same number in the child
 * by extending the stdio array with `"ignore"` padding up to index N and
 * setting `stdio[N] = N` (Bun's "inherit parent fd identity" form).
 *
 * Returns `base` unchanged when:
 *   - `KESHA_DEBUG_FD` is unset / empty.
 *   - The value isn't a non-negative integer.
 *   - The value is 0/1/2 (covered by base stdin/stdout/stderr entries).
 *
 * Exported so `synth.ts` (the `kesha say` spawn site) can share the
 * forwarding logic without duplicating the env-parse code.
 */
export function spawnStdioWithDebugFd(
  base: [SpawnStdioEntry, SpawnStdioEntry, SpawnStdioEntry],
): [SpawnStdioEntry, SpawnStdioEntry, SpawnStdioEntry, ...SpawnStdioEntry[]] {
  const envFd = process.env.KESHA_DEBUG_FD;
  if (!envFd) return base;
  const fd = Number(envFd);
  if (!Number.isInteger(fd) || fd < 3 || fd > MAX_FORWARDED_FD) return base;
  const out: SpawnStdioEntry[] = [...base];
  while (out.length < fd) out.push("ignore");
  out[fd] = fd;
  return out as [SpawnStdioEntry, SpawnStdioEntry, SpawnStdioEntry, ...SpawnStdioEntry[]];
}

export interface RunEngineOptions {
  signal?: AbortSignal;
}

async function runEngine(
  args: string[],
  opts: RunEngineOptions = {},
): Promise<{ stdout: string; stderr: string; exitCode: number }> {
  if (opts.signal?.aborted) throw engineAbortError();
  const binPath = getEngineBinPath();
  const startedAt = performance.now();
  log.debug(`spawn ${binPath} ${args.join(" ")}`);
  const proc = Bun.spawn([binPath, ...args], {
    detached: true,
    stdio: spawnStdioWithDebugFd(["ignore", "pipe", "pipe"]),
  });
  const tree = registerProcessTree(proc);
  let aborted = false;
  let forceKillTimer: Timer | undefined;
  const abort = () => {
    aborted = true;
    tree.terminate("SIGTERM");
    forceKillTimer ??= tree.forceKillAfterGrace();
  };
  opts.signal?.addEventListener("abort", abort, { once: true });
  // `stdio: [...]` widens stdout/stderr into a union; indices 1/2 are
  // pinned to "pipe" by the helper, so the narrow ReadableStream type
  // is correct. Cast to drop the spurious `number` arm.
  const stdoutStream = proc.stdout as ReadableStream<Uint8Array>;
  const stderrStream = proc.stderr as ReadableStream<Uint8Array>;

  let stdout: string;
  let stderr: string;
  let exitCode: number;
  try {
    [stdout, stderr, exitCode] = await Promise.all([
      new Response(stdoutStream).text(),
      new Response(stderrStream).text(),
      proc.exited,
    ]);
  } finally {
    opts.signal?.removeEventListener("abort", abort);
    tree.dispose();
    if (!aborted && forceKillTimer) clearTimeout(forceKillTimer);
  }

  log.debug(`exit=${exitCode} dt=${Math.round(performance.now() - startedAt)}ms args=${JSON.stringify(args)}`);
  if (aborted) {
    log.debug(`aborted args=${JSON.stringify(args)}`);
    throw engineAbortError();
  }

  // #275 D4: surface engine stderr on the success path so warnings like
  // `hint: audio is 180s`, `Model mirror active:`, and the dtrace lines
  // emitted under KESHA_DEBUG=1 reach the user. On non-zero exit we leave
  // the buffer for callers to fold into a thrown Error — otherwise the
  // user would see the warning AND a duplicate inside the error message.
  if (exitCode === 0 && stderr.length > 0) {
    process.stderr.write(stderr.endsWith("\n") ? stderr : stderr + "\n");
  }
  return { stdout: stdout.trim(), stderr: stderr.trim(), exitCode };
}

/** VAD preprocessing selector.
 *  - `"auto"` (default): engine decides — VAD when audio ≥ 120 s and model installed
 *  - `"on"`: force VAD (requires `kesha install --vad`)
 *  - `"off"`: force full-file pass regardless of duration or install state
 */
export type VadMode = "auto" | "on" | "off";

export interface TranscribeEngineOptions {
  vad?: VadMode;
  signal?: AbortSignal;
  /** Request speaker labels in transcript segments. Requires the engine to
   * advertise `transcribe.diarize` (darwin-arm64 only — see #199). */
  speakers?: boolean;
}

function defaultDiarizeModelPath(): string {
  return join(keshaCacheDir(), "models", "diarize", "SortformerNvidiaLow_v2.mlpackage");
}

function hasDiarizeModelLayout(modelPath: string): boolean {
  return (
    existsSync(join(modelPath, "Manifest.json")) &&
    existsSync(join(modelPath, "Data", "com.apple.CoreML", "model.mlmodel")) &&
    existsSync(join(modelPath, "Data", "com.apple.CoreML", "weights", "0-weight.bin")) &&
    existsSync(join(modelPath, "Data", "com.apple.CoreML", "weights", "1-weight.bin"))
  );
}

export async function preflightTranscribeEngineWithSegments(
  opts: TranscribeEngineOptions = {},
): Promise<void> {
  const caps = await getEngineCapabilities();
  if (!caps?.features.includes(TRANSCRIBE_SEGMENTS_FEATURE)) {
    throw new Error(
      "Timestamped segments require a newer kesha-engine. Run `kesha install` after upgrading Kesha Voice Kit.",
    );
  }

  if (!opts.speakers) return;

  if (!caps.features.includes(TRANSCRIBE_DIARIZE_FEATURE)) {
    throw new Error(
      "speaker diarization is currently darwin-arm64 only " +
        "(see https://github.com/drakulavich/kesha-voice-kit/issues/199)",
    );
  }

  const envPath = process.env.KESHA_DIARIZE_MODEL_PATH;
  if (envPath !== undefined) {
    if (existsSync(envPath)) return;
    throw new Error(
      `speaker diarization requires a model path\n\nCaused by:\n    KESHA_DIARIZE_MODEL_PATH set but path does not exist: ${envPath}`,
    );
  }

  const modelPath = defaultDiarizeModelPath();
  if (hasDiarizeModelLayout(modelPath)) return;
  throw new Error(
    `speaker diarization requires a model path\n\nCaused by:\n    diarization model not found at ${modelPath}. ` +
      "Run `kesha install --diarize` (or set KESHA_DIARIZE_MODEL_PATH).",
  );
}

export async function transcribeEngine(
  audioPath: string,
  opts: TranscribeEngineOptions = {},
): Promise<string> {
  const args = ["transcribe", audioPath];
  if (opts.vad === "on") args.push("--vad");
  else if (opts.vad === "off") args.push("--no-vad");
  const { stdout, stderr, exitCode } = await runEngine(args, { signal: opts.signal });
  if (exitCode !== 0) {
    throw new Error(stderr || `kesha-engine exited with code ${exitCode}`);
  }
  return stdout;
}

function parseTranscriptionOutput(stdout: string): TranscriptionOutput {
  const parsed = JSON.parse(stdout);
  if (typeof parsed?.text !== "string" || !Array.isArray(parsed?.segments)) {
    throw new Error("Invalid transcription JSON returned by kesha-engine");
  }

  const segments = parsed.segments.map((segment: unknown) => {
    const s = segment as Record<string, unknown>;
    if (
      typeof s.start !== "number" ||
      typeof s.end !== "number" ||
      typeof s.text !== "string"
    ) {
      throw new Error("Invalid transcription segment returned by kesha-engine");
    }
    const out: TranscriptionSegment = { start: s.start, end: s.end, text: s.text };
    if (typeof s.speaker === "number") out.speaker = s.speaker;
    return out;
  });

  return { text: parsed.text, segments };
}

export async function transcribeEngineWithSegments(
  audioPath: string,
  opts: TranscribeEngineOptions = {},
): Promise<TranscriptionOutput> {
  await preflightTranscribeEngineWithSegments(opts);

  const args = ["transcribe", audioPath, "--json"];
  if (opts.vad === "on") args.push("--vad");
  else if (opts.vad === "off") args.push("--no-vad");
  if (opts.speakers) {
    args.push("--speakers");
  }
  const { stdout, stderr, exitCode } = await runEngine(args, { signal: opts.signal });
  if (exitCode !== 0) {
    throw new Error(stderr || `kesha-engine exited with code ${exitCode}`);
  }
  try {
    return parseTranscriptionOutput(stdout);
  } catch (err: unknown) {
    const message = err instanceof Error ? err.message : String(err);
    throw new Error(`${message}: ${stdout}`);
  }
}

export async function recordEngine(outPath: string, maxSeconds: number): Promise<void> {
  const binPath = getEngineBinPath();
  const args = ["record", "--out", outPath, "--max-seconds", String(maxSeconds)];
  const startedAt = performance.now();
  log.debug(`spawn ${binPath} ${args.join(" ")}`);
  const proc = Bun.spawn([binPath, ...args], {
    detached: true,
    stdio: spawnStdioWithDebugFd(["inherit", "inherit", "inherit"]),
  });
  const tree = registerProcessTree(proc);
  let exitCode: number;
  try {
    exitCode = await proc.exited;
  } finally {
    tree.dispose();
  }
  log.debug(`exit=${exitCode} dt=${Math.round(performance.now() - startedAt)}ms args=${JSON.stringify(args)}`);
  if (exitCode !== 0) {
    throw new Error(`kesha-engine record exited with code ${exitCode}`);
  }
}

export function parseLangResult(stdout: string): LangDetectResult | null {
  try {
    const parsed = JSON.parse(stdout);
    if (typeof parsed.code !== "string" || typeof parsed.confidence !== "number") {
      return null;
    }
    return { code: parsed.code, confidence: parsed.confidence };
  } catch {
    return null;
  }
}

export async function detectAudioLanguageEngine(
  audioPath: string,
  opts: RunEngineOptions = {},
): Promise<LangDetectResult | null> {
  if (!isEngineInstalled()) return null;
  const { stdout, exitCode } = await runEngine(["detect-lang", audioPath], opts);
  if (exitCode !== 0) return null;
  return parseLangResult(stdout);
}

export async function detectTextLanguageEngine(
  text: string,
  opts: RunEngineOptions = {},
): Promise<LangDetectResult | null> {
  if (text.trim().length === 0) return null;
  if (!isEngineInstalled()) return null;
  const { stdout, exitCode } = await runEngine(["detect-text-lang", text], opts);
  if (exitCode !== 0) return null;
  return parseLangResult(stdout);
}

export interface EngineCapabilities {
  protocolVersion: number;
  backend: string;
  features: string[];
}

let cachedEngineCapabilities:
  | { binPath: string; mtime: number; capabilities: EngineCapabilities }
  | null = null;

export async function getEngineCapabilities(): Promise<EngineCapabilities | null> {
  const binPath = getEngineBinPath();
  // Cache key includes `mtimeMs` so the cache invalidates when `kesha
  // install` overwrites the binary in-place within a single long-lived
  // process (#248). `statSync` throws on missing-file; the catch returns
  // `null` — same effect as the previous explicit `isEngineInstalled()`
  // pre-flight, one fewer redundant fs call.
  let mtime: number;
  try {
    mtime = statSync(binPath).mtimeMs;
  } catch {
    return null;
  }
  if (
    cachedEngineCapabilities?.binPath === binPath &&
    cachedEngineCapabilities.mtime === mtime
  ) {
    return cachedEngineCapabilities.capabilities;
  }
  const { stdout, exitCode } = await runEngine(["--capabilities-json"]);
  if (exitCode !== 0) return null;
  try {
    const capabilities = JSON.parse(stdout) as EngineCapabilities;
    cachedEngineCapabilities = { binPath, mtime, capabilities };
    return capabilities;
  } catch {
    return null;
  }
}
