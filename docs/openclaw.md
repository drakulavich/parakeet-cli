# OpenClaw Integration

Kesha Voice Kit ships as a plugin for [OpenClaw](https://github.com/openclaw/openclaw) — give your LLM agent ears. No API keys, everything runs locally on your machine.

```bash
bun add -g @drakulavich/kesha-voice-kit && kesha install
openclaw plugins install @drakulavich/kesha-voice-kit
openclaw config set tools.media.audio.models \
  '[{"type":"cli","command":"kesha","args":["--format","transcript","{{MediaPath}}"],"timeoutSeconds":15}]'
```

> If audio transcription is not already enabled: `openclaw config set tools.media.audio.enabled true`

For agents that need per-utterance timestamps (chapters, navigation, downstream editing), append `--json --timestamps` instead of `--format transcript` (requires engine v1.9.0+):
```bash
openclaw config set tools.media.audio.models \
  '[{"type":"cli","command":"kesha","args":["--json","--timestamps","{{MediaPath}}"],"timeoutSeconds":30}]'
```
Each segment carries `start`, `end`, and `text`. VAD-preprocessed long files yield one segment per utterance; short non-VAD files return one whole-file segment.

Your agent receives a voice message in Telegram/WhatsApp/Slack, Kesha transcribes it locally, and the agent sees enriched context:

```
Таити, Таити! Не были мы ни в какой Таити! Нас и тут неплохо кормят.
[lang: ru, confidence: 1.00]
```

Manage the plugin with `openclaw plugins list`, `openclaw plugins disable kesha-voice-kit`, or `openclaw plugins uninstall kesha-voice-kit`.
