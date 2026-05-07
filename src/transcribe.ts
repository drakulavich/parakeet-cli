import {
  isEngineInstalled,
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
}

export async function transcribe(audioPath: string, opts: TranscribeOptions = {}): Promise<string> {
  return (await transcribeWithSegments(audioPath, opts)).text;
}

export async function transcribeWithSegments(
  audioPath: string,
  opts: TranscribeOptions = {},
): Promise<TranscriptionOutput> {
  if (!isEngineInstalled()) {
    throw new Error(
      "Error: No transcription backend is installed\n\n" +
      "╔══════════════════════════════════════════════════════════╗\n" +
      "║ Please run the following command to get started:         ║\n" +
      "║                                                          ║\n" +
      "║     bunx @drakulavich/kesha-voice-kit install               ║\n" +
      "╚══════════════════════════════════════════════════════════╝",
    );
  }

  if (opts.timestamps) {
    return transcribeEngineWithSegments(audioPath, { vad: opts.vad });
  }

  const text = await transcribeEngine(audioPath, { vad: opts.vad });
  return { text, segments: [] };
}
