# Questions for the user

Collected during the autonomous build (phases 0–3), for the end of the project.

## Decisions that are yours

None open. The one thing left for you: install the Renovate GitHub app on the repository (its
config is ready).

## Maybe later

- **WSL processes in the rail** ("I might but not sure"): the rail sees `wsl.exe`, not the
  processes inside Linux. A small helper run through `wsl.exe` could fix that; not built.

## Needs a real Windows or Linux machine

- Windows 11: the installer by hand, `kiln.exe` (PowerShell), `kiln.exe --wsl`, key repeat in
  nvim (GPUI zed#61469), Alt+key in the shell (zed#62404), mouse wheel in `less` (zed#64695),
  window controls, Ctrl+Shift shortcuts, high-DPI scaling, notifications (toasts), an update.
- Linux desktop: GNOME on Wayland (Kiln's own title bar), KDE and X11, fractional scaling,
  notifications, the `.deb` from the menu.

## Needs you on this Mac

Most visuals were checked headless (real shells, real rendering, no display), because the
screen was locked or the test window sat behind your work, where macOS stops drawing.

- By hand: mouse selection across blocks, the hover actions, the title-bar drag, IME and dead
  keys (Option+E, E), ⌘C / ⌘V end to end, ⌘I and ⌘., clicking a changed file in the rail (diff
  split) and a port (its dialog), quitting with sessions open and starting again, ⌘K projects.
- The packaged app: `scripts/package.sh`, then open `target/packages/Kiln.app` (or the dmg).
  Notifications only work from the app: run `sleep 12`, switch to another app. macOS asks once
  whether Kiln may notify you; that is your choice, so I never answered it.
- Keystroke latency against WezTerm: `bench/LATENCY.md` (my tools may not type into WezTerm).

## Things I installed or left behind on this Mac

- Tools kept: the Rust toolchain Kiln builds with (rustup), `cargo-packager`, `cargo-about`
  (in `~/.cargo/bin`) and `actionlint` (Homebrew).
- Removed on 2026-10-19 at the user's request: the colima VM with its `kiln-linux` image and
  cargo cache volumes (`~/.colima`), the `colima`, `lima` and `docker` Homebrew packages, and
  the Windows and Linux std targets of the 1.98.1 toolchain. Linux builds and screenshots now
  run in CI.
- `npx` fetched Renovate's config validator into the npm cache once (to check
  `renovate.json`).
- A demo project's `.nvmrc` made mise start installing Node 26.12.0 (phase 2); it was stopped
  before anything was installed. The empty `~/Library/Caches/mise/node/26.12.0` folder it left
  was removed.
- Kiln's own folders: `~/Library/Application Support/Kiln` (logs, saved sessions) and
  `~/Library/Caches/kiln` (shell integration).

## Answered

- Actions minutes and visibility (item 0): the repository is public (2026-10-19), so CI builds
  all three systems on every push again, without minute limits.
- Updates and releases (items 2 and 3): the repository is public, so the default download
  address works. The signing key is yours to make and keep: `./scripts/setup-updates.sh` does
  the key, the GitHub secrets and variables, weekly releases and the first release.
- Licence (item 1): MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`); `pty_loop.rs` stays
  Apache-2.0 only, as it is adapted from alacritty_terminal. Both licences ship in every package.

- Paid code signing (Apple Developer ID, Azure Trusted Signing): "No need" (2026-10-19). The
  workflow's signing steps stay dormant; the README explains the first launch (Open Anyway on
  macOS, More info → Run anyway for SmartScreen).
- Renovate: yes (2026-10-19); you install the GitHub app. Its config is unchanged apart from a
  rule for the AppImage runtime pin.
- Linux AppImage with in-app updates: yes (2026-10-19). Built: `scripts/linux/make-appimage.sh`
  with the runtime pinned and hash-checked; the updater replaces an installed AppImage after a
  yes. The `.deb` stays for manual installs.
- Push to `MaxB-Coder/kiln`: yes (2026-10-18).
