#!/usr/bin/env bash
# Checks that the packages scripts/package.sh built carry the icons scripts/make-icons.sh made
# from assets/icon/kiln.svg, byte for byte:
#   macOS: the app bundle's .icns (and that its signature still verifies)
#   Linux: the .deb's PNGs, the AppImage's icon and .DirIcon, the tarball's PNG
# Usage: scripts/check-icons.sh [path to Kiln.app]   (default: target/packages/Kiln.app)
# Windows has scripts/check-icons.ps1 (the icon is a resource inside the .exe there).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/target/packages"
icon="$root/assets/icon"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

same() { # file, expected, what
  if ! cmp -s "$1" "$2"; then
    echo "check-icons: $3 is not ${2#"$root"/} ($1)" >&2
    exit 1
  fi
  echo "ok: $3"
}

# A PNG under …/NxN/… is checked against kiln-N.png.
same_png_by_size() { # file, what
  local size
  size="$(echo "$1" | grep -oE '/[0-9]+x[0-9]+/' | head -1 | tr -d '/' | cut -dx -f1)"
  [ -n "$size" ] && [ -f "$icon/kiln-$size.png" ] || {
    echo "check-icons: no kiln-<size>.png for $1" >&2
    exit 1
  }
  same "$1" "$icon/kiln-$size.png" "$2 (${size}px)"
}

case "$(uname -s)" in
  Darwin)
    app="${1:-$out/Kiln.app}"
    name="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIconFile' "$app/Contents/Info.plist")"
    [[ $name == *.icns ]] || name="$name.icns"
    same "$app/Contents/Resources/$name" "$icon/kiln.icns" "Kiln.app's icon ($name)"
    codesign --verify --deep --strict "$app"
    echo "ok: codesign --verify --deep --strict"
    ;;
  Linux)
    deb="$(ls "$out"/*.deb)"
    dpkg-deb -x "$deb" "$work/deb"
    found=0
    while IFS= read -r png; do
      same_png_by_size "$png" ".deb ${png#"$work/deb"}"
      found=1
    done < <(find "$work/deb/usr/share/icons" -name '*.png')
    [ "$found" = 1 ] || { echo "check-icons: no icons in the .deb" >&2; exit 1; }

    image="$(ls "$out"/Kiln-*.AppImage)"
    (cd "$work" && "$image" --appimage-extract >/dev/null)
    same "$work/squashfs-root/.DirIcon" "$icon/kiln-256.png" "AppImage .DirIcon"
    same_png_by_size "$work/squashfs-root/usr/share/icons/hicolor/256x256/apps/kiln.png" \
      "AppImage icon"
    grep -qx 'Icon=kiln' "$work/squashfs-root/kiln.desktop" && echo "ok: AppImage kiln.desktop"

    tar -xzf "$(ls "$out"/kiln-*-linux-*.tar.gz)" -C "$work"
    same "$(ls "$work"/kiln-*-linux-*/kiln.png)" "$icon/kiln-256.png" "tarball kiln.png"
    ;;
  *)
    echo "check-icons: use scripts/check-icons.ps1 on Windows" >&2
    exit 1
    ;;
esac
