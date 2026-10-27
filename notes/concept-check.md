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
| App icon tiles, 60 px, 2 px #e07b39 ring 3 px out | [s01](screenshots/settings/s01-settings.png), [s03](screenshots/settings/s03-icon-flame.png) (→ picks Flame; the file gets `icon = "flame"`) | Matches |
| Rows, toggles, segmented prompt, steppers ("off" at 0) | [s01](screenshots/settings/s01-settings.png), [s04](screenshots/settings/s04-changed.png) | Matches |
| Keyboard: Tab/↑↓ move (the row lights), ←→ change, Space/Enter, Esc closes | [s02](screenshots/settings/s02-keyboard.png), [s04](screenshots/settings/s04-changed.png); asserted | Matches |
| Writes keep the file: only changed lines change, comments stay | [before](screenshots/settings/s00-kiln-before.toml) → [after](screenshots/settings/s04-kiln-after.toml); tests in `config_write.rs` | Matches |
| A broken file: error shown, editing off, file untouched | [s05](screenshots/settings/s05-broken-file.png); asserted | Matches |
| Windows/Linux: no Option row, Ctrl+Shift+, | [w-settings](screenshots/settings/w-settings.png) | Matches |

Deliberate difference: the footer shows this platform's key only (⌘, or Ctrl+Shift+,), where
the mockup lists both.

## Themes (notes/concept/kiln-themes.html)

Same run. The mockup's app-icon tiles are simplified; kiln-settings.html is the reference for
those. Comparisons: [Custom](screenshots/settings/compare-themes.png),
[Kiln Light](screenshots/settings/compare-themes-light.png).

| Item | Kiln | Verdict |
| --- | --- | --- |
| Eight 96×60 cards, four per row (two full rows: Follow system, Kiln Dark, Kiln Light, Catppuccin Mocha / Everforest, Nord, Kanagawa, Custom), each drawn from its own palette; Follow system split diagonally; the chosen one ringed | [t0](screenshots/settings/t0-system.png) to [t6](screenshots/settings/t6-kanagawa.png) | Matches |
| Everforest and Kanagawa replace Tokyo Night, checked against upstream (two Kanagawa values corrected, see DECISIONS) | [t4 Everforest](screenshots/settings/t4-everforest.png), [t6 Kanagawa](screenshots/settings/t6-kanagawa.png) applied; `theme::tests` (tokyo-night now falls back) | Matches |
| A card applies its theme to everything at once, the pop-up and veil included (light veil on light themes) | [t2 Kiln Light](screenshots/settings/t2-kiln-light.png), [t3](screenshots/settings/t3-catppuccin-mocha.png), [t4](screenshots/settings/t4-everforest.png) | Matches |
| Custom: Start from, 13 colours with swatch and hex, 16 terminal colours, Reset to base, "Saved to kiln.toml as you edit" | [c1](screenshots/settings/c1-custom.png); the file then holds name, base and every colour ([toml](screenshots/settings/c1-kiln-custom.toml)), the user's comments intact | Matches |
| Colour picker (hue strip and saturation/value square) | [c2](screenshots/settings/c2-custom-picker.png) | Matches |
| Custom is one more card: a preset after custom colours is exactly the preset, and Custom again brings the colours back (replaces the "N colours overridden · clear" note) | [c3](screenshots/settings/c3-preset-after-custom.png) Nord with a pink custom accent kept in `[theme.custom]` ([toml](screenshots/settings/c3-kiln-nord.toml)); [c4](screenshots/settings/c4-custom-again.png) Custom again, pink accent back. Tests: `settings::tests::presets_are_exact_and_custom_comes_back`, `colours_from_before_the_custom_table_move_into_it` | Matches |
| Palettes are the mockup's values | `theme::tests`, `config::tests::themes_resolve_by_name_with_overrides_on_top` | Matches |
| Prompt: Kiln flame / Shell's own switches every session, open and new; a waiting empty prompt switches at once | [ps1](screenshots/settings/ps1-kiln.png) flame; [ps2](screenshots/settings/ps2-open-session-shell.png) the open session right after choosing "Shell's own", no Enter: only the line typed on shows (`❯`), not the Starship pills above it; [ps3](screenshots/settings/ps3-new-session-shell.png) a new one; [ps4](screenshots/settings/ps4-new-session-kiln.png), [ps5](screenshots/settings/ps5-open-session-kiln.png) back to the flame in both, at once; [ps6](screenshots/settings/ps6-typed-waits.png) with "echo kept" typed, left alone; [ps7](screenshots/settings/ps7-after-enter-shell.png) its next prompt follows. All asserted. Tests: `kiln_term/tests/prompt_switch.rs` (switching and the empty-prompt check; zsh, bash, PowerShell in CI) | Matches |


## App icon row (notes/concept/kiln-icons.html)

Same run (`snapshot settings`). The mockup's grid of every design in every colour, against the
generated masters: the same shapes and colours (checked by eye, rendered side by side).

| Item | Kiln | Verdict |
| --- | --- | --- |
| Five tiles in one row: Kiln, Flame, Beehive, Line, Custom ("+" dashed until there is an image); the chosen one ringed, with a colour-dot badge and "▾" | [i1](screenshots/settings/i1-icon-row.png) | Matches |
| Clicking the chosen tile opens the colour pop-up under it: seven round swatches in equal 58 px columns, names below, the current one ringed; no note under it (dropped at the user's request) | [i2](screenshots/settings/i2-colour-popup.png); Esc closes the pop-up first (asserted); ←→ change the colour while it is open | Matches |
| Each design keeps its own colour; the tiles show theirs | [i3 Kiln in sage](screenshots/settings/i3-kiln-sage.png), [i4 Beehive in slate](screenshots/settings/i4-beehive-slate.png) (pop-up kept inside the panel), [i5 Line in ember](screenshots/settings/i5-line-ember.png); the file then has `[icon_colours]` ([toml](screenshots/settings/i5-kiln.toml)); back to Kiln gives sage again (asserted). Tests: `settings::tests::each_design_keeps_its_own_colour`, `config::tests::the_icon_keys_read_each_design_and_colour` | Matches |
| Old `icon = "black"` loads as Kiln in black and is rewritten on the first write | `config::tests::icons_from_before_the_designs_are_the_kiln_design`, `settings::tests::icons_from_before_the_designs_are_rewritten_on_the_first_write` | Matches |
| Custom: a 900×600 picture, cropped square from the middle, rounded, copied into the config folder; "Using sunset.png · 512×512 · Choose another… · Remove" | [i6](screenshots/settings/i6-custom.png) ([toml](screenshots/settings/i6-kiln.toml)); Remove goes back to Kiln in sage and deletes the copy (asserted). Tests: `custom_icon::tests` (crop, scale, JPEG, too large, over 4096 px, not an image) | Matches |
| A file that is not PNG or JPEG is refused inline, nothing changes | [i7](screenshots/settings/i7-custom-refused.png) (a shell script named `.png`) | Matches |
| Masters from the generator; Kiln terracotta is still the bundle icon | `scripts/make-icon-masters.py --check` (CI lint), `app_icon::tests::the_bundle_icon_is_the_default_master`; the old Kiln terracotta, black and white masters reproduced byte for byte | Matches |
| Runtime switching | macOS: every Dock PNG and a custom one decode as `NSImage` (`app_icon::tests::dock_icons_decode_as_appkit_images`); Windows and Linux X11: CI smoke with `icon = "beehive"` and `beehive = "slate"` reads the window's icon back | Matches |

Deliberate difference: the custom note shows the copy's size (at most 512 px), not the
original's, so it reads the same after a restart.

Not shown in pictures: Follow system switching with the OS (unit-tested in
`theme::tests::system_follows_the_appearance_and_custom_its_base`; the live switch uses GPUI's
appearance observer, not run headless).

## Sidebar: renaming and project colours (no mockup)

`snapshot sidebar`: two sessions, `kiln` and a demo project whose config still has an old
`color = "#83a598"`. Everything below is asserted in the run.

| Item | Kiln | Verdict |
| --- | --- | --- |
| No name at the top left of the panes (the pill shows the project); header height and drag area unchanged; "⌘/ Shortcuts" at the bottom of the sidebar | [sb1](screenshots/sidebar/sb1-sidebar.png); the old "#rrggbb" colour on the demo project's dot | Matches |
| ⌘R / F2: the session's name edited in place; Enter keeps it, Esc drops it, empty restores the automatic name; it stays through `cd` and is saved in `state.json` | [sb2](screenshots/sidebar/sb2-rename-session.png), [sb3](screenshots/sidebar/sb3-session-renamed.png). Tests: `state::tests::session_names_are_saved_with_the_sessions` | Matches |
| ⌘⇧R: the project's name, written to `[projects."…"] name`, shown in the sidebar, the header pill and the session label | [sb4](screenshots/sidebar/sb4-project-renamed.png) ([toml](screenshots/sidebar/sb-kiln.toml)). Tests: `state::tests::projects_can_be_renamed_in_the_config` | Matches |
| "Project colour" from the palette, all by keyboard (it opens on the project's old "#rrggbb" in the custom field; ↑ → → is Blue); the project then sits on a blue-tinted card and the pill is blue | [sb5](screenshots/sidebar/sb5-colour-popup.png), [sb6](screenshots/sidebar/sb6-project-blue.png); `color = "blue"` | Matches |
| Tinted cards replace the dot: header and sessions on one rounded card, the colour at ~14% (more on light), the name bold in the colour mixed with the text, the active row a little darker; Auto is no card | Kiln Dark: [sc2](screenshots/sidebar/sc2-dark-cards.png) (orange, a custom "#2f8f6e", Home on Auto). Kiln Light: [sc4](screenshots/sidebar/sc4-light-cards.png) | Matches |
| The colour pop-up: "Colour · project" and esc, Auto with a dashed ring, sixteen fixed colours in two rows of eight equal columns with the name below, Custom "#rrggbb" with a live preview | The current colour has the accent ring, the keyboard a thin dim one; the name below is the hovered, focused or current one. [sc1](screenshots/sidebar/sc1-dark-typed-hex.png): Blue current, magenta's own hex typed, which points at Magenta and is saved as `magenta`; [sc3](screenshots/sidebar/sc3-dark-popup.png): opened again, Magenta current and named, Custom empty; [sc5](screenshots/sidebar/sc5-light-popup.png): Kiln Light, a custom "#2f8f6e" current, Indigo focused ([toml](screenshots/sidebar/sc-kiln.toml)). Tests: `state::tests::project_colour_names_map_onto_the_fixed_set` (every name, any case, the first set's names, hex), `project_colours_are_stable_and_configurable` | Matches |
| The context rail starts at the top: its first title in the header row, no gap on the right on macOS | Before: [rb1](screenshots/sidebar/rb1-rail-before.png) (from phase 2, 19 Oct: below an empty strip). After: [rb2](screenshots/sidebar/rb2-rail-after.png). Windows and Linux keep the strip for the window controls (not shown) | Matches |
| Right-click a project: Rename, Colour…; ↓ Enter opens the colours, Esc closes | [sb7](screenshots/sidebar/sb7-context-menu.png) | Matches |
| Double-click a session: rename in place | [sb8](screenshots/sidebar/sb8-double-click.png) | Matches |
| The shortcuts hint opens the pop-up, which lists Rename session (⌘R F2) and Rename project (⇧⌘R); every bound action is listed | [sb9](screenshots/sidebar/sb9-shortcuts.png). Test: `shortcuts::tests::the_popup_shows_every_bound_action`. Command palette in one column: [s7](screenshots/shortcuts/s7-narrow-scrolled.png) | Matches |

