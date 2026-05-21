# bash completion for kesha.
# Source this file or copy it into your bash-completion directory.

_kesha_completion() {
  local cur command opts commands
  COMPREPLY=()
  cur="${COMP_WORDS[COMP_CWORD]}"
  command="${COMP_WORDS[1]}"
  commands="completions doctor init install manpage record say stats status support-bundle"

  if [[ "$COMP_CWORD" -eq 1 ]]; then
    if [[ "$cur" == -* ]]; then
      COMPREPLY=( $(compgen -W "--help -h --version -v --json --toon --timestamps --speakers --include-errors --verbose --format --lang --debug --vad --no-vad" -- "$cur") )
    else
      COMPREPLY=( $(compgen -W "$commands" -- "$cur") )
    fi
    return 0
  fi

  case "$command" in
    completions) opts="--help -h" ;;
    doctor) opts="--help -h --json --redact" ;;
    init) opts="--help -h --coreml --onnx --no-cache --plan --yes --tts --vad --diarize" ;;
    install) opts="--help -h --coreml --onnx --no-cache --plan --tts --vad --diarize" ;;
    manpage) opts="--help -h" ;;
    record) opts="--help -h --out --max-seconds --debug" ;;
    say) opts="--help -h --voice --lang --out --rate --list-voices --ssml --format --bitrate --sample-rate --no-expand-abbrev --verbose --debug" ;;
    stats) opts="--help -h --format" ;;
    status) opts="--help -h --disk" ;;
    support-bundle) opts="--help -h --output" ;;
    *) opts="--help -h --version -v --json --toon --timestamps --speakers --include-errors --verbose --format --lang --debug --vad --no-vad" ;;
  esac

  if [[ "$cur" == -* ]]; then
    COMPREPLY=( $(compgen -W "$opts" -- "$cur") )
  fi
}

complete -F _kesha_completion kesha
