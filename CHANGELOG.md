# Changelog

All notable changes to wowlua studio. Each minor version closes a milestone of the [roadmap](spec/roadmap.md).

## Unreleased

### New

- **wowlua studio**: a native window for Windows, macOS and Linux, drawn on the GPU, with wowlua built in.
- **Start page**: open a folder, make a new addon (as `wowlua new` does) or adopt one (as `wowlua init` does, on a new Git branch), your recent workspaces, the WoW installation and Blizzard's UI for its flavors, and what's new in the wowlua built in.
- Open a package or workspace folder from the file menu (`ctrl o`, `cmd o` on macOS), by dropping it onto the window, or with `wowlua-studio <folder>`. The explorer lists its packages and their files, and shows files changed outside Studio at once.
- A package whose `wowlua` requirement excludes the wowlua built in opens with checks off, and says so in the status bar.
- One workspace per window: `ctrl shift n` opens a new window, and opening another folder asks whether to open it here or in a new window.
- The rail's pages, each with its own layout of panels. Drag a panel by its tab or grip to another dock or a split, float it in a window of its own, maximize it with a double click, or move it from its `…` menu or with `ctrl shift m` (`cmd shift m` on macOS). Studio remembers each page's layout.
- The command palette: `ctrl k` (`cmd k` on macOS), then `>` for commands.
- Dark and light themes, from the command palette or the view menu.
- The [wl] mark as the app's icon, in the macOS Dock's shape on macOS.
