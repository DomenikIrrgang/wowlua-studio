# Decisions

Why Studio is the way it is. **(user)** marks a decision the user made,
**(proposal)** one waiting for the user's word.

| # | Decision | Why |
|---|----------|-----|
| S001 | Studio is a native Rust application: no HTML, no JavaScript, no webview **(user, M0)** | The user's requirement; wowlua's decision T300 already planned "a native Rust IDE" that links the libraries in its own process. wowlua's roadmap M15 described a browser front-end with a desktop shell; it changes to this. |
| S002 | Desktop first, on Windows, macOS and Linux; a browser build stays possible later: everything is drawn on `wgpu`, and dependencies must build for `wasm32-unknown-unknown` **(user, M0)** | WoW's players and addon authors use all three; wowlua's libraries below Studio already build for the browser (T301). |
| S003 | The UI is egui (eframe) on `wgpu`; Studio's heavy views (the editor, trees, tables, the flame graph) are its own widgets painted with egui **(user, M0)** | Proven for large native tools that also run on the web; egui-wgpu 0.37 uses `wgpu` 30, as `wowlua-render` does, so the game's screen and Studio share one device. |
| S004 | wowlua is a git dependency pinned to a release tag; working on both repositories at once, a path patch in `.cargo/config.toml` (kept out of git) points to a local checkout **(user, M0)** | CI and releases build what the tag says; local work changes both without publishing. |
| S005 | Performance comes first: Studio redraws only when something changed, draws only what is visible, and keeps checks, tests and the game off the UI thread; frame times are measured from M0 on **(user, M0)** | It has to feel snappy. |
| S006 | Studio has its own semantic version; a release names the wowlua version built in. Each minor version closes a milestone of the roadmap; releases within a milestone bump PATCH **(user, M0)** | Studio and wowlua release on their own schedules. |
| S007 | Studio's spec (this roadmap and these decisions) lives in this repository; wowlua's roadmap M15 points to it **(user, M0)** | Studio's plans change with its design, not with the compiler's. |
| S008 | On macOS Studio follows the platform: its window buttons, the system menu bar and `cmd`; on Windows and Linux the title bar is as designed **(user, M1)** | The design draws Windows' window buttons and `ctrl` everywhere. |
| S009 | The first release opens packages from folders, with local git; GitHub (pull requests, Actions) and the online workspace with no folder are a later milestone (M12) **(user, M0)** | The online workspace needs a host per workspace in wowlua and an in-process command runner. |
| S010 | A dependencies view for wowlua's registry (search, add, update, outdated) is designed and built for the first release (M10) **(user, M0)** | wowlua 0.13 added the registry; the design has no view for it. |
| S011 | The profiler opens the in-game recordings of wowlua's dev window too, in the first release (M7) **(user, M0)** | wowlua 0.13 records combat in game (`wowlua prof report`); the design's profiler only shows Studio's sessions and speedscope files. |
| S012 | Studio keeps its own state (settings, keybindings, recent workspaces, layouts keyed by the workspace's path) in the system's folder for application data, never in the workspace **(user, M1)** | Repositories stay clean. |
| S013 | On macOS the window has no title bar of its own: the traffic lights sit at the left of Studio's drawn bar, which holds the design's row; the menu is the system menu bar **(user, M1)** | As Zed and VS Code do on macOS; the design's row stays one row. |
| S014 | The frame budget: building a frame's UI takes at most 4 ms at the 99th percentile, so a 120 Hz display never drops a frame for Studio's own work; the frame statistics check it **(user, M0)** | Performance comes first (S005); a number makes it testable. |
| S015 | The name is written **wowlua studio**, all lowercase, as wowlua is; the short form in running text is Studio, as the design writes it **(user, M0)** | The brand writes its product names in lowercase; the design's prose calls it Studio. |
