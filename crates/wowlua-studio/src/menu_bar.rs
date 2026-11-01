//! macOS's menu bar (decision S013): the application menu, then Studio's
//! menus from its commands.

use std::sync::mpsc;

use eframe::egui;
use muda::{
    Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu,
    accelerator::{Accelerator, Code, Modifiers},
};
use wowlua_studio_app::commands::{Command, Entry, Menu as StudioMenu};

pub struct MenuBar {
    // Kept alive: the system's menu bar refers to it.
    _menu: Menu,
    items: Vec<(MenuId, Command)>,
    events: mpsc::Receiver<MenuEvent>,
}

impl MenuBar {
    pub fn new(ctx: &egui::Context, entries: &[Entry]) -> Self {
        let (menu, items) = build(entries);
        // A menu click wakes Studio up to run it.
        let ctx = ctx.clone();
        // With a handler set, muda sends nothing to its own receiver: the
        // handler forwards the events to Studio's.
        let (send, events) = mpsc::channel();
        MenuEvent::set_event_handler(Some(move |event| {
            let _ = send.send(event);
            ctx.request_repaint();
        }));
        MenuBar {
            _menu: menu,
            items,
            events,
        }
    }

    /// Makes the menus again: the commands changed (a workspace opened or
    /// closed).
    pub fn rebuild(&mut self, entries: &[Entry]) {
        let (menu, items) = build(entries);
        self._menu = menu;
        self.items = items;
    }

    /// The commands picked in the menu bar since the last frame.
    pub fn poll(&self) -> Vec<Command> {
        let mut out = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            if let Some((_, command)) = self.items.iter().find(|(id, _)| id == event.id()) {
                out.push(*command);
            }
        }
        out
    }
}

/// The application menu, Studio's menus and the window menu, set as the
/// application's menu bar.
fn build(entries: &[Entry]) -> (Menu, Vec<(MenuId, Command)>) {
    let menu = Menu::new();
    let mut items = Vec::new();

    let app = Submenu::new("wowlua studio", true);
    let about = MenuItem::new("about wowlua studio", true, None);
    items.push((about.id().clone(), Command::About));
    // Settings is a page of a workspace's window.
    let has_settings = entries.iter().any(|e| e.command == Command::CloseFolder);
    let settings = MenuItem::new(
        "settings…",
        has_settings,
        Some(Accelerator::new(Modifiers::META, Code::Comma)),
    );
    items.push((
        settings.id().clone(),
        Command::ShowPage(wowlua_studio_app::pages::Page::Settings),
    ));
    let _ = app.append_items(&[
        &about,
        &PredefinedMenuItem::separator(),
        &settings,
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::services(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::hide(None),
        &PredefinedMenuItem::hide_others(None),
        &PredefinedMenuItem::show_all(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::quit(None),
    ]);
    let _ = menu.append(&app);

    for studio_menu in StudioMenu::ALL {
        let in_menu: Vec<&Entry> = entries
            .iter()
            .filter(|e| e.menu == Some(studio_menu))
            .collect();
        if in_menu.is_empty() {
            continue;
        }
        let sub = Submenu::new(studio_menu.label(), true);
        for e in in_menu {
            let item = MenuItem::new(&e.title, true, e.key.as_ref().and_then(accelerator));
            items.push((item.id().clone(), e.command));
            let _ = sub.append(&item);
        }
        let _ = menu.append(&sub);
    }
    let window = Submenu::new("window", true);
    let _ = window.append_items(&[
        &PredefinedMenuItem::minimize(None),
        &PredefinedMenuItem::fullscreen(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::bring_all_to_front(None),
    ]);
    let _ = menu.append(&window);
    menu.init_for_nsapp();
    (menu, items)
}

/// The menu bar's form of a shortcut (letters only so far).
fn accelerator(key: &egui::KeyboardShortcut) -> Option<Accelerator> {
    let m = key.modifiers;
    let mut mods = Modifiers::empty();
    if m.command || m.mac_cmd {
        mods |= Modifiers::META;
    }
    if m.shift {
        mods |= Modifiers::SHIFT;
    }
    if m.alt {
        mods |= Modifiers::ALT;
    }
    let code = match key.logical_key {
        egui::Key::K => Code::KeyK,
        egui::Key::M => Code::KeyM,
        egui::Key::N => Code::KeyN,
        egui::Key::O => Code::KeyO,
        egui::Key::S => Code::KeyS,
        egui::Key::W => Code::KeyW,
        _ => return None,
    };
    Some(Accelerator::new(mods, code))
}
