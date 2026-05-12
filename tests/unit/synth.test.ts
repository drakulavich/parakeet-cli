import { describe, it, expect } from "bun:test";
import { buildSayArgs } from "../../src/synth";

describe("buildSayArgs", () => {
  it("starts with the 'say' subcommand", () => {
    expect(buildSayArgs({})[0]).toBe("say");
  });

  it("appends text as a trailing positional", () => {
    expect(buildSayArgs({ text: "Hello" })).toContain("Hello");
  });

  it("omits empty text (caller will pipe via stdin)", () => {
    expect(buildSayArgs({ text: "" })).toEqual(["say"]);
  });

  it("omits undefined text (caller will pipe via stdin)", () => {
    expect(buildSayArgs({})).toEqual(["say"]);
  });

  it("passes --voice when given", () => {
    expect(buildSayArgs({ text: "Hi", voice: "en-am_michael" })).toEqual(
      expect.arrayContaining(["--voice", "en-am_michael"]),
    );
  });

  it("passes --lang when given", () => {
    expect(buildSayArgs({ text: "Hi", lang: "en-gb" })).toEqual(
      expect.arrayContaining(["--lang", "en-gb"]),
    );
  });

  it("passes --out when given", () => {
    expect(buildSayArgs({ text: "Hi", out: "reply.wav" })).toEqual(
      expect.arrayContaining(["--out", "reply.wav"]),
    );
  });

  it("omits --rate when default (1.0)", () => {
    expect(buildSayArgs({ text: "Hi", rate: 1.0 })).not.toContain("--rate");
  });

  it("includes --rate when non-default", () => {
    expect(buildSayArgs({ text: "Hi", rate: 1.25 })).toEqual(
      expect.arrayContaining(["--rate", "1.25"]),
    );
  });

  it("omits --ssml when false or undefined", () => {
    expect(buildSayArgs({ text: "hi" })).not.toContain("--ssml");
    expect(buildSayArgs({ text: "hi", ssml: false })).not.toContain("--ssml");
  });

  it("includes --ssml when true", () => {
    const args = buildSayArgs({ text: "<speak>hi</speak>", ssml: true });
    expect(args).toContain("--ssml");
  });
});

describe("--no-expand-abbrev (#232)", () => {
  const baseOpts = {
    voice: "ru-vosk-m02",
    out: "/tmp/x.wav",
    text: "ВОЗ",
  };

  it("not present by default", () => {
    const args = buildSayArgs({
      ...baseOpts,
      noExpandAbbrev: false,
    }, { protocolVersion: 1, backend: "onnx", features: ["tts", "tts.ru_acronym_expansion"] });
    expect(args).not.toContain("--no-expand-abbrev");
  });

  it("forwarded when flag is set and engine supports it", () => {
    const args = buildSayArgs({
      ...baseOpts,
      noExpandAbbrev: true,
    }, { protocolVersion: 1, backend: "onnx", features: ["tts", "tts.ru_acronym_expansion"] });
    expect(args).toContain("--no-expand-abbrev");
  });

  it("dropped from argv with a warning when engine lacks the capability (#275 D3)", () => {
    // The drop is no longer silent — `buildSayArgs` emits a `log.warn` so
    // the user notices their flag had no effect. We can't intercept the
    // colored stderr from bun:test cleanly, so we only assert the argv
    // shape here; the warn-call is exercised by the e2e tests on stderr.
    const args = buildSayArgs({
      ...baseOpts,
      noExpandAbbrev: true,
    }, { protocolVersion: 1, backend: "onnx", features: ["tts"] });
    expect(args).not.toContain("--no-expand-abbrev");
  });
});

