//! Opening workspaces (the roadmap's M2): a folder opens in the window and
//! is kept in the recent list; a folder with no `wowlua.json` offers to
//! adopt it (S032); closing the folder brings the start page back.

mod common;

use std::path::Path;

use egui::{Pos2, RawInput, Rect, Vec2};
use wowlua_studio_app::{
    Studio,
    commands::{Command, Menu},
};

fn frame(ctx: &egui::Context, studio: &mut Studio) {
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 900.0))),
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| studio.ui(ui));
    // No renderer takes the textures here.
    output.textures_delta.clear();
}

fn has(studio: &Studio, command: Command) -> bool {
    studio.entries().iter().any(|e| e.command == command)
}

#[test]
fn an_opened_folder_is_the_windows_workspace_and_a_recent_one() {
    let ctx = egui::Context::default();
    let (mut studio, files) = common::studio(&ctx);
    frame(&ctx, &mut studio);
    assert!(
        !has(&studio, Command::CloseFolder),
        "the start page has nothing to close"
    );

    studio.open(Path::new("/p/suuui"));
    frame(&ctx, &mut studio);
    assert!(has(&studio, Command::CloseFolder));
    assert!(
        studio.entries().iter().any(|e| e.menu == Some(Menu::Go)),
        "a workspace's window has its pages"
    );
    assert!(studio.take_menus_changed(), "the menu bar is made again");
    let recents = files
        .0
        .borrow()
        .get("recents.json")
        .cloned()
        .unwrap_or_default();
    assert!(recents.contains("/p/suuui"), "{recents}");
    assert!(recents.contains("foreverlfg"), "{recents}");

    // A file opens in the editor and can be closed.
    studio.edit(Path::new("/p/suuui/src/main.wowlua"));
    frame(&ctx, &mut studio);
    frame(&ctx, &mut studio);
    studio.queue(Command::CloseEditor);
    frame(&ctx, &mut studio);

    studio.queue(Command::CloseFolder);
    frame(&ctx, &mut studio);
    frame(&ctx, &mut studio);
    assert!(!has(&studio, Command::CloseFolder));
}

#[test]
fn a_dot_typed_reaches_the_language_server_before_the_completion_it_asks() {
    let ctx = egui::Context::default();
    let (mut studio, sent) = common::studio_recording(&ctx);
    studio.open(Path::new("/p/suuui"));
    frame(&ctx, &mut studio);
    studio.edit(Path::new("/p/suuui/src/main.wowlua"));
    frame(&ctx, &mut studio);
    frame(&ctx, &mut studio);
    // `.` typed at the end of the file's text (`local x = 1`).
    let input = |events| RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 900.0))),
        events,
        ..Default::default()
    };
    for key in [egui::Key::ArrowDown, egui::Key::End] {
        let press = egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let _ = ctx.run_ui(input(vec![press]), |ui| studio.ui(ui));
    }
    let _ = ctx.run_ui(input(vec![egui::Event::Text(".".into())]), |ui| {
        studio.ui(ui)
    });
    let sent = sent.0.borrow();
    let asked = sent
        .iter()
        .position(|m| m["method"] == "textDocument/completion")
        .expect("completion asked for");
    let changed = sent
        .iter()
        .position(|m| {
            m["method"] == "textDocument/didChange"
                && m["params"]["contentChanges"][0]["text"]
                    .as_str()
                    .is_some_and(|t| t.contains("local x = 1."))
        })
        .expect("the dot sent");
    assert!(changed < asked, "{sent:#?}");
}

#[test]
fn a_folder_without_a_manifest_is_offered_to_adopt() {
    let ctx = egui::Context::default();
    let (mut studio, files) = common::studio(&ctx);
    studio.open(Path::new("/p/plain"));
    frame(&ctx, &mut studio);
    frame(&ctx, &mut studio);
    assert!(!has(&studio, Command::CloseFolder), "no workspace opened");
    assert!(!files.0.borrow().contains_key("recents.json"));
}
