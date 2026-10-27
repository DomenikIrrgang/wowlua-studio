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

    studio.queue(Command::CloseFolder);
    frame(&ctx, &mut studio);
    frame(&ctx, &mut studio);
    assert!(!has(&studio, Command::CloseFolder));
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
