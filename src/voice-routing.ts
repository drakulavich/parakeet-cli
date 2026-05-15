/**
 * Darwin keeps Russian on AVSpeech Milena for the zero-install path.
 * Other Chatterbox-supported languages route to `<lang>-chatterbox-m01`.
 */
const RU_DARWIN_FALLBACK_VOICE = "macos-com.apple.voice.compact.ru-RU.Milena";
const CHATTERBOX_LANGS = new Set([
  "ar", "da", "de", "el", "es", "fi", "fr", "he", "hi", "it", "ja", "ko",
  "ms", "nl", "no", "pl", "pt", "ru", "sv", "sw", "tr", "zh",
]);

/** Map a detected language code to a default voice id. Unknown / low-confidence → undefined. */
export function pickVoiceForLang(
  code: string | undefined,
  confidence: number,
  platform: NodeJS.Platform = process.platform,
): string | undefined {
  if (!code || confidence < 0.5) return undefined;
  switch (code) {
    case "en":
      return "en-am_michael";
    case "ru":
      return platform === "darwin" ? RU_DARWIN_FALLBACK_VOICE : "ru-chatterbox-m01";
    default:
      if (CHATTERBOX_LANGS.has(code)) {
        return `${code}-chatterbox-m01`;
      }
      return undefined;
  }
}
