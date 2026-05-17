import { existsSync } from "fs";
import {
  transcribe as internalTranscribe,
  transcribeWithSegments as internalTranscribeWithSegments,
  type TranscribeOptions,
} from "./transcribe";
import { downloadEngine } from "./engine-install";

export type { TranscribeOptions };
export type { TranscriptionOutput, TranscriptionSegment } from "./engine";
export { downloadEngine as downloadModel };
export { say, type SayOptions, SayError } from "./synth";

/**
 * Encode a `TranscribeResult[]` as TOON (#138). Same data shape as the
 * `--json` / `--toon` CLI output; the CLI reads from stdin of a transcribe
 * run, this helper is for programmatic callers that already have the array.
 */
export { formatToonOutput as toToon } from "./toon";

/**
 * Output shape returned by `kesha --json` and the input shape expected by
 * `toToon`. Lives in `./types` (since #179) so the public API stops
 * reaching into the CLI-layer file.
 */
export type {
  TranscribeErrorRecord,
  TranscribeJsonOutput,
  TranscribeResult,
} from "./types";

/** Install Kokoro TTS models. Shorthand for `downloadModel({ tts: true })`. */
export async function downloadTts(noCache = false): Promise<void> {
  await downloadEngine(noCache, undefined, { tts: true });
}

/** @deprecated Use `downloadModel` instead. */
export const downloadCoreML = downloadEngine;

export async function transcribe(
  audioPath: string,
  options: TranscribeOptions = {},
): Promise<string> {
  if (!existsSync(audioPath)) {
    throw new Error(`File not found: ${audioPath}`);
  }

  return internalTranscribe(audioPath, { ...options, silent: true });
}

export async function transcribeWithTimestamps(
  audioPath: string,
  options: TranscribeOptions = {},
) {
  if (!existsSync(audioPath)) {
    throw new Error(`File not found: ${audioPath}`);
  }

  return internalTranscribeWithSegments(audioPath, {
    ...options,
    timestamps: true,
    silent: true,
  });
}

/**
 * @deprecated Renamed to {@link transcribeWithTimestamps} (#248). The old
 * name shipped briefly in v1.9.0; this alias keeps existing imports working.
 * No removal is scheduled before the next major version.
 */
export const transcribeWithSegments = transcribeWithTimestamps;
