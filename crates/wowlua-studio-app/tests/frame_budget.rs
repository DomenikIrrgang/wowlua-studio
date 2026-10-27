//! The frame budget (decision S014): building a frame's UI and turning it
//! into triangles takes at most 4 ms at the 99th percentile. Checked in
//! optimized builds, one test at a time (`cargo test --release -p
//! wowlua-studio-app --test frame_budget -- --test-threads=1`: tests side
//! by side time each other); unoptimized ones only report the numbers.

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
        // A large file in the editor, from frame 300.
        if frame == 300 {
            studio.edit(Path::new("/p/suuui/src/big.wowlua"));
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

/// A data file on one 2 MB line: opening it and drawing it with the
/// pointer over it.
#[test]
fn a_file_on_one_long_line_stays_within_the_budget() {
    let ctx = egui::Context::default();
    let (mut studio, _) = common::studio(&ctx);
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 900.0));
    studio.open(Path::new("/p/suuui"));
    let mut times = Vec::new();
    for frame in 0..120u32 {
        if frame == 10 {
            studio.edit(Path::new("/p/suuui/data/long.json"));
        }
        let mut input = RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let x = (300 + frame * 7 % 1000) as f32;
        input.events.push(Event::PointerMoved(Pos2::new(x, 140.0)));
        let started = Instant::now();
        let mut output = ctx.run_ui(input, |ui| studio.ui(ui));
        let _ = ctx.tessellate(output.shapes, output.pixels_per_point);
        output.textures_delta.clear();
        if frame >= 20 {
            times.push(started.elapsed());
        }
    }
    times.sort_unstable();
    let at = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    eprintln!(
        "long line: median {:.3} ms · p99 {:.3} ms · slowest {:.3} ms",
        ms(at(0.5)),
        ms(at(0.99)),
        ms(at(1.0))
    );
    if !cfg!(debug_assertions) {
        assert!(at(0.99) <= BUDGET, "p99 {:.3} ms", ms(at(0.99)));
    }
}

/// The search panel with as many matches as a search keeps, on long
/// lines, scrolled through.
#[test]
fn the_search_panel_stays_within_the_budget() {
    use wowlua_studio_app::search::{self, SearchState, Tab};
    use wowlua_studio_app::system::{Found, Hit, Search};
    let ctx = egui::Context::default();
    // The fonts and the palette.
    let _ = common::studio(&ctx);
    let line = "local x = \"test\" ".repeat(120);
    let files = (0..300)
        .map(|f| {
            let hits = (0..17)
                .map(|i| Hit {
                    line: i,
                    at: (11, 15),
                    text: line.clone(),
                    start: 11,
                    end: 15,
                })
                .collect();
            (Path::new("/p/suuui/src").join(format!("f{f}.wowlua")), hits)
        })
        .collect();
    let query = Search {
        query: "test".to_owned(),
        ..Search::default()
    };
    let mut state = SearchState {
        tab: Tab::Text,
        search: query.clone(),
        found: Some((
            query,
            Found {
                files,
                searched: 300,
                millis: 40,
                error: None,
                cut: true,
            },
        )),
        ..SearchState::default()
    };
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(420.0, 900.0));
    let mut times = Vec::new();
    for frame in 0..300u32 {
        let mut input = RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        input.events.push(Event::PointerMoved(Pos2::new(
            200.0,
            (frame * 3 % 900) as f32,
        )));
        input.events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: Vec2::new(0.0, -400.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
        });
        let started = Instant::now();
        let mut output = ctx.run_ui(input, |ui| {
            search::show(ui, &mut state, Path::new("/p/suuui"));
        });
        let _ = ctx.tessellate(output.shapes, output.pixels_per_point);
        output.textures_delta.clear();
        if frame >= 20 {
            times.push(started.elapsed());
        }
    }
    times.sort_unstable();
    let at = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    eprintln!(
        "search panel: median {:.3} ms · p99 {:.3} ms",
        ms(at(0.5)),
        ms(at(0.99))
    );
    if !cfg!(debug_assertions) {
        assert!(at(0.99) <= BUDGET, "p99 {:.3} ms", ms(at(0.99)));
    }
}
