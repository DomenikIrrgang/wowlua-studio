//! The parts' forms (S060): the designed ones (character, units, auras,
//! spells, action bars, equipment, bags, SavedVariables) and the one
//! built from the schema for the others. A form reads the profile's JSON
//! and makes changes; it never writes the file itself.

use std::collections::HashMap;

use egui::{Align2, CornerRadius, Rect, Sense, Stroke, Ui, Vec2};
use serde_json::{Value as Json, json};
use wowlua_studio_ui::widgets::{self, ButtonKind};
use wowlua_studio_ui::{Face, palette};
use wowlua_studio_ui::{controls, forms};

use super::json::Key;
use super::schema::{self, Form, Kind};
use super::{Action, ArtCache, Change, list};
use crate::system::{Art, Find, System};

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
    /// What the ID pickers found; the questions to ask.
    pub found: &'a HashMap<(Find, String, String), Vec<crate::system::SpellRow>>,
    pub ask: &'a mut Vec<(Find, String)>,
    /// A unit to place in the game view, or its path to draw (S080).
    pub place: &'a mut Option<super::Placing>,
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
        Kind::Choice(_) => Json::String(t.to_string()),
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

/// A number one step at a time under its label, with presets beside it
/// (S065); `default` while unset.
#[allow(clippy::too_many_arguments)]
fn stepped(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    label: &str,
    path: Vec<Key>,
    width: f32,
    (min, max): (Option<f64>, Option<f64>),
    unit: &str,
    presets: &[(String, f64)],
    default: f64,
) {
    ui.vertical(|ui| {
        ui.set_width(width);
        forms::label(ui, label);
        let before = at(cx.root, &path).and_then(Json::as_f64).unwrap_or(default);
        let mut v = before;
        // (the presets on their own row: the columns are narrow)
        controls::stepper(ui, &path_of(&path), &mut v, min, max, 1.0, unit);
        let options: Vec<(&str, f64)> = presets.iter().map(|(l, n)| (l.as_str(), *n)).collect();
        if let Some(picked) = controls::presets(ui, &options, v) {
            v = picked;
        }
        if (v - before).abs() > 1e-9 {
            cx.set(path, Some(number(v)));
        }
    });
}

/// A pool (health, power) under its label: how much is left on a slider up
/// to the maximum, the maximum a stepper (S065).
#[allow(clippy::too_many_arguments)]
fn pool(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    label: &str,
    now: Vec<Key>,
    max: Vec<Key>,
    width: f32,
    default_max: f64,
) {
    ui.vertical(|ui| {
        ui.set_width(width);
        forms::label(ui, label);
        let mut top = at(cx.root, &max)
            .and_then(Json::as_f64)
            .unwrap_or(default_max)
            .max(1.0);
        let mut left = at(cx.root, &now)
            .and_then(Json::as_f64)
            .unwrap_or(top)
            .min(top);
        if controls::slider(
            ui,
            &path_of(&now),
            &mut left,
            0.0,
            top,
            1.0,
            1.0,
            "",
            &[],
            width,
        ) {
            cx.set(now.clone(), Some(number(left)));
        }
        if controls::stepper(ui, &path_of(&max), &mut top, Some(1.0), None, 1.0, "max") {
            cx.set(max, Some(number(top)));
        }
    });
}

/// A level's presets: 1, 10, the flavor's highest.
fn level_presets(max: u32) -> Vec<(String, f64)> {
    vec![
        ("1".into(), 1.0),
        ("10".into(), 10.0),
        (format!("max · {max}"), f64::from(max)),
    ]
}

/// A field without its label.
fn field(ui: &mut Ui, cx: &mut Cx<'_>, path: Vec<Key>, kind: Kind, width: f32, hint: &str) {
    if let Kind::Choice(values) = &kind {
        let now = shown(at(cx.root, &path));
        let mut list: Vec<String> = vec!["(none)".into()];
        list.extend(values.iter().cloned());
        if !now.is_empty() && !list.contains(&now) {
            list.push(now.clone());
        }
        let mut index = list.iter().position(|o| o == &now).unwrap_or(0);
        let opts: Vec<(&str, bool)> = list.iter().map(|o| (o.as_str(), true)).collect();
        if forms::select(ui, &path_of(&path), &opts, &mut index, width) {
            cx.set(path, (index > 0).then(|| Json::String(list[index].clone())));
        }
        return;
    }
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
        "money" => money(ui, cx),
        "position" => position(ui, cx),
        "screen" => screen(ui, cx),
        _ => {}
    }
    None
}

/// The money (S065): gold, silver and copper, kept in copper.
fn money(ui: &mut Ui, cx: &mut Cx<'_>) {
    let path = vec![name("money")];
    let copper = at(cx.root, &path)
        .and_then(Json::as_f64)
        .unwrap_or(0.0)
        .max(0.0)
        .round();
    let (mut g, mut s, mut c) = (
        (copper / 10_000.0).floor(),
        ((copper / 100.0) % 100.0).floor(),
        copper % 100.0,
    );
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        changed |= controls::stepper(ui, "money.gold", &mut g, Some(0.0), None, 1.0, "gold");
        changed |= controls::stepper(
            ui,
            "money.silver",
            &mut s,
            Some(0.0),
            Some(99.0),
            1.0,
            "silver",
        );
        changed |= controls::stepper(
            ui,
            "money.copper",
            &mut c,
            Some(0.0),
            Some(99.0),
            1.0,
            "copper",
        );
    });
    if changed {
        cx.set(path, Some(number(g * 10_000.0 + s * 100.0 + c)));
    }
    caption(ui, "Applied as mock.player:setMoney, in copper.");
}

/// Where the character is (S065): the map and zone, the place on the map
/// as sliders, the facing on a compass, `UnitPosition`'s coordinates as
/// fields.
fn position(ui: &mut Ui, cx: &mut Cx<'_>) {
    let p = |f: &str| vec![name("position"), name(f)];
    let w = col(ui, 3);
    ui.horizontal(|ui| {
        stepped(
            ui,
            cx,
            "map ID",
            p("mapID"),
            w,
            (Some(0.0), None),
            "",
            &[],
            0.0,
        );
        input(ui, cx, "zone", p("zone"), Kind::Text, w, "Elwynn Forest");
        input(
            ui,
            cx,
            "sub-zone",
            p("subZone"),
            Kind::Text,
            w,
            "Northshire Valley",
        );
    });
    ui.add_space(8.0);
    let width = ui.available_width().min(520.0);
    for (key, label) in [("x", "x · across the map"), ("y", "y · down the map")] {
        forms::label(ui, label);
        let path = p(key);
        let mut v = at(cx.root, &path).and_then(Json::as_f64).unwrap_or(0.5);
        let ticks = [(0.0, "0%"), (0.5, "50%"), (1.0, "100%")];
        if controls::slider(
            ui,
            &path_of(&path),
            &mut v,
            0.0,
            1.0,
            0.001,
            100.0,
            "%",
            &ticks,
            width,
        ) {
            cx.set(path, Some(number(v)));
        }
        ui.add_space(4.0);
    }
    forms::label(ui, "facing · counter-clockwise from north");
    let path = p("facing");
    let mut v = at(cx.root, &path).and_then(Json::as_f64).unwrap_or(0.0);
    let tau = std::f64::consts::TAU;
    let ticks = [
        (0.0, "N"),
        (tau / 4.0, "W"),
        (tau / 2.0, "S"),
        (tau * 0.75, "E"),
        (tau, "N"),
    ];
    if controls::slider(
        ui,
        &path_of(&path),
        &mut v,
        0.0,
        tau,
        // (radians kept, shown in whole degrees)
        std::f64::consts::PI / 180.0,
        180.0 / std::f64::consts::PI,
        "°",
        &ticks,
        width,
    ) {
        cx.set(path, Some(number(v)));
    }
    ui.add_space(8.0);
    forms::label(ui, "world · UnitPosition's coordinates");
    ui.horizontal_wrapped(|ui| {
        for (key, unit, step) in [
            ("y", "y", 0.1),
            ("x", "x", 0.1),
            ("z", "z", 0.1),
            ("instanceID", "instance", 1.0),
        ] {
            let path = vec![name("position"), name("world"), name(key)];
            let before = at(cx.root, &path).and_then(Json::as_f64).unwrap_or(0.0);
            let mut v = before;
            if controls::stepper(ui, &path_of(&path), &mut v, None, None, step, unit)
                && (v - before).abs() > 1e-9
            {
                cx.set(path, Some(number(v)));
            }
        }
    });
    caption(ui, "Applied as mock.moveTo.");
}

/// The screen (S065): its size (three, or custom), its DPI scale and the
/// game's own UI Scale control; off leaves the client's scale.
fn screen(ui: &mut Ui, cx: &mut Cx<'_>) {
    const SIZES: &[(u32, u32)] = &[(1920, 1080), (2560, 1440), (3840, 2160)];
    let p = |f: &str| vec![name("screen"), name(f)];
    let width = ui.available_width().min(520.0);
    let w = at(cx.root, &p("width")).and_then(Json::as_f64);
    let h = at(cx.root, &p("height")).and_then(Json::as_f64);
    let mut labels: Vec<String> = SIZES.iter().map(|(w, h)| format!("{w} × {h}")).collect();
    labels.push("custom…".into());
    let at_size = match (w, h) {
        (None, None) => 0,
        (Some(w), Some(h)) => SIZES
            .iter()
            .position(|(sw, sh)| f64::from(*sw) == w && f64::from(*sh) == h)
            .unwrap_or(SIZES.len()),
        _ => SIZES.len(),
    };
    let custom_id = egui::Id::unique("wowlua-studio-profile-screen-custom");
    let custom: bool = ui.data(|d| d.get_temp(custom_id)).unwrap_or(false);
    let mut index = if custom { SIZES.len() } else { at_size };
    forms::label(ui, "size");
    let opts: Vec<(&str, bool)> = labels.iter().map(|l| (l.as_str(), true)).collect();
    if forms::select(
        ui,
        "profile-screen-size",
        &opts,
        &mut index,
        width.min(240.0),
    ) {
        ui.data_mut(|d| d.insert_temp(custom_id, index == SIZES.len()));
        if let Some((sw, sh)) = SIZES.get(index) {
            cx.set(p("width"), Some(json!(sw)));
            cx.set(p("height"), Some(json!(sh)));
        }
    }
    if index == SIZES.len() {
        ui.horizontal_wrapped(|ui| {
            for (key, unit, fallback, (lo, hi)) in [
                ("width", "×", 1920.0, (640.0, 7680.0)),
                ("height", "px", 1080.0, (480.0, 4320.0)),
            ] {
                let path = p(key);
                let mut v = at(cx.root, &path)
                    .and_then(Json::as_f64)
                    .unwrap_or(fallback);
                if controls::stepper(ui, &path_of(&path), &mut v, Some(lo), Some(hi), 1.0, unit) {
                    cx.set(path, Some(number(v)));
                }
            }
        });
    }
    ui.add_space(6.0);
    forms::label(ui, "DPI scale");
    let dpis = [1.0, 1.25, 1.5, 2.0];
    let dpi = at(cx.root, &p("dpiScale"))
        .and_then(Json::as_f64)
        .unwrap_or(1.0);
    let names: Vec<String> = dpis.iter().map(|d| format!("{d}")).collect();
    let mut at_dpi = dpis
        .iter()
        .position(|d| (d - dpi).abs() < 1e-9)
        .unwrap_or(0);
    let opts: Vec<(&str, bool)> = names.iter().map(|l| (l.as_str(), true)).collect();
    if forms::select(
        ui,
        "profile-screen-dpi",
        &opts,
        &mut at_dpi,
        width.min(240.0),
    ) {
        cx.set(p("dpiScale"), Some(number(dpis[at_dpi])));
    }
    ui.add_space(6.0);
    let path = p("uiScale");
    let scale = at(cx.root, &path).and_then(Json::as_f64);
    let mut on = scale.is_some();
    if forms::checkbox(
        ui,
        &mut on,
        "Use UI Scale",
        Some("off: the client's own scale"),
    ) {
        cx.set(path.clone(), on.then(|| number(1.0)));
    }
    if let Some(s) = scale {
        let mut v = s;
        let ticks = [(0.65, "65%"), (1.0, "100%"), (1.15, "115%")];
        if controls::slider(
            ui,
            &path_of(&path),
            &mut v,
            0.65,
            1.15,
            0.01,
            100.0,
            "%",
            &ticks,
            width,
        ) {
            cx.set(path, Some(number(v)));
        }
    }
}

/// The classes' short names on their art tiles.
const SHORT: &[(&str, &str)] = &[
    ("WARRIOR", "WAR"),
    ("PALADIN", "PAL"),
    ("HUNTER", "HUN"),
    ("ROGUE", "ROG"),
    ("PRIEST", "PRI"),
    ("SHAMAN", "SHA"),
    ("MAGE", "MAG"),
    ("WARLOCK", "LOC"),
    ("DRUID", "DRU"),
    ("DEATHKNIGHT", "DK"),
    ("MONK", "MNK"),
    ("DEMONHUNTER", "DH"),
    ("EVOKER", "EVO"),
];

/// The flavor as wowlua's table has it; retail's (every class and race)
/// for one it doesn't know.
fn flavor_of(flavor: &str) -> Option<&'static wowlua_flavors::Flavor> {
    let table = wowlua_flavors::table();
    table.flavor(flavor).or_else(|| table.flavor("standard"))
}

/// The classes a flavor has (wowlua's flavor table, in the client's
/// order), by file name and short name.
pub fn classes(flavor: &str) -> Vec<(&'static str, &'static str)> {
    flavor_of(flavor)
        .map(|f| {
            f.classes
                .iter()
                .map(|c| {
                    let file = c.file.as_str();
                    let short = SHORT
                        .iter()
                        .find(|(f, _)| *f == file)
                        .map_or_else(|| &file[..file.len().min(3)], |(_, s)| *s);
                    (file, short)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The races a flavor has (wowlua's flavor table), by file name.
pub fn races(flavor: &str) -> Vec<&'static str> {
    flavor_of(flavor)
        .map(|f| f.race_files().collect())
        .unwrap_or_default()
}

/// The powers a class has on a flavor (wowlua's table), the shown one
/// first.
fn powers_of(flavor: &str, class: &str) -> Vec<&'static str> {
    flavor_of(flavor)
        .and_then(|f| f.class(class))
        .map(|c| c.powers.iter().map(String::as_str).collect())
        .unwrap_or_default()
}

/// Every power a class has (T512): one slider each up to its maximum, in
/// `powers.<TOKEN>`, the flavor's defaults while unset; which one shows.
fn class_powers(ui: &mut Ui, cx: &mut Cx<'_>, base: &[Key], class: &str, width: f32) {
    let flavor = cx.look.flavor;
    let powers = powers_of(flavor, class);
    let table = wowlua_flavors::table();
    let f = flavor_of(flavor);
    ui.horizontal_wrapped(|ui| {
        for token in &powers {
            let default = f.and_then(|f| f.power_default(token));
            let mut now: Vec<Key> = base.to_vec();
            now.extend([name("powers"), name(token), name("current")]);
            let mut max: Vec<Key> = base.to_vec();
            max.extend([name("powers"), name(token), name("max")]);
            let label = token.to_ascii_lowercase().replace('_', " ");
            pool(
                ui,
                cx,
                &label,
                now,
                max,
                width,
                default.map_or(100.0, |d| d.max),
            );
        }
    });
    // Which power the unit shows (a druid's form): one of the class's.
    if powers.len() > 1 {
        let mut path: Vec<Key> = base.to_vec();
        path.push(name("powerType"));
        let shown = at(cx.root, &path)
            .and_then(Json::as_f64)
            .and_then(|id| table.power_token(id as u32))
            .unwrap_or(powers[0]);
        let mut index = powers.iter().position(|t| *t == shown).unwrap_or(0);
        let labels: Vec<String> = powers
            .iter()
            .map(|t| t.to_ascii_lowercase().replace('_', " "))
            .collect();
        let opts: Vec<(&str, bool)> = labels.iter().map(|l| (l.as_str(), true)).collect();
        ui.vertical(|ui| {
            ui.set_width(width);
            forms::label(ui, "shown power");
            if forms::select(ui, &path_of(&path), &opts, &mut index, width)
                && let Some(id) = table.power_type(powers[index])
            {
                cx.set(path, Some(number(f64::from(id))));
            }
        });
    }
}

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
    let race = data
        .as_ref()
        .and_then(|d| d["race"].as_str())
        .unwrap_or("")
        .to_string();
    let chosen = data
        .as_ref()
        .and_then(|d| d["faction"].as_str())
        .and_then(wowlua_flavors::Faction::parse);
    let flavor = flavor_of(look.flavor);
    let either = flavor
        .and_then(|f| f.race(&race))
        .is_some_and(wowlua_flavors::Race::is_either);
    let faction = flavor.and_then(|f| f.faction_of(&race, chosen));
    // (the classes the race can be, of its faction: every one without a race)
    let allowed: Option<Vec<&str>> = flavor
        .filter(|f| f.has_race(&race))
        .map(|f| f.classes_for(&race, faction));
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
        let max = crate::flavors::max_level(look.flavor);
        stepped(
            ui,
            cx,
            &format!("level · 1 to {max}"),
            p("level"),
            w,
            (Some(1.0), Some(f64::from(max))),
            "",
            &level_presets(max),
            1.0,
        );
    });
    ui.add_space(10.0);
    forms::label(ui, "class");
    let list = classes(look.flavor);
    let per = list.len().min(9);
    let w = ((ui.available_width() - 6.0 * (per as f32 - 1.0)) / per as f32).max(60.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
        for (file, short) in &list {
            let can = allowed.as_ref().is_none_or(|a| a.contains(file));
            if tile(
                ui,
                w,
                &look.pic(short, class_art(file)),
                file,
                "",
                class == *file,
                !can,
            ) && can
            {
                cx.set(p("class"), Some(Json::String((*file).to_string())));
            }
        }
    });
    if allowed
        .as_ref()
        .is_some_and(|a| !class.is_empty() && !a.contains(&class.as_str()))
    {
        caption(
            ui,
            &format!(
                "A {race} can't be a {class} in this flavor's client: pick another class or race."
            ),
        );
    }
    caption(
        ui,
        "By file name, as the client names classes; the icons are the installation's (CLASS_ICON_TCOORDS on the class sheet). Only the classes the race can be, as the flavor's client pairs them.",
    );
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        let w = col(ui, 4);
        let races = races(look.flavor);
        // The race; a race of one faction takes the chosen one away.
        ui.vertical(|ui| {
            ui.set_width(w);
            forms::label(ui, "race");
            let mut list: Vec<&str> = vec!["(none)"];
            list.extend(races.iter().copied());
            if !race.is_empty() && !list.contains(&race.as_str()) {
                list.push(&race);
            }
            let mut index = list.iter().position(|r| *r == race).unwrap_or(0);
            let opts: Vec<(&str, bool)> = list.iter().map(|r| (*r, true)).collect();
            if forms::select(ui, &path_of(&p("race")), &opts, &mut index, w) {
                let picked = (index > 0).then(|| list[index].to_owned());
                let keeps = picked.as_deref().is_some_and(|r| {
                    flavor
                        .and_then(|f| f.race(r))
                        .is_some_and(wowlua_flavors::Race::is_either)
                });
                cx.set(p("race"), picked.map(Json::String));
                if !keeps && chosen.is_some() {
                    cx.set(p("faction"), None);
                }
            }
        });
        choice(ui, cx, "sex", p("sex"), SEX, w);
        choice(ui, cx, "role", p("role"), ROLES, w);
        // The faction: the race's, chosen only where the race lets you.
        if either {
            choice(ui, cx, "faction", p("faction"), &["Alliance", "Horde"], w);
        } else {
            ui.vertical(|ui| {
                ui.set_width(w);
                forms::label(ui, "faction");
                let shown = match faction {
                    Some(f) => format!("{f} · from {race}"),
                    None => "from the race".to_owned(),
                };
                ui.add_sized(
                    Vec2::new(w, forms::CONTROL_H),
                    egui::Label::new(
                        egui::RichText::new(shown)
                            .font(Face::Plex.at(12.0))
                            .color(palette(ui.ctx()).ink_muted),
                    ),
                );
            });
        }
    });
    ui.add_space(10.0);
    let health_max = max_key(data.as_ref(), "maxHealth", "healthMax");
    let power_max = max_key(data.as_ref(), "maxPower", "powerMax");
    ui.horizontal(|ui| {
        let w = col(ui, 4);
        pool(ui, cx, "health", p("health"), p(&health_max), w, 100.0);
        // (with a class, its powers below; without, the one power)
        if class.is_empty() {
            pool(ui, cx, "power", p("power"), p(&power_max), w, 100.0);
            let tokens: Vec<String> = wowlua_flavors::table()
                .power_types
                .iter()
                .map(|t| {
                    format!(
                        "{} · {}",
                        t.id,
                        t.token.to_ascii_lowercase().replace('_', " ")
                    )
                })
                .collect();
            let options: Vec<&str> = tokens.iter().map(String::as_str).collect();
            choice(ui, cx, "power type", p("powerType"), &options, w);
        }
        ui.vertical(|ui| {
            check(ui, cx, "dead", p("dead"), false);
            check(ui, cx, "pvp", p("pvp"), false);
        });
    });
    if !class.is_empty() {
        ui.add_space(10.0);
        let w = col(ui, 4);
        class_powers(ui, cx, &base, &class, w);
    }
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
                            let classes: Vec<&str> = classes(cx.look.flavor)
                                .iter()
                                .map(|(file, _)| *file)
                                .collect();
                            choice(
                                ui,
                                cx,
                                "class",
                                vec![name("character"), name("class")],
                                &classes,
                                w,
                            );
                            let max = crate::flavors::max_level(cx.look.flavor);
                            stepped(
                                ui,
                                cx,
                                "level",
                                vec![name("character"), name("level")],
                                w,
                                (Some(1.0), Some(f64::from(max))),
                                "",
                                &level_presets(max),
                                1.0,
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
                            // (the flavor's classes; none: a creature)
                            let classes: Vec<&str> = classes(cx.look.flavor)
                                .iter()
                                .map(|(file, _)| *file)
                                .collect();
                            choice(ui, cx, "class", unit_path(&key, "class"), &classes, w);
                            let max = crate::flavors::max_level(cx.look.flavor);
                            let yours = cx.root["character"]["level"].as_f64().unwrap_or(1.0);
                            let mut presets = vec![(format!("yours · {yours}"), yours)];
                            presets.extend(level_presets(max));
                            presets.push(("?? (boss)".into(), -1.0));
                            stepped(
                                ui,
                                cx,
                                "level",
                                unit_path(&key, "level"),
                                w,
                                (Some(-1.0), Some(f64::from(max) + 3.0)),
                                "",
                                &presets,
                                1.0,
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
                            let yards =
                                [("5 · melee", 5.0), ("10", 10.0), ("30", 30.0), ("40", 40.0)];
                            stepped(
                                ui,
                                cx,
                                "distance",
                                unit_path(&key, "distance"),
                                w,
                                (Some(0.0), None),
                                "yd",
                                &yards.map(|(l, n)| (l.to_owned(), n)),
                                20.0,
                            );
                            pool(
                                ui,
                                cx,
                                "health",
                                unit_path(&key, "health"),
                                unit_path(&key, &health_max),
                                w,
                                100.0,
                            );
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
                                    // (UnitClassification's answers)
                                    field(
                                        ui,
                                        cx,
                                        unit_path(&key, "classification"),
                                        Kind::Choice(
                                            [
                                                "normal",
                                                "elite",
                                                "rare",
                                                "rareelite",
                                                "worldboss",
                                                "trivial",
                                                "minus",
                                            ]
                                            .map(str::to_owned)
                                            .to_vec(),
                                        ),
                                        half,
                                        "normal",
                                    );
                                });
                            });
                        });
                        // A unit with a class has every power it has (T512).
                        if let Some(class) = data["class"].as_str().filter(|c| !c.is_empty()) {
                            ui.add_space(6.0);
                            let w = col(ui, 4);
                            class_powers(ui, cx, &[name("units"), key.clone()], class, w);
                        }
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
                        where_and_what(ui, cx, &key, &data);
                        creature_and_role(ui, cx, &key, &data);
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

/// What an ID picker found for a query, asked for when not yet.
fn found<'b>(cx: &mut Cx<'b>, what: Find, query: &str) -> Option<&'b [crate::system::SpellRow]> {
    let all: &'b HashMap<_, _> = cx.found;
    let hit = all.get(&(what, cx.look.flavor.to_owned(), query.to_owned()));
    if hit.is_none() && !cx.ask.iter().any(|(w, q)| *w == what && q == query) {
        cx.ask.push((what, query.to_owned()));
    }
    hit.map(Vec::as_slice)
}

/// The picker a field of this name is (`spellId`, `itemID`).
fn find_of(field: &str) -> Option<Find> {
    match field.to_ascii_lowercase().as_str() {
        "spellid" => Some(Find::Spell),
        "itemid" => Some(Find::Item),
        _ => None,
    }
}

/// An ID found by name among the client's spells or items (S065): the
/// picked one's name and ID; open, a search and what it finds.
fn find(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    label: &str,
    path: Vec<Key>,
    what: Find,
    width: f32,
) -> Option<crate::system::SpellRow> {
    let p = palette(ui.ctx());
    // (the row picked this frame, for what else it fills in)
    let mut chosen: Option<crate::system::SpellRow> = None;
    let now = at(cx.root, &path).and_then(Json::as_u64).map(|n| n as u32);
    let key = path_of(&path);
    let open_id = egui::Id::unique(("wowlua-studio-find", key.clone()));
    let mut open: bool = ui.data(|d| d.get_temp(open_id)).unwrap_or(false);
    ui.vertical(|ui| {
        ui.set_width(width);
        if !label.is_empty() {
            forms::label(ui, label);
        }
        // The picked one: its name (asked by its ID), the ID.
        let picked = now.and_then(|id| {
            found(cx, what, &id.to_string())
                .and_then(|rows| rows.iter().find(|r| r.id == id))
                .map(|r| r.name.clone())
        });
        let main = match (now, picked) {
            (None, _) => "(none)".to_owned(),
            (Some(_), Some(n)) => n,
            (Some(id), None) => format!("{} {id}", what.word()),
        };
        let meta = now.map(|id| id.to_string()).unwrap_or_default();
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(width, forms::CONTROL_H), Sense::click());
        let painter = ui.painter();
        painter.rect_filled(
            rect,
            CornerRadius::ZERO,
            if response.hovered() { p.bg_300 } else { p.bg_000 },
        );
        let stroke = if open {
            Stroke::new(2.0, p.amber_ink)
        } else {
            Stroke::new(1.0, p.line_strong)
        };
        painter.rect_stroke(rect, CornerRadius::ZERO, stroke, egui::StrokeKind::Inside);
        let mut x = rect.left() + 8.0;
        let icon = match what {
            Find::Spell => now.map(Art::Spell),
            Find::Item => now.map(Art::Item),
            Find::Creature => None,
        };
        if let Some(tex) = icon.and_then(|a| cx.look.pic("", Some(a)).tex) {
            let r = Rect::from_min_size(egui::pos2(x, rect.center().y - 10.0), Vec2::splat(20.0));
            painter.image(
                tex,
                r,
                Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            x += 28.0;
        }
        let meta_w = painter
            .text(
                egui::pos2(rect.right() - 26.0, rect.center().y),
                Align2::RIGHT_CENTER,
                &meta,
                Face::Plex.at(11.5),
                p.ink_muted,
            )
            .width();
        let room = (rect.right() - 26.0 - meta_w - 8.0 - x).max(0.0);
        let shown = fit_to(ui, &main, Face::Plex.at(12.5), room);
        ui.painter().text(
            egui::pos2(x, rect.center().y),
            Align2::LEFT_CENTER,
            shown,
            Face::Plex.at(12.5),
            if now.is_some() { p.ink } else { p.ink_muted },
        );
        // (the chevron)
        let c = egui::pos2(rect.right() - 13.0, rect.center().y);
        let chevron = Stroke::new(1.5, p.ink_muted);
        ui.painter().line_segment([c + Vec2::new(-3.5, -1.5), c + Vec2::new(0.0, 2.0)], chevron);
        ui.painter().line_segment([c + Vec2::new(0.0, 2.0), c + Vec2::new(3.5, -1.5)], chevron);
        if response.clicked() {
            open = !open;
        }
        if !open {
            return;
        }
        // Open: a search and what it finds.
        egui::Frame::new()
            .fill(p.bg_100)
            .stroke(Stroke::new(1.0, p.line_strong))
            .inner_margin(egui::Margin::same(6))
            .show(ui, |ui| {
                ui.set_width(width - 12.0);
                let search = format!("{key}/find");
                let typed = {
                    let buffer = cx.buffers.entry(search.clone()).or_default();
                    forms::text_input(
                        ui,
                        buffer,
                        width - 12.0,
                        &format!("search the client's {}s by name or ID", what.word()),
                    );
                    buffer.trim().to_owned()
                };
                let typed_id = typed.parse::<u32>().ok();
                let mut pick: Option<Option<u32>> = None;
                if typed.chars().count() < 2 && typed_id.is_none() {
                    caption(ui, "Type two letters of its name, or its ID.");
                } else {
                    match found(cx, what, &typed) {
                        None => caption(ui, "searching the client…"),
                        Some([]) => {
                            caption(
                                ui,
                                &format!(
                                    "No {} of this flavor's client found (no installation with its client: use the ID).",
                                    what.word()
                                ),
                            );
                        }
                        Some(rows) => {
                            for r in rows.iter().take(12) {
                                if found_row(ui, cx, what, r, width - 12.0) {
                                    pick = Some(Some(r.id));
                                    chosen = Some(r.clone());
                                }
                            }
                        }
                    }
                    if let Some(id) = typed_id
                        && widgets::button(ui, ButtonKind::Inline, &format!("use ID {id}"), true, None)
                            .clicked()
                    {
                        pick = Some(Some(id));
                    }
                }
                if now.is_some()
                    && widgets::button(ui, ButtonKind::Inline, "clear", true, None).clicked()
                {
                    pick = Some(None);
                }
                if let Some(id) = pick {
                    cx.set(path.clone(), id.map(|i| number(f64::from(i))));
                    cx.buffers.insert(search, String::new());
                    open = false;
                }
            });
    });
    ui.data_mut(|d| d.insert_temp(open_id, open));
    chosen
}

/// A row an ID picker found: icon, name, its text, the ID; whether it
/// was clicked.
fn found_row(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    what: Find,
    row: &crate::system::SpellRow,
    width: f32,
) -> bool {
    let p = palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 28.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_300);
    }
    let art = match what {
        Find::Spell => Some(Art::Spell(row.id)),
        Find::Item => Some(Art::Item(row.id)),
        Find::Creature => None,
    };
    let mut x = rect.left() + 6.0;
    if let Some(tex) = cx.look.pic("", art).tex {
        let r = Rect::from_min_size(egui::pos2(x, rect.center().y - 10.0), Vec2::splat(20.0));
        ui.painter().image(
            tex,
            r,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
    x += 28.0;
    let id_w = ui
        .painter()
        .text(
            egui::pos2(rect.right() - 6.0, rect.center().y),
            Align2::RIGHT_CENTER,
            row.id.to_string(),
            Face::Plex.at(11.5),
            p.ink_muted,
        )
        .width();
    let room = (rect.right() - 6.0 - id_w - 12.0 - x).max(0.0);
    let name = fit_to(ui, &row.name, Face::PlexMedium.at(12.0), room);
    let name_w = ui
        .painter()
        .text(
            egui::pos2(x, rect.center().y),
            Align2::LEFT_CENTER,
            name,
            Face::PlexMedium.at(12.0),
            p.ink,
        )
        .width();
    let rest = room - name_w - 10.0;
    if rest > 40.0 && !row.text.is_empty() {
        let text = fit_to(
            ui,
            row.text.lines().next().unwrap_or(""),
            Face::Plex.at(11.5),
            rest,
        );
        ui.painter().text(
            egui::pos2(x + name_w + 10.0, rect.center().y),
            Align2::LEFT_CENTER,
            text,
            Face::Plex.at(11.5),
            p.ink_muted,
        );
    }
    response.clicked()
}

/// `text` cut with an ellipsis to fit `width` in `font`.
fn fit_to(ui: &Ui, text: &str, font: egui::FontId, width: f32) -> String {
    let w = |s: &str| {
        ui.fonts_mut(|f| {
            f.layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::WHITE)
                .size()
                .x
        })
    };
    if w(text) <= width {
        return text.to_owned();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let s: String = chars.iter().collect::<String>() + "…";
        if w(&s) <= width {
            return s;
        }
    }
    String::new()
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
                } else if let Some(what) = find_of(f) {
                    find(ui, cx, f, path, what, w);
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

    // The race's spells (T513): known with the race, not listed.
    let race = root["character"]["race"].as_str().unwrap_or("");
    if let Some(f) = flavor_of(cx.look.flavor)
        && f.has_race(race)
    {
        let faction = f.faction_of(
            race,
            root["character"]["faction"]
                .as_str()
                .and_then(wowlua_flavors::Faction::parse),
        );
        let class_file = (!class.is_empty()).then_some(class.as_str());
        let racials: Vec<&wowlua_flavors::Racial> = f.racials(race, faction, class_file).collect();
        ui.label(
            egui::RichText::new(format!("From the race · {race}"))
                .font(Face::PlexMedium.at(12.0))
                .color(p.ink),
        );
        if racials.is_empty() {
            caption(
                ui,
                "This flavor's racial spells weren't read from its client: the race's spells come from the game's tables.",
            );
        } else {
            ui.horizontal_wrapped(|ui| {
                for r in &racials {
                    egui::Frame::new()
                        .stroke(Stroke::new(1.0, p.line))
                        .inner_margin(egui::Margin::symmetric(8, 4))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(format!("{} · {}", r.name, r.spell))
                                    .font(Face::Plex.at(12.0))
                                    .color(p.ink),
                            );
                        });
                }
            });
            caption(
                ui,
                "Known with the race, without training: not in the profile.",
            );
        }
        ui.add_space(10.0);
    }

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
                    match current.as_ref().and_then(|a| a["type"].as_str()) {
                        Some("spell") => {
                            find(ui, cx, "spell", id, Find::Spell, w);
                        }
                        Some("item") => {
                            find(ui, cx, "item", id, Find::Item, w);
                        }
                        _ => input(ui, cx, "ID", id, Kind::Scalar, w, "0"),
                    }
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
                            find(ui, cx, "", base.clone(), Find::Item, 300.0);
                            if forms::text_button(ui, "as a stack").clicked() {
                                cx.set(base.clone(), Some(json!({ "id": v, "count": 1 })));
                            }
                            if forms::text_button(ui, "remove").clicked() {
                                cx.set(base.clone(), None);
                            }
                        });
                    } else {
                        let mut id = base.clone();
                        id.push(name("id"));
                        find(ui, cx, "item", id, Find::Item, 300.0);
                        ui.add_space(6.0);
                        grid(
                            ui,
                            cx,
                            &base,
                            &[
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
                    } else if !known(ui, cx, &f.name, &f.kind, path.clone(), w) {
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

/// The item qualities, as the client numbers them.
const QUALITIES: &[&str] = &[
    "0 · Poor",
    "1 · Common",
    "2 · Uncommon",
    "3 · Rare",
    "4 · Epic",
    "5 · Legendary",
    "6 · Artifact",
    "7 · Heirloom",
];

/// Where an item is worn (`C_Item.GetItemInventoryTypeByID`'s tokens).
const EQUIP_LOCS: &[&str] = &[
    "INVTYPE_HEAD",
    "INVTYPE_NECK",
    "INVTYPE_SHOULDER",
    "INVTYPE_BODY",
    "INVTYPE_CHEST",
    "INVTYPE_ROBE",
    "INVTYPE_WAIST",
    "INVTYPE_LEGS",
    "INVTYPE_FEET",
    "INVTYPE_WRIST",
    "INVTYPE_HAND",
    "INVTYPE_FINGER",
    "INVTYPE_TRINKET",
    "INVTYPE_CLOAK",
    "INVTYPE_WEAPON",
    "INVTYPE_SHIELD",
    "INVTYPE_2HWEAPON",
    "INVTYPE_WEAPONMAINHAND",
    "INVTYPE_WEAPONOFFHAND",
    "INVTYPE_HOLDABLE",
    "INVTYPE_RANGED",
    "INVTYPE_THROWN",
    "INVTYPE_RANGEDRIGHT",
    "INVTYPE_RELIC",
    "INVTYPE_TABARD",
    "INVTYPE_BAG",
    "INVTYPE_AMMO",
    "INVTYPE_QUIVER",
];

/// A field of a schema's form that has a control of its own (S065): any
/// number a stepper (a quality a select, a level up to the flavor's
/// highest, a count with presets, seconds by halves), an item's slot a
/// select. False: the plain field draws it.
fn known(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    name: &str,
    kind: &Kind,
    path: Vec<Key>,
    width: f32,
) -> bool {
    let lower = name.to_ascii_lowercase();
    if let Some(what) = find_of(name)
        && *kind == Kind::Number
    {
        find(ui, cx, "", path, what, width);
        return true;
    }
    if (lower == "equiploc" || lower == "inventorytype") && *kind == Kind::Text {
        let now = at(cx.root, &path)
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_owned();
        let mut list: Vec<&str> = vec!["(none)"];
        list.extend(EQUIP_LOCS);
        let mut index = list.iter().position(|l| *l == now).unwrap_or(0);
        let opts: Vec<(&str, bool)> = list.iter().map(|l| (*l, true)).collect();
        if forms::select(ui, &path_of(&path), &opts, &mut index, width) {
            cx.set(
                path,
                (index > 0).then(|| Json::String(list[index].to_owned())),
            );
        }
        return true;
    }
    if *kind != Kind::Number {
        return false;
    }
    let now = at(cx.root, &path).and_then(Json::as_f64);
    if lower == "quality" {
        let mut index = now.map_or(1, |q| q.clamp(0.0, 7.0) as usize);
        let opts: Vec<(&str, bool)> = QUALITIES.iter().map(|q| (*q, true)).collect();
        if forms::select(ui, &path_of(&path), &opts, &mut index, width) {
            cx.set(path, Some(number(index as f64)));
        }
        return true;
    }
    let before = now.unwrap_or(0.0);
    let mut v = before;
    let id = path_of(&path);
    let changed = if lower.contains("level") && !lower.contains("item") {
        let max = f64::from(crate::flavors::max_level(cx.look.flavor));
        controls::stepper(ui, &id, &mut v, Some(1.0), Some(max), 1.0, "")
    } else if lower.contains("count") || lower.contains("stack") {
        let mut changed = controls::stepper(ui, &id, &mut v, Some(0.0), None, 1.0, "");
        if let Some(p) = controls::presets(ui, &[("1", 1.0), ("20", 20.0), ("200", 200.0)], v) {
            v = p;
            changed = true;
        }
        changed
    } else if [
        "delay",
        "duration",
        "cooldown",
        "remaining",
        "time",
        "seconds",
    ]
    .iter()
    .any(|w| lower.contains(w))
    {
        controls::stepper(ui, &id, &mut v, Some(0.0), None, 0.5, "s")
    } else {
        // (a whole number steps by one, a fraction by a tenth)
        let step = if before.fract() == 0.0 { 1.0 } else { 0.1 };
        controls::stepper(ui, &id, &mut v, None, None, step, "")
    };
    if changed && (v - before).abs() > 1e-9 {
        cx.set(path, Some(number(v)));
    }
    true
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

/// A key as the client names it in a binding: `ALT-CTRL-SHIFT-F`.
fn binding_key(key: egui::Key, m: egui::Modifiers) -> Option<String> {
    use egui::Key as K;
    let name = match key {
        K::Space => "SPACE".to_owned(),
        K::Escape => return None,
        K::Enter => "ENTER".to_owned(),
        K::Tab => "TAB".to_owned(),
        K::Backspace => "BACKSPACE".to_owned(),
        K::Delete => "DELETE".to_owned(),
        K::Insert => "INSERT".to_owned(),
        K::Home => "HOME".to_owned(),
        K::End => "END".to_owned(),
        K::PageUp => "PAGEUP".to_owned(),
        K::PageDown => "PAGEDOWN".to_owned(),
        K::ArrowUp => "UP".to_owned(),
        K::ArrowDown => "DOWN".to_owned(),
        K::ArrowLeft => "LEFT".to_owned(),
        K::ArrowRight => "RIGHT".to_owned(),
        other => other.name().to_ascii_uppercase(),
    };
    let mut out = String::new();
    if m.alt {
        out.push_str("ALT-");
    }
    if m.ctrl {
        out.push_str("CTRL-");
    }
    if m.shift {
        out.push_str("SHIFT-");
    }
    if m.mac_cmd {
        out.push_str("META-");
    }
    out.push_str(&name);
    Some(out)
}

/// The bindings (S065): each a recorded key and an action picked from the
/// flavor's client (Blizzard's UI's bindings), a new one by pressing it.
fn bindings(ui: &mut Ui, cx: &mut Cx<'_>, base: &[Key], value: &Json) {
    let p = palette(ui.ctx());
    // (the actions read once per flavor)
    let id = egui::Id::unique(("wowlua-studio-binding-actions", cx.look.flavor.to_owned()));
    let actions: std::sync::Arc<Vec<(String, String)>> = match ui.data(|d| d.get_temp(id)) {
        Some(a) => a,
        None => {
            let a = std::sync::Arc::new(cx.system.binding_actions(cx.look.flavor));
            ui.data_mut(|d| d.insert_temp(id, a.clone()));
            a
        }
    };
    let labels: Vec<String> = actions
        .iter()
        .map(|(n, c)| {
            if c.is_empty() {
                n.clone()
            } else {
                format!("{n} · {c}")
            }
        })
        .collect();
    let entries: Vec<(String, String)> = value
        .as_object()
        .map(|o| {
            o.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_owned()))
                .collect()
        })
        .unwrap_or_default();
    for (key, action) in &entries {
        let mut path = base.to_vec();
        path.push(name(key));
        forms::bar(ui, |ui| {
            cell(
                ui,
                220.0,
                egui::RichText::new(key)
                    .font(Face::PlexMedium.at(12.0))
                    .color(p.ink),
            );
            if actions.is_empty() {
                field(ui, cx, path.clone(), Kind::Text, 260.0, "MOVEFORWARD");
            } else {
                let mut names: Vec<&str> = vec!["(none)"];
                names.extend(labels.iter().map(String::as_str));
                let mut index = actions
                    .iter()
                    .position(|(n, _)| n == action)
                    .map_or(0, |i| i + 1);
                let opts: Vec<(&str, bool)> = names.iter().map(|n| (*n, true)).collect();
                if forms::select(ui, &path_of(&path), &opts, &mut index, 300.0) {
                    let v = actions
                        .get(index.wrapping_sub(1))
                        .map(|(n, _)| Json::String(n.clone()));
                    cx.set(path.clone(), v.or_else(|| Some(json!(""))));
                }
            }
            if widgets::button(ui, ButtonKind::Tertiary, "remove", true, None).clicked() {
                cx.set(path.clone(), None);
            }
        });
    }
    if actions.is_empty() {
        caption(
            ui,
            "The client's actions show once its Blizzard UI is fetched (the start page).",
        );
    }
    // A new binding: press its key.
    let recording_id = egui::Id::unique(("wowlua-studio-binding-recording", base.len()));
    let recording: bool = ui.data(|d| d.get_temp(recording_id)).unwrap_or(false);
    if recording {
        let pressed = ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        ui.horizontal(|ui| {
            caption(ui, "press the key, with its modifiers (escape: cancel)…");
            if widgets::button(ui, ButtonKind::Tertiary, "cancel", true, None).clicked() {
                ui.data_mut(|d| d.insert_temp(recording_id, false));
            }
        });
        if let Some((key, modifiers)) = pressed {
            ui.data_mut(|d| d.insert_temp(recording_id, false));
            if let Some(k) = binding_key(key, modifiers) {
                let mut path = base.to_vec();
                path.push(name(&k));
                cx.set(path, Some(json!("")));
            }
        }
    } else if widgets::button(ui, ButtonKind::Secondary, "record a key", true, None).clicked() {
        ui.data_mut(|d| d.insert_temp(recording_id, true));
    }
}

/// The CVars (S065): each set one with its value and the client's default,
/// a new one found by name among the flavor's (wowlua's recorded ones),
/// starting at its default; typed by name where wowlua has none.
fn cvars(ui: &mut Ui, cx: &mut Cx<'_>, base: &[Key], value: &Json, kind: &Kind) {
    let p = palette(ui.ctx());
    // (the CVars read once per flavor)
    let id = egui::Id::unique(("wowlua-studio-cvars", cx.look.flavor.to_owned()));
    let known: std::sync::Arc<Vec<crate::system::CVarInfo>> = match ui.data(|d| d.get_temp(id)) {
        Some(c) => c,
        None => {
            let c = std::sync::Arc::new(cx.system.cvars(cx.look.flavor));
            ui.data_mut(|d| d.insert_temp(id, c.clone()));
            c
        }
    };
    if known.len() <= 1 {
        return map_rows(ui, cx, base, value, kind, ("CVar", "value", "+ CVar"));
    }
    let info = |n: &str| known.iter().find(|c| c.name.eq_ignore_ascii_case(n));
    let entries: Vec<String> = value
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    for k in &entries {
        let mut path = base.to_vec();
        path.push(name(k));
        let cvar = info(k);
        forms::bar(ui, |ui| {
            cell(
                ui,
                260.0,
                egui::RichText::new(k)
                    .font(Face::PlexMedium.at(12.0))
                    .color(p.ink),
            );
            field(ui, cx, path.clone(), kind.clone(), 160.0, "");
            let note = match cvar {
                Some(c) if c.read_only => format!("default {} · read-only", c.default),
                Some(c) if c.secure => {
                    format!("default {} · secure: addons can't set it", c.default)
                }
                Some(c) => format!("default {}", c.default),
                None => "not one of the client's".to_owned(),
            };
            ui.label(
                egui::RichText::new(note)
                    .font(Face::Plex.at(11.5))
                    .color(p.ink_muted),
            );
            if widgets::button(ui, ButtonKind::Tertiary, "remove", true, None).clicked() {
                cx.set(path.clone(), None);
            }
        });
    }
    // A new one: found by name.
    let search = format!("{}/find", path_of(base));
    let typed = {
        let buffer = cx.buffers.entry(search.clone()).or_default();
        forms::text_input(ui, buffer, 260.0, "find a CVar…");
        buffer.trim().to_ascii_lowercase()
    };
    if !typed.is_empty() {
        let found: Vec<&crate::system::CVarInfo> = known
            .iter()
            .filter(|c| c.name.to_ascii_lowercase().contains(&typed))
            .filter(|c| !entries.iter().any(|e| e.eq_ignore_ascii_case(&c.name)))
            .take(12)
            .collect();
        if found.is_empty() {
            caption(ui, "No CVar of this flavor's client has that in its name.");
        }
        let names: Vec<String> = found.iter().map(|c| c.name.clone()).collect();
        if let Some(picked) = controls::suggestions(ui, &names)
            && let Some(c) = info(&picked)
        {
            let mut path = base.to_vec();
            path.push(name(&c.name));
            let v = parse(kind, &c.default)
                .ok()
                .flatten()
                .unwrap_or_else(|| json!(c.default));
            cx.set(path, Some(v));
            cx.buffers.insert(search, String::new());
        }
    }
    caption(
        ui,
        &format!(
            "The {} CVars of this flavor's client, with their defaults as wowlua recorded them.",
            known.len()
        ),
    );
}

/// Keys and values (`cvars`, `bindings`, `stats`).
fn map(ui: &mut Ui, cx: &mut Cx<'_>, base: &[Key], value: &Json, kind: &Kind, part: &str) {
    if part == "bindings" {
        return bindings(ui, cx, base, value);
    }
    if part == "cvars" {
        return cvars(ui, cx, base, value, kind);
    }
    let heads = match part {
        "stats" => ("stat", "value", "+ stat"),
        _ => ("key", "value", "+ entry"),
    };
    map_rows(ui, cx, base, value, kind, heads);
}

/// Keys and values as typed: a key's name, its value.
fn map_rows(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    base: &[Key],
    value: &Json,
    kind: &Kind,
    (key_head, value_head, word): (&str, &str, &str),
) {
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

/// Where a unit is and what it does (S080; wowlua T515 to T518): its
/// place on the map (typed, or placed in the game view), how it moves
/// (its patrol's waypoints drawn there too) and its actions.
fn where_and_what(ui: &mut Ui, cx: &mut Cx<'_>, key: &Key, data: &Json) {
    let label = data["name"].as_str().unwrap_or("the unit").to_owned();
    let id = path_of(&unit_path(key, ""));
    ui.add_space(10.0);
    forms::label(ui, "where it is and what it does");
    ui.add_space(4.0);
    // Its place: north, west and up yards on the map.
    ui.horizontal(|ui| {
        for (axis, hint) in [("y", "north"), ("x", "west"), ("z", "up")] {
            ui.vertical(|ui| {
                forms::label(ui, hint);
                let mut path = unit_path(key, "world");
                path.push(name(axis));
                field(ui, cx, path, Kind::Number, 96.0, "—");
            });
        }
        ui.vertical(|ui| {
            forms::label(ui, " ");
            ui.horizontal(|ui| {
                if widgets::button(
                    ui,
                    ButtonKind::Secondary,
                    "place it in the game view",
                    true,
                    None,
                )
                .clicked()
                {
                    *cx.place = Some(super::Placing {
                        profile: String::new(),
                        unit: key.clone(),
                        label: label.clone(),
                        path: false,
                        movement: data.get("movement").cloned(),
                    });
                }
                if data.get("world").is_some() && forms::text_button(ui, "clear ×").clicked() {
                    cx.set(unit_path(key, "world"), None);
                }
            });
        });
    });
    hint(
        ui,
        "or only a distance, as before: it stands that far in front of you",
    );
    ui.add_space(8.0);
    // How it moves.
    let movement = data.get("movement");
    let kind = movement
        .and_then(|m| m["kind"].as_str())
        .unwrap_or("stand")
        .to_owned();
    let kinds = ["stand", "patrol", "follow", "approach", "flee"];
    ui.horizontal(|ui| {
        forms::label(ui, "movement");
        let at_kind = kinds.iter().position(|k| *k == kind).unwrap_or(0);
        if let Some(i) = widgets::seg(ui, &format!("{id}movement"), &kinds, at_kind, false) {
            if i == 0 {
                cx.set(unit_path(key, "movement"), None);
            } else {
                let mut path = unit_path(key, "movement");
                path.push(name("kind"));
                cx.set(path, Some(json!(kinds[i])));
            }
        }
    });
    let mpath = |k: &str| {
        let mut p = unit_path(key, "movement");
        p.push(name(k));
        p
    };
    match kind.as_str() {
        "patrol" => {
            ui.add_space(4.0);
            let points = movement
                .and_then(|m| m.get("path"))
                .map(list)
                .unwrap_or_default();
            ui.horizontal_wrapped(|ui| {
                forms::label(ui, &format!("path · {} waypoints", points.len()));
                for (k, p) in &points {
                    let text = format!(
                        "{:.1}, {:.1}",
                        p["y"].as_f64().unwrap_or(0.0),
                        p["x"].as_f64().unwrap_or(0.0)
                    );
                    if forms::text_button(ui, &format!("{text} ×")).clicked() {
                        let mut path = mpath("path");
                        path.push(k.clone());
                        cx.set(path, None);
                    }
                }
                if widgets::button(
                    ui,
                    ButtonKind::Secondary,
                    "draw in the game view",
                    true,
                    None,
                )
                .clicked()
                {
                    *cx.place = Some(super::Placing {
                        profile: String::new(),
                        unit: key.clone(),
                        label: label.clone(),
                        path: true,
                        movement: movement.cloned(),
                    });
                }
            });
            ui.horizontal(|ui| {
                check(ui, cx, "loops", mpath("loop"), true);
                speed(ui, cx, mpath("speed"), "2.5");
            });
        }
        "follow" | "approach" | "flee" => {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if kind != "flee" {
                    stepped(
                        ui,
                        cx,
                        "keeps",
                        mpath("keep"),
                        180.0,
                        (Some(0.0), None),
                        "yd",
                        &[("melee 2".to_owned(), 2.0), ("10".to_owned(), 10.0)],
                        2.0,
                    );
                }
                speed(ui, cx, mpath("speed"), "7");
            });
            if kind == "follow" {
                hint(
                    ui,
                    "it walks after you when you move, so your range checks change as you play",
                );
            }
        }
        _ => {}
    }
    ui.add_space(8.0);
    // What it does.
    forms::label(ui, "actions");
    hint(
        ui,
        "seconds after the session starts, every so many seconds, or when you come near",
    );
    let actions = data.get("actions").map(list).unwrap_or_default();
    for (k, a) in &actions {
        let apath = |f: &str| {
            let mut p = unit_path(key, "actions");
            p.push(k.clone());
            if !f.is_empty() {
                p.push(name(f));
            }
            p
        };
        ui.horizontal(|ui| {
            let whens = ["at", "every", "within"];
            let when = whens.iter().position(|w| a.get(*w).is_some()).unwrap_or(0);
            if let Some(i) = widgets::seg(
                ui,
                &format!("{}when", path_of(&apath(""))),
                &whens,
                when,
                false,
            ) && i != when
            {
                let value = a.get(whens[when]).cloned().unwrap_or(json!(0));
                cx.set(apath(whens[when]), None);
                cx.set(apath(whens[i]), Some(value));
            }
            field(ui, cx, apath(whens[when]), Kind::Number, 64.0, "0");
            ui.label(match whens[when] {
                "within" => "yd",
                _ => "s",
            });
            field(
                ui,
                cx,
                apath("do"),
                Kind::Choice(["attack", "cast", "face"].map(str::to_owned).to_vec()),
                110.0,
                "attack",
            );
            match a["do"].as_str().unwrap_or("attack") {
                "attack" => {
                    forms::label(ui, "damage");
                    field(ui, cx, apath("damage"), Kind::Number, 64.0, "10");
                    forms::label(ui, "every");
                    field(ui, cx, apath("speed"), Kind::Number, 56.0, "2");
                    ui.label("s");
                }
                "cast" => {
                    forms::label(ui, "spell");
                    field(ui, cx, apath("spell"), Kind::Number, 80.0, "ID");
                    forms::label(ui, "cast time");
                    field(ui, cx, apath("castTime"), Kind::Number, 56.0, "its own");
                    ui.label("s");
                }
                _ => {}
            }
            if forms::text_button(ui, "×").clicked() {
                cx.set(apath(""), None);
            }
        });
    }
    if forms::text_button(ui, "+ action").clicked() {
        let new = json!({ "at": 0, "do": "attack", "damage": 10 });
        let mut path = unit_path(key, "actions");
        if data.get("actions").is_some() {
            path.push(Key::Index(actions.len()));
            cx.set(path, Some(new));
        } else {
            cx.set(path, Some(json!([new])));
        }
    }
}

/// A line of help under a field, in the muted ink.
fn hint(ui: &mut Ui, text: &str) {
    let ink = palette(ui.ctx()).ink_muted;
    forms::paragraph(ui, text, 12.0, 18.0, ink);
}

/// A walk's speed (yards a second, fractions too), its default `hint`,
/// with a walk's and a run's (wowlua T517).
fn speed(ui: &mut Ui, cx: &mut Cx<'_>, path: Vec<Key>, hint: &str) {
    ui.vertical(|ui| {
        forms::label(ui, "speed");
        ui.horizontal(|ui| {
            field(ui, cx, path.clone(), Kind::Number, 64.0, hint);
            ui.label("yd/s");
            for (label, v) in [("walk 2.5", 2.5), ("run 7", 7.0)] {
                if forms::text_button(ui, label).clicked() {
                    cx.set(path.clone(), Some(json!(v)));
                }
            }
        });
    });
}

/// What a unit is (S082; wowlua T530, T531): the creature (searched among
/// those the installation knows, its model previewed), how it fights,
/// what it is to the player (a vendor, a trainer, gossip, a service) and
/// what it drops.
fn creature_and_role(ui: &mut Ui, cx: &mut Cx<'_>, key: &Key, data: &Json) {
    let p = palette(ui.ctx());
    let id = path_of(&unit_path(key, ""));
    // The creature.
    ui.add_space(12.0);
    forms::label(ui, "the creature");
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        // Its model, as a model frame draws it; a browser of others.
        let display = data
            .get("displayId")
            .or(data.get("displayID"))
            .and_then(Json::as_u64)
            .map(|d| d as u32);
        ui.vertical(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(132.0), Sense::hover());
            ui.painter().rect_filled(rect, CornerRadius::same(4), p.bg_000);
            ui.painter()
                .rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0, p.line_strong), egui::StrokeKind::Inside);
            match display.and_then(|d| cx.look.pic("", Some(Art::Model(d))).tex) {
                Some(tex) => {
                    ui.painter().image(
                        tex,
                        rect.shrink(1.0),
                        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
                None => {
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        match display {
                            Some(d) => format!("model {d}"),
                            None => "no model".to_owned(),
                        },
                        Face::Plex.at(11.5),
                        p.ink_muted,
                    );
                }
            }
            let browse_id = egui::Id::unique(("wowlua-studio-models", id.clone()));
            let mut browsing: bool = ui.data(|d| d.get_temp(browse_id)).unwrap_or(false);
            if forms::text_button(ui, "browse models…").clicked() {
                browsing = !browsing;
            }
            ui.data_mut(|d| d.insert_temp(browse_id, browsing));
        });
        ui.vertical(|ui| {
            if let Some(row) = find(ui, cx, "creature", unit_path(key, "npcID"), Find::Creature, 380.0) {
                // (the creature picked: its name, model, type and
                // classification as the installation knows them)
                cx.set(unit_path(key, "name"), Some(json!(row.name)));
                if let Some(d) = row.icon {
                    cx.set(unit_path(key, "displayId"), Some(json!(d)));
                }
                let mut parts = row.text.split(" · ");
                if let Some(kind) = parts.next().filter(|k| *k != "—") {
                    cx.set(unit_path(key, "creatureType"), Some(json!(kind)));
                }
                if let Some(class) = parts.next().filter(|c| *c != "—") {
                    cx.set(unit_path(key, "classification"), Some(json!(class)));
                }
                if let Some(family) = parts.next().filter(|f| *f != "—") {
                    cx.set(unit_path(key, "creatureFamily"), Some(json!(family)));
                }
            }
            hint(
                ui,
                "the creatures your installation knows: its creature table and cache (the cache grows as you play)",
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    forms::label(ui, "model");
                    field(ui, cx, unit_path(key, "displayId"), Kind::Number, 96.0, "display ID");
                });
                choice(
                    ui,
                    cx,
                    "type",
                    unit_path(key, "creatureType"),
                    &[
                        "Beast", "Humanoid", "Undead", "Elemental", "Dragonkin", "Demon", "Giant",
                        "Mechanical", "Critter",
                    ],
                    130.0,
                );
                ui.vertical(|ui| {
                    forms::label(ui, "family");
                    field(ui, cx, unit_path(key, "creatureFamily"), Kind::Text, 110.0, "Wolf");
                });
                choice(ui, cx, "faction", unit_path(key, "faction"), &["Alliance", "Horde"], 120.0);
            });
        });
    });
    // The model browser: display IDs from the one shown, a page at a time.
    let browse_id = egui::Id::unique(("wowlua-studio-models", id.clone()));
    if ui.data(|d| d.get_temp::<bool>(browse_id)).unwrap_or(false) {
        let page_id = egui::Id::unique(("wowlua-studio-models-page", id.clone()));
        let base = data
            .get("displayId")
            .or(data.get("displayID"))
            .and_then(Json::as_u64)
            .unwrap_or(1) as u32;
        let mut page: u32 = ui.data(|d| d.get_temp(page_id)).unwrap_or(0);
        ui.horizontal(|ui| {
            if forms::text_button(ui, "‹ earlier").clicked() {
                page = page.saturating_sub(1);
            }
            let first = base.saturating_add(page * 8);
            hint(ui, &format!("models {first} to {}", first + 7));
            if forms::text_button(ui, "later ›").clicked() {
                page += 1;
            }
        });
        ui.data_mut(|d| d.insert_temp(page_id, page));
        ui.horizontal_wrapped(|ui| {
            let first = base.saturating_add(page * 8);
            for d in first..first + 8 {
                let (rect, r) = ui.allocate_exact_size(Vec2::splat(76.0), Sense::click());
                let stroke = if r.hovered() {
                    p.amber_ink
                } else {
                    p.line_strong
                };
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(3), p.bg_000);
                ui.painter().rect_stroke(
                    rect,
                    CornerRadius::same(3),
                    Stroke::new(1.0, stroke),
                    egui::StrokeKind::Inside,
                );
                if let Some(tex) = cx.look.pic("", Some(Art::Model(d))).tex {
                    ui.painter().image(
                        tex,
                        rect.shrink(1.0),
                        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
                ui.painter().text(
                    rect.left_bottom() + Vec2::new(4.0, -4.0),
                    Align2::LEFT_BOTTOM,
                    d.to_string(),
                    Face::Plex.at(10.5),
                    p.ink_muted,
                );
                if r.clicked() {
                    cx.set(unit_path(key, "displayId"), Some(json!(d)));
                }
            }
        });
    }

    // How it fights.
    ui.add_space(12.0);
    forms::label(ui, "combat");
    hint(ui, "what the API shows addons of it, and how hard it hits");
    ui.horizontal(|ui| {
        let attack = |k: &str| {
            let mut p = unit_path(key, "attack");
            p.push(name(k));
            p
        };
        ui.vertical(|ui| {
            forms::label(ui, "damage a swing");
            field(ui, cx, attack("damage"), Kind::Number, 96.0, "10");
        });
        ui.vertical(|ui| {
            forms::label(ui, "every (s)");
            field(ui, cx, attack("speed"), Kind::Number, 96.0, "2");
        });
        ui.vertical(|ui| {
            forms::label(ui, "your threat (%)");
            field(ui, cx, unit_path(key, "threat"), Kind::Number, 96.0, "—");
        });
        ui.vertical(|ui| {
            forms::label(ui, "its spells");
            hint(ui, "cast by its actions above");
        });
    });

    // What it is to the player.
    ui.add_space(12.0);
    forms::label(ui, "what it is to you");
    hint(ui, "right-click it in play mode to talk to it");
    let roles = [
        "none",
        "vendor",
        "class trainer",
        "gossip",
        "banker",
        "mailbox",
        "auctioneer",
        "flight master",
    ];
    let services = ["banker", "mailbox", "auctioneer", "flightmaster"];
    let role = if data.get("gossip").is_some() {
        3
    } else if data.get("trainer").is_some() {
        2
    } else if data.get("vendor").is_some() {
        1
    } else {
        match data["service"].as_str() {
            Some(s) => services.iter().position(|x| *x == s).map_or(0, |i| i + 4),
            None => 0,
        }
    };
    if let Some(i) = widgets::seg(ui, &format!("{id}role"), &roles, role, false)
        && i != role
    {
        for k in ["vendor", "trainer", "gossip", "service"] {
            if data.get(k).is_some() {
                cx.set(unit_path(key, k), None);
            }
        }
        match i {
            1 => cx.set(
                unit_path(key, "vendor"),
                Some(json!({ "items": [], "repairs": false })),
            ),
            2 => cx.set(
                unit_path(key, "trainer"),
                Some(json!({ "class": "WARRIOR" })),
            ),
            3 => cx.set(
                unit_path(key, "gossip"),
                Some(json!({ "text": "Greetings.", "options": [] })),
            ),
            4..=7 => cx.set(unit_path(key, "service"), Some(json!(services[i - 4]))),
            _ => {}
        }
    }
    ui.add_space(6.0);
    let sub = |k: &str, rest: &[Key]| {
        let mut p = unit_path(key, k);
        p.extend(rest.iter().cloned());
        p
    };
    match role {
        1 => {
            forms::label(ui, "it sells · its window opens when you talk to it");
            let items = data["vendor"].get("items").map(list).unwrap_or_default();
            for (k, item) in &items {
                ui.horizontal(|ui| {
                    find(
                        ui,
                        cx,
                        "",
                        sub("vendor", &[name("items"), k.clone(), name("id")]),
                        Find::Item,
                        260.0,
                    );
                    ui.vertical(|ui| {
                        forms::label(ui, "price · copper");
                        field(
                            ui,
                            cx,
                            sub("vendor", &[name("items"), k.clone(), name("price")]),
                            Kind::Number,
                            80.0,
                            "the item's",
                        );
                    });
                    ui.vertical(|ui| {
                        forms::label(ui, "stack");
                        field(
                            ui,
                            cx,
                            sub("vendor", &[name("items"), k.clone(), name("stack")]),
                            Kind::Number,
                            56.0,
                            "1",
                        );
                    });
                    ui.vertical(|ui| {
                        forms::label(ui, "limited");
                        field(
                            ui,
                            cx,
                            sub("vendor", &[name("items"), k.clone(), name("available")]),
                            Kind::Number,
                            56.0,
                            "—",
                        );
                    });
                    let _ = item;
                    if forms::text_button(ui, "×").clicked() {
                        cx.set(sub("vendor", &[name("items"), k.clone()]), None);
                    }
                });
            }
            ui.horizontal(|ui| {
                if forms::text_button(ui, "+ item").clicked() {
                    let path = sub("vendor", &[name("items"), Key::Index(items.len())]);
                    cx.set(path, Some(json!({ "id": 159 })));
                }
                check(ui, cx, "repairs", sub("vendor", &[name("repairs")]), false);
            });
        }
        2 => {
            ui.horizontal(|ui| {
                let classes: Vec<&str> = classes(cx.look.flavor).iter().map(|(f, _)| *f).collect();
                choice(
                    ui,
                    cx,
                    "the class it trains",
                    sub("trainer", &[name("class")]),
                    &classes,
                    180.0,
                );
                hint(ui, "its services as the client lists them for the class");
            });
        }
        3 => {
            ui.vertical(|ui| {
                forms::label(ui, "its text");
                field(
                    ui,
                    cx,
                    sub("gossip", &[name("text")]),
                    Kind::Text,
                    420.0,
                    "Greetings.",
                );
            });
            let options = data["gossip"].get("options").map(list).unwrap_or_default();
            for (k, _) in &options {
                ui.horizontal(|ui| {
                    field(
                        ui,
                        cx,
                        sub("gossip", &[name("options"), k.clone(), name("name")]),
                        Kind::Text,
                        260.0,
                        "an option",
                    );
                    field(
                        ui,
                        cx,
                        sub("gossip", &[name("options"), k.clone(), name("opens")]),
                        Kind::Choice(
                            ["close", "vendor", "trainer", "taxi"]
                                .map(str::to_owned)
                                .to_vec(),
                        ),
                        120.0,
                        "close",
                    );
                    if forms::text_button(ui, "×").clicked() {
                        cx.set(sub("gossip", &[name("options"), k.clone()]), None);
                    }
                });
            }
            if forms::text_button(ui, "+ option").clicked() {
                let path = sub("gossip", &[name("options"), Key::Index(options.len())]);
                cx.set(
                    path,
                    Some(json!({ "name": "Tell me more.", "opens": "close" })),
                );
            }
            hint(
                ui,
                "an option that opens vendor or trainer opens this unit's own; give it those too",
            );
        }
        4 => hint(ui, "a banker: talking to it opens your bank"),
        5 => hint(ui, "a mailbox: talking to it opens your inbox"),
        6 => hint(ui, "an auctioneer: talking to it opens the auction house"),
        7 => hint(ui, "a flight master: talking to it offers the flight map"),
        _ => {}
    }

    // What it drops.
    ui.add_space(12.0);
    forms::label(ui, "loot");
    hint(ui, "what its corpse holds when you kill it");
    let loot = data.get("loot").map(list).unwrap_or_default();
    for (k, _) in &loot {
        ui.horizontal(|ui| {
            find(
                ui,
                cx,
                "",
                sub("loot", &[k.clone(), name("id")]),
                Find::Item,
                260.0,
            );
            ui.vertical(|ui| {
                forms::label(ui, "count");
                field(
                    ui,
                    cx,
                    sub("loot", &[k.clone(), name("count")]),
                    Kind::Number,
                    56.0,
                    "1",
                );
            });
            if forms::text_button(ui, "×").clicked() {
                cx.set(sub("loot", &[k.clone()]), None);
            }
        });
    }
    if forms::text_button(ui, "+ item").clicked() {
        let new = json!({ "id": 159, "count": 1 });
        if data.get("loot").is_some() {
            cx.set(sub("loot", &[Key::Index(loot.len())]), Some(new));
        } else {
            cx.set(unit_path(key, "loot"), Some(json!([new])));
        }
    }
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
        assert_eq!(classes("tbc").len(), 9);
        assert_eq!(classes("mists").len(), 11);
        assert!(races("mists").contains(&"Pandaren") && !races("mists").contains(&"Nightborne"));
        assert_eq!(classes("standard").len(), 13);
        assert!(races("vanilla").len() == 8 && races("tbc").contains(&"Draenei"));
    }
}
