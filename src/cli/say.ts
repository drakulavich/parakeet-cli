import { defineCommand } from "citty";
import { detectTextLanguageEngine, getEngineBinPath } from "../engine";
import { log } from "../log";
import { say, SayError, type SayFormat } from "../synth";
import { artifactFromBytes, artifactFromFile, createStatsRecorder } from "../stats";
import { pickVoiceForLang } from "../voice-routing";

/** Run NLLanguageRecognizer (via engine) on the text and pick a default voice. */
async function autoRouteVoice(text: string): Promise<string | undefined> {
  if (!text) return undefined;
  const detected = await detectTextLanguageEngine(text);
  return pickVoiceForLang(detected?.code, detected?.confidence ?? 0);
}

/** Resolve the text to synthesize: inline positional, else read from stdin. */
async function resolveText(inline: string | undefined): Promise<string> {
  if (inline !== undefined && inline.length > 0) return inline;
  const chunks: Uint8Array[] = [];
  for await (const chunk of Bun.stdin.stream()) {
    chunks.push(chunk);
  }
  const total = chunks.reduce((n, c) => n + c.byteLength, 0);
  const merged = new Uint8Array(total);
  let offset = 0;
  for (const c of chunks) {
    merged.set(c, offset);
    offset += c.byteLength;
  }
  return new TextDecoder().decode(merged).trim();
}

export const sayCommand = defineCommand({
  meta: {
    name: "say",
    description:
      "Synthesize speech from text (TTS). Writes audio to stdout (or --out file). Defaults to WAV; use --format ogg-opus for messenger-ready voice notes.",
  },
  args: {
    text: { type: "positional", required: false, description: "Text to speak (stdin if omitted)" },
    voice: { type: "string", description: "Voice id, e.g. en-am_michael" },
    lang: { type: "string", description: "BCP 47 language code (default en-us)" },
    out: { type: "string", description: "Write audio to file instead of stdout" },
    rate: { type: "string", description: "Speaking rate 0.5–2.0", default: "1.0" },
    "list-voices": { type: "boolean", description: "List installed voices and exit" },
    ssml: {
      type: "boolean",
      description: "Parse input as SSML (supports <speak>, <break>; strips unknown tags)",
    },
    format: {
      type: "string",
      description:
        "Output format: wav (default) or ogg-opus (Telegram-ready voice note). Inferred from --out extension when omitted.",
    },
    bitrate: {
      type: "string",
      description: "Opus bitrate in bits/sec (e.g. 32000). Only with --format ogg-opus.",
    },
    "sample-rate": {
      type: "string",
      description:
        "Opus encoder sample rate (8000/12000/16000/24000/48000). Only with --format ogg-opus.",
    },
    "no-expand-abbrev": {
      type: "boolean",
      description:
        "Disable Russian acronym auto-expansion (ВОЗ → 'вэ о зэ') for ru-vosk-* voices. " +
        "<say-as interpret-as='characters'> still works. Applies to Russian (ru-vosk-*) and English (en-*) voices.",
    },
    verbose: {
      type: "boolean",
      description: "Log TTS synthesis time to stderr",
      default: false,
    },
    debug: {
      type: "boolean",
      description: "Trace engine subprocess calls on stderr (or KESHA_DEBUG=1)",
      default: false,
    },
  },
  async run({ args }) {
    if (args.debug) log.debugEnabled = true;
    if (args["list-voices"]) {
      // The engine prints the list directly — just relay its stdout + exit code.
      const proc = Bun.spawn([getEngineBinPath(), "say", "--list-voices"], {
        stdout: "inherit",
        stderr: "inherit",
      });
      process.exit(await proc.exited);
    }

    const inlineText = typeof args.text === "string" ? args.text : undefined;
    const text = await resolveText(inlineText);
    const explicitVoice = typeof args.voice === "string" ? args.voice : undefined;
    const voice = explicitVoice ?? (await autoRouteVoice(text));

    // Validate --format up front so we surface a clear error before spawning
    // the engine subprocess. The engine repeats the check authoritatively, but
    // catching it here gives the user a faster failure mode in scripts.
    const fmtArg = typeof args.format === "string" ? args.format.toLowerCase() : undefined;
    let format: SayFormat | undefined;
    if (fmtArg) {
      if (fmtArg === "wav" || fmtArg === "ogg-opus") {
        format = fmtArg;
      } else if (fmtArg === "opus" || fmtArg === "ogg") {
        format = "ogg-opus";
      } else {
        log.error(`unknown --format '${args.format}'. supported: wav, ogg-opus`);
        process.exit(2);
      }
    }

    // Reject --bitrate / --sample-rate with WAV up front to surface the error fast.
    const hasOpusOnlyFlag = Boolean(args.bitrate) || Boolean(args["sample-rate"]);
    if (hasOpusOnlyFlag) {
      const outExt = typeof args.out === "string"
        ? args.out.split(".").pop()?.toLowerCase()
        : undefined;
      const impliesOpus = outExt && ["ogg", "opus", "oga"].includes(outExt);
      if (format === "wav" || (format === undefined && !impliesOpus)) {
        log.error("--bitrate and --sample-rate are only valid with --format ogg-opus");
        process.exit(2);
      }
    }

    const opts = {
      text,
      voice,
      lang: typeof args.lang === "string" ? args.lang : undefined,
      out: typeof args.out === "string" ? args.out : undefined,
      rate: args.rate ? Number(args.rate) : undefined,
      ssml: Boolean(args.ssml),
      format,
      bitrate: args.bitrate ? Number(args.bitrate) : undefined,
      sampleRate: args["sample-rate"] ? Number(args["sample-rate"]) : undefined,
      noExpandAbbrev: Boolean(args["no-expand-abbrev"]),
    };
    const stats = createStatsRecorder("say");

    try {
      const startedAt = performance.now();
      const audio = await stats.timeStage("tts", () => say(opts));
      const ttsTimeMs = Math.round(performance.now() - startedAt);
      if (args.verbose) {
        // stderr — stdout may carry raw audio bytes when --out is omitted.
        console.error(`TTS time: ${ttsTimeMs}ms`);
      }
      if (opts.out) {
        const outputArtifact = artifactFromFile(opts.out, "output_audio");
        if (outputArtifact) stats.recordArtifact(outputArtifact);
      } else {
        stats.recordArtifact(artifactFromBytes(audio.byteLength, "output_audio", opts.format ?? "wav"));
      }
      if (!opts.out) {
        process.stdout.write(audio);
      }
      stats.finish("success", 1);
    } catch (err) {
      stats.recordError("tts", err);
      stats.finish("failed", 1);
      if (err instanceof SayError) {
        log.error(err.stderr.trim() || err.message);
        process.exit(err.exitCode);
      }
      const message = err instanceof Error ? err.message : String(err);
      log.error(message);
      process.exit(4);
    }
  },
});
