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
  (`assets/icon/kiln.svg`; PNG, icns and ico made from it). Since 2026-10-24 the arch holds
  the prompt's own flame (frame 1 of `assets/flame`, concept A, "Fired").
  `scripts/make-icons.sh` makes every format from the SVG: the .icns keeps the macOS grid's
  margin (824 of 1024); the .ico and the Linux PNGs are cut to the body (248 of 256), as
  Windows and Linux icons fill their tile. kiln.exe now carries the .ico as a resource
  (`build.rs`, winresource; before, Explorer showed the default program icon), the installer
  shows it too (`installerIcon`; it had NSIS's default), and
  `scripts/check-icons.sh` / `.ps1` check every package against the assets, byte for byte,
  in the release workflow (and the exe in CI).
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

## After phase 3: the user's decisions (2026-10-19)

- **No paid code signing.** The Developer ID, notarisation and Azure steps stay in the release
  workflow but never run; the README explains the first launch on macOS and Windows.
- **AppImage on Linux**, built by `scripts/linux/make-appimage.sh`: the stripped binary in an
  AppDir, squashed with the distribution's `mksquashfs` (zstd), behind the AppImage project's
  type2 runtime. The runtime is pinned to release 20251108 and its SHA-256 is checked before
  use (a mismatch stops the build); Renovate proposes newer releases and the hash is updated by
  hand. Not cargo-packager's AppImage step, which downloads its tools unpinned. Built on
  Ubuntu 22.04 for older glibc. The `.deb` and tarball stay for manual installs.
- **In-app updates on Linux** replace the AppImage the app runs from. Kiln reads `APPIMAGE` at
  startup and removes the runtime's variables (`APPIMAGE`, `APPDIR`, `ARGV0`, `OWD`), so its
  shells do not inherit them; after an update it restarts the AppImage, not the binary in the
  old image. `latest.json` gets a signed `linux-x86_64` entry.
- **`kiln_update`**: the updater's core (manifest, signature-checked download, install, where
  to install) moved into its own crate without UI code, so the release workflow tests the
  packages it has just built (`accepts_a_signed_release_build`) in seconds.
- **Release artifacts** hold only the release files; the smoke output is a separate artifact.
  With both in one artifact the files kept a `target/packages/` prefix, so the first published
  run found no update files and stopped before creating the release.
- **Renovate**: yes; the user installs the app. **WSL processes in the rail**: maybe later.
- **The Windows update entry** was missing from every manifest: cargo-packager names the
  installer after the package (`kiln_<version>_x64-setup.exe`), and the manifest script looked
  for `Kiln_*`. The manifest check now requires all three systems, so a missing one fails the
  run before anything is published.
- **Keyboard shortcuts pop-up** (⌘/, Ctrl+Shift+/; also ⌘K and the macOS Help menu): built
  each time from the keymap in effect, so it lists exactly what is bound on that platform.
  `keymap.rs` holds the bindings (registered by `main`) and one table giving each bound action
  its name, section and order; a test fails when a bound action has no entry, or does not
  appear exactly once with its keys. Keys read the platform's way (⌘⇧ glyphs in Apple's order on
  macOS, "Ctrl+Shift+D" elsewhere; the Windows chrome preview uses the Windows bindings). The
  filter matches names and keys ("split", "⌘D", "cmd+d", "ctrl+shift+d"). The palette's
  shortcut hints now come from the keymap too: several were wrong on Windows and Linux
  (Next session showed Ctrl+Shift+] where the binding is Ctrl+Tab).
- **The concept's ten gaps** (`notes/concept/kiln-concept.html`, checked in
  `notes/concept-check.md`): pills in the input box (project, branch and change count,
  runtimes, WSL distribution) replace the dim context line; the header shows the branch and
  "✱N", or "● N running" in focus mode, which narrows the timeline to 72% (at most 100
  columns). ⌘↑/⌘↓ (Ctrl+Shift+↑/↓) select whole commands, ⇧ extends (Ctrl+Alt+Shift+↑/↓ on
  Windows and Linux, since Ctrl+Shift+↑/↓ is taken by moving), and one copy gives all of them
  in one fence. A note above the input box confirms each copy for 2.5 s. Pinned commands leave
  the timeline for a strip at its top (3 lines, a click shows up to 24, "pinned · HH:MM" in
  local time through `jiff`) and survive `clear`; a find match below the fold unfolds the pin.
  Find is a bar across the top of the pane; the timeline starts below it, while the terminal
  keeps its size. The palette groups results (Commands, Projects, Sessions, Actions, 4 rows
  each, 12 when one group has them all) with no type column. The sidebar ends with the
  palette's keys as a link.
- **Header pills and the flame** (the concept's revised items 1 and 2; the in-box pills of
  5da7876 lasted one day). Each pane has its own header line: its name, dim, and on the right
  the pills (project, branch with ✱N, runtimes, WSL distribution), with "● N running" before
  them in focus mode. The headers replace the window-wide title strip; the traffic lights, the
  `>_` mark and the window controls sit over the panes' corners and the headers keep clear of
  them. A runtime pill needs a marker file in the project: from the folder up to its
  repository's root, or the folder alone outside one; never the home folder.
- **The flame prompt**: the minimal prompt is now two blank cells (zsh, bash, PowerShell, and a
  Starship config whose format is just those). Kiln paints the flame over the first, at the
  prompt's OSC 133 start (`LiveState::prompt_column`), from the four SVG frames in
  `assets/flame` parsed at startup and filled as vector paths (so it is as sharp as the text
  at any scale). It moves every 150 ms only while its window is active, its pane has the
  keyboard and the system allows motion (macOS Reduce motion, Windows animation effects,
  GNOME's enable-animations); otherwise it holds frame 1, checking twice a second. While a
  command runs or a full-screen program is open there is no flame and no timer. Red follows
  the last exit code (130, Ctrl-C, does not count). `--bench-flame` keeps it moving without
  focus, to measure what it costs.

- **Name pill outside projects (2026-10-25, the user's call).** Home and plain folders keep a
  single name pill ("Home", the folder name) in the header, with no branch or runtime pill.
  Projects show project, branch and runtime pills as before.
- **Settings pop-up** (⌘, / Ctrl+Shift+,, the EditConfig binding; "Open config file" in it and
  in the palette does what ⌘, did before). Every row is an existing key in kiln.toml, plus the
  new `icon`. A change applies at once and is written with `toml_edit` (`config_write.rs`):
  only that value changes, keeping its spacing and trailing comment; a missing key is added to
  its table (a top-level one before the first table), a missing table at the end, and a
  missing file starts from the commented template. The write is atomic (a temporary file
  renamed over it; through a symlink, onto its target, keeping its permissions). A file that
  does not parse, or holds a value Kiln cannot use, is shown in the pop-up and never written.
  Each window remembers the config it applied, so the file watcher's reload after Kiln's own
  write changes nothing (no flicker, no loop). The prompt style applies live, see below.
  Keyboard: Tab or ↑↓ move, ←→ change, Space or Enter toggle, Esc closes. The footer shows
  this platform's key only.
- **App icon variants** (first Terracotta, Black and White; now the designs below,
  `assets/icon/variants`, made by `scripts/make-icons.sh`). Switched while Kiln runs, from PNGs built into the binary:
  macOS sets the Dock icon (`NSApplication.applicationIconImage`; the default resets it to the
  bundle's). Finder, Launchpad and the app switcher's file icon keep the bundle icon: changing
  the bundle would break its signature. Windows sets each window's title-bar and taskbar icon
  (`WM_SETICON`, sized for the window's DPI); the exe and installer stay terracotta. Linux
  sets `_NET_WM_ICON` on X11 windows (x11rb, already in the tree); on Wayland an app cannot
  set its window icon (no stable protocol; `xdg-toplevel-icon` is not widely available), so
  the compositor shows the desktop file's terracotta icon.
- **App icon designs**: Kiln (the default), Flame, Beehive and Line, each in seven colours
  (terracotta, black, white, ember, slate, sage, sand), and Custom. `icon` names the design;
  `[icon_colours]` holds each design's own colour (terracotta when missing), so switching
  design brings back that design's colour. Beehive's kiln is always brick; only its tile takes
  the colour. The 28 masters are written by `scripts/make-icon-masters.py` (standard library)
  from the approved mockup's tables (`notes/concept/kiln-icons.html`); its Kiln output is the
  hand-made masters byte for byte (halves rounded up, as they were), and Kiln terracotta stays
  `assets/icon/kiln.svg`, so the bundle, exe and installer icons are unchanged. CI runs it
  with `--check`. `icon = "black"` and the other old values still load as the Kiln design in
  that colour; the first write from Settings turns them into `icon = "kiln"` plus
  `[icon_colours] kiln = "black"`. Unknown designs or colours warn and fall back to Kiln and
  terracotta. Each platform embeds only the PNGs it uses: the 128 px Settings tiles everywhere
  (255 KB for all 28), then the Dock at 256 px on macOS (128 pt at 2x is its largest; 789 KB
  in all), window icons at 16 to 128 px on Windows (876 KB) and 16, 32, 48, 64 and 128 px on
  Linux (841 KB). The three variants before took about 150 to 200 KB, so the binary grows by
  about 0.6 to 0.7 MB. Rendering the SVGs at run time (resvg is in the tree through GPUI)
  would be smaller, but PNGs keep start-up and the icon switch free of rasterising.
- **Custom app icon**: the "+" tile opens the system file picker (GPUI's
  `prompt_for_paths`). Only PNG and JPEG are read, told apart by their first bytes, never the
  file name, through the `image` crate already in the tree (its `jpeg` feature uses
  zune-jpeg, also already there). Over 10 MB, over 4096 px a side (checked from the header,
  before decoding), or anything else is refused with a message under the row. The picture is
  cropped square from the middle, scaled to at most 512 px, given the tiles' rounded corners
  and saved as a PNG copy in `icons/` next to kiln.toml, named after the original with a hash
  of its bytes in front (a new image is a new path, so every window updates); `icon_path`
  points at the copy, so moving the original changes nothing. "Remove" goes back to Kiln in
  its colour and deletes the copy (only a PNG in that folder). The copy is checked again each
  time it is read; if it is gone or broken, the default icon shows, with a warning. The Dock
  gets it on the macOS grid (824 of 1024), windows get it filling the tile, like the drawn
  icons.
- **Themes** (`[theme] name`): Kiln Dark (the default palette, unchanged), Kiln Light, System
  (Kiln Dark or Light with the OS appearance, followed live through GPUI's window appearance
  on all three systems; dark where it is not known), Catppuccin Mocha, Everforest, Nord and Kanagawa
  (MIT palettes, credited in `assets/themes/THEME-LICENSES.md`, which every package ships),
  and Custom. They are a table in `theme.rs`, not files. Custom lives in its own table,
  `[theme.custom]` (`base` plus colours), used only when `name = "custom"`: Custom is one more
  card, a preset is exactly that preset, and choosing Custom again brings its colours back.
  Choosing any theme in Settings also removes colour keys right in `[theme]`; a file from
  before `[theme.custom]` (colours and `base` in `[theme]`) still loads, as Custom when it has
  no name, and the first theme choice in Settings moves them into `[theme.custom]`. Colours
  written by hand in `[theme]` next to a preset name still apply on top of it. An unknown
  name logs a warning and gives Kiln Dark. Custom from a base writes `name`, and the base and
  every colour in `[theme.custom]`, so the file describes the theme on its own. The picker (a saturation/value square and a hue strip, drawn with GPUI
  gradients) shows a colour while it is dragged and writes it on release; hex fields write on
  each valid `#rrggbb`. Everything takes its colours from the palette: the flame's inner colour
  is `blend(accent, fg, 0.45)` (from `failed` after a failure), project dots use palette
  colours, and the veil and pop-up shadows are warm and light on light themes, near black on
  dark ones (by background luminance). The Windows close button keeps the system red. The app
  icon is its own setting, not tied to the theme.
- **Prompt style switches live.** `KILN_PROMPT` in the environment only reached new sessions,
  so each Kiln process keeps the style in a small file (`<temp>/kiln-prompt-<pid>`, "kiln" or
  "shell"; its path in `KILN_PROMPT_FILE`, through WSLENV as a path) that the zsh, bash and
  PowerShell scripts read before every prompt. Changing the setting rewrites the file, so every
  open session follows at its next prompt; one waiting at an empty prompt gets an Enter, so
  it draws its new prompt at once (seen without it: switching looked like it did nothing until
  the next command). An empty prompt is the cursor where typing starts with the rest of the
  line blank; anything typed, or a running program, is left alone. With the shell's own
  prompt the input box shows only the line typing starts on (from the OSC 133 input mark):
  lines above it, such as a two-line Starship prompt's folder and git pills, repeat what the
  header shows, so "Shell's own" reads as the prompt character (e.g. `❯`) where Kiln's reads
  as the flame. Two things keep the pills from flashing up on a switch: a reported style
  applies from the next prompt's start mark (the shell reports in its hook, before Starship has
  drawn anything, and the prompt on screen keeps its own style until then), and while a shell
  prompt is being drawn, before its input mark, the box is one blank line. The scripts keep the user's own prompt and
  `STARSHIP_CONFIG` from when they loaded and put them back for "shell". Each prompt reports
  its style (`OSC 1337;KilnPrompt=kiln|shell`), and Kiln draws the flame only where the shell
  printed Kiln's two blank cells; the blank first line Starship adds is not kept above a shell
  prompt in the input box.
- **Everforest and Kanagawa replace Tokyo Night**: eight cards, two full rows. A config still
  naming `tokyo-night` gets Kiln Dark with the unknown-name warning. Everforest is the dark,
  medium palette (sainnhe/everforest `autoload/everforest.vim`); every value matches upstream.
  Its bright black is `bg5` (#56635f), where upstream repeats black (#475258), so bright black
  stays visible. Kanagawa is the wave palette (rebelot/kanagawa.nvim `colors.lua`, `themes.lua`),
  with two corrections to the values first proposed: faint text #625e5a is the Dragon
  variant's `dragonBlack6`, so it is wave's `sumiInk6` #54546d (its non-text colour, the same
  as the dividers); ANSI black #090618 is the old terminal extras, so it is #16161d
  (`sumiInk0`, what wave's terminal palette uses now).
- **Renaming sessions and projects; project colours.** Double-click a sidebar row, or
  right-click it and choose Rename: the name is edited in place, Enter keeps it, Esc drops it,
  and an empty name brings the automatic one back. Keys: ⌘R and F2 rename the active session
  (Ctrl+Shift+R on Windows and Linux, as `k()` maps the others), ⌘⇧R its project
  (Ctrl+Alt+Shift+R, like the other ⇧ pair); none of them was bound. F2 is bound in the
  terminal only while no full-screen program runs (the terminal's key context gains
  `full_screen` then), so htop, mc and others keep it. The palette has "Rename session",
  "Rename project" and "Project colour". The editors live in the sidebar, so these open it when
  it is hidden. A session's name goes with the saved sessions (`state.json`, `TabState.name`,
  read as unnamed from older files), never the config, and it no longer follows the folder or
  the program. A project's name and colour go into `[projects."~/path"]` through the
  `toml_edit` writer (`name`, `color`); a project keeps the key it has, matched as written or
  with its links resolved (Kiln's roots are resolved: `/private/var` on macOS), and a new one
  is written with `~` for home. The name shows wherever the project's does (sidebar, header
  pill, palette, session labels), since `naming::project_name` reads it. Colours are sixteen fixed
  names (`state::PROJECT_COLOURS`: red, vermilion, orange, amber, yellow, lime, green, teal,
  cyan, sky, blue, indigo, violet, magenta, pink, grey), the same in every theme. They were
  the theme's ANSI colours at first, but in Kiln Dark (Gruvbox) the blue and cyan read as
  teal and green, so "Blue" did not look blue; the first set's names (red, orange, yellow,
  green, cyan, blue, magenta, grey) are among the sixteen, so those configs still work, as
  does "#rrggbb"; anything else is Auto. A chosen colour puts the project and its sessions on
  a card (radius 9, 3 px inside) tinted with it at 14% over the sidebar (22% on light themes,
  where a light wash is harder to see); the project's name is bold, in the colour mixed 60/40
  with the text; the active session's row is the card a little darker. The dot is gone. Auto
  is no card, the plain look; the header pill takes the full colour (Auto keeps the accent).
  The colour pop-up sits under the project: "Colour · <project>" and esc, an Auto row with a
  dashed ring, the sixteen in two rows of eight equal columns with the hovered or chosen
  one's name below, and a Custom "#rrggbb" field with a live preview dot. The current colour has
  the accent ring and the keyboard's place a thin dim one, so the two never look alike; the
  name below is the hovered, focused or current colour's. A "#rrggbb" equal to one of the
  sixteen, stored or typed, is that swatch (and is saved by its name), with Custom left empty.
  It and the right-click menu take the keys: the arrows move (↓ from the grid goes to the
  custom field, ↑ from it back), Enter picks, Esc closes.
- **Header and sidebar hint.** The name at the top left of the panes is gone (the header pill
  shows the project); the header keeps its height and the empty space stays a drag area. The
  sidebar's bottom line shows the shortcuts key from the keymap ("⌘/ Shortcuts",
  "Ctrl+Shift+/ Shortcuts") instead of ⌘K, and opens the pop-up.
- **Shortcuts pop-up coverage.** A reported missing "Command palette" did not reproduce: it is
  listed in both layouts (in one column, under 900 px wide, below the fold, and the list
  scrolls to it; see `notes/screenshots/shortcuts/s7-narrow-scrolled.png`). A test now builds
  the pop-up view from each platform's live keymap and checks that every described action
  with a binding is shown (`shortcuts::tests::the_popup_shows_every_bound_action`).
- **One key for the palette.** ⌘K (Ctrl+Shift+K) only; the second binding, ⌘P
  (Ctrl+Shift+P), is gone and left unbound (a test checks nothing takes it).
- **The context rail starts at the top.** It sat below an empty title-bar strip plus its own
  16 px. Now its first section's title is in the header row, level with the pills. The strip
  stays only where the window controls are (Windows and Linux, at the rail's top right); on
  macOS they are on the left, so a 24 px strip lies over the first title to move the window
  by, and nothing else.
- **"Quiet": less on screen, same layout.** The header keeps the project and git pills; the
  runtime pill is gone (the rail lists runtimes). A command's time shows only from 1 s up
  (`copy::shown_duration`; most commands are quicker, so the slow ones stand out); copies, logs
  and notifications keep the exact time. Command lines show paths in the home folder as `~/…`
  (`copy::display_command`, the home path as a whole word or before "/"), for the eye only:
  copies, logs and Rerun use the command as typed, and while find highlights a command it
  shows as typed (the highlights index the typed text). Older blocks already sat at 60%, with
  the latest, pinned, hovered and selected ones at full strength; that stays. The rail:
  Changes counts the files (the branch is in the pill) and, like Ports and Runtimes, is left
  out when empty; Processes always shows, "Nothing running" included; Session is one muted line,
  "zsh · 30 s", with a failed last command's exit at its end in red, and the size, pid,
  command count and last exit under the mouse or after a click (the size was on the line at
  first; it was not worth the room).
- **Resizable sidebar and rail.** Each panel's inner edge is a 6 px handle with the resize
  cursor and a 1 px accent line on hover and while dragging (nothing otherwise). The sidebar
  goes from 160 to 420 px (default 220), the rail from 180 to 480 (default 280); a
  double-click on the edge gives the default back. The terminal keeps at least 40 columns
  (`panels::terminal_min`): while it is short of room the rail gives way to its least, then
  the sidebar (`panels::widths`), and a drag stops there; only a window too small even then
  squeezes the terminal. The layout follows the mouse; the shells keep their size until the
  drag ends (`terminal_view::hold_resize`), then each gets its new size once, so full-screen
  programs do not redraw at every step. The drag listens at the window level (capture), so a
  pane under the mouse cannot keep the moves. The widths go into the saved sessions
  (`WindowState.sidebar_width`, `rail_width`; older files read as the defaults), not the
  config. No key binding.

