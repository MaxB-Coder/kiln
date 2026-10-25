#!/usr/bin/env bash
# Makes every app icon from the one source, assets/icon/kiln.svg (1024×1024, the body on the
# macOS 824/1024 grid). Run it after changing the SVG, then commit what it writes:
#
#   assets/icon/kiln.icns      macOS app (16–512 pt, @1x and @2x), with the grid's margin
#   assets/icon/kiln.ico       Windows exe and installer (16, 24, 32, 48, 64, 128, 256)
#   assets/icon/kiln-*.png     Linux .deb, AppImage and tarball (16, 32, 128, 256, 512)
#
# and, for each variant in assets/icon/variants/kiln-<name>.svg (the app-icon setting), the
# PNGs Kiln embeds to switch icons while it runs, in assets/icon/variants/<name>/:
#   mac-128.png, mac-512.png         the Settings tiles and the macOS Dock (with the margin)
#   tile-{16,24,32,48,64,128,256}.png  the window icon on Windows and Linux (filling the tile)
#
# kiln.svg is the default variant and must be the same file as variants/kiln-terracotta.svg.
#
# Windows and Linux icons fill their tile (the body takes about 248/256 of it), so those are
# rendered from the SVG with the margin trimmed. Needs rsvg-convert and ImageMagick (magick);
# iconutil on macOS for the .icns (elsewhere the .icns is left as it is).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
icon="$root/assets/icon"
svg="$icon/kiln.svg"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

for tool in rsvg-convert magick; do
  command -v "$tool" >/dev/null || { echo "make-icons: $tool is needed" >&2; exit 1; }
done

render() { # source, size, output
  rsvg-convert --width "$2" --height "$2" --keep-aspect-ratio "$1" --output "$3"
}

cmp -s "$svg" "$icon/variants/kiln-terracotta.svg" || {
  echo "make-icons: kiln.svg differs from variants/kiln-terracotta.svg (the default)" >&2
  exit 1
}

# The tile version: the same drawing with a viewBox cut to the body plus a thin margin, so the
# 824 px body fills 248/256 of the tile.
tile_size=$(awk 'BEGIN { printf "%.1f", 824 * 256 / 248 }')
tile_origin=$(awk -v s="$tile_size" 'BEGIN { printf "%.1f", 512 - s / 2 }')
tile_svg() { # source, output
  sed "s|viewBox=\"0 0 1024 1024\"|viewBox=\"$tile_origin $tile_origin $tile_size $tile_size\"|" \
    "$1" > "$2"
  grep -q "viewBox=\"$tile_origin" "$2" || { echo "make-icons: no viewBox to trim in $1" >&2; exit 1; }
}
tile_svg "$svg" "$work/tile.svg"

# Linux PNGs.
for size in 16 32 128 256 512; do
  render "$work/tile.svg" "$size" "$icon/kiln-$size.png"
done

# Windows .ico.
ico_parts=()
for size in 16 24 32 48 64 128 256; do
  render "$work/tile.svg" "$size" "$work/ico-$size.png"
  ico_parts+=("$work/ico-$size.png")
done
magick "${ico_parts[@]}" "$icon/kiln.ico"

# macOS .icns, keeping the grid's margin.
if command -v iconutil >/dev/null; then
  set_dir="$work/kiln.iconset"
  mkdir "$set_dir"
  for size in 16 32 128 256 512; do
    render "$svg" "$size" "$set_dir/icon_${size}x${size}.png"
    render "$svg" $((size * 2)) "$set_dir/icon_${size}x${size}@2x.png"
  done
  iconutil --convert icns --output "$icon/kiln.icns" "$set_dir"
else
  echo "make-icons: iconutil not found (macOS only); kiln.icns left as it is" >&2
fi

# The variants, for switching while Kiln runs.
for variant_svg in "$icon"/variants/kiln-*.svg; do
  name="$(basename "$variant_svg" .svg)"
  name="${name#kiln-}"
  out="$icon/variants/$name"
  mkdir -p "$out"
  render "$variant_svg" 128 "$out/mac-128.png"
  render "$variant_svg" 512 "$out/mac-512.png"
  tile_svg "$variant_svg" "$work/tile-$name.svg"
  for size in 16 24 32 48 64 128 256; do
    render "$work/tile-$name.svg" "$size" "$out/tile-$size.png"
  done
  echo "made variants/$name/"
done

echo "made kiln.icns, kiln.ico and kiln-{16,32,128,256,512}.png from kiln.svg"
