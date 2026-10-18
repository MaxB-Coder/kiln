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
  if [[ $PS1 != *'133;A'* ]]; then
    PS1='\[\e]133;A\a\]'"${PS1}"'\[\e]133;B\a\]'
  fi
  return $ret
}

if (( BASH_VERSINFO[0] > 4 || (BASH_VERSINFO[0] == 4 && BASH_VERSINFO[1] >= 4) )); then
  PS0='\[\e]133;C\a\]'"${PS0-}"
fi

PROMPT_COMMAND="_kiln_prompt_command${_kiln_user_pc:+;$_kiln_user_pc}"
unset _kiln_user_pc
_kiln_prompt_command
