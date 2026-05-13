import { defineCommand } from "citty";
import { detect } from "tinyld";
import { transcribeWithSegments } from "../transcribe";
import { detectAudioLanguageEngine, detectTextLanguageEngine } from "../engine";
import type { LangDetectResult } from "../engine";
import { log } from "../log";
import type { TranscribeResult } from "../types";
import {
  formatJsonOutput,
  formatTextOutput,
  formatTranscriptOutput,
  formatVerboseOutput,
} from "../format";
import { formatToonOutput } from "../toon";

const pkg = await Bun.file(new URL("../../package.json", import.meta.url)).json();

interface MainCommandArgs {
  _: string[];
  json: boolean;
  toon: boolean;
  verbose: boolean;
  debug: boolean;
  vad: boolean;
  "no-vad": boolean;
  timestamps: boolean;
  speakers: boolean;
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
  return { ok: true, wantsJson, wantsToon, wantsTranscript };
}

export function checkLanguageMismatch(expected: string | undefined, detected: string): string | null {
  if (!expected || !detected || expected === detected) return null;
  return `warning: expected language "${expected}" but detected "${detected}"`;
}

export const mainCommand = defineCommand({
  meta: {
    name: "kesha",
    version: pkg.version,
    description:
      "Kesha Voice Kit — open-source voice toolkit for Apple Silicon.\n" +
      "  Run 'kesha install [--no-cache]' to download engine and models.\n" +
      "  Run 'kesha status' to inspect installed backend.",
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
  async run({ args }: { args: MainCommandArgs }) {
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

    if (args.vad && args["no-vad"]) {
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
    const vadMode = args.vad ? "on" : args["no-vad"] ? "off" : "auto";

    if (files.length === 0) {
      log.info("Usage: kesha <audio_file> [audio_file ...]\n       kesha install [--no-cache]\n       kesha status");
      process.exit(1);
    }

    let hasError = false;
    const results: TranscribeResult[] = [];

    const wantsLangId = !!(args.lang || args.verbose || wantsJson || wantsToon || wantsTranscript);

    for (const file of files) {
      const startedAt = performance.now();
      try {
        // Run audio lang-id and transcription concurrently.
        const [audioResult, transcript] = await Promise.all([
          wantsLangId ? detectAudioLanguageEngine(file) : Promise.resolve(null),
          transcribeWithSegments(file, { vad: vadMode, timestamps: args.timestamps, speakers: args.speakers }),
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
          const engineTextResult = await detectTextLanguageEngine(text);
          if (engineTextResult && engineTextResult.code) {
            textLanguage = engineTextResult;
          }
        }

        const lang = textLanguage?.code || tinyldLang;

        const mismatchWarning = checkLanguageMismatch(args.lang, lang);
        if (mismatchWarning) log.warn(`${file}: ${mismatchWarning}`);

        const result: TranscribeResult = {
          file,
          text,
          lang,
          audioLanguage,
          textLanguage: textLanguage ?? (tinyldLang ? { code: tinyldLang, confidence: 0 } : undefined),
          sttTimeMs: Math.round(performance.now() - startedAt),
        };
        if (args.timestamps) {
          result.segments = segments;
        }
        results.push(result);
      } catch (err: unknown) {
        hasError = true;
        const message = err instanceof Error ? err.message : String(err);
        log.error(`${file}: ${message}`);
      }
    }

    if (wantsJson) {
      process.stdout.write(formatJsonOutput(results));
    } else if (wantsToon) {
      process.stdout.write(formatToonOutput(results));
    } else if (wantsTranscript) {
      process.stdout.write(formatTranscriptOutput(results));
    } else if (args.verbose) {
      process.stdout.write(formatVerboseOutput(results));
    } else {
      process.stdout.write(formatTextOutput(results));
    }

    if (hasError) process.exit(1);
  },
});
