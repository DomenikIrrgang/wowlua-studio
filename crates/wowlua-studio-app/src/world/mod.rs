//! The world profiles page (M8, S060): the package's profiles
//! (`tests/profiles/<name>.json`), their parts, a form for each over the
//! file's JSON (the same document as the json view and an editor tab: a
//! form's edit changes only its value's text), checks as you type (E4400
//! to E4402), and the live world: edits applied to a session started in
//! the profile, snapshots taken in the game, where a profile is used.

pub mod json;
mod parts;
pub mod schema;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use egui::{Align2, CornerRadius, Rect, Sense, Ui, Vec2};
use serde_json::Value as Json;
use wowlua_studio_ui::forms::{self, CalloutKind};
use wowlua_studio_ui::widgets::{self, BadgeKind, ButtonKind};
use wowlua_studio_ui::{Face, Icon, Palette, palette};

use crate::debug::DebugState;
use crate::editor::Editors;
use crate::editor::doc::{EditKind, Pos};
use crate::lsp::Diag;
use crate::system::Workspace;
use crate::system::{
    Art, ClassSpell, GameRequest, Image, ProfileProblem, SpellRow, System, WorldProfile,
    WorldSnapshot,
};
use json::Key;

/// The schema a new profile names, for editors.
pub const SCHEMA_URL: &str =
    "https://raw.githubusercontent.com/DomenikIrrgang/wowlua/main/schema/world-profile-0.1.json";

/// A profile's parts as the page lists them: the key, its label, what it
/// is (wowlua's schema has these 22).
pub const PARTS: &[(&str, &str, &str)] = &[
    (
        "character",
        "character",
        "The player's character (mock.setPlayer).",
    ),
    (
        "position",
        "position",
        "Where the character is (mock.moveTo): map, zone, x and y, world coordinates, facing.",
    ),
    (
        "units",
        "group and units",
        "The units around: creatures and other players, your group, target and focus (mock.spawnUnit).",
    ),
    (
        "auras",
        "auras",
        "The character's auras (mock.player:applyAura).",
    ),
    (
        "spells",
        "spells",
        "The spells the character has trained (trained, mock.player:learn), and spells whose data the profile overrides (mock.defineSpell).",
    ),
    (
        "actions",
        "action bars",
        "The action bars' actions by slot (mock.setAction).",
    ),
    (
        "equipment",
        "equipment",
        "What is worn, by slot ID or the client's slot name.",
    ),
    ("bags", "bags", "The bags by bag ID (0 the backpack)."),
    ("items", "items", "Item data by item ID (mock.defineItem)."),
    ("money", "money", "The character's money, in copper."),
    (
        "stats",
        "stats",
        "The character's stats by key (mock.player:setStats).",
    ),
    (
        "macros",
        "macros",
        "The account's and the character's macros (mock.setMacros).",
    ),
    (
        "mounts",
        "mounts",
        "The mount journal by mount ID (mock.setMounts).",
    ),
    ("toys", "toys", "The toy box by item ID (mock.setToys)."),
    (
        "appearances",
        "appearances",
        "The wardrobe (mock.setAppearances).",
    ),
    (
        "channels",
        "chat channels",
        "The chat channels the character is in (mock.setChannels).",
    ),
    ("cvars", "cvars", "CVars by name."),
    (
        "bindings",
        "key bindings",
        "Key bindings: key to action (SetBinding).",
    ),
    (
        "savedVariables",
        "SavedVariables",
        "SavedVariables by global name, as the addon finds them at load.",
    ),
    (
        "screen",
        "screen",
        "The screen the client draws on: its size, DPI scale and UI scale (over the manifest's test.screen).",
    ),
    (
        "client",
        "client",
        "The client a recorded profile comes from (/wl snapshot).",
    ),
    (
        "leaderBoards",
        "leader boards",
        "The leader boards a snapshot recorded: kept, not applied.",
    ),
];

/// The parts drawn by a form of their own; the others' form comes from
/// the schema.
const DESIGNED: &[&str] = &[
    "character",
    "units",
    "auras",
    "spells",
    "actions",
    "equipment",
    "bags",
    "savedVariables",
];

/// Game art read from the installation, as textures (S060): asked for
/// the first time a form wants it; the design's stand-in until then, or
/// when the installation has none.
#[derive(Default)]
pub struct ArtCache {
    textures: std::cell::RefCell<HashMap<Art, Option<egui::TextureHandle>>>,
}

impl ArtCache {
    pub fn get(&self, system: &dyn System, flavor: &str, art: Art) -> Option<egui::TextureId> {
        let mut textures = self.textures.borrow_mut();
        if let Some(t) = textures.get(&art) {
            return t.as_ref().map(egui::TextureHandle::id);
        }
        textures.insert(art.clone(), None);
        system.game_art(flavor, art);
        None
    }

    /// Art read: its texture.
    pub fn read(&self, ctx: &egui::Context, art: Art, image: Option<Image>) {
        let texture = image.filter(|i| i.width > 0 && i.height > 0).map(|i| {
            ctx.load_texture(
                format!("game-art-{art:?}"),
                egui::ColorImage::from_rgba_unmultiplied(
                    [i.width as usize, i.height as usize],
                    &i.rgba,
                ),
                egui::TextureOptions::LINEAR,
            )
        });
        self.textures.borrow_mut().insert(art, texture);
    }
}

/// The snapshots found, and warnings; or why none could be looked for.
pub type Snapshots = Result<(Vec<WorldSnapshot>, Vec<String>), String>;

/// A change a form makes: the value at `path` set (none: removed).
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub path: Vec<Key>,
    pub value: Option<Json>,
    /// Typed: joins the undo step before it.
    pub typing: bool,
}

/// The profile's text parsed, as of a version of its document.
struct Parsed {
    path: PathBuf,
    version: u64,
    value: Result<Json, String>,
    problems: Vec<ProfileProblem>,
    /// The parts with problems.
    wrong: Vec<String>,
}

/// The page's state.
pub struct WorldState {
    pub profiles: Vec<WorldProfile>,
    listed_at: Option<f64>,
    pub selected: Option<String>,
    pub part: String,
    pub json_view: bool,
    parsed: Option<Parsed>,
    /// The text of fields being typed in, by their path.
    pub(crate) buffers: HashMap<String, String>,
    /// The unit picked in the units part (none: the player).
    pub(crate) unit: Option<usize>,
    /// The entry, slot or aura picked in a part.
    pub(crate) entry: usize,
    pub(crate) slot: usize,
    pub(crate) bag: usize,
    new_name: Option<String>,
    uses: Option<(String, Vec<String>)>,
    /// Edits go to a session started in the profile.
    pub auto_apply: bool,
    /// What was applied to the running world: what, and the frame.
    pub applied: Vec<(String, String)>,
    /// Scripts for the running world (`mock`), each with what it applies.
    live: Vec<(String, String)>,
    pub snapshots: Option<Snapshots>,
    snapshot_name: String,
    /// The schema (wowlua's), read once.
    schema: Option<Option<Json>>,
    /// The installation's art.
    pub art: ArtCache,
    /// The spells part's search: what is typed, what was asked, the
    /// spells found for it.
    pub(crate) spell_query: String,
    spell_asked: String,
    pub(crate) spells: Option<(String, Vec<SpellRow>)>,
    /// The class's spells from the client, as asked for a class.
    class_asked: String,
    pub(crate) class_spells: Option<(String, Vec<ClassSpell>)>,
    /// The spells part shows every rank and level.
    pub(crate) all_ranks: bool,
    /// Changes made from outside a form (the screenshot harness).
    pub pending: Vec<Change>,
}

impl Default for WorldState {
    fn default() -> Self {
        WorldState {
            profiles: Vec::new(),
            listed_at: None,
            selected: None,
            part: "character".into(),
            json_view: false,
            parsed: None,
            buffers: HashMap::new(),
            unit: None,
            entry: 0,
            slot: 0,
            bag: 0,
            new_name: None,
            uses: None,
            auto_apply: true,
            applied: Vec::new(),
            live: Vec::new(),
            snapshots: None,
            snapshot_name: String::new(),
            schema: None,
            art: ArtCache::default(),
            spell_query: String::new(),
            spell_asked: String::new(),
            spells: None,
            class_asked: String::new(),
            class_spells: None,
            all_ranks: false,
            pending: Vec::new(),
        }
    }
}

/// What the page asks Studio to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Open a file in the editor.
    Open(PathBuf),
    /// Run the tests that start in the profile.
    RunTests(String),
    /// Start a session of the addon in the profile.
    Start(String),
    /// Save the profile's document.
    Save(PathBuf),
    /// A message: a title and its text.
    Note(String, String),
}

impl WorldState {
    /// The selected profile.
    pub fn profile(&self) -> Option<&WorldProfile> {
        let name = self.selected.as_ref()?;
        self.profiles.iter().find(|p| &p.name == name)
    }

    /// Lists the profiles again every 2 seconds while the page shows.
    fn list(&mut self, workspace: &Workspace, system: &dyn System, now: f64) {
        if self.listed_at.is_some_and(|t| now - t < 2.0) {
            return;
        }
        self.listed_at = Some(now);
        self.profiles = system.world_profiles(&workspace.root);
        if self
            .selected
            .as_ref()
            .is_none_or(|s| !self.profiles.iter().any(|p| &p.name == s))
        {
            self.selected = self.profiles.first().map(|p| p.name.clone());
        }
    }

    /// The profile's document held, and parsed as of its version.
    fn refresh(&mut self, editors: &mut Editors, system: &dyn System) -> Option<usize> {
        let path = self.profile()?.path.clone();
        let i = editors.held(&path, system).ok()?;
        let doc = &editors.tabs[i].doc;
        if self
            .parsed
            .as_ref()
            .is_some_and(|p| p.path == path && p.version == doc.version)
        {
            return Some(i);
        }
        let text = doc.text();
        let value = serde_json::from_str::<Json>(&text).map_err(|e| e.to_string());
        let problems = system.check_world_profile(&text);
        let spans: Vec<(String, std::ops::Range<usize>)> = PARTS
            .iter()
            .filter_map(|(k, _, _)| Some((k.to_string(), json::span(&text, &[(*k).into()])?)))
            .collect();
        // Each problem's part: the one it is in, or the one a misspelt
        // part was meant to be.
        let wrong = problems
            .iter()
            .filter_map(|pr| {
                spans
                    .iter()
                    .find(|(_, s)| s.start <= pr.start && pr.start < s.end)
                    .map(|(k, _)| k.clone())
                    .or_else(|| unknown_part(&text, pr).map(str::to_owned))
            })
            .collect();
        self.parsed = Some(Parsed {
            path,
            version: doc.version,
            value,
            problems,
            wrong,
        });
        Some(i)
    }

    fn schema(&mut self, system: &dyn System) -> Option<&Json> {
        self.schema
            .get_or_insert_with(|| {
                system
                    .schema(true)
                    .and_then(|t| serde_json::from_str(&t).ok())
            })
            .as_ref()
    }

    /// The profiles as listed, kept until `until` (seconds of the UI's
    /// clock; the frame budget's test).
    pub fn listed(&mut self, profiles: Vec<WorldProfile>, until: f64) {
        self.selected = profiles.first().map(|p| p.name.clone());
        self.profiles = profiles;
        self.listed_at = Some(until);
    }

    /// Scripts for the running world, taken.
    pub fn take_live(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.live)
    }

    /// The client's spells found for a search.
    pub fn spells_found(&mut self, query: String, found: Vec<SpellRow>) {
        if query == self.spell_asked {
            self.spells = Some((query, found));
        }
    }

    /// A class's spells from the client.
    pub fn class_spells_found(&mut self, class: String, found: Vec<ClassSpell>) {
        if class == self.class_asked {
            self.class_spells = Some((class, found));
        }
    }

    /// The snapshots read.
    pub fn snapshots_read(&mut self, result: Snapshots) {
        self.snapshots = Some(result);
    }
}

// ------------------------------------------------------------- profiles --

/// The profiles panel: the package's profiles, a new one.
pub fn profiles(
    ui: &mut Ui,
    state: &mut WorldState,
    editors: &mut Editors,
    workspace: &Workspace,
    system: &dyn System,
) -> Option<Action> {
    let p = palette(ui.ctx());
    let now = ui.input(|i| i.time);
    state.list(workspace, system, now);
    state.refresh(editors, system);
    let mut action = None;
    egui::ScrollArea::vertical()
        .id_salt("world-profiles")
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.add_space(4.0);
            if forms::text_button(ui, "+ new profile").clicked() {
                state.new_name = Some(String::new());
            }
            if let Some(name) = &mut state.new_name {
                forms::padded(ui, 12, 6, |ui| {
                    let r = forms::text_input(ui, name, (ui.available_width() - 70.0).max(80.0), "raid-pull");
                    let ok = !name.is_empty()
                        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if (forms::text_button(ui, "add").clicked() || enter) && ok {
                        let path = workspace
                            .root
                            .join("tests/profiles")
                            .join(format!("{name}.json"));
                        let text = format!("{{\n  \"$schema\": \"{SCHEMA_URL}\"\n}}\n");
                        match system.write_file(&path, &text) {
                            Ok(()) => {
                                state.selected = Some(name.clone());
                                state.listed_at = None;
                            }
                            Err(e) => action = Some(Action::Note("new profile".into(), e)),
                        }
                    }
                });
                if state.selected.as_ref().is_some_and(|s| state.new_name.as_ref() == Some(s)) {
                    state.new_name = None;
                }
            }
            if state.profiles.is_empty() {
                forms::padded(ui, 12, 12, |ui| {
                    forms::paragraph(
                        ui,
                        "No profiles yet. Tests and sessions start in a plain level 1 character in an empty world.",
                        12.5,
                        19.0,
                        p.ink_muted,
                    );
                });
                return;
            }
            let problems = state
                .parsed
                .as_ref()
                .map(|x| (x.path.clone(), x.problems.len()));
            for i in 0..state.profiles.len() {
                let profile = state.profiles[i].clone();
                let selected = state.selected.as_ref() == Some(&profile.name);
                let count = problems
                    .as_ref()
                    .filter(|(path, _)| path == &profile.path)
                    .map_or(0, |(_, n)| *n);
                if row(ui, p, &profile.name, count, selected) {
                    state.selected = Some(profile.name.clone());
                    state.unit = None;
                    state.entry = 0;
                }
            }
        });
    action
}

/// A profile's row; whether it was clicked.
fn row(ui: &mut Ui, p: &Palette, name: &str, problems: usize, selected: bool) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::click());
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, CornerRadius::ZERO, p.bg_400);
        painter.rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(2.0, rect.height())),
            CornerRadius::ZERO,
            p.amber,
        );
    } else if response.hovered() {
        painter.rect_filled(rect, CornerRadius::ZERO, p.bg_300);
    }
    let cy = rect.center().y;
    widgets::text_at(
        ui,
        egui::pos2(rect.left() + 12.0, cy),
        Align2::LEFT_CENTER,
        name,
        Face::Plex.at(12.5),
        p.ink,
    );
    if problems > 0 {
        ui.painter().text(
            egui::pos2(rect.right() - 12.0, cy),
            Align2::RIGHT_CENTER,
            problems.to_string(),
            Face::PlexSemiBold.at(11.0),
            p.error,
        );
    }
    response.clicked()
}

// ---------------------------------------------------------------- parts --

/// What a set part holds, in a few words.
fn summary(key: &str, v: &Json) -> String {
    let count = |v: &Json| match v {
        Json::Array(a) => a.len(),
        Json::Object(o) => o.len(),
        _ => 0,
    };
    match key {
        "character" => format!(
            "{} {}",
            v["class"].as_str().unwrap_or(""),
            v["level"]
                .as_f64()
                .map(|l| format!("{l}"))
                .unwrap_or_default()
        )
        .trim()
        .to_string(),
        "position" => v["zone"].as_str().unwrap_or("set").to_string(),
        "money" => v.as_f64().map(money).unwrap_or_default(),
        "client" => v.as_str().unwrap_or("set").chars().take(12).collect(),
        "spells" if v.is_string() => v.as_str().unwrap_or_default().to_string(),
        "savedVariables" => v
            .as_object()
            .map(|o| o.keys().cloned().collect::<Vec<_>>().join(", "))
            .unwrap_or_default(),
        "units" => {
            let units = list(v);
            let grouped = units
                .iter()
                .filter(|(_, u)| u.get("group").is_some())
                .count();
            format!("{grouped} + {}", units.len() - grouped)
        }
        _ => match count(v) {
            0 => "set".into(),
            n => n.to_string(),
        },
    }
}

/// Copper as gold, silver and copper.
pub fn money(copper: f64) -> String {
    let c = copper.max(0.0) as u64;
    format!("{}g {}s {}c", c / 10_000, c / 100 % 100, c % 100)
}

/// A list's items with their keys (an array's places, an object's keys).
pub(crate) fn list(v: &Json) -> Vec<(Key, &Json)> {
    match v {
        Json::Array(a) => a
            .iter()
            .enumerate()
            .map(|(i, v)| (Key::Index(i), v))
            .collect(),
        Json::Object(o) => o.iter().map(|(k, v)| (Key::Name(k.clone()), v)).collect(),
        _ => Vec::new(),
    }
}

/// The parts panel: each part, whether the profile sets it.
pub fn parts_panel(
    ui: &mut Ui,
    state: &mut WorldState,
    editors: &mut Editors,
    workspace: &Workspace,
    system: &dyn System,
) {
    let p = palette(ui.ctx());
    let now = ui.input(|i| i.time);
    state.list(workspace, system, now);
    state.refresh(editors, system);
    let root = state
        .parsed
        .as_ref()
        .and_then(|x| x.value.as_ref().ok())
        .cloned()
        .unwrap_or(Json::Null);
    let wrong: Vec<String> = state
        .parsed
        .as_ref()
        .map(|x| x.wrong.clone())
        .unwrap_or_default();
    let set = PARTS
        .iter()
        .filter(|(k, _, _)| {
            root.get(*k).is_some() || (*k == "spells" && root.get("trained").is_some())
        })
        .count();
    forms::padded(ui, 12, 4, |ui| {
        ui.label(
            egui::RichText::new(format!("{set} of {} set", PARTS.len()))
                .font(Face::Plex.at(11.0))
                .color(p.ink_faint),
        );
    });
    egui::ScrollArea::vertical()
        .id_salt("world-parts")
        .auto_shrink(false)
        .show(ui, |ui| {
            for (key, label, _) in PARTS {
                // (the spells part holds `trained` too)
                let trained = (*key == "spells")
                    .then(|| root.get("trained"))
                    .flatten()
                    .map(|t| Json::String(format!("{} trained", list(t).len())));
                let value = trained.as_ref().or(root.get(*key));
                let (rect, response) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::click());
                let current = state.part == *key;
                let painter = ui.painter();
                if current {
                    painter.rect_filled(rect, CornerRadius::ZERO, p.bg_400);
                    painter.rect_filled(
                        Rect::from_min_size(rect.min, Vec2::new(2.0, rect.height())),
                        CornerRadius::ZERO,
                        p.amber,
                    );
                } else if response.hovered() {
                    painter.rect_filled(rect, CornerRadius::ZERO, p.bg_300);
                }
                let cy = rect.center().y;
                let dot = if wrong.iter().any(|w| w == key) {
                    p.error
                } else if value.is_some() {
                    p.amber
                } else {
                    p.line_strong
                };
                painter.circle_filled(egui::pos2(rect.left() + 14.0, cy), 3.0, dot);
                painter.text(
                    egui::pos2(rect.left() + 24.0, cy),
                    Align2::LEFT_CENTER,
                    *label,
                    Face::Plex.at(12.5),
                    if value.is_some() { p.ink } else { p.ink_muted },
                );
                if let Some(v) = value {
                    let mut s = summary(key, v);
                    if s.chars().count() > 14 {
                        s = s.chars().take(13).collect::<String>() + "…";
                    }
                    painter.text(
                        egui::pos2(rect.right() - 10.0, cy),
                        Align2::RIGHT_CENTER,
                        s,
                        Face::Plex.at(11.0),
                        p.ink_muted,
                    );
                }
                if response.clicked() {
                    state.part = (*key).to_string();
                    state.entry = 0;
                    state.slot = 0;
                }
            }
        });
}

// --------------------------------------------------------------- editor --

/// The profile editor: its bar, its problems, then the part's form (or,
/// in the json view, the rectangle the document's code goes in, which
/// the caller draws with the editor's context). `diagnostics`: the
/// language server's, for E4402.
pub fn editor(
    ui: &mut Ui,
    state: &mut WorldState,
    editors: &mut Editors,
    workspace: &Workspace,
    system: &dyn System,
    diagnostics: &BTreeMap<PathBuf, Vec<Diag>>,
    live: Option<&str>,
) -> (Option<(Rect, usize)>, Option<Action>) {
    let p = palette(ui.ctx());
    let now = ui.input(|i| i.time);
    state.list(workspace, system, now);
    let Some(tab) = state.refresh(editors, system) else {
        forms::padded(ui, 22, 18, |ui| {
            forms::paragraph(
                ui,
                "Pick a profile, or make one with + new profile: the world a test or a session starts in.",
                12.5,
                19.0,
                p.ink_muted,
            );
        });
        return (None, None);
    };
    let profile = state.profile().cloned().expect("refreshed");
    let mut action = None;
    if state.uses.as_ref().is_none_or(|(n, _)| n != &profile.name) {
        state.uses = Some((
            profile.name.clone(),
            system.profile_uses(&workspace.root, &profile.name),
        ));
    }
    let used = state.uses.as_ref().map_or(0, |(_, u)| u.len());
    let dirty = editors.tabs[tab].doc.is_dirty();

    // The bar.
    forms::padded(ui, 12, 6, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(&profile.name)
                    .font(Face::PlexMedium.at(12.5))
                    .color(p.ink),
            );
            let wide = ui.available_width() > 1000.0;
            if wide {
                ui.label(
                    egui::RichText::new(format!("tests/profiles/{}.json", profile.name))
                        .font(Face::Plex.at(11.0))
                        .color(p.ink_faint),
                );
            }
            if let Some(i) = widgets::seg(
                ui,
                "world-view",
                &["form", "json"],
                usize::from(state.json_view),
                false,
            ) {
                state.json_view = i == 1;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let word = if dirty { "save" } else { "saved" };
                if widgets::button(ui, ButtonKind::Primary, word, true, None).clicked() && dirty {
                    action = Some(Action::Save(profile.path.clone()));
                }
                if widgets::button(
                    ui,
                    ButtonKind::Secondary,
                    "start a session in it",
                    true,
                    None,
                )
                .clicked()
                {
                    action = Some(Action::Start(profile.name.clone()));
                }
                if widgets::button(ui, ButtonKind::Secondary, "run its tests", true, None).clicked()
                {
                    action = Some(Action::RunTests(profile.name.clone()));
                }
                if wide
                    && forms::text_button(
                        ui,
                        &format!("used by {used} place{}", if used == 1 { "" } else { "s" }),
                    )
                    .clicked()
                {
                    action = Some(Action::Open(profile.path.clone()));
                }
            });
        });
    });

    // Its problems, and the tests naming no profile (E4402).
    let text = editors.tabs[tab].doc.text();
    let problems = state
        .parsed
        .as_ref()
        .map(|x| x.problems.clone())
        .unwrap_or_default();
    let mut fix: Option<(std::ops::Range<usize>, String)> = None;
    for pr in problems.iter().take(3) {
        forms::callout(ui, CalloutKind::Error, |ui| {
            ui.horizontal_wrapped(|ui| {
                let (line, col) = line_col(&text, pr.start);
                ui.label(
                    egui::RichText::new(format!("error[{}]", pr.code))
                        .font(Face::PlexSemiBold.at(12.0))
                        .color(p.error),
                );
                ui.label(
                    egui::RichText::new(format!(
                        "{} ({}.json:{}:{})",
                        pr.message,
                        profile.name,
                        line + 1,
                        col + 1
                    ))
                    .font(Face::Plex.at(12.0))
                    .color(p.ink),
                );
                if let Some(part) = unknown_part(&text, pr) {
                    let range = pr.start..pr.end;
                    if widgets::button(
                        ui,
                        ButtonKind::Secondary,
                        &format!("change to {part}"),
                        true,
                        None,
                    )
                    .clicked()
                    {
                        fix = Some((range, format!("\"{part}\"")));
                    }
                }
            });
        });
    }
    for (path, list) in diagnostics {
        for d in list.iter().filter(|d| d.code.as_deref() == Some("E4402")) {
            let rel = path.strip_prefix(&workspace.root).unwrap_or(path);
            forms::callout(ui, CalloutKind::Error, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new("error[E4402]")
                            .font(Face::PlexSemiBold.at(12.0))
                            .color(p.error),
                    );
                    ui.label(
                        egui::RichText::new(format!("{} in {}", d.message, rel.display()))
                            .font(Face::Plex.at(12.0))
                            .color(p.ink),
                    );
                });
            });
        }
    }
    if let Some((range, with)) = fix {
        let doc = &mut editors.tabs[tab].doc;
        let (a, b) = (pos_of(&text, range.start), pos_of(&text, range.end));
        doc.edit(a, b, &with, EditKind::Other);
    }

    if state.json_view {
        let rect = ui.available_rect_before_wrap();
        ui.allocate_rect(rect, Sense::hover());
        return (Some((rect, tab)), action);
    }

    // The form.
    let root = match state.parsed.as_ref().map(|x| &x.value) {
        Some(Ok(v)) if v.is_object() => v.clone(),
        Some(Ok(_)) | Some(Err(_)) | None => {
            forms::padded(ui, 22, 18, |ui| {
                forms::paragraph(
                    ui,
                    "The file is no JSON object: fix it in the json view, the form shows it again then.",
                    12.5,
                    19.0,
                    p.ink_muted,
                );
            });
            return (None, action);
        }
    };
    let schema = state.schema(system).cloned();
    let part = state.part.clone();
    let (label, desc) = PARTS
        .iter()
        .find(|(k, _, _)| *k == part)
        .map(|(_, l, d)| (*l, *d))
        .unwrap_or(("", ""));
    let mut changes: Vec<Change> = Vec::new();
    egui::ScrollArea::vertical()
        .id_salt(("world-form", &part))
        .auto_shrink(false)
        .show(ui, |ui| {
            forms::padded(ui, 22, 18, |ui| {
                ui.label(
                    egui::RichText::new(label)
                        .font(Face::PlexSemiBold.at(15.0))
                        .color(p.ink),
                );
                ui.label(
                    egui::RichText::new(desc)
                        .font(Face::Plex.at(11.5))
                        .color(p.ink_muted),
                );
                ui.add_space(12.0);
                let flavor = workspace_flavor(workspace);
                // The spells part's search, asked as it is typed.
                if part == "spells" && state.spell_query != state.spell_asked {
                    state.spell_asked = state.spell_query.clone();
                    state.spells = None;
                    system.search_spells(&flavor, &state.spell_query);
                }
                // The class's spells, asked for the character's class.
                let class = root["character"]["class"]
                    .as_str()
                    .unwrap_or("")
                    .to_uppercase();
                if part == "spells" && !class.is_empty() && class != state.class_asked {
                    state.class_asked = class.clone();
                    state.class_spells = None;
                    system.class_spells(&flavor, &class);
                }
                let mut cx = parts::Cx {
                    root: &root,
                    changes: &mut changes,
                    buffers: &mut state.buffers,
                    unit: &mut state.unit,
                    entry: &mut state.entry,
                    slot: &mut state.slot,
                    bag: &mut state.bag,
                    look: parts::Look {
                        art: &state.art,
                        system,
                        flavor: &flavor,
                    },
                    system,
                    spell_query: &mut state.spell_query,
                    spells: state.spells.as_ref(),
                    class_spells: state.class_spells.as_ref(),
                    all_ranks: &mut state.all_ranks,
                };
                if DESIGNED.contains(&part.as_str()) {
                    if let Some(a) = parts::designed(ui, &mut cx, &part) {
                        action = Some(a);
                    }
                } else {
                    parts::generic(ui, &mut cx, &part, schema.as_ref());
                }
            });
        });
    // The changes, as edits of the document's text.
    changes.append(&mut state.pending);
    for change in changes {
        let text = editors.tabs[tab].doc.text();
        let Some((range, with)) = json::set(&text, &change.path, change.value.as_ref()) else {
            continue;
        };
        let (a, b) = (pos_of(&text, range.start), pos_of(&text, range.end));
        let kind = if change.typing {
            EditKind::Typing
        } else {
            EditKind::Other
        };
        editors.tabs[tab].doc.edit(a, b, &with, kind);
        if state.auto_apply && live == Some(profile.name.as_str()) {
            let before: Json = serde_json::from_str(&text).unwrap_or(Json::Null);
            let now_root: Json =
                serde_json::from_str(&editors.tabs[tab].doc.text()).unwrap_or(Json::Null);
            if let Some(script) = live_trained(&before, &now_root, &change.path)
                .or_else(|| live_script(&now_root, &change.path))
            {
                state.live.push(script);
            }
        }
    }
    (None, action)
}

/// The package's first flavor (the classes and races a character may have).
fn workspace_flavor(workspace: &Workspace) -> String {
    workspace.flavors.first().cloned().unwrap_or_default()
}

/// An E4401 on an unknown part a part's name was meant for: that name.
fn unknown_part(text: &str, pr: &ProfileProblem) -> Option<&'static str> {
    if pr.code != "E4401" || !pr.message.contains("unknown") {
        return None;
    }
    let key = text.get(pr.start..pr.end)?.trim_matches('"');
    PARTS
        .iter()
        .map(|(k, _, _)| *k)
        .filter(|k| distance(k, key) <= 2)
        .min_by_key(|k| distance(k, key))
}

/// The edit distance of two words.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let here = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != *cb))
                .min(row[j] + 1)
                .min(here + 1);
            prev = here;
        }
    }
    row[b.len()]
}

/// A byte's line and column (characters), from 0.
pub fn line_col(text: &str, byte: usize) -> (usize, usize) {
    let byte = byte.min(text.len());
    let before = &text[..byte];
    let line = before.matches('\n').count();
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    (line, before[start..].chars().count())
}

/// A byte as a document position.
pub fn pos_of(text: &str, byte: usize) -> Pos {
    let (line, col) = line_col(text, byte);
    Pos::new(line, col)
}

/// A profile's problems as the editor's diagnostics (the json view).
pub fn diagnostics(state: &WorldState, text: &str) -> Vec<Diag> {
    let wire = |b: usize| {
        let (line, character) = line_col(text, b);
        crate::lsp::Wire { line, character }
    };
    state
        .parsed
        .as_ref()
        .map(|x| {
            x.problems
                .iter()
                .map(|pr| Diag {
                    start: wire(pr.start),
                    end: wire(pr.end.max(pr.start + 1)),
                    severity: 1,
                    code: Some(pr.code.clone()),
                    message: pr.message.clone(),
                    raw: Json::Null,
                })
                .collect()
        })
        .unwrap_or_default()
}

// ----------------------------------------------------------------- live --

/// A JSON value as a Lua literal.
pub fn lua(v: &Json) -> String {
    match v {
        Json::Null => "nil".into(),
        Json::Bool(b) => b.to_string(),
        Json::Number(n) => n.to_string(),
        Json::String(s) => format!("{s:?}"),
        Json::Array(a) => format!("{{ {} }}", a.iter().map(lua).collect::<Vec<_>>().join(", ")),
        Json::Object(o) => format!(
            "{{ {} }}",
            o.iter()
                .map(|(k, v)| {
                    let key = if k.parse::<i64>().is_ok() {
                        format!("[{k}]")
                    } else if k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                        && !k.starts_with(|c: char| c.is_ascii_digit())
                    {
                        k.clone()
                    } else {
                        format!("[{k:?}]")
                    };
                    format!("{key} = {}", lua(v))
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// The script that applies the change at `path` to the running world, and
/// what it applies; none for what needs a `/reload` (SavedVariables,
/// bindings) or the next start.
pub fn live_script(root: &Json, path: &[Key]) -> Option<(String, String)> {
    let Some(Key::Name(part)) = path.first() else {
        return None;
    };
    let at = |p: &[Key]| -> Json {
        let mut v = root;
        for k in p {
            v = match k {
                Key::Name(n) => v.get(n).unwrap_or(&Json::Null),
                Key::Index(i) => v.get(*i).unwrap_or(&Json::Null),
            };
        }
        v.clone()
    };
    let field = path.get(1);
    match part.as_str() {
        "character" => {
            let Some(Key::Name(f)) = field else {
                return None;
            };
            if f == "dead" {
                return None;
            }
            let v = at(&path[..2]);
            Some((
                format!("mock.player:update({{ {f} = {} }})", lua(&v)),
                format!("character · {f} {}", short(&v)),
            ))
        }
        "units" => {
            let Some(Key::Index(i)) = field else {
                return None;
            };
            let unit = format!("mock.unitAt({})", i + 1);
            let data = at(&path[..2]);
            let name = data["name"].as_str().unwrap_or("unit").to_string();
            let Some(Key::Name(f)) = path.get(2) else {
                // A unit added: spawned.
                return Some((
                    format!("mock.spawnUnit({})", lua(&data)),
                    format!("{name} · spawned"),
                ));
            };
            let v = at(&path[..3]);
            let script = match f.as_str() {
                "target" => match v.as_bool() {
                    Some(true) => format!("mock.player:target({unit})"),
                    _ => "mock.player:target(nil)".into(),
                },
                "focus" => match v.as_bool() {
                    Some(true) => format!("mock.player:focus({unit})"),
                    _ => "mock.player:focus(nil)".into(),
                },
                "nameplate" => match v.as_bool() {
                    Some(true) => format!("{unit}:showNameplate()"),
                    _ => format!("{unit}:hideNameplate()"),
                },
                "group" => match v.as_str() {
                    Some(kind) => format!("mock.addToGroup({unit}, {})", kind == "raid"),
                    None => format!("mock.removeFromGroup({unit})"),
                },
                "auras" | "acceptsInvites" => return None,
                _ => format!("{unit}:update({{ {f} = {} }})", lua(&v)),
            };
            Some((script, format!("{name} · {f} {}", short(&v))))
        }
        "money" => {
            let v = at(&path[..1]);
            Some((
                format!("mock.player:setMoney({})", lua(&v)),
                format!("money {}", short(&v)),
            ))
        }
        "position" => Some((
            format!("mock.moveTo({})", lua(&at(&path[..1]))),
            "position".into(),
        )),
        "stats" => Some((
            format!("mock.player:setStats({})", lua(&at(&path[..1]))),
            "stats".into(),
        )),
        "cvars" => {
            let Some(Key::Name(name)) = field else {
                return None;
            };
            let v = at(&path[..2]);
            let value = match &v {
                Json::String(s) => s.clone(),
                other => other.to_string(),
            };
            Some((
                format!("C_CVar.SetCVar({name:?}, {value:?})"),
                format!("cvar {name} {value}"),
            ))
        }
        "items" | "spells" | "actions" | "bags" => {
            let key = field?;
            let id = match key {
                Key::Name(k) => k.clone(),
                Key::Index(i) => (i + 1).to_string(),
            };
            let data = at(&path[..2]);
            let call = match part.as_str() {
                "items" => "mock.defineItem",
                "spells" => "mock.defineSpell",
                "actions" => "mock.setAction",
                _ => "mock.setBag",
            };
            Some((
                format!("{call}({id}, {})", lua(&data)),
                format!("{part} {id}"),
            ))
        }
        "equipment" => {
            let Some(Key::Name(slot)) = field else {
                return None;
            };
            let v = at(&path[..2]);
            let slot = match slot.parse::<i64>() {
                Ok(n) => n.to_string(),
                Err(_) => format!("GetInventorySlotInfo({slot:?})"),
            };
            Some((
                format!("mock.player:equip({slot}, {})", lua(&v)),
                format!("equipment {}", short(&v)),
            ))
        }
        "macros" | "mounts" | "toys" | "appearances" | "channels" => {
            let call = match part.as_str() {
                "macros" => "setMacros",
                "mounts" => "setMounts",
                "toys" => "setToys",
                "appearances" => "setAppearances",
                _ => "setChannels",
            };
            Some((
                format!("mock.{call}({})", lua(&at(&path[..1]))),
                part.clone(),
            ))
        }
        _ => None,
    }
}

/// The script for a change of `trained`: the spells it added learned, the
/// ones it took away forgotten (`mock.player:learn`, `unlearn`).
pub fn live_trained(before: &Json, after: &Json, path: &[Key]) -> Option<(String, String)> {
    if path.first() != Some(&Key::Name("trained".into())) {
        return None;
    }
    let ids = |v: &Json| -> Vec<i64> {
        list(&v["trained"])
            .iter()
            .filter_map(|(_, v)| v.as_i64())
            .collect()
    };
    let (was, now) = (ids(before), ids(after));
    let added: Vec<String> = now
        .iter()
        .filter(|i| !was.contains(i))
        .map(i64::to_string)
        .collect();
    let removed: Vec<String> = was
        .iter()
        .filter(|i| !now.contains(i))
        .map(i64::to_string)
        .collect();
    let mut script = Vec::new();
    if !added.is_empty() {
        script.push(format!("mock.player:learn({{ {} }})", added.join(", ")));
    }
    if !removed.is_empty() {
        script.push(format!("mock.player:unlearn({{ {} }})", removed.join(", ")));
    }
    let what = match (added.len(), removed.len()) {
        (0, 0) => return None,
        (_, 0) => format!("trained {}", added.join(", ")),
        (0, _) => format!("trained without {}", removed.join(", ")),
        _ => "trained spells".into(),
    };
    Some((script.join("; "), what))
}

/// A value in a few characters.
fn short(v: &Json) -> String {
    let s = match v {
        Json::String(s) => s.clone(),
        Json::Null => "removed".into(),
        other => other.to_string(),
    };
    if s.chars().count() > 24 {
        s.chars().take(23).collect::<String>() + "…"
    } else {
        s
    }
}

/// The profile a running session started in: its configuration's, else
/// the one the manifest names (`test.profile`), as `uses` says.
pub fn session_profile(debug: &DebugState, state: &WorldState) -> Option<String> {
    if !debug.running() {
        return None;
    }
    if let Some(p) = debug.config.as_ref().and_then(|c| c.profile.clone()) {
        return Some(p);
    }
    let (name, uses) = state.uses.as_ref()?;
    uses.iter()
        .any(|u| u == "wowlua.json · test.profile")
        .then(|| name.clone())
}

/// Sends the edits' scripts to the running session.
pub fn flush_live(state: &mut WorldState, debug: &mut DebugState, system: &dyn System) {
    for (script, what) in state.take_live() {
        if !debug.running() {
            continue;
        }
        debug.send(system, GameRequest::World(script));
        let frame = format!("frame {}", debug.clock.1);
        // A field typed into: one entry.
        let key = what.split(' ').next().unwrap_or("").to_string();
        if state
            .applied
            .first()
            .is_some_and(|(w, _)| w.split(' ').next() == Some(key.as_str()))
        {
            state.applied[0] = (what, frame);
        } else {
            state.applied.insert(0, (what, frame));
            state.applied.truncate(30);
        }
    }
}

/// The live world panel on the world page: the session in the profile,
/// edits applied to it, the running world saved, a snapshot imported,
/// where the profile is used.
pub fn live(
    ui: &mut Ui,
    state: &mut WorldState,
    debug: &mut DebugState,
    workspace: &Workspace,
    system: &dyn System,
) -> Option<Action> {
    let p = palette(ui.ctx());
    let mut action = None;
    let name = state.selected.clone().unwrap_or_default();
    let session = session_profile(debug, state);
    egui::ScrollArea::vertical()
        .id_salt("world-live")
        .auto_shrink(false)
        .show(ui, |ui| {
            forms::padded(ui, 14, 12, |ui| {
                match &session {
                    Some(started) => {
                        forms::callout(ui, CalloutKind::Ok, |ui| {
                            ui.label(
                                egui::RichText::new(format!("session · started in {started}"))
                                    .font(Face::Plex.at(12.0))
                                    .color(p.ink),
                            );
                        });
                        if started == &name {
                            forms::checkbox(ui, &mut state.auto_apply, "apply edits", Some("to the running world"));
                            forms::paragraph(
                                ui,
                                "Each change runs as a world request between frames: the units, auras and character \
                                 change in the session without a restart. SavedVariables and bindings need a /reload.",
                                11.0,
                                16.0,
                                p.ink_faint,
                            );
                            if !state.applied.is_empty() {
                                section(ui, "applied", p);
                                for (what, frame) in state.applied.iter().take(12) {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(what).font(Face::Plex.at(12.0)).color(p.ink));
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.label(egui::RichText::new(frame).font(Face::Plex.at(11.0)).color(p.ink_faint));
                                        });
                                    });
                                }
                            }
                        } else {
                            forms::paragraph(
                                ui,
                                &format!("Edits of {name} apply to a session started in it."),
                                11.0,
                                16.0,
                                p.ink_faint,
                            );
                        }
                        ui.add_space(6.0);
                        // (the panel is narrow: the name, then the button)
                        ui.vertical(|ui| {
                            forms::text_input(ui, &mut state.snapshot_name, ui.available_width().min(220.0), "raid-pull");
                            if widgets::button(ui, ButtonKind::Ghost, "save the running world as a profile", true, None)
                                .clicked()
                                && valid_name(&state.snapshot_name)
                            {
                                let path = workspace
                                    .root
                                    .join("tests/profiles")
                                    .join(format!("{}.json", state.snapshot_name));
                                debug.save_world(system, path);
                                state.selected = Some(state.snapshot_name.clone());
                                state.listed_at = None;
                            }
                        });
                    }
                    None => {
                        forms::paragraph(
                            ui,
                            "No session runs. Start one in this profile to change its world while it runs.",
                            12.0,
                            18.0,
                            p.ink_muted,
                        );
                        if !name.is_empty()
                            && widgets::button(ui, ButtonKind::Secondary, &format!("start a session in {name}"), true, None)
                                .clicked()
                        {
                            action = Some(Action::Start(name.clone()));
                        }
                    }
                }

                ui.add_space(12.0);
                section(ui, "from the game", p);
                forms::paragraph(
                    ui,
                    "Record the world in game with /wl snapshot, in the dev window of a dev build: the character, \
                     items, spells, bags, actions, auras and position. After a /reload Studio finds it in the \
                     installation's SavedVariables and makes a profile of it (the client part says which build it \
                     came from).",
                    11.0,
                    16.0,
                    p.ink_faint,
                );
                if widgets::button(ui, ButtonKind::Secondary, "import a snapshot…", true, None).clicked() {
                    system.world_snapshots(&workspace.root);
                    state.snapshots = Some(Ok((Vec::new(), vec!["looking in the installation…".into()])));
                }
                if let Some(found) = state.snapshots.clone() {
                    match found {
                        Err(e) => {
                            forms::paragraph(ui, &e, 11.0, 16.0, p.error);
                        }
                        Ok((list, notes)) => {
                            for n in &notes {
                                forms::paragraph(ui, n, 11.0, 16.0, p.ink_faint);
                            }
                            for s in list.clone() {
                                ui.horizontal(|ui| {
                                    let when = s.at.map(crate::world::when).unwrap_or_default();
                                    ui.label(
                                        egui::RichText::new(format!("{} · {} {when}", s.client, s.account))
                                            .font(Face::Plex.at(12.0))
                                            .color(p.ink),
                                    );
                                });
                                ui.horizontal(|ui| {
                                    forms::text_input(ui, &mut state.snapshot_name, 140.0, "my-character");
                                    if forms::text_button(ui, "write the profile").clicked() && valid_name(&state.snapshot_name) {
                                        let path = workspace
                                            .root
                                            .join("tests/profiles")
                                            .join(format!("{}.json", state.snapshot_name));
                                        if system.exists(&path) {
                                            action = Some(Action::Note(
                                                "import a snapshot".into(),
                                                format!("tests/profiles/{}.json exists: give another name", state.snapshot_name),
                                            ));
                                        } else {
                                            match system.write_file(&path, &s.text) {
                                                Ok(()) => {
                                                    state.selected = Some(state.snapshot_name.clone());
                                                    state.listed_at = None;
                                                    state.snapshots = None;
                                                }
                                                Err(e) => action = Some(Action::Note("import a snapshot".into(), e)),
                                            }
                                        }
                                    }
                                });
                                for f in &s.failed {
                                    forms::paragraph(ui, &format!("the client refused: {f}"), 11.0, 16.0, p.warning);
                                }
                            }
                            if list.is_empty() && notes.is_empty() {
                                forms::paragraph(
                                    ui,
                                    "No snapshot in the installation: /wl snapshot in the game, then /reload.",
                                    11.0,
                                    16.0,
                                    p.ink_muted,
                                );
                            }
                        }
                    }
                }

                ui.add_space(12.0);
                section(ui, "where it's used", p);
                let uses = state
                    .uses
                    .as_ref()
                    .filter(|(n, _)| n == &name)
                    .map(|(_, u)| u.clone())
                    .unwrap_or_default();
                let configs: Vec<String> = debug
                    .configs
                    .iter()
                    .filter(|c| c.profile.as_deref() == Some(name.as_str()))
                    .map(|c| format!("debug configuration {}", c.name))
                    .collect();
                if uses.is_empty() && configs.is_empty() {
                    forms::paragraph(ui, "Nothing names it yet.", 11.0, 16.0, p.ink_muted);
                }
                for u in uses.iter().chain(&configs) {
                    ui.label(egui::RichText::new(u).font(Face::Plex.at(11.5)).color(p.ink_muted));
                }
            });
        });
    action
}

/// A time as `YYYY-MM-DD HH:MM` (UTC).
pub fn when(secs: f64) -> String {
    let secs = secs as i64;
    let days = secs.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let s = secs.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", s / 3600, s / 60 % 60)
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn section(ui: &mut Ui, title: &str, p: &Palette) {
    ui.label(
        egui::RichText::new(title)
            .font(Face::PlexMedium.at(11.0))
            .color(p.ink_muted),
    );
    ui.add_space(2.0);
}

#[allow(dead_code)]
fn _unused(_: &Path, _: Icon, _: BadgeKind) {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn edits_become_scripts_for_the_running_world() {
        let root = json!({
            "character": { "level": 61, "class": "WARRIOR" },
            "units": [ { "name": "Ogre", "distance": 6, "target": true }, { "name": "Rogue" } ],
            "cvars": { "nameplateShowEnemies": "1" },
            "equipment": { "MainHandSlot": 19019 },
            "money": 500,
            "savedVariables": { "DB": {} }
        });
        let k = |p: &[&str]| -> Vec<Key> {
            p.iter()
                .map(|s| {
                    s.parse::<usize>()
                        .map(Key::Index)
                        .unwrap_or_else(|_| (*s).into())
                })
                .collect()
        };
        assert_eq!(
            live_script(&root, &k(&["character", "level"])).unwrap().0,
            "mock.player:update({ level = 61 })"
        );
        assert_eq!(
            live_script(&root, &k(&["units", "0", "distance"])).unwrap(),
            (
                "mock.unitAt(1):update({ distance = 6 })".into(),
                "Ogre · distance 6".into()
            )
        );
        assert_eq!(
            live_script(&root, &k(&["units", "0", "target"])).unwrap().0,
            "mock.player:target(mock.unitAt(1))"
        );
        assert_eq!(
            live_script(&root, &k(&["units", "1"])).unwrap().0,
            "mock.spawnUnit({ name = \"Rogue\" })"
        );
        assert_eq!(
            live_script(&root, &k(&["cvars", "nameplateShowEnemies"]))
                .unwrap()
                .0,
            "C_CVar.SetCVar(\"nameplateShowEnemies\", \"1\")"
        );
        assert_eq!(
            live_script(&root, &k(&["equipment", "MainHandSlot"]))
                .unwrap()
                .0,
            "mock.player:equip(GetInventorySlotInfo(\"MainHandSlot\"), 19019)"
        );
        assert_eq!(
            live_script(&root, &k(&["money"])).unwrap().0,
            "mock.player:setMoney(500)"
        );
        // A /reload's.
        assert!(live_script(&root, &k(&["savedVariables", "DB"])).is_none());
    }

    #[test]
    fn trained_spells_are_learned_and_forgotten_live() {
        let before = json!({ "trained": [78, 72] });
        let after = json!({ "trained": [78, 871, 6343] });
        let path = vec![Key::Name("trained".into())];
        assert_eq!(
            live_trained(&before, &after, &path).unwrap().0,
            "mock.player:learn({ 871, 6343 }); mock.player:unlearn({ 72 })"
        );
        assert_eq!(
            live_trained(&json!({}), &json!({ "trained": [78] }), &path)
                .unwrap()
                .1,
            "trained 78"
        );
        assert!(live_trained(&before, &before, &path).is_none());
        assert!(live_trained(&before, &after, &[Key::Name("money".into())]).is_none());
    }

    #[test]
    fn a_misspelt_part_is_offered_its_name() {
        let text = "{\n  \"untis\": []\n}";
        let pr = ProfileProblem {
            code: "E4401".into(),
            message: "unknown key `untis`".into(),
            start: 4,
            end: 11,
        };
        assert_eq!(unknown_part(text, &pr), Some("units"));
        assert_eq!(money(123456.0), "12g 34s 56c");
        assert_eq!(when(0.0), "1970-01-01 00:00");
        // (one key each: the order of an object's keys is serde_json's)
        assert_eq!(lua(&json!({ "a b": [1, "x"] })), "{ [\"a b\"] = { 1, \"x\" } }");
        assert_eq!(lua(&json!({ "3": true })), "{ [3] = true }");
        assert_eq!(lua(&json!({ "level": 61 })), "{ level = 61 }");
    }
}
