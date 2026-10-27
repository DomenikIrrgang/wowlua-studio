//! The [wl] mark (`wowlua-mark-*.svg`, decision S023): brackets in `ink`,
//! letters in `amber-ink`, from 20 px up. Its letters are not convex, so
//! it is rasterized ([`raster`]) once per size and theme.

use std::sync::OnceLock;

use egui::{Color32, Context, Painter, Pos2, Rect, Vec2};

use crate::Palette;
use crate::raster::{self, Layer};

/// The mark's box: the SVG's 48 units.
const BOX: f32 = 48.0;

/// The letters' outlines (Martian Mono ExtraBold), placed as in the SVG.
const LETTERS: &str = "M0.36 -11.16H3.33L3.73 -6.48L3.87 -2.72H4.23L4.9 -11.16H8.08L8.69 -2.72H9.05L9.18 -6.48L9.56 -11.16H12.24L11 0.0H7.11L6.52 -8.41H6.16L5.51 0.0H1.6Z M20.12 -15.26V-2.84H23.36V0.0H13.64V-2.84H16.88V-12.42H13.64V-15.26Z";
const LETTERS_AT: Vec2 = Vec2::new(12.14, 31.63);

/// The brackets: `M9.5 5H4V43H9.5 M38.5 5H44V43H38.5`, a 3.5 wide stroke
/// with square caps, as rectangles.
fn brackets() -> Vec<Vec<Pos2>> {
    let h = 1.75;
    let r = |x0: f32, y0: f32, x1: f32, y1: f32| {
        vec![
            Pos2::new(x0, y0),
            Pos2::new(x1, y0),
            Pos2::new(x1, y1),
            Pos2::new(x0, y1),
        ]
    };
    vec![
        r(4.0 - h, 5.0 - h, 4.0 + h, 43.0 + h),
        r(4.0 - h, 5.0 - h, 9.5 + h, 5.0 + h),
        r(4.0 - h, 43.0 - h, 9.5 + h, 43.0 + h),
        r(44.0 - h, 5.0 - h, 44.0 + h, 43.0 + h),
        r(38.5 - h, 5.0 - h, 44.0 + h, 5.0 + h),
        r(38.5 - h, 43.0 - h, 44.0 + h, 43.0 + h),
    ]
}

fn letters() -> &'static [Vec<Pos2>] {
    static LETTERS_POLYS: OnceLock<Vec<Vec<Pos2>>> = OnceLock::new();
    LETTERS_POLYS.get_or_init(|| raster::outlines(LETTERS, LETTERS_AT))
}

/// Paints the mark into the square `rect` (in points), its edges on the
/// display's pixels.
pub fn paint_mark(ctx: &Context, painter: &Painter, rect: Rect, palette: &Palette) {
    raster::paint(
        ctx,
        painter,
        rect,
        Vec2::splat(BOX),
        "wowlua-mark",
        &layers(palette.ink, palette.amber_ink, &brackets()),
    );
}

fn layers(ink: Color32, amber: Color32, brackets: &[Vec<Pos2>]) -> [Layer<'_>; 2] {
    [
        Layer {
            polygons: brackets,
            color: ink,
        },
        Layer {
            polygons: letters(),
            color: amber,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mark_has_its_inks_where_the_svg_puts_them() {
        let brackets = brackets();
        let img = raster::rasterize(
            48,
            48,
            Vec2::splat(BOX),
            &layers(Color32::WHITE, Color32::RED, &brackets),
        );
        let at = |x: usize, y: usize| img.pixels[y * 48 + x];
        // A bracket's upright, the `l`'s stem, the space between them.
        assert_eq!(at(4, 24), Color32::WHITE);
        assert_eq!(at(30, 24), Color32::RED);
        assert_eq!(at(24, 40), Color32::TRANSPARENT);
    }
}
