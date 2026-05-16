import { describe, it, expect } from "bun:test";
import { AUTO_VOICE_BY_LANG, pickVoiceForLang, resolveSayVoice } from "../../src/voice-routing";

const CHATTERBOX_LANGS_EXCEPT_EN = [
  "ar", "da", "de", "el", "es", "fi", "fr", "he", "hi", "it", "ja", "ko",
  "ms", "nl", "no", "pl", "pt", "ru", "sv", "sw", "tr", "zh",
];

describe("pickVoiceForLang (auto-routing)", () => {
  it("keeps an explicit auto-routing dictionary", () => {
    expect(AUTO_VOICE_BY_LANG.en).toBe("en-am_michael");
    for (const lang of CHATTERBOX_LANGS_EXCEPT_EN) {
      expect(AUTO_VOICE_BY_LANG[lang]).toBe(`${lang}-chatterbox-m01`);
    }
  });

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

describe("resolveSayVoice", () => {
  it("uses --lang as a default voice shortcut when --voice is omitted", () => {
    expect(resolveSayVoice({ explicitLang: "de", platform: "linux" })).toBe(
      "de-chatterbox-m01",
    );
    expect(resolveSayVoice({ explicitLang: "en", platform: "linux" })).toBe("en-am_michael");
  });

  it("lets explicit --voice win over --lang", () => {
    expect(resolveSayVoice({
      explicitVoice: "fr-chatterbox-m01",
      explicitLang: "de",
      platform: "linux",
    })).toBe("fr-chatterbox-m01");
  });

  it("does not invent a voice for unsupported --lang values", () => {
    expect(resolveSayVoice({ explicitLang: "uk", platform: "linux" })).toBeUndefined();
  });

  it("uses detected language only when neither --voice nor --lang is present", () => {
    expect(resolveSayVoice({
      detectedCode: "de",
      detectedConfidence: 0.95,
      platform: "linux",
    })).toBe("de-chatterbox-m01");
  });

  it("keeps darwin Russian on Milena until Chatterbox is installed", () => {
    expect(resolveSayVoice({
      explicitLang: "ru",
      platform: "darwin",
      chatterboxInstalled: false,
    })).toBe("macos-com.apple.voice.compact.ru-RU.Milena");
    expect(resolveSayVoice({
      explicitLang: "ru",
      platform: "darwin",
      chatterboxInstalled: true,
    })).toBe("ru-chatterbox-m01");
  });
});
