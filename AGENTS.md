# Agents working on Wakecue

## Where things are

- `STATUS.md` — **the only record of where the build stands**: the current
  roadmap move, what has been seen on the board, what is in hand, what is
  still to buy, open questions. Read it first, and read all of it.
- `docs/` — the project pages, as reference. They describe the design and the
  plan and **must not be used to work out what the user has or has done**:
  - `index.html` — the roadmap (moves 1–3 and their gates), overview, how it
    works, what you can schedule, buttons and menu.
  - `hardware.html` — parts list, the printed base, wiring, sound, sources.
  - `software.html` — prayer times and the firmware layout.
  - `build-log.html` — history of what was run on the board, oldest first.
  - `battery.html` — the battery calculator.
  - `style.css`, `site.js` — shared by every page.
- `firmware_ws/` — the Rust workspace (`core`, `settings-tool`, `wakecue`).
- `cad/base.py` — generates the STLs in `cad/`.
- `scratch/` — experiments, not the lamp firmware.

## Rules

- Before saying the user has not done, bought or wired something, check
  `STATUS.md`. If it disagrees with a page in `docs/`, `STATUS.md` wins and
  the page is the bug.
- Do not write progress into `docs/`: no "in stock", "done", "as of <date>",
  "not yet written" claims about the build. The one exception is the build
  log, which records what was run and when.
- When a gate passes or something arrives, add the build-log row and update
  `STATUS.md` in the same commit.
- Plans and gates live in the roadmap section of `docs/index.html`. Number
  work by roadmap move (1, 2, 3); there are no milestones or stages any more.
- The five prayers are handled alike, and a user alarm is built the same way.
  Do not single out Fajr in advice, checks or examples; say "the prayers" or
  use a mix of them.
- A new page in `docs/` links `style.css` and `site.js` and carries the same
  `nav.toc` as the others.
