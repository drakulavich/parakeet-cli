import { defineCommand } from "citty";
import { existsSync } from "fs";
import { detect } from "tinyld";
import { preflightTranscribeWithSegments, transcribeWithSegments } from "../transcribe";
import { detectAudioLanguageEngine, detectTextLanguageEngine } from "../engine";
import type { LangDetectResult } from "../engine";
import { log } from "../log";
import type { TranscribeErrorRecord, TranscribeResult } from "../types";
import {
  formatJsonOutput,
  formatTextOutput,
  formatTranscriptOutput,
  formatVerboseOutput,
} from "../format";
import { packageVersion } from "../package-info";
import { formatToonOutput } from "../toon";
import { artifactFromFile, createStatsRecorder } from "../stats";
import { createActivityProgress } from "../progress";

interface MainCommandArgs {
  _: string[];
  json: boolean;
  toon: boolean;
  verbose: boolean;
  debug: boolean;
  vad: boolean;
  "no-vad": boolean;
  noVad?: boolean;
  no_vad?: boolean;
  timestamps: boolean;
  speakers: boolean;
  "include-errors": boolean;
  format?: string;
  lang?: string;
}

export function detectLanguage(text: string): string {
  if (!text) return "";
  return detect(text);
}

/**
 * Pure validation + normalization of the output-format selection. Pulled
 * out of the citty `run` handler so the contract is unit-testable without
 * spawning the CLI binary; the handler just owns the side effects
 * (log.error + process.exit) when this returns `{ ok: false }`.
 *
 * Inputs accept the three knobs the user can flip:
 * - `--json` (boolean) — long-form alias for `--format json`
 * - `--toon` (boolean) — long-form alias for `--format toon`
 * - `--format <value>` — must be one of `transcript`, `json`, `toon`
 *
 * Mutex: `--json` and `--toon` cannot both be requested. Either via the
 * booleans or `--format` cross-pollination (`--json --format toon` →
 * error). The mutex check happens AFTER format validation, so unknown
 * `--format` still surfaces with its own clearer error first.
 */
export type ResolvedOutputFormat =
  | {
      ok: true;
      wantsJson: boolean;
      wantsToon: boolean;
      wantsTranscript: boolean;
    }
  | { ok: false; error: string };

const SUPPORTED_FORMATS = ["transcript", "json", "toon"] as const;

export function resolveOutputFormat(input: {
  json?: boolean;
  toon?: boolean;
  format?: string;
}): ResolvedOutputFormat {
  if (input.format !== undefined && !SUPPORTED_FORMATS.includes(input.format as never)) {
    return {
      ok: false,
      error: `unknown --format '${input.format}'. supported: ${SUPPORTED_FORMATS.join(", ")}`,
    };
  }
  const wantsJson = !!input.json || input.format === "json";
  const wantsToon = !!input.toon || input.format === "toon";
  const wantsTranscript = input.format === "transcript";
  if (wantsJson && wantsToon) {
    return {
      ok: false,
      error: "--json and --toon are mutually exclusive (pick one output format).",
    };
  }
  // `--format transcript` + boolean `--json` / `--toon` was previously
  // accepted and silently produced the boolean's format (because the
  // dispatch checked wantsJson/wantsToon first). Greptile P2 on #300
  // flagged the silent override. Fail loudly with the same shape as
  // the json/toon mutex — symmetric across all three formats.
  if (wantsTranscript && (wantsJson || wantsToon)) {
    return {
      ok: false,
      error:
        "--format transcript is mutually exclusive with --json / --toon " +
        "(pick one output format).",
    };
  }
  return { ok: true, wantsJson, wantsToon, wantsTranscript };
}

export function checkLanguageMismatch(expected: string | undefined, detected: string): string | null {
  if (!expected || !detected || expected === detected) return null;
  return `warning: expected language "${expected}" but detected "${detected}"`;
}

export const mainCommand = defineCommand({
  meta: {
    name: "kesha",
    version: packageVersion,
    description:
      "Kesha Voice Kit — open-source voice toolkit for Apple Silicon.\n" +
      "\n" +
      "Commands:\n" +
      "  completions  Print shell completion script.\n" +
      "  doctor     Collect support diagnostics.\n" +
      "  install    Download engine and models.\n" +
      "  manpage    Print the kesha(1) manpage.\n" +
      "  status     Inspect installed backend.\n" +
      "  say        Synthesize speech from text.\n" +
      "  stats      Manage local anonymous performance stats.\n" +
      "  support-bundle  Create a redacted diagnostics archive.",
  },
  args: {
    json: {
      type: "boolean",
      description: "Output results as JSON",
      default: false,
    },
    toon: {
      type: "boolean",
      description: "Output results as TOON (compact, LLM-friendly encoding of the same data as --json)",
      default: false,
    },
    timestamps: {
      type: "boolean",
      description: "Include timestamped transcript segments in JSON/TOON output",
      default: false,
    },
    speakers: {
      type: "boolean",
      description: "Include speaker labels in transcript segments. Requires --json / --toon / --format json. Implies --timestamps. Currently darwin-arm64 only (#199).",
      default: false,
    },
    "include-errors": {
      type: "boolean",
      description: "With --json, output { results, errors } so scripts can read per-file failures without parsing stderr",
      default: false,
    },
    verbose: {
      type: "boolean",
      description: "Show language detection details",
      default: false,
    },
    format: {
      type: "string",
      description: "Output format: transcript | json | toon (long-form alias for --json / --toon)",
    },
    lang: {
      type: "string",
      description: "Expected language code (ISO 639-1), warn if mismatch",
    },
    debug: {
      type: "boolean",
      description: "Trace engine subprocess calls on stderr (or KESHA_DEBUG=1)",
      default: false,
    },
    vad: {
      type: "boolean",
      description: "Force Silero VAD preprocessing (kesha install --vad first). Without this, VAD auto-engages on audio ≥ 120s.",
      default: false,
    },
    "no-vad": {
      type: "boolean",
      description: "Disable VAD preprocessing regardless of duration or install state",
      default: false,
    },
  },
  async run({ args, rawArgs }: { args: MainCommandArgs; rawArgs: string[] }) {
    if (args.debug) log.debugEnabled = true;
    const files = args._;

    // Validate `--format <value>` and normalize into the boolean flags
    // that the rest of this handler consults. Routing happens in
    // `resolveOutputFormat` so the contract is unit-testable without
    // spawning the CLI; the handler just owns the side-effect arms
    // (log.error + process.exit).
    const fmt = resolveOutputFormat({
      json: args.json,
      toon: args.toon,
      format: args.format,
    });
    if (!fmt.ok) {
      log.error(fmt.error);
      process.exit(2);
    }
    const { wantsJson, wantsToon, wantsTranscript } = fmt;

    // citty treats --no-vad as the negated form of --vad, so read rawArgs
    // to distinguish "off" from the default auto mode and to catch both flags.
    const vad = rawArgs.includes("--vad") || Boolean(args.vad);
    const noVad = rawArgs.includes("--no-vad") || Boolean(args["no-vad"] ?? args.noVad ?? args.no_vad);

    if (vad && noVad) {
      log.error("--vad and --no-vad are mutually exclusive.");
      process.exit(2);
    }
    if (args.timestamps && !(wantsJson || wantsToon)) {
      log.error("--timestamps requires --json, --toon, or --format {json,toon}.");
      process.exit(2);
    }
    if (args.speakers && !(wantsJson || wantsToon)) {
      log.error("--speakers requires --json, --toon, or --format {json,toon}.");
      process.exit(2);
    }
    if (args["include-errors"] && !wantsJson) {
      log.error("--include-errors requires --json or --format json.");
      process.exit(2);
    }
    const vadMode = vad ? "on" : noVad ? "off" : "auto";

    if (files.length === 0) {
      log.info(
        "Usage: kesha <audio_file> [audio_file ...]\n" +
          "       kesha completions <bash|zsh|fish>\n" +
          "       kesha doctor [--json] [--redact]\n" +
          "       kesha install [--no-cache]\n" +
          "       kesha manpage\n" +
          "       kesha status\n" +
          "       kesha say <text>\n" +
          "       kesha stats [enable|disable|status|week|errors|export|reset|vacuum|retention]\n" +
          "       kesha support-bundle [--output path.tar.gz]",
      );
      process.exit(1);
    }

    let hasError = false;
    const results: TranscribeResult[] = [];
    const errors: TranscribeErrorRecord[] = [];
    const stats = createStatsRecorder("transcribe");

    const wantsLangId = !!(args.lang || args.verbose || wantsJson || wantsToon || wantsTranscript);
    const reportProgress = process.stderr.isTTY === true || process.stdout.isTTY !== true;

    for (const file of files) {
      if (!existsSync(file)) {
        hasError = true;
        stats.recordError("input", new Error("File not found"), "file_not_found");
        errors.push({ file, code: "file_not_found", message: "File not found" });
        log.error(`${file}: File not found`);
        continue;
      }
      const inputArtifact = artifactFromFile(file, "input_audio");
      if (inputArtifact) stats.recordArtifact(inputArtifact);

      const startedAt = performance.now();
      let progress: ReturnType<typeof createActivityProgress> | null = null;
      try {
        await preflightTranscribeWithSegments({
          vad: vadMode,
          timestamps: args.timestamps,
          speakers: args.speakers,
        });
        progress = reportProgress ? createActivityProgress(`Transcribing ${file}`) : null;
        // Run audio lang-id and transcription concurrently.
        const [audioResult, transcript] = await Promise.all([
          wantsLangId
            ? stats.timeStage("lang_id_audio", () => detectAudioLanguageEngine(file))
            : Promise.resolve(null),
          stats.timeStage("transcribe", () =>
            transcribeWithSegments(file, { vad: vadMode, timestamps: args.timestamps, speakers: args.speakers })
          ),
        ]);
        const { text, segments } = transcript;

        let audioLanguage: LangDetectResult | undefined;
        if (audioResult && audioResult.code) {
          audioLanguage = audioResult;
        }

        if (audioLanguage && args.lang && audioLanguage.confidence > 0.8) {
          const mismatch = checkLanguageMismatch(args.lang, audioLanguage.code);
          if (mismatch) log.warn(`${file}: ${mismatch} (from audio)`);
        }

        const tinyldLang = wantsLangId ? detectLanguage(text) : "";
        let textLanguage: LangDetectResult | undefined;

        if (wantsLangId) {
          const engineTextResult = await stats.timeStage("lang_id_text", () => detectTextLanguageEngine(text));
          if (engineTextResult && engineTextResult.code) {
            textLanguage = engineTextResult;
          }
        }

        const lang = textLanguage?.code || tinyldLang;

        const mismatchWarning = checkLanguageMismatch(args.lang, lang);
        if (mismatchWarning) log.warn(`${file}: ${mismatchWarning}`);

        const sttTimeMs = Math.round(performance.now() - startedAt);
        const result: TranscribeResult = {
          file,
          text,
          lang,
          audioLanguage,
          textLanguage: textLanguage ?? (tinyldLang ? { code: tinyldLang, confidence: 0 } : undefined),
          sttTimeMs,
        };
        if (args.timestamps || args.speakers) {
          result.segments = segments;
        }
        results.push(result);
        progress?.finish(`Transcribed ${file} (${sttTimeMs}ms)`);
      } catch (err: unknown) {
        progress?.stop();
        hasError = true;
        stats.recordError("transcribe", err);
        const message = err instanceof Error ? err.message : String(err);
        errors.push({ file, code: "transcribe_failed", message });
        log.error(`${file}: ${message}`);
      }
    }

    if (wantsJson) {
      process.stdout.write(formatJsonOutput(results, args["include-errors"] ? errors : undefined));
    } else if (wantsToon) {
      process.stdout.write(formatToonOutput(results));
    } else if (wantsTranscript) {
      process.stdout.write(formatTranscriptOutput(results));
    } else if (args.verbose) {
      process.stdout.write(formatVerboseOutput(results));
    } else {
      process.stdout.write(formatTextOutput(results));
    }

    stats.finish(hasError ? "failed" : "success", files.length);

    if (hasError) process.exit(1);
  },
});
