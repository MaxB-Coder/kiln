# Phase 1: daily driver

Date: 2026-10-18. All screenshots are real renders of the app (real shells on real PTYs, real
text shaping, real GPU rendering), made headless with `cargo run -p kiln --example snapshot`,
because the Mac's screen was locked while this was built. The same tool runs in CI.

## What works

| Plan item | Status | Screenshot |
| --- | --- | --- |
| Blocks: one per command, old ones dim, hover actions Copy / Rerun / Pin / Open log, failures as a small dot | Done | [02](screenshots/phase1/02-blocks.png), [03](screenshots/phase1/03-hover-actions.png) |
| Commands bright, output muted (program colours kept), metadata dim | Done | [02](screenshots/phase1/02-blocks.png) |
| Running command shown live, with a green dot and elapsed time | Done | [04](screenshots/phase1/04-running.png) |
| Input bar pinned at the bottom: the shell's own prompt and line editor (autosuggestions, highlighting, fzf all keep working) | Done | [01](screenshots/phase1/01-prompt.png) |
| Dim context in the input bar: Node version in JS projects, `WSL · distro` on Windows | Done (not in the shots: this repo is not a JS project) | |
| Sidebar: sessions grouped by project, status dots | Done | [09](screenshots/phase1/09-sessions.png) |
| Splits side by side (⌘D) | Done | [08](screenshots/phase1/08-split.png) |
| Find (⌘F) with highlights and a match count | Done | [05](screenshots/phase1/05-find.png) |
| Palette basics (⌘K): actions, sessions, past commands | Done | [06](screenshots/phase1/06-palette.png) |
| Copy last output (⌘⇧C) as a paste-ready fenced block; ⌘⌥C raw | Done | [07](screenshots/phase1/07-copy-last-output.md) |
| Config and theme: `kiln.toml`, live reload, palette overrides, text size | Done | |
| Full-screen apps take the whole pane | Done | [10](screenshots/phase1/10-fullscreen-nvim.png) |
| Windows 11 controls and `>_` mark; Ctrl+Shift for every ⌘ shortcut | Done, previewed on the Mac | [11](screenshots/phase1/11-windows-chrome-preview.png), [12](screenshots/phase1/12-windows-chrome-with-sidebar.png) |
| Linux: own title bar and resize edges | Done, not seen on a real Linux desktop | |

The copy from screenshot 07, exactly as it lands on the clipboard:

````markdown
```console
~/developer/kiln $ for i in 1 2 3 4 5 6; do echo "tick $i"; sleep 1; done
tick 1
…
tick 6
# exit 0 · 6.07 s
```
````

## How it is built

- **Shell integration** (`crates/kiln_term/shell/`): zsh, bash and PowerShell scripts mark the
  prompt, the command line and its exit code (OSC 133) and the folder (OSC 7). Kiln loads them
  without touching any startup file (details in `DECISIONS.md`). Shells without integration
  (fish, nu, ssh to another machine) get the classic full terminal.
- **Block recorder** (`blocks.rs`): runs on the PTY thread at each mark, with the engine fed
  exactly up to the mark. A finished command's output is copied out of the live grid, so
  blocks survive scrollback limits and re-wrap when panes resize (screenshot 08).
- **Timeline** (`terminal_view.rs`, `block_element.rs`, `live_element.rs`): a virtualised list
  of frozen blocks, then the running command drawn straight from the live grid, then the prompt
  in the input bar. Only rows in view are laid out, so a 10,000-line block costs what is
  visible.
- **Workspace** (`workspace.rs`): sessions, splits, sidebar, find bar, palette, title strip.

## Tests

- 30 unit and integration tests, all passing. Real shells: zsh, bash 5 and bash 3.2 sessions
  record the right command, output and exit code; a PowerShell test is ready for Windows CI.
- `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo deny` clean.
- `kiln_term` cross-checks clean for Windows and Linux targets. The GUI crate needs each
  platform's SDK and runs in CI (Windows) and in a Linux container (see below).

## Not done or not verified

- **Partly tried in the real window.** With access granted later, keystrokes went through the
  real macOS path (key events, input method, PTY): `echo héllo-宽` and a failing `ls` were
  recorded with the exact command line, exit codes 0 and 2, and the folder change. The window
  stayed behind the user's work, where macOS pauses drawing, so the visuals come from the
  headless renders. Still to try by hand: mouse selection across blocks, the title-bar drag,
  and IME composition.
- Find covers output, not the command lines in block headers.
- Splits are equal width (no dragging) and side by side only.
- Sessions and layouts are not restored after a restart (phase 2: project spaces with layouts).
- Box and block characters still come from the font (seams possible); drawing them is a
  rendering task for phase 2.
- Bash 3.2 (macOS's built-in bash) drops non-ASCII typed input; that is its old readline.

## Next: phase 2

Context rail (⌘I: changes, running processes and ports, runtimes, session), live process
tracking, notifications, project spaces with layouts and colours, focus mode (⌘.), SSH and
production tints.
