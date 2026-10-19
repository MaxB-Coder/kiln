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
  PowerShell via `-NoExit -EncodedCommand` (inline, because Windows client machines block
  `.ps1` files by default), WSL by passing the same through `WSLENV`. Scripts live in
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

## Phase 2

- **Rail** (⌘I / Ctrl+Shift+I): 280 px on the right, full height, for the focused pane. On
  Windows and Linux the window controls move to its top while it is open. It reads every 2 s
  in the background while open, and right away when the focus moves; nothing is read while it
  is closed. Up to 14 changed files and 12 processes, then "N more".
- **Changes** come from `git status --porcelain=v2` with `core.fsmonitor=false`,
  `GIT_OPTIONAL_LOCKS=0` and no prompts: entering a folder must never run a repository's
  hooks or tools. Clicking a file pages `git diff HEAD -- file` (untracked: against
  `/dev/null`) in a split with `less -R`, kept open for short diffs; paths are passed as
  arguments, never pasted into shell text. The pane closes when the pager quits.
- **Ports**: clicking one asks with a system dialog before opening `http://localhost:PORT`,
  because the plan says links ask first.
- **Processes** are the tree under the pane's shell (sysinfo); ports are the TCP listeners
  those processes own (listeners), IPv4 and IPv6 on one port shown once.
- **Notifications**: after `notify_after_secs` (default 10; 0 turns them off), only when the
  pane is out of sight: the window is not in front, the session is not shown, or focus mode
  hides the pane. Title: program and first argument only. Body: finished or failed (with exit
  code), the duration and the folder. Clicking one brings the window and session back. No
  sound, and Ctrl-C (exit 130) counts as finished, not failed.
- **Attention dot** in the sidebar: orange (the plan keeps orange for things to act on), from
  a bell or a long command finishing out of sight; cleared when the session is shown. A
  failed command's red dot wins. A bell in a pane you are looking at does nothing.
- **Focus mode** (⌘. / Ctrl+Shift+.) hides the sidebar, the rail and the session's other
  panes. ⌘B or ⌘I leave it.
- **Tints** come from the running command line: `ssh`, `autossh`, `mosh` (options with values,
  `ssh://` URLs and IPv6 handled). Remote: 10% of the palette's blue mixed into the
  background. Production: 13% of its red, for hosts matching `[tints] production` (globs,
  case-insensitive, default `["*prod*"]`). A small label at the top right names the host.
  `[tints] remote = false` keeps only the production tint.
- **Saved state**: `state.json` in Kiln's data folder (macOS Application Support, Windows
  LocalAppData, Linux XDG state), private and written atomically, checked every 2 s and on
  quit, written only when something changed. It keeps the window that changed last (folders,
  splits, sidebar, rail) and one layout per git project (24 kept). One-off panes (diffs, logs,
  the config editor) are not saved. Closing the last session saves an empty window, so the
  next launch starts fresh. `--fresh` or `[window] restore = false` skips restoring; a given
  program (`kiln -- prog`) and `--bench-startup` never restore or save.
- **Projects** are git roots, with links resolved on macOS and Linux (`/var` and
  `/private/var` are one project); Windows paths are left as they are. The palette lists
  them; choosing one shows its open session, else reopens its saved layout, else starts a
  session at its root.
- **Project colours**: picked from eight muted palette colours by a stable hash (FNV-1a) of
  the folder, never the orange accent; `[projects."~/path"] color = "#rrggbb"` overrides.
  Home and loose folders get no colour.
- **Headless scenario** advances GPUI's test clock with real time, so timers (the rail, saving)
  run as they do in the app; it saves state to a scratch file and turns the cursor blink off
  for stable pictures.
- **CI skips pushes that only touch `notes/` or Markdown**, to save runner minutes (Windows
  minutes count double on a private repo). Pull requests and the weekly run still build all.
