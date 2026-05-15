/**
 * Darwin keeps Russian on AVSpeech Milena for the zero-install path.
 * Other Chatterbox-supported languages route to `<lang>-chatterbox-m01`.
 */
const RU_DARWIN_FALLBACK_VOICE = "macos-com.apple.voice.compact.ru-RU.Milena";

export const AUTO_VOICE_BY_LANG: Readonly<Record<string, string>> = Object.freeze({
  en: "en-am_michael",
  ar: "ar-chatterbox-m01",
  da: "da-chatterbox-m01",
  de: "de-chatterbox-m01",
  el: "el-chatterbox-m01",
  es: "es-chatterbox-m01",
  fi: "fi-chatterbox-m01",
  fr: "fr-chatterbox-m01",
  he: "he-chatterbox-m01",
  hi: "hi-chatterbox-m01",
  it: "it-chatterbox-m01",
  ja: "ja-chatterbox-m01",
  ko: "ko-chatterbox-m01",
  ms: "ms-chatterbox-m01",
  nl: "nl-chatterbox-m01",
  no: "no-chatterbox-m01",
  pl: "pl-chatterbox-m01",
  pt: "pt-chatterbox-m01",
  ru: "ru-chatterbox-m01",
  sv: "sv-chatterbox-m01",
  sw: "sw-chatterbox-m01",
  tr: "tr-chatterbox-m01",
  zh: "zh-chatterbox-m01",
});

/** Map a detected language code to a default voice id. Unknown / low-confidence → undefined. */
export function pickVoiceForLang(
  code: string | undefined,
  confidence: number,
  platform: NodeJS.Platform = process.platform,
): string | undefined {
  if (!code || confidence < 0.5) return undefined;
  if (code === "ru" && platform === "darwin") {
    return RU_DARWIN_FALLBACK_VOICE;
  }
  return AUTO_VOICE_BY_LANG[code];
}
