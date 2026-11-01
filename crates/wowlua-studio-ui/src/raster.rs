//! Drawing the brand's outlines (the [wl] mark, the wordmark) into images
//! on the CPU: egui fills convex shapes only, and the letters are not.
//! Polygons are filled with nonzero winding, 4 × 4 samples a pixel, at the
//! display's own pixel size, and kept as textures.

use egui::{
    Color32, ColorImage, Context, Id, Painter, Pos2, Rect, TextureHandle, TextureOptions, Vec2,
};

const SAMPLES: usize = 4;

/// A shape of an outline in its box's units, and its colour.
pub struct Layer<'a> {
    pub polygons: &'a [Vec<Pos2>],
    pub color: Color32,
}

/// Paints `layers` (drawn in a `box_size` box) into `rect`, keeping the
/// box's proportions; the image is made once per size and colours.
pub fn paint(
    ctx: &Context,
    painter: &Painter,
    rect: Rect,
    box_size: Vec2,
    key: &str,
    layers: &[Layer<'_>],
) {
    let ppp = ctx.pixels_per_point();
    let h = (rect.height() * ppp).round().max(1.0) as usize;
    let w = ((box_size.x / box_size.y) * h as f32).round().max(1.0) as usize;
    let colors: Vec<Color32> = layers.iter().map(|l| l.color).collect();
    let id = Id::unique((key, w, h, colors));
    let texture = ctx
        .data(|d| d.get_temp::<TextureHandle>(id))
        .unwrap_or_else(|| {
            let image = rasterize(w, h, box_size, layers);
            let handle = ctx.load_texture(key, image, TextureOptions::NEAREST);
            ctx.data_mut(|d| d.insert_temp(id, handle.clone()));
            handle
        });
    // On the pixel grid, so the image's pixels are the display's.
    let min = Pos2::new(
        (rect.min.x * ppp).round() / ppp,
        (rect.min.y * ppp).round() / ppp,
    );
    painter.image(
        texture.id(),
        Rect::from_min_size(min, Vec2::new(w as f32, h as f32) / ppp),
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        Color32::WHITE,
    );
}

/// The layers at `w` × `h` pixels, on transparency; later layers cover
/// earlier ones.
pub fn rasterize(w: usize, h: usize, box_size: Vec2, layers: &[Layer<'_>]) -> ColorImage {
    let scale = Vec2::new(w as f32 / box_size.x, h as f32 / box_size.y);
    let mut pixels = vec![Color32::TRANSPARENT; w * h];
    for layer in layers {
        let placed: Vec<Vec<Pos2>> = layer
            .polygons
            .iter()
            .map(|p| {
                p.iter()
                    .map(|q| Pos2::new(q.x * scale.x, q.y * scale.y))
                    .collect()
            })
            .collect();
        let cov = coverage(&placed, w, h);
        for (pixel, c) in pixels.iter_mut().zip(cov) {
            let c = c.min(1.0);
            if c > 0.0 {
                let top = layer.color.gamma_multiply(c);
                *pixel = if *pixel == Color32::TRANSPARENT {
                    top
                } else {
                    blend(*pixel, top)
                };
            }
        }
    }
    ColorImage::new([w, h], pixels)
}

/// `top` over `under`, both premultiplied.
fn blend(under: Color32, top: Color32) -> Color32 {
    let a = top.a() as f32 / 255.0;
    let mix = |u: u8, t: u8| (t as f32 + u as f32 * (1.0 - a)).round() as u8;
    Color32::from_rgba_premultiplied(
        mix(under.r(), top.r()),
        mix(under.g(), top.g()),
        mix(under.b(), top.b()),
        mix(under.a(), top.a()),
    )
}

/// The share of each pixel the polygons cover (nonzero winding).
pub fn coverage(polys: &[Vec<Pos2>], w: usize, h: usize) -> Vec<f32> {
    let mut cov = vec![0.0f32; w * h];
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
    for py in 0..h {
        let row = &mut cov[py * w..(py + 1) * w];
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
    let len = row.len();
    let x0 = x0.max(0.0);
    let x1 = x1.min(len as f32);
    if x1 <= x0 {
        return;
    }
    let (i0, i1) = (x0 as usize, x1 as usize);
    if i0 == i1 {
        row[i0] += (x1 - x0) * weight;
        return;
    }
    row[i0] += (i0 as f32 + 1.0 - x0) * weight;
    let end = i1.min(len);
    for c in &mut row[i0 + 1..end] {
        *c += weight;
    }
    if i1 < len {
        row[i1] += (x1 - i1 as f32) * weight;
    }
}

/// An SVG path's outlines (`M L H V Q Z`, absolute or relative), its curves
/// flattened, moved by `offset`.
pub fn outlines(d: &str, offset: Vec2) -> Vec<Vec<Pos2>> {
    let mut out: Vec<Vec<Pos2>> = Vec::new();
    let mut cur = Pos2::ZERO;
    let mut start = Pos2::ZERO;
    let mut cmd = 'M';
    let mut nums = Numbers::new(d);
    loop {
        if let Some(c) = nums.command() {
            cmd = c;
            if c.eq_ignore_ascii_case(&'z') {
                cur = start;
                continue;
            }
        }
        let rel = cmd.is_ascii_lowercase();
        let base = if rel { cur.to_vec2() } else { Vec2::ZERO };
        match cmd.to_ascii_uppercase() {
            'M' => {
                let Some(p) = nums.point() else { break };
                cur = p + base;
                start = cur;
                out.push(vec![cur + offset]);
                cmd = if rel { 'l' } else { 'L' };
            }
            'L' => {
                let Some(p) = nums.point() else { break };
                cur = p + base;
                push(&mut out, cur + offset);
            }
            'H' => {
                let Some(x) = nums.number() else { break };
                cur = Pos2::new(x + base.x, cur.y);
                push(&mut out, cur + offset);
            }
            'V' => {
                let Some(y) = nums.number() else { break };
                cur = Pos2::new(cur.x, y + base.y);
                push(&mut out, cur + offset);
            }
            'Q' => {
                let (Some(c), Some(p)) = (nums.point(), nums.point()) else {
                    break;
                };
                let (c, p) = (c + base, p + base);
                for i in 1..=8 {
                    let t = i as f32 / 8.0;
                    let u = 1.0 - t;
                    let q = Pos2::new(
                        u * u * cur.x + 2.0 * u * t * c.x + t * t * p.x,
                        u * u * cur.y + 2.0 * u * t * c.y + t * t * p.y,
                    );
                    push(&mut out, q + offset);
                }
                cur = p;
            }
            _ => break,
        }
    }
    out
}

fn push(out: &mut [Vec<Pos2>], p: Pos2) {
    if let Some(last) = out.last_mut() {
        last.push(p);
    }
}

/// The numbers and commands of path data.
struct Numbers<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Numbers<'a> {
    fn new(d: &'a str) -> Self {
        Numbers {
            s: d.as_bytes(),
            i: 0,
        }
    }

    fn skip(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b',' | b'\n') {
            self.i += 1;
        }
    }

    fn command(&mut self) -> Option<char> {
        self.skip();
        let b = *self.s.get(self.i)?;
        b.is_ascii_alphabetic().then(|| {
            self.i += 1;
            b as char
        })
    }

    fn number(&mut self) -> Option<f32> {
        self.skip();
        let from = self.i;
        let mut dot = false;
        while let Some(&c) = self.s.get(self.i) {
            let sign = (c == b'-' || c == b'+') && self.i == from;
            if c.is_ascii_digit() || sign || (c == b'.' && !dot) {
                dot |= c == b'.';
                self.i += 1;
            } else {
                break;
            }
        }
        std::str::from_utf8(&self.s[from..self.i])
            .ok()?
            .parse()
            .ok()
    }

    fn point(&mut self) -> Option<Pos2> {
        Some(Pos2::new(self.number()?, self.number()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_square_covers_its_pixels() {
        let square = vec![vec![
            Pos2::new(1.0, 1.0),
            Pos2::new(3.0, 1.0),
            Pos2::new(3.0, 3.0),
            Pos2::new(1.0, 3.0),
        ]];
        let cov = coverage(&square, 4, 4);
        assert_eq!(cov[4 + 1], 1.0);
        assert_eq!(cov[0], 0.0);
    }

    #[test]
    fn curves_are_flattened() {
        let o = outlines("M0 0Q5 10 10 0Z", Vec2::ZERO);
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].len(), 9);
        assert_eq!(o[0][8], Pos2::new(10.0, 0.0));
        assert!(o[0][4].y > 4.0, "the curve bends down: {:?}", o[0][4]);
    }
}
