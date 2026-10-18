# Kiln plan

Kiln is a cross-platform terminal app. It must work the same on macOS, Windows (WSL and
PowerShell) and Linux, have the best possible UX, look clean and calm (functionality
revealed on hover or shortcut), and stay current on security updates over time.
No AI integration. Copying output in a paste-ready form is a first-class feature instead.

## Stack

- Rust.
- GPUI (Zed's UI framework, Apache-2.0) for windows and drawing.
- alacritty_terminal (Apache-2.0) as the terminal engine. It stays an unmodified dependency,
  so its security fixes arrive as version bumps.
- The engine's `tty` module for PTYs: Unix PTY on macOS and Linux, ConPTY on Windows.
- Zed's own terminal panel uses this exact pairing. Its `terminal` and `terminal_view` crates
  are reference reading only: they are GPL, so no code is copied from them.

## Layout

- **Sidebar** with spaces: projects, then sessions, with small status dots.
- **Timeline** of commands. Old commands dim. Each command is a block with hover actions
  (Copy, Rerun, Pin, Open log). A failure shows a small dot, not a word.
- **Input bar** pinned at the bottom, with dim context (Node version; WSL/Ubuntu on Windows).
- **⌘I context rail**: changes, running processes and ports, runtimes, session.
- **⌘K palette**: projects, sessions, past commands, actions.
- **⌘F** search. **⌘D** splits side by side. **⌘.** focus mode.
- **⌘⇧C** copies the last output as a paste-ready fenced block: folder, command, output, and
  exit code with timing. **⌥** copies the raw output.

## Windows and Linux

- Windows 11 window controls at top right, with a small `>_` app mark where the traffic
  lights would be.
- Every ⌘ shortcut becomes Ctrl+Shift (the same on Linux).
- Ctrl+Shift+C copies the selection, or the last output when nothing is selected.

## Visual rules

- One surface. No borders, no dividers.
- Commands bright, output muted, metadata dim.
- Orange `#e07b39` only where you act: prompt, cursor, primary action.
- Muted status colours: failed `#b8645a`, running/OK `#8f9a6c`, modified `#c4a35e`.

## Palette

| Token          | Hex       |
| -------------- | --------- |
| bg             | `#282828` |
| bar / sidebar  | `#1d2021` |
| raised         | `#32302f` |
| bg1            | `#3c3836` |
| bg2            | `#504945` |
| muted          | `#928374` |
| fg             | `#ebdbb2` |
| fg_dim         | `#bdae93` |
| accent         | `#e07b39` |

The full palette (ANSI 16 included) is in `~/dotfiles/home/dot_config/wezterm/palette.lua`.

## Blocks and shell integration

- Blocks come from shell integration scripts the app ships and loads automatically
  (zsh, bash, pwsh).
- They use OSC 133 prompt/command/output markers and OSC 7 for the current folder.
- In the first version, the input bar is a pinned view of the shell's own input line, so zsh
  keeps autosuggestions, syntax highlighting, fzf and completions.
- A custom input editor is a later option.

## Phases

0. **Spike** (1–2 weeks). Prove GPUI + alacritty_terminal on all three platforms.
   Done when:
   1. A window opens on macOS and runs the login zsh (`zsh -l`), drawn in JetBrains Mono 13pt
      with the palette above. Typing, resize, scrollback, selection and copy/paste work, and
      nvim runs full screen.
   2. The same builds and runs on Windows (sessions in WSL Ubuntu and in PowerShell) and on
      Linux, with a GitHub Actions matrix from day one.
   3. A notes file compares the prototype with WezTerm on the same Mac: keystroke-to-screen
      latency (rough is fine), idle and busy memory, startup time, and text rendering
      quality, with screenshots side by side.
   4. A clear go/no-go on GPUI + alacritty_terminal, especially GPUI's Windows and Linux
      maturity, plus anything that would change the plan.

   Out of scope: sidebar, command blocks, input bar, palette, rail. The architecture must
   leave room for them.
1. **Daily driver** (3–4 weeks): sidebar, splits, blocks, input bar, copy, search, palette
   basics, config and theme, full-screen apps.
2. **Context** (2–3 weeks): rail, live processes, notifications, project spaces with layouts
   and colours, focus mode, SSH and production tints.
3. **Ship** (1–2 weeks; the pipeline starts in phase 0): signed installers, cargo-packager
   updater, Renovate, cargo deny/audit, weekly releases.

## Security

- The engine stays a plain dependency.
- OSC 52 clipboard reads are off.
- Links ask before opening.
- The window title is never reported back to programs.
- No telemetry. The only network use is checking for updates.
- Signing needs an Apple Developer account and Windows Azure Trusted Signing. Anything that
  costs money is raised with the user first.
