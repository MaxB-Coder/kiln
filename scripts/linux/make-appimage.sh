#!/usr/bin/env bash
# Builds Kiln's AppImage, the format the in-app updater replaces on Linux: the binary in an
# AppDir, squashed with the distribution's mksquashfs, behind the AppImage project's runtime
# (github.com/AppImage/type2-runtime). The runtime is pinned to a release and checked against
# the SHA-256 below before it is used; nothing unpinned is downloaded or run.
# Usage: scripts/linux/make-appimage.sh BINARY VERSION OUT_DIR   (prints the AppImage's path)
set -euo pipefail
binary=$1
version=$2
out=$3
root="$(cd "$(dirname "$0")/../.." && pwd)"
arch="$(uname -m)"

# type2-runtime release, and the SHA-256 of its runtime for each architecture Kiln ships.
# Renovate proposes newer releases; update the hash with the version (the build fails closed
# until it matches).
RUNTIME_VERSION=20251108
RUNTIME_SHA256_X86_64=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d
RUNTIME_SHA256_AARCH64=00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444

case "$arch" in
  x86_64) sha256=$RUNTIME_SHA256_X86_64 ;;
  aarch64) sha256=$RUNTIME_SHA256_AARCH64 ;;
  *)
    echo "make-appimage: no pinned AppImage runtime for $arch; skipping" >&2
    exit 0
    ;;
esac
if ! command -v mksquashfs >/dev/null; then
  echo "make-appimage: needs mksquashfs (squashfs-tools)" >&2
  exit 1
fi

cache="$root/target/appimage"
runtime="$cache/runtime-$arch-$RUNTIME_VERSION"
mkdir -p "$cache"
if ! echo "$sha256  $runtime" | sha256sum -c --status - 2>/dev/null; then
  curl -fsSL --proto '=https' --tlsv1.2 -o "$runtime.part" \
    "https://github.com/AppImage/type2-runtime/releases/download/$RUNTIME_VERSION/runtime-$arch"
  if ! echo "$sha256  $runtime.part" | sha256sum -c --status -; then
    rm -f "$runtime.part"
    echo "make-appimage: runtime-$arch $RUNTIME_VERSION does not match its pinned SHA-256" >&2
    exit 1
  fi
  mv "$runtime.part" "$runtime"
fi

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
app="$stage/Kiln.AppDir"
mkdir -p "$app/usr/bin" "$app/usr/share/applications" \
  "$app/usr/share/icons/hicolor/256x256/apps" "$app/usr/share/doc/kiln"
install -m 755 "$binary" "$app/usr/bin/kiln"
install -m 644 "$root/assets/icon/kiln-256.png" "$app/usr/share/icons/hicolor/256x256/apps/kiln.png"
cat > "$app/usr/share/applications/kiln.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Kiln
Comment=A calm, cross-platform terminal
Exec=kiln
Icon=kiln
Categories=System;TerminalEmulator;
Terminal=false
DESKTOP
cp "$app/usr/share/applications/kiln.desktop" "$app/kiln.desktop"
cp "$root/assets/icon/kiln-256.png" "$app/kiln.png"
ln -s kiln.png "$app/.DirIcon"
ln -s usr/bin/kiln "$app/AppRun"
for f in target/THIRD_PARTY_LICENSES.md LICENSE-MIT LICENSE-APACHE \
  assets/fonts/JetBrainsMono-OFL.txt assets/fonts/NerdFonts-LICENSE; do
  if [ -f "$root/$f" ]; then install -m 644 "$root/$f" "$app/usr/share/doc/kiln/"; fi
done

mksquashfs "$app" "$stage/kiln.squashfs" -root-owned -noappend -comp zstd -quiet >&2
image="$out/Kiln-$version-$arch.AppImage"
cat "$runtime" "$stage/kiln.squashfs" > "$image"
chmod 755 "$image"
echo "$image"
