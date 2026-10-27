//! The hover over code: it stands where the pointer rested, stays a
//! moment once the pointer leaves, and stays while the pointer is on it
//! (to scroll a long one).

mod common;

use std::path::Path;

use egui::{Event, Pos2, RawInput, Rect, Vec2};
use wowlua_studio_app::Studio;

/// A frame at `time`, the pointer at `pointer`, with `more` events.
fn frame(ctx: &egui::Context, studio: &mut Studio, time: f64, pointer: Pos2, more: Vec<Event>) {
    frame_with(ctx, studio, time, pointer, more, egui::Modifiers::NONE);
}

/// [`frame`], with `modifiers` held.
fn frame_with(
    ctx: &egui::Context,
    studio: &mut Studio,
    time: f64,
    pointer: Pos2,
    more: Vec<Event>,
    modifiers: egui::Modifiers,
) {
    let mut events = vec![
        Event::ModifiersChanged(modifiers),
        Event::PointerMoved(pointer),
    ];
    events.extend(more);
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 900.0))),
        time: Some(time),
        events,
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| studio.ui(ui));
    output.textures_delta.clear();
}

/// Rests the pointer over the code until a hover shows; where it rested.
fn rest_until_hover(ctx: &egui::Context, studio: &mut Studio, time: &mut f64) -> Pos2 {
    for y in (100..500).step_by(7) {
        let at = Pos2::new(420.0, y as f32);
        for _ in 0..14 {
            frame(ctx, studio, *time, at, Vec::new());
            *time += 0.05;
            if studio.hovered().is_some() {
                return at;
            }
        }
    }
    panic!("no hover over the code");
}

#[test]
fn a_name_with_a_definition_is_underlined_while_cmd_is_held() {
    let ctx = egui::Context::default();
    let (mut studio, _) = common::studio(&ctx);
    let mut t = 0.0;
    let far = Pos2::new(1430.0, 895.0);
    studio.open(Path::new("/p/suuui"));
    frame(&ctx, &mut studio, t, far, Vec::new());
    studio.edit(Path::new("/p/suuui/src/main.wowlua"));
    for _ in 0..3 {
        t += 0.05;
        frame(&ctx, &mut studio, t, far, Vec::new());
    }
    // A place over a name: where a hover comes.
    let on = rest_until_hover(&ctx, &mut studio, &mut t);
    assert_eq!(studio.linked(), None, "without cmd, no link");
    let cmd = egui::Modifiers::COMMAND;
    for _ in 0..3 {
        t += 0.05;
        frame_with(&ctx, &mut studio, t, on, Vec::new(), cmd);
    }
    let (line, start, end) = studio.linked().expect("underlined while cmd is held");
    assert_eq!(Some(line), studio.hovered().map(|h| h.0));
    assert!(start < end);
    t += 0.05;
    frame(&ctx, &mut studio, t, on, Vec::new());
    assert_eq!(studio.linked(), None, "cmd let go: no link");
}

#[test]
fn a_hover_stays_a_moment_and_while_the_pointer_is_on_it() {
    let ctx = egui::Context::default();
    let (mut studio, _) = common::studio(&ctx);
    let mut t = 0.0;
    let far = Pos2::new(1430.0, 895.0);
    studio.open(Path::new("/p/suuui"));
    for _ in 0..3 {
        frame(&ctx, &mut studio, t, far, Vec::new());
        t += 0.05;
    }
    studio.edit(Path::new("/p/suuui/src/main.wowlua"));
    for _ in 0..3 {
        frame(&ctx, &mut studio, t, far, Vec::new());
        t += 0.05;
    }

    // Away from it, not onto it: it stays a moment, then goes.
    let on = rest_until_hover(&ctx, &mut studio, &mut t);
    let shown = studio.hovered();
    let away = Pos2::new(on.x + 200.0, on.y + 160.0);
    let left = t;
    frame(&ctx, &mut studio, left, away, Vec::new());
    frame(&ctx, &mut studio, left + 0.1, away, Vec::new());
    assert_eq!(studio.hovered(), shown, "it stays a moment");
    frame(&ctx, &mut studio, left + 0.42, away, Vec::new());
    assert_eq!(studio.hovered(), None, "then it goes");
    t = left + 0.5;

    // Onto it (it stands above the place), scrolling in it: it stays.
    let on = rest_until_hover(&ctx, &mut studio, &mut t);
    let shown = studio.hovered();
    let inside = Pos2::new(on.x + 40.0, on.y - 40.0);
    for i in 0..30 {
        let wheel = Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: Vec2::new(0.0, -40.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
        };
        let more = if i % 3 == 0 { vec![wheel] } else { Vec::new() };
        frame(&ctx, &mut studio, t, inside, more);
        t += 0.05;
    }
    assert_eq!(studio.hovered(), shown, "on the hover, it stays");
}
