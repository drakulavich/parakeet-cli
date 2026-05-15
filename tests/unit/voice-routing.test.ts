import { describe, it, expect } from "bun:test";
import { pickVoiceForLang } from "../../src/voice-routing";

describe("pickVoiceForLang (auto-routing)", () => {
  it("returns en-am_michael for English with high confidence", () => {
    expect(pickVoiceForLang("en", 0.95)).toBe("en-am_michael");
  });

  it("returns Milena for Russian on darwin (zero-install AVSpeech path)", () => {
    expect(pickVoiceForLang("ru", 0.95, "darwin")).toBe(
      "macos-com.apple.voice.compact.ru-RU.Milena",
    );
  });

  it("falls back to Chatterbox for Russian on non-darwin", () => {
    expect(pickVoiceForLang("ru", 0.95, "linux")).toBe("ru-chatterbox-m01");
    expect(pickVoiceForLang("ru", 0.95, "win32")).toBe("ru-chatterbox-m01");
  });

  it("routes Chatterbox-supported languages", () => {
    expect(pickVoiceForLang("de", 0.95, "linux")).toBe("de-chatterbox-m01");
    expect(pickVoiceForLang("fr", 0.95, "linux")).toBe("fr-chatterbox-m01");
    expect(pickVoiceForLang("zh", 0.95, "linux")).toBe("zh-chatterbox-m01");
    expect(pickVoiceForLang("de", 0.95, "darwin")).toBe("de-chatterbox-m01");
  });

  it("returns undefined below 0.5 confidence (too ambiguous)", () => {
    expect(pickVoiceForLang("ru", 0.3)).toBeUndefined();
  });

  it("returns undefined for unsupported languages", () => {
    expect(pickVoiceForLang("uk", 0.95)).toBeUndefined();
    expect(pickVoiceForLang("cs", 0.95)).toBeUndefined();
  });

  it("returns undefined when code is missing", () => {
    expect(pickVoiceForLang(undefined, 0.95)).toBeUndefined();
    expect(pickVoiceForLang("", 0.95)).toBeUndefined();
  });
});
