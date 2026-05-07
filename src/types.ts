import type { LangDetectResult, TranscriptionSegment } from "./engine";

/**
 * One row of `kesha --json` / `kesha --toon` output. Canonical output shape
 * for programmatic callers of `transcribe(...)` + `toToon(...)` in the
 * public API (`@drakulavich/kesha-voice-kit/core`).
 */
export type TranscribeResult = {
  file: string;
  text: string;
  lang: string;
  audioLanguage?: LangDetectResult;
  textLanguage?: LangDetectResult;
  /** Timestamped transcript segments when requested via `--timestamps`. */
  segments?: TranscriptionSegment[];
  /** Wall-clock time around the engine subprocess calls for this file, ms. See #139. */
  sttTimeMs?: number;
};

export type { LangDetectResult, TranscriptionSegment };
