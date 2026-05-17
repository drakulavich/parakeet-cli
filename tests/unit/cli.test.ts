import { describe, test, expect } from "bun:test";
import { renderUsage } from "citty";
import { decode as decodeToon } from "@toon-format/toon";
import { mainCommand, doctorCommand, installCommand, statusCommand, statsCommand, supportBundleCommand, sayCommand, formatTextOutput, formatJsonOutput, formatToonOutput, detectLanguage, checkLanguageMismatch, resolveOutputFormat } from "../../src/cli";

describe("CLI help", () => {
  test("main help contains usage and install info", async () => {
    const usage = await renderUsage(mainCommand);
    expect(usage).toContain("USAGE");
    expect(usage).toContain("install");
  });

  test("main help shows subcommand inventory (#324)", async () => {
    const usage = await renderUsage(mainCommand);
    expect(usage).toContain("Commands:");
    expect(usage).toContain("doctor     Collect support diagnostics.");
    expect(usage).toContain("install    Download engine and models.");
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

  test("main help contains --json flag", async () => {
    const usage = await renderUsage(mainCommand);
    expect(usage).toContain("--json");
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

  test("stats help has command description", async () => {
    const usage = await renderUsage(statsCommand);
    expect(usage).toContain("stats");
    expect(usage).toContain("enable");
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
