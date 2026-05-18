# Product positioning

Kesha Voice Kit is a local-first voice toolkit for developers and agent workflows. It is built for small, scriptable jobs where a CLI can turn audio into text, text into audio, or voice messages into structured data without sending content to a hosted API.

The product promise is not "one model for every audio problem." The promise is a boring, automatable local voice stack with clear platform limits, predictable install paths, and machine-readable output.

## Who this is for

- Developers and agent builders who need local speech-to-text for voice messages, meetings, support clips, or batch audio files.
- OpenClaw users who want private, low-latency voice-message transcription through a CLI model route.
- Hermes Agent users who want command-provider STT/TTS without cloud audio uploads.
- macOS users who want the fastest path on Apple Silicon through CoreML, with a CPU/ONNX fallback on Linux and Windows.
- Automation-heavy users who prefer stdout/stderr contracts, JSON/TOON output, and shell-friendly commands over a hosted dashboard.
- Projects that can accept explicit model downloads and local cache management in exchange for no API keys and no cloud audio upload.

## Primary workflows

| Workflow | Status | Command surface |
|---|---|---|
| Transcribe one or more audio files to plain text | Stable | `kesha audio.ogg` |
| Transcribe to structured output for scripts or agents | Stable | `kesha --json audio.ogg`, `kesha --toon audio.ogg`, `kesha --format transcript audio.ogg` |
| Detect likely audio/text language during transcription | Stable | `--lang`, `--verbose`, JSON/TOON language fields |
| Skip silence in long audio | Beta | `kesha --vad meeting.m4a`, auto-on for long audio when VAD is installed |
| Synthesize English or Russian speech locally | Beta | `kesha say "text"`, `kesha install --tts` |
| Use macOS system voices without model downloads | Stable on macOS | `kesha say --voice macos-*` |
| Label speakers in meeting transcripts | Preview, darwin-arm64 only | `kesha --json --vad --speakers meeting.m4a` |
| Integrate with OpenClaw as a local voice model | Stable integration surface, user-configured route | `docs/openclaw.md` |
| Integrate with Hermes Agent as local STT/TTS commands | Stable CLI surface, user-configured route | `docs/hermes.md` |
| Use Raycast actions for selected-file transcription or clipboard speech | Beta, macOS only | `raycast/` extension |
| Install through Nix instead of Bun/npm | Beta, selected systems | `docs/nix-install.md` |

## Non-goals

- Hosted SaaS transcription, dashboards, accounts, billing, or team management.
- Streaming phone-call transcription or real-time conversation infrastructure.
- Speaker identity across files. Diarization emits per-file cluster IDs, not names or persistent voice profiles.
- Full SSML coverage. Kesha supports a practical subset for current TTS engines; unsupported tags should fail or warn clearly.
- Universal TTS across all 25 STT languages. TTS is intentionally scoped to English, Russian, and macOS system voices.
- Model training, fine-tuning, or benchmark leadership on every dataset.
- Browser/mobile SDKs. The supported product surface is CLI-first, with thin integrations around it.

## Feature maturity

| Feature | Maturity | Notes |
|---|---|---|
| Core STT CLI | Stable | Main product path; stdout transcript, stderr progress/errors. |
| JSON / TOON / transcript output formats | Stable | Intended for scripts, agents, and downstream tooling. |
| Audio language detection | Stable | Used for warnings and structured metadata. |
| VAD preprocessing | Beta | Useful for long or silence-heavy audio; short voice messages stay on the fast path. |
| English/Russian TTS | Beta | Requires `kesha install --tts`; model cache and first-run cost are expected. |
| macOS AVSpeech voices | Stable on macOS | Zero-install system voices; quality is OS voice quality, not neural TTS. |
| SSML subset | Preview | `<prosody rate>`, Russian stress markers, and IPA support are engine-specific. |
| Speaker diarization | Preview | darwin-arm64 only; cluster IDs are stable within one file only. |
| Raycast extension | Beta | Convenience UI over the CLI; macOS-only. |
| OpenClaw plugin | Stable integration surface | Discovery is packaged; actual audio route remains explicit user config. |
| Hermes Agent command provider | Stable CLI surface | Hermes owns the command-provider lifecycle; Kesha supplies the local STT/TTS commands. |
| Nix flake | Beta | Reproducible alternate install path for selected systems; Bun/npm remains canonical. |

## Platform matrix

| Capability | macOS arm64 | macOS x64 | Linux x64 | Windows x64 | Nix path |
|---|---|---|---|---|---|
| STT | Supported, CoreML path | Not shipped | Supported, ONNX CPU path | Supported, ONNX CPU path | `aarch64-darwin`, `x86_64-linux` |
| Audio language detection | Supported | Not shipped | Supported | Supported | Supported where the flake builds |
| VAD | Supported with `kesha install --vad` | Not shipped | Supported with `kesha install --vad` | Supported with `kesha install --vad` | Supported where the engine path includes VAD assets |
| TTS: English Kokoro | Supported, FluidAudio/CoreML in release builds | Not shipped | Supported, ONNX path | Supported, ONNX path | Supported except where noted in `docs/nix-install.md` |
| TTS: Russian Vosk-TTS | Supported | Not shipped | Supported | Supported | Supported except where noted in `docs/nix-install.md` |
| macOS system voices | Supported | Not shipped | Not applicable | Not applicable | Supported on `aarch64-darwin` |
| Speaker diarization | Preview, darwin-arm64 only | Not supported | Not supported | Not supported | Not wired into the Nix build yet |
| Raycast extension | Supported | Not shipped | Not applicable | Not applicable | Not applicable |
| OpenClaw integration | Supported through CLI route | Not shipped | Supported through CLI route | Supported through CLI route | Use the installed `kesha` command from the chosen path |
| Hermes Agent integration | Supported through command providers | Not shipped | Supported through command providers | Supported through command providers | Use the installed `kesha` command from the chosen path |

`macOS x64` means Intel Macs. Kesha does not currently publish a `darwin-x64` engine binary, so Intel Macs are intentionally marked as not shipped rather than implied to use the ONNX fallback.

## When to choose something else

Use a hosted transcription provider when you need managed uptime, centralized audit logs, team permissions, or no local model cache. Use a streaming ASR stack when partial transcripts during a live call matter more than local-first batch reliability. Use a domain-specific ASR model when medical, legal, or noisy industrial audio quality is the primary requirement.

Kesha is strongest when the input is already a file, the caller is an engineer or agent, and the desired output is a transcript, JSON object, TOON payload, or WAV file that can continue through a local pipeline.
