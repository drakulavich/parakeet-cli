# OpenClaw Integration

Kesha Voice Kit ships as a plugin for [OpenClaw](https://github.com/openclaw/openclaw) — give your LLM agent ears. No API keys, everything runs locally on your machine.

```bash
bun add -g @drakulavich/kesha-voice-kit && kesha install
openclaw plugins install @drakulavich/kesha-voice-kit
openclaw config patch --stdin <<'JSON5'
{
  tools: {
    media: {
      audio: {
        enabled: true,
        models: [
          {
            type: "cli",
            command: "kesha",
            args: ["{{MediaPath}}"],
            timeoutSeconds: 15,
          },
        ],
        echoTranscript: true,
        echoFormat: '🦜 "{transcript}"',
      },
    },
  },
}
JSON5
```

The default setup echoes each transcript back to the originating chat as `🦜 "{transcript}"` before the agent responds.

For agents that need per-utterance timestamps (chapters, navigation, downstream editing), use `--json --timestamps` instead of the default bare transcript output (requires engine v1.9.0+):
```bash
openclaw config set tools.media.audio.models \
  '[{"type":"cli","command":"kesha","args":["--json","--timestamps","{{MediaPath}}"],"timeoutSeconds":30}]'
```
Each segment carries `start`, `end`, and `text`. VAD-preprocessed long files yield one segment per utterance; short non-VAD files return one whole-file segment.

Your agent receives a voice message in Telegram/WhatsApp/Slack, Kesha transcribes it locally, and the agent sees the bare transcript text:

```
Таити, Таити! Не были мы ни в какой Таити! Нас и тут неплохо кормят.
```

Manage the plugin with `openclaw plugins list`, `openclaw plugins disable kesha-voice-kit`, or `openclaw plugins uninstall kesha-voice-kit`.
