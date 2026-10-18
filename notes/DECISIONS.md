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
