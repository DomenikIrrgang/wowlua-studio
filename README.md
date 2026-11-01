<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset=".github/assets/wowlua-studio-banner-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset=".github/assets/wowlua-studio-banner-light.svg">
    <img src=".github/assets/wowlua-studio-banner-dark.svg" alt="wowlua studio: The IDE for wowlua" width="100%">
  </picture>
</p>

<p align="center">
  <a href="https://github.com/DomenikIrrgang/wowlua-studio/actions/workflows/ci.yml"><img alt="ci" src="https://img.shields.io/github/actions/workflow/status/DomenikIrrgang/wowlua-studio/ci.yml?branch=main&style=flat-square&label=ci&labelColor=14120e"></a>
  <a href="https://github.com/DomenikIrrgang/wowlua/releases/tag/v0.13.0"><img alt="wowlua 0.13.0" src="https://img.shields.io/badge/wowlua-0.13.0-ffb547?style=flat-square&labelColor=14120e"></a>
  <img alt="Windows, macOS, Linux" src="https://img.shields.io/badge/runs%20on-windows%20%C2%B7%20macos%20%C2%B7%20linux-ffb547?style=flat-square&labelColor=14120e">
  <a href="LICENSE"><img alt="MIT licence" src="https://img.shields.io/badge/licence-MIT-ffb547?style=flat-square&labelColor=14120e"></a>
</p>

<p align="center">
  <a href="https://wowlua.dev"><b>wowlua.dev</b></a> ·
  <a href="https://github.com/DomenikIrrgang/wowlua">wowlua</a> ·
  <a href="#building">building</a> ·
  <a href="spec/roadmap.md">roadmap</a> ·
  <a href="spec/appendix/decisions.md">decisions</a> ·
  <a href="https://discord.gg/Dn795czPYk">discord</a>
</p>

---

**wowlua studio** is the IDE for [wowlua](https://github.com/DomenikIrrgang/wowlua):
edit, check, test, run, debug and profile World of Warcraft AddOns in
wowlua's emulated client, with the game's screen beside your code.

Studio is a native application for Windows, macOS and Linux, written in
Rust and drawn on the GPU. wowlua's compiler, language server, test runner,
debugger and renderer run inside it.

> [!NOTE]
> Studio is at its beginning: milestone M1 of the [roadmap](spec/roadmap.md).
> It has its window, pages and panels; opening folders comes next.

## Building

Studio needs Rust 1.96 or newer.

```console
$ cargo run --release
```

wowlua comes from its repository at the release tag set in `Cargo.toml`.

## License

[MIT](LICENSE)
