#!/usr/bin/env python3
"""Rough Kiln vs WezTerm comparison on one Mac: startup, memory and output throughput.

Both apps are started from here with the same zsh command and the same grid size, using the
user's normal WezTerm config. Nothing is typed into either app. Results go to stdout as JSON.

Usage: python3 bench/compare.py [--runs 8] [--kiln path/to/kiln] [--only kiln|wezterm]
"""

import argparse
import json
import os
import re
import signal
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
WEZTERM = "/Applications/WezTerm.app/Contents/MacOS/wezterm-gui"
COLS, ROWS = 120, 36


def build_waitwin() -> str:
    out = Path(tempfile.gettempdir()) / "kiln-bench-waitwin"
    src = ROOT / "bench" / "waitwin.swift"
    if not out.exists() or out.stat().st_mtime < src.stat().st_mtime:
        subprocess.run(["swiftc", "-O", str(src), "-o", str(out)], check=True)
    return str(out)


def argv_for(app: str, kiln: str, shell_cmd: str) -> list[str]:
    shell = ["/bin/zsh", "-lc", shell_cmd]
    if app == "kiln":
        return [kiln, "--cols", str(COLS), "--rows", str(ROWS), "--", *shell]
    return [
        WEZTERM,
        "--config", f"initial_cols={COLS}",
        "--config", f"initial_rows={ROWS}",
        "start", "--always-new-process", "--", *shell,
    ]


def pids_named(needle: str) -> list[int]:
    out = subprocess.run(["pgrep", "-if", needle], capture_output=True, text=True).stdout
    return [int(p) for p in out.split()]


def stop(proc: subprocess.Popen) -> None:
    try:
        os.killpg(proc.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        os.killpg(proc.pid, signal.SIGKILL)
        proc.wait()
    time.sleep(0.5)


def wait_for_file(path: Path, timeout: float = 60) -> str:
    deadline = time.time() + timeout
    while time.time() < deadline:
        if path.exists() and path.stat().st_size > 0:
            time.sleep(0.05)
            return path.read_text().strip()
        time.sleep(0.01)
    raise TimeoutError(f"{path} never appeared")


def footprint_mb(pid: int) -> float:
    out = subprocess.run(["footprint", "-p", str(pid)], capture_output=True, text=True).stdout
    m = re.search(r"Footprint: ([\d.]+) (KB|MB|GB)", out)
    if not m:
        raise RuntimeError(f"no footprint for {pid}: {out[:200]}")
    value, unit = float(m.group(1)), m.group(2)
    return value * {"KB": 1 / 1024, "MB": 1, "GB": 1024}[unit]


def startup(app: str, kiln: str, waitwin: str, runs: int) -> dict:
    """Time from exec to window on screen, and to the shell running inside it."""
    needle = "kiln" if app == "kiln" else "wezterm"
    window_ms, shell_ms = [], []
    for i in range(runs + 1):
        stamp = Path(tempfile.mkdtemp()) / "shell-started"
        cmd = f"zmodload zsh/datetime; print $EPOCHREALTIME > {stamp}; sleep 30"
        ignore = [str(p) for p in pids_named(needle)]
        t0 = time.time()
        proc = subprocess.Popen(argv_for(app, kiln, cmd), stdout=subprocess.DEVNULL,
                                stderr=subprocess.DEVNULL, start_new_session=True)
        seen = subprocess.run([waitwin, needle, *ignore], capture_output=True, text=True)
        shell_at = float(wait_for_file(stamp))
        stop(proc)
        if i == 0:
            continue  # warm-up: first launch pays for cold caches
        window_ms.append((float(seen.stdout.split()[0]) - t0) * 1000)
        shell_ms.append((shell_at - t0) * 1000)
    return {
        "window_on_screen_ms_median": round(statistics.median(window_ms), 1),
        "window_on_screen_ms_min": round(min(window_ms), 1),
        "shell_running_ms_median": round(statistics.median(shell_ms), 1),
        "runs": runs,
    }


def memory_and_throughput(app: str, kiln: str) -> dict:
    """Footprint at an idle prompt, and after pushing ~25 MB of output through the terminal."""
    tmp = Path(tempfile.mkdtemp())
    idle_flag, busy_flag, timing = tmp / "idle", tmp / "busy", tmp / "timing"
    # Idle: a login shell that has started. Busy: plain and coloured output, then idle again.
    cmd = (
        "zmodload zsh/datetime; "
        f"print ok > {idle_flag}; sleep 10; "
        "s=$EPOCHREALTIME; seq 1 2000000; p=$EPOCHREALTIME; "
        "for i in {1..200000}; do print -P '%F{red}red%f %F{green}green%f %F{blue}blue%f %B%F{yellow}bold%f%b' $i; done; "
        "c=$EPOCHREALTIME; "
        f"print $((p - s)) $((c - p)) > {timing}; print ok > {busy_flag}; sleep 60"
    )
    proc = subprocess.Popen(argv_for(app, kiln, cmd), stdout=subprocess.DEVNULL,
                            stderr=subprocess.DEVNULL, start_new_session=True)
    try:
        wait_for_file(idle_flag)
        time.sleep(5)
        gui_pid = proc.pid
        idle = footprint_mb(gui_pid)
        wait_for_file(busy_flag, timeout=600)
        time.sleep(5)
        busy = footprint_mb(gui_pid)
        plain_s, colour_s = (float(x) for x in timing.read_text().split())
    finally:
        stop(proc)
    return {
        "idle_footprint_mb": round(idle, 1),
        "after_output_footprint_mb": round(busy, 1),
        "seq_2M_lines_s": round(plain_s, 2),
        "coloured_200k_lines_s": round(colour_s, 2),
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--runs", type=int, default=8)
    parser.add_argument("--kiln", default=str(ROOT / "target/release/Kiln.app/Contents/MacOS/kiln"))
    parser.add_argument("--only", choices=["kiln", "wezterm"])
    args = parser.parse_args()

    waitwin = build_waitwin()
    apps = [args.only] if args.only else ["kiln", "wezterm"]
    results = {}
    for app in apps:
        print(f"== {app}", file=sys.stderr)
        results[app] = {"startup": startup(app, args.kiln, waitwin, args.runs)}
        results[app].update(memory_and_throughput(app, args.kiln))
        print(json.dumps(results[app]), file=sys.stderr)
    print(json.dumps(results, indent=2))


if __name__ == "__main__":
    main()
