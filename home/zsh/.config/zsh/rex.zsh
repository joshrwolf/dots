# Rex shell integration, sourced only in Rex terminals.
#
# OSC 133 marks prompts and commands, so Rex drops records a crashed program
# left behind and can jump between prompts. OSC 7501 reports commands: working
# while one runs, then done or error with its duration once it took long enough
# to have been worth waiting for. A quick command needs no report; the next
# prompt mark clears its working record.

# Seconds a command must run before its result is reported.
typeset -gi DOTS_STATUS_AFTER=${DOTS_STATUS_AFTER:-10}
# Programs that report their own status or are used interactively; their
# runs are never reported.
typeset -ga DOTS_STATUS_IGNORE=(
  claude codex pi
  nvim vim vi less man more ssh mosh top htop btop watch tail
  zsh bash fish exit lazygit tig fzf rex dots-pick
)

typeset -g _dots_status_cmd=''
typeset -gi _dots_status_ran=0
typeset -gi _dots_status_start=0

_dots_status_b64() {
  # Report text must be one line without control characters.
  print -rn -- "${${1//[[:cntrl:]]/ }[1,120]}" | base64 | tr -d '\n'
}

_dots_status_report() { # state app message
  printf '\e]7501;state=%s:app=%s:msg=%s\e\\' "$1" "$2" "$(_dots_status_b64 "$3")"
}

_dots_status_preexec() {
  _dots_status_ran=1
  printf '\e]133;C\e\\'
  local words=(${(z)1})
  local program=${words[1]:t}
  if [[ -z $program ]] || (( ${DOTS_STATUS_IGNORE[(Ie)$program]} )); then
    _dots_status_cmd=''
    return
  fi
  _dots_status_cmd=$1
  _dots_status_start=$EPOCHSECONDS
  _dots_status_report working "${${program//[^A-Za-z0-9_.+-]/}[1,32]}" "$1"
}

_dots_status_precmd() {
  local code=$?
  if [[ -n $_dots_status_cmd ]]; then
    local elapsed=$(( EPOCHSECONDS - _dots_status_start ))
    local words=(${(z)_dots_status_cmd})
    local program=${${${words[1]:t}//[^A-Za-z0-9_.+-]/}[1,32]}
    # Ctrl+C is a cancel, not a failure; the prompt mark clears the record.
    if (( elapsed >= DOTS_STATUS_AFTER && code != 130 )); then
      local took
      if (( elapsed >= 60 )); then took="$(( elapsed / 60 ))m$(( elapsed % 60 ))s"; else took="${elapsed}s"; fi
      if (( code == 0 )); then
        _dots_status_report done "$program" "$_dots_status_cmd · $took"
      else
        _dots_status_report error "$program" "$_dots_status_cmd · exit $code after $took"
      fi
    fi
    _dots_status_cmd=''
  fi
  if (( _dots_status_ran )); then
    _dots_status_ran=0
    printf '\e]133;D;%d\e\\' "$code"
  fi
  printf '\e]133;A\e\\'
}

zmodload zsh/datetime
# First, so the precmd hook reads the command's exit status before others run.
preexec_functions=(_dots_status_preexec ${preexec_functions:#_dots_status_preexec})
precmd_functions=(_dots_status_precmd ${precmd_functions:#_dots_status_precmd})
