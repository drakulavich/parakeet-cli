import { join } from "path";
import { homedir } from "os";
import { existsSync, statSync } from "fs";
import { log } from "./log";

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

const DEFAULT_ENGINE_BIN_PATH = join(homedir(), ".cache", "kesha", "engine", "bin", "kesha-engine");

/**
 * Path to the `kesha-engine` binary. Defaults to the install location under
 * `~/.cache/kesha/engine/bin/`. The `KESHA_ENGINE_BIN` env var overrides — useful
 * for running against a freshly-built engine during development or in e2e tests.
 */
export function getEngineBinPath(): string {
  return process.env.KESHA_ENGINE_BIN ?? DEFAULT_ENGINE_BIN_PATH;
}

export function isEngineInstalled(): boolean {
  return existsSync(getEngineBinPath());
}

async function runEngine(args: string[]): Promise<{ stdout: string; stderr: string; exitCode: number }> {
  const binPath = getEngineBinPath();
  const startedAt = performance.now();
  log.debug(`spawn ${binPath} ${args.join(" ")}`);
  const proc = Bun.spawn([binPath, ...args], {
    stdout: "pipe",
    stderr: "pipe",
  });

  const [stdout, stderr, exitCode] = await Promise.all([
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
    proc.exited,
  ]);

  log.debug(`exit=${exitCode} dt=${Math.round(performance.now() - startedAt)}ms args=${JSON.stringify(args)}`);

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
  /** Request speaker labels in transcript segments. Requires the engine to
   * advertise `transcribe.diarize` (darwin-arm64 only — see #199). */
  speakers?: boolean;
}

export async function transcribeEngine(
  audioPath: string,
  opts: TranscribeEngineOptions = {},
): Promise<string> {
  const args = ["transcribe", audioPath];
  if (opts.vad === "on") args.push("--vad");
  else if (opts.vad === "off") args.push("--no-vad");
  const { stdout, stderr, exitCode } = await runEngine(args);
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
  const caps = await getEngineCapabilities();
  if (!caps?.features.includes(TRANSCRIBE_SEGMENTS_FEATURE)) {
    throw new Error(
      "Timestamped segments require a newer kesha-engine. Run `kesha install` after upgrading Kesha Voice Kit.",
    );
  }

  const args = ["transcribe", audioPath, "--json"];
  if (opts.vad === "on") args.push("--vad");
  else if (opts.vad === "off") args.push("--no-vad");
  if (opts.speakers) {
    if (!caps.features.includes(TRANSCRIBE_DIARIZE_FEATURE)) {
      throw new Error(
        "speaker diarization is currently darwin-arm64 only " +
          "(see https://github.com/drakulavich/kesha-voice-kit/issues/199)",
      );
    }
    args.push("--speakers");
  }
  const { stdout, stderr, exitCode } = await runEngine(args);
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

export async function detectAudioLanguageEngine(audioPath: string): Promise<LangDetectResult | null> {
  if (!isEngineInstalled()) return null;
  const { stdout, exitCode } = await runEngine(["detect-lang", audioPath]);
  if (exitCode !== 0) return null;
  return parseLangResult(stdout);
}

export async function detectTextLanguageEngine(text: string): Promise<LangDetectResult | null> {
  if (!isEngineInstalled()) return null;
  const { stdout, exitCode } = await runEngine(["detect-text-lang", text]);
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
