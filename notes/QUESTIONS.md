# Questions for the user

Collected during the autonomous build (phases 0–3), for the end of the project.

## Decisions that are yours

0. **GitHub Actions minutes ran out** (2026-10-19). GitHub now refuses to start jobs on this
   account ("spending limit" / "Billing & plans"); nothing was charged. My test runs used the
   monthly allowance: private repositories bill macOS minutes ten times and Windows twice. The
   allowance comes back with the next billing month. To lift it sooner or for good: make the
   repository public (no minute limit, and the updater could then use its release files), or
   raise the spending limit (costs money). Meanwhile pushes build on Linux only (DECISIONS).
1. **Licence.** Kiln has none yet, so nobody else may use the code. MIT or Apache-2.0 (or
   both, like most Rust projects) fit its dependencies; the bundled code and fonts are
   Apache-2.0, OFL and MIT. Once chosen: a `LICENSE` file and `license` in the Cargo.toml files
   (cargo-about then lists Kiln itself too).
2. **Publishing releases.** Nothing is published. The Release workflow builds, installs and
   tests the installers on all three systems and keeps them as artifacts. To publish: run it
   with "publish" ticked, or set the repository variable `KILN_RELEASES=on` for weekly
   releases. Releases of a private repository are private too.
3. **Where updates come from.** The updater needs files it can download without a GitHub login.
   Either make the repository public (the defaults then just work), or host `latest.json` and
   the files elsewhere and set `KILN_UPDATE_ENDPOINT` and `KILN_DOWNLOAD_BASE`. Then make the
   update key (`cargo packager signer generate --path kiln-update.key`; keep the private key
   safe and off this repository), and set `KILN_UPDATE_PUBKEY` (variable) and
   `KILN_UPDATE_PRIVATE_KEY` (+ `_PASSWORD`) (secrets). Without these, builds never check.
4. **Code signing costs money**, so I left it off:
   - macOS: Apple Developer Program (99 USD a year) for Developer ID signing and notarisation.
     Until then the first launch needs Privacy & Security → Open Anyway.
   - Windows: Azure Trusted Signing (about 10 USD a month) or another certificate. Until then
     SmartScreen warns.
   The release workflow already has the steps; they run once the secrets exist (names in
   `.github/workflows/release.yml`). They have never run, so expect a fix or two the first time.
5. **Linux updates.** In-app updates on Linux need an AppImage, and cargo-packager's AppImage
   tools are downloaded unpinned. If you want them, I can add an AppImage step with pinned,
   hash-checked tools.
6. **Renovate**: install the Renovate GitHub app on the repository; its config is ready
   (GPUI, the engine, Rust, cargo-packager, Actions, security fixes at any time).

## Needs a real Windows or Linux machine

- Windows 11: the installer by hand, `kiln.exe` (PowerShell), `kiln.exe --wsl`, key repeat in
  nvim (GPUI zed#61469), Alt+key in the shell (zed#62404), mouse wheel in `less` (zed#64695),
  window controls, Ctrl+Shift shortcuts, high-DPI scaling, notifications (toasts), an update.
- Linux desktop: GNOME on Wayland (Kiln's own title bar), KDE and X11, fractional scaling,
  notifications, the `.deb` from the menu.
- WSL: the rail cannot see processes inside Linux. A small helper run through `wsl.exe` could
  fix that, if you use the rail there.

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

- Tools: Rust toolchains (rustup), `cargo-packager`, `cargo-about` (in `~/.cargo/bin`),
  `actionlint`, `colima` and `docker` (Homebrew). The colima VM is stopped; its `kiln-linux`
  image and two cargo cache volumes take disk space (`docker image rm kiln-linux`,
  `docker volume rm kiln-cargo-registry kiln-cargo-git` with colima running).
- `npx` fetched Renovate's config validator into the npm cache once (to check
  `renovate.json`).
- A demo project's `.nvmrc` made mise start installing Node 26.12.0 (phase 2); it was stopped
  before anything was installed. Left: an empty `~/Library/Caches/mise/node/26.12.0` folder.
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

- Push to `MaxB-Coder/kiln`: yes, the private repository (2026-10-18).
