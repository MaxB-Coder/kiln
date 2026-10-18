# Kiln shell integration for bash.
#
# Kiln exports PROMPT_COMMAND so bash sources this file at the first prompt. It marks the
# prompt, the command and its output (OSC 133) and reports the current folder (OSC 7).
# Commands are marked through PS0 (bash 4.4 and later); older bash still gets prompts.

if [[ -n "${_KILN_INTEGRATED-}" ]]; then
  return 0 2>/dev/null || true
fi
_KILN_INTEGRATED=1
_kiln_first=1

# Remove the bootstrap from PROMPT_COMMAND; keep whatever the user's startup files added.
_kiln_user_pc=${PROMPT_COMMAND//"[ -n \"\$KILN_BASH_INTEGRATION\" ] && . \"\$KILN_BASH_INTEGRATION\""/}
_kiln_user_pc=${_kiln_user_pc#;}
_kiln_user_pc=${_kiln_user_pc%;}

_kiln_urlencode() {
  local LC_ALL=C str=$1 out= c hex i
  for (( i = 0; i < ${#str}; i++ )); do
    c=${str:i:1}
    case $c in
      [a-zA-Z0-9/._~-]) out+=$c ;;
      *) printf -v hex '%%%02X' "'$c"; out+=$hex ;;
    esac
  done
  REPLY=$out
}

_kiln_prompt_command() {
  local ret=$?
  if [[ -z $_kiln_first ]]; then
    printf '\e]133;D;%d\a' "$ret"
  fi
  _kiln_first=
  _kiln_urlencode "$PWD"
  printf '\e]7;file://%s%s\a' "${HOSTNAME}" "$REPLY"
  # Runtimes on PATH for the input bar and the rail (searched in-process, no subprocess).
  local tools= tool dir IFS=:
  for tool in node python3 ruby go rustc java deno bun; do
    for dir in $PATH; do
      if [[ -x "$dir/$tool" && ! -d "$dir/$tool" ]]; then tools+="$tool=$dir/$tool;"; break; fi
    done
  done
  unset IFS
  if [[ $tools != "${_kiln_tools-}" ]]; then
    _kiln_tools=$tools
    _kiln_urlencode "$tools"
    printf '\e]1337;KilnTools=%s\a' "$REPLY"
  fi
  if [[ $PS1 != *'133;A'* ]]; then
    PS1='\[\e]133;A\a\]'"${PS1}"'\[\e]133;B\a\]'
  fi
  return $ret
}

if (( BASH_VERSINFO[0] > 4 || (BASH_VERSINFO[0] == 4 && BASH_VERSINFO[1] >= 4) )); then
  PS0='\[\e]133;C\a\]'"${PS0-}"
elif [[ -z "$(trap -p DEBUG)" ]]; then
  # Older bash (macOS ships 3.2) has no PS0: the first command after each prompt marks the
  # output start from a DEBUG trap. Only when the user has no DEBUG trap of their own.
  _kiln_at_prompt=
  _kiln_debug() {
    [[ -n "$_kiln_at_prompt" && -z "${COMP_LINE-}" ]] || return 0
    case "$BASH_COMMAND" in _kiln_arm*) return 0 ;; esac
    _kiln_at_prompt=
    printf '\e]133;C\a'
  }
  trap '_kiln_debug' DEBUG
fi

# Runs last in PROMPT_COMMAND: from here on, the next command is the user's.
_kiln_arm() { _kiln_at_prompt=1; }
PROMPT_COMMAND="_kiln_prompt_command${_kiln_user_pc:+;$_kiln_user_pc};_kiln_arm"
unset _kiln_user_pc
_kiln_prompt_command
_kiln_arm
