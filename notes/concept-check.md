# Concept check

Each item of `notes/concept/kiln-concept.html` against Kiln as built, rendered headless with
the real renderer and real shells:

```bash
cargo run -p kiln --example snapshot -- notes/screenshots/concept-check concept
```

The `w*` shots are the Windows chrome preview (`KILN_CHROME=windows`) with the Windows and
Linux key bindings. Screenshots are in `notes/screenshots/concept-check/`. The demo projects
are scratch git repositories: `acme-web` (Node, four changes), `notes-site` (clean) and
`rust-cli` (Rust, clean); `scratch` is a folder outside git. The run asserts what a picture
cannot show (the flame's frames changing, holding still, the sidebar hint opening the palette).

| # | Item | Concept state | Kiln | Verdict |
| --- | --- | --- | --- | --- |
| 1 | Header pills, in a repository | Left: the session name, dim, no breadcrumb. Right: project (#e07b39, dark bold text), branch with its glyph and "✱2" in orange (#3c3836, #d5c4a1 text), runtime (#32302f, muted, "node 25"); about 18 px high. | [01](screenshots/concept-check/01-header-repo.png): "acme-web" dim; `acme-web`, ` main ✱4`, `node 25`. | Matches |
| 1 | Clean repository | No count. | [01b](screenshots/concept-check/01b-header-clean.png): `notes-site`, ` main`. | Matches |
| 1 | Plain folder | No branch pill. | [01c](screenshots/concept-check/01c-header-plain-folder.png): `scratch` only. | Matches |
| 1 | Home | No branch, no runtime. | [01d](screenshots/concept-check/01d-header-home.png): name "zsh", a `Home` pill only. | Matches |
| 1 | Windows | Pills left of the window controls; WSL distribution as a pill. | [w01](screenshots/concept-check/w01-header-pills.png). The WSL pill: `pills::tests::outside_a_repository_there_is_no_branch` ("WSL · Ubuntu" → "Ubuntu"); a WSL session cannot run on macOS. | Matches (WSL by unit test) |
| 1 | Split panes, focus mode | Each pane its own header and pills; in focus mode "● N running" just left of the pills. | [12d](screenshots/concept-check/12d-flame-split.png): two headers. [03](screenshots/concept-check/03-focus-mode.png), [w03](screenshots/concept-check/w03-focus-mode.png): "● 1 running" before the pills. | Matches |
| 2 | Runtime only where it belongs | A runtime pill only when the project has its marker file (package.json, .nvmrc, .node-version; Cargo.toml, rust-toolchain(.toml); go.mod; pyproject.toml, .python-version, requirements.txt); never in Home or plain folders, never in the input box. | `node 25` in acme-web ([01](screenshots/concept-check/01-header-repo.png)), `rust 1.98` in rust-cli ([02](screenshots/concept-check/02-runtime-rust.png)), none in [01c](screenshots/concept-check/01c-header-plain-folder.png) and [01d](screenshots/concept-check/01d-header-home.png). Test: `context::tests::runtimes_only_where_the_project_uses_them` (Cargo.toml in Home ignored; repository searched to its root; plain folder only itself). | Matches |
| 2 | Flame prompt, ready | The input box holds only the flame and what is typed; four frames swapped every 150 ms, 1→2→3→4; outer #e07b39, inner #f2b36b; about one cell wide, a little under the line height. | [12](screenshots/concept-check/12-flame-ready.png). The four frames as Kiln drew them, enlarged 4×: [12a](screenshots/concept-check/12a-flame-frames.png) (the run waits for each frame in turn). The shell prints two blank cells; Kiln paints the vector paths from `assets/flame` at the OSC 133 start. Tests: `flame::tests`, `zsh/bash5/bash3/pwsh_minimal_prompt` (two blank cells, typing starts at column 2). | Matches |
| 2 | Flame after a failed command | Outer #b8645a, inner #d0927f; Ctrl-C and startup files do not count. | [12b](screenshots/concept-check/12b-flame-failed.png) after `false`. | Matches |
| 2 | Holds still | Frame 1 without window focus, with reduced motion, while a command runs or a full-screen program is open. | Asserted: frame 1 while the palette has the keyboard, and with reduced motion ([12c](screenshots/concept-check/12c-flame-reduced-motion.png)). Window focus uses the same check (`Window::is_window_active`). While a command runs there is no flame and no timer. | Matches |
| 2 | Split panes | Each input box has its flame; only the focused pane's moves. | [12d](screenshots/concept-check/12d-flame-split.png); asserted over 1.2 s: the right pane's flame changes frames, the left one stays on frame 1. | Matches |
| 2 | Idle cost | Only the flame redraws; idle CPU close to idle. | A frame redraws the workspace only (panes are cached views): 0.21 ms of CPU per frame in a release build, so 0.14% of one core at 6.7 frames a second (`snapshot … bench-flame`; it was 0.64 ms, 0.43%, before the panes were cached). A real window behind others: 0.10% of one core over 30 s with the flame running (`--bench-flame`), the same as without it, because macOS stops frames for covered windows. GPU work for a visible window comes on top and was not measured. | Matches |
| 3 | Focus mode (⌘.) | Timeline and input box about 72% wide, centred, at most ~100 columns; the header's right side shows what is running. | [03](screenshots/concept-check/03-focus-mode.png): 72% of the window, centred, "● 1 running" (a `sleep` in another session) before the pills. [w03](screenshots/concept-check/w03-focus-mode.png) Windows. | Matches |
| 4 | ⌘↑ / ⌘↓ | A selection moves between commands, hover background #2f2d2c, actions shown, scrolled into view; ⌘C or Enter copies, Esc clears. | [04](screenshots/concept-check/04-select-command.png): two ⌘↑ select `true` (raised background, Copy · Rerun · Pin · Open log). [w04](screenshots/concept-check/w04-select.png) with Ctrl+Shift+↑. Logic: `terminal_view::tests::stepping_through_commands`. | Matches |
| 5 | ⇧-click, ⇧⌘↑/↓ | Extends the selection; copy gives one paste-ready block, in order. | [05](screenshots/concept-check/05-extend-selection.png): two commands selected; the copy is [05-multi-copy.md](screenshots/concept-check/05-multi-copy.md) (one fence, oldest first). Tests: `extending_keeps_the_anchor`, `copy::tests::several_commands_share_one_block_in_order`. | Matches |
| 6 | Copy note | Above the input box for ~2.5 s: "⧉ Copied, ready to paste" (or "Copied raw output") and the first lines dim; panel #1d2021, ring #504945, radius 10, 60% wide. | [06](screenshots/concept-check/06-copy-note.png): the note with the first 8 lines; [06b](screenshots/concept-check/06b-note-gone.png): gone after 2.5 s, selection cleared. | Matches |
| 7 | Pinned commands | At the top of the timeline: header and a few lines, quiet grey dot, "pinned · time"; click to expand; unpin from the hover actions. | [07](screenshots/concept-check/07-pinned.png): `git status --short` pinned, then `clear` and two commands: it stays on top, 3 lines, "pinned · HH:MM" (local time). [07b](screenshots/concept-check/07b-pinned-hover.png): Copy · Rerun · Unpin · Open log. [07c](screenshots/concept-check/07c-pinned-open.png): unfolded. Test: `pinned_blocks_stay_through_clear_and_new_commands`. | Matches |
| 8 | Find bar | Across the top of the timeline, #32302f, radius 8: "⌕ query" left, "2 of 5 · ⏎ next · ⇧⏎ previous · esc" dim right. | [08](screenshots/concept-check/08-find.png): "⌕ auth", "1 of 1 · ⏎ next · ⇧⏎ previous · esc", edges in line with the blocks; the match in the pinned block unfolds it. [w08](screenshots/concept-check/w08-find.png) Windows. Test: `the_find_bar_says_where_you_are`. | Matches |
| 9 | Palette (⌘K) | Groups Commands, Projects, Sessions, Actions under quiet headings; dim detail right ("rerun · project", "open", shortcut); no type tags; `❯` before the query; box 58%, #1d2021, radius 12, ring #3c3836. | [09](screenshots/concept-check/09-palette.png): all four groups, project dots, "rerun · acme-web" on the selected command. [09b](screenshots/concept-check/09b-palette-query.png): "git" leaves Commands. [w09](screenshots/concept-check/w09-palette.png): Windows keys. Tests: `results_sit_under_their_group_in_order`, `groups_are_capped_unless_alone`. | Matches |
| 10 | Sidebar hint | A dim "⌘K" (Ctrl+Shift+K) at the bottom of the sidebar; a click opens the palette. | Bottom left in every shot with the sidebar, e.g. [01](screenshots/concept-check/01-header-repo.png); "Ctrl+Shift+K" in [w01](screenshots/concept-check/w01-header-pills.png). [10](screenshots/concept-check/10-sidebar-hint.png): the palette opened by a click on it (asserted). | Matches |
| 11 | Shortcuts pop-up (⌘/) | Lists the new bindings under "Commands and copy": "Select previous / next command ⌘↑ ⌘↓", and adding to the selection. | [11](screenshots/concept-check/11-shortcuts-select.png): filtered by "select": ⌘↑ ⌘↓, ⇧⌘↑ ⇧⌘↓. [w11](screenshots/concept-check/w11-shortcuts-select.png): Ctrl+Shift+↑/↓, Ctrl+Alt+Shift+↑/↓. Test: `keymap::tests` (every bound action listed once). | Matches |

## Deliberate differences

- Key glyphs are in Apple's order (⇧⌘C), where the concept writes ⌘⇧C.
- Adding to the selection on Windows and Linux is Ctrl+Alt+Shift+↑/↓: Ctrl+Shift+↑/↓ already
  moves the selection there.
- The palette shows at most 4 rows per group (12 when only one group matches); typing narrows
  it. The concept shows a handful of rows without saying how many.
- Runtimes also show for Ruby, Java, Deno and Bun projects (by their own marker files), and
  for Python's setup.py and Pipfile; the rule is the same.
- Home and plain folders get a project pill with their name ("Home", "scratch"), like the
  sidebar's groups; the branch and runtime pills stay away.
- Reduced motion on Linux reads GNOME's `enable-animations` once at startup.
- The pills and blocks use Kiln's text size (13 pt) rather than the concept's 12.5 px.

## Settings pop-up (notes/concept/kiln-settings.html)

`cargo run -p kiln --example snapshot -- notes/screenshots/settings settings` (with a config
folder of its own; it never touches yours). The mockup, rendered with WebKit, beside Kiln:
[compare-mockup](screenshots/settings/compare-mockup.png).

| Item | Kiln | Verdict |
| --- | --- | --- |
| Panel: veil, #1d2021, radius 12, ring #3c3836, 470 wide, "Settings" and "esc" | [s01](screenshots/settings/s01-settings.png) | Matches |
| App icon tiles, 60 px, 2 px #e07b39 ring 3 px out | [s01](screenshots/settings/s01-settings.png), [s03](screenshots/settings/s03-icon-black.png) (Black picked; the file gets `icon = "black"`) | Matches |
| Rows, toggles, segmented prompt, steppers ("off" at 0) | [s01](screenshots/settings/s01-settings.png), [s04](screenshots/settings/s04-changed.png) | Matches |
| Keyboard: Tab/↑↓ move (the row lights), ←→ change, Space/Enter, Esc closes | [s02](screenshots/settings/s02-keyboard.png), [s04](screenshots/settings/s04-changed.png); asserted | Matches |
| Writes keep the file: only changed lines change, comments stay | [before](screenshots/settings/s00-kiln-before.toml) → [after](screenshots/settings/s04-kiln-after.toml); tests in `config_write.rs` | Matches |
| A broken file: error shown, editing off, file untouched | [s05](screenshots/settings/s05-broken-file.png); asserted | Matches |
| Windows/Linux: no Option row, Ctrl+Shift+, | [w-settings](screenshots/settings/w-settings.png) | Matches |

Deliberate difference: the footer shows this platform's key only (⌘, or Ctrl+Shift+,), where
the mockup lists both.
