#!/usr/bin/env bash
# Builds Kiln's installers for this platform into target/packages:
#   macOS:   Kiln.app and Kiln-<version>-<arch>.dmg
#   Windows: kiln_<version>_x64-setup.exe (NSIS, per-user; run from Git Bash)
#   Linux:   .deb, .tar.gz and .AppImage
# With CARGO_PACKAGER_SIGN_PRIVATE_KEY (and _PASSWORD) set, cargo-packager also writes the
# update archives and their .sig files the in-app updater checks.
# Needs: cargo install cargo-packager --version 0.11.8 --locked
#        cargo install cargo-about --version 0.9.2 --locked --features cli
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
version="$(grep -m1 '^version' crates/kiln/Cargo.toml | cut -d'"' -f2)"
out="$root/target/packages"
rm -rf "$out"
# The licences of everything in the binary, shipped inside each package. cargo-about reads
# the crates offline, so fetch them first.
cargo fetch --locked
mkdir -p target
cargo about generate --frozen --fail -m crates/kiln/Cargo.toml about.hbs \
  -o target/THIRD_PARTY_LICENSES.md

case "$(uname -s)" in
  Darwin)
    cargo packager --profile dist --formats app
    # The dmg is made with macOS's own hdiutil (cargo-packager's dmg step downloads a script).
    arch="$(uname -m)"
    stage="$(mktemp -d)"
    trap 'rm -rf "$stage"' EXIT
    ditto "$out/Kiln.app" "$stage/Kiln.app"
    ln -s /Applications "$stage/Applications"
    hdiutil create -quiet -volname "Kiln" -srcfolder "$stage" -fs HFS+ -format UDZO -ov \
      "$out/Kiln-$version-$arch.dmg"
    ;;
  Linux)
    cargo packager --profile dist --formats deb
    # A plain tarball too, for other distributions. (cargo-packager's own AppImage step
    # fetches its tools unpinned, so the AppImage is built below instead.)
    arch="$(uname -m)"
    stage="$(mktemp -d)"
    trap 'rm -rf "$stage"' EXIT
    dir="$stage/kiln-$version-linux-$arch"
    mkdir -p "$dir"
    cp target/dist/kiln "$dir/kiln"
    cp assets/icon/kiln-256.png "$dir/kiln.png"
    cp target/THIRD_PARTY_LICENSES.md assets/fonts/JetBrainsMono-OFL.txt \
      assets/fonts/NerdFonts-LICENSE "$dir/"
    cat > "$dir/kiln.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Kiln
Comment=A calm, cross-platform terminal
Exec=kiln
Icon=kiln
Categories=System;TerminalEmulator;
Terminal=false
DESKTOP
    tar -czf "$out/kiln-$version-linux-$arch.tar.gz" -C "$stage" "kiln-$version-linux-$arch"
    # The AppImage: what the in-app updater replaces on Linux (signed for it when the update
    # key is set). Its runtime is pinned and hash-checked (scripts/linux/make-appimage.sh).
    image="$(scripts/linux/make-appimage.sh target/dist/kiln "$version" "$out")"
    if [ -n "$image" ] && [ -n "${CARGO_PACKAGER_SIGN_PRIVATE_KEY:-}" ]; then
      cargo packager signer sign "$image"
    fi
    ;;
  MINGW* | MSYS* | CYGWIN*)
    cargo packager --profile dist --formats nsis
    ;;
  *)
    echo "package.sh: unsupported system $(uname -s)" >&2
    exit 1
    ;;
esac
ls -la "$out"
