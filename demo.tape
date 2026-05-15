# VHS tape for the README demo GIF.
# Run with: vhs demo.tape
# Output:    demo.gif (committed at repo root, referenced by README.md).
#
# Shows the hero use case: transcribe English + Russian audio with
# automatic language detection, no flags beyond `--format transcript`.

Output demo.gif

# Width 1000 fits the English transcript line ("Please check your email...
# about the deployment.") without wrapping; Height 400 is sized to the
# 4-5 visible lines of two-command output.
Set Width 1000
Set Height 400

Set FontSize 14
Set Theme "Catppuccin Mocha"
Set Padding 16

# Slight typing slowdown so the viewer can register the command before
# the transcription output appears.
Set TypingSpeed 60ms

Sleep 800ms

# English transcription — short voice message, clean output.
Type "kesha --format transcript fixtures/benchmark-en/01-check-email.ogg"
Sleep 400ms
Enter
Sleep 4500ms

# Russian transcription — same shape, different language. The
# `[lang: ru, confidence: 1.00]` tail demonstrates automatic language
# detection without any flags.
Type "kesha --format transcript fixtures/benchmark/03-ty-dobavil-sebe-v-pamyat.ogg"
Sleep 400ms
Enter
Sleep 4000ms
