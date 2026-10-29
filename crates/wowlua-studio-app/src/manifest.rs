//! A `wowlua.json` tab's form (M10, S062, S063): the package's manifest by
//! section, the designed rows, then every other key in a form from
//! wowlua's schema; the dependencies section the dependencies view; the
//! json view the tab's code. An edit changes only that value's text,
//! keeping the file's order.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use egui::{CornerRadius, Rect, Ui, UiBuilder, Vec2};
use serde_json::Value as Json;
use wowlua_studio_ui::controls;
use wowlua_studio_ui::forms::{self, CalloutKind};
use wowlua_studio_ui::widgets::{self, ButtonKind};
use wowlua_studio_ui::{Face, Palette, palette};

use crate::deps::{self, DepsState};
use crate::editor::Editors;
use crate::editor::doc::EditKind;
use crate::system::{BuildProfile, System, Workspace};
use crate::world::json::{self, Key};
use crate::world::pos_of;
use crate::world::schema::{self, Kind};

/// The sections, as the tab lists them.
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
        "What goes into a build, and the build profiles the ship page builds.",
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
    /// Several of a known set, each a chip.
    Toggles(&'static [&'static str]),
    /// fmt's indent: a tab, or 2, 4 or 8 spaces.
    Indent,
    /// A number one step at a time with presets; `default` when unset.
    Steps {
        min: f64,
        max: Option<f64>,
        step: f64,
        unit: &'static str,
        presets: &'static [(&'static str, f64)],
        default: f64,
    },
    /// Minimums in % by sub-key (coverage's): each on or off, a slider.
    Minimums(&'static [(&'static str, &'static str)]),
    /// A screen: its size, DPI scale and the game's UI Scale.
    Screen,
    /// One of the package's world profiles (`tests/profiles/*.json`).
    WorldProfile,
    /// A version, typed, with the next ones suggested.
    Version,
    /// A version requirement: `^`, `~` or `=`, then a version.
    Requirement,
    /// The changelog: a file, git's history or none.
    Changelog,
    /// Extra TOC directives: one of wowlua's known ones, and its value.
    TocExtra,
}

/// The game's UI Scale slider (Blizzard's Graphics.lua, wowlua T508).
const UI_SCALE: (f64, f64, f64) = (0.65, 1.15, 0.01);

/// The screens offered; `custom…` types another.
const SCREENS: &[(u32, u32)] = &[(1920, 1080), (2560, 1440), (3840, 2160)];

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
            Version,
        ),
        r(
            "package",
            &["package", "wowlua"],
            "The wowlua the package needs: ^ any later minor, ~ any later patch, = exactly.",
            Requirement,
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
            &["addon", "toc-extra"],
            "TOC lines wowlua doesn't write from wowlua.json, written as they are.",
            TocExtra,
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
            "Search the registry, add, update and remove them in the dependencies section.",
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
        r("fmt", &["fmt", "indent"], "A tab, or spaces.", Indent),
        r(
            "fmt",
            &["fmt", "line-width"],
            "Where wowlua fmt wraps.",
            Steps {
                min: 40.0,
                max: None,
                step: 1.0,
                unit: "columns",
                presets: &[("80", 80.0), ("100", 100.0), ("120", 120.0), ("160", 160.0)],
                default: 100.0,
            },
        ),
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
            WorldProfile,
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
            "The emulated client's screen for tests.",
            Screen,
        ),
        r(
            "test",
            &["test", "coverage"],
            "Minimums in %: wowlua test --coverage fails below one that is on.",
            Minimums(&[
                ("lines", "lines"),
                ("statements", "statements"),
                ("branches", "branches"),
                ("functions", "functions"),
                ("conditions", "conditions"),
            ]),
        ),
        r(
            "test",
            &["test", "coverage", "exclude"],
            "Files left out of coverage (globs).",
            List,
        ),
        r(
            "test",
            &["test", "reporters"],
            "What wowlua test writes; pretty when none.",
            Toggles(&[
                "pretty",
                "junit",
                "tap",
                "json",
                "github",
                "lcov",
                "cobertura",
                "coverage-json",
                "html",
            ]),
        ),
        r(
            "publish",
            &["publish", "changelog"],
            "The release notes wowlua publish uploads.",
            Changelog,
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

/// What the tab asks of the rest of Studio.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Save(PathBuf),
    Go(Go),
}

/// The wowlua.json tabs' forms (S063): each file's own, the schema read
/// once, the dependencies section's view.
#[derive(Default)]
pub struct Manifests {
    views: HashMap<PathBuf, ManifestState>,
    schema: Option<Option<Json>>,
    pub deps: DepsState,
    /// The build profiles, by the package's folder (S064).
    profiles: HashMap<PathBuf, ProfilesView>,
}

impl Manifests {
    /// The form of `path` on a section by its name (the screenshot
    /// harness's `manifest:`).
    pub fn show_section(&mut self, path: &Path, name: &str) {
        let i = SECTIONS.iter().position(|(n, _)| *n == name).unwrap_or(0);
        self.views.entry(path.to_path_buf()).or_default().section = i;
        if SECTIONS[i].0 == "dependencies" {
            self.deps.opened();
        }
    }

    /// The form of `path` on its build section, a profile shown (the ship
    /// page's "edit in wowlua.json", S064).
    pub fn show_profile(&mut self, path: &Path, profile: &str) {
        self.show_section(path, "build");
        if let Some(dir) = path.parent() {
            self.profiles.entry(dir.to_path_buf()).or_default().selected = Some(profile.to_owned());
        }
    }

    /// The build profiles of the package in `dir`, as wowlua resolves them.
    pub fn profiles_read(&mut self, dir: PathBuf, read: Result<Vec<BuildProfile>, String>) {
        let view = self.profiles.entry(dir).or_default();
        view.reading = false;
        view.read = Some(read);
    }

    /// Files changed on disk: what wowlua resolves is read again.
    pub fn files_changed(&mut self) {
        for view in self.profiles.values_mut() {
            view.stale = true;
        }
        self.deps.files_changed();
    }
}

/// A package's build profiles in its form.
#[derive(Default)]
struct ProfilesView {
    read: Option<Result<Vec<BuildProfile>, String>>,
    reading: bool,
    stale: bool,
    selected: Option<String>,
    /// A new profile's name being typed.
    new_profile: Option<String>,
    buffers: HashMap<String, String>,
}

/// A wowlua.json's form: its section, what is searched, what is typed.
#[derive(Default)]
struct ManifestState {
    section: usize,
    query: String,
    parsed: Option<(u64, Json)>,
    buffers: HashMap<String, String>,
    new_flavor: String,
}

/// The edits a form makes: a path and its new value (none: removed).
struct Fx<'a> {
    root: &'a Json,
    changes: Vec<(Vec<Key>, Option<Json>, bool)>,
    buffers: &'a mut HashMap<String, String>,
    /// The workspace's world profiles, by name.
    profiles: &'a [String],
    /// The wowlua built into Studio.
    wowlua: &'a str,
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
    forms::centred_row(
        ui,
        ("manifest", &path_text),
        width - control - 16.0,
        control,
        |ui| {
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
        },
        |ui| edit_value(ui, fx, r.path, r.how.clone(), control, go),
    );
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
                    Go::Dependencies => "the dependencies section",
                    Go::Ship => "the ship page",
                };
                if widgets::button(ui, ButtonKind::Secondary, label, true, None).clicked() {
                    *go = Some(to);
                }
            });
        }
        How::Toggles(options) => {
            let mut on = strings(now.as_ref());
            if controls::toggle_chips(ui, options, &mut on) {
                fx.set(path, list(on), false);
            }
        }
        How::Indent => {
            let at = match &now {
                None => 0,
                Some(Json::String(s)) if s == "tab" => 0,
                Some(v) => match v.as_u64() {
                    Some(2) => 1,
                    Some(4) => 2,
                    _ => 3,
                },
            };
            let id = format!("mf-{}", path.join("."));
            if let Some(i) = widgets::seg(ui, &id, &["tab", "2", "4", "8"], at, true) {
                let value = match i {
                    0 => Json::String("tab".into()),
                    1 => Json::from(2),
                    2 => Json::from(4),
                    _ => Json::from(8),
                };
                fx.set(path, Some(value), false);
            }
        }
        How::Steps {
            min,
            max,
            step,
            unit,
            presets,
            default,
        } => {
            let before = now.as_ref().and_then(Json::as_f64).unwrap_or(default);
            let mut v = before;
            ui.horizontal_wrapped(|ui| {
                controls::stepper(ui, &path.join("."), &mut v, Some(min), max, step, unit);
                if let Some(p) = controls::presets(ui, presets, v) {
                    v = p;
                }
            });
            if (v - before).abs() > 1e-9 {
                fx.set(path, Some(json_number(v)), false);
            }
        }
        How::Minimums(subs) => {
            ui.vertical(|ui| {
                for (sub, label) in subs {
                    let mut keys = path.to_vec();
                    keys.push(sub);
                    let current = fx.get(&keys).and_then(Json::as_f64);
                    let mut on = current.is_some();
                    if forms::checkbox(ui, &mut on, label, None) {
                        fx.set(&keys, on.then(|| json_number(80.0)), false);
                    }
                    if let Some(c) = current {
                        let mut v = c;
                        let id = keys.join(".");
                        let ticks = [(0.0, "0%"), (50.0, "50%"), (100.0, "100%")];
                        if controls::slider(
                            ui, &id, &mut v, 0.0, 100.0, 0.5, 1.0, "%", &ticks, width,
                        ) {
                            fx.set(&keys, Some(json_number(v)), true);
                        }
                    }
                    ui.add_space(4.0);
                }
            });
        }
        How::Screen => screen(ui, fx, path, width),
        How::Version => {
            let text = shown(now.as_ref(), &How::Text);
            typed_text(ui, fx, path, &text, width, "1.0.0");
            let next = next_versions(&text);
            if let Some(v) = controls::suggestions(ui, &next) {
                fx.set(path, Some(Json::String(v)), false);
            }
        }
        How::Requirement => {
            let text = shown(now.as_ref(), &How::Text);
            let (op, version) = match text.chars().next() {
                Some(c @ ('^' | '~' | '=')) => (c, text[1..].trim().to_owned()),
                _ => ('^', text.trim().to_owned()),
            };
            let at = ['^', '~', '='].iter().position(|c| *c == op).unwrap_or(0);
            ui.horizontal(|ui| {
                let id = format!("mf-{}", path.join("."));
                if let Some(i) = widgets::seg(ui, &id, &["^", "~", "="], at, true) {
                    let op = ['^', '~', '='][i];
                    fx.set(path, Some(Json::String(format!("{op}{version}"))), false);
                }
                let key = format!("{}.version", path.join("."));
                let buffer = fx
                    .buffers
                    .entry(key.clone())
                    .or_insert_with(|| version.clone());
                let response = forms::text_input(ui, buffer, (width - 120.0).max(100.0), "0.13");
                let typed = (response.changed() && !buffer.trim().is_empty())
                    .then(|| format!("{op}{}", buffer.trim()));
                if typed.is_none() && !response.has_focus() {
                    *buffer = version.clone();
                }
                if let Some(requirement) = typed {
                    fx.set(path, Some(Json::String(requirement)), true);
                }
            });
            let built_in = requirement_for(fx.wowlua);
            if built_in != text {
                let label = format!("{built_in} · the wowlua built into Studio");
                if controls::suggestions(ui, std::slice::from_ref(&label)).is_some() {
                    fx.set(path, Some(Json::String(built_in)), false);
                }
            }
        }
        How::Changelog => {
            let at = match &now {
                Some(Json::String(s)) if s == "git" => 1,
                Some(Json::Null) => 2,
                _ => 0,
            };
            let id = format!("mf-{}", path.join("."));
            if let Some(i) = widgets::seg(ui, &id, &["a file", "git", "none"], at, true) {
                let value = match i {
                    0 => Json::String("CHANGELOG.md".into()),
                    1 => Json::String("git".into()),
                    _ => Json::Null,
                };
                fx.set(path, Some(value), false);
            }
            if at == 0 {
                let text = shown(now.as_ref(), &How::Text);
                typed_text(ui, fx, path, &text, width, "CHANGELOG.md");
            }
        }
        How::TocExtra => {
            let set: Vec<(String, String)> = now
                .as_ref()
                .and_then(Json::as_object)
                .map(|o| {
                    o.iter()
                        .map(|(k, v)| {
                            (
                                k.clone(),
                                v.as_str().map_or_else(|| v.to_string(), str::to_owned),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            let p = palette(ui.ctx());
            ui.vertical(|ui| {
                for (key, value) in &set {
                    let keys: Vec<&str> = path.iter().copied().chain([key.as_str()]).collect();
                    // (the name centred on its value's field)
                    forms::bar(ui, |ui| {
                        ui.label(
                            egui::RichText::new(key)
                                .font(Face::PlexMedium.at(12.0))
                                .color(p.ink),
                        );
                        typed_text(ui, fx, &keys, value, (width - 200.0).max(120.0), "");
                        if widgets::button(ui, ButtonKind::Tertiary, "remove", true, None).clicked()
                        {
                            fx.set(&keys, None, false);
                        }
                    });
                    if let Some(d) = wowlua_toc::extra_directives()
                        .iter()
                        .find(|d| d.name.eq_ignore_ascii_case(key))
                    {
                        caption(ui, d.meaning, p);
                    }
                    ui.add_space(4.0);
                }
                // (the known ones not set yet; any X- field can be typed in the json)
                let offered: Vec<&wowlua_toc::KnownDirective> = wowlua_toc::extra_directives()
                    .iter()
                    .filter(|d| !set.iter().any(|(k, _)| k.eq_ignore_ascii_case(d.name)))
                    .collect();
                if !offered.is_empty() {
                    let mut names: Vec<&str> = vec!["add a directive"];
                    names.extend(offered.iter().map(|d| d.name));
                    let opts: Vec<(&str, bool)> = names.iter().map(|n| (*n, true)).collect();
                    let mut at = 0;
                    if forms::select(
                        ui,
                        &format!("mf-{}-add", path.join(".")),
                        &opts,
                        &mut at,
                        width.min(300.0),
                    ) && at > 0
                    {
                        let keys: Vec<&str> = path.iter().copied().chain([names[at]]).collect();
                        fx.set(&keys, Some(Json::String(String::new())), false);
                    }
                }
            });
        }
        How::WorldProfile => {
            let current = now.as_ref().and_then(Json::as_str).unwrap_or("").to_owned();
            let mut names: Vec<String> = vec!["none · a plain level 1 character".into()];
            names.extend(fx.profiles.iter().cloned());
            if !current.is_empty() && !fx.profiles.contains(&current) {
                names.push(format!("{current} · not in tests/profiles"));
            }
            let mut index = fx.profiles.iter().position(|n| *n == current).map_or(
                if current.is_empty() {
                    0
                } else {
                    names.len() - 1
                },
                |i| i + 1,
            );
            let opts: Vec<(&str, bool)> = names.iter().map(|n| (n.as_str(), true)).collect();
            if forms::select(
                ui,
                &format!("mf-{}", path.join(".")),
                &opts,
                &mut index,
                width.min(300.0),
            ) {
                let value = fx
                    .profiles
                    .get(index.wrapping_sub(1))
                    .map(|n| Json::String(n.clone()));
                if index == 0 || value.is_some() {
                    fx.set(path, value, false);
                }
            }
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
        How::List => {
            let mut items = strings(now.as_ref());
            if controls::tags(
                ui,
                &path.join("."),
                &mut items,
                "type, then Enter",
                width,
                &|_| None,
            ) {
                fx.set(path, list(items), false);
            }
        }
        How::Text | How::Number | How::Json => {
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

/// A text typed into the form, kept while it is typed.
fn typed_text(ui: &mut Ui, fx: &mut Fx<'_>, path: &[&str], text: &str, width: f32, hint: &str) {
    let key = path.join(".");
    let buffer = fx.buffers.entry(key).or_insert_with(|| text.to_owned());
    let response = forms::text_input(ui, buffer, width, hint);
    if response.changed() {
        let value = (!buffer.trim().is_empty()).then(|| Json::String(buffer.clone()));
        fx.set(path, value, true);
    } else if !response.has_focus() {
        *buffer = text.to_owned();
    }
}

/// The versions after `version`: a beta's next beta and its release; a
/// release's next patch, minor and first beta of the next minor.
pub fn next_versions(version: &str) -> Vec<String> {
    let (base, pre) = version
        .trim()
        .split_once('-')
        .unwrap_or((version.trim(), ""));
    let mut n = base.split('.').map(|p| p.parse::<u64>().unwrap_or(0));
    let (major, minor, patch) = (
        n.next().unwrap_or(0),
        n.next().unwrap_or(0),
        n.next().unwrap_or(0),
    );
    if let Some((kind, number)) = pre.split_once('.') {
        let number = number.parse::<u64>().unwrap_or(0);
        return vec![
            format!("{major}.{minor}.{patch}-{kind}.{}", number + 1),
            format!("{major}.{minor}.{patch}"),
        ];
    }
    vec![
        format!("{major}.{minor}.{}", patch + 1),
        format!("{major}.{}.0", minor + 1),
        format!("{major}.{}.0-beta.1", minor + 1),
    ]
}

/// A JSON list of strings as strings.
fn strings(v: Option<&Json>) -> Vec<String> {
    v.and_then(Json::as_array)
        .map(|a| {
            a.iter()
                .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Strings as a JSON list; none when empty (the key is removed).
fn list(items: Vec<String>) -> Option<Json> {
    (!items.is_empty()).then(|| Json::Array(items.into_iter().map(Json::String).collect()))
}

/// A number as JSON: whole numbers without a fraction.
fn json_number(v: f64) -> Json {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        Json::from(v as i64)
    } else {
        serde_json::Number::from_f64(v).map_or(Json::Null, Json::Number)
    }
}

/// A screen (`test.screen`): its size (three, or custom), its DPI scale and
/// the game's own UI Scale control: off leaves the client's scale.
fn screen(ui: &mut Ui, fx: &mut Fx<'_>, path: &[&str], width: f32) {
    let key = |sub: &'static str| {
        let mut k = path.to_vec();
        k.push(sub);
        k
    };
    let get = |fx: &Fx<'_>, sub: &'static str| fx.get(&key(sub)).and_then(Json::as_f64);
    let w = get(fx, "width");
    let h = get(fx, "height");
    let id = path.join(".");
    ui.vertical(|ui| {
        // The size.
        let mut labels: Vec<String> = SCREENS.iter().map(|(w, h)| format!("{w} × {h}")).collect();
        labels.push("custom…".into());
        let custom_key = format!("{id}.custom");
        let mut custom: bool = ui
            .data(|d| d.get_temp(egui::Id::unique(&custom_key)))
            .unwrap_or(false);
        let at = match (w, h) {
            (None, None) => 0,
            (Some(w), Some(h)) => SCREENS
                .iter()
                .position(|(sw, sh)| f64::from(*sw) == w && f64::from(*sh) == h)
                .unwrap_or(SCREENS.len()),
            _ => SCREENS.len(),
        };
        let mut index = if custom { SCREENS.len() } else { at };
        let opts: Vec<(&str, bool)> = labels.iter().map(|l| (l.as_str(), true)).collect();
        if forms::select(
            ui,
            &format!("mf-{id}-size"),
            &opts,
            &mut index,
            width.min(240.0),
        ) {
            custom = index == SCREENS.len();
            if let Some((sw, sh)) = SCREENS.get(index) {
                fx.set(&key("width"), Some(Json::from(*sw)), false);
                fx.set(&key("height"), Some(Json::from(*sh)), false);
            }
            ui.data_mut(|d| d.insert_temp(egui::Id::unique(&custom_key), custom));
        }
        if custom || index == SCREENS.len() {
            ui.horizontal_wrapped(|ui| {
                let mut cw = w.unwrap_or(1920.0);
                let mut ch = h.unwrap_or(1080.0);
                if controls::stepper(
                    ui,
                    &format!("{id}.width"),
                    &mut cw,
                    Some(640.0),
                    Some(7680.0),
                    1.0,
                    "×",
                ) {
                    fx.set(&key("width"), Some(json_number(cw)), true);
                }
                if controls::stepper(
                    ui,
                    &format!("{id}.height"),
                    &mut ch,
                    Some(480.0),
                    Some(4320.0),
                    1.0,
                    "px",
                ) {
                    fx.set(&key("height"), Some(json_number(ch)), true);
                }
            });
        }
        ui.add_space(4.0);
        // The DPI scale.
        let dpis = [1.0, 1.25, 1.5, 2.0];
        let dpi = get(fx, "dpiScale").unwrap_or(1.0);
        let dpi_labels: Vec<String> = dpis.iter().map(|d| format!("dpi {d}")).collect();
        let mut at = dpis
            .iter()
            .position(|d| (d - dpi).abs() < 1e-9)
            .unwrap_or(0);
        let opts: Vec<(&str, bool)> = dpi_labels.iter().map(|l| (l.as_str(), true)).collect();
        if forms::select(
            ui,
            &format!("mf-{id}-dpi"),
            &opts,
            &mut at,
            width.min(240.0),
        ) {
            fx.set(&key("dpiScale"), Some(json_number(dpis[at])), false);
        }
        ui.add_space(4.0);
        // The game's UI Scale: a check box, then its slider.
        let scale = get(fx, "uiScale");
        let mut on = scale.is_some();
        if forms::checkbox(
            ui,
            &mut on,
            "Use UI Scale",
            Some("off: the client's own scale"),
        ) {
            fx.set(&key("uiScale"), on.then(|| json_number(1.0)), false);
        }
        if let Some(s) = scale {
            let mut v = s;
            let (min, max, step) = UI_SCALE;
            let ticks = [(0.65, "65%"), (1.0, "100%"), (1.15, "115%")];
            if controls::slider(
                ui,
                &format!("{id}.uiScale"),
                &mut v,
                min,
                max,
                step,
                100.0,
                "%",
                &ticks,
                width,
            ) {
                fx.set(&key("uiScale"), Some(json_number(v)), true);
            }
        }
    });
}

/// The flavors (S064): the package's, each removed but the last; one added
/// from wowlua's flavor table, those coming soon named but not picked.
fn flavors(ui: &mut Ui, fx: &mut Fx<'_>, new: &mut String, p: &Palette) {
    let current: Vec<String> = fx
        .get(&["flavors"])
        .and_then(Json::as_object)
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    let width = ui.available_width();
    let control = 380.0_f32.min(width * 0.5);
    for f in &current {
        let clients: Vec<String> = wowlua_flavors::table()
            .resolve(f)
            .map(|flavor| {
                wowlua_flavors::table()
                    .clients_of(&flavor.id)
                    .map(|c| format!("{} · {}", c.name, c.folders.join(", ")))
                    .collect()
            })
            .unwrap_or_default();
        forms::centred_row(
            ui,
            ("flavor", f.clone()),
            width - control - 16.0,
            control,
            |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new(f)
                            .font(Face::PlexMedium.at(12.5))
                            .color(p.ink),
                    );
                    ui.label(
                        egui::RichText::new(format!("flavors.{f}"))
                            .font(Face::Plex.at(11.0))
                            .color(p.ink_faint),
                    );
                });
                let state = if crate::flavors::supported(f) {
                    "supported"
                } else {
                    "coming soon"
                };
                caption(ui, &format!("{} · {state}", crate::flavors::name(f)), p);
            },
            |ui| {
                ui.horizontal_wrapped(|ui| {
                    if !clients.is_empty() {
                        caption(ui, &clients.join(" · "), p);
                    }
                    ui.add_enabled_ui(current.len() > 1, |ui| {
                        if widgets::button(ui, ButtonKind::Tertiary, "remove", true, None).clicked()
                        {
                            let keys = vec![Key::Name("flavors".into()), Key::Name(f.clone())];
                            fx.changes.push((keys, None, false));
                        }
                    });
                });
            },
        );
        ui.add_space(10.0);
    }
    // (a flavor already there, under its id or an alias, is not offered)
    let have = |id: &str| {
        current.iter().any(|c| {
            wowlua_flavors::table()
                .resolve(c)
                .is_some_and(|f| f.id == id)
        })
    };
    let offered: Vec<&wowlua_flavors::Flavor> =
        crate::flavors::all().filter(|f| !have(&f.id)).collect();
    forms::centred_row(
        ui,
        ("flavor", "add".to_owned()),
        width - control - 16.0,
        control,
        |ui| {
            ui.label(
                egui::RichText::new("add a flavor")
                    .font(Face::PlexMedium.at(12.5))
                    .color(p.ink),
            );
            caption(
                ui,
                "From wowlua's flavor table. Coming soon: named and checked, but the game page, the debugger and Blizzard's UI don't run it yet.",
                p,
            );
        },
        |ui| {
            if offered.is_empty() {
                caption(ui, "Every flavor wowlua knows is here.", p);
                return;
            }
            let labels: Vec<String> = std::iter::once("pick a flavor".to_owned())
                .chain(offered.iter().map(|f| crate::flavors::label(&f.id)))
                .collect();
            let opts: Vec<(&str, bool)> = labels
                .iter()
                .enumerate()
                .map(|(i, l)| (l.as_str(), i == 0 || offered[i - 1].is_supported()))
                .collect();
            let mut at = offered
                .iter()
                .position(|f| f.id == *new)
                .map_or(0, |i| i + 1);
            ui.horizontal(|ui| {
                if forms::select(ui, "mf-add-flavor", &opts, &mut at, control.min(260.0)) {
                    *new = if at == 0 {
                        String::new()
                    } else {
                        offered[at - 1].id.clone()
                    };
                }
                let pick = (!new.is_empty()).then(|| new.clone());
                if widgets::button(ui, ButtonKind::Secondary, "add", pick.is_some(), None).clicked()
                    && let Some(id) = pick
                {
                    let keys = vec![Key::Name("flavors".into()), Key::Name(id)];
                    fx.changes.push((keys, Some(serde_json::json!({})), false));
                    new.clear();
                }
            });
        },
    );
    ui.add_space(12.0);
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
/// The package a manifest is of: its name and where it is in the
/// workspace.
fn package_of(dir: &Path, workspace: &Workspace) -> String {
    let name = workspace
        .members
        .iter()
        .find(|m| m.dir == dir)
        .map(|m| m.name.clone())
        .or_else(|| (dir == workspace.root).then(|| workspace.name.clone()))
        .unwrap_or_else(|| {
            dir.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
    if dir == workspace.root {
        format!("{name} · the root package of the workspace")
    } else {
        format!("{name} · a package of the workspace")
    }
}

/// A wowlua.json tab's bar: the form or the json, finding a key, saving.
pub fn bar(
    ui: &mut Ui,
    manifests: &mut Manifests,
    editors: &mut Editors,
    tab: usize,
    workspace: &Workspace,
) -> Option<Action> {
    let p = palette(ui.ctx());
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_100);
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(1.0, p.line),
    );
    let path = editors.tabs[tab].doc.path.clone();
    let dirty = editors.tabs[tab].doc.is_dirty();
    let json_view = editors.as_json.contains(&path);
    let dir = path.parent().unwrap_or(&workspace.root).to_path_buf();
    let state = manifests.views.entry(path.clone()).or_default();
    let mut action = None;
    forms::padded(ui, 16, 8, |ui| {
        forms::bar(ui, |ui| {
            if let Some(i) = widgets::seg(
                ui,
                "mf-view",
                &["form", "json"],
                usize::from(json_view),
                false,
            ) {
                if i == 1 {
                    editors.as_json.insert(path.clone());
                    editors.focus = true;
                } else {
                    editors.as_json.remove(&path);
                }
            }
            caption(ui, &package_of(&dir, workspace), p);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let word = if dirty { "save" } else { "saved" };
                if widgets::button(ui, ButtonKind::Primary, word, true, None).clicked() && dirty {
                    action = Some(Action::Save(path.clone()));
                }
                if !json_view {
                    forms::text_input(ui, &mut state.query, 220.0, "find a setting");
                }
            });
        });
    });
    action
}

/// A wowlua.json tab's form: its sections on the left, the one shown (or
/// what the search finds) on the right.
#[allow(clippy::too_many_arguments)]
pub fn form(
    ui: &mut Ui,
    manifests: &mut Manifests,
    editors: &mut Editors,
    tab: usize,
    workspace: &Workspace,
    system: &dyn System,
    wowlua_version: &str,
    world_profiles: &[String],
) -> Option<Action> {
    let p = palette(ui.ctx());
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_100);
    let path = editors.tabs[tab].doc.path.clone();
    let dir = path.parent().unwrap_or(&workspace.root).to_path_buf();
    if manifests.schema.is_none() {
        manifests.schema = Some(
            system
                .schema(false)
                .and_then(|t| serde_json::from_str(&t).ok()),
        );
    }
    let Manifests {
        views,
        schema,
        deps: deps_state,
        profiles,
    } = manifests;
    let state = views.entry(path.clone()).or_default();
    let doc = &editors.tabs[tab].doc;
    if state.parsed.as_ref().is_none_or(|(v, _)| *v != doc.version) {
        let root = serde_json::from_str::<Json>(&doc.text()).unwrap_or(Json::Null);
        state.parsed = Some((doc.version, root));
    }
    let root = state
        .parsed
        .as_ref()
        .map(|(_, r)| r.clone())
        .unwrap_or(Json::Null);
    let deps_section = SECTIONS
        .iter()
        .position(|(n, _)| *n == "dependencies")
        .unwrap_or(0);

    // The sections.
    let list = Rect::from_min_size(
        rect.min,
        Vec2::new(180.0_f32.min(rect.width() * 0.3), rect.height()),
    );
    let body = Rect::from_min_max(egui::pos2(list.right(), rect.top()), rect.max);
    ui.painter().vline(
        list.right() - 0.5,
        list.y_range(),
        egui::Stroke::new(1.0, p.line),
    );
    ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
        egui::ScrollArea::vertical()
            .id_salt(("manifest-sections", &path))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(6.0);
                for (i, (name, _)) in SECTIONS.iter().enumerate() {
                    let current = state.query.trim().is_empty() && state.section == i;
                    if crate::settings::nav(ui, name, current, p) {
                        state.section = i;
                        state.query.clear();
                        if i == deps_section {
                            deps_state.opened();
                        }
                    }
                }
            });
    });

    let mut changes: Vec<(Vec<Key>, Option<Json>, bool)> = Vec::new();
    let mut go = None;
    ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
        // A requirement that excludes the wowlua built in, with its fix.
        if dir == workspace.root
            && let Some(why) = &workspace.cannot_check
        {
            let wanted = requirement_for(wowlua_version);
            forms::padded(ui, 16, 8, |ui| {
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
        let query = state.query.trim().to_lowercase();
        let (name, desc) = SECTIONS.get(state.section).copied().unwrap_or(SECTIONS[0]);
        let all = rows();
        let mut fx = Fx {
            root: &root,
            changes: Vec::new(),
            buffers: &mut state.buffers,
            profiles: world_profiles,
            wowlua: wowlua_version,
        };
        if query.is_empty() && state.section == deps_section {
            // The dependencies: the TOC's other AddOns, then the view of
            // the package's dependencies from the registry.
            forms::padded(ui, 24, 10, |ui| {
                ui.set_width(ui.available_width());
                ui.label(egui::RichText::new(name).font(Face::PlexMedium.at(15.0)).color(p.ink));
                caption(ui, desc, p);
                ui.add_space(8.0);
                for r in all.iter().filter(|r| r.section == name && !matches!(r.how, How::Link(_))) {
                    row(ui, &mut fx, r, p, &mut go);
                }
            });
            changes.append(&mut fx.changes);
            deps_state.package(&dir, workspace);
            deps::show(ui, deps_state, workspace, system);
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt(("manifest-rows", &path))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                forms::padded(ui, 24, 10, |ui| {
                    ui.set_width(ui.available_width());
                    if query.is_empty() {
                        ui.label(egui::RichText::new(name).font(Face::PlexMedium.at(15.0)).color(p.ink));
                        caption(ui, desc, p);
                        ui.add_space(8.0);
                        match name {
                            "flavors" => flavors(ui, &mut fx, &mut state.new_flavor, p),
                            "other keys" => other_keys(ui, &mut fx, schema.as_ref().and_then(Option::as_ref), p),
                            _ => {}
                        }
                    }
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
                    if query.is_empty() && name == "build" {
                        let view = profiles.entry(dir.clone()).or_default();
                        let schema = schema.as_ref().and_then(Option::as_ref);
                        profiles_section(ui, view, &root, schema, &mut fx.changes, system, &dir, p);
                    }
                    caption(
                        ui,
                        "Every key is the schema's, with completion and checks in the json view too. An edit rewrites only that value in wowlua.json, keeping its order.",
                        p,
                    );
                });
            });
        changes.append(&mut fx.changes);
    });
    apply(editors, tab, changes);
    match go {
        // (the section of this tab)
        Some(Go::Dependencies) => {
            state.section = deps_section;
            state.query.clear();
            deps_state.opened();
            None
        }
        other => other.map(Action::Go),
    }
}

// ---------------------------------------------------------- build profiles

type Change = (Vec<Key>, Option<Json>, bool);

/// A profile's settings the designed rows draw; the schema's form has the
/// others.
const PROFILE_DESIGNED: &[&str] = &[
    "inherits",
    "bundle",
    "minify",
    "minify-libraries",
    "optimize",
    "exclude",
    "config",
];

/// A profile's settings in wowlua's schema: its fields, by name.
fn profile_fields(schema: &Json) -> Vec<schema::Field> {
    use crate::editor::json::resolve;
    let Some(profiles) = schema.get("properties").and_then(|p| p.get("profiles")) else {
        return Vec::new();
    };
    let profiles = resolve(schema, profiles);
    let Some(settings) = profiles.get("additionalProperties") else {
        return Vec::new();
    };
    schema::fields(schema, settings).0
}

/// A profile's edits of `wowlua.json`.
struct ProfileFx<'a> {
    root: &'a Json,
    profile: &'a BuildProfile,
    changes: &'a mut Vec<Change>,
    buffers: &'a mut HashMap<String, String>,
}

impl ProfileFx<'_> {
    /// The value this profile sets.
    fn own(&self, keys: &[&str]) -> Option<&Json> {
        let mut v = self.root.get("profiles")?.get(&self.profile.name)?;
        for k in keys {
            v = v.get(*k)?;
        }
        Some(v)
    }

    /// The value as wowlua resolves it (as of the last read).
    fn effective(&self, keys: &[&str]) -> Option<&Json> {
        let mut v = &self.profile.effective;
        for k in keys {
            v = v.get(*k)?;
        }
        Some(v)
    }

    fn set(&mut self, keys: &[&str], value: Option<Json>, typing: bool) {
        let mut path: Vec<Key> = vec!["profiles".into(), Key::Name(self.profile.name.clone())];
        path.extend(keys.iter().map(|k| Key::Name((*k).to_owned())));
        self.changes.push((path, value, typing));
    }
}

/// How a profile's setting is set.
enum Ctl<'a> {
    Flag,
    Pick(&'a [String]),
    Text(&'a Kind, &'a str),
    /// A number one step at a time (S065): its unit and presets.
    Stepper(&'a str, &'a [(&'a str, f64)]),
}

/// A profile's setting as a row: its name and help, its control, and
/// whether it is inherited (or a reset of the profile's own value).
fn profile_row(
    ui: &mut Ui,
    fx: &mut ProfileFx<'_>,
    label: &str,
    keys: &[&str],
    help: &str,
    ctl: Ctl<'_>,
    p: &Palette,
) {
    let width = ui.available_width();
    let control = 380.0_f32.min(width * 0.5);
    let path_text = format!("profiles.{}.{}", fx.profile.name, keys.join("."));
    forms::centred_row(
        ui,
        ("profile", path_text.clone()),
        width - control - 16.0,
        control,
        |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(label)
                        .font(Face::PlexMedium.at(12.5))
                        .color(p.ink),
                );
                ui.label(
                    egui::RichText::new(&path_text)
                        .font(Face::Plex.at(11.0))
                        .color(p.ink_faint),
                );
            });
            if !help.is_empty() {
                caption(ui, help, p);
            }
        },
        |ui| {
            let own = fx.own(keys).cloned();
            let effective = fx.effective(keys).cloned();
            ui.horizontal_wrapped(|ui| {
                match ctl {
                    Ctl::Flag => {
                        let mut on = own
                            .as_ref()
                            .or(effective.as_ref())
                            .and_then(Json::as_bool)
                            .unwrap_or(false);
                        let word = if on { "on" } else { "off" };
                        if forms::checkbox(ui, &mut on, word, None) {
                            fx.set(keys, Some(Json::Bool(on)), false);
                        }
                    }
                    Ctl::Pick(values) => {
                        let now = own
                            .as_ref()
                            .or(effective.as_ref())
                            .and_then(Json::as_str)
                            .unwrap_or("")
                            .to_owned();
                        let mut list = values.to_vec();
                        if !now.is_empty() && !list.contains(&now) {
                            list.push(now.clone());
                        }
                        let mut index = list.iter().position(|v| *v == now).unwrap_or(0);
                        let opts: Vec<(&str, bool)> =
                            list.iter().map(|o| (o.as_str(), true)).collect();
                        let id = format!("mf-{path_text}");
                        if forms::select(ui, &id, &opts, &mut index, control.min(240.0)) {
                            fx.set(keys, Some(Json::String(list[index].clone())), false);
                        }
                    }
                    Ctl::Stepper(unit, presets) => {
                        let before = own
                            .as_ref()
                            .or(effective.as_ref())
                            .and_then(Json::as_f64)
                            .unwrap_or(0.0);
                        let mut v = before;
                        controls::stepper(ui, &path_text, &mut v, Some(0.0), None, 0.5, unit);
                        if let Some(picked) = controls::presets(ui, presets, v) {
                            v = picked;
                        }
                        if (v - before).abs() > 1e-9 {
                            fx.set(keys, Some(json_number(v)), false);
                        }
                    }
                    Ctl::Text(kind, hint) => {
                        let how = match kind {
                            Kind::Number => How::Number,
                            Kind::Numbers | Kind::Json => How::List,
                            _ => How::Text,
                        };
                        let text = shown(own.as_ref(), &how);
                        let inherited = shown(effective.as_ref(), &how);
                        let buffer = fx
                            .buffers
                            .entry(path_text.clone())
                            .or_insert_with(|| text.clone());
                        let hint = if inherited.is_empty() {
                            hint.to_owned()
                        } else {
                            inherited
                        };
                        let response = forms::text_input(ui, buffer, control.min(240.0), &hint);
                        if response.changed() {
                            if let Ok(value) = parse(buffer, &how) {
                                let value = match (&value, &how) {
                                    (Some(_), How::Text) => Some(Json::String(buffer.clone())),
                                    _ => value,
                                };
                                fx.set(keys, value, true);
                            }
                        } else if !response.has_focus() {
                            *buffer = text;
                        }
                    }
                }
                if own.is_some() {
                    if forms::text_button(ui, "reset").clicked() {
                        fx.set(keys, None, false);
                    }
                } else {
                    ui.label(
                        egui::RichText::new("inherited")
                            .font(Face::Plex.at(10.5))
                            .color(p.ink_faint),
                    );
                }
            });
        },
    );
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

/// The build section's profiles (S064): one shown at a time, each of its
/// settings set here or inherited; a new one added.
#[allow(clippy::too_many_arguments)]
fn profiles_section(
    ui: &mut Ui,
    view: &mut ProfilesView,
    root: &Json,
    schema: Option<&Json>,
    changes: &mut Vec<Change>,
    system: &dyn System,
    dir: &Path,
    p: &Palette,
) {
    if (view.read.is_none() || view.stale) && !view.reading {
        view.reading = true;
        view.stale = false;
        system.profiles_read(dir);
    }
    ui.add_space(14.0);
    ui.label(
        egui::RichText::new("profiles")
            .font(Face::PlexMedium.at(14.0))
            .color(p.ink),
    );
    caption(
        ui,
        "How wowlua build makes each profile. The ship page builds them; their settings are edited here. A setting not set comes from what the profile inherits, or wowlua's default.",
        p,
    );
    ui.add_space(6.0);
    let profiles = match &view.read {
        Some(Ok(list)) => list.clone(),
        Some(Err(why)) => {
            forms::callout(ui, CalloutKind::Error, |ui| {
                forms::paragraph(ui, why, 12.0, 18.0, p.ink);
            });
            return;
        }
        None => {
            caption(ui, "reading the profiles…", p);
            return;
        }
    };
    if view
        .selected
        .as_ref()
        .is_none_or(|s| !profiles.iter().any(|p| &p.name == s))
    {
        view.selected = profiles
            .iter()
            .find(|p| p.name == "release")
            .or(profiles.first())
            .map(|p| p.name.clone());
    }
    // The profiles, and a new one.
    let names: Vec<&str> = profiles.iter().map(|p| p.name.as_str()).collect();
    let at = names
        .iter()
        .position(|n| Some(*n) == view.selected.as_deref())
        .unwrap_or(0);
    let mut added: Option<Option<String>> = None;
    ui.horizontal_wrapped(|ui| {
        if let Some(i) = widgets::seg(
            ui,
            &format!("mf-profiles-{}", dir.display()),
            &names,
            at,
            true,
        ) {
            view.selected = Some(names[i].to_owned());
        }
        match &mut view.new_profile {
            None => {
                if forms::text_button(ui, "new profile").clicked() {
                    view.new_profile = Some(String::new());
                }
            }
            Some(name) => {
                forms::text_input(ui, name, 160.0, "readable");
                let valid = !name.trim().is_empty() && !names.contains(&name.trim());
                ui.add_enabled_ui(valid, |ui| {
                    if widgets::button(
                        ui,
                        ButtonKind::Secondary,
                        "add, inheriting release",
                        true,
                        None,
                    )
                    .clicked()
                    {
                        added = Some(Some(name.trim().to_owned()));
                    }
                });
                if forms::text_button(ui, "cancel").clicked() {
                    added = Some(None);
                }
            }
        }
    });
    if let Some(added) = added {
        view.new_profile = None;
        if let Some(name) = added {
            changes.push((
                vec!["profiles".into(), Key::Name(name.clone())],
                Some(serde_json::json!({ "inherits": "release" })),
                false,
            ));
            view.selected = Some(name);
            view.stale = true;
        }
    }
    let Some(profile) = view
        .selected
        .as_ref()
        .and_then(|s| profiles.iter().find(|p| &p.name == s))
    else {
        return;
    };
    ui.add_space(4.0);
    if profile.chain.len() > 1 {
        caption(
            ui,
            &format!(
                "{} inherits {}",
                profile.name,
                profile.chain[..profile.chain.len() - 1].join(" → ")
            ),
            p,
        );
    }
    if let Some(why) = &profile.problem {
        forms::callout(ui, CalloutKind::Error, |ui| {
            forms::paragraph(ui, why, 12.0, 18.0, p.ink);
        });
        return;
    }
    ui.add_space(6.0);
    let mut fx = ProfileFx {
        root,
        profile,
        changes,
        buffers: &mut view.buffers,
    };
    let levels: Vec<String> = ["none", "strip", "compact", "rename", "max"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    let cache: Vec<String> = vec!["off".into(), "safe".into()];
    profile_row(
        ui,
        &mut fx,
        "bundle",
        &["bundle"],
        "Into one file.",
        Ctl::Flag,
        p,
    );
    profile_row(
        ui,
        &mut fx,
        "minify",
        &["minify"],
        "",
        Ctl::Pick(&levels),
        p,
    );
    profile_row(
        ui,
        &mut fx,
        "minify libraries",
        &["minify-libraries"],
        "",
        Ctl::Flag,
        p,
    );
    profile_row(
        ui,
        &mut fx,
        "inline",
        &["optimize", "inline"],
        "",
        Ctl::Flag,
        p,
    );
    profile_row(
        ui,
        &mut fx,
        "cache globals",
        &["optimize", "cache-globals"],
        "",
        Ctl::Pick(&cache),
        p,
    );
    profile_row(
        ui,
        &mut fx,
        "remove dead code",
        &["optimize", "dead-code"],
        "",
        Ctl::Flag,
        p,
    );
    profile_row(
        ui,
        &mut fx,
        "constants",
        &["optimize", "constants"],
        "",
        Ctl::Flag,
        p,
    );
    profile_row(
        ui,
        &mut fx,
        "exclude",
        &["exclude"],
        "Files left out of this profile's build (globs).",
        Ctl::Text(&Kind::Numbers, "src/Diagnostics/**"),
        p,
    );
    let fields = schema.map(profile_fields).unwrap_or_default();
    let mut left = Vec::new();
    for f in fields
        .iter()
        .filter(|f| !PROFILE_DESIGNED.contains(&f.name.as_str()))
    {
        let keys = [f.name.as_str()];
        match &f.kind {
            Kind::Bool => profile_row(ui, &mut fx, &f.name, &keys, &f.help, Ctl::Flag, p),
            Kind::Choice(values) => {
                profile_row(ui, &mut fx, &f.name, &keys, &f.help, Ctl::Pick(values), p)
            }
            Kind::Number => {
                // (the dev profile's budgets in ms, with the usual ones)
                let presets: &[(&str, f64)] = match f.name.as_str() {
                    "frame-budget" => &[("0.5", 0.5), ("1", 1.0), ("2", 2.0), ("4", 4.0)],
                    "slow-call" => &[("1", 1.0), ("2", 2.0), ("5", 5.0), ("10", 10.0)],
                    _ => &[],
                };
                let unit = if presets.is_empty() { "" } else { "ms" };
                profile_row(
                    ui,
                    &mut fx,
                    &f.name,
                    &keys,
                    &f.help,
                    Ctl::Stepper(unit, presets),
                    p,
                )
            }
            Kind::Text | Kind::Scalar => profile_row(
                ui,
                &mut fx,
                &f.name,
                &keys,
                &f.help,
                Ctl::Text(&f.kind, ""),
                p,
            ),
            _ => left.push(f.name.clone()),
        }
    }
    if fields.is_empty() {
        caption(
            ui,
            "wowlua's schema is not here: edit the other settings in the json.",
            p,
        );
    } else if !left.is_empty() {
        caption(ui, &format!("In the json: {}.", left.join(", ")), p);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_next_versions_are_suggested() {
        assert_eq!(next_versions("0.6.0-beta.1"), ["0.6.0-beta.2", "0.6.0"]);
        assert_eq!(next_versions("1.2.3"), ["1.2.4", "1.3.0", "1.3.0-beta.1"]);
    }

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
