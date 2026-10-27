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

  # 3. The icon setting (X11): with the Beehive design in slate (not the default design or
  #    colour), the window's _NET_WM_ICON is that icon: its top edge is blue-grey (more blue
  #    than red), where the default's is orange and black's is warm grey.
  if command -v xprop >/dev/null; then
    config_home="$(mktemp -d)"
    mkdir -p "$config_home/kiln"
    printf 'icon = "beehive"\n[icon_colours]\nbeehive = "slate"\n' > "$config_home/kiln/kiln.toml"
    XDG_CONFIG_HOME="$config_home" KILN_STATE_FILE="$config_home/state.json" \
      "$kiln" --fresh --cols 80 --rows 20 -- /bin/sh -c 'sleep 30' &
    pid=$!
    # Wait for the window and its icon (start-up under lavapipe can take several seconds).
    window="" icon=""
    # Kiln's window is the top-level one with an icon (Xvfb runs no window manager, so nothing
    # else sets one); its title is the session's name, so it is not looked up by name.
    for _ in $(seq 30); do
      sleep 1
      for candidate in $(xwininfo -root -children | awk '$1 ~ /^0x/ { print $1 }'); do
        # The raw numbers (xprop otherwise draws the icon as text): width, height, pixels.
        icon="$(xprop -id "$candidate" -notype -f _NET_WM_ICON 32c ' = $0+' _NET_WM_ICON \
          2>/dev/null | sed 's/.*= //' || true)"
        case "$icon" in [0-9]*) window="$candidate"; break 2 ;; esac
      done
    done
    kill "$pid" || true
    case "$icon" in
      [0-9]*) ;;
      *)
        echo "ci-smoke: no _NET_WM_ICON on Kiln's window ($window) after 30 s" >&2
        # What X11 has, to see why: the windows and their names.
        xwininfo -root -tree | head -40 >&2 || true
        exit 1
        ;;
    esac
    echo "first icon: $(echo "$icon" | cut -d, -f1-2)"
    # Row 0, column 8 of the 16 px icon: the third value is its first pixel.
    pixel="$(echo "$icon" | tr -d ' ' | cut -d, -f11)"
    red=$(( (pixel >> 16) & 255 ))
    blue=$(( pixel & 255 ))
    echo "window $window, top edge pixel $(printf '#%06x' $(( pixel & 0xffffff ))) (red $red, blue $blue)"
    if [ -z "$pixel" ] || [ "$blue" -le "$red" ]; then
      echo "ci-smoke: the window icon is not Beehive in slate" >&2
      exit 1
    fi
    echo "ok: the window icon follows icon = \"beehive\" in slate"
  fi
fi
