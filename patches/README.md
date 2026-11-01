# Patched dependencies

Copies of published crates with a change Studio needs before their authors
release it, patched in with `[patch.crates-io]` in the workspace's
`Cargo.toml`. Each keeps its license; the changes are marked with a
`wowlua studio:` comment.

## egui-wgpu 0.37.0

egui-wgpu writes each frame's vertices and indices with
`Queue::write_buffer_with`. wgpu makes a new staging buffer for every such
write, and making a buffer is a call into the GPU driver: on macOS the UI
thread waited there, often for milliseconds, and some frames took 4 to
60 ms (S059).

The change, in `src/renderer.rs`: the renderer keeps a `wgpu::util::StagingBelt`
and writes both through it, recalled when the next frame's buffers are
written (the previous frame's encoder is submitted by then), and made again
with a larger chunk when a frame's writes outgrow it.

Drop the copy when an egui-wgpu release writes its buffers without a new
staging buffer per frame: remove the folder, its `exclude` and its
`[patch.crates-io]` line.
