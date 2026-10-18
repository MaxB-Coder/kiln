#!/usr/bin/env bash
# Builds and tests Kiln on Linux in a container, then renders the headless screenshots.
# Needs Docker (on a Mac: `brew install colima docker && colima start`).
# Output: target/linux (build), notes/screenshots/linux (PNGs).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
docker build -q -t kiln-linux "$root/scripts/linux" >/dev/null
docker run --rm \
  -v "$root":/work \
  -v kiln-cargo-registry:/usr/local/cargo/registry \
  -v kiln-cargo-git:/usr/local/cargo/git \
  -e CARGO_TARGET_DIR=/work/target/linux \
  -e RUST_LOG=warn \
  kiln-linux bash -lc '
    git config --global --add safe.directory /work
    cargo test --workspace --locked
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo run --locked -p kiln --example snapshot -- notes/screenshots/linux
  '
