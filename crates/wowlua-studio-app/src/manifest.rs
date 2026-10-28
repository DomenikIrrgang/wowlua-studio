//! The `wowlua.json` view of the settings page (M10, S062): a package's
//! manifest by section, the designed rows, then every other key in a form
//! from wowlua's schema; the json view the same document (the editor's).
//! An edit changes only that value's text, keeping the file's order.

use std::collections::HashMap;
use std::path::PathBuf;

use egui::Ui;
use serde_json::Value as Json;
use wowlua_studio_ui::forms::{self, CalloutKind};
use wowlua_studio_ui::widgets::{self, ButtonKind};
use wowlua_studio_ui::{Face, Palette, palette};

use crate::editor::Editors;
use crate::editor::doc::EditKind;
use crate::system::{System, Workspace};
use crate::world::json::{self, Key};
use crate::world::pos_of;
use crate::world::schema::{self, Kind};

/// The sections, as the settings page lists them.
pub const SECTIONS: &[(&str, &str)] = &[
    (
        "package",
        "The package: its name, version and the wowlua it needs.",
    ),
    (
        "addon",
        "The AddOn and its TOC: wowlua.json generates the TOC.",
    ),
    ("flavors", "The clients the package supports."),
    (
        "dependencies",
        "Libraries embedded into the build, and other AddOns the TOC names.",
    ),
    (
        "build",
        "What goes into a build. Build profiles are on the ship page.",
    ),
    ("check", "How wowlua checks the sources."),
    ("fmt", "wowlua fmt and format on save."),
    ("test", "wowlua test, and the test explorer's defaults."),
    ("publish", "Where wowlua publish uploads."),
    ("workspace", "The packages of this workspace."),
    (
        "other keys",
        "Every other key of wowlua.json, as its schema describes it.",
    ),
];

/// The section names, for the settings page's list.
pub fn section_names() -> Vec<&'static str> {
    SECTIONS.iter().map(|(n, _)| *n).collect()
}

/// How a designed row is edited.
#[derive(Clone)]
enum How {
    Text,
    Number,
    Flag,
    Choice(&'static [&'static str]),
    /// The values a schema lists.
    Values(Vec<String>),
    /// Strings, written `a, b`.
    List,
    /// Any JSON, in a box.
    Json,
    /// A link to another page.
    Link(Go),
}

/// The pages a row leads to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Go {
    Dependencies,
    Ship,
}

struct Row {
    section: &'static str,
    path: &'static [&'static str],
    desc: &'static str,
    how: How,
}

fn rows() -> Vec<Row> {
    use How::*;
    let r = |section, path, desc, how| Row {
        section,
        path,
        desc,
        how,
    };
    vec![
        r(
            "package",
            &["package", "name"],
            "Package ID: lowercase [a-z0-9-], 2–64 characters.",
            Text,
        ),
        r(
            "package",
            &["package", "version"],
            "SemVer 2.0. A -beta.N or -alpha.N suffix publishes as a beta or alpha file.",
            Text,
        ),
        r(
            "package",
            &["package", "wowlua"],
            "Required compiler version (semver requirement).",
            Text,
        ),
        r(
            "package",
            &["package", "kind"],
            "What the package is.",
            Choice(&["addon", "library", "lua-library", "types"]),
        ),
        r("package", &["package", "description"], "", Text),
        r("package", &["package", "repository"], "", Text),
        r(
            "addon",
            &["addon", "name"],
            "AddOn folder and TOC name.",
            Text,
        ),
        r("addon", &["addon", "title"], "", Text),
        r("addon", &["addon", "notes"], "", Text),
        r("addon", &["addon", "icon-texture"], "", Text),
        r(
            "addon",
            &["addon", "saved-variables"],
            "Account-wide SavedVariables.",
            List,
        ),
        r(
            "addon",
            &["addon", "saved-variables-per-character"],
            "",
            List,
        ),
        r(
            "addon",
            &["addon", "default-state"],
            "",
            Choice(&["enabled", "disabled"]),
        ),
        r(
            "dependencies",
            &["dependencies"],
            "Search the registry, add, update and remove them on the dependencies page.",
            Link(Go::Dependencies),
        ),
        r(
            "dependencies",
            &["addon-dependencies"],
            "Other AddOns, written to the TOC; never fetched.",
            Json,
        ),
        r(
            "build",
            &["build", "assets"],
            "Files copied as they are.",
            List,
        ),
        r(
            "build",
            &["build", "flavor-mode"],
            "",
            Choice(&["split", "shared"]),
        ),
        r(
            "build",
            &["profiles"],
            "The build profiles: edited on the ship page.",
            Link(Go::Ship),
        ),
        r(
            "check",
            &["check", "mode"],
            "",
            Choice(&["nocheck", "basic", "strict"]),
        ),
        r(
            "check",
            &["check", "lints"],
            "Lints by code: allow, warn or deny.",
            Json,
        ),
        r("fmt", &["fmt", "indent"], "", Number),
        r("fmt", &["fmt", "line-width"], "", Number),
        r(
            "fmt",
            &["fmt", "quote-style"],
            "",
            Choice(&["double", "single", "preserve"]),
        ),
        r(
            "fmt",
            &["fmt", "exclude"],
            "Generated files fmt leaves as they are.",
            List,
        ),
        r(
            "test",
            &["test", "blizzard-ui"],
            "Load the flavor's Blizzard UI code as secure code before the addon.",
            Flag,
        ),
        r(
            "test",
            &["test", "profile"],
            "The world profile of test files that name none.",
            Text,
        ),
        r(
            "test",
            &["test", "platform"],
            "The machine the emulated client runs on.",
            Choice(&["mac", "windows"]),
        ),
        r(
            "test",
            &["test", "screen"],
            "width, height, dpiScale, uiScale",
            Json,
        ),
        r(
            "test",
            &["test", "coverage"],
            "Minimums by kind (lines, branches, functions, conditions) and exclude.",
            Json,
        ),
        r(
            "test",
            &["test", "reporters"],
            "pretty, junit, tap, json, github, lcov, cobertura, coverage-json, html",
            List,
        ),
        r(
            "publish",
            &["publish", "changelog"],
            "Path, \"git\", or null.",
            Text,
        ),
        r(
            "publish",
            &["publish", "release-type"],
            "",
            Choice(&["auto", "release", "beta", "alpha"]),
        ),
        r(
            "publish",
            &["publish", "targets"],
            "The targets and their tokens are on the ship page.",
            Link(Go::Ship),
        ),
        r("workspace", &["workspace", "members"], "", List),
    ]
}

/// The keys the designed sections show (the rest: other keys).
const DESIGNED: &[&str] = &[
    "$schema",
    "package",
    "addon",
    "flavors",
    "dependencies",
    "dev-dependencies",
    "addon-dependencies",
    "build",
    "check",
    "fmt",
    "test",
    "publish",
    "workspace",
    "profiles",
];

/// What the page asks of the rest of Studio.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Save(PathBuf),
    Go(Go),
}

#[derive(Default)]
pub struct ManifestState {
    /// The package's folder (none: the workspace's root).
    pub picked: Option<PathBuf>,
    pub json_view: bool,
    pub query: String,
    parsed: Option<(PathBuf, u64, Json)>,
    schema: Option<Option<Json>>,
    buffers: HashMap<String, String>,
    new_flavor: String,
}

impl ManifestState {
    /// The manifest the page edits.
    pub fn path(&self, workspace: &Workspace) -> PathBuf {
        self.picked
            .clone()
            .unwrap_or_else(|| workspace.root.clone())
            .join("wowlua.json")
    }
}

/// The edits a form makes: a path and its new value (none: removed).
struct Fx<'a> {
    root: &'a Json,
    changes: Vec<(Vec<Key>, Option<Json>, bool)>,
    buffers: &'a mut HashMap<String, String>,
}

impl Fx<'_> {
    fn get(&self, path: &[&str]) -> Option<&Json> {
        let mut v = self.root;
        for k in path {
            v = v.get(*k)?;
        }
        Some(v)
    }

    fn set(&mut self, path: &[&str], value: Option<Json>, typing: bool) {
        let keys = path.iter().map(|k| Key::Name((*k).to_owned())).collect();
        self.changes.push((keys, value, typing));
    }
}

fn shown(v: Option<&Json>, how: &How) -> String {
    match (v, how) {
        (None | Some(Json::Null), _) => String::new(),
        (Some(Json::String(s)), _) => s.clone(),
        (Some(Json::Array(a)), How::List) => a
            .iter()
            .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned))
            .collect::<Vec<_>>()
            .join(", "),
        (Some(v), How::Json) => serde_json::to_string_pretty(v).unwrap_or_default(),
        (Some(v), _) => v.to_string(),
    }
}

/// What typed text means; `Err`: not that kind yet (JSON being typed).
fn parse(text: &str, how: &How) -> Result<Option<Json>, ()> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(None);
    }
    Ok(Some(match how {
        How::Number => match t.parse::<f64>().ok().and_then(serde_json::Number::from_f64) {
            Some(n) if n.as_f64().is_some_and(|f| f.fract() == 0.0) => {
                Json::from(t.parse::<i64>().unwrap_or(0))
            }
            Some(n) => Json::Number(n),
            None => Json::String(t.to_owned()),
        },
        How::List => Json::Array(
            t.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| Json::String(s.to_owned()))
                .collect(),
        ),
        How::Json => serde_json::from_str(t).map_err(|_| ())?,
        _ => Json::String(text.to_owned()),
    }))
}

fn caption(ui: &mut Ui, text: &str, p: &Palette) {
    ui.label(
        egui::RichText::new(text)
            .font(Face::Plex.at(11.5))
            .color(p.ink_muted),
    );
}

/// A row: its key and path, its description, its control.
fn row(ui: &mut Ui, fx: &mut Fx<'_>, r: &Row, p: &Palette, go: &mut Option<Go>) {
    let width = ui.available_width();
    let control = 380.0_f32.min(width * 0.5);
    let key = r.path.last().copied().unwrap_or("");
    let path_text = r.path.join(".");
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(width - control - 16.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(key)
                        .font(Face::PlexMedium.at(12.5))
                        .color(p.ink),
                );
                ui.label(
                    egui::RichText::new(&path_text)
                        .font(Face::Plex.at(11.0))
                        .color(p.ink_faint),
                );
            });
            if !r.desc.is_empty() {
                caption(ui, r.desc, p);
            }
        });
        ui.vertical(|ui| {
            ui.set_width(control);
            edit_value(ui, fx, r.path, r.how.clone(), control, go);
        });
    });
    ui.add_space(4.0);
    let y = ui.cursor().top();
    ui.painter().line_segment(
        [
            egui::pos2(ui.min_rect().left(), y),
            egui::pos2(ui.min_rect().right(), y),
        ],
        egui::Stroke::new(1.0, p.line),
    );
    ui.add_space(6.0);
}

fn edit_value(
    ui: &mut Ui,
    fx: &mut Fx<'_>,
    path: &[&str],
    how: How,
    width: f32,
    go: &mut Option<Go>,
) {
    let now = fx.get(path).cloned();
    match how {
        How::Link(to) => {
            let text = match &now {
                Some(Json::Object(o)) => o.keys().cloned().collect::<Vec<_>>().join(", "),
                _ => String::new(),
            };
            ui.horizontal_wrapped(|ui| {
                if !text.is_empty() {
                    ui.label(egui::RichText::new(text).font(Face::Plex.at(11.5)));
                }
                let label = match to {
                    Go::Dependencies => "the dependencies page",
                    Go::Ship => "the ship page",
                };
                if widgets::button(ui, ButtonKind::Secondary, label, true, None).clicked() {
                    *go = Some(to);
                }
            });
        }
        How::Flag => {
            let mut on = now.as_ref().and_then(Json::as_bool).unwrap_or(false);
            let word = if on { "on" } else { "off" };
            if forms::checkbox(ui, &mut on, word, None) {
                fx.set(path, Some(Json::Bool(on)), false);
            }
        }
        How::Choice(_) | How::Values(_) => {
            let options: Vec<String> = match &how {
                How::Choice(o) => o.iter().map(|s| (*s).to_owned()).collect(),
                How::Values(v) => v.clone(),
                _ => Vec::new(),
            };
            let current = now.as_ref().and_then(Json::as_str).unwrap_or("").to_owned();
            let mut list: Vec<String> = vec!["(not set)".into()];
            list.extend(options);
            if !current.is_empty() && !list.contains(&current) {
                list.push(current.clone());
            }
            let mut index = list.iter().position(|o| *o == current).unwrap_or(0);
            let opts: Vec<(&str, bool)> = list.iter().map(|o| (o.as_str(), true)).collect();
            if forms::select(
                ui,
                &format!("mf-{}", path.join(".")),
                &opts,
                &mut index,
                width.min(240.0),
            ) {
                fx.set(
                    path,
                    (index > 0).then(|| Json::String(list[index].clone())),
                    false,
                );
            }
        }
        How::Text | How::Number | How::List | How::Json => {
            let text = shown(now.as_ref(), &how);
            let key = path.join(".");
            let buffer = fx.buffers.entry(key).or_insert_with(|| text.clone());
            let response = if matches!(how, How::Json) {
                ui.add(
                    egui::TextEdit::multiline(buffer)
                        .font(Face::Plex.at(12.0))
                        .desired_width(width)
                        .desired_rows(3),
                )
            } else {
                forms::text_input(ui, buffer, width, "")
            };
            if response.changed() {
                if let Ok(value) = parse(buffer, &how) {
                    let value = match (&value, how) {
                        // (a text keeps what was typed, spaces too)
                        (Some(_), How::Text) => Some(Json::String(buffer.clone())),
                        _ => value,
                    };
                    fx.set(path, value, true);
                }
            } else if !response.has_focus() {
                *buffer = text;
            }
        }
    }
}

/// The flavors: the package's, removed one by one; one added by name.
fn flavors(ui: &mut Ui, fx: &mut Fx<'_>, new: &mut String, p: &Palette) {
    let current: Vec<String> = fx
        .get(&["flavors"])
        .and_then(Json::as_object)
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    for f in &current {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(f)
                    .font(Face::PlexMedium.at(12.5))
                    .color(p.ink),
            );
            if forms::text_button(ui, "remove").clicked() {
                let keys = vec![Key::Name("flavors".into()), Key::Name(f.clone())];
                fx.changes.push((keys, None, false));
            }
        });
    }
    ui.horizontal(|ui| {
        forms::text_input(ui, new, 200.0, "a flavor's ID or alias");
        let name = new.trim().to_owned();
        if widgets::button(ui, ButtonKind::Secondary, "add", true, None).clicked()
            && !name.is_empty()
            && !current.contains(&name)
        {
            let keys = vec![Key::Name("flavors".into()), Key::Name(name)];
            fx.changes.push((keys, Some(serde_json::json!({})), false));
            new.clear();
        }
    });
    caption(ui, "camelot is WoW Forever (_forever_).", p);
}

/// Every other top-level key, by the schema: an object's fields, else the
/// value in a JSON box.
fn other_keys(ui: &mut Ui, fx: &mut Fx<'_>, schema: Option<&Json>, p: &Palette) {
    let Some(schema) = schema else {
        caption(
            ui,
            "wowlua's schema is not here: edit these keys in the json view.",
            p,
        );
        return;
    };
    let Some(props) = schema.get("properties").and_then(Json::as_object) else {
        return;
    };
    let mut go = None;
    for (key, node) in props {
        if DESIGNED.contains(&key.as_str()) {
            continue;
        }
        let node = crate::editor::json::resolve(schema, node);
        let desc = node.get("description").and_then(Json::as_str).unwrap_or("");
        let desc = desc.split("\n\n").next().unwrap_or("").replace('\n', " ");
        ui.label(
            egui::RichText::new(key)
                .font(Face::PlexMedium.at(12.5))
                .color(p.ink),
        );
        if !desc.is_empty() {
            caption(ui, &desc, p);
        }
        let (fields, _) = schema::fields(schema, node);
        if fields.is_empty() {
            let width = ui.available_width().min(520.0);
            edit_value(ui, fx, &[key.as_str()], How::Json, width, &mut go);
        } else {
            for f in fields {
                let how = match f.kind {
                    Kind::Bool => How::Flag,
                    Kind::Number => How::Number,
                    Kind::Text | Kind::Scalar => How::Text,
                    Kind::Numbers | Kind::Json => How::Json,
                    Kind::Choice(values) => How::Values(values),
                };
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.label(
                        egui::RichText::new(&f.name)
                            .font(Face::Plex.at(12.0))
                            .color(p.ink),
                    );
                });
                let width = ui.available_width().min(420.0);
                edit_value(
                    ui,
                    fx,
                    &[key.as_str(), f.name.as_str()],
                    how,
                    width,
                    &mut go,
                );
            }
        }
        ui.add_space(10.0);
    }
}

/// The page for section `section`; the json view draws the document's
/// code, which the caller does (it holds the editor).
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    state: &mut ManifestState,
    editors: &mut Editors,
    workspace: &Workspace,
    system: &dyn System,
    section: usize,
    wowlua_version: &str,
) -> (Option<Action>, Option<(egui::Rect, usize)>) {
    let p = palette(ui.ctx());
    let path = state.path(workspace);
    let Ok(tab) = editors.held(&path, system) else {
        forms::padded(ui, 16, 10, |ui| {
            caption(ui, &format!("{} can't be read.", path.display()), p)
        });
        return (None, None);
    };
    let doc = &editors.tabs[tab].doc;
    let dirty = doc.is_dirty();
    if state
        .parsed
        .as_ref()
        .is_none_or(|(pa, v, _)| *pa != path || *v != doc.version)
    {
        let root = serde_json::from_str::<Json>(&doc.text()).unwrap_or(Json::Null);
        state.parsed = Some((path.clone(), doc.version, root));
    }
    if state.schema.is_none() {
        state.schema = Some(
            system
                .schema(false)
                .and_then(|t| serde_json::from_str(&t).ok()),
        );
    }
    let mut action = None;
    // The bar: the package, the view, saving, finding a key.
    forms::padded(ui, 16, 8, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("wowlua.json")
                    .font(Face::PlexMedium.at(12.5))
                    .color(p.ink),
            );
            package_select(ui, state, workspace);
            if let Some(i) = widgets::seg(
                ui,
                "mf-view",
                &["form", "json"],
                usize::from(state.json_view),
                false,
            ) {
                state.json_view = i == 1;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let word = if dirty { "save" } else { "saved" };
                if widgets::button(ui, ButtonKind::Primary, word, true, None).clicked() && dirty {
                    action = Some(Action::Save(path.clone()));
                }
                if !state.json_view {
                    forms::text_input(ui, &mut state.query, 220.0, "find a setting");
                }
            });
        });
    });
    // A requirement that excludes the wowlua built in, with its fix.
    let root = state
        .parsed
        .as_ref()
        .map(|(_, _, r)| r.clone())
        .unwrap_or(Json::Null);
    let mut changes: Vec<(Vec<Key>, Option<Json>, bool)> = Vec::new();
    if state.picked.is_none()
        && let Some(why) = &workspace.cannot_check
    {
        let wanted = requirement_for(wowlua_version);
        forms::padded(ui, 16, 4, |ui| {
            forms::callout(ui, CalloutKind::Error, |ui| {
                ui.horizontal_wrapped(|ui| {
                    forms::paragraph(ui, why, 12.5, 19.0, p.ink);
                    if widgets::button(
                        ui,
                        ButtonKind::Primary,
                        &format!("require {wanted}"),
                        true,
                        None,
                    )
                    .clicked()
                    {
                        changes.push((
                            vec![Key::Name("package".into()), Key::Name("wowlua".into())],
                            Some(Json::String(wanted.clone())),
                            false,
                        ));
                    }
                });
            });
        });
    }
    if state.json_view {
        let rect = ui.available_rect_before_wrap();
        apply(editors, tab, changes);
        return (action, Some((rect, tab)));
    }
    let mut go = None;
    egui::ScrollArea::vertical()
        .id_salt("manifest-rows")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            forms::padded(ui, 24, 10, |ui| {
                ui.set_width(ui.available_width());
                let mut fx = Fx {
                    root: &root,
                    changes: Vec::new(),
                    buffers: &mut state.buffers,
                };
                let query = state.query.trim().to_lowercase();
                let (name, desc) = SECTIONS.get(section).copied().unwrap_or(SECTIONS[0]);
                if query.is_empty() {
                    ui.label(egui::RichText::new(name).font(Face::PlexMedium.at(15.0)).color(p.ink));
                    caption(ui, desc, p);
                    ui.add_space(8.0);
                    match name {
                        "flavors" => flavors(ui, &mut fx, &mut state.new_flavor, p),
                        "other keys" => other_keys(ui, &mut fx, state.schema.as_ref().and_then(Option::as_ref), p),
                        _ => {}
                    }
                }
                let all = rows();
                let shown: Vec<&Row> = all
                    .iter()
                    .filter(|r| {
                        if query.is_empty() {
                            r.section == name
                        } else {
                            format!("{} {}", r.path.join("."), r.desc).to_lowercase().contains(&query)
                        }
                    })
                    .collect();
                if !query.is_empty() && shown.is_empty() {
                    caption(ui, &format!("No setting of wowlua.json matches \"{}\".", state.query.trim()), p);
                }
                for r in shown {
                    row(ui, &mut fx, r, p, &mut go);
                }
                caption(
                    ui,
                    "Every key is the schema's, with completion and checks in the json view too. An edit rewrites only that value in wowlua.json, keeping its order.",
                    p,
                );
                changes.append(&mut fx.changes);
            });
        });
    if let Some(to) = go {
        action = Some(Action::Go(to));
    }
    apply(editors, tab, changes);
    (action, None)
}

/// The changes, as edits of the manifest's text.
fn apply(editors: &mut Editors, tab: usize, changes: Vec<(Vec<Key>, Option<Json>, bool)>) {
    for (path, value, typing) in changes {
        let text = editors.tabs[tab].doc.text();
        let Some((range, with)) = json::set(&text, &path, value.as_ref()) else {
            continue;
        };
        let (a, b) = (pos_of(&text, range.start), pos_of(&text, range.end));
        let kind = if typing {
            EditKind::Typing
        } else {
            EditKind::Other
        };
        editors.tabs[tab].doc.edit(a, b, &with, kind);
    }
}

/// `^0.13` for wowlua 0.13.6: the requirement that allows the wowlua built
/// in.
pub fn requirement_for(version: &str) -> String {
    let mut parts = version.split(['.', '-']);
    match (parts.next(), parts.next()) {
        (Some(major), Some(minor)) => format!("^{major}.{minor}"),
        _ => format!("^{version}"),
    }
}

/// The package select: the workspace's packages, the root first.
fn package_select(ui: &mut Ui, state: &mut ManifestState, workspace: &Workspace) {
    let mut members: Vec<(String, PathBuf)> = workspace
        .members
        .iter()
        .map(|m| (m.name.clone(), m.dir.clone()))
        .collect();
    members.sort_by_key(|(_, dir)| *dir != workspace.root);
    if !members.iter().any(|(_, d)| *d == workspace.root) {
        members.insert(0, (workspace.name.clone(), workspace.root.clone()));
    }
    let current = state
        .picked
        .clone()
        .unwrap_or_else(|| workspace.root.clone());
    let mut index = members.iter().position(|(_, d)| *d == current).unwrap_or(0);
    let labels: Vec<String> = members
        .iter()
        .map(|(n, d)| {
            if *d == workspace.root {
                format!("{n} · root")
            } else {
                n.clone()
            }
        })
        .collect();
    let opts: Vec<(&str, bool)> = labels.iter().map(|l| (l.as_str(), true)).collect();
    if forms::select(ui, "mf-package", &opts, &mut index, 180.0) {
        let dir = members[index].1.clone();
        state.picked = (dir != workspace.root).then_some(dir);
        state.parsed = None;
        state.buffers.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requirements_allow_the_wowlua_built_in() {
        assert_eq!(requirement_for("0.13.6"), "^0.13");
        assert_eq!(requirement_for("1.0.0-beta.1"), "^1.0");
    }

    #[test]
    fn typed_values_have_their_kinds() {
        assert_eq!(parse("120", &How::Number), Ok(Some(serde_json::json!(120))));
        assert_eq!(
            parse("76.5", &How::Number),
            Ok(Some(serde_json::json!(76.5)))
        );
        assert_eq!(
            parse("a, b", &How::List),
            Ok(Some(serde_json::json!(["a", "b"])))
        );
        assert_eq!(parse("{ \"a\": ", &How::Json), Err(()));
        assert_eq!(parse("  ", &How::Text), Ok(None));
    }

    #[test]
    fn every_section_has_rows() {
        let all = rows();
        for (name, _) in SECTIONS {
            let special = matches!(*name, "flavors" | "other keys");
            assert!(special || all.iter().any(|r| r.section == *name), "{name}");
        }
    }
}
