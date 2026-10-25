# Kiln shell integration for bash.
#
# Kiln exports PROMPT_COMMAND so bash sources this file at the first prompt. It marks the
# prompt, the command and its output (OSC 133) and reports the current folder (OSC 7).
# Commands are marked through PS0 (bash 4.4 and later); older bash still gets prompts.
# With KILN_PROMPT=kiln the prompt is two blank cells, where Kiln draws its flame (Starship gets
# a config that prints only those). Before each prompt the mode is read from KILN_PROMPT_FILE
# (Kiln writes it when the setting changes), so switching applies to open sessions too.
# Readline and completion are untouched.

if [[ -n "${_KILN_INTEGRATED-}" ]]; then
  return 0 2>/dev/null || true
fi
_KILN_INTEGRATED=1
_kiln_first=1
# Kiln's prompt, as PS1 holds it; and the shell's own, to put back for "shell".
_kiln_ours='\[\e]133;A\a\]  \[\e]133;B\a\]'
_kiln_shell_ps1=
# The user's Starship config, to put back when the shell's own prompt is chosen.
_kiln_user_starship=${STARSHIP_CONFIG-}

# The prompt setting, as Kiln last wrote it.
_kiln_read_mode() {
  local mode=
  [[ -n ${KILN_PROMPT_FILE-} && -r $KILN_PROMPT_FILE ]] || return 0
  read -r mode < "$KILN_PROMPT_FILE"
  if [[ $mode == kiln || $mode == shell ]]; then export KILN_PROMPT=$mode; fi
  return 0
}

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
  _kiln_read_mode
  if [[ ${KILN_PROMPT-} == kiln && -n ${KILN_STARSHIP_CONFIG-} ]]; then
    export STARSHIP_CONFIG=$KILN_STARSHIP_CONFIG
  elif [[ -n ${KILN_STARSHIP_CONFIG-} && ${STARSHIP_CONFIG-} == "$KILN_STARSHIP_CONFIG" ]]; then
    if [[ -n $_kiln_user_starship ]]; then
      export STARSHIP_CONFIG=$_kiln_user_starship
    else
      unset STARSHIP_CONFIG
    fi
  fi
  # Whose prompt follows: Kiln draws the flame only over its own.
  if [[ ${KILN_PROMPT-} == kiln ]]; then
    printf '\e]1337;KilnPrompt=kiln\a'
  else
    printf '\e]1337;KilnPrompt=shell\a'
  fi
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

# Runs last in PROMPT_COMMAND (after any prompt theme): sets Kiln's prompt, then from here on
# the next command is the user's.
_kiln_arm() {
  # Whatever is not Kiln's prompt is the shell's own (a theme may have rebuilt it).
  if [[ $PS1 != "$_kiln_ours" ]]; then _kiln_shell_ps1=$PS1; fi
  # Starship's own hook, not $STARSHIP_SHELL: that variable is inherited from wherever Kiln
  # was started.
  if [[ ${KILN_PROMPT-} == kiln ]] && ! declare -F starship_precmd >/dev/null; then
    PS1=$_kiln_ours
  elif [[ $PS1 == "$_kiln_ours" ]]; then
    PS1=$_kiln_shell_ps1
  fi
  _kiln_at_prompt=1
}
PROMPT_COMMAND="_kiln_prompt_command${_kiln_user_pc:+;$_kiln_user_pc};_kiln_arm"
unset _kiln_user_pc
_kiln_prompt_command
_kiln_arm
