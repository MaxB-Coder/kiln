#!/usr/bin/env python3
"""Sets the kiln crate's version (Cargo.toml and Cargo.lock), for release builds.

Usage: scripts/set-version.py 2026.10.26
"""
import re
import sys
from pathlib import Path

root = Path(__file__).resolve().parent.parent
version = sys.argv[1]
if not re.fullmatch(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", version):
    sys.exit(f"not a plain semver version: {version}")

manifest = root / "crates/kiln/Cargo.toml"
text = manifest.read_text()
text, n = re.subn(r'(?m)^version = "[^"]*"', f'version = "{version}"', text, count=1)
if n != 1:
    sys.exit("no version line in crates/kiln/Cargo.toml")
manifest.write_text(text)

lock = root / "Cargo.lock"
text = lock.read_text()
text, n = re.subn(r'(\[\[package\]\]\nname = "kiln"\nversion = )"[^"]*"', rf'\1"{version}"', text)
if n != 1:
    sys.exit("no kiln entry in Cargo.lock")
lock.write_text(text)
print(f"kiln {version}")
