//! The frame budget (decision S014): building a frame's UI and turning it
//! into triangles takes at most 4 ms at the 99th percentile. Checked in
//! optimized builds (`cargo test --release -p wowlua-studio-app --test
//! frame_budget`); unoptimized ones only report the numbers.

mod common;

use std::{
    path::Path,
    time::{Duration, Instant},
};

use egui::{Event, Pos2, RawInput, Rect, Vec2};
use wowlua_studio_app::{commands::Command, pages::Page};

const BUDGET: Duration = Duration::from_millis(4);

#[test]
fn frames_stay_within_the_budget() {
    let ctx = egui::Context::default();
    let (mut studio, _) = common::studio(&ctx);
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 900.0));
    let mut times = Vec::new();
    for frame in 0..600u32 {
        // The start page first, then a workspace: the pointer crosses the
        // window; pages change; the palette opens.
        if frame == 200 {
            studio.open(Path::new("/p/suuui"));
        }
        if frame < 200 && frame % 50 == 25 {
            studio.queue(if frame % 100 == 25 {
                Command::NewAddon
            } else {
                Command::AdoptAddon
            });
        }
        let mut input = RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let x = (frame * 7 % 1440) as f32;
        let y = (frame * 3 % 900) as f32;
        input.events.push(Event::PointerMoved(Pos2::new(x, y)));
        if frame > 200 && frame % 40 == 0 {
            let page = Page::ALL[(frame / 40) as usize % Page::ALL.len()];
            studio.queue(Command::ShowPage(page));
        }
        if frame % 100 == 50 {
            studio.queue(Command::OpenPalette);
        }
        let started = Instant::now();
        let mut output = ctx.run_ui(input, |ui| studio.ui(ui));
        let _ = ctx.tessellate(output.shapes, output.pixels_per_point);
        // No renderer takes the font atlas's updates here.
        output.textures_delta.clear();
        // The first frames build the font atlas.
        if frame >= 20 {
            times.push(started.elapsed());
        }
    }
    times.sort_unstable();
    let at = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    eprintln!(
        "frames: median {:.3} ms · p99 {:.3} ms · slowest {:.3} ms (budget {} ms at p99)",
        ms(at(0.5)),
        ms(at(0.99)),
        ms(at(1.0)),
        ms(BUDGET)
    );
    if !cfg!(debug_assertions) {
        assert!(
            at(0.99) <= BUDGET,
            "the 99th percentile frame took {:.3} ms",
            ms(at(0.99))
        );
    }
}
