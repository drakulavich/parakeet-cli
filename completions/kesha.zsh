#compdef kesha
# zsh completion for kesha.

_kesha() {
  local -a commands
  commands=(
    'completions:Print shell completion script for bash, zsh, or fish'
    'doctor:Collect support diagnostics without changing local state'
    'install:Download inference engine and models'
    'manpage:Print the kesha(1) manpage'
    'record:Record microphone audio to a WAV file'
    'say:Synthesize speech from text (TTS). Writes audio to stdout (or --out file). Defaults to WAV; use --format ogg-opus for messenger-ready voice notes.'
    'stats:Manage local anonymous Kesha Stats'
    'status:Show backend installation status'
    'support-bundle:Create a redacted diagnostics archive for support'
  )

  if (( CURRENT == 2 )); then
    if [[ "$words[CURRENT]" == -* ]]; then
      _arguments '--help[Show help]' \
      '-h[Show help]' \
      '--version[Show version]' \
      '-v[Show version]' \
      '--json[Output results as JSON]' \
      '--toon[Output results as TOON (compact, LLM-friendly encoding of the same data as --json)]' \
      '--timestamps[Include timestamped transcript segments in JSON/TOON output]' \
      '--speakers[Include speaker labels in transcript segments. Requires --json / --toon / --format json. Implies --timestamps. Currently darwin-arm64 only (#199).]' \
      '--include-errors[With --json, output { results, errors } so scripts can read per-file failures without parsing stderr]' \
      '--verbose[Show language detection details]' \
      '--format=[Output format: transcript | json | toon (long-form alias for --json / --toon)]:format:' \
      '--lang=[Expected language code (ISO 639-1), warn if mismatch]:lang:' \
      '--debug[Trace engine subprocess calls on stderr (or KESHA_DEBUG=1)]' \
      '--vad[Force Silero VAD preprocessing (kesha install --vad first). Without this, VAD auto-engages on audio ≥ 120s.]' \
      '--no-vad[Disable VAD preprocessing regardless of duration or install state]'
    else
      _describe -t commands 'kesha command' commands
    fi
    return
  fi

  case "$words[2]" in
    completions)
      _arguments '--help[Show help]' \
        '-h[Show help]'
      ;;
    doctor)
      _arguments '--help[Show help]' \
        '-h[Show help]' \
        '--json[Output diagnostics as JSON]' \
        '--redact[Redact secrets and user-home paths from diagnostic output]'
      ;;
    install)
      _arguments '--help[Show help]' \
        '-h[Show help]' \
        '--coreml[Force CoreML backend (macOS arm64)]' \
        '--onnx[Force ONNX backend]' \
        '--no-cache[Re-download even if cached]' \
        '--plan[Show download, disk, and warm-up plan without changing local state]' \
        '--tts[Also install TTS models (Kokoro EN + Vosk-TTS RU, ~990MB)]' \
        '--vad[Also install Silero VAD (~2.3MB) for long-audio preprocessing]' \
        '--diarize[Also install the Sortformer streaming-diarization model (~245MB, darwin-arm64 only — #199)]'
      ;;
    manpage)
      _arguments '--help[Show help]' \
        '-h[Show help]'
      ;;
    record)
      _arguments '--help[Show help]' \
        '-h[Show help]' \
        '--out=[Write recorded WAV audio to this path]:out:' \
        '--max-seconds=[Maximum recording duration in seconds]:max seconds:' \
        '--debug[Trace engine subprocess calls on stderr (or KESHA_DEBUG=1)]'
      ;;
    say)
      _arguments '--help[Show help]' \
        '-h[Show help]' \
        '--voice=[Voice id, e.g. en-am_michael]:voice:' \
        '--lang=[BCP 47 language code (default en-us)]:lang:' \
        '--out=[Write audio to file instead of stdout]:out:' \
        '--rate=[Speaking rate 0.5–2.0]:rate:' \
        '--list-voices[List installed voices and exit]' \
        '--ssml[Parse input as SSML (supports <speak>, <break>; strips unknown tags)]' \
        '--format=[Output format: wav (default) or ogg-opus (Telegram-ready voice note). Inferred from --out extension when omitted.]:format:' \
        '--bitrate=[Opus bitrate in bits/sec (e.g. 32000). Only with --format ogg-opus.]:bitrate:' \
        '--sample-rate=[Opus encoder sample rate (8000/12000/16000/24000/48000). Only with --format ogg-opus.]:sample rate:' \
        '--no-expand-abbrev[Disable Russian acronym auto-expansion (ВОЗ → '\''вэ о зэ'\'') for ru-vosk-* voices. <say-as interpret-as='\''characters'\''> still works. Applies to Russian (ru-vosk-*) and English (en-*) voices.]' \
        '--verbose[Log TTS synthesis time to stderr]' \
        '--debug[Trace engine subprocess calls on stderr (or KESHA_DEBUG=1)]'
      ;;
    stats)
      _arguments '--help[Show help]' \
        '-h[Show help]' \
        '--format=[Export format: json | csv]:format:'
      ;;
    status)
      _arguments '--help[Show help]' \
        '-h[Show help]' \
        '--disk[Include recursive cache disk usage]'
      ;;
    support-bundle)
      _arguments '--help[Show help]' \
        '-h[Show help]' \
        '--output=[Write archive to this .tar.gz path]:output:'
      ;;
    *)
      _arguments '--help[Show help]' \
      '-h[Show help]' \
      '--version[Show version]' \
      '-v[Show version]' \
      '--json[Output results as JSON]' \
      '--toon[Output results as TOON (compact, LLM-friendly encoding of the same data as --json)]' \
      '--timestamps[Include timestamped transcript segments in JSON/TOON output]' \
      '--speakers[Include speaker labels in transcript segments. Requires --json / --toon / --format json. Implies --timestamps. Currently darwin-arm64 only (#199).]' \
      '--include-errors[With --json, output { results, errors } so scripts can read per-file failures without parsing stderr]' \
      '--verbose[Show language detection details]' \
      '--format=[Output format: transcript | json | toon (long-form alias for --json / --toon)]:format:' \
      '--lang=[Expected language code (ISO 639-1), warn if mismatch]:lang:' \
      '--debug[Trace engine subprocess calls on stderr (or KESHA_DEBUG=1)]' \
      '--vad[Force Silero VAD preprocessing (kesha install --vad first). Without this, VAD auto-engages on audio ≥ 120s.]' \
      '--no-vad[Disable VAD preprocessing regardless of duration or install state]'
      ;;
  esac
}

_kesha "$@"
