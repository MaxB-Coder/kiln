# Kiln shell integration for zsh.
#
# Marks the prompt, the command and its output (OSC 133) and reports the current folder
# (OSC 7), so Kiln can show each command as a block. Loaded automatically by Kiln; it changes
# nothing about how the shell behaves.

[[ -o interactive ]] || return 0
(( ${+_KILN_INTEGRATED} )) && return 0
typeset -g _KILN_INTEGRATED=1
typeset -gi _kiln_running=0

# Percent-encodes $1 into REPLY (for OSC 7 and the command line).
_kiln_urlencode() {
  emulate -L zsh
  local LC_ALL=C str=$1 out= c hex
  local -i i
  for (( i = 1; i <= ${#str}; i++ )); do
    c=${str[i]}
    case $c in
      [a-zA-Z0-9/._~-]) out+=$c ;;
      *) printf -v hex '%%%02X' "'$c"; out+=$hex ;;
    esac
  done
  REPLY=$out
}

_kiln_precmd() {
  local -i ret=$?
  if (( _kiln_running )); then
    printf '\e]133;D;%d\a' $ret
    _kiln_running=0
  fi
  _kiln_urlencode "$PWD"
  printf '\e]7;file://%s%s\a' "${HOST}" "$REPLY"
  # Context for the input bar: which node is on PATH (a hash lookup, no subprocess).
  local node=${commands[node]-}
  if [[ $node != ${_kiln_node-} ]]; then
    typeset -g _kiln_node=$node
    _kiln_urlencode "$node"
    printf '\e]1337;KilnNode=%s\a' "$REPLY"
  fi
  # Prompt themes may rebuild PS1 in their own precmd, so the wrap also runs last (from the
  # next prompt on: zsh does not see hooks added while it runs them).
  _kiln_wrap_prompt
  if [[ ${precmd_functions[-1]} != _kiln_wrap_prompt ]]; then
    precmd_functions=(${precmd_functions:#_kiln_wrap_prompt} _kiln_wrap_prompt)
  fi
  return $ret
}

_kiln_wrap_prompt() {
  if [[ $PS1 != *$'\e]133;A'* ]]; then
    PS1=$'%{\e]133;A\a%}'"${PS1}"$'%{\e]133;B\a%}'
  fi
}

_kiln_preexec() {
  _kiln_urlencode "$1"
  printf '\e]133;C;cmdline_url=%s\a' "$REPLY"
  _kiln_running=1
}

autoload -Uz add-zsh-hook
# First in line, so $? is still the command's exit status.
precmd_functions=(_kiln_precmd ${precmd_functions:#_kiln_precmd})
add-zsh-hook preexec _kiln_preexec
