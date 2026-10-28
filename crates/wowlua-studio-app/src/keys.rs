//! Keybindings (M10, S062): every command a key starts (the palette's and
//! the editor's), its keys (its own, or those recorded in the settings),
//! clashes, and the settings page's table to record new keys in.

use egui::{Key, KeyboardShortcut, Modifiers, Ui};
use wowlua_studio_ui::forms::{self, CalloutKind};
use wowlua_studio_ui::{Face, palette};

use crate::Platform;
use crate::commands::{self, Command};
use crate::state::Settings;

/// A command a key starts.
#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    /// Its name in the settings: the palette command's (`RunAllTests`),
    /// or the editor's (`editor.rename`).
    pub name: String,
    pub title: String,
    /// Where it works: `anywhere`, `editor`.
    pub when: &'static str,
    /// Its own keys.
    pub default: Option<KeyboardShortcut>,
}

/// The editor's own keys (S062: bindable as the commands are).
pub const EDITOR: &[(&str, &str)] = &[
    ("editor.complete", "completion"),
    ("editor.fixes", "quick fix"),
    ("editor.definition", "go to definition"),
    ("editor.references", "find references"),
    ("editor.rename", "rename"),
];

/// An editor binding's own keys.
pub fn editor_default(name: &str) -> Option<KeyboardShortcut> {
    Some(match name {
        "editor.complete" => KeyboardShortcut::new(Modifiers::CTRL, Key::Space),
        "editor.fixes" => KeyboardShortcut::new(Modifiers::COMMAND, Key::Period),
        "editor.definition" => KeyboardShortcut::new(Modifiers::NONE, Key::F12),
        "editor.references" => KeyboardShortcut::new(Modifiers::SHIFT, Key::F12),
        "editor.rename" => KeyboardShortcut::new(Modifiers::NONE, Key::F2),
        _ => return None,
    })
}

/// A palette command's name in the settings.
pub fn name_of(command: &Command) -> String {
    format!("{command:?}")
}

/// Every command a key can start: the palette's with a workspace open,
/// then the editor's.
pub fn bindings(platform: Platform) -> Vec<Binding> {
    let mut out: Vec<Binding> = Vec::new();
    for e in commands::entries(platform, true, false, true) {
        let name = name_of(&e.command);
        if out.iter().any(|b| b.name == name) {
            continue;
        }
        out.push(Binding {
            name,
            title: e.title.clone(),
            when: "anywhere",
            default: e.key,
        });
    }
    for (name, title) in EDITOR {
        out.push(Binding {
            name: (*name).to_owned(),
            title: (*title).to_owned(),
            when: "editor",
            default: editor_default(name),
        });
    }
    out
}

/// A shortcut as the settings keep it: `cmd+shift+m` (`cmd`: ⌘ on macOS,
/// ctrl elsewhere).
pub fn write(k: &KeyboardShortcut) -> String {
    let m = k.modifiers;
    let mut parts: Vec<&str> = Vec::new();
    if m.command || m.mac_cmd {
        parts.push("cmd");
    }
    if m.ctrl && !m.command {
        parts.push("ctrl");
    }
    if m.alt {
        parts.push("alt");
    }
    if m.shift {
        parts.push("shift");
    }
    let mut out = parts.join("+");
    if !out.is_empty() {
        out.push('+');
    }
    out.push_str(&k.logical_key.name().to_lowercase());
    out
}

/// A shortcut the settings keep.
pub fn parse(text: &str) -> Option<KeyboardShortcut> {
    let mut modifiers = Modifiers::NONE;
    let mut key = None;
    for part in text.split('+').map(str::trim) {
        match part.to_lowercase().as_str() {
            "cmd" | "command" => modifiers = modifiers | Modifiers::COMMAND,
            "ctrl" | "control" => modifiers = modifiers | Modifiers::CTRL,
            "alt" | "option" => modifiers = modifiers | Modifiers::ALT,
            "shift" => modifiers = modifiers | Modifiers::SHIFT,
            other => {
                key = Key::from_name(other).or_else(|| {
                    let mut c = other.chars();
                    let first = c.next()?.to_uppercase().collect::<String>();
                    Key::from_name(&(first + c.as_str()))
                });
            }
        }
    }
    Some(KeyboardShortcut::new(modifiers, key?))
}

/// A binding's keys: those recorded in the settings, else its own.
pub fn key_of(
    settings: &Settings,
    name: &str,
    default: Option<KeyboardShortcut>,
) -> Option<KeyboardShortcut> {
    match settings.keys.get(name) {
        Some(keys) => keys.first().and_then(|k| parse(k)),
        None => default,
    }
}

/// Whether a pressed key is a binding's (the editor's, read in its keys).
pub fn pressed(settings: &Settings, name: &str, key: Key, modifiers: Modifiers) -> bool {
    key_of(settings, name, editor_default(name)).is_some_and(|k| {
        k.logical_key == key
            && k.modifiers.shift == modifiers.shift
            && k.modifiers.alt == modifiers.alt
            && (k.modifiers.command == modifiers.command
                || (k.modifiers.ctrl && modifiers.ctrl && !k.modifiers.command))
    })
}

/// Bindings with the same keys where both work: their titles and the keys.
pub fn clashes(list: &[Binding], settings: &Settings) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let keys: Vec<Option<String>> = list
        .iter()
        .map(|b| key_of(settings, &b.name, b.default).map(|k| write(&k)))
        .collect();
    for i in 0..list.len() {
        for j in i + 1..list.len() {
            let both = list[i].when == "anywhere"
                || list[j].when == "anywhere"
                || list[i].when == list[j].when;
            if both && keys[i].is_some() && keys[i] == keys[j] {
                out.push((
                    list[i].title.clone(),
                    list[j].title.clone(),
                    keys[i].clone().unwrap_or_default(),
                ));
            }
        }
    }
    out
}

/// The keybindings section: each binding with its keys; a double click
/// records new keys (escape stops). Returns whether the settings changed.
pub fn section(
    ui: &mut Ui,
    settings: &mut Settings,
    platform: Platform,
    recording: &mut Option<String>,
) -> bool {
    let p = palette(ui.ctx());
    let list = bindings(platform);
    let mut changed = false;
    // A key pressed while recording is the binding's new key.
    if let Some(name) = recording.clone() {
        let got = ui.input_mut(|i| {
            let mut got = None;
            i.events.retain(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if got.is_none() => {
                    got = Some((*key, *modifiers));
                    false
                }
                egui::Event::Text(_) | egui::Event::Key { .. } => false,
                _ => true,
            });
            got
        });
        if let Some((key, modifiers)) = got {
            if key != Key::Escape {
                let k = KeyboardShortcut::new(modifiers, key);
                settings.keys.insert(name, vec![write(&k)]);
                changed = true;
            }
            *recording = None;
        }
    }
    for (a, b, keys) in clashes(&list, settings) {
        forms::callout(ui, CalloutKind::Warning, |ui| {
            forms::paragraph(
                ui,
                &format!("clash: {keys} is both \"{a}\" and \"{b}\". Change one of them."),
                12.5,
                19.0,
                p.ink,
            );
        });
    }
    egui::Grid::new("keybindings")
        .num_columns(4)
        .spacing([24.0, 8.0])
        .show(ui, |ui| {
            for h in ["command", "keys", "when", ""] {
                ui.label(
                    egui::RichText::new(h)
                        .font(Face::PlexMedium.at(11.0))
                        .color(p.ink_muted),
                );
            }
            ui.end_row();
            for b in &list {
                let resp = ui.add(
                    egui::Label::new(
                        egui::RichText::new(&b.title)
                            .font(Face::Plex.at(12.5))
                            .color(p.ink),
                    )
                    .sense(egui::Sense::click()),
                );
                let keys = if recording.as_deref() == Some(b.name.as_str()) {
                    "press the keys… (escape stops)".to_owned()
                } else {
                    key_of(settings, &b.name, b.default)
                        .map(|k| commands::key_words(&k, platform).join(" "))
                        .unwrap_or_else(|| "—".into())
                };
                let keys_resp = ui.add(
                    egui::Label::new(
                        egui::RichText::new(keys)
                            .font(Face::Plex.at(12.0))
                            .color(p.ink),
                    )
                    .sense(egui::Sense::click()),
                );
                if resp.double_clicked() || keys_resp.double_clicked() {
                    *recording = Some(b.name.clone());
                }
                ui.label(
                    egui::RichText::new(b.when)
                        .font(Face::Plex.at(11.5))
                        .color(p.ink_muted),
                );
                if settings.keys.contains_key(&b.name) {
                    if forms::text_button(ui, "reset").clicked() {
                        settings.keys.remove(&b.name);
                        changed = true;
                    }
                } else {
                    ui.label("");
                }
                ui.end_row();
            }
        });
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new("Double-click a row to record new keys; a changed row shows a reset. Kept in Studio's data on this computer.")
                .font(Face::Plex.at(11.5))
                .color(p.ink_muted),
        );
        if !settings.keys.is_empty() && forms::text_button(ui, "reset every key").clicked() {
            settings.keys.clear();
            changed = true;
        }
    });
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_written_and_read_back() {
        for text in [
            "cmd+shift+m",
            "f12",
            "shift+f12",
            "ctrl+space",
            "cmd+period",
            "alt+f",
        ] {
            let k = parse(text).unwrap_or_else(|| panic!("{text}"));
            assert_eq!(write(&k), text);
        }
        assert!(parse("cmd+nothing").is_none());
    }

    #[test]
    fn recorded_keys_replace_a_command_s_own_and_clash() {
        let mut s = Settings::default();
        let list = bindings(Platform::MacOs);
        assert!(clashes(&list, &s).is_empty(), "{:?}", clashes(&list, &s));
        let rename = list.iter().find(|b| b.name == "editor.rename").unwrap();
        assert_eq!(
            key_of(&s, &rename.name, rename.default)
                .map(|k| write(&k))
                .as_deref(),
            Some("f2")
        );
        assert!(pressed(&s, "editor.rename", Key::F2, Modifiers::NONE));
        s.keys.insert("editor.rename".into(), vec!["f12".into()]);
        assert!(!pressed(&s, "editor.rename", Key::F2, Modifiers::NONE));
        assert!(pressed(&s, "editor.rename", Key::F12, Modifiers::NONE));
        let found = clashes(&list, &s);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].2, "f12");
    }
}
