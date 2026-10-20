# Concept check

Each item of `notes/concept/kiln-concept.html` against Kiln as built, rendered headless with
the real renderer and real shells:

```bash
cargo run -p kiln --example snapshot -- notes/screenshots/concept-check concept
```

The `w*` shots are the Windows chrome preview (`KILN_CHROME=windows`) with the Windows and
Linux key bindings. Screenshots are in `notes/screenshots/concept-check/`. The demo project is
a scratch git repository with four changes (`acme-web`); `notes-site` is a clean one and
`scratch` is a folder outside git.

| # | Item | Concept state | Kiln | Verdict |
| --- | --- | --- | --- | --- |
| 1 | Pills in the input box | ❯ line on top; under it the project pill (#e07b39, dark text), the branch pill (#3c3836) with "✱2" in orange, the runtime pill (#282828, muted). Nothing above the box. | [01](screenshots/concept-check/01-pills-changes.png): `acme-web`, ` main ✱4`, `node 25`. [01b](screenshots/concept-check/01b-pills-clean.png) clean repo: no count. [01c](screenshots/concept-check/01c-pills-no-repo.png) outside git: no branch pill. [w01](screenshots/concept-check/w01-pills-header.png) Windows. | Matches |
| 1 | WSL distribution pill | On Windows, the distribution ("Ubuntu") as a runtime-style pill. | Covered by `pills::tests::outside_a_repository_there_is_no_branch` ("WSL · Ubuntu" → "Ubuntu"); a WSL session cannot be shown on macOS. | Matches (unit test only) |
| 2 | Header right | The branch and "✱N", the count in #c4a35e. | [01](screenshots/concept-check/01-pills-changes.png): ` main ✱4` top right, count in #c4a35e; [01b](screenshots/concept-check/01b-pills-clean.png): ` main`. With Windows controls: [w01](screenshots/concept-check/w01-pills-header.png). | Matches |
| 3 | Focus mode (⌘.) | Timeline and input box about 72% wide, centred, at most ~100 columns; header right shows what is running. | [03](screenshots/concept-check/03-focus-mode.png): 72% of the window, centred, "● 1 running" (a `sleep` in another session). [w03](screenshots/concept-check/w03-focus-mode.png) Windows. | Matches |
| 4 | ⌘↑ / ⌘↓ | A selection moves between commands, hover background #2f2d2c, actions shown, scrolled into view; ⌘C or Enter copies, Esc clears. | [04](screenshots/concept-check/04-select-command.png): two ⌘↑ select `cd ../acme-web` (raised background, Copy · Rerun · Pin · Open log). [w04](screenshots/concept-check/w04-select.png) with Ctrl+Shift+↑. Logic: `terminal_view::tests::stepping_through_commands`. | Matches |
| 5 | ⇧-click, ⇧⌘↑/↓ | Extends the selection; copy gives one paste-ready block, in order. | [05](screenshots/concept-check/05-extend-selection.png): two commands selected; the copy is [05-multi-copy.md](screenshots/concept-check/05-multi-copy.md) (one fence, oldest first). Tests: `extending_keeps_the_anchor`, `copy::tests::several_commands_share_one_block_in_order`. | Matches |
| 6 | Copy note | Above the input box for ~2.5 s: "⧉ Copied, ready to paste" (or "Copied raw output") and the first lines dim; panel #1d2021, ring #504945, radius 10, 60% wide. | [06](screenshots/concept-check/06-copy-note.png): the note with the first 8 lines; [06b](screenshots/concept-check/06b-note-gone.png): gone after 2.5 s, selection cleared. | Matches |
| 7 | Pinned commands | At the top of the timeline: header and a few lines, quiet grey dot, "pinned · time"; click to expand; unpin from the hover actions. | [07](screenshots/concept-check/07-pinned.png): `git status --short` pinned, then `clear` and two commands: it stays on top, 3 lines, "pinned · 19:40". [07b](screenshots/concept-check/07b-pinned-hover.png): Copy · Rerun · Unpin · Open log. [07c](screenshots/concept-check/07c-pinned-open.png): unfolded. Test: `pinned_blocks_stay_through_clear_and_new_commands`. | Matches |
| 8 | Find bar | Across the top of the timeline, #32302f, radius 8: "⌕ query" left, "2 of 5 · ⏎ next · ⇧⏎ previous · esc" dim right. | [08](screenshots/concept-check/08-find.png): "⌕ auth", "1 of 1 · ⏎ next · ⇧⏎ previous · esc", edges in line with the blocks; the match in the pinned block unfolds it. [w08](screenshots/concept-check/w08-find.png) Windows. Test: `the_find_bar_says_where_you_are`. | Matches |
| 9 | Palette (⌘K) | Groups Commands, Projects, Sessions, Actions under quiet headings; dim detail right ("rerun · project", "open", shortcut); no type tags; `❯` before the query; box 58%, #1d2021, radius 12, ring #3c3836. | [09](screenshots/concept-check/09-palette.png): all four groups, project dots, "rerun · acme-web" on the selected command. [09b](screenshots/concept-check/09b-palette-query.png): "git" leaves Commands. [w09](screenshots/concept-check/w09-palette.png): Windows keys. Tests: `results_sit_under_their_group_in_order`, `groups_are_capped_unless_alone`. | Matches |
| 10 | Sidebar hint | A dim "⌘K" (Ctrl+Shift+K) at the bottom of the sidebar; a click opens the palette. | Bottom left in every shot with the sidebar, e.g. [01](screenshots/concept-check/01-pills-changes.png); "Ctrl+Shift+K" in [w01](screenshots/concept-check/w01-pills-header.png). [10](screenshots/concept-check/10-sidebar-hint.png): the palette opened by a click on it (the run checks `palette_open()`). | Matches |
| 11 | Shortcuts pop-up (⌘/) | Lists the new bindings under "Commands and copy": "Select previous / next command ⌘↑ ⌘↓", and adding to the selection. | [11](screenshots/concept-check/11-shortcuts-select.png): filtered by "select": ⌘↑ ⌘↓, ⇧⌘↑ ⇧⌘↓. [w11](screenshots/concept-check/w11-shortcuts-select.png): Ctrl+Shift+↑/↓, Ctrl+Alt+Shift+↑/↓. Test: `keymap::tests` (every bound action listed once). | Matches |

## Deliberate differences

- Key glyphs are in Apple's order (⇧⌘C), where the concept writes ⌘⇧C.
- Adding to the selection on Windows and Linux is Ctrl+Alt+Shift+↑/↓: Ctrl+Shift+↑/↓ already
  moves the selection there.
- The palette shows at most 4 rows per group (12 when only one group matches); typing narrows
  it. The concept shows a handful of rows without saying how many.
- The pills and blocks use Kiln's text size (13 pt) rather than the concept's 12.5 px.
