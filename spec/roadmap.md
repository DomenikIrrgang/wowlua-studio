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
S053; run on save. wowlua 0.13.3, pinned, makes a run's steps a
library (`wowlua_test::runs`, `run_with`'s progress and stop, the
functions each test called). SuuUI's suite runs in Studio as fast as
`wowlua test` does (3,618 tests, 185 files). The frame budget holds with
3,700 results: 0.13 ms median, 0.19 ms at the 99th percentile. Not yet:
checked by hand on macOS only.

## M5: The game

Artboard: game (its UI; the 3D world is M13).

- `wowlua-render` drawing a session's screen into Studio's window, on
  Studio's `wgpu` device; the frame tree, the inspector, input, events,
  screenshots; the installation's art read and kept in memory.
- The addon's session in Studio's process, the game's settings as in game,
  the units (S055).

Built 2026-10-28: the game page runs the workspace's addon on wowlua's
debugger in Studio's process (wowlua 0.13.4, pinned:
`session::run_hosted`'s frame hook, the inspector, pick, units, target,
the event log, the game's settings at runtime and a world profile's
`screen`). Each changed scene is drawn on eframe's device; the screen
fits or zooms; input in six modes and slash commands; the frame tree with
a filter and hidden frames; the inspector (strata, level, rectangle,
alpha and scale as drawn, anchors, scripts and where they are, events,
regions, children, where the frame was made; go to code, screenshot it);
the units, a click targeting one; the events told with their arguments,
frames and time; the input given; screenshots; the window size and UI
scale on the toolbar. SuuUI runs on it: 0.33 ms median frames with its UI
live. The frame budget holds with 5,000 frames and 1,000 events: 0.07 ms
median. Each frame made two GPU buffers for egui's vertices and indices
(wgpu's `write_buffer`), a wait in the GPU driver on macOS: egui-wgpu is
patched to write them through memory kept between frames (S059,
`patches/README.md`), and wowlua's renderer keeps its own (wowlua's pull
request #98). The UI's slow frames while SuuUI runs (10 to 60 ms, some
longer) took about 1 ms of the thread's time: it was not running, measured
with the screen locked and other programs busy; to check with Studio in
front (`WOWLUA_STUDIO_FRAME_STATS=1`). Checked by hand on macOS only. Before M8 (S058): the frame
tree's "Blizzard's" switch (wowlua marks the frames the client and
Blizzard's UI made).

## M6: Debugger

Artboards: debugger, sessions.

- Sessions of the addon or a test file, launch configurations, every kind
  of breakpoint, stepping through dispatch, the stack, variables with taint
  and secrets, watches, the console, the clock, `/reload`, the live world.
- One session for the game page and the debugger; configurations in
  `.wowlua/launch.json`; breakpoints, watches and history kept (S056).

Built 2026-10-28: sessions of the addon or a test file from launch
configurations (`.wowlua/launch.json`, VS Code's wowlua entries, a form
that writes Studio's), their breakpoints set before they run; breakpoints
in the editor's gutter (placed, not loaded yet, conditional, log points;
a right click edits one) and in the breakpoints panel with their hits,
function, event, script and timer breakpoints, what stops the session;
the session's bar over the editor (continue, pause, the steps, restart,
stop, `/reload`, the clock switched and advanced, the game's time) and
the stop's banner (an error's message and stack, a blocked action's or a
taint's chain, with help); the call stack with dispatch frames and
Blizzard's code; variables by scope, opened as they are needed, taint and
secrets marked; watches; the console (output, expressions in the
selected frame, slash commands, history); the live world (units with
threat, targeting, Lua with `wowlua:mock`, events fired, the world saved
as a world profile); the recent sessions and the last one's summary
(time, frames, stops, log lines, errors); debug lenses over tests,
"debug this test", the inspector's "break on its scripts". wowlua 0.13.5
adds pausing a running session, hit counts, stop details,
the clock switched while running, the world as a profile and the frame
hook at a stop. The frame budget holds with 2,000 console lines and 300
breakpoints: 0.07 ms median. Finished before M8 (S058): the stop's
values after its lines in the editor (`s = 2`); the call stack's switches
while a session runs (dispatch frames shown, Blizzard's code stepped into:
wowlua's `setDebugBlizzardUi`); watches and hovers that only read
(wowlua's T496); the start's checklist and the breakpoints placed; a
breakpoint turned off without removing it, every breakpoint off at once.
Editing a unit in the live world moved to M8. Checked by hand on macOS;
Windows and Linux are on the checklist below.

## M7: Profiler

Artboard: profiler.

- Recording a session; functions, call tree, flame graph, dispatches and
  game frames; comparing two profiles; speedscope files in and out.
- The in-game recordings of wowlua's dev window, found in the
  installation's SavedVariables (S011; the design adds this source first).

Built 2026-10-28: recording the running session from the profiler's bar
(wowlua's profiler on the session, its report's speedscope file kept in
`.wowlua/profiles` named by the branch and the time); a test file or one
test profiled from its lens, the tests page or a failure (each test file's
profile kept, the first one shown); a speedscope file opened, another
tool's read as the tree of its samples; the game's exported recordings
from the installation's SavedVariables (each client folder and account,
their locations in the source). The timeline: the newest 600 game frames
(a game recording's seconds against its frame budget), a frame picked
scopes the call tree and the flame graph to that frame. Views: functions by
instructions, bytes, host time or calls with their share and, compared
with another profile, the change; the call tree (the hottest path open);
the flame graph (click to zoom); dispatches with their worst frame; a game
recording's slow calls and hints. The function panel: its sums and per
call, callers and callees, go to code, break on it (wowlua's breakpoint by
definition, T495). wowlua's pull request #96 adds per-frame trees, the
speedscope file that keeps the whole profile, the game's recordings in a
library and breakpoints by definition (S057). The frame budget holds with
3,000 functions, 20,000 nodes and 600 frame trees: 0.13 ms median.
Finished before M8 (S058): functions named by their sources' outlines
(`ThreatModule:Update`, the place beside it); a game recording's events,
scripts, timers and slash commands with their frames over the budget, and
its memory under the seconds; the hints' explanations (wowlua's, opened
under each hint). Checked by hand on macOS; Windows and Linux are on the
checklist below.

## M8: World profiles

Artboard: world profiles.

- A form for every part of a profile over its schema, kept in step with the
  JSON; checks as you type (E4400 to E4402); the live world; profiles from
  a snapshot (`/stdtest snapshot`) or SavedVariables.
- Editing a unit in the live world (moved from M6, S058): the profile
  editor's units, applied to the running session.

Built (S060): the profiles and their 22 parts (wowlua's schema); a form for
each over the file's JSON, the same document as the json view and an editor
tab, a form's edit changing only its value's text; the designed forms
(character, group and units, auras, spells, action bars, equipment, bags,
SavedVariables) and one built from the schema for the others; the checks as
you type (E4400, E4401 with a fix for a misspelt part, E4402 from the
language server); the installation's art (class icons from the class sheet,
spell and item icons) and spells; the spells part's class spells to check as
trained (wowlua's `trained`, T500: the client's tables, learned from a
trainer or by level); the live world (each edit a `mock` request to a
session started in the profile: wowlua's `unit:update`, T497; what was
applied), a snapshot from the game (wowlua's `/wl snapshot`, T498) and
SavedVariables imported; where a profile is used, its tests run, a session
started in it. With wowlua's fixes found on the way: camelot's client in
`_forever_` (T499) and a profile's nameplates after the login. The frame
budget holds with 40 units and 120 class spells: 0.43 ms median. Not yet:
completion and hover in the json view (typing, selection and undo work);
the spellbook's frames from `trained` (wowlua answers them from a recorded
spellbook); checked by hand on macOS only.

## M9: Ship

Artboard: ship.

- Build profiles, install into `Interface/AddOns` (copied or linked), the
  `wowlua dev` loop, publishing with its dry run and changelog.

Built (S061): the page works on the workspace's root package (a workspace
without one picks an addon), in Studio's process through wowlua's library.
The build profiles, built in and the manifest's, with what each inherits:
the designed settings and a form from wowlua's schema for the others, an
edit rewriting only that value in `wowlua.json`, inherited values shown as
such. A build with its progress (wowlua's `build::Step`, T503), its output
folder, the sizes of each profile's output and the compiler's output, the
first errors linked to their source. The installation, changed with a
folder picked and kept as `wow.path`; where the addon is installed in each
client and what something else changed there since (wowlua's install
record, T502), confirmed before an install replaces it. The dev loop over
the disk's changes, copied or linked, with its log. Publishing: the checks
(a clean tree, a new version, the changelog's section, the tests, the tag),
the targets with their tokens read when the tab opens and saved or removed
in the OS credential store (`wowlua login`'s, T501), a dry run, a publish
confirmed against a fresh dry run, the version tagged and pushed after a
confirm showing the commands. The frame budget holds with SuuUI's size:
0.09 ms median, 0.17 ms p99. Not yet: checked by hand on macOS only.

## M10: Settings and dependencies

Artboards: settings, wowlua.json (an editor tab, S063).

- Studio's settings and keybindings; `wowlua.json` as forms that keep the
  file's order.
- Dependencies from the registry: search, add, update, outdated (S010;
  the design adds this view first).

Built (S062): Studio's settings on this computer by section, found by name,
every one the canvas has and the features behind them: the theme (the OS's
too), the interface's size, density, reduced motion; the editor's font
size, format on save, inlay hints, lenses, coverage in the gutter, word
wrap (at the window or the package's line width); completion (while
typing, the keys that accept, auto-import from wowlua's completion, T506,
documentation beside the list, signature help); the language server's
check timing, threads and profile (wowlua T504) and its check cache kept on
disk and cleared (T505); the debugger's stops, dispatch frames, inline
values, secrets and default clock, one with the debugger page's; the game
screen's size, the installation's art (kept on disk) and data; the
installation and Blizzard's UI refreshed (T505); layouts per workspace,
floating panels as OS windows; Studio's log and releases. Keybindings
recorded per command, the editor's too, with clashes. GitHub's device
sign-in and the Claude API key in the OS credential store (their other
settings kept for M12 and M14). A `wowlua.json` opens in its editor tab as
its form (S063), its sections listed inside the tab: the designed rows,
every other key from the schema, the json view the tab's code, a
requirement that excludes the built-in wowlua fixed. Its dependencies
section: newer versions when it opens, the registry searched, added,
updated, removed, a new requirement, yanked versions. The settings page is
Studio's own settings only. Flavors come from wowlua's flavor table
everywhere (S064); build profiles are edited in wowlua.json's build
section, the coverage minimums and the test screen as fields. Buttons lead
with their verb's icon (Option D) and settings are picked, not typed from
blank: sliders, steppers with presets, tags, chips and pickers (S065):
world profiles' CVars found among the client's with their defaults
(wowlua's `cvars`), key bindings recorded and their actions picked from
Blizzard's UI, spell and item IDs found by name or ID in the installation
(items in its `ItemSparse`). World profiles' classes, races and powers
come from wowlua's flavor table (S066, wowlua T512 and T513): every power
of a class on its own slider, the faction following the race, only the
classes the race can be, its racial spells shown. A publish target's
build profile is picked on the ship page and written to `wowlua.json`
(S067). The frame budget holds: 0.05 ms median,
0.12 ms p99. GitHub's sign-in uses
the "wowlua studio" OAuth App (its client ID built in; no secret). Not yet:
checked by hand on macOS only.

## M11: Git

Artboard: git.

- Changes, staging, diff, commit (with the repository's hooks), branches,
  history, pull and push, merge conflicts.

Built (S068, S069, S070): the git page with its changes (the branch, the
message, commit and commit and push, format and check first, conflicts,
staged and changed files with their letters, staged by file), the
history (a commit's message, files and diffs; switch to it, a branch from
it, revert it), a file's diff hunk by hunk side by side or inline in the
editor's colours (stage, unstage, discard each hunk), switching with the
changes carried or stashed, new branches, pull as git config says, push
and fetch with the GitHub sign-in first, a merge's conflicts taken from a
side or aborted. Outside it: the branch in the status bar with every
branch a click away, the explorer's colours, the gutter's marks against
the last commit with their hunk, the cursor line's blame, a conflict's
lenses in the editor, Studio's own fetch every 5 minutes. Not yet:
checked by hand on macOS only.

**Exit (wowlua M15):** SuuUI opened from disk in Studio on Windows, macOS
and Linux: edited and checked, its tests run, run against the default UI
in a world profile and debugged, the world changed while it runs, profiled,
installed into the WoW folder, committed.

## M12: GitHub

Artboards: pull request, online workspace.

- Signing in with GitHub; pull requests, reviews and Actions runs.
- A repository from GitHub opened as a folder would: cloned into Studio's
  data, deleted when it closes (S075).

Built, the first half (S072, S073, S074): the pull requests page with the
open pull requests and the workflow runs; a pull request's conversation,
its files with line comments and threads, reviews with pending comments,
merging, and new pull requests from a pushed branch (also from the git
page after a push); a run's jobs, steps, logs and annotations, re-run and
cancel; your runs in progress as status bar jobs, told when they end;
GitHub read on open, with git's fetch and every 15 s while a run goes.

Built, the second half (S075): "from GitHub" on the start page (a typed
repository or link, your repositories, the branch), the palette's and file
menu's "open from GitHub…", recents that clone again; the clone opened as
a folder, marked in the status bar and the explorer; what closing would
lose asked first, then the clone deleted; clones left by a window that quit
cleaned up or offered; a read-only repository's fork. Not yet: checked by
hand on macOS only.

## M13: The 3D world (planned)

Needs new work in wowlua: unit positions in the engine and world profiles,
a world renderer with models from the installation.

## M14: Claude (planned, last)

Building AddOns with Claude inside Studio, through Studio's tools, every
change approved.

## Checked by hand on Windows and Linux

Studio is checked by hand on macOS as each milestone is built; CI builds
and tests it on Windows and Linux (S058). On each, with SuuUI's workspace:

1. **M1**: the window's own title bar (drag, double click, the buttons),
   its resize edges, dragging a panel to each dock and floating it.
2. **M2**: open a folder from the file menu and by dropping it; make a new
   addon and adopt one from the forms; the installation found.
3. **M3**: type, undo, copy and paste in the editor; hover, completion, go
   to definition; the terminal runs your shell (`cmd`, `bash`).
4. **M4**: run all tests, a failure's source, accept a screenshot.
5. **M5**: run the addon; click and type in the game's screen; change the
   window size in the game's settings.
6. **M6**: a breakpoint stops; step, watch, the console; turn a
   breakpoint off; `/reload`.
7. **M7**: record the session and stop; pick a spike; profile a test from
   its lens; open a speedscope file.
8. **M8**: open a profile, edit a unit in its form and see the json view
   follow; the class and spell icons from the installation; start a session
   in the profile and change a unit's distance live; import a snapshot.
9. **M9**: build release and see its progress; change the installation;
   install release, edit a file in the installed folder and see the
   warning; the dev loop with a save (copied and linked); save a publish
   token and see it in `wowlua login`'s store; a dry run.
10. **M10**: the system theme following the OS; the interface size; record
    a key and use it; word wrap at the window; save the Claude key; open
    `wowlua.json`, edit a key in its form and see the json view; search the
    registry and add a dependency in its dependencies section; pick a flavor
    from the table; open a build profile from the ship page in
    `wowlua.json`'s build section; in a world profile pick a race and see
    the faction and classes follow, a druid's powers, a CVar, a binding by
    pressing its key, a spell and an item by name; pick a target's profile
    and see `wowlua.json` follow.

11. **M11**: stage a hunk and commit with format and check; switch branch
    with changes and stash them, then switch back; pull, push with the
    GitHub sign-in and with your git setup signed out; resolve a merge
    conflict in the editor with its lenses; the status bar's branch list;
    the gutter's marks and blame as you type.
12. **M12**: open a pull request, comment on a line and send it with a
    review; reply to a thread and resolve it; merge one; push a new branch
    and create its pull request from the git page; open a failed run's
    step log and annotations, re-run its failed jobs and watch it in the
    status bar until the notification; open a repository from GitHub by
    typing it and from your list, change a file and close: the dialog lists
    it; commit and push, close: it goes without asking; quit with a change
    and see the start page offer the clone; open a repository you can't
    push to and fork it.

## Open questions

Asked when their milestone starts; nothing here is decided yet.

- Installers and code signing per system (first release).
- Whether "sign in with Claude" is possible for a third-party app (M14).

## Design changes owed

Where Studio was decided to differ from the canvas, or the canvas is
missing a part; the canvas is updated to match. On 2026-10-30 the settings board got the git section (fetch every) and the editor's blame row, the git board the changes' stage all and the staged files' unstage all (S070). Owed (S075): the closing dialog's last button is "discard them and close" (a removing verb, drawn as danger) where the board says "close and lose them"; a clone's progress shows on the start page (the workspace opens once it is cloned), where the board's "cloning" state draws it in the workspace's explorer.

Brought up to date on 2026-10-27: the panels board's presets
for every page (S017), the palette's prompt (S018), the menus (S019), the
app icon and the title bar's mark (S020, S023), the status bar without the
theme (S021), the start page without GitHub and Claude (S024), the
question when opening another folder (S025, a new board), the registry's
dependencies (S010, a new board) and the profiler's recordings from the
game (S011). On 2026-10-28: the tests board's explorer pinned by a
function's lens, "the tests that call stateOf" with `show all` (S053). The game board's
toolbar with the game's settings (window size, UI scale), the loading
note reading the art once a Studio start, the frame tree's refresh
(S055). On 2026-10-28 for M7: the start
page's CI workflow path (S036), the debugger board's bar on two rows, the
configuration as a dialog over the page and the watch panel's note (S056);
the profile lenses and the tests page's "profile the selected test", the
profiles kept in `.wowlua/profiles`, a game recording's timeline per second
(S057) Before M8: the watch panel's note (watches only read) and a game
recording's events, scripts, timers and commands, and its memory (S058).
