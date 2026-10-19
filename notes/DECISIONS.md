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

## Phase 3

- **Installers** come from cargo-packager (MIT/Apache, CrabNebula), configured in
  `crates/kiln/Cargo.toml`, so the version has one source. `scripts/package.sh` builds:
  macOS `Kiln.app` and a dmg made with `hdiutil` (cargo-packager's dmg step downloads a
  create-dmg script at build time; not in the release path); Windows a per-user NSIS installer
  (no admin rights, and the updater can replace it quietly); Linux a `.deb` and a tarball.
- **No AppImage yet**: cargo-packager downloads linuxdeploy's AppImage plugin from a moving
  "continuous" release with no hash check. A release pipeline must not run an unpinned tool.
  So Linux has no in-app updates for now (they need an AppImage); see QUESTIONS.
- **Signing without paid accounts**: macOS builds are ad hoc signed (Gatekeeper asks on first
  launch); Windows builds are unsigned (SmartScreen asks). The release workflow switches on
  Developer ID signing with notarisation, and Azure Trusted Signing, as soon as their
  secrets exist. Neither has run yet (no accounts).
- **Versions are dates** (`2026.10.26`): weekly releases, plain semver for the updater, and a
  dev build (0.0.1) is always older than any release. One release per day at most.
- **Releases publish only on request**: a manual run builds, installs and starts the
  installers on all three systems and keeps them as artifacts; ticking "publish" (or setting
  the repository variable `KILN_RELEASES=on` for the weekly run) creates the GitHub release.
- **Updates** use cargo-packager-updater for the download, the minisign signature check and the
  install, but Kiln checks the manifest itself: the crate's check sets `SSL_CERT_FILE` and
  `SSL_CERT_DIR` process-wide on Linux at run time, which every shell Kiln starts would
  inherit (breaking TLS for programs on distributions where that path does not exist) and which
  races other threads reading the environment. The manifest is limited to 256 KB, HTTPS only.
- **When updates run**: only in builds that carry an update address and key (set by the release
  workflow from repository variables) and that were installed by a Kiln installer (an app
  bundle, the NSIS install folder, an AppImage). First check a minute after launch, then daily.
  The offer is a quiet line at the bottom of the sidebar and the first palette item; nothing
  installs without a yes. On Windows the installer takes over and Kiln closes, and the dialog
  says so first. After an install, Kiln offers to restart; sessions reopen in their folders.
- **Where updates come from is open**: the private repository's release files need a GitHub
  login, so the updater cannot read them. `KILN_DOWNLOAD_BASE` and `KILN_UPDATE_ENDPOINT` point
  elsewhere when the repository stays private; see QUESTIONS.
- **Licences travel with the app**: cargo-about writes `THIRD_PARTY_LICENSES.md` (all crates in
  the binary, from their own licence files, offline) and every package carries it with the
  font licences. Kiln's own crates are skipped until it has a licence.
- **App icon**: a kiln's arch with the prompt glowing inside, in the palette's colours
  (`assets/icon/kiln.svg`; PNG, icns and ico made from it).
- **Daily security check**: `security.yml` runs the RustSec advisories check every day, so a
  new advisory against a shipped crate fails a run (GitHub emails about failed scheduled runs)
  without waiting for a push. Renovate (once installed) opens the fix.
- **Renovate** now also bumps the Rust toolchain, cargo-packager and cargo-about, and the test
  image's PowerShell. GPUI and the engine keep their own rules.
- **CI on pushes is Linux only** (build, tests, lint, deny). On 2026-10-19 the account's
  GitHub Actions allowance for private repositories ran out (macOS minutes count ten times,
  Windows twice; a full run cost about 150 minutes and a release dry run about 170). Pull
  requests, the weekly run and manual runs still build macOS and Windows. Undo this if the
  repository goes public (public repositories have no minute limit).
- **Installers use a `dist` profile** (release minus debug line tables; symbol names kept).

## Polish

- **Names** (`naming.rs`): a project is its repository or folder name (`~` for home, "other"
  before a folder is known). A session is its subfolder relative to the project, else the
  program it runs (the host for ssh), else the project's name; repeats within a project get a
  number ("api 2"). The sidebar, the palette and the header use the same labels; the palette
  adds the full folder as dim detail. The OS window title is the breadcrumb.
- **Header**: a dim breadcrumb on the left ("acme-web › api"; the project alone when the session
  is named after it) and the branch on the right, read from `.git/HEAD` (no git process), at
  most 3 s old.
- **Block actions** appear over the end of a hovered block's header instead of reserving room
  in every header, so commands only truncate when they really do not fit next to their timing.
- **Pinned blocks** get a dim dot: orange stays for things to act on.
- **Home** is not a project: sessions in the home folder sit under a quiet "Home" header (no
  colour dot) and are named after the running program, else the shell ("zsh", "zsh 2"),
  never "~". The breadcrumb reads "Home › zsh".
- **The input box** is a raised, rounded box (`raised`, radius 10) inset 8 px from the pane, its
  text lined up with the blocks above (blocks now sit 20 px in). It holds the shell's line:
  inside Kiln the integration gives the shell a minimal prompt, `❯ ` in the accent, or in
  `failed` after a failed command (startup files and Ctrl-C do not count). zsh and bash set it
  last in their prompt hooks, PowerShell in its prompt function (`>` when the console is not
  UTF-8). Starship users keep Starship, pointed at a generated one-character config
  (`STARSHIP_CONFIG`, re-exported each prompt so it wins over their own setting). Line editors,
  plugins, completion menus (the box grows upward) and the user's own zle hooks are untouched;
  outside Kiln nothing changes. Dim context sits inside the box on the right, and steps aside
  when the typed line would reach it. `[shell] prompt = "shell"` keeps the shell's own prompt.
