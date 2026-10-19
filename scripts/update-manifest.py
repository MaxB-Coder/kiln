#!/usr/bin/env python3
"""Writes latest.json, the manifest Kiln's updater reads, from a release's signed files.

Usage: scripts/update-manifest.py VERSION BASE_URL DIR > latest.json
  VERSION   the release version (2026.10.26)
  BASE_URL  where the files will be downloadable (no trailing slash)
  DIR       the folder holding the update files and their .sig files:
            Kiln.app.tar.gz (macOS, Apple silicon), Kiln_*-setup.exe (Windows x64)
Only platforms whose files are present are listed.
"""
import json
import sys
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import quote

version, base, folder = sys.argv[1], sys.argv[2].rstrip("/"), Path(sys.argv[3])

# Updater platform key -> (file pattern, updater format)
PLATFORMS = {
    "macos-aarch64": ("Kiln.app.tar.gz", "app"),
    "windows-x86_64": ("Kiln_*_x64-setup.exe", "nsis"),
}

platforms = {}
for key, (pattern, fmt) in PLATFORMS.items():
    matches = sorted(folder.glob(pattern))
    if not matches:
        continue
    file = matches[-1]
    sig = file.with_name(file.name + ".sig")
    if not sig.is_file():
        sys.exit(f"{file.name} has no signature ({sig.name})")
    platforms[key] = {
        "url": f"{base}/{quote(file.name)}",
        "signature": sig.read_text().strip(),
        "format": fmt,
    }

if not platforms:
    sys.exit(f"no update files in {folder}")

json.dump(
    {
        "version": version,
        "notes": f"Kiln {version}",
        "pub_date": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": platforms,
    },
    sys.stdout,
    indent=2,
)
print()
