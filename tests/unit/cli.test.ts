import { describe, test, expect } from "bun:test";
import { renderUsage } from "citty";
import { decode as decodeToon } from "@toon-format/toon";
import { mainCommand, completionsCommand, doctorCommand, installCommand, manpageCommand, recordCommand, statusCommand, statsCommand, supportBundleCommand, sayCommand, formatTextOutput, formatJsonOutput, formatToonOutput, detectLanguage, checkLanguageMismatch, estimateTranscriptDurationSeconds, resolveOutputFormat, resolveRecordArgs, shouldReportTranscribeProgress, shouldRunAudioLanguageDetection } from "../../src/cli";

type MainRun = (input: { args: Record<string, unknown>; rawArgs: string[] }) => Promise<void>;

function normalizeUsage(usage: string): string {
  return usage
    .replace(/\(kesha v\d+\.\d+\.\d+(?:[-+][^)]+)?\)/g, "(kesha v<version>)")
    .split("\n")
    .map((line) => line.trimEnd())
    .join("\n")
    .trim();
}

function defaultMainArgs(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    _: [],
    json: false,
    toon: false,
    verbose: false,
    debug: false,
    vad: false,
    "no-vad": false,
    timestamps: false,
    speakers: false,
    "include-errors": false,
    ...overrides,
  };
}

async function expectMainExit(
  args: Record<string, unknown>,
  rawArgs: string[],
): Promise<number> {
  const savedExit = process.exit;
  const savedLog = console.log;
  const savedError = console.error;
  try {
    console.log = () => {};
    console.error = () => {};
    process.exit = ((code?: string | number | null | undefined) => {
      throw new Error(`exit:${code ?? 0}`);
    }) as typeof process.exit;
    await (mainCommand.run as MainRun)({ args, rawArgs });
    throw new Error("main command did not exit");
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    expect(message.startsWith("exit:")).toBe(true);
    return Number(message.slice("exit:".length));
  } finally {
    process.exit = savedExit;
    console.log = savedLog;
    console.error = savedError;
  }
}

describe("CLI help", () => {
  test("main help contains usage and install info", async () => {
    const usage = await renderUsage(mainCommand);
    expect(usage).toContain("USAGE");
    expect(usage).toContain("install");
  });

  test("main help shows subcommand inventory (#324)", async () => {
    const usage = await renderUsage(mainCommand);
    expect(usage).toContain("Commands:");
    expect(usage).toContain("completions");
    expect(usage).toContain("doctor     Collect support diagnostics.");
    expect(usage).toContain("install    Download engine and models.");
    expect(usage).toContain("manpage");
    expect(usage).toContain("record     Record microphone audio to a WAV file.");
    expect(usage).toContain("status     Inspect installed backend.");
    expect(usage).toContain("say        Synthesize speech from text.");
    expect(usage).toContain("stats      Manage local anonymous performance stats.");
    expect(usage).toContain("support-bundle");
  });

  test("install help contains backend and cache options", async () => {
    const usage = await renderUsage(installCommand);
    expect(usage).toContain("--coreml");
    expect(usage).toContain("--onnx");
    expect(usage).toContain("--no-cache");
    expect(usage).toContain("--plan");
  });

  test("doctor help contains JSON and redaction options (#345 P0)", async () => {
    const usage = await renderUsage(doctorCommand);
    expect(usage).toContain("--json");
    expect(usage).toContain("--redact");
    expect(usage).toContain("support diagnostics");
  });

  test("support-bundle help contains archive output option (#345 P0)", async () => {
    const usage = await renderUsage(supportBundleCommand);
    expect(usage).toContain("support-bundle");
    expect(usage).toContain("--output");
    expect(usage).toContain("redacted diagnostics archive");
  });

  test("completions help contains supported shells (#344 P2)", async () => {
    const usage = await renderUsage(completionsCommand);
    expect(usage).toContain("completions");
    expect(usage).toContain("bash | zsh | fish");
  });

  test("manpage help contains command description (#344 P2)", async () => {
    const usage = await renderUsage(manpageCommand);
    expect(usage).toContain("manpage");
    expect(usage).toContain("kesha(1)");
  });

  test("main help contains --json flag", async () => {
    const usage = await renderUsage(mainCommand);
    expect(usage).toContain("--json");
  });

  test("main help contains --include-errors flag (#324 P1)", async () => {
    const usage = await renderUsage(mainCommand);
    expect(usage).toContain("--include-errors");
  });

  test("main help contains --toon flag (#138)", async () => {
    const usage = await renderUsage(mainCommand);
    expect(usage).toContain("--toon");
    expect(usage).toMatch(/TOON|LLM/i);
  });

  test("main help contains --timestamps flag", async () => {
    const usage = await renderUsage(mainCommand);
    expect(usage).toContain("--timestamps");
  });

  test("main help contains --lang flag", async () => {
    const usage = await renderUsage(mainCommand);
    expect(usage).toContain("--lang");
  });

  test("main help contains --verbose flag", async () => {
    const usage = await renderUsage(mainCommand);
    expect(usage).toContain("--verbose");
  });

  test("main help contains --debug flag (#148)", async () => {
    const usage = await renderUsage(mainCommand);
    expect(usage).toContain("--debug");
    expect(usage).toMatch(/KESHA_DEBUG|trace/i);
  });

  test("status help has command description", async () => {
    const usage = await renderUsage(statusCommand);
    expect(usage).toContain("status");
    expect(usage).toContain("Show backend installation status");
  });

  test("record help contains output and duration options", async () => {
    const usage = await renderUsage(recordCommand);
    expect(usage).toContain("record");
    expect(usage).toContain("--out");
    expect(usage).toContain("--max-seconds");
  });

  test("stats help has command description", async () => {
    const usage = await renderUsage(statsCommand);
    expect(usage).toContain("stats");
    expect(usage).toContain("enable");
  });
});

describe("main command validation side effects", () => {
  test("--timestamps requires machine-readable output", async () => {
    await expect(
      expectMainExit(defaultMainArgs({ timestamps: true, _: ["audio.wav"] }), ["--timestamps"]),
    ).resolves.toBe(2);
  });

  test("--vad and --no-vad are mutually exclusive", async () => {
    await expect(
      expectMainExit(defaultMainArgs({ vad: true, _: ["audio.wav"] }), ["--vad", "--no-vad"]),
    ).resolves.toBe(2);
  });

  test("empty invocation exits after printing usage", async () => {
    await expect(expectMainExit(defaultMainArgs(), [])).resolves.toBe(1);
  });

});

describe("transcription progress reporting", () => {
  test("reports progress on terminals and redirected stdout unless debug is enabled", () => {
    expect(
      shouldReportTranscribeProgress({
        stderrIsTty: true,
        stdoutIsTty: true,
        debugEnabled: false,
      }),
    ).toBe(true);
    expect(
      shouldReportTranscribeProgress({
        stderrIsTty: false,
        stdoutIsTty: false,
        debugEnabled: false,
      }),
    ).toBe(true);
    expect(
      shouldReportTranscribeProgress({
        stderrIsTty: true,
        stdoutIsTty: true,
        debugEnabled: true,
      }),
    ).toBe(false);
  });
});

describe("audio language detection routing", () => {
  test("estimates transcript duration from segment ends", () => {
    expect(
      estimateTranscriptDurationSeconds([
        { start: 0, end: 2.5, text: "short" },
        { start: 2.5, end: 601, text: "long" },
      ]),
    ).toBe(601);
    expect(estimateTranscriptDurationSeconds([])).toBeNull();
  });

  test("skips whole-file audio language detection for long ASR timelines", () => {
    expect(
      shouldRunAudioLanguageDetection({
        wantsLangId: true,
        transcriptDurationSeconds: 600,
      }),
    ).toBe(true);
    expect(
      shouldRunAudioLanguageDetection({
        wantsLangId: true,
        transcriptDurationSeconds: 601,
      }),
    ).toBe(false);
    expect(
      shouldRunAudioLanguageDetection({
        wantsLangId: false,
        transcriptDurationSeconds: 601,
      }),
    ).toBe(false);
    expect(
      shouldRunAudioLanguageDetection({
        wantsLangId: true,
        transcriptDurationSeconds: null,
      }),
    ).toBe(true);
  });
});

describe("record command validation", () => {
  test("requires --out", () => {
    expect(resolveRecordArgs({})).toEqual({
      ok: false,
      error: "kesha record requires --out <path>.",
    });
  });

  test("normalizes default max seconds", () => {
    expect(resolveRecordArgs({ out: "mic.wav" })).toEqual({
      ok: true,
      out: "mic.wav",
      maxSeconds: 120,
    });
  });

  test("rejects invalid max seconds", () => {
    expect(resolveRecordArgs({ out: "mic.wav", "max-seconds": "0" })).toEqual({
      ok: false,
      error: "--max-seconds must be an integer between 1 and 3600.",
    });
    expect(resolveRecordArgs({ out: "mic.wav", "max-seconds": "1.5" })).toEqual({
      ok: false,
      error: "--max-seconds must be an integer between 1 and 3600.",
    });
    expect(resolveRecordArgs({ out: "mic.wav", "max-seconds": "nope" })).toEqual({
      ok: false,
      error: "--max-seconds must be a finite number.",
    });
  });
});

describe("CLI help golden contracts (#324 P1)", () => {
  test("normalizer replaces every rendered version token", () => {
    expect(normalizeUsage("first (kesha v1.18.0)\nsecond (kesha v1.18.1-cli)")).toBe(
      "first (kesha v<version>)\nsecond (kesha v<version>)",
    );
  });

  test("main help matches the normalized golden output", async () => {
    expect(normalizeUsage(await renderUsage(mainCommand))).toBe(`Kesha Voice Kit — open-source voice toolkit for Apple Silicon.

Commands:
  completions  Print shell completion script.
  doctor     Collect support diagnostics.
  install    Download engine and models.
  manpage    Print the kesha(1) manpage.
  record     Record microphone audio to a WAV file.
  status     Inspect installed backend.
  say        Synthesize speech from text.
  stats      Manage local anonymous performance stats.
  support-bundle  Create a redacted diagnostics archive. (kesha v<version>)

USAGE kesha [OPTIONS]

OPTIONS

             --json    Output results as JSON (Default: false)
             --toon    Output results as TOON (compact, LLM-friendly encoding of the same data as --json) (Default: false)
       --timestamps    Include timestamped transcript segments in JSON/TOON output (Default: false)
         --speakers    Include speaker labels in transcript segments. Requires --json / --toon / --format json. Implies --timestamps. Currently darwin-arm64 only (#199). (Default: false)
   --include-errors    With --json, output { results, errors } so scripts can read per-file failures without parsing stderr (Default: false)
          --verbose    Show language detection details (Default: false)
  --format=<format>    Output format: transcript | json | toon (long-form alias for --json / --toon)
      --lang=<lang>    Expected language code (ISO 639-1), warn if mismatch
            --debug    Trace engine subprocess calls on stderr (or KESHA_DEBUG=1) (Default: false)
              --vad    Force Silero VAD preprocessing (kesha install --vad first). Without this, VAD auto-engages on audio ≥ 120s. (Default: false)
           --no-vad    Force full-file ASR for short/medium files; long audio fails early (Default: false)`);
  });

  test("install help matches the normalized golden output", async () => {
    expect(normalizeUsage(await renderUsage(installCommand))).toBe(`Download inference engine and models (install)

USAGE install [OPTIONS]

OPTIONS

    --coreml    Force CoreML backend (macOS arm64) (Default: false)
      --onnx    Force ONNX backend (Default: false)
  --no-cache    Re-download even if cached (Default: false)
      --plan    Show download, disk, and warm-up plan without changing local state (Default: false)
       --tts    Also install TTS models (Kokoro EN + Vosk-TTS RU, ~990MB) (Default: false)
       --vad    Also install Silero VAD (~2.3MB) for long-audio preprocessing (Default: false)
   --diarize    Also install the Sortformer streaming-diarization model (~245MB, darwin-arm64 only — #199) (Default: false)`);
  });

  test("status help matches the normalized golden output", async () => {
    expect(normalizeUsage(await renderUsage(statusCommand))).toBe(`Show backend installation status (status)

USAGE status [OPTIONS]

OPTIONS

  --disk    Include recursive cache disk usage (Default: false)`);
  });

  test("say help matches the normalized golden output", async () => {
    expect(normalizeUsage(await renderUsage(sayCommand))).toBe(`Synthesize speech from text (TTS). Writes audio to stdout (or --out file). Defaults to WAV; use --format ogg-opus for messenger-ready voice notes. (say)

USAGE say [OPTIONS] [TEXT]

ARGUMENTS

  TEXT    Text to speak (stdin if omitted)

OPTIONS

              --voice=<voice>    Voice id, e.g. en-am_michael
                --lang=<lang>    BCP 47 language code (default en-us)
                  --out=<out>    Write audio to file instead of stdout
                --rate=<rate>    Speaking rate 0.5–2.0 (Default: 1.0)
                --list-voices    List installed voices and exit
                       --ssml    Parse input as SSML (supports <speak>, <break>; strips unknown tags)
            --format=<format>    Output format: wav (default) or ogg-opus (Telegram-ready voice note). Inferred from --out extension when omitted.
          --bitrate=<bitrate>    Opus bitrate in bits/sec (e.g. 32000). Only with --format ogg-opus.
  --sample-rate=<sample_rate>    Opus encoder sample rate (8000/12000/16000/24000/48000). Only with --format ogg-opus.
           --no-expand-abbrev    Disable Russian acronym auto-expansion (ВОЗ → 'вэ о зэ') for ru-vosk-* voices. <say-as interpret-as='characters'> still works. Applies to Russian (ru-vosk-*) and English (en-*) voices.
                    --verbose    Log TTS synthesis time to stderr (Default: false)
                      --debug    Trace engine subprocess calls on stderr (or KESHA_DEBUG=1) (Default: false)`);
  });
});

describe("output formatting", () => {
  test("single file text: no header", () => {
    const output = formatTextOutput([{ file: "a.ogg", text: "Hello", lang: "en" }]);
    expect(output).toBe("Hello\n");
  });

  test("JSON output: always array, pretty-printed", () => {
    const output = formatJsonOutput([{ file: "a.ogg", text: "Hello", lang: "en" }]);
    const parsed = JSON.parse(output);
    expect(Array.isArray(parsed)).toBe(true);
    expect(parsed).toEqual([{ file: "a.ogg", text: "Hello", lang: "en" }]);
    expect(output).toContain("\n");
  });

  test("JSON output: multiple files", () => {
    const output = formatJsonOutput([
      { file: "a.ogg", text: "Hello", lang: "en" },
      { file: "b.mp3", text: "World", lang: "en" },
    ]);
    const parsed = JSON.parse(output);
    expect(parsed).toHaveLength(2);
    expect(parsed[0].file).toBe("a.ogg");
    expect(parsed[1].file).toBe("b.mp3");
  });

  test("JSON output: empty array when no results", () => {
    const output = formatJsonOutput([]);
    expect(JSON.parse(output)).toEqual([]);
  });

  test("JSON output can opt into structured file errors (#324 P1)", () => {
    const output = formatJsonOutput(
      [{ file: "ok.ogg", text: "Hello", lang: "en" }],
      [{ file: "missing.ogg", code: "file_not_found", message: "File not found" }],
    );
    expect(JSON.parse(output)).toEqual({
      results: [{ file: "ok.ogg", text: "Hello", lang: "en" }],
      errors: [{ file: "missing.ogg", code: "file_not_found", message: "File not found" }],
    });
  });

  test("JSON output with --include-errors and no errors still uses envelope shape", () => {
    const output = formatJsonOutput(
      [{ file: "ok.ogg", text: "Hello", lang: "en" }],
      [],
    );
    expect(JSON.parse(output)).toEqual({
      results: [{ file: "ok.ogg", text: "Hello", lang: "en" }],
      errors: [],
    });
  });

  test("JSON output preserves timestamped segments", () => {
    const output = formatJsonOutput([
      {
        file: "a.ogg",
        text: "Hello",
        lang: "en",
        segments: [{ start: 0, end: 1.25, text: "Hello" }],
      },
    ]);
    const parsed = JSON.parse(output);
    expect(parsed[0].segments).toEqual([{ start: 0, end: 1.25, text: "Hello" }]);
  });
});

describe("TOON output (#138)", () => {
  test("round-trips multi-file through decode", () => {
    const input = [
      { file: "a.ogg", text: "Hello", lang: "en" },
      { file: "b.ogg", text: "Hola", lang: "es" },
    ];
    const output = formatToonOutput(input);
    const decoded = decodeToon(output);
    expect(decoded).toEqual(input);
  });

  test("multi-file uniform array has a single schema header row", () => {
    const output = formatToonOutput([
      { file: "a.ogg", text: "Hello", lang: "en" },
      { file: "b.ogg", text: "World", lang: "en" },
    ]);
    // The tabular form emits the schema exactly once (`{file,text,lang}`);
    // if the encoder ever fell back to per-object mode the field list would
    // appear on every row — this guards that regression.
    const schemaRows = output.match(/\{file,text,lang\}/g) ?? [];
    expect(schemaRows).toHaveLength(1);
  });

  test("preserves sttTimeMs and nested language fields", () => {
    const input = [{
      file: "a.ogg",
      text: "Hello",
      lang: "en",
      sttTimeMs: 427,
      audioLanguage: { code: "en", confidence: 0.94 },
      textLanguage: { code: "en", confidence: 0.98 },
    }];
    const decoded = decodeToon(formatToonOutput(input));
    expect(decoded).toEqual(input);
  });

  test("preserves timestamped segments", () => {
    const input = [{
      file: "a.ogg",
      text: "Hello",
      lang: "en",
      segments: [{ start: 0, end: 1.25, text: "Hello" }],
    }];
    const decoded = decodeToon(formatToonOutput(input));
    expect(decoded).toEqual(input);
  });

  test("empty array", () => {
    const output = formatToonOutput([]);
    expect(decodeToon(output)).toEqual([]);
  });

  test("ends with a trailing newline so it composes in pipelines", () => {
    const output = formatToonOutput([{ file: "a.ogg", text: "Hi", lang: "en" }]);
    expect(output.endsWith("\n")).toBe(true);
  });
});

describe("output formatting with lang", () => {
  test("JSON output includes lang field", () => {
    const output = formatJsonOutput([{ file: "a.ogg", text: "Hello world", lang: "en" }]);
    const parsed = JSON.parse(output);
    expect(parsed[0].lang).toBe("en");
  });

  test("JSON output includes empty lang when not detected", () => {
    const output = formatJsonOutput([{ file: "a.ogg", text: "", lang: "" }]);
    const parsed = JSON.parse(output);
    expect(parsed[0].lang).toBe("");
  });
});

describe("language detection", () => {
  test("detects English text", () => {
    const lang = detectLanguage("This is a simple English sentence for testing.");
    expect(lang).toBe("en");
  });

  test("detects Russian text", () => {
    const lang = detectLanguage("Это простое предложение на русском языке для тестирования.");
    expect(lang).toBe("ru");
  });

  test("returns empty string for empty text", () => {
    const lang = detectLanguage("");
    expect(lang).toBe("");
  });

  test("checkLanguageMismatch returns null when no expected lang", () => {
    const warning = checkLanguageMismatch(undefined, "en");
    expect(warning).toBeNull();
  });

  test("checkLanguageMismatch returns null when languages match", () => {
    const warning = checkLanguageMismatch("en", "en");
    expect(warning).toBeNull();
  });

  test("checkLanguageMismatch returns warning when languages differ", () => {
    const warning = checkLanguageMismatch("ru", "en");
    expect(warning).toContain("expected language");
    expect(warning).toContain("ru");
    expect(warning).toContain("en");
  });

  test("checkLanguageMismatch returns null when detected is empty", () => {
    const warning = checkLanguageMismatch("en", "");
    expect(warning).toBeNull();
  });
});


describe("CLI help with status", () => {
  test("main description mentions install command", async () => {
    const usage = await renderUsage(mainCommand);
    expect(usage).toContain("install");
  });

  test("main help includes status command", async () => {
    const usage = await renderUsage(mainCommand);
    expect(usage).toContain("status");
  });

  test("main description mentions stats command", async () => {
    const usage = await renderUsage(mainCommand);
    expect(usage).toContain("stats");
  });
});

describe("say --verbose (TTS time, parallel to #139)", () => {
  test("say help advertises --verbose", async () => {
    const usage = await renderUsage(sayCommand);
    expect(usage).toContain("--verbose");
  });

  test("say help explains --verbose prints TTS time", async () => {
    const usage = await renderUsage(sayCommand);
    expect(usage).toMatch(/TTS|synthesis time/i);
  });
});

describe("sttTimeMs field (#139)", () => {
  test("JSON output preserves sttTimeMs when set", () => {
    const results = [{ file: "a.ogg", text: "Hello", lang: "en", sttTimeMs: 427 }];
    const parsed = JSON.parse(formatJsonOutput(results));
    expect(parsed[0].sttTimeMs).toBe(427);
  });

  test("JSON output omits sttTimeMs when undefined", () => {
    const parsed = JSON.parse(formatJsonOutput([{ file: "a.ogg", text: "Hello", lang: "en" }]));
    expect(parsed[0].sttTimeMs).toBeUndefined();
  });

  test("plain-text output is unchanged by sttTimeMs", () => {
    const results = [{ file: "a.ogg", text: "Hello", lang: "en", sttTimeMs: 427 }];
    expect(formatTextOutput(results)).toBe("Hello\n");
  });
});

describe("JSON output with lang-id fields", () => {
  test("JSON includes audioLanguage and textLanguage when present", () => {
    const results = [{
      file: "a.ogg", text: "Hello", lang: "en",
      audioLanguage: { code: "en", confidence: 0.94 },
      textLanguage: { code: "en", confidence: 0.98 },
    }];
    const parsed = JSON.parse(formatJsonOutput(results));
    expect(parsed[0].audioLanguage).toEqual({ code: "en", confidence: 0.94 });
    expect(parsed[0].textLanguage).toEqual({ code: "en", confidence: 0.98 });
    expect(parsed[0].lang).toBe("en");
  });

  test("JSON omits audioLanguage when not detected", () => {
    const results = [{ file: "a.ogg", text: "Hello", lang: "en" }];
    const parsed = JSON.parse(formatJsonOutput(results));
    expect(parsed[0].audioLanguage).toBeUndefined();
    expect(parsed[0].lang).toBe("en");
  });
});

describe("resolveOutputFormat (#300 regression)", () => {
  // Pre-#300 bug: `--format toon` set args.format to the string but the
  // dispatch only checked the boolean args.toon flag, so output silently
  // fell through to plain text. Same class hit unknown --format values
  // and any cross-form mutex (e.g. --json --format toon). These tests
  // lock in the contract behind `resolveOutputFormat`.

  describe("boolean flags route to their format", () => {
    test("--json sets wantsJson", () => {
      const r = resolveOutputFormat({ json: true });
      expect(r.ok).toBe(true);
      if (r.ok) {
        expect(r.wantsJson).toBe(true);
        expect(r.wantsToon).toBe(false);
        expect(r.wantsTranscript).toBe(false);
      }
    });

    test("--toon sets wantsToon", () => {
      const r = resolveOutputFormat({ toon: true });
      expect(r.ok).toBe(true);
      if (r.ok) {
        expect(r.wantsToon).toBe(true);
        expect(r.wantsJson).toBe(false);
        expect(r.wantsTranscript).toBe(false);
      }
    });

    test("no flags → all false (default plain-text)", () => {
      const r = resolveOutputFormat({});
      expect(r.ok).toBe(true);
      if (r.ok) {
        expect(r.wantsJson).toBe(false);
        expect(r.wantsToon).toBe(false);
        expect(r.wantsTranscript).toBe(false);
      }
    });
  });

  describe("--format string is an alias for the boolean", () => {
    test("--format json", () => {
      const r = resolveOutputFormat({ format: "json" });
      expect(r.ok).toBe(true);
      if (r.ok) expect(r.wantsJson).toBe(true);
    });

    test("--format toon (the bug fixed in #300)", () => {
      const r = resolveOutputFormat({ format: "toon" });
      expect(r.ok).toBe(true);
      if (r.ok) {
        expect(r.wantsToon).toBe(true);
        expect(r.wantsJson).toBe(false);
        expect(r.wantsTranscript).toBe(false);
      }
    });

    test("--format transcript", () => {
      const r = resolveOutputFormat({ format: "transcript" });
      expect(r.ok).toBe(true);
      if (r.ok) {
        expect(r.wantsTranscript).toBe(true);
        expect(r.wantsJson).toBe(false);
        expect(r.wantsToon).toBe(false);
      }
    });
  });

  describe("mutex: --json + --toon are exclusive", () => {
    test("both booleans → error", () => {
      const r = resolveOutputFormat({ json: true, toon: true });
      expect(r.ok).toBe(false);
      if (!r.ok) expect(r.error).toContain("mutually exclusive");
    });

    test("boolean --json + --format toon → error (cross-form)", () => {
      const r = resolveOutputFormat({ json: true, format: "toon" });
      expect(r.ok).toBe(false);
      if (!r.ok) expect(r.error).toContain("mutually exclusive");
    });

    test("boolean --toon + --format json → error (cross-form)", () => {
      const r = resolveOutputFormat({ toon: true, format: "json" });
      expect(r.ok).toBe(false);
      if (!r.ok) expect(r.error).toContain("mutually exclusive");
    });

    test("--format transcript + --json → error (Greptile P2 on #300)", () => {
      // Pre-fix: wantsTranscript was set but the dispatch checked
      // wantsJson first → silent JSON output. Now rejected with a
      // symmetric mutex message.
      const r = resolveOutputFormat({ json: true, format: "transcript" });
      expect(r.ok).toBe(false);
      if (!r.ok) {
        expect(r.error).toContain("--format transcript");
        expect(r.error).toContain("mutually exclusive");
      }
    });

    test("--format transcript + --toon → error", () => {
      const r = resolveOutputFormat({ toon: true, format: "transcript" });
      expect(r.ok).toBe(false);
      if (!r.ok) expect(r.error).toContain("mutually exclusive");
    });
  });

  describe("unknown --format values are rejected", () => {
    test("--format gibberish → error", () => {
      const r = resolveOutputFormat({ format: "gibberish" });
      expect(r.ok).toBe(false);
      if (!r.ok) {
        expect(r.error).toContain("unknown --format 'gibberish'");
        expect(r.error).toContain("supported: transcript, json, toon");
      }
    });

    test("--format \"\" → error (empty string is not a valid value)", () => {
      const r = resolveOutputFormat({ format: "" });
      expect(r.ok).toBe(false);
      if (!r.ok) expect(r.error).toContain("unknown --format");
    });

    test("unknown format wins over mutex (clearer error first)", () => {
      // --json + --format gibberish: report the unknown format,
      // not the mutex — the user can't fix mutex until format is valid.
      const r = resolveOutputFormat({ json: true, format: "gibberish" });
      expect(r.ok).toBe(false);
      if (!r.ok) expect(r.error).toContain("unknown --format");
    });
  });

  describe("boolean + --format same value is harmless (idempotent)", () => {
    test("--json --format json → wantsJson true, no mutex", () => {
      const r = resolveOutputFormat({ json: true, format: "json" });
      expect(r.ok).toBe(true);
      if (r.ok) expect(r.wantsJson).toBe(true);
    });

    test("--toon --format toon → wantsToon true, no mutex", () => {
      const r = resolveOutputFormat({ toon: true, format: "toon" });
      expect(r.ok).toBe(true);
      if (r.ok) expect(r.wantsToon).toBe(true);
    });
  });
});
