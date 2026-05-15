# VHS tape for the README demo GIF.
# Run with: vhs demo.tape
# Output:    demo.gif (committed at repo root, referenced by README.md).
#
# Two consecutive transcriptions on two languages, two output formats:
#   1. English voice message → `--format transcript`   (human-friendly)
#   2. Russian voice message → `--json`                (machine/agent)
#
# Demonstrates: multilingual ASR, automatic language detection, and the
# fact that the same engine answers in whichever shape the consumer asks
# for (transcript line + lang tail vs structured JSON envelope).

Output demo.gif

Set Shell "bash"

# Larger canvas so JSON output fits without wrapping on small fonts.
Set Width 1500
Set Height 940
Set FontSize 22
Set Theme "Catppuccin Mocha"
Set Padding 24

# Slight typing slowdown so the viewer can register the command before
# the transcription output appears.
Set TypingSpeed 60ms

Sleep 1s

# 1) English transcript — hero one-liner. Voice message in, plain text +
#    `[lang: en, confidence: 1.00]` tail out.
Type "kesha --format transcript fixtures/benchmark-en/01-check-email.ogg"
Sleep 500ms
Enter
# Sleep long enough for the user to read the 2-line output comfortably.
Sleep 6s

# 2) Russian JSON — same engine, different consumer. Stable JSON envelope
#    with `text`, `lang`, `audioLanguage`, `textLanguage` — the shape
#    agents (OpenClaw skill, MCP tools) consume.
Type "kesha --json fixtures/benchmark/03-ty-dobavil-sebe-v-pamyat.ogg"
Sleep 500ms
Enter
# JSON output is ~14 lines; give the viewer enough time to scan it.
Sleep 8s
