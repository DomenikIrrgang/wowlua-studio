# wowlua Studio

The IDE for wowlua. The spec (`spec/`) is the source of truth: a change that
differs from it updates it too (the roadmap, and a row in
`spec/appendix/decisions.md`).

## Ground rules

- Pure native Rust: no HTML, no JavaScript, no webview (S001).
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
tool (`project/canvas.json` lists the artboards). Where Studio differs from
it, the design changes first.

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

## Checks

Before a commit: `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings` and
`cargo test --workspace`.

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
