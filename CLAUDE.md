# wowlua studio

The IDE for wowlua. The spec (`spec/`) is the source of truth: a change that
differs from it updates it too (the roadmap, and a row in
`spec/appendix/decisions.md`).

## Ground rules

- Pure native Rust: no HTML, no JavaScript, no webview (S001).
- No `unsafe`, except in `crates/wowlua-studio/src/traffic_lights.rs`
  (macOS's window buttons, S022).
- Windows, macOS and Linux; a browser build must stay possible, so every
  dependency builds for `wasm32-unknown-unknown` and everything is drawn on
  `wgpu` (S002).
- egui (eframe) on `wgpu`; heavy views are Studio's own widgets (S003).
- Performance comes first (S005): redraw only when something changed, draw
  only what is visible, never block the UI thread with checks, tests or the
  game. `WOWLUA_STUDIO_FRAME_STATS=1` prints frame times.
- Never assume. When the design, the spec or wowlua are unclear, missing a
  part or disagree with each other, ask the user; don't guess.

## The design

The canvas [wowlua studio](https://claude.ai/artifact/RBjKPKR9gzEDfr6KA4u9CV)
is the design: one artboard per screen, its states as tweaks, drawn with the
wowlua design system (tokens, fonts, components). Read it with the Artifact
tool (`project/canvas.json` lists the artboards). Where Studio is decided
to differ from it, the decision is a row in `spec/appendix/decisions.md`
and the canvas change is listed under "Design changes owed" in the
roadmap until the canvas has it.

## Crates

- `wowlua-studio-ui`: the design system in egui (theme, fonts, icons, the
  [wl] mark and the wordmark, widgets, form controls). egui only.
- `wowlua-studio-app`: the application (chrome, docking, commands, the
  palette, pages, the start page, the explorer, the editor, the language
  server's client, search, the terminal panel, notifications, jobs, the
  tests page and coverage, the game page, the debugger). egui
  and serde only: the host gives it the platform, wowlua's version, a file
  store and the system (`system::System`: the disk, Git, wowlua's library,
  the language server, highlighting, the terminal, new windows; slow work
  comes back as events). Both build for wasm (CI checks).
- `wowlua-studio`: the native host (eframe, the store in the system's
  folder for application data, `native.rs`'s system, the language server's
  thread `lsp.rs`, the terminals `terminal.rs`, test runs `tests.rs`,
  the game's session and screen `game.rs`, macOS's menu bar) and
  wowlua's crates.

## Seeing it

`WOWLUA_STUDIO_SCREENSHOT=out.png` draws a few frames, saves the window as
a PNG and quits; `WOWLUA_STUDIO_SCREENSHOT_RUN="open:<folder>;page:debug;theme:light;palette"`
runs commands first (`new` and `adopt` show the start page's forms;
`world:<profile>[/<part>][@json]`, `spells:<query>` and
`worldlive:<profile>|<path>=<json>|…` the world profiles page). `WOWLUA_STUDIO_DATA=<dir>` keeps settings and layouts
there instead of the user's folder.

## wowlua

Studio depends on wowlua's crates by git, pinned to a release tag in
`Cargo.toml` (S004). To work on both at once, create `.cargo/config.toml`
(ignored by git) with a patch for every wowlua crate Studio depends on:

```toml
[patch."https://github.com/DomenikIrrgang/wowlua"]
wowlua-driver = { path = "../wowlua/crates/wowlua-driver" }
```

Cargo uses the patch only when the checkout's version is compatible with
the pinned one (0.13.x for `v0.13.0`); otherwise it warns that the patch
was not used. A change Studio needs in wowlua is made in wowlua, by its own
rules, and Studio moves its pin to the release that has it.

## Patched dependencies

`patches/` holds copies of published crates with a change Studio needs
before their authors release it, patched in with `[patch.crates-io]` and
kept out of the workspace (`exclude`), so Studio's checks leave them alone.
`patches/README.md` says what each changes and when to drop it.

## Checks

Before a commit: `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`, and the frame budget optimized, one test at a
time: `cargo test --release -p wowlua-studio-app --test frame_budget --
--test-threads=1`.

## Versions

Studio has its own [semantic version](https://semver.org) (S006): each
minor version closes a milestone of the [roadmap](spec/roadmap.md); releases
within a milestone bump PATCH; pre-releases add `-alpha.N` or `-beta.N`.

## Changelog

`CHANGELOG.md` has wowlua's form: a section per version, newest first,
headed `## <version> (<YYYY-MM-DD>)`, with the changes between releases
under `## Unreleased`; entries under `### New`, `### Changed` and
`### Fixed`, one `- ` line per change an addon author notices, in plain
language. Each release names the wowlua version built in.
