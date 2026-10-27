//! The [wl] mark's 16 px pixel version (`wowlua-mark-16-*.svg`): brackets
//! in `ink`, letters in `amber-ink`, drawn with crisp edges at whole
//! multiples of 16 px only.

use egui::{Mesh, Painter, Pos2, Rect, Shape, Vec2};

use crate::Palette;

/// The design system's grid: `#` the brackets, `a` the letters.
const GRID: [&str; 16] = [
    "................",
    ".##..........##.",
    ".#............#.",
    ".#.......aa...#.",
    ".#........a...#.",
    ".#........a...#.",
    ".#........a...#.",
    ".#........a...#.",
    ".#.a...a..a...#.",
    ".#.a...a..a...#.",
    ".#.a.a.a..a...#.",
    ".#.a.a.a..a...#.",
    ".#..a.a...aaa.#.",
    ".#............#.",
    ".##..........##.",
    "................",
];

/// Paints the mark with its top left at `at`, `scale` times 16 px: one
/// mesh of square cells, without anti-aliasing, so its edges stay crisp.
pub fn paint_mark16(painter: &Painter, at: Pos2, scale: u8, palette: &Palette) {
    let px = f32::from(scale.max(1));
    let mut mesh = Mesh::default();
    for (y, row) in GRID.iter().enumerate() {
        for (x, cell) in row.bytes().enumerate() {
            let color = match cell {
                b'#' => palette.ink,
                b'a' => palette.amber_ink,
                _ => continue,
            };
            let min = at + Vec2::new(x as f32, y as f32) * px;
            mesh.add_colored_rect(Rect::from_min_size(min, Vec2::splat(px)), color);
        }
    }
    painter.add(Shape::mesh(mesh));
}
