//! The wowlua wordmark (`wowlua-wordmark-*.svg`): the name in Martian Mono
//! ExtraBold, in `ink`, as outlines, rasterized ([`raster`]) like the mark.

use std::sync::OnceLock;

use egui::{Context, Painter, Pos2, Rect, Vec2};

use crate::Palette;
use crate::raster::{self, Layer};

/// The SVG's box.
const BOX: Vec2 = Vec2::new(133.69, 48.0);

const OUTLINES: &str = "M0.78 -20.84H5.78L6.83 -11.32L7.28 -4.28H7.82L9.32 -20.84H15.03L16.49 -4.28H17.03L17.44 -11.32L18.5 -20.84H23.02L20.23 0.0H13.67L12.24 -16.52H11.66L10.2 0.0H3.57ZM24.75 -10.44Q24.75 -13.8 25.98 -16.3Q27.2 -18.8 29.44 -20.15Q31.69 -21.49 34.68 -21.49Q37.67 -21.49 39.92 -20.15Q42.16 -18.8 43.38 -16.3Q44.61 -13.8 44.61 -10.44Q44.61 -7.07 43.38 -4.57Q42.16 -2.07 39.92 -0.71Q37.67 0.65 34.68 0.65Q31.69 0.65 29.44 -0.71Q27.2 -2.07 25.98 -4.57Q24.75 -7.07 24.75 -10.44ZM39.47 -10.44Q39.47 -13.43 38.22 -15.1Q36.96 -16.76 34.68 -16.76Q32.4 -16.76 31.14 -15.1Q29.89 -13.43 29.89 -10.44Q29.89 -7.45 31.14 -5.78Q32.4 -4.11 34.68 -4.11Q36.96 -4.11 38.22 -5.78Q39.47 -7.45 39.47 -10.44ZM46.34 -20.84H51.34L52.39 -11.32L52.84 -4.28H53.38L54.88 -20.84H60.59L62.05 -4.28H62.59L63 -11.32L64.06 -20.84H68.58L65.79 0.0H59.23L57.8 -16.52H57.22L55.76 0.0H49.13ZM83.5 -28.83V-4.73H89.62V0.0H72.05V-4.73H78.2V-24.11H72.05V-28.83ZM111.83 0.0H106.52V-1.87H105.26Q104.58 -0.68 103.34 -0.02Q102.1 0.65 100.54 0.65Q97.41 0.65 95.66 -1.53Q93.91 -3.71 93.91 -7.58V-20.84H99.21V-8.91Q99.21 -6.63 100.13 -5.47Q101.05 -4.32 102.85 -4.32Q104.65 -4.32 105.59 -5.47Q106.52 -6.63 106.52 -8.91V-20.84H111.83ZM125.6 -21.49Q129.91 -21.49 132.19 -19.47Q134.47 -17.44 134.47 -13.67V0.0H129.3V-2.04H128.08Q126.79 0.51 123.25 0.51Q121.35 0.51 119.88 -0.26Q118.42 -1.02 117.62 -2.4Q116.82 -3.77 116.82 -5.54Q116.82 -8.26 118.51 -9.96Q120.19 -11.66 123.42 -12.17L129.3 -13.09V-13.26Q129.3 -15.03 128.35 -15.95Q127.4 -16.86 125.56 -16.86Q123.66 -16.86 122.49 -15.96Q121.31 -15.06 120.97 -13.36H115.87Q116.18 -17.1 118.78 -19.3Q121.38 -21.49 125.6 -21.49ZM124.75 -3.91Q125.97 -3.91 127.02 -4.47Q128.08 -5.03 128.69 -5.98Q129.3 -6.94 129.3 -8.06V-9.08L124.54 -8.36Q123.42 -8.19 122.81 -7.58Q122.2 -6.97 122.2 -6.05Q122.2 -5.07 122.86 -4.49Q123.52 -3.91 124.75 -3.91Z";
const OUTLINES_AT: Vec2 = Vec2::new(-0.78, 38.09);

fn outlines() -> &'static [Vec<Pos2>] {
    static POLYS: OnceLock<Vec<Vec<Pos2>>> = OnceLock::new();
    POLYS.get_or_init(|| raster::outlines(OUTLINES, OUTLINES_AT))
}

/// The wordmark's width at `height` points.
pub fn width(height: f32) -> f32 {
    height * BOX.x / BOX.y
}

/// Paints the wordmark `rect.height()` high from `rect`'s left.
pub fn paint_wordmark(ctx: &Context, painter: &Painter, rect: Rect, palette: &Palette) {
    raster::paint(
        ctx,
        painter,
        rect,
        BOX,
        "wowlua-wordmark",
        &[Layer {
            polygons: outlines(),
            color: palette.ink,
        }],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Color32;

    #[test]
    fn the_letters_are_filled_and_their_counters_open() {
        let img = raster::rasterize(
            134,
            48,
            BOX,
            &[Layer {
                polygons: outlines(),
                color: Color32::WHITE,
            }],
        );
        let at = |x: usize, y: usize| img.pixels[y * 134 + x];
        // The `l`'s stem, the `o`'s ring and the hole inside it.
        assert_eq!(at(80, 24), Color32::WHITE);
        assert_eq!(at(27, 28), Color32::WHITE);
        assert_eq!(at(34, 28), Color32::TRANSPARENT);
    }
}
