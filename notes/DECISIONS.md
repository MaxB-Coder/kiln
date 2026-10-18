# Decisions

Choices made while building Kiln autonomously (user's go-ahead, 2026-10-18). Each one picks
what fits PLAN.md best: best UX, calm visuals, the same on macOS, Windows and Linux, secure.
Newest at the bottom.

## Phase 0

- **Name and place**: Kiln, `~/developer/kiln` (user's choice).
- **GPUI source**: git pin on Zed's repository by commit, not crates.io (stale) or third-party
  snapshots. Official source, exact provenance; Renovate bumps it weekly, CI is the gate.
- **Engine**: alacritty_terminal from crates.io, unmodified. Kiln runs its own copy of the PTY
  loop (Apache-2.0, attributed) to capture OSC 133 / OSC 7 at exact grid positions.
- **Font**: Nerd Fonts-patched JetBrains Mono embedded in the binary (GPUI cannot fall back to
  embedded symbol fonts). Same glyphs on every OS, nothing to install.
- **OSC 52**: copy allowed, paste (clipboard read) refused.
- **Go**: GPUI + alacritty_terminal, conditions in notes/phase0.md.

## Phase 1
- **Blocks are frozen copies.** When a command ends, its output is copied out of the live grid
  as logical lines and the scrollback is cleared, so row anchors stay valid and blocks re-wrap
  to any width. Output printed before the first prompt becomes a quiet "shell started" block.
- **Loading the shell integration** never touches the user's files: zsh via `ZDOTDIR` (Kiln's
  `.zshenv` restores it and sources the user's), bash via an exported `PROMPT_COMMAND`,
  PowerShell via `-NoExit -Command`, WSL by passing the same through `WSLENV`. Scripts live in
  a versioned per-user cache folder with mode 0700. fish and nu get the classic terminal.
- **Command line in the mark**: kitty's `OSC 133;C;cmdline_url=` extension. The node path for the
  input bar goes through `OSC 1337;KilnNode=` (iTerm2's namespace; other terminals ignore it).
- **Locale**: if no `LC_ALL`/`LC_CTYPE`/`LANG` is set (apps started from Finder or a launcher),
  shells get a UTF-8 `LANG`, as WezTerm does: `en_US.UTF-8` on macOS, `C.UTF-8` on Linux (built
  into glibc; `en_US.UTF-8` is missing on minimal systems). Without it, non-ASCII input broke in
  zsh and bash. (This was first described in a commit but only landed later, after CI caught it.)
- **Copy format**: a ```` ```console ```` fence holding `folder $ command`, the output, and
  `# exit N · time`; the fence grows past any backtick run in the output. ⌘⇧C fenced, ⌘⌥C raw.
  Windows/Linux: Ctrl+Shift+C copies the selection, or the last output when nothing is
  selected; Ctrl+Shift+Alt+C copies the raw output.
- **`clear`/`reset`/`cls`/`Clear-Host` clear the timeline**; pinned blocks stay.
- **Ctrl-C is not a failure**: exit 130 gets no red dot.
- **Old blocks dim** to 60%: every block but the newest command. Hovering or pinning brings one
  back to full strength.
- **Pin** keeps a block bright and through `clear`, with a small accent dot. A pinned shelf is
  left for phase 2.
- **Rerun** types the command at the current prompt after clearing the line (Ctrl+E Ctrl+U);
  it does nothing while another command runs. It runs in the current folder, like ↑ + Enter.
- **Open log** writes a private (0600) plain-text log to the app data folder (200 kept) and opens
  it in a split with `less -R +G` (Windows: `Get-Content | more`).
- **Splits** are side by side with equal widths. Panes without focus fade to 72% instead of
  having dividers.
- **Sidebar** groups sessions by git root (else folder; the home folder shows as `~`). Dots:
  green running, red failed, none idle.
- **Find**: Enter goes to the older match, Shift+Enter to the newer one (the way you scroll a
  terminal). It searches output, not the command lines (yet).
- **Palette** lists actions with their shortcuts, sessions, then past commands from every
  session (newest first, each once), with a fuzzy subsequence filter.
- **One font for all chrome** (the embedded JetBrains Mono), so the UI looks the same everywhere.
- **Config**: TOML at `~/.config/kiln/kiln.toml` (`$XDG_CONFIG_HOME` respected) or
  `%APPDATA%\Kiln\kiln.toml`; checked for changes every second; bad values are ignored and
  logged, and a broken file falls back to defaults so Kiln always starts.
- **Window chrome**: Linux always draws its own title bar and resize edges (same look on GNOME,
  KDE and X11). Windows 11 controls use the Codicon glyphs from the embedded font and GPUI's
  window-control areas, so snap layouts work. `KILN_CHROME=windows|linux|mac` previews another
  platform's chrome.
- **Sessions**: ⌘1–9 / Ctrl+Alt+1–9, ⌘⇧] ⌘⇧[ / Ctrl+Tab, Ctrl+Shift+Tab. Sidebar ⌘B.
  Text size ⌘= ⌘- ⌘0. Config ⌘,.
- **Windows build** uses the Windows subsystem in release and attaches to a parent console when
  there is one, so `--help` and `--bench-startup` still print.
- **Visual checks without a display**: `cargo run -p kiln --example snapshot` renders the real app
  headless (real shells, real text and GPU rendering) and saves PNGs; it also runs in CI.
- **Pushing**: the user allowed pushes to the private `MaxB-Coder/kiln` repo (2026-10-18), so CI
  checks every phase on macOS, Windows (PowerShell, WSL) and Linux.
- **`--background`** opens Kiln without taking focus (used for every test launch, so a test
  window never catches the user's typing).
- **Find also matches command lines** in block headers, highlighted in place.
