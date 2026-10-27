# Roadmap

wowlua Studio is wowlua's IDE: a native application drawn with egui on
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

## M2: Workspaces

Artboard: start.

- Open a package or workspace folder (all its members), recent workspaces
  (gone, incompatible), files changed outside Studio, a WoW installation
  found or picked.
- A new addon (`wowlua new`) and adopting one (`wowlua init`).

## M3: Editor

Artboards: editor, search.

- A text buffer and editor view that draw only the visible lines; the
  wowlua grammar's highlighting and the language server's semantic tokens.
- The language server in Studio's process (`wowlua_lsp::embedded`):
  diagnostics, hover, completion, signature help, quick fixes, go to
  definition, references, rename, outline, folding, format on save.
- Problems, output and a terminal; text search across the workspace;
  JSON with schemas (`wowlua.json`, world profiles).

## M4: Tests and coverage

Artboards: tests, coverage.

- The test explorer, code lenses, results, snapshots and screenshot diffs
  (accept one, or all), run on save; coverage per file and in the editor's
  gutter, minimums from `wowlua.json`.

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
- The terminal: a full terminal emulator or a command runner (M3).
- Git through the `git` program or a library (M11).
- Installers and code signing per system (first release).
- The GitHub app Studio signs in with, and who registers it (M12).
- Whether "sign in with Claude" is possible for a third-party app (M14).
