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

## Compose with X/Twitter workflows

Kesha handles local voice input and speech output. To let an OpenClaw agent turn a transcribed voice brief into public X/Twitter research, tweet search, or a reviewed tweet draft, install [TweetClaw](https://github.com/Xquik-dev/tweetclaw) separately:

```bash
openclaw plugins install @xquik/tweetclaw
openclaw config set plugins.entries.tweetclaw.config.apiKey "$XQUIK_API_KEY"
openclaw config set tools.alsoAllow '["explore", "tweetclaw"]'
```

Useful voice-driven prompts:

- "Transcribe this voice note, search tweets about the named launch keywords, and summarize what people are saying."
- "Turn this voice memo into a tweet draft. Ask me before posting."
- "Read the latest monitor alert aloud as an OGG voice note."

Keep the Xquik API key out of voice transcripts, prompts, and shared logs. Review every TweetClaw write action before posting, replying, following, sending DMs, or changing account state.

Manage the plugin with `openclaw plugins list`, `openclaw plugins disable kesha-voice-kit`, or `openclaw plugins uninstall kesha-voice-kit`.
