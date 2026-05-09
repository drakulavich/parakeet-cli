# Kesha Voice Kit Changelog

## [Initial Version] - {PR_MERGED_AT}

- Add **Transcribe Selected Audio** command — transcribes the audio file selected in Finder using the local `kesha` CLI, shows transcript + detected language, pre-copies to clipboard.
- Add **Speak Clipboard** command — synthesizes the current clipboard text via `kesha say` and plays it through the default output; voice auto-routed by detected language (Kokoro for English, Vosk-TTS for Russian, AVSpeech for macOS system voices).
- Preferences for overriding the `kesha` binary path and default voice. Default-voice placeholder lists the male Kokoro / Vosk picks (`en-am_michael`, `ru-vosk-m02`) per the project's brand-voice rule.
- Recommended CLI: `@drakulavich/kesha-voice-kit@1.11.0` or newer (engine v1.11.0+). Highlights for users running `kesha` from the terminal alongside the extension: timestamped transcript segments (`kesha --json --timestamps`), English acronym auto-expansion + IPA injection for Kokoro voices (EPAM, JSON, Anthropic, Microsoft, Kubernetes, …), Russian abbreviation auto-expansion + `<emphasis>` stress markers for Vosk-TTS, OGG/Opus voice notes, and a hand-rolled IEEE_FLOAT WAV writer that plays in both ears on stereo expanders.
