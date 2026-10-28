//! The parts' forms (S060): the designed ones (character, units, auras,
//! spells, action bars, equipment, bags, SavedVariables) and the one
//! built from the schema for the others. A form reads the profile's JSON
//! and makes changes; it never writes the file itself.

use std::collections::HashMap;

use egui::{Align2, CornerRadius, Rect, Sense, Stroke, Ui, Vec2};
use serde_json::{Value as Json, json};
use wowlua_studio_ui::forms;
use wowlua_studio_ui::widgets::{self, ButtonKind};
use wowlua_studio_ui::{Face, palette};

use super::json::Key;
use super::schema::{self, Form, Kind};
use super::{Action, ArtCache, Change, list};
use crate::system::{Art, System};

/// What a form works with.
pub struct Cx<'a> {
    pub root: &'a Json,
    pub changes: &'a mut Vec<Change>,
    pub buffers: &'a mut HashMap<String, String>,
    pub unit: &'a mut Option<usize>,
    pub entry: &'a mut usize,
    pub slot: &'a mut usize,
    pub bag: &'a mut usize,
    pub look: Look<'a>,
    pub system: &'a dyn System,
    /// The spells part's search: what is typed, the spells found.
    pub spell_query: &'a mut String,
    pub spells: Option<&'a (String, Vec<crate::system::SpellRow>)>,
    pub class_spells: Option<&'a (String, Vec<crate::system::ClassSpell>)>,
    pub all_ranks: &'a mut bool,
}

/// Where the installation's art comes from.
#[derive(Clone, Copy)]
pub struct Look<'a> {
    pub art: &'a ArtCache,
    pub system: &'a dyn System,
    pub flavor: &'a str,
}

/// Art to draw: the stand-in's label, the texture once read.
pub struct Pic {
    label: String,
    tex: Option<egui::TextureId>,
}

impl Look<'_> {
    fn pic(&self, label: &str, art: Option<Art>) -> Pic {
        Pic {
            label: label.to_string(),
            tex: art.and_then(|a| self.art.get(self.system, self.flavor, a)),
        }
    }
}

/// A class's art, by file name.
fn class_art(class: &str) -> Option<Art> {
    (!class.is_empty()).then(|| Art::Class(class.to_uppercase()))
}

/// An action's art: its spell's or item's icon.
fn action_art(a: &Json) -> Option<Art> {
    let id = a["id"].as_u64()? as u32;
    match a["type"].as_str()? {
        "spell" => Some(Art::Spell(id)),
        "item" => Some(Art::Item(id)),
        _ => None,
    }
}

/// An item reference's art (an ID, or a stack with `id`).
fn item_art(v: &Json) -> Option<Art> {
    let id = v.as_u64().or_else(|| v["id"].as_u64())?;
    Some(Art::Item(id as u32))
}

impl Cx<'_> {
    fn set(&mut self, path: Vec<Key>, value: Option<Json>) {
        self.changes.push(Change {
            path,
            value,
            typing: false,
        });
    }
}

/// The value at `path`.
fn at<'a>(root: &'a Json, path: &[Key]) -> Option<&'a Json> {
    let mut v = root;
    for k in path {
        v = match k {
            Key::Name(n) => v.get(n)?,
            Key::Index(i) => v.get(*i)?,
        };
    }
    Some(v)
}

fn name(k: &str) -> Key {
    Key::Name(k.to_string())
}

fn path_of(parts: &[Key]) -> String {
    parts
        .iter()
        .map(|k| match k {
            Key::Name(n) => n.clone(),
            Key::Index(i) => i.to_string(),
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// A value as a field shows it.
fn shown(v: Option<&Json>) -> String {
    match v {
        None | Some(Json::Null) => String::new(),
        Some(Json::String(s)) => s.clone(),
        Some(Json::Array(a)) if a.iter().all(Json::is_number) => a
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(", "),
        Some(other) => other.to_string(),
    }
}

/// A number as JSON: whole numbers as integers.
fn number(n: f64) -> Json {
    if n.fract() == 0.0 && n.abs() < 9.0e15 {
        json!(n as i64)
    } else {
        json!(n)
    }
}

/// What typed text means for a kind of field; none: removed. `Err`: not
/// that kind yet (JSON being typed), nothing changes.
fn parse(kind: &Kind, text: &str) -> Result<Option<Json>, ()> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(None);
    }
    Ok(Some(match kind {
        Kind::Text => Json::String(text.to_string()),
        // Not a number: written as typed (E4401 says so).
        Kind::Number => t
            .parse::<f64>()
            .map(number)
            .unwrap_or(Json::String(t.to_string())),
        Kind::Bool => Json::Bool(t == "true"),
        Kind::Scalar => match t {
            "true" => Json::Bool(true),
            "false" => Json::Bool(false),
            _ => t
                .parse::<f64>()
                .map(number)
                .unwrap_or(Json::String(text.to_string())),
        },
        Kind::Numbers => Json::Array(
            t.split(',')
                .filter_map(|n| n.trim().parse::<f64>().ok())
                .map(number)
                .collect(),
        ),
        Kind::Json => serde_json::from_str(t).map_err(|_| ())?,
    }))
}

/// A labelled field of `kind` at `path`.
fn input(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    label: &str,
    path: Vec<Key>,
    kind: Kind,
    width: f32,
    hint: &str,
) {
    ui.vertical(|ui| {
        ui.set_width(width);
        forms::label(ui, label);
        field(ui, cx, path, kind, width, hint);
    });
}

/// A field without its label.
fn field(ui: &mut Ui, cx: &mut Cx<'_>, path: Vec<Key>, kind: Kind, width: f32, hint: &str) {
    if kind == Kind::Bool {
        let mut on = at(cx.root, &path).and_then(Json::as_bool).unwrap_or(false);
        if forms::checkbox(ui, &mut on, "", None) {
            cx.set(path, on.then_some(Json::Bool(true)));
        }
        return;
    }
    let now = shown(at(cx.root, &path));
    let key = path_of(&path);
    let buffer = cx.buffers.entry(key).or_insert_with(|| now.clone());
    let response = if kind == Kind::Json {
        let p = palette(ui.ctx());
        ui.add(
            egui::TextEdit::multiline(buffer)
                .font(Face::Plex.at(12.0))
                .desired_width(width)
                .desired_rows(2)
                .text_color(p.ink)
                .hint_text(hint),
        )
    } else {
        forms::text_input(ui, buffer, width, hint)
    };
    if response.changed() {
        if let Ok(value) = parse(&kind, buffer) {
            cx.changes.push(Change {
                path,
                value,
                typing: true,
            });
        }
    } else if !response.has_focus() {
        *buffer = now;
    }
}

/// A choice among `options` (`(none)` removes the value).
fn choice(ui: &mut Ui, cx: &mut Cx<'_>, label: &str, path: Vec<Key>, options: &[&str], width: f32) {
    ui.vertical(|ui| {
        ui.set_width(width);
        forms::label(ui, label);
        let now = shown(at(cx.root, &path));
        let mut list: Vec<String> = vec!["(none)".into()];
        list.extend(options.iter().map(|s| s.to_string()));
        if !now.is_empty() && !list.contains(&now) {
            list.push(now.clone());
        }
        let mut index = list.iter().position(|o| o == &now).unwrap_or(0);
        let opts: Vec<(&str, bool)> = list.iter().map(|o| (o.as_str(), true)).collect();
        if forms::select(ui, &path_of(&path), &opts, &mut index, width) {
            let v = (index > 0).then(|| {
                let s = &list[index];
                s.split(' ')
                    .next()
                    .and_then(|n| n.parse::<f64>().ok())
                    .filter(|_| s.contains(" · "))
                    .map(number)
                    .unwrap_or_else(|| Json::String(s.clone()))
            });
            cx.set(path, v);
        }
    });
}

/// A check box at `path`; `default` when the profile doesn't say.
fn check(ui: &mut Ui, cx: &mut Cx<'_>, label: &str, path: Vec<Key>, default: bool) {
    let mut on = at(cx.root, &path)
        .and_then(Json::as_bool)
        .unwrap_or(default);
    if forms::checkbox(ui, &mut on, label, None) {
        cx.set(path, (on != default).then_some(Json::Bool(on)));
    }
}

/// A label in a fixed width, at its left.
fn cell(ui: &mut Ui, width: f32, text: egui::RichText) {
    ui.allocate_ui_with_layout(
        Vec2::new(width, 22.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_width(width);
            ui.label(text);
        },
    );
}

/// The width of a field in a row of `n`.
fn col(ui: &Ui, n: usize) -> f32 {
    ((ui.available_width() - 12.0 * (n as f32 - 1.0)) / n as f32).max(90.0)
}

fn caption(ui: &mut Ui, text: &str) {
    let p = palette(ui.ctx());
    ui.add_space(6.0);
    forms::paragraph(ui, text, 11.0, 16.0, p.ink_faint);
}

/// Game art: in Studio the installation's icon; here the design's
/// stand-in, a hatched box with what it shows.
fn art(
    ui: &mut Ui,
    size: f32,
    label: &str,
    selected: bool,
    tex: Option<egui::TextureId>,
) -> egui::Response {
    let p = palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let painter = ui.painter();
    if let Some(tex) = tex {
        painter.image(
            tex,
            rect,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        let stroke = if selected {
            Stroke::new(2.0, p.amber)
        } else {
            Stroke::new(1.0, p.line_strong)
        };
        painter.rect_stroke(rect, CornerRadius::ZERO, stroke, egui::StrokeKind::Inside);
        return response;
    }
    painter.rect_filled(rect, CornerRadius::ZERO, p.bg_300);
    let mut x = rect.left() - rect.height();
    while x < rect.right() {
        painter.line_segment(
            [
                egui::pos2(x.max(rect.left()), rect.bottom() - (x.max(rect.left()) - x)),
                egui::pos2(
                    (x + rect.height()).min(rect.right()),
                    rect.bottom() - ((x + rect.height()).min(rect.right()) - x),
                ),
            ],
            Stroke::new(1.0, p.bg_200),
        );
        x += 6.0;
    }
    let stroke = if selected {
        Stroke::new(2.0, p.amber)
    } else {
        Stroke::new(1.0, p.line_strong)
    };
    painter.rect_stroke(rect, CornerRadius::ZERO, stroke, egui::StrokeKind::Inside);
    painter.text(
        rect.left_bottom() + Vec2::new(2.0, -2.0),
        Align2::LEFT_BOTTOM,
        label,
        Face::Plex.at((size / 4.5).clamp(7.0, 9.0)),
        p.ink_muted,
    );
    response
}

/// A slot or tile: art, a name and a line under it.
fn tile(
    ui: &mut Ui,
    width: f32,
    pic: &Pic,
    name: &str,
    sub: &str,
    selected: bool,
    empty: bool,
) -> bool {
    let p = palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 86.0), Sense::click());
    let painter = ui.painter().clone();
    painter.rect_filled(
        rect,
        CornerRadius::ZERO,
        if response.hovered() {
            p.bg_300
        } else {
            p.bg_100
        },
    );
    let stroke = if selected {
        Stroke::new(1.0, p.amber_ink)
    } else {
        Stroke::new(1.0, p.line)
    };
    if empty {
        widgets::dashed_rect(ui, rect, p.line_strong);
    } else {
        painter.rect_stroke(rect, CornerRadius::ZERO, stroke, egui::StrokeKind::Inside);
    }
    let icon = Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + 24.0),
        Vec2::splat(32.0),
    );
    if !empty {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(icon));
        art(&mut child, 32.0, &pic.label, selected, pic.tex);
    }
    let fit = |s: &str, n: usize| -> String {
        if s.chars().count() > n {
            s.chars().take(n - 1).collect::<String>() + "…"
        } else {
            s.to_string()
        }
    };
    let chars = (width / 7.0) as usize;
    painter.text(
        egui::pos2(rect.center().x, rect.top() + 52.0),
        Align2::CENTER_CENTER,
        fit(name, chars),
        Face::Plex.at(11.0),
        if empty { p.ink_muted } else { p.ink },
    );
    painter.text(
        egui::pos2(rect.center().x, rect.top() + 68.0),
        Align2::CENTER_CENTER,
        fit(sub, chars),
        Face::Plex.at(10.0),
        p.ink_muted,
    );
    response.clicked()
}

// ------------------------------------------------------------ designed --

/// A designed part's form.
pub fn designed(ui: &mut Ui, cx: &mut Cx<'_>, part: &str) -> Option<Action> {
    match part {
        "character" => character(ui, cx),
        "units" => units(ui, cx),
        "auras" => auras(ui, cx),
        "spells" => spells(ui, cx),
        "actions" => actions(ui, cx),
        "equipment" => equipment(ui, cx),
        "bags" => bags(ui, cx),
        "savedVariables" => return saved_variables(ui, cx),
        _ => {}
    }
    None
}

/// The classes a flavor has, by file name and short name.
pub fn classes(flavor: &str) -> Vec<(&'static str, &'static str)> {
    let mut out = vec![
        ("WARRIOR", "WAR"),
        ("PALADIN", "PAL"),
        ("HUNTER", "HUN"),
        ("ROGUE", "ROG"),
        ("PRIEST", "PRI"),
        ("SHAMAN", "SHA"),
        ("MAGE", "MAG"),
        ("WARLOCK", "LOC"),
        ("DRUID", "DRU"),
    ];
    let later = |from: &[&str]| from.contains(&flavor) || flavor.is_empty();
    if later(&["wrath", "cata", "mists", "standard"]) {
        out.push(("DEATHKNIGHT", "DK"));
    }
    if later(&["mists", "standard"]) {
        out.push(("MONK", "MNK"));
    }
    if later(&["standard"]) {
        out.push(("DEMONHUNTER", "DH"));
        out.push(("EVOKER", "EVO"));
    }
    out
}

/// The races a flavor has, by file name.
pub fn races(flavor: &str) -> Vec<&'static str> {
    let mut out = vec![
        "Human", "Dwarf", "NightElf", "Gnome", "Orc", "Scourge", "Tauren", "Troll",
    ];
    let later = |from: &[&str]| from.contains(&flavor) || flavor.is_empty();
    if later(&["tbc", "wrath", "cata", "mists", "standard"]) {
        out.extend(["BloodElf", "Draenei"]);
    }
    if later(&["cata", "mists", "standard"]) {
        out.extend(["Worgen", "Goblin"]);
    }
    if later(&["mists", "standard"]) {
        out.push("Pandaren");
    }
    if later(&["standard"]) {
        out.extend([
            "Nightborne",
            "HighmountainTauren",
            "VoidElf",
            "LightforgedDraenei",
            "ZandalariTroll",
            "KulTiran",
            "DarkIronDwarf",
            "Vulpera",
            "MagharOrc",
            "Mechagnome",
            "Dracthyr",
            "EarthenDwarf",
        ]);
    }
    out
}

const POWER: &[&str] = &[
    "0 · mana",
    "1 · rage",
    "2 · focus",
    "3 · energy",
    "6 · runic power",
];
const SEX: &[&str] = &["2 · male", "3 · female", "1 · unknown"];
const ROLES: &[&str] = &["TANK", "HEALER", "DAMAGER", "NONE"];

/// The key a unit's maximum is under (`maxHealth` as recorded, else the
/// declared `healthMax`).
fn max_key(data: Option<&Json>, recorded: &str, declared: &str) -> String {
    match data {
        Some(d) if d.get(declared).is_some() && d.get(recorded).is_none() => declared.into(),
        _ => recorded.into(),
    }
}

fn character(ui: &mut Ui, cx: &mut Cx<'_>) {
    let base = vec![name("character")];
    let p = |f: &str| {
        let mut v = base.clone();
        v.push(name(f));
        v
    };
    let data = cx.root.get("character").cloned();
    let class = data
        .as_ref()
        .and_then(|d| d["class"].as_str())
        .unwrap_or("")
        .to_string();
    let look = cx.look;
    ui.horizontal(|ui| {
        let pic = look.pic(
            if class.is_empty() { "class" } else { &class },
            class_art(&class),
        );
        art(ui, 52.0, &pic.label, false, pic.tex);
        ui.add_space(12.0);
        let w = col(ui, 4);
        input(ui, cx, "name", p("name"), Kind::Text, w, "Player");
        input(
            ui,
            cx,
            "last name",
            p("lastName"),
            Kind::Text,
            w,
            "WoW Forever only",
        );
        input(ui, cx, "realm", p("realm"), Kind::Text, w, "Realm");
        input(ui, cx, "level", p("level"), Kind::Number, w, "1");
    });
    ui.add_space(10.0);
    forms::label(ui, "class");
    let list = classes(look.flavor);
    let per = list.len().min(9);
    let w = ((ui.available_width() - 6.0 * (per as f32 - 1.0)) / per as f32).max(60.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
        for (file, short) in &list {
            if tile(
                ui,
                w,
                &look.pic(short, class_art(file)),
                file,
                "",
                class == *file,
                false,
            ) {
                cx.set(p("class"), Some(Json::String((*file).to_string())));
            }
        }
    });
    caption(
        ui,
        "By file name, as the client names classes; the icons are the installation's (CLASS_ICON_TCOORDS on the class sheet).",
    );
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        let w = col(ui, 4);
        let races = races(look.flavor);
        choice(ui, cx, "race", p("race"), &races, w);
        choice(ui, cx, "sex", p("sex"), SEX, w);
        choice(ui, cx, "role", p("role"), ROLES, w);
        choice(ui, cx, "faction", p("faction"), &["Alliance", "Horde"], w);
    });
    ui.add_space(10.0);
    let health_max = max_key(data.as_ref(), "maxHealth", "healthMax");
    let power_max = max_key(data.as_ref(), "maxPower", "powerMax");
    ui.horizontal(|ui| {
        let w = col(ui, 4);
        let half = (w - 8.0) / 2.0;
        ui.vertical(|ui| {
            ui.set_width(w);
            forms::label(ui, "health · max");
            ui.horizontal(|ui| {
                field(ui, cx, p("health"), Kind::Number, half, "100");
                field(ui, cx, p(&health_max), Kind::Number, half, "100");
            });
        });
        ui.vertical(|ui| {
            ui.set_width(w);
            forms::label(ui, "power · max");
            ui.horizontal(|ui| {
                field(ui, cx, p("power"), Kind::Number, half, "0");
                field(ui, cx, p(&power_max), Kind::Number, half, "100");
            });
        });
        choice(ui, cx, "power type", p("powerType"), POWER, w);
        ui.vertical(|ui| {
            check(ui, cx, "dead", p("dead"), false);
            check(ui, cx, "pvp", p("pvp"), false);
        });
    });
    caption(
        ui,
        "Applied as mock.setPlayer. Leave a field empty for the client's default.",
    );
}

/// A unit's place in the profile's `units`.
fn unit_path(key: &Key, f: &str) -> Vec<Key> {
    vec![name("units"), key.clone(), name(f)]
}

fn units(ui: &mut Ui, cx: &mut Cx<'_>) {
    let pal = palette(ui.ctx());
    let root = cx.root;
    let look = cx.look;
    let all: Vec<(Key, Json)> = root
        .get("units")
        .map(|u| list(u).into_iter().map(|(k, v)| (k, v.clone())).collect())
        .unwrap_or_default();
    let grouped: Vec<usize> = (0..all.len())
        .filter(|&i| all[i].1.get("group").is_some())
        .collect();
    let target = (0..all.len()).find(|&i| all[i].1["target"] == Json::Bool(true));
    let raid = grouped.iter().any(|&i| all[i].1["group"] == "raid");
    let kind = if grouped.is_empty() {
        2
    } else if raid {
        1
    } else {
        0
    };
    let next = match &root.get("units") {
        Some(Json::Object(o)) => Key::Name(
            (o.keys()
                .filter_map(|k| k.parse::<usize>().ok())
                .max()
                .unwrap_or(0)
                + 1)
            .to_string(),
        ),
        _ => Key::Index(all.len()),
    };
    let add = |cx: &mut Cx<'_>, data: Json| {
        if root.get("units").is_none() {
            cx.set(vec![name("units")], Some(Json::Array(vec![data])));
        } else {
            cx.set(vec![name("units"), next.clone()], Some(data));
        }
    };
    ui.horizontal(|ui| {
        if let Some(k) = widgets::seg(ui, "world-group", &["party", "raid", "no group"], kind, true) {
            match k {
                2 => {
                    for &i in &grouped {
                        cx.set(unit_path(&all[i].0, "group"), None);
                    }
                }
                _ => {
                    let word = if k == 1 { "raid" } else { "party" };
                    if grouped.is_empty() {
                        add(cx, json!({ "name": "Member", "player": true, "class": "PRIEST", "group": word }));
                    }
                    for &i in &grouped {
                        cx.set(unit_path(&all[i].0, "group"), Some(json!(word)));
                    }
                }
            }
        }
        let text = match kind {
            2 => "alone, with a target".to_string(),
            1 => format!("a raid of {}", grouped.len() + 1),
            _ => format!("a party of {}", grouped.len() + 1),
        };
        ui.label(egui::RichText::new(text).font(Face::Plex.at(11.0)).color(pal.ink_muted));
    });
    ui.add_space(8.0);
    let character = root.get("character").cloned().unwrap_or(Json::Null);
    let label = |u: &Json, fallback: &str| -> (String, String) {
        let class = u["class"].as_str().unwrap_or("").to_string();
        let short = class.chars().take(3).collect::<String>();
        let short = if short.is_empty() {
            if u["player"] == Json::Bool(true) {
                "PC".into()
            } else {
                "NPC".into()
            }
        } else {
            short
        };
        (u["name"].as_str().unwrap_or(fallback).to_string(), short)
    };
    forms::card(
        ui,
        "group",
        |ui| {
            ui.label(
                egui::RichText::new("click a slot to edit")
                    .font(Face::Plex.at(11.0))
                    .color(pal.ink_faint),
            );
        },
        |ui| {
            forms::padded(ui, 12, 10, |ui| {
                let w = 92.0;
                let row = |ui: &mut Ui, label: &str, add: &mut dyn FnMut(&mut Ui)| {
                    ui.horizontal(|ui| {
                        cell(
                            ui,
                            52.0,
                            egui::RichText::new(label)
                                .font(Face::Plex.at(11.0))
                                .color(pal.ink_muted),
                        );
                        add(ui);
                    });
                };
                let mut picked: Option<Option<usize>> = None;
                let mut add_member = false;
                let mut add_target = false;
                row(ui, if kind == 2 { "you" } else { "group" }, &mut |ui| {
                    let (n, short) = label(&character, "Player");
                    let pic =
                        look.pic(&short, class_art(character["class"].as_str().unwrap_or("")));
                    if tile(ui, w, &pic, &n, "you", cx.unit.is_none(), false) {
                        picked = Some(None);
                    }
                    if kind != 2 {
                        for &i in grouped.iter().take(if raid { 39 } else { 4 }) {
                            let (n, short) = label(&all[i].1, "Member");
                            let sub = all[i].1["role"].as_str().unwrap_or("member").to_lowercase();
                            let pic = look
                                .pic(&short, class_art(all[i].1["class"].as_str().unwrap_or("")));
                            if tile(ui, w, &pic, &n, &sub, *cx.unit == Some(i), false) {
                                picked = Some(Some(i));
                            }
                        }
                        if grouped.len() < if raid { 39 } else { 4 }
                            && tile(ui, w, &look.pic("", None), "+ member", "empty", false, true)
                        {
                            add_member = true;
                        }
                    }
                });
                ui.add_space(8.0);
                row(ui, "target", &mut |ui| match target {
                    Some(i) => {
                        let (n, short) = label(&all[i].1, "Target");
                        let sub = format!(
                            "{} {}",
                            all[i].1["classification"].as_str().unwrap_or(""),
                            all[i].1["level"]
                                .as_f64()
                                .map(|l| l.to_string())
                                .unwrap_or_default()
                        );
                        let pic =
                            look.pic(&short, class_art(all[i].1["class"].as_str().unwrap_or("")));
                        if tile(ui, w, &pic, &n, sub.trim(), *cx.unit == Some(i), false) {
                            picked = Some(Some(i));
                        }
                    }
                    None => {
                        if tile(ui, w, &look.pic("", None), "+ target", "empty", false, true) {
                            add_target = true;
                        }
                    }
                });
                if let Some(p) = picked {
                    *cx.unit = p;
                }
                if add_member {
                    let word = if raid { "raid" } else { "party" };
                    add(
                        cx,
                        json!({ "name": "Member", "player": true, "class": "PRIEST", "group": word }),
                    );
                    *cx.unit = Some(all.len());
                }
                if add_target {
                    add(
                        cx,
                        json!({ "name": "Target", "reaction": "hostile", "level": 1, "target": true, "nameplate": true, "distance": 6 }),
                    );
                    *cx.unit = Some(all.len());
                }
            });
        },
    );
    ui.add_space(10.0);
    // The picked unit.
    let picked = cx.unit.filter(|&i| i < all.len());
    match picked {
        None => {
            forms::card(
                ui,
                "you · the player",
                |_| {},
                |ui| {
                    forms::padded(ui, 12, 10, |ui| {
                        ui.horizontal(|ui| {
                            let w = col(ui, 4);
                            input(
                                ui,
                                cx,
                                "name",
                                vec![name("character"), name("name")],
                                Kind::Text,
                                w,
                                "Player",
                            );
                            input(
                                ui,
                                cx,
                                "class",
                                vec![name("character"), name("class")],
                                Kind::Text,
                                w,
                                "WARRIOR",
                            );
                            input(
                                ui,
                                cx,
                                "level",
                                vec![name("character"), name("level")],
                                Kind::Number,
                                w,
                                "1",
                            );
                            choice(
                                ui,
                                cx,
                                "role",
                                vec![name("character"), name("role")],
                                ROLES,
                                w,
                            );
                        });
                        caption(
                            ui,
                            "The player's character: the character part has all of it.",
                        );
                    });
                },
            );
        }
        Some(i) => {
            let (key, data) = all[i].clone();
            let title = format!(
                "{} · {}",
                data["name"].as_str().unwrap_or("unit"),
                if data.get("group").is_some() {
                    "group member"
                } else if data["target"] == Json::Bool(true) {
                    "target"
                } else {
                    "around"
                }
            );
            let mut remove = false;
            forms::card(
                ui,
                &title,
                |ui| {
                    if forms::text_button(ui, "remove").clicked() {
                        remove = true;
                    }
                },
                |ui| {
                    forms::padded(ui, 12, 10, |ui| {
                        let health_max = max_key(Some(&data), "maxHealth", "healthMax");
                        ui.horizontal(|ui| {
                            let w = col(ui, 4);
                            input(
                                ui,
                                cx,
                                "name",
                                unit_path(&key, "name"),
                                Kind::Text,
                                w,
                                "Unit",
                            );
                            input(
                                ui,
                                cx,
                                "class",
                                unit_path(&key, "class"),
                                Kind::Text,
                                w,
                                "ROGUE",
                            );
                            input(
                                ui,
                                cx,
                                "level",
                                unit_path(&key, "level"),
                                Kind::Number,
                                w,
                                "1",
                            );
                            choice(ui, cx, "role", unit_path(&key, "role"), ROLES, w);
                        });
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            let w = col(ui, 4);
                            let half = (w - 8.0) / 2.0;
                            choice(
                                ui,
                                cx,
                                "reaction",
                                unit_path(&key, "reaction"),
                                &["hostile", "neutral", "friendly"],
                                w,
                            );
                            input(
                                ui,
                                cx,
                                "distance (yd)",
                                unit_path(&key, "distance"),
                                Kind::Number,
                                w,
                                "unknown",
                            );
                            ui.vertical(|ui| {
                                ui.set_width(w);
                                forms::label(ui, "health · max");
                                ui.horizontal(|ui| {
                                    field(
                                        ui,
                                        cx,
                                        unit_path(&key, "health"),
                                        Kind::Number,
                                        half,
                                        "100",
                                    );
                                    field(
                                        ui,
                                        cx,
                                        unit_path(&key, &health_max),
                                        Kind::Number,
                                        half,
                                        "100",
                                    );
                                });
                            });
                            ui.vertical(|ui| {
                                ui.set_width(w);
                                forms::label(ui, "npc ID · classification");
                                ui.horizontal(|ui| {
                                    field(
                                        ui,
                                        cx,
                                        unit_path(&key, "npcID"),
                                        Kind::Number,
                                        half,
                                        "—",
                                    );
                                    field(
                                        ui,
                                        cx,
                                        unit_path(&key, "classification"),
                                        Kind::Text,
                                        half,
                                        "normal",
                                    );
                                });
                            });
                        });
                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            let player = data["player"] == Json::Bool(true);
                            check(ui, cx, "a player", unit_path(&key, "player"), false);
                            let was_target = data["target"] == Json::Bool(true);
                            let mut on = was_target;
                            if forms::checkbox(ui, &mut on, "your target", None) {
                                // One target: the others' cleared.
                                for (k, u) in &all {
                                    if u["target"] == Json::Bool(true) && k != &key {
                                        cx.set(unit_path(k, "target"), None);
                                    }
                                }
                                cx.set(unit_path(&key, "target"), on.then_some(Json::Bool(true)));
                            }
                            check(ui, cx, "your focus", unit_path(&key, "focus"), false);
                            check(
                                ui,
                                cx,
                                "nameplate shows",
                                unit_path(&key, "nameplate"),
                                false,
                            );
                            check(
                                ui,
                                cx,
                                "accepts invites",
                                unit_path(&key, "acceptsInvites"),
                                player && data.get("group").is_none(),
                            );
                            check(ui, cx, "dead", unit_path(&key, "dead"), false);
                        });
                        ui.add_space(6.0);
                        forms::label(ui, "its auras");
                        ui.horizontal_wrapped(|ui| {
                            let auras = data.get("auras").map(|a| list(a).len()).unwrap_or(0);
                            for (k, a) in data.get("auras").map(list).unwrap_or_default() {
                                let label = format!(
                                    "{} {}",
                                    a["name"].as_str().unwrap_or("aura"),
                                    a["duration"]
                                        .as_f64()
                                        .map(|d| format!("{d} s"))
                                        .unwrap_or_default()
                                );
                                if forms::text_button(ui, &format!("{} ×", label.trim())).clicked()
                                {
                                    let mut path = unit_path(&key, "auras");
                                    path.push(k.clone());
                                    cx.set(path, None);
                                }
                            }
                            if forms::text_button(ui, "+ aura").clicked() {
                                let mut path = unit_path(&key, "auras");
                                if data.get("auras").is_some() {
                                    path.push(Key::Index(auras));
                                    cx.set(path, Some(json!({ "name": "Aura", "duration": 10 })));
                                } else {
                                    cx.set(path, Some(json!([{ "name": "Aura", "duration": 10 }])));
                                }
                            }
                        });
                    });
                },
            );
            if remove {
                cx.set(vec![name("units"), key], None);
                *cx.unit = None;
            }
        }
    }
    ui.add_space(10.0);
    // The others around.
    let others: Vec<usize> = (0..all.len())
        .filter(|&i| all[i].1.get("group").is_none() && Some(i) != target)
        .collect();
    let mut add_other = false;
    forms::card(
        ui,
        "other units around",
        |ui| {
            if forms::text_button(ui, "+ unit").clicked() {
                add_other = true;
            }
        },
        |ui| {
            forms::padded(ui, 12, 8, |ui| {
                if others.is_empty() {
                    ui.label(
                        egui::RichText::new("None.")
                            .font(Face::Plex.at(12.0))
                            .color(pal.ink_muted),
                    );
                }
                for &i in &others {
                    let u = &all[i].1;
                    let text = format!(
                        "{:<22} {:>4}  {:<9} {:>6}  {}",
                        u["name"].as_str().unwrap_or("Unit"),
                        u["level"]
                            .as_f64()
                            .map(|l| l.to_string())
                            .unwrap_or_default(),
                        u["reaction"].as_str().unwrap_or("hostile"),
                        u["distance"]
                            .as_f64()
                            .map(|d| format!("{d} yd"))
                            .unwrap_or_default(),
                        if u["nameplate"] == Json::Bool(true) {
                            "nameplate"
                        } else {
                            "—"
                        }
                    );
                    let selected = *cx.unit == Some(i);
                    if ui
                        .selectable_label(
                            selected,
                            egui::RichText::new(text).font(Face::Plex.at(12.0)),
                        )
                        .clicked()
                    {
                        *cx.unit = Some(i);
                    }
                }
            });
        },
    );
    if add_other {
        add(
            cx,
            json!({ "name": "Unit", "reaction": "hostile", "level": 1, "distance": 20 }),
        );
        *cx.unit = Some(all.len());
    }
}

/// A list's entries as cards; the picked one's fields under them.
fn auras(ui: &mut Ui, cx: &mut Cx<'_>) {
    let all: Vec<(Key, Json)> = cx
        .root
        .get("auras")
        .map(|a| list(a).into_iter().map(|(k, v)| (k, v.clone())).collect())
        .unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(10.0, 10.0);
        for (i, (_, a)) in all.iter().enumerate() {
            let selected = *cx.entry == i;
            let icon = shown(a.get("icon"));
            let kind = if a["harmful"] == Json::Bool(true) {
                "harmful"
            } else {
                "helpful"
            };
            let meta = match a["duration"].as_f64() {
                Some(d) => format!("{d} s"),
                None => "no duration".into(),
            };
            let art = a["icon"]
                .as_u64()
                .map(|f| Art::File(f as u32))
                .or_else(|| a["spellId"].as_u64().map(|s| Art::Spell(s as u32)));
            if card_button(
                ui,
                &cx.look.pic(&icon, art),
                a["name"].as_str().unwrap_or("Aura"),
                &format!("spell {} · {kind}", shown(a.get("spellId"))),
                &meta,
                selected,
            ) {
                *cx.entry = i;
            }
        }
        if dashed_button(ui, "+ aura") {
            let data = json!({ "name": "Aura", "duration": 10 });
            if cx.root.get("auras").is_none() {
                cx.set(vec![name("auras")], Some(Json::Array(vec![data])));
            } else {
                cx.set(vec![name("auras"), Key::Index(all.len())], Some(data));
            }
            *cx.entry = all.len();
        }
    });
    if let Some((key, a)) = all.get(*cx.entry).cloned() {
        ui.add_space(12.0);
        entry_card(
            ui,
            cx,
            &format!("{} · aura", a["name"].as_str().unwrap_or("Aura")),
            vec![name("auras"), key],
            &[
                ("spellId", Kind::Number),
                ("name", Kind::Text),
                ("icon", Kind::Scalar),
                ("duration", Kind::Number),
                ("applications", Kind::Number),
                ("source", Kind::Text),
                ("dispelName", Kind::Text),
                ("harmful", Kind::Bool),
                ("stealable", Kind::Bool),
            ],
        );
    }
}

/// An entry's fields in a card, with its removal.
fn entry_card(ui: &mut Ui, cx: &mut Cx<'_>, title: &str, base: Vec<Key>, fields: &[(&str, Kind)]) {
    let mut remove = false;
    forms::card(
        ui,
        title,
        |ui| {
            if forms::text_button(ui, "remove").clicked() {
                remove = true;
            }
        },
        |ui| {
            forms::padded(ui, 12, 10, |ui| {
                grid(ui, cx, &base, fields);
            });
        },
    );
    if remove {
        cx.set(base, None);
        *cx.entry = 0;
    }
}

/// Fields four to a row, under `base`.
fn grid(ui: &mut Ui, cx: &mut Cx<'_>, base: &[Key], fields: &[(&str, Kind)]) {
    for row in fields.chunks(4) {
        ui.horizontal(|ui| {
            let w = col(ui, 4);
            for (f, kind) in row {
                let mut path = base.to_vec();
                path.push(name(f));
                if *kind == Kind::Bool {
                    ui.vertical(|ui| {
                        ui.set_width(w);
                        forms::label(ui, f);
                        check(ui, cx, "", path, false);
                    });
                } else {
                    input(ui, cx, f, path, kind.clone(), w, "");
                }
            }
        });
        ui.add_space(6.0);
    }
}

/// A card with art, a title and two lines; whether it was clicked.
fn card_button(
    ui: &mut Ui,
    icon: &Pic,
    title: &str,
    line: &str,
    meta: &str,
    selected: bool,
) -> bool {
    let p = palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(Vec2::new(300.0, 62.0), Sense::click());
    let painter = ui.painter().clone();
    painter.rect_filled(
        rect,
        CornerRadius::ZERO,
        if response.hovered() {
            p.bg_300
        } else {
            p.bg_200
        },
    );
    painter.rect_stroke(
        rect,
        CornerRadius::ZERO,
        if selected {
            Stroke::new(1.0, p.amber_ink)
        } else {
            Stroke::new(1.0, p.line)
        },
        egui::StrokeKind::Inside,
    );
    let icon_rect = Rect::from_min_size(rect.min + Vec2::new(10.0, 13.0), Vec2::splat(36.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(icon_rect));
    art(&mut child, 36.0, &icon.label, false, icon.tex);
    let x = icon_rect.right() + 10.0;
    painter.text(
        egui::pos2(x, rect.top() + 14.0),
        Align2::LEFT_CENTER,
        title,
        Face::PlexMedium.at(12.0),
        p.ink,
    );
    painter.text(
        egui::pos2(x, rect.top() + 31.0),
        Align2::LEFT_CENTER,
        line,
        Face::Plex.at(11.0),
        p.ink_muted,
    );
    painter.text(
        egui::pos2(x, rect.top() + 46.0),
        Align2::LEFT_CENTER,
        meta,
        Face::Plex.at(11.0),
        p.ink_muted,
    );
    response.clicked()
}

fn dashed_button(ui: &mut Ui, label: &str) -> bool {
    let p = palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(Vec2::new(300.0, 62.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_300);
    }
    widgets::dashed_rect(ui, rect, p.line_strong);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        Face::Plex.at(11.5),
        p.ink_muted,
    );
    response.clicked()
}

/// The spells part: the class's spells to check as trained (`trained`,
/// wowlua T500), others found by a search, and spells whose data the
/// profile overrides (`spells`).
fn spells(ui: &mut Ui, cx: &mut Cx<'_>) {
    let p = palette(ui.ctx());
    let root = cx.root;
    let trained: Vec<i64> = root
        .get("trained")
        .map(|t| list(t).iter().filter_map(|(_, v)| v.as_i64()).collect())
        .unwrap_or_default();
    let class = root["character"]["class"]
        .as_str()
        .unwrap_or("")
        .to_uppercase();
    let level = root["character"]["level"].as_f64().unwrap_or(1.0) as u32;
    // Trained set or not: the whole list written again (an array).
    let set_trained = |cx: &mut Cx<'_>, id: i64, on: bool| {
        let mut ids = trained.clone();
        ids.retain(|i| *i != id);
        if on {
            ids.push(id);
        }
        cx.set(vec![name("trained")], Some(json!(ids)));
    };

    // The class's spells.
    if class.is_empty() {
        ui.label(
            egui::RichText::new(
                "Give the character a class: its spells show here to check as trained.",
            )
            .font(Face::Plex.at(12.0))
            .color(p.ink_muted),
        );
    } else {
        match cx.class_spells.filter(|(c, _)| *c == class) {
            None => {
                ui.label(
                    egui::RichText::new(format!("Reading {class}'s spells from the installation…"))
                        .font(Face::Plex.at(12.0))
                        .color(p.ink_muted),
                );
            }
            Some((_, list)) if list.is_empty() => {
                ui.label(
                    egui::RichText::new(format!(
                        "No installation with this flavor's client: {class}'s spells come from its tables. Add trained spells by ID below."
                    ))
                    .font(Face::Plex.at(12.0))
                    .color(p.ink_muted),
                );
            }
            Some((_, list)) => {
                let shown: Vec<&crate::system::ClassSpell> = list
                    .iter()
                    .filter(|s| *cx.all_ranks || (s.level > 0 && s.level <= level))
                    .collect();
                // (a spell without a level is learned another way: a
                // talent, an item; shown with every rank)
                let up_to: Vec<&crate::system::ClassSpell> = list
                    .iter()
                    .filter(|s| s.level > 0 && s.level <= level)
                    .collect();
                let count = up_to
                    .iter()
                    .filter(|s| trained.contains(&i64::from(s.id)))
                    .count();
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "{class} at level {level} · {count} of {} trained",
                            up_to.len()
                        ))
                        .font(Face::PlexMedium.at(12.0))
                        .color(p.ink),
                    );
                    if forms::text_button(ui, "train all to its level").clicked() {
                        let mut ids = trained.clone();
                        for s in &up_to {
                            if !ids.contains(&i64::from(s.id)) {
                                ids.push(i64::from(s.id));
                            }
                        }
                        cx.set(vec![name("trained")], Some(json!(ids)));
                    }
                    forms::checkbox(ui, cx.all_ranks, "show all ranks and levels", None);
                });
                ui.add_space(6.0);
                let mut line = "";
                for s in shown {
                    if s.line != line {
                        line = &s.line;
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(line)
                                .font(Face::PlexMedium.at(11.0))
                                .color(p.ink_muted),
                        );
                    }
                    let id = i64::from(s.id);
                    let mut on = trained.contains(&id);
                    let above = s.level > level;
                    ui.horizontal(|ui| {
                        if forms::checkbox(ui, &mut on, "", None) {
                            set_trained(cx, id, on);
                        }
                        let tex = s
                            .icon
                            .and_then(|f| cx.look.art.get(cx.system, cx.look.flavor, Art::File(f)));
                        art(ui, 24.0, "", false, tex);
                        cell(
                            ui,
                            220.0,
                            egui::RichText::new(&s.name)
                                .font(Face::Plex.at(12.0))
                                .color(if above { p.ink_muted } else { p.ink }),
                        );
                        cell(
                            ui,
                            80.0,
                            egui::RichText::new(&s.rank)
                                .font(Face::Plex.at(11.0))
                                .color(p.ink_muted),
                        );
                        cell(
                            ui,
                            70.0,
                            egui::RichText::new(if s.level == 0 {
                                "no level".to_string()
                            } else {
                                format!("level {}", s.level)
                            })
                            .font(Face::Plex.at(11.0))
                            .color(p.ink_muted),
                        );
                        ui.label(
                            egui::RichText::new(s.id.to_string())
                                .font(Face::Plex.at(11.0))
                                .color(p.ink_faint),
                        );
                    });
                }
            }
        }
    }

    // Other trained spells (racials, professions…): found by a search.
    ui.add_space(14.0);
    forms::label(ui, "other spells");
    let class_ids: Vec<i64> = cx
        .class_spells
        .map(|(_, l)| l.iter().map(|s| i64::from(s.id)).collect())
        .unwrap_or_default();
    let others: Vec<i64> = trained
        .iter()
        .copied()
        .filter(|i| !class_ids.contains(i))
        .collect();
    if !others.is_empty() {
        ui.horizontal_wrapped(|ui| {
            for id in &others {
                if forms::text_button(ui, &format!("{id} ×")).clicked() {
                    set_trained(cx, *id, false);
                }
            }
        });
    }
    forms::text_input(
        ui,
        cx.spell_query,
        520.0_f32.min(ui.available_width()),
        "search the client's spells by name or ID (from the installation's tables)",
    );
    if let Some((query, found)) = cx.spells.filter(|(q, _)| !q.trim().is_empty()) {
        ui.add_space(6.0);
        if found.is_empty() {
            ui.label(
                egui::RichText::new(format!(
                    "No spell of the installation's is named like \"{query}\" (or no installation has this flavor's client)."
                ))
                .font(Face::Plex.at(12.0))
                .color(p.ink_muted),
            );
        }
        for s in found {
            let id = i64::from(s.id);
            let is_trained = trained.contains(&id);
            ui.horizontal(|ui| {
                let tex = s
                    .icon
                    .and_then(|f| cx.look.art.get(cx.system, cx.look.flavor, Art::File(f)));
                art(ui, 24.0, "", false, tex);
                cell(
                    ui,
                    220.0,
                    egui::RichText::new(&s.name)
                        .font(Face::Plex.at(12.0))
                        .color(p.ink),
                );
                cell(
                    ui,
                    70.0,
                    egui::RichText::new(s.id.to_string())
                        .font(Face::Plex.at(12.0))
                        .color(p.ink_muted),
                );
                if is_trained {
                    ui.label(
                        egui::RichText::new("trained")
                            .font(Face::Plex.at(11.0))
                            .color(p.ink_faint),
                    );
                } else if forms::text_button(ui, "+ trained").clicked() {
                    set_trained(cx, id, true);
                }
                let text: String = s.text.chars().take(90).collect();
                ui.label(
                    egui::RichText::new(text)
                        .font(Face::Plex.at(11.0))
                        .color(p.ink_muted),
                );
            });
        }
    }

    // Spells whose data the profile overrides.
    let all: Vec<(Key, Json)> = root
        .get("spells")
        .map(|s| list(s).into_iter().map(|(k, v)| (k, v.clone())).collect())
        .unwrap_or_default();
    ui.add_space(14.0);
    forms::label(ui, "spell data overrides");
    let new_key = "spells/+".to_string();
    ui.horizontal(|ui| {
        let buffer = cx.buffers.entry(new_key.clone()).or_default();
        forms::text_input(ui, buffer, 160.0, "spell ID");
        let id = buffer.trim().parse::<u64>().ok();
        if forms::text_button(ui, "+ override").clicked()
            && let Some(id) = id
        {
            let data = json!({ "name": format!("Spell {id}") });
            if root.get("spells").is_none() {
                cx.set(vec![name("spells")], Some(json!({ id.to_string(): data })));
            } else {
                cx.set(vec![name("spells"), name(&id.to_string())], Some(data));
            }
            cx.buffers.insert(new_key.clone(), String::new());
        }
    });
    for (i, (key, s)) in all.iter().enumerate() {
        let id = match key {
            Key::Name(k) => k.clone(),
            Key::Index(i) => i.to_string(),
        };
        let text = format!(
            "{:<26} {:>7}  {}",
            s["name"].as_str().unwrap_or("Spell"),
            id,
            s["description"].as_str().unwrap_or("")
        );
        if ui
            .selectable_label(
                *cx.entry == i,
                egui::RichText::new(text).font(Face::Plex.at(12.0)),
            )
            .clicked()
        {
            *cx.entry = i;
        }
    }
    if let Some((key, s)) = all.get(*cx.entry).cloned() {
        ui.add_space(8.0);
        entry_card(
            ui,
            cx,
            &format!("{} · its data", s["name"].as_str().unwrap_or("Spell")),
            vec![name("spells"), key],
            &[
                ("name", Kind::Text),
                ("iconID", Kind::Number),
                ("castTime", Kind::Number),
                ("description", Kind::Text),
                ("minRange", Kind::Number),
                ("maxRange", Kind::Number),
                ("petKnown", Kind::Bool),
            ],
        );
    }
    caption(
        ui,
        "Trained: the spells the character knows (mock.player:learn). An override replaces what the client's tables answer for a spell (mock.defineSpell); without one the installation's data answers.",
    );
}

/// An action slot's key in the profile (its slot, or an array's place).
fn slot_key(root: &Json, slot: usize) -> Key {
    match root.get("actions") {
        Some(Json::Array(_)) => Key::Index(slot - 1),
        _ => Key::Name(slot.to_string()),
    }
}

fn actions(ui: &mut Ui, cx: &mut Cx<'_>) {
    let p = palette(ui.ctx());
    let root = cx.root;
    let action = |slot: usize| -> Option<Json> {
        at(root, &[name("actions"), slot_key(root, slot)])
            .filter(|v| !v.is_null())
            .cloned()
    };
    let highest = (1..=180).rev().find(|&s| action(s).is_some()).unwrap_or(0);
    let bars = highest.div_ceil(12).max(2);
    let selected = (*cx.slot).max(1);
    for bar in 0..bars {
        ui.label(
            egui::RichText::new(format!(
                "bar {} · slots {}–{}",
                bar + 1,
                bar * 12 + 1,
                bar * 12 + 12
            ))
            .font(Face::Plex.at(11.0))
            .color(p.ink_muted),
        );
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for i in 1..=12 {
                let slot = bar * 12 + i;
                let tex = action(slot)
                    .as_ref()
                    .and_then(action_art)
                    .and_then(|a| cx.look.art.get(cx.system, cx.look.flavor, a));
                let label = action(slot)
                    .map(|a| {
                        format!(
                            "{}\n{}",
                            &a["type"].as_str().unwrap_or("?")
                                [..1.min(a["type"].as_str().unwrap_or("?").len())],
                            shown(a.get("id"))
                        )
                    })
                    .unwrap_or_default();
                if art(ui, 40.0, &format!("{slot} {label}"), selected == slot, tex).clicked() {
                    *cx.slot = slot;
                }
            }
        });
        ui.add_space(4.0);
    }
    ui.add_space(8.0);
    let key = slot_key(root, selected);
    let current = action(selected);
    let base = vec![name("actions"), key.clone()];
    forms::card(
        ui,
        &format!("slot {selected}"),
        |_| {},
        |ui| {
            forms::padded(ui, 12, 10, |ui| {
                ui.horizontal(|ui| {
                let w = col(ui, 3);
                ui.vertical(|ui| {
                    ui.set_width(w);
                    forms::label(ui, "type");
                    let options = ["(empty)", "spell", "item", "macro"];
                    let now = current.as_ref().and_then(|a| a["type"].as_str()).unwrap_or("(empty)");
                    let mut index = options.iter().position(|o| *o == now).unwrap_or(0);
                    let opts: Vec<(&str, bool)> = options.iter().map(|o| (*o, true)).collect();
                    if forms::select(ui, "world-action-type", &opts, &mut index, w) {
                        if index == 0 {
                            cx.set(base.clone(), None);
                        } else if current.is_some() {
                            let mut p = base.clone();
                            p.push(name("type"));
                            cx.set(p, Some(json!(options[index])));
                        } else if root.get("actions").is_none() {
                            cx.set(
                                vec![name("actions")],
                                Some(json!({ selected.to_string(): { "type": options[index], "id": 0 } })),
                            );
                        } else {
                            cx.set(base.clone(), Some(json!({ "type": options[index], "id": 0 })));
                        }
                    }
                });
                if current.is_some() {
                    let mut id = base.clone();
                    id.push(name("id"));
                    input(ui, cx, "ID", id, Kind::Scalar, w, "0");
                    let mut usable = base.clone();
                    usable.push(name("usable"));
                    ui.vertical(|ui| {
                        ui.set_width(w);
                        forms::label(ui, "usable");
                        let options = ["true", "false", "{ usable = false, noPower = true }"];
                        let now = current.as_ref().map(|a| a["usable"].clone()).unwrap_or(Json::Null);
                        let mut index = match &now {
                            Json::Bool(false) => 1,
                            Json::Object(_) => 2,
                            _ => 0,
                        };
                        let opts: Vec<(&str, bool)> = options.iter().map(|o| (*o, true)).collect();
                        if forms::select(ui, "world-action-usable", &opts, &mut index, w) {
                            let v = match index {
                                1 => Some(json!(false)),
                                2 => Some(json!({ "usable": false, "noPower": true })),
                                _ => None,
                            };
                            cx.set(usable, v);
                        }
                    });
                }
            });
            });
        },
    );
}

/// The equipment slots, by the client's names and IDs.
pub const SLOTS: &[(&str, u32)] = &[
    ("HeadSlot", 1),
    ("NeckSlot", 2),
    ("ShoulderSlot", 3),
    ("BackSlot", 15),
    ("ChestSlot", 5),
    ("ShirtSlot", 4),
    ("TabardSlot", 19),
    ("WristSlot", 9),
    ("HandsSlot", 10),
    ("WaistSlot", 6),
    ("LegsSlot", 7),
    ("FeetSlot", 8),
    ("Finger0Slot", 11),
    ("Finger1Slot", 12),
    ("Trinket0Slot", 13),
    ("Trinket1Slot", 14),
    ("MainHandSlot", 16),
    ("SecondaryHandSlot", 17),
    ("RangedSlot", 18),
];

const QUALITY: &[&str] = &[
    "poor",
    "common",
    "uncommon",
    "rare",
    "epic",
    "legendary",
    "artifact",
    "heirloom",
];

fn equipment(ui: &mut Ui, cx: &mut Cx<'_>) {
    let root = cx.root;
    let worn = |slot: &str, id: u32| -> Option<(String, Json)> {
        let e = root.get("equipment")?;
        e.get(slot)
            .map(|v| (slot.to_string(), v.clone()))
            .or_else(|| e.get(id.to_string()).map(|v| (id.to_string(), v.clone())))
    };
    let item_word = |v: &Json| -> String {
        let id = match v {
            Json::Number(n) => n.to_string(),
            other => shown(other.get("id")),
        };
        let quality = root["items"][&id]["quality"]
            .as_u64()
            .and_then(|q| QUALITY.get(q as usize))
            .copied();
        match quality {
            Some(q) => format!("item {id} · {q}"),
            None => format!("item {id}"),
        }
    };
    let selected = (*cx.slot).min(SLOTS.len() - 1);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
        for (i, (slot, id)) in SLOTS.iter().enumerate() {
            let w = worn(slot, *id);
            let sub = w
                .as_ref()
                .map(|(_, v)| item_word(v))
                .unwrap_or_else(|| "empty".into());
            let pic = cx.look.pic(
                if w.is_some() { "icon" } else { "" },
                w.as_ref().and_then(|(_, v)| item_art(v)),
            );
            if tile(
                ui,
                92.0,
                &pic,
                slot.trim_end_matches("Slot"),
                &sub,
                selected == i,
                w.is_none(),
            ) {
                *cx.slot = i;
            }
        }
    });
    ui.add_space(10.0);
    let (slot, id) = SLOTS[selected];
    let key = worn(slot, id)
        .map(|(k, _)| k)
        .unwrap_or_else(|| slot.to_string());
    let path = vec![name("equipment"), name(&key)];
    forms::card(
        ui,
        slot,
        |_| {},
        |ui| {
            forms::padded(ui, 12, 10, |ui| {
                ui.horizontal(|ui| {
                    let w = col(ui, 2);
                    if root.get("equipment").is_none() {
                        let buffer = cx.buffers.entry("equipment/+".into()).or_default();
                        ui.vertical(|ui| {
                            ui.set_width(w);
                            forms::label(ui, "item ID");
                            if forms::text_input(ui, buffer, w, "19019").changed()
                                && let Ok(n) = buffer.trim().parse::<u64>()
                            {
                                cx.changes.push(Change {
                                    path: vec![name("equipment")],
                                    value: Some(json!({ slot: n })),
                                    typing: false,
                                });
                            }
                        });
                    } else {
                        input(
                            ui,
                            cx,
                            "item ID, or an item with its own data",
                            path.clone(),
                            Kind::Json,
                            w,
                            "19019",
                        );
                    }
                });
            });
        },
    );
    caption(
        ui,
        "By the client's slot name or slot ID: an item ID, or an item with its own data. Item quality shows as a word; the game's colours stay out of Studio's chrome.",
    );
}

fn bags(ui: &mut Ui, cx: &mut Cx<'_>) {
    let p = palette(ui.ctx());
    let root = cx.root;
    let all: Vec<(Key, Json)> = root
        .get("bags")
        .map(|b| list(b).into_iter().map(|(k, v)| (k, v.clone())).collect())
        .unwrap_or_default();
    if all.is_empty() {
        ui.label(
            egui::RichText::new("No bags: the backpack is empty.")
                .font(Face::Plex.at(12.0))
                .color(p.ink_muted),
        );
        if widgets::button(
            ui,
            ButtonKind::Secondary,
            "+ the backpack (bag 0)",
            true,
            None,
        )
        .clicked()
        {
            cx.set(
                vec![name("bags")],
                Some(json!({ "0": { "slots": 16, "items": {} } })),
            );
        }
        return;
    }
    let names: Vec<String> = all
        .iter()
        .map(|(k, _)| match k {
            Key::Name(n) => format!("bag {n}"),
            Key::Index(i) => format!("bag {i}"),
        })
        .collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let current = (*cx.bag).min(all.len() - 1);
    if let Some(i) = widgets::seg(ui, "world-bags", &refs, current, false) {
        *cx.bag = i;
        *cx.entry = 0;
    }
    let (key, bag) = all[current].clone();
    let size = bag["slots"].as_u64().unwrap_or(16) as usize;
    let items = bag.get("items").cloned().unwrap_or(Json::Null);
    let item_at = |slot: usize| -> Option<(Key, Json)> {
        match &items {
            Json::Array(a) => a
                .get(slot - 1)
                .filter(|v| !v.is_null())
                .map(|v| (Key::Index(slot - 1), v.clone())),
            Json::Object(o) => o
                .get(&slot.to_string())
                .map(|v| (Key::Name(slot.to_string()), v.clone())),
            _ => None,
        }
    };
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(format!("{} · {size} slots", names[current]))
            .font(Face::Plex.at(11.0))
            .color(p.ink_muted),
    );
    let selected = (*cx.entry).clamp(1, size.max(1));
    for row in (1..=size).collect::<Vec<_>>().chunks(8) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for &slot in row {
                let label = item_at(slot)
                    .map(|(_, v)| {
                        let id = if v.is_number() {
                            v.to_string()
                        } else {
                            shown(v.get("id"))
                        };
                        let count = v.get("count").map(|c| format!(" ×{c}")).unwrap_or_default();
                        format!("{id}{count}")
                    })
                    .unwrap_or_default();
                let tex = item_at(slot)
                    .and_then(|(_, v)| item_art(&v))
                    .and_then(|a| cx.look.art.get(cx.system, cx.look.flavor, a));
                if art(ui, 40.0, &label, selected == slot, tex).clicked() {
                    *cx.entry = slot;
                }
            }
        });
        ui.add_space(4.0);
    }
    ui.add_space(8.0);
    let mut base = vec![name("bags"), key.clone(), name("items")];
    let existing = item_at(selected);
    forms::card(
        ui,
        &format!("slot {selected}"),
        |_| {},
        |ui| {
            forms::padded(ui, 12, 10, |ui| match existing {
                Some((k, v)) => {
                    base.push(k);
                    if v.is_number() {
                        // An ID alone: made a stack to give it a count.
                        ui.horizontal(|ui| {
                            field(ui, cx, base.clone(), Kind::Number, 140.0, "item ID");
                            if forms::text_button(ui, "as a stack").clicked() {
                                cx.set(base.clone(), Some(json!({ "id": v, "count": 1 })));
                            }
                            if forms::text_button(ui, "remove").clicked() {
                                cx.set(base.clone(), None);
                            }
                        });
                    } else {
                        grid(
                            ui,
                            cx,
                            &base,
                            &[
                                ("id", Kind::Number),
                                ("count", Kind::Number),
                                ("bound", Kind::Bool),
                                ("locked", Kind::Bool),
                            ],
                        );
                        if forms::text_button(ui, "remove").clicked() {
                            cx.set(base.clone(), None);
                        }
                    }
                }
                None => {
                    if widgets::button(ui, ButtonKind::Secondary, "+ an item stack", true, None)
                        .clicked()
                    {
                        match &items {
                            Json::Array(a) if selected == a.len() + 1 => {
                                base.push(Key::Index(a.len()));
                                cx.set(base.clone(), Some(json!({ "id": 0, "count": 1 })));
                            }
                            Json::Object(_) => {
                                base.push(name(&selected.to_string()));
                                cx.set(base.clone(), Some(json!({ "id": 0, "count": 1 })));
                            }
                            _ => {
                                // (none yet, or an array with a gap: keyed by slot)
                                let mut map = serde_json::Map::new();
                                for s in 1..=size {
                                    if let Some((_, v)) = item_at(s) {
                                        map.insert(s.to_string(), v);
                                    }
                                }
                                map.insert(selected.to_string(), json!({ "id": 0, "count": 1 }));
                                cx.set(base.clone(), Some(Json::Object(map)));
                            }
                        }
                    }
                }
            });
        },
    );
    caption(
        ui,
        "Each stack: an item ID, a count, and optionally bound, locked or a cooldown { remaining, duration }.",
    );
}

fn saved_variables(ui: &mut Ui, cx: &mut Cx<'_>) -> Option<Action> {
    let p = palette(ui.ctx());
    let mut action = None;
    let vars: Vec<(String, Json)> = cx
        .root
        .get("savedVariables")
        .and_then(Json::as_object)
        .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    for (var, value) in &vars {
        let mut remove = false;
        forms::card(
            ui,
            var,
            |ui| {
                if forms::text_button(ui, "remove").clicked() {
                    remove = true;
                }
            },
            |ui| {
                forms::padded(ui, 12, 10, |ui| {
                    let text = serde_json::to_string_pretty(value).unwrap_or_default();
                    let lines: Vec<&str> = text.lines().collect();
                    let shown: String = lines
                        .iter()
                        .take(40)
                        .copied()
                        .collect::<Vec<_>>()
                        .join("\n");
                    ui.label(
                        egui::RichText::new(shown)
                            .font(Face::Plex.at(12.0))
                            .color(p.ink),
                    );
                    if lines.len() > 40 {
                        ui.label(
                            egui::RichText::new(format!(
                                "… {} more lines: the json view has all of it",
                                lines.len() - 40
                            ))
                            .font(Face::Plex.at(11.0))
                            .color(p.ink_faint),
                        );
                    }
                });
            },
        );
        if remove {
            cx.set(vec![name("savedVariables"), name(var)], None);
        }
        ui.add_space(8.0);
    }
    ui.horizontal(|ui| {
        if widgets::button(
            ui,
            ButtonKind::Secondary,
            "import from a SavedVariables file…",
            true,
            None,
        )
        .clicked()
            && let Some(file) = cx.system.pick_file("a SavedVariables file", &["lua"])
        {
            match cx.system.read_saved_variables(&file) {
                Ok(found) if found.is_empty() => {
                    action = Some(Action::Note(
                        "SavedVariables".into(),
                        format!("{} holds no variables", file.display()),
                    ));
                }
                Ok(found) => {
                    for (var, value) in found {
                        if cx.root.get("savedVariables").is_none() {
                            cx.set(vec![name("savedVariables")], Some(json!({ var: value })));
                        } else {
                            cx.set(vec![name("savedVariables"), name(&var)], Some(value));
                        }
                    }
                }
                Err(e) => action = Some(Action::Note("SavedVariables".into(), e)),
            }
        }
        ui.label(
            egui::RichText::new("WTF/Account/…/SavedVariables/<Addon>.lua: read as Lua, never run")
                .font(Face::Plex.at(11.0))
                .color(p.ink_faint),
        );
    });
    action
}

// -------------------------------------------------------------- generic --

/// The form of a part from its schema.
pub fn generic(ui: &mut Ui, cx: &mut Cx<'_>, part: &str, schema: Option<&Json>) {
    let p = palette(ui.ctx());
    let Some(schema) = schema else {
        ui.label(
            egui::RichText::new("The schema is not here: edit this part in the json view.")
                .font(Face::Plex.at(12.0))
                .color(p.ink_muted),
        );
        return;
    };
    let form = schema::form(schema, part);
    let label = super::PARTS
        .iter()
        .find(|(k, _, _)| *k == part)
        .map_or(part, |(_, l, _)| *l);
    let value = cx.root.get(part).cloned();
    let Some(value) = value else {
        forms::card(
            ui,
            "",
            |_| {},
            |ui| {
                forms::padded(ui, 12, 10, |ui| {
                    forms::paragraph(
                        ui,
                        &format!(
                            "This profile doesn't set {label}: the session uses the client's default."
                        ),
                        12.5,
                        19.0,
                        p.ink,
                    );
                    ui.add_space(6.0);
                    if part != "leaderBoards"
                        && widgets::button(
                            ui,
                            ButtonKind::Secondary,
                            &format!("add {label}"),
                            true,
                            None,
                        )
                        .clicked()
                    {
                        let empty = match &form {
                            Form::Value(Kind::Number) => json!(0),
                            Form::Value(Kind::Text) => json!(""),
                            Form::Value(Kind::Bool) => json!(false),
                            _ => json!({}),
                        };
                        cx.set(vec![name(part)], Some(empty));
                    }
                });
            },
        );
        return;
    };
    if part == "leaderBoards" {
        forms::callout(ui, forms::CalloutKind::Plain, |ui| {
            ui.label(
                egui::RichText::new(
                    "Recorded by a snapshot and kept with the profile; the world doesn't apply it.",
                )
                .font(Face::Plex.at(12.0))
                .color(p.ink),
            );
        });
        ui.label(
            egui::RichText::new(serde_json::to_string_pretty(&value).unwrap_or_default())
                .font(Face::Plex.at(12.0))
                .color(p.ink),
        );
        return;
    }
    let base = vec![name(part)];
    match &form {
        Form::Value(kind) => {
            ui.horizontal(|ui| {
                let w = col(ui, 2);
                input(ui, cx, part, base.clone(), kind.clone(), w, "");
                if part == "money"
                    && let Some(c) = value.as_f64()
                {
                    ui.label(
                        egui::RichText::new(super::money(c))
                            .font(Face::Plex.at(12.0))
                            .color(p.ink_muted),
                    );
                }
            });
        }
        Form::Fields { fields, other } => {
            schema_grid(ui, cx, &base, fields);
            if *other {
                others(ui, cx, &base, &value, fields);
            }
        }
        Form::Map(kind) => map(ui, cx, &base, &value, kind, part),
        Form::Entries { fields, other } => {
            let all = list(&value);
            for (i, (k, v)) in all.iter().enumerate() {
                let key = match k {
                    Key::Name(n) => n.clone(),
                    Key::Index(i) => format!("#{}", i + 1),
                };
                let set: Vec<&str> = v
                    .as_object()
                    .map(|o| o.keys().map(String::as_str).take(5).collect())
                    .unwrap_or_default();
                let text = format!(
                    "{key:>8}  {:<32} {}",
                    v["name"].as_str().unwrap_or(""),
                    set.join(", ")
                );
                if ui
                    .selectable_label(
                        *cx.entry == i,
                        egui::RichText::new(text).font(Face::Plex.at(12.0)),
                    )
                    .clicked()
                {
                    *cx.entry = i;
                }
            }
            // A new entry by its key.
            let new = format!("{part}/+");
            ui.horizontal(|ui| {
                let buffer = cx.buffers.entry(new.clone()).or_default();
                forms::text_input(ui, buffer, 140.0, "ID");
                let typed = buffer.trim().to_string();
                if forms::text_button(ui, "+ entry").clicked() && !typed.is_empty() {
                    let entry = match &value {
                        Json::Array(a) => Key::Index(a.len()),
                        _ => Key::Name(typed.clone()),
                    };
                    let mut path = base.clone();
                    path.push(entry);
                    cx.set(path, Some(json!({})));
                    cx.buffers.insert(new.clone(), String::new());
                    *cx.entry = all.len();
                }
            });
            if let Some((k, v)) = all.get(*cx.entry) {
                let mut path = base.clone();
                path.push(k.clone());
                let title = match k {
                    Key::Name(n) => n.clone(),
                    Key::Index(i) => format!("#{}", i + 1),
                };
                ui.add_space(10.0);
                let mut remove = false;
                let v = (*v).clone();
                forms::card(
                    ui,
                    &title,
                    |ui| {
                        if forms::text_button(ui, "remove").clicked() {
                            remove = true;
                        }
                    },
                    |ui| {
                        forms::padded(ui, 12, 10, |ui| {
                            schema_grid(ui, cx, &path, fields);
                            if *other {
                                others(ui, cx, &path, &v, fields);
                            }
                        });
                    },
                );
                if remove {
                    cx.set(path, None);
                    *cx.entry = 0;
                }
            }
        }
        Form::Json => {
            input(ui, cx, part, base, Kind::Json, ui.available_width(), "");
        }
    }
    caption(
        ui,
        "This form is built from schema/world-profile-0.1.json: a field's type is checked as you type, an empty field is left out of the file.",
    );
}

/// The schema's fields four to a row.
fn schema_grid(ui: &mut Ui, cx: &mut Cx<'_>, base: &[Key], fields: &[schema::Field]) {
    for row in fields.chunks(4) {
        ui.horizontal(|ui| {
            let w = col(ui, 4);
            for f in row {
                let mut path = base.to_vec();
                path.push(name(&f.name));
                ui.vertical(|ui| {
                    ui.set_width(w);
                    let p = palette(ui.ctx());
                    ui.horizontal(|ui| {
                        forms::label(ui, &f.name);
                        ui.label(
                            egui::RichText::new(f.kind.word())
                                .font(Face::Plex.at(10.0))
                                .color(p.ink_faint),
                        );
                    });
                    if f.kind == Kind::Bool {
                        check(ui, cx, "", path, false);
                    } else {
                        field(ui, cx, path, f.kind.clone(), w, "");
                    }
                    if !f.help.is_empty() {
                        forms::help(ui, &f.help);
                    }
                });
            }
        });
        ui.add_space(6.0);
    }
}

/// The fields the schema doesn't name, as keys and JSON values.
fn others(ui: &mut Ui, cx: &mut Cx<'_>, base: &[Key], value: &Json, fields: &[schema::Field]) {
    let p = palette(ui.ctx());
    let extra: Vec<(String, Json)> = value
        .as_object()
        .map(|o| {
            o.iter()
                .filter(|(k, _)| !fields.iter().any(|f| &f.name == *k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
        .unwrap_or_default();
    ui.add_space(6.0);
    forms::card(
        ui,
        "other fields",
        |ui| {
            ui.label(
                egui::RichText::new("not in the schema: passed to the engine as they are")
                    .font(Face::Plex.at(11.0))
                    .color(p.ink_faint),
            );
        },
        |ui| {
            forms::padded(ui, 12, 10, |ui| {
                for (k, _) in &extra {
                    ui.horizontal(|ui| {
                        cell(
                            ui,
                            180.0,
                            egui::RichText::new(k)
                                .font(Face::Plex.at(12.0))
                                .color(p.ink),
                        );
                        let mut path = base.to_vec();
                        path.push(name(k));
                        field(
                            ui,
                            cx,
                            path.clone(),
                            Kind::Json,
                            (ui.available_width() - 40.0).max(120.0),
                            "",
                        );
                        if forms::text_button(ui, "×").clicked() {
                            cx.set(path, None);
                        }
                    });
                }
                new_key(ui, cx, base, "+ field");
            });
        },
    );
}

/// A key typed, added with an empty string.
fn new_key(ui: &mut Ui, cx: &mut Cx<'_>, base: &[Key], word: &str) {
    let id = format!("{}/+", path_of(base));
    ui.horizontal(|ui| {
        let buffer = cx.buffers.entry(id.clone()).or_default();
        forms::text_input(ui, buffer, 180.0, "name");
        let typed = buffer.trim().to_string();
        if forms::text_button(ui, word).clicked() && !typed.is_empty() {
            let mut path = base.to_vec();
            path.push(name(&typed));
            cx.set(path, Some(json!("")));
            cx.buffers.insert(id.clone(), String::new());
        }
    });
}

/// Keys and values (`cvars`, `bindings`, `stats`).
fn map(ui: &mut Ui, cx: &mut Cx<'_>, base: &[Key], value: &Json, kind: &Kind, part: &str) {
    let (key_head, value_head, word) = match part {
        "cvars" => ("CVar", "value", "+ CVar"),
        "bindings" => ("key", "action", "+ binding"),
        "stats" => ("stat", "value", "+ stat"),
        _ => ("key", "value", "+ entry"),
    };
    let p = palette(ui.ctx());
    ui.horizontal(|ui| {
        cell(
            ui,
            220.0,
            egui::RichText::new(key_head)
                .font(Face::PlexMedium.at(11.0))
                .color(p.ink_muted),
        );
        ui.label(
            egui::RichText::new(value_head)
                .font(Face::PlexMedium.at(11.0))
                .color(p.ink_muted),
        );
    });
    let entries: Vec<String> = value
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    for k in &entries {
        ui.horizontal(|ui| {
            cell(
                ui,
                220.0,
                egui::RichText::new(k)
                    .font(Face::Plex.at(12.0))
                    .color(p.ink),
            );
            let mut path = base.to_vec();
            path.push(name(k));
            field(
                ui,
                cx,
                path.clone(),
                kind.clone(),
                (ui.available_width() - 40.0).max(120.0),
                "",
            );
            if forms::text_button(ui, "×").clicked() {
                cx.set(path, None);
            }
        });
    }
    new_key(ui, cx, base, word);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_text_becomes_its_kind() {
        assert_eq!(parse(&Kind::Number, "62"), Ok(Some(json!(62))));
        assert_eq!(parse(&Kind::Number, "0.5"), Ok(Some(json!(0.5))));
        // Not a number yet: written as typed, the check says so.
        assert_eq!(parse(&Kind::Number, "6x"), Ok(Some(json!("6x"))));
        assert_eq!(parse(&Kind::Number, " "), Ok(None));
        assert_eq!(parse(&Kind::Scalar, "true"), Ok(Some(json!(true))));
        assert_eq!(parse(&Kind::Scalar, "1"), Ok(Some(json!(1))));
        assert_eq!(
            parse(&Kind::Scalar, "ACTIONBUTTON1"),
            Ok(Some(json!("ACTIONBUTTON1")))
        );
        assert_eq!(
            parse(&Kind::Numbers, "1, 2,x, 3"),
            Ok(Some(json!([1, 2, 3])))
        );
        assert_eq!(parse(&Kind::Json, "{ \"a\": "), Err(()));
        assert_eq!(
            parse(&Kind::Json, "{ \"a\": 1 }"),
            Ok(Some(json!({ "a": 1 })))
        );
    }

    #[test]
    fn a_flavor_s_classes_and_races() {
        assert_eq!(classes("camelot").len(), 9);
        assert_eq!(classes("wrath").len(), 10);
        assert_eq!(classes("standard").len(), 13);
        assert!(races("vanilla").len() == 8 && races("tbc").contains(&"Draenei"));
    }
}
