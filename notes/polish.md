# Polish pass (2026-10-19)

Fixes asked for by the overseeing session after phase 3, checked with headless renders
(before: the phase 1 and 2 shots; after: the same scenarios now).

| Fix | Before | After |
| --- | --- | --- |
| Short names in the sidebar: project = repo or folder name, session = subfolder, else running program, else project name; `~` for home; repeats numbered | [09](screenshots/polish/before/09-sessions.png), [17](screenshots/polish/before/17-attention.png) | [09](screenshots/polish/after/09-sessions.png), [17](screenshots/polish/after/17-attention.png) |
| Palette session rows: the same labels, the full folder as dim detail | [18](screenshots/polish/before/18-palette-projects.png) | [18](screenshots/polish/after/18-palette-projects.png) |
| Header: dim breadcrumb on the left (`acme-web › db.prod.example.com`), branch on the right | [15](screenshots/polish/before/15-production-tint.png) | [15](screenshots/polish/after/15-production-tint.png) |
| No `cd '…'` blocks: the scenarios start like a restart does, the shell already in its folder | [01](screenshots/polish/before/01-prompt.png), [13](screenshots/polish/before/13-rail.png) | [01](screenshots/polish/after/01-prompt.png), [13](screenshots/polish/after/13-rail.png) |
| Command headers in splits keep their full width (`git --no-pager log --oneline`, `node src/server.js`); hover actions cover the end of the header instead of reserving room | [17](screenshots/polish/before/17-attention.png), [03](screenshots/polish/before/03-hover-actions.png) | [17](screenshots/polish/after/17-attention.png), [03](screenshots/polish/after/03-hover-actions.png) |
| Visual rules: no borders anywhere; status dots #8f9a6c / #b8645a; orange only for the cursor, caret, attention, the update offer and "open" on a port. The pinned-block dot was orange; it is now dim. | | |

Notes:

- Restored sessions never typed anything: they start through the PTY's working directory. The
  `cd` blocks came from the screenshot scenario itself, which typed `cd`.
- The orange dot on the first session in some shots is real behaviour: the headless window
  never counts as "in front", so long commands that finish there mark their session.
- Kiln has no rename yet, so the "a user rename wins" rule has nothing to apply to.
- Not run in CI: the account's Actions minutes are used up (QUESTIONS, item 0). Locally: 60
  tests, clippy with warnings as errors, fmt.
