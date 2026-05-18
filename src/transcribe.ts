import {
  isEngineInstalled,
  preflightTranscribeEngineWithSegments,
  transcribeEngine,
  transcribeEngineWithSegments,
  type TranscriptionOutput,
  type VadMode,
} from "./engine";

export type { VadMode };
export type { TranscriptionOutput };

export interface TranscribeOptions {
  silent?: boolean;
  /** Silero VAD preprocessing selector. Defaults to `"auto"`. */
  vad?: VadMode;
  /** Request timestamped transcript segments from the engine. */
  timestamps?: boolean;
  /** Request speaker labels in transcript segments (#199). Implies `timestamps`.
   * Currently darwin-arm64 only — throws when the engine doesn't advertise
   * `transcribe.diarize`. */
  speakers?: boolean;
}

export async function transcribe(audioPath: string, opts: TranscribeOptions = {}): Promise<string> {
  return (await transcribeWithSegments(audioPath, opts)).text;
}

export async function preflightTranscribeWithSegments(opts: TranscribeOptions = {}): Promise<void> {
  if (!isEngineInstalled()) {
    throw new Error(
      "Error: No transcription backend is installed\n\n" +
      "╔══════════════════════════════════════════════════════════╗\n" +
      "║ Please run the following commands to get started:        ║\n" +
      "║                                                          ║\n" +
      "║     bun add -g @drakulavich/kesha-voice-kit              ║\n" +
      "║     kesha install                                        ║\n" +
      "╚══════════════════════════════════════════════════════════╝",
    );
  }

  if (opts.timestamps || opts.speakers) {
    await preflightTranscribeEngineWithSegments({
      vad: opts.vad,
      speakers: opts.speakers,
    });
  }
}

export async function transcribeWithSegments(
  audioPath: string,
  opts: TranscribeOptions = {},
): Promise<TranscriptionOutput> {
  await preflightTranscribeWithSegments(opts);

  if (opts.timestamps || opts.speakers) {
    return transcribeEngineWithSegments(audioPath, {
      vad: opts.vad,
      speakers: opts.speakers,
    });
  }

  const text = await transcribeEngine(audioPath, { vad: opts.vad });
  return { text, segments: [] };
}
