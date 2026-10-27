# Roadmap

wowlua studio is wowlua's IDE: a native application drawn with egui on
`wgpu`, for Windows, macOS and Linux, over wowlua's libraries in its own
process (decisions S001 to S005). It closes wowlua's milestone M15.

The design is the canvas
[wowlua studio](https://claude.ai/artifact/RBjKPKR9gzEDfr6KA4u9CV): one
artboard per screen with its states, the panels and the components, drawn
with the wowlua design system. A milestone builds what its artboards show;
where Studio differs from the design, the design changes first.

Each minor version closes a milestone (S006). The milestones up to M11
make the first release's Studio: packages in folders, with local git
(S009). Milestones are Studio's own; wowlua's are named "wowlua M15".

## M0: Foundation

- The repository: a Cargo workspace, CI on Windows, macOS and Linux (format,
  clippy, tests, a build per OS), this spec, the changelog.
- A window drawn with eframe on `wgpu` that redraws only when something
  changed, with wowlua linked in (the version it was built with).
- Frame statistics (`WOWLUA_STUDIO_FRAME_STATS`): how long each frame's UI
  takes, measured from M0 on (S005).

**Exit:** the window opens on all three systems from CI's builds and shows
the wowlua version built in.

## M1: Shell

Artboards: overview, icons, panels, title bar, rail, status bar, command
palette.

- The design system in egui: its tokens for dark and light, Martian Mono
  and IBM Plex Mono, the icons (16 × 16 strokes), buttons, selects,
  segmented controls, badges, tables, cards, callouts.
- The title bar, the rail and the status bar; on macOS the traffic lights
  in Studio's bar, the system menu bar and `cmd` (S008, S013).
- Docking: left, main, bottom and right docks; panels as tabs, splits, and
  floating in windows of their own; a layout per workspace and a preset per
  page.
- The command palette (files, symbols, commands, lines) and keybindings.

**Exit:** every page of the rail opens its layout preset with placeholder
panels; a panel is dragged to another dock, split and floated; the theme
switches; every frame's UI is built within the budget (S014).

Until M2 opens workspaces, the shell shows with no folder open; M2 adds
the start page the design shows then (no rail).

Built 2026-10-27: the design system's tokens, fonts, icons, the [wl]
mark, buttons, segmented controls, badges, key caps and icon buttons
(selects, tables, cards and callouts come with the first page that uses
them); the title bar, rail and status bar, Studio's window buttons and
resize edges on Windows and Linux, the menu bar on macOS; docking with a
preset per page (S017), splitters, drag and drop onto the panels board's
five drop zones, a panel menu (move, split, float, maximize, close),
floating windows, `ctrl shift m`; the command palette (S018) and the
command registry behind it, the menus and the keys (S019); settings and
layouts saved in the system's folder for application data. The frame
budget is checked in CI's optimized builds: 0.08 ms median and 0.17 ms
at the 99th percentile on an Apple M-series laptop. Not yet checked by
hand: dragging and floating panels, and the window on Windows and Linux.

## M2: Workspaces

Artboards: start, open another folder.

- The start page while a window has no workspace (S024): open a folder,
  a new addon, adopt one, recent workspaces (gone, can't be checked), this
  computer (wowlua, the WoW installation, Blizzard's UI), what's new.
- Open a package or workspace folder with all its members, read through
  wowlua's driver; a package whose `wowlua` requirement excludes this
  wowlua (E4300) opens with checks off. A folder with no `wowlua.json`
  offers to adopt it (S032). The explorer lists the members and their
  files; files changed outside Studio show at once.
- One workspace per window, a process per window (S025, S031).
- A new addon through `wowlua new`'s library with its check mode, Git and
  starter world profile (S026, wowlua T476), and adopting one through
  `wowlua init`'s, on a new branch with the `git` program (S027).
- The WoW installation found as `wowlua` finds it, or picked and kept in
  `~/.wowlua/config.json` (S028); Blizzard's UI per flavor the
  installation has, from wowlua's cache (S029); what's new from wowlua's
  changelog (S030).

Built 2026-10-27: the start page with its three forms (open, new addon,
adopt), the recent workspaces (gone: locate or remove; can't be checked:
a warning), this computer (wowlua, the installation with its client
folders, Blizzard's UI per flavor with a download) and what's new; the
design system's fields, selects, check boxes, cards, callouts and terminal
blocks; the wordmark; opening a folder from the file menu (`ctrl o`), by
dropping it onto the window or as `wowlua-studio <folder>`; the explorer
with the packages, their files and the design's file icons, read again
when files change on disk (a changed `wowlua.json` reads the workspace
again); the title bar's workspace and branch, the status bar's flavor and
client, the window's title; the question when opening another folder, new
windows (`ctrl shift n`) and closing the folder. Checked by hand on macOS
with SuuUI's workspace (it opens with checks off: it requires wowlua
^0.12). Not yet checked by hand: creating and adopting an addon from the
forms, Windows and Linux.

## M3: Editor

Artboards: editor, search, notifications.

- A text buffer and editor view that draw only the visible lines; the
  highlighting of wowlua's lexer and the language server's semantic
  tokens (S045).
- The language server in Studio's process (`wowlua_lsp::embedded`):
  diagnostics, hover, completion, signature help, quick fixes, go to
  definition, references, rename, outline, folding, format on save.
  Semantic tokens, quick fixes and folding are new in wowlua's server
  (S042).
- Problems, output and a terminal emulator (S043); text search across the
  workspace; JSON with schemas (`wowlua.json`, world profiles, S046).
- Opening another folder in a window with unsaved edits asks to save them
  first (the open-another-folder board).
- Files changed outside Studio: open files reload, or the changed-on-disk
  banner with compare, keep mine, take theirs (S049); a file deleted
  outside closes with a notification to restore it (S048).
- Notifications: toasts and the status bar's tray (the notifications
  board, S047).
- Background jobs in the status bar (the jobs board): the check is the
  first; the later milestones' builds, test runs, installs, publishes
  and recordings run as jobs too.
- Inlay hints and a references lens over functions (S050).
- Find and replace in the file: the find bar (S051).

Built 2026-10-27: the editor (tabs and preview tabs, the path, undo and
redo, the clipboard, words and lines, indentation, IME text, the caret's
blink) drawing only the rows on screen, with wowlua's lexer and the
server's semantic tokens, folding from the gutter, inlay hints and
references lenses; the embedded server on its own thread, with
diagnostics (squiggles, the problems panel with its scope, the status
bar's counts), hover, completion, signature help, go to definition,
references and rename in the search panel (a rename's changes checked
first), the outline, quick fixes (`cmd .`), format on save; text search
with replace; the output panel; a terminal emulator; JSON completion and
hover from wowlua's schemas; files changed or deleted outside; the
unsaved-edits question; notifications; background jobs (the check, told
by the server's `wowlua/checking`). wowlua 0.13.2, pinned, adds the
server's semantic tokens, folding, inlay hints, quick fixes, lenses, the
checking notification and a function's overloads one per line. The frame budget holds with a 4000-line file
open: 0.09 ms median, 0.25 ms at the 99th percentile; a line of megabytes
is laid out only where it is seen. The find bar (S051): find and replace
in the file, every match marked. The terminal selects as a terminal app
does (a drag, words, lines) and copies and pastes. Not yet: checked by
hand on macOS only.

## M4: Tests and coverage

Artboards: tests, coverage.

- The test explorer, code lenses, results, snapshots and screenshot diffs
  (accept one, or all), run on save; coverage per file and in the editor's
  gutter, minimums from `wowlua.json`.
- Runs with wowlua's own steps, as jobs; results and coverage kept until
  the next run, across restarts; run on save runs the affected tests
  (S052).
- The tests' lenses, and over a function the tests that call it (S053).
- No App Nap while tests run on macOS (S054).

Built 2026-10-28: the test explorer (the workspace's test files as the
server reads them, filters, failed only, each test's state), runs of all,
the failed, a file or a test, with progress and stop, a world profile
over `test.profile`; the results with a failure's source and stack, a
snapshot's changes, a screenshot's stored, new and different images,
accepting one or all, the output; the coverage panel (totals against the
minimums, the files with where to test next, a file's lines, branches and
functions never called) and bars in the editor's gutter; the lenses of
S053; run on save. wowlua's pull request #64 makes a run's steps a
library (`wowlua_test::runs`, `run_with`'s progress and stop, the
functions each test called). SuuUI's suite runs in Studio as fast as
`wowlua test` does (3,618 tests, 185 files). The frame budget holds with
3,700 results: 0.13 ms median, 0.19 ms at the 99th percentile. Not yet:
checked by hand on macOS only; wowlua's pull request #64 is not released.

## M5: The game

Artboard: game (its UI; the 3D world is M13).

- `wowlua-render` drawing a session's screen into Studio's window, on
  Studio's `wgpu` device; the frame tree, the inspector, input, events,
  screenshots; the installation's art read and cached.

## M6: Debugger

Artboards: debugger, sessions.

- Sessions of the addon or a test file, launch configurations, every kind
  of breakpoint, stepping through dispatch, the stack, variables with taint
  and secrets, watches, the console, the clock, `/reload`, the live world.

## M7: Profiler

Artboard: profiler.

- Recording a session; functions, call tree, flame graph, dispatches and
  game frames; comparing two profiles; speedscope files in and out.
- The in-game recordings of wowlua's dev window, found in the
  installation's SavedVariables (S011; the design adds this source first).

## M8: World profiles

Artboard: world profiles.

- A form for every part of a profile over its schema, kept in step with the
  JSON; checks as you type (E4400 to E4402); the live world; profiles from
  a snapshot (`/stdtest snapshot`) or SavedVariables.

## M9: Ship

Artboard: ship.

- Build profiles, install into `Interface/AddOns` (copied or linked), the
  `wowlua dev` loop, publishing with its dry run and changelog.

## M10: Settings and dependencies

Artboards: settings, wowlua.json.

- Studio's settings and keybindings; `wowlua.json` as forms that keep the
  file's order.
- Dependencies from the registry: search, add, update, outdated (S010;
  the design adds this view first).

## M11: Git

Artboard: git.

- Changes, staging, diff, commit (with the repository's hooks), branches,
  history, pull and push, merge conflicts.

**Exit (wowlua M15):** SuuUI opened from disk in Studio on Windows, macOS
and Linux: edited and checked, its tests run, run against the default UI
in a world profile and debugged, the world changed while it runs, profiled,
installed into the WoW folder, committed.

## M12: GitHub

Artboards: pull request, online workspace.

- Signing in with GitHub; pull requests, reviews and Actions runs.
- A repository opened online with no folder, edits kept as drafts on this
  machine. Needs a host per workspace in wowlua (today `wowlua_host`
  installs one for the whole process) and wowlua's commands run in Studio's
  process for its terminal.

## M13: The 3D world (planned)

Needs new work in wowlua: unit positions in the engine and world profiles,
a world renderer with models from the installation.

## M14: Claude (planned, last)

Building AddOns with Claude inside Studio, through Studio's tools, every
change approved.

## Open questions

Asked when their milestone starts; nothing here is decided yet.

- Where launch configurations are kept (M6).
- Installers and code signing per system (first release).
- The GitHub app Studio signs in with, and who registers it (M12).
- Whether "sign in with Claude" is possible for a third-party app (M14).

## Design changes owed

Where Studio was decided to differ from the canvas, or the canvas is
missing a part; the canvas is updated to match.

- The start page's Blizzard's UI row: per flavor the installation has,
  from wowlua's cache, with a download (S029).
- The start page's installation row lists the installation's client
  folders with their flavors; what's new is titled with the exact
  version (S037).
- The adopt form's folder has a `choose…` button (S039).
- The frame stats in the status bar (S041).
- The new-addon form's preview: `wowlua new`'s real output line
  (`Created addon package in pull-timer`), `--check strict`, and the CI
  workflow at `.github/workflows/ci.yml` (S036).
- A recent workspace that can't be checked opens with `open` until the
  editor (M3) can open its `wowlua.json`.
- The tests board's explorer pinned by a function's lens: "the tests that
  call double" with `show all` over the tree (S053).

Brought up to date on 2026-10-27: the panels board's presets
for every page (S017), the palette's prompt (S018), the menus (S019), the
app icon and the title bar's mark (S020, S023), the status bar without the
theme (S021), the start page without GitHub and Claude (S024), the
question when opening another folder (S025, a new board), the registry's
dependencies (S010, a new board) and the profiler's recordings from the
game (S011).
