# Hermes Agent Integration

Kesha Voice Kit can run as a local voice backend for
[Hermes Agent](https://hermes-agent.nousresearch.com/) through Hermes' command
provider surfaces. No API keys are required; audio stays on the machine where
Hermes runs.

Hermes exposes two useful integration points:

- **Voice message transcription (STT)** through `HERMES_LOCAL_STT_COMMAND`.
- **Text-to-speech (TTS)** through `tts.providers.<name>.type: command`.

## Install Kesha

```bash
bun add -g @drakulavich/kesha-voice-kit
kesha install
```

Install TTS models only if you want Hermes replies as audio:

```bash
kesha install --tts
```

## Speech-to-text

Hermes writes each incoming voice message to `{input_path}`, runs the command
template, then reads a `.txt` transcript from `{output_dir}`.

```bash
export HERMES_LOCAL_STT_COMMAND='sh -c '"'"'kesha --format transcript "$1" > "$2/transcript.txt"'"'"' sh {input_path} {output_dir}'
```

In `~/.hermes/config.yaml`:

```yaml
stt:
  provider: local_command
```

Use plain text output if you do not want the `[lang: ...]` footer:

```bash
export HERMES_LOCAL_STT_COMMAND='sh -c '"'"'kesha "$1" > "$2/transcript.txt"'"'"' sh {input_path} {output_dir}'
```

## Text-to-speech

Hermes command providers write input text to `{input_path}` and expect the
configured command to create audio at `{output_path}`. Kesha's `say` command
already reads text from stdin, so no wrapper script is required.

In `~/.hermes/config.yaml`:

```yaml
tts:
  provider: kesha
  providers:
    kesha:
      type: command
      command: "kesha say --format ogg-opus --out {output_path} < {input_path}"
      output_format: ogg
      timeout: 60
      voice_compatible: true
      max_text_length: 2000
```

To force a specific Kesha voice, add `--voice`:

```yaml
tts:
  provider: kesha-ru
  providers:
    kesha-ru:
      type: command
      command: "kesha say --voice ru-vosk-m02 --format ogg-opus --out {output_path} < {input_path}"
      output_format: ogg
      timeout: 60
      voice_compatible: true
```

## Verify

```bash
which kesha
kesha status

tmp="$(mktemp -d)"
# Replace /path/to/audio.ogg with your own audio file:
kesha --format transcript /path/to/audio.ogg > "$tmp/transcript.txt"
cat "$tmp/transcript.txt"

echo "Hello from Hermes" | kesha say --format ogg-opus --out "$tmp/reply.ogg"
test -s "$tmp/reply.ogg"
```

## Notes

- `kesha --format transcript` keeps the transcript on stdout and progress or
  warnings on stderr, which matches Hermes' command contract.
- `kesha say --format ogg-opus` emits messenger-ready OGG/Opus directly.
- Hermes command providers run trusted local shell commands with the user's
  permissions. Keep the command template in your own config or deployment
  automation.

Hermes reference docs:
[Voice & TTS](https://hermes-agent.nousresearch.com/docs/user-guide/features/tts/)
and
[Build a Hermes Plugin](https://hermes-agent.nousresearch.com/docs/guides/build-a-hermes-plugin/).
