//! The [wl] mark (`wowlua-mark-*.svg`, decision S023): brackets in `ink`,
//! letters in `amber-ink`, from 20 px up. Its letters are not convex, so
//! it is rasterized from its outlines (4 × 4 samples a pixel) at the
//! display's own pixel size, once per size and theme, and drawn as a
//! texture.

use std::sync::OnceLock;

use egui::{
    Color32, ColorImage, Context, Id, Painter, Pos2, Rect, TextureHandle, TextureOptions, Vec2,
};

use crate::{Palette, icons};

/// The mark's box: the SVG's 48 units.
const BOX: f32 = 48.0;
const SAMPLES: usize = 4;

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
    LETTERS_POLYS.get_or_init(|| {
        icons::parse(LETTERS)
            .into_iter()
            .map(|s| s.points.into_iter().map(|p| p + LETTERS_AT).collect())
            .collect()
    })
}

/// Paints the mark into the square `rect` (in points), its edges on the
/// display's pixels.
pub fn paint_mark(ctx: &Context, painter: &Painter, rect: Rect, palette: &Palette) {
    let ppp = ctx.pixels_per_point();
    let px = (rect.width() * ppp).round().max(1.0) as usize;
    let key = Id::unique(("wowlua-studio-mark", px, palette.ink, palette.amber_ink));
    let texture = ctx
        .data(|d| d.get_temp::<TextureHandle>(key))
        .unwrap_or_else(|| {
            let image = rasterize(px, palette.ink, palette.amber_ink);
            let handle = ctx.load_texture("wowlua-mark", image, TextureOptions::NEAREST);
            ctx.data_mut(|d| d.insert_temp(key, handle.clone()));
            handle
        });
    // On the pixel grid, so the texture's pixels are the display's.
    let min = Pos2::new(
        (rect.min.x * ppp).round() / ppp,
        (rect.min.y * ppp).round() / ppp,
    );
    let size = Vec2::splat(px as f32 / ppp);
    painter.image(
        texture.id(),
        Rect::from_min_size(min, size),
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        Color32::WHITE,
    );
}

/// The mark at `px` × `px` pixels, on transparency.
fn rasterize(px: usize, ink: Color32, amber: Color32) -> ColorImage {
    let scale = px as f32 / BOX;
    let place = |polys: &[Vec<Pos2>]| -> Vec<Vec<Pos2>> {
        polys
            .iter()
            .map(|p| {
                p.iter()
                    .map(|q| Pos2::new(q.x * scale, q.y * scale))
                    .collect()
            })
            .collect()
    };
    let layers = [
        (coverage(&place(&brackets()), px), ink),
        (coverage(&place(letters()), px), amber),
    ];
    let mut pixels = vec![Color32::TRANSPARENT; px * px];
    for (i, pixel) in pixels.iter_mut().enumerate() {
        // The letters never overlap the brackets: each pixel takes one.
        for (cov, color) in &layers {
            let c = cov[i].min(1.0);
            if c > 0.0 {
                *pixel = color.gamma_multiply(c);
            }
        }
    }
    ColorImage::new([px, px], pixels)
}

/// The share of each pixel the polygons cover (nonzero winding).
fn coverage(polys: &[Vec<Pos2>], px: usize) -> Vec<f32> {
    let mut cov = vec![0.0f32; px * px];
    let edges: Vec<(Pos2, Pos2)> = polys
        .iter()
        .flat_map(|p| {
            p.iter()
                .zip(p.iter().cycle().skip(1))
                .map(|(a, b)| (*a, *b))
        })
        .filter(|(a, b)| a.y != b.y)
        .collect();
    let sub = 1.0 / SAMPLES as f32;
    let mut crossings: Vec<(f32, i32)> = Vec::new();
    for py in 0..px {
        let row = &mut cov[py * px..(py + 1) * px];
        for s in 0..SAMPLES {
            let y = py as f32 + (s as f32 + 0.5) * sub;
            crossings.clear();
            for (a, b) in &edges {
                if (a.y <= y && y < b.y) || (b.y <= y && y < a.y) {
                    let x = a.x + (y - a.y) * (b.x - a.x) / (b.y - a.y);
                    crossings.push((x, if b.y > a.y { 1 } else { -1 }));
                }
            }
            crossings.sort_by(|l, r| l.0.total_cmp(&r.0));
            let mut wind = 0;
            let mut start = 0.0;
            for &(x, dir) in &crossings {
                let before = wind;
                wind += dir;
                if before == 0 && wind != 0 {
                    start = x;
                } else if before != 0 && wind == 0 {
                    span(row, start, x, sub);
                }
            }
        }
    }
    cov
}

/// Adds one sub-row's span `x0..x1` to the row's coverage.
fn span(row: &mut [f32], x0: f32, x1: f32, weight: f32) {
    let x0 = x0.max(0.0);
    let x1 = x1.min(row.len() as f32);
    if x1 <= x0 {
        return;
    }
    let (i0, i1) = (x0 as usize, x1 as usize);
    if i0 == i1 {
        row[i0] += (x1 - x0) * weight;
        return;
    }
    row[i0] += (i0 as f32 + 1.0 - x0) * weight;
    let end = i1.min(row.len());
    for c in &mut row[i0 + 1..end] {
        *c += weight;
    }
    if i1 < row.len() {
        row[i1] += (x1 - i1 as f32) * weight;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mark_has_its_inks_where_the_svg_puts_them() {
        let img = rasterize(48, Color32::WHITE, Color32::RED);
        let at = |x: usize, y: usize| img.pixels[y * 48 + x];
        // A bracket's upright, the `l`'s stem, the space between them.
        assert_eq!(at(4, 24), Color32::WHITE);
        assert_eq!(at(30, 24), Color32::RED);
        assert_eq!(at(24, 40), Color32::TRANSPARENT);
    }
}
