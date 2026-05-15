/**
 * Darwin defaults to AVSpeech Milena — zero install, no model download required.
 * Linux/Windows fall through to Chatterbox `ru-chatterbox-m01`.
 */
const RU_DARWIN_FALLBACK_VOICE = "macos-com.apple.voice.compact.ru-RU.Milena";

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
      return undefined;
  }
}
