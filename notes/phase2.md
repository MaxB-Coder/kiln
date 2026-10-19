# Phase 2: context

Date: 2026-10-19. The screenshots are real renders of the app (real shells on real PTYs, real
text shaping and GPU rendering), made headless with
`cargo run -p kiln --example snapshot -- notes/screenshots/phase2 phase2`. The scenario builds
a small git project (`acme-web`) in a scratch folder, so the rail has something to show. The
`ssh` in the tint shots is a stand-in shell function: nothing connected anywhere.

## What works

| Plan item | Status | Screenshot |
| --- | --- | --- |
| ⌘I context rail: changes, processes and ports, runtimes, session | Done | [13](screenshots/phase2/13-rail.png) |
| Live process tracking: everything under the pane's shell, with CPU and memory, refreshed every 2 s while the rail is open | Done | [13](screenshots/phase2/13-rail.png) |
| Changes: branch, ahead/behind, changed files; clicking a file pages its diff in a split | Done | [13](screenshots/phase2/13-rail.png) |
| Ports the pane's programs listen on; clicking one asks, then opens `http://localhost:PORT` | Done (the dialog not seen: it needs the real window) | [13](screenshots/phase2/13-rail.png) |
| Notifications: a command of 10 s or more that finishes out of sight posts a system notification; clicking it brings the session back | Done; shows only from the app bundle on macOS (see below) | |
| Attention dot: a bell or a finished long command marks the session orange until it is shown | Done | [17](screenshots/phase2/17-attention.png) |
| Focus mode (⌘.): only the focused pane | Done | [14](screenshots/phase2/14-focus-mode.png) |
| SSH tint: panes running ssh or mosh get a faint blue and a corner label | Done | [16](screenshots/phase2/16-remote-tint.png) |
| Production tint: hosts matching `[tints] production` (default `*prod*`) get a faint red | Done | [15](screenshots/phase2/15-production-tint.png) |
| Project spaces with layouts: sessions (folders and splits), sidebar and rail come back at launch; each project keeps its layout | Done | [19](screenshots/phase2/19-restored.png) |
| Projects in the ⌘K palette | Done | [18](screenshots/phase2/18-palette-projects.png) |
| Project colours in the sidebar (stable per folder, or set in the config) | Done | all phase 2 shots |
| Windows and Linux: the window controls move to the top of the rail when it is open | Done, previewed on the Mac | [20](screenshots/phase2/20-windows-chrome-with-rail.png) |

## How it is built

- **Process watcher** (`kiln_term/src/process.rs`): `sysinfo` reads the process table, and
  `listeners` the TCP sockets in the listening state. The rail shows the tree under the pane's
  shell (the shell's PID now comes from the PTY), and the ports those processes own.
- **Runtimes** (`context.rs`): the shell integration reports where `node`, `python3`, `ruby`,
  `go`, `rustc`, `java`, `deno` and `bun` are on its PATH (`OSC 1337;KilnTools=…`). A runtime
  shows when the project has its marker file (`package.json`, `Cargo.toml`, `go.mod`, …), with
  the version read from the install path or from `--version` (once per binary, off the UI
  thread).
- **Git** (`git.rs`): `git status --porcelain=v2 -z`, with fsmonitor off, no optional locks and
  no prompts, so a repository's config cannot run anything when you `cd` into it.
- **Rail** (`rail.rs`): reads in the background every 2 s while open and right away when the
  focus moves; a slow read is never stacked on another.
- **Notifications** (`workspace.rs`): GPUI's system notifications (macOS user notifications,
  Windows toasts, Linux desktop notifications). The title is only the program and its first
  argument (`cargo test`), because notifications can show on a locked screen and the rest of
  a command line may hold secrets.
- **Tints** (`remote.rs`): the host is read from the running command line, including options
  that take values (`ssh -p 2222 -i key me@host`), `ssh://` URLs, IPv6 and mosh's options.
- **Saved state** (`state.rs`): `state.json` in Kiln's data folder, private (0600) and written
  atomically, only when something changed.

## Tests

- 50 tests in all. New in this phase: ssh and mosh parsing, production patterns, colour
  mixing, notification tags and titles, state merging and JSON compatibility, project colours,
  labels, git status parsing, runtime versions, and the process watcher on a real child
  process, and marks following reflowed text. The phase 1 shell tests (zsh, bash 5,
  bash 3.2, PowerShell) still pass.
- macOS: tests, clippy and the phase 1 and phase 2 scenarios. Startup unchanged: 274 ms to the
  first frame with the prompt (phase 0: 310 ms). A benchmark run does not save or restore
  sessions.
- Linux container (`scripts/linux-check.sh`, now runs both scenarios): tests, clippy with
  warnings as errors, and the screenshots in `screenshots/linux-phase2/` (bash; python serves
  the demo there, since the image has no node).

- CI ([run 39254173739](https://github.com/MaxB-Coder/kiln/actions/runs/39254173739), private):
  all six jobs green on the final commit: macOS arm64, Linux x64, Windows x64 (tests and a
  PowerShell session on ConPTY), a WSL Ubuntu session, clippy with warnings as errors, and
  cargo-deny.

Bug the Linux shots caught and fixed: opening the rail narrows the pane, the engine re-wraps
the lines, and the row where a running command's output starts pointed into its own wrapped
command line (the block began with `.0.0.1`, the end of `--bind 127.0.0.1`). The prompt,
input and output marks now move with the text on every resize. Before the fix this hit any
long command running while a pane changed width.

## On macOS, notifications need the app bundle

GPUI only posts notifications from an app bundle; a bare `cargo run` binary skips them
quietly. `scripts/bundle-macos.sh` makes `Kiln.app`. The first notification asks for
permission (a macOS prompt); I did not answer it for you.

## Not done or not verified

- **Not tried in the real window**: clicking a changed file (the diff split), the port dialog,
  ⌘I and ⌘. from the keyboard, a notification and its click, and restore after a real quit.
  The headless scenario covers the same code paths, but not the platform dialog or the
  notification centre.
- **WSL sessions**: the rail sees `wsl.exe`, not the processes inside Linux, so it says
  "Nothing running". Ports forwarded by WSL do show when Windows owns the socket.
- **Remote shells** with their own Kiln integration are not detected as remote (the tint comes
  from the local `ssh` command line). `kubectl exec`, `docker exec` and ssh started by a
  script are not tinted.
- **Several windows**: the window that changed last is the one restored.
- Restore brings back folders and splits, not scrollback or running programs.

## Side effect on this Mac

The first run of the scenario put a `.nvmrc` with `26` in the demo project. Your mise saw it
when the scenario ran `node` and started installing Node 26.12.0; the scenario's Ctrl-C
stopped the install, so nothing was installed (mise still lists 24.12.0 and 25.0.0). Left
behind: an empty `~/Library/Caches/mise/node/26.12.0` folder and mise's cached list of
remote versions. The demo has no `.nvmrc` now.
