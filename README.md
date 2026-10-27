# wowlua Studio

The IDE for [wowlua](https://github.com/DomenikIrrgang/wowlua): edit, check,
test, run, debug and profile World of Warcraft AddOns in wowlua's emulated
client, with the game's screen beside your code.

Studio is a native application for Windows, macOS and Linux, written in
Rust and drawn on the GPU. wowlua's compiler, language server, test runner,
debugger and renderer run inside it.

> [!NOTE]
> Studio is at its beginning: milestone M0 of the [roadmap](spec/roadmap.md).
> It opens a window and nothing more yet.

## Building

Studio needs Rust 1.96 or newer.

```console
$ cargo run --release
```

wowlua comes from its repository at the release tag set in `Cargo.toml`.

## License

[MIT](LICENSE)
