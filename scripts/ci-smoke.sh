#!/usr/bin/env bash
# CI smoke test (macOS, Linux): starts Kiln with a shell command that prints a marker, checks
# that a frame with shell output was drawn, and saves a screenshot where the runner allows it.
# Usage: scripts/ci-smoke.sh path/to/kiln
set -euo pipefail
kiln="$1"
mkdir -p smoke

# 1. Startup: Kiln quits after the first frame that shows shell output.
out="$("$kiln" --bench-startup --cols 100 --rows 30 -- /bin/sh -c 'echo kiln-smoke; sleep 20')"
echo "$out"
grep -q '^startup_ms=' <<<"$out"
echo "$out" > smoke/startup.txt

# 2. Screenshot of a colourful session (Linux only: macOS runners deny screen recording).
if [[ "$(uname)" == "Linux" ]]; then
  "$kiln" --cols 100 --rows 30 -- /bin/sh -c '
    printf "\033[1mKiln on Linux\033[0m  $(uname -sr)\n\n";
    for i in 0 1 2 3 4 5 6 7; do printf "\033[3${i}m colour$i \033[9${i}m bright$i\033[0m\n"; done;
    printf "\n\033[4munderline\033[0m \033[3mitalic\033[0m \033[7minverse\033[0m 宽字符 -> != \356\202\266\356\202\264 ─┬─█▓▒░\n";
    sleep 30' &
  pid=$!
  sleep 8
  import -window root smoke/linux.png
  kill "$pid" || true
fi
