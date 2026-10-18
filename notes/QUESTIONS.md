# Questions for the user

Collected during the autonomous build; raised at the end of the project.

## Needs a real Windows or Linux machine

- Windows 11 PC: `kiln.exe` (PowerShell), `kiln.exe --wsl`, key repeat in nvim
  (GPUI zed#61469), Alt+key in the shell (zed#62404), mouse wheel in `less` (zed#64695),
  window controls, Ctrl+Shift shortcuts, high-DPI scaling.
- Linux desktop: GNOME on Wayland (client-side title bar), KDE/X11, fractional scaling.

## Needs you on this Mac

- Phase 1 was checked with headless renders (real shells and real rendering, no display): the
  screen was locked while I worked. Please try it by hand: mouse selection across blocks,
  hover actions, the palette and find with the keyboard, splits, ⌘B, ⌘T.

- ⌘C / ⌘V end to end, dead keys (Option+E then E), and the keystroke latency procedure in
  `bench/LATENCY.md` (my tools may not touch the clipboard or type into WezTerm).

## Repository and accounts

- Push the local commits made after 2026-10-18 16:10 to `MaxB-Coder/kiln`? (Pushing is paused.)
  CI then runs the new PowerShell block test on Windows and the headless screenshots on
  macOS and Linux.
- Install the Renovate GitHub app on the repo.
- Choose a licence (none yet). Third-party code inside Kiln: alacritty's PTY loop (Apache-2.0,
  attributed); fonts (OFL + MIT).
