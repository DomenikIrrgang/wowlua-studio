//! The game page (the game board, S055): the workspace's addon running in
//! the emulated client, its screen drawn on Studio's GPU device every game
//! frame (wowlua decision T484), input as a user's, the frame tree and an
//! inspector, the world's units, the events told, screenshots; the
//! resolution and UI scale are the game's settings (T482).

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};

use egui::{Align2, CornerRadius, Key, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use serde_json::Value as Json;
use wowlua_studio_ui::forms::{self, CalloutKind};
use wowlua_studio_ui::widgets::{self, ButtonKind};
use wowlua_studio_ui::{Face, Palette, palette};

use crate::system::{GameEvent, GameInput, GameRequest, System, Workspace};

/// A row, at the settings' density (S062).
fn row_h() -> f32 {
    wowlua_studio_ui::row(22.0)
}
/// The most events kept in the events panel.
const EVENTS_KEPT: usize = 1000;

/// What the pointer does on the screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Picks the frame drawn there for the inspector.
    #[default]
    Inspect,
    Click,
    Hover,
    Drag,
    Type,
    Key,
}

const MODES: [(Mode, &str); 6] = [
    (Mode::Inspect, "inspect"),
    (Mode::Click, "click"),
    (Mode::Hover, "hover"),
    (Mode::Drag, "drag"),
    (Mode::Type, "type"),
    (Mode::Key, "key"),
];

/// The screen's zoom: fitted to the panel, or a share of its pixels.
const ZOOMS: [(&str, Option<f32>); 4] = [
    ("fit", None),
    ("50%", Some(0.5)),
    ("100%", Some(1.0)),
    ("200%", Some(2.0)),
];

/// The window sizes the toolbar offers (the client's own list on a
/// 1920 by 1080 monitor, and common larger ones).
pub const SIZES: [(u32, u32); 9] = [
    (3440, 1440),
    (2560, 1440),
    (1920, 1200),
    (1920, 1080),
    (1600, 900),
    (1366, 768),
    (1344, 756),
    (1280, 720),
    (1024, 576),
];

/// A request asked, by its id: what its answer is for.
#[derive(Clone, Debug, PartialEq)]
enum Asked {
    Tree,
    Inspect(usize),
    Pick,
    Units,
    Target,
    Events,
    Settings,
    Input(usize),
    Shot(PathBuf),
}

/// The running addon, once it runs.
#[derive(Clone, Debug, PartialEq)]
pub struct Started {
    pub addon: String,
    pub flavor: String,
    pub installation: Option<String>,
}

/// A row of the frame tree.
#[derive(Clone, Debug)]
struct Row {
    id: usize,
    depth: usize,
    name: String,
    class: String,
    shown: bool,
    children: bool,
}

/// An event told, as the events panel shows it (made once, when read).
#[derive(Clone, Debug)]
struct EventRow {
    event: String,
    args: String,
    frames: String,
    time: String,
}

impl EventRow {
    fn of(e: &Json) -> EventRow {
        let texts = |v: &Json| -> Vec<String> {
            v.as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default()
        };
        let frames = texts(&e["frames"]);
        EventRow {
            event: e["event"].as_str().unwrap_or("").to_owned(),
            args: texts(&e["args"]).join(", "),
            frames: match frames.len() {
                0 => "—".to_owned(),
                1 => frames[0].clone(),
                n => format!("{} +{}", frames[0], n - 1),
            },
            time: if frames.is_empty() {
                String::new()
            } else {
                e["ms"]
                    .as_f64()
                    .map_or(String::new(), |ms| format!("{ms:.2} ms"))
            },
        }
    }
}

/// An input given: what, the details, and its answer.
#[derive(Clone, Debug)]
struct Given {
    kind: &'static str,
    text: String,
    result: Option<Result<(), String>>,
}

#[derive(Default)]
pub struct GameState {
    /// The running session's number.
    pub session: Option<u64>,
    next_session: u64,
    /// What it does before the addon runs.
    pub status: Option<String>,
    pub started: Option<Started>,
    /// The last session's end: the error that ended it.
    pub ended: Option<Option<String>>,
    next_request: u64,
    asked: HashMap<u64, Asked>,
    /// When each kind of polling last asked (seconds).
    polled: HashMap<&'static str, f64>,
    // The frames.
    tree: Option<Json>,
    tree_generation: u64,
    pub hidden: bool,
    /// Blizzard's frames left out (those that hold none of the addon's).
    pub mine_only: bool,
    pub query: String,
    open: BTreeSet<usize>,
    rows: Option<(RowsKey, Vec<Row>)>,
    pub selected: Option<usize>,
    inspected: Option<Json>,
    /// The workspace's files the inspector named, as found (once each).
    files: HashMap<String, Option<PathBuf>>,
    // The units.
    units: Vec<Json>,
    // The events and input.
    events: VecDeque<EventRow>,
    given: Vec<Given>,
    pub shots: Vec<PathBuf>,
    // The screen.
    pub mode: Mode,
    zoom: usize,
    slash: String,
    drag_from: Option<Pos2>,
    hovered_at: Option<(i32, i32)>,
    /// The game's settings as the toolbar shows them.
    pub size: (u32, u32),
    pub use_ui_scale: bool,
    pub ui_scale: f32,
    toast: Option<(String, f64)>,
}

/// What the frame tree's rows were made from: the tree's read, the
/// query, the nodes open, Blizzard's frames left out.
type RowsKey = (u64, String, usize, bool);

/// What the game page asks Studio to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Start,
    Stop,
    /// Opens a file at a line (from 1).
    Open(PathBuf, usize),
    ShowInspector,
    /// Function breakpoints on a frame's scripts (`script:Frame:OnX`, S056).
    BreakOn(Vec<String>),
}

impl GameState {
    /// A new session's number: the page's state is the new one's.
    pub fn begin(&mut self) -> u64 {
        self.next_session += 1;
        let keep = (
            self.mode,
            self.zoom,
            self.hidden,
            self.size,
            self.use_ui_scale,
            self.ui_scale,
            self.mine_only,
        );
        let next = self.next_session;
        *self = GameState {
            next_session: next,
            session: Some(next),
            status: Some("starting".into()),
            mode: keep.0,
            zoom: keep.1,
            hidden: keep.2,
            size: keep.3,
            use_ui_scale: keep.4,
            ui_scale: keep.5,
            mine_only: keep.6,
            shots: std::mem::take(&mut self.shots),
            ..GameState::default()
        };
        next
    }

    /// Whether the addon runs (requests are answered).
    pub fn running(&self) -> bool {
        self.session.is_some() && self.started.is_some()
    }

    /// A session's news.
    pub fn event(&mut self, session: u64, event: GameEvent, now: f64) {
        if self.session != Some(session) {
            return;
        }
        match event {
            GameEvent::Status(s) => self.status = Some(s),
            GameEvent::Started {
                addon,
                flavor,
                installation,
            } => {
                self.status = None;
                self.started = Some(Started {
                    addon,
                    flavor,
                    installation,
                });
            }
            GameEvent::Output { .. } => {}
            GameEvent::Ended { error } => {
                self.session = None;
                self.status = None;
                self.started = None;
                self.ended = Some(error);
            }
            GameEvent::Reply { id, result } => {
                let Some(asked) = self.asked.remove(&id) else {
                    return;
                };
                self.answer(asked, result, now);
            }
            // (the debugger's: the screen draws what the stop shows)
            GameEvent::Stopped { .. }
            | GameEvent::Continued
            | GameEvent::Breakpoint(_)
            | GameEvent::Reloaded => {}
        }
    }

    fn answer(&mut self, asked: Asked, result: Result<Json, String>, now: f64) {
        match (asked, result) {
            (Asked::Tree, Ok(tree)) => {
                self.tree = Some(tree);
                self.tree_generation += 1;
            }
            (Asked::Inspect(id), Ok(data)) if self.selected == Some(id) => {
                self.inspected = Some(data);
            }
            (Asked::Pick, Ok(data)) => {
                // (the region drawn there when there is one: the inspector
                // shows its frame and the region among the frame's)
                if let Some(frame) = data["frame"].as_u64() {
                    self.selected = Some(frame as usize);
                    self.inspected = None;
                }
            }
            (Asked::Units, Ok(Json::Array(list))) => self.units = list,
            (Asked::Events, Ok(Json::Array(list))) => {
                for e in &list {
                    if self.events.len() == EVENTS_KEPT {
                        self.events.pop_front();
                    }
                    self.events.push_back(EventRow::of(e));
                }
            }
            (Asked::Input(i), r) => {
                if let Some(g) = self.given.get_mut(i) {
                    g.result = Some(r.map(|_| ()));
                }
                self.polled.remove("tree");
            }
            (Asked::Target | Asked::Settings, Ok(_)) => {
                self.polled.remove("tree");
            }
            (Asked::Shot(path), Ok(_)) => {
                self.toast = Some((format!("saved {}", path.display()), now));
                self.shots.push(path);
            }
            (_, Err(e)) => self.toast = Some((e, now)),
            _ => {}
        }
    }

    fn ask(&mut self, system: &dyn System, asked: Asked, request: GameRequest) {
        if !self.running() {
            return;
        }
        self.next_request += 1;
        let id = self.next_request;
        self.asked.insert(id, asked);
        system.game_request(id, request);
    }

    /// Whether a request of this kind waits for its answer.
    fn waiting(&self, kind: &Asked) -> bool {
        self.asked.values().any(|a| a == kind)
    }

    /// Asks again, every `every` seconds, what a shown panel shows.
    fn poll(&mut self, now: f64, key: &'static str, every: f64) -> bool {
        // (never asked, or asked again: at once)
        let due = self.polled.get(key).is_none_or(|last| now - last >= every);
        if !due || !self.running() {
            return false;
        }
        self.polled.insert(key, now);
        true
    }

    fn give(&mut self, system: &dyn System, kind: &'static str, text: String, input: GameInput) {
        if !self.running() {
            return;
        }
        self.given.push(Given {
            kind,
            text,
            result: None,
        });
        if self.given.len() > 500 {
            self.given.remove(0);
        }
        let i = self.given.len() - 1;
        self.ask(system, Asked::Input(i), GameRequest::Input(input));
    }

    /// Sets the game's settings: the window's size and the UI scale.
    pub fn set_screen(&mut self, system: &dyn System, size: (u32, u32), ui_scale: Option<f32>) {
        self.size = size;
        self.use_ui_scale = ui_scale.is_some();
        if let Some(s) = ui_scale {
            self.ui_scale = s;
        }
        self.apply_settings(system);
    }

    /// Picks what draws at a point of the screen for the inspector.
    pub fn pick(&mut self, system: &dyn System, x: f64, y: f64) {
        self.ask(system, Asked::Pick, GameRequest::Pick { x, y });
    }

    /// Picks a frame for the inspector (from the tree, or the screen).
    pub fn select(&mut self, system: &dyn System, id: usize) {
        self.selected = Some(id);
        self.inspected = None;
        self.ask(system, Asked::Inspect(id), GameRequest::Inspect(id));
    }

    /// The toolbar's settings, sent as the game's.
    fn apply_settings(&mut self, system: &dyn System) {
        let (w, h) = self.size;
        self.ask(
            system,
            Asked::Settings,
            GameRequest::SetScreen {
                width: f64::from(w),
                height: f64::from(h),
                // (the slider's steps, as the game writes the CVar: 0.8)
                ui_scale: self
                    .use_ui_scale
                    .then_some((f64::from(self.ui_scale) * 100.0).round() / 100.0),
            },
        );
    }

    /// The frame tree's rows: open nodes' children, the query's matches.
    fn rows(&mut self) -> &[Row] {
        let key = (
            self.tree_generation,
            self.query.clone(),
            self.open.len(),
            self.mine_only,
        );
        if self.rows.as_ref().is_none_or(|(k, _)| *k != key) {
            let mut out = Vec::new();
            let query = self.query.trim().to_lowercase();
            if let Some(Json::Array(roots)) = &self.tree {
                for n in roots {
                    if self.mine_only && !holds_mine(n) {
                        continue;
                    }
                    flatten(n, 0, &self.open, &query, self.mine_only, &mut out);
                }
            }
            self.rows = Some((key, out));
        }
        self.rows.as_ref().map_or(&[][..], |(_, r)| r.as_slice())
    }
}

/// Whether a frame is the addon's, or holds one of the addon's.
fn holds_mine(n: &Json) -> bool {
    n["blizzard"].as_bool() != Some(true)
        || n["children"]
            .as_array()
            .is_some_and(|c| c.iter().any(holds_mine))
}

fn flatten(
    n: &Json,
    depth: usize,
    open: &BTreeSet<usize>,
    query: &str,
    mine_only: bool,
    out: &mut Vec<Row>,
) {
    let id = n["id"].as_u64().unwrap_or(0) as usize;
    let name = n["name"].as_str().unwrap_or("").to_owned();
    let class = n["type"].as_str().unwrap_or("").to_owned();
    let children = n["children"].as_array();
    let row = Row {
        id,
        depth,
        name: if name.is_empty() {
            format!("{class}#{id}")
        } else {
            name
        },
        class,
        shown: n["shown"].as_bool().unwrap_or(true),
        children: children.is_some_and(|c| !c.is_empty()),
    };
    if query.is_empty() {
        out.push(row);
        if open.contains(&id)
            && let Some(c) = children
        {
            for child in c.iter().filter(|c| !mine_only || holds_mine(c)) {
                flatten(child, depth + 1, open, query, mine_only, out);
            }
        }
        return;
    }
    // With a query: every match, flat.
    let text = n["text"].as_str().unwrap_or("").to_lowercase();
    if row.name.to_lowercase().contains(query)
        || row.class.to_lowercase().contains(query)
        || text.contains(query)
    {
        out.push(Row { depth: 0, ..row });
    }
    if let Some(c) = children {
        for child in c.iter().filter(|c| !mine_only || holds_mine(c)) {
            flatten(child, 0, open, query, mine_only, out);
        }
    }
}

// ------------------------------------------------------------------ screen

/// The game screen panel: the toolbar, the screen, the states before it.
pub fn screen(
    ui: &mut Ui,
    state: &mut GameState,
    workspace: &Workspace,
    system: &dyn System,
) -> Option<Action> {
    let p = palette(ui.ctx());
    let now = ui.input(|i| i.time);
    let mut action = None;
    toolbar(ui, state, system, p);
    rule(ui, p);
    if let Some((text, at)) = &state.toast {
        if now - at > 4.0 {
            state.toast = None;
        } else {
            let text = text.clone();
            forms::padded(ui, 12, 4, |ui| {
                forms::paragraph(ui, &text, 12.0, 18.0, p.ink_muted);
            });
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(500));
        }
    }
    if let Some(s) = &state.started
        && s.installation.is_none()
    {
        forms::padded(ui, 12, 6, |ui| {
            forms::callout(ui, CalloutKind::Warning, |ui| {
                forms::paragraph(
                    ui,
                    "warning: no WoW installation with this flavor's client, so the game's art is \
                     missing: its textures draw as the magenta checker, fonts as the fallback. \
                     Layout, text and tests are unaffected.",
                    12.0,
                    18.0,
                    p.ink,
                );
            });
        });
    }
    let Some(screen) = system.game_screen().filter(|_| state.running()) else {
        // No session, or one starting.
        let name = workspace.name.clone();
        forms::padded(ui, 24, 24, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() / 4.0);
                if let Some(status) = &state.status {
                    ui.label(
                        egui::RichText::new(format!("{status}…"))
                            .font(Face::Plex.at(13.0))
                            .color(p.ink_muted),
                    );
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(250));
                    return;
                }
                ui.label(
                    egui::RichText::new("No session")
                        .font(Face::Plex.at(16.0))
                        .color(p.ink),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(
                        "The game view shows the running session: your addon logged in, its \
                         frames running.",
                    )
                    .font(Face::Plex.at(12.5))
                    .color(p.ink_muted),
                );
                if let Some(Some(error)) = &state.ended {
                    ui.add_space(8.0);
                    forms::callout(ui, CalloutKind::Error, |ui| {
                        forms::paragraph(ui, error, 12.0, 18.0, p.ink);
                    });
                }
                ui.add_space(12.0);
                if widgets::button(ui, ButtonKind::Primary, &format!("run {name}"), true, None)
                    .clicked()
                {
                    action = Some(Action::Start);
                }
            });
        });
        return action;
    };
    let size = Vec2::new(screen.size[0] as f32, screen.size[1] as f32);
    let zoom = ZOOMS[state.zoom].1;
    let avail = ui.available_size();
    let scale = zoom.unwrap_or_else(|| (avail.x / size.x).min(avail.y / size.y).max(0.05));
    let shown = size * scale;
    egui::ScrollArea::both()
        .id_salt("game-screen")
        .auto_shrink(false)
        .show(ui, |ui| {
            let room = ui.available_size().max(shown);
            let (outer, _) = ui.allocate_exact_size(room, Sense::hover());
            ui.painter()
                .rect_filled(outer, CornerRadius::ZERO, p.bg_000);
            let rect = Rect::from_center_size(outer.center(), shown);
            let rect = Rect::from_min_size(rect.min.max(outer.min), shown);
            let response = ui.interact(
                rect,
                ui.make_persistent_id("game-screen"),
                Sense::click_and_drag(),
            );
            ui.painter().image(
                screen.texture,
                rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            pointer(ui, state, system, &response, rect, scale);
            outline(ui, state, rect, scale, p);
        });
    action
}

fn toolbar(ui: &mut Ui, state: &mut GameState, system: &dyn System, p: &Palette) {
    // A narrow panel puts the game's settings on a row of their own.
    let wide = ui.available_width() >= 1180.0;
    forms::padded(ui, 12, 6, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let labels: Vec<&str> = MODES.iter().map(|(_, l)| *l).collect();
            let at = MODES
                .iter()
                .position(|(m, _)| *m == state.mode)
                .unwrap_or(0);
            if let Some(i) = widgets::seg(ui, "game-mode", &labels, at, false) {
                state.mode = MODES[i].0;
            }
            ui.label(
                egui::RichText::new("/")
                    .font(Face::Plex.at(12.0))
                    .color(p.ink_faint),
            );
            // (the zoom and screenshot at the right take about 280, the
            // settings about 280 more)
            let right = if wide { 580.0 } else { 290.0 };
            let width = (ui.available_width() - right).clamp(90.0, 240.0);
            let r = forms::text_input(ui, &mut state.slash, width, "a slash command");
            if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                let text = state.slash.trim().to_owned();
                if !text.is_empty() {
                    let cmd = if text.starts_with('/') {
                        text
                    } else {
                        format!("/{text}")
                    };
                    state.give(system, "slash", cmd.clone(), GameInput::Slash(cmd));
                    state.slash.clear();
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(state.running(), |ui| {
                    if widgets::button(ui, ButtonKind::Tertiary, "screenshot", true, None).clicked()
                    {
                        shoot(state, system, None);
                    }
                });
                let labels: Vec<&str> = ZOOMS.iter().map(|(l, _)| *l).collect();
                if let Some(i) = widgets::seg(ui, "game-zoom", &labels, state.zoom, false) {
                    state.zoom = i;
                }
                if wide {
                    settings(ui, state, system);
                }
            });
        });
        if !wide {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(
                    egui::RichText::new("the game's settings")
                        .font(Face::Plex.at(11.5))
                        .color(p.ink_faint),
                );
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    settings_row(ui, state, system);
                });
            });
        }
    });
}

/// The settings left to right (a row of their own).
fn settings_row(ui: &mut Ui, state: &mut GameState, system: &dyn System) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.set_max_width(420.0);
        settings(ui, state, system);
    });
}

/// The game's settings: the window's size and the UI scale (T482).
fn settings(ui: &mut Ui, state: &mut GameState, system: &dyn System) {
    if state.size == (0, 0) {
        state.size = (1920, 1080);
        state.ui_scale = 1.0;
    }
    let mut changed = false;
    ui.add_enabled_ui(state.running(), |ui| {
        if state.use_ui_scale {
            let r = ui.add(
                egui::Slider::new(&mut state.ui_scale, 0.65..=1.15)
                    .step_by(0.01)
                    .show_value(true)
                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
            );
            changed |= r.drag_stopped() || (r.changed() && !r.dragged());
        }
        changed |= forms::checkbox(ui, &mut state.use_ui_scale, "UI scale", None);
        let labels: Vec<String> = SIZES.iter().map(|(w, h)| format!("{w}×{h}")).collect();
        let options: Vec<(&str, bool)> = labels.iter().map(|l| (l.as_str(), true)).collect();
        let mut chosen = SIZES.iter().position(|s| *s == state.size).unwrap_or(3);
        if forms::select(ui, "game-size", &options, &mut chosen, 120.0) {
            state.size = SIZES[chosen];
            changed = true;
        }
    });
    if changed {
        state.apply_settings(system);
    }
}

/// The pointer on the screen, as the mode gives it to the client.
fn pointer(
    ui: &mut Ui,
    state: &mut GameState,
    system: &dyn System,
    response: &egui::Response,
    rect: Rect,
    scale: f32,
) {
    let at = |pos: Pos2| {
        let v = (pos - rect.min) / scale;
        (f64::from(v.x.floor()), f64::from(v.y.floor()))
    };
    match state.mode {
        Mode::Inspect => {
            if response.clicked()
                && let Some(pos) = response.interact_pointer_pos()
            {
                let (x, y) = at(pos);
                state.ask(system, Asked::Pick, GameRequest::Pick { x, y });
            }
        }
        Mode::Click => {
            for (clicked, right) in [
                (response.clicked(), false),
                (response.secondary_clicked(), true),
            ] {
                if clicked && let Some(pos) = response.interact_pointer_pos() {
                    let (x, y) = at(pos);
                    let button = if right { "RightButton" } else { "LeftButton" };
                    state.give(
                        system,
                        "click",
                        format!("{button} at {x}, {y}"),
                        GameInput::Click { x, y, right },
                    );
                }
            }
        }
        Mode::Hover => {
            if let Some(pos) = response.hover_pos() {
                let (x, y) = at(pos);
                let cell = ((x / 4.0) as i32, (y / 4.0) as i32);
                let busy = state.asked.values().any(|a| matches!(a, Asked::Input(_)));
                if state.hovered_at != Some(cell) && !busy {
                    state.hovered_at = Some(cell);
                    state.give(
                        system,
                        "hover",
                        format!("{x}, {y}"),
                        GameInput::Hover { x, y },
                    );
                }
            }
        }
        Mode::Drag => {
            if response.drag_started() {
                state.drag_from = response.interact_pointer_pos();
            }
            if response.drag_stopped()
                && let (Some(from), Some(to)) =
                    (state.drag_from.take(), response.interact_pointer_pos())
            {
                let (x, y) = at(from);
                let (tx, ty) = at(to);
                let (dx, dy) = (tx - x, ty - y);
                state.give(
                    system,
                    "drag",
                    format!("from {x}, {y} by {dx}, {dy}"),
                    GameInput::Drag { x, y, dx, dy },
                );
            }
        }
        Mode::Type | Mode::Key => {
            if response.clicked() {
                response.request_focus();
            }
            if response.has_focus() {
                let events = ui.input(|i| i.events.clone());
                for e in events {
                    match (state.mode, e) {
                        (Mode::Type, egui::Event::Text(t)) => {
                            state.give(system, "type", t.clone(), GameInput::Type(t));
                        }
                        (
                            Mode::Key,
                            egui::Event::Key {
                                key,
                                pressed: true,
                                modifiers,
                                ..
                            },
                        ) => {
                            if let Some(name) = wow_key(key, modifiers) {
                                state.give(system, "key", name.clone(), GameInput::Key(name));
                            }
                        }
                        _ => {}
                    }
                }
                // (keys stay the client's while the screen has the focus)
                ui.memory_mut(|m| {
                    m.set_focus_lock_filter(
                        response.id,
                        egui::EventFilter {
                            tab: true,
                            horizontal_arrows: true,
                            vertical_arrows: true,
                            escape: true,
                        },
                    );
                });
            }
        }
    }
}

/// A key as WoW names it in bindings (`SHIFT-1`, `ESCAPE`).
pub fn wow_key(key: Key, modifiers: egui::Modifiers) -> Option<String> {
    let name = match key {
        Key::Escape => "ESCAPE".to_owned(),
        Key::Enter => "ENTER".to_owned(),
        Key::Tab => "TAB".to_owned(),
        Key::Space => "SPACE".to_owned(),
        Key::Backspace => "BACKSPACE".to_owned(),
        Key::Delete => "DELETE".to_owned(),
        Key::ArrowUp => "UP".to_owned(),
        Key::ArrowDown => "DOWN".to_owned(),
        Key::ArrowLeft => "LEFT".to_owned(),
        Key::ArrowRight => "RIGHT".to_owned(),
        Key::Home => "HOME".to_owned(),
        Key::End => "END".to_owned(),
        Key::PageUp => "PAGEUP".to_owned(),
        Key::PageDown => "PAGEDOWN".to_owned(),
        k => {
            let n = k.name();
            let plain = n.len() == 1 && n.chars().all(|c| c.is_ascii_alphanumeric());
            let function = n.starts_with('F') && n[1..].parse::<u8>().is_ok();
            if !(plain || function) {
                return None;
            }
            n.to_ascii_uppercase()
        }
    };
    let mut out = String::new();
    if modifiers.alt {
        out.push_str("ALT-");
    }
    if modifiers.ctrl || modifiers.mac_cmd {
        out.push_str("CTRL-");
    }
    if modifiers.shift {
        out.push_str("SHIFT-");
    }
    out.push_str(&name);
    Some(out)
}

/// The inspected frame's rectangle over the screen.
fn outline(ui: &Ui, state: &GameState, rect: Rect, scale: f32, p: &Palette) {
    let Some(r) = state.inspected.as_ref().map(|i| &i["rect"]) else {
        return;
    };
    let (Some(x), Some(y), Some(w), Some(h)) = (
        r["x"].as_f64(),
        r["y"].as_f64(),
        r["width"].as_f64(),
        r["height"].as_f64(),
    ) else {
        return;
    };
    let min = rect.min + Vec2::new(x as f32, y as f32) * scale;
    let frame = Rect::from_min_size(min, Vec2::new(w as f32, h as f32) * scale);
    ui.painter().rect_stroke(
        frame,
        CornerRadius::ZERO,
        Stroke::new(1.5, p.amber),
        StrokeKind::Outside,
    );
}

fn shoot(state: &mut GameState, system: &dyn System, frame: Option<usize>) {
    let n = state.shots.len()
        + 1
        + state
            .asked
            .values()
            .filter(|a| matches!(a, Asked::Shot(_)))
            .count();
    let name = match frame {
        Some(f) => format!("frame-{f}-{n}.png"),
        None => format!("screen-{n}.png"),
    };
    let to = PathBuf::from("dist").join("screenshots").join(name);
    state.ask(
        system,
        Asked::Shot(to.clone()),
        GameRequest::Screenshot { frame, to },
    );
}

// ------------------------------------------------------------------ frames

/// The frame tree (the frames panel).
pub fn frames(ui: &mut Ui, state: &mut GameState, system: &dyn System) {
    let p = palette(ui.ctx());
    let now = ui.input(|i| i.time);
    if !state.running() {
        empty(ui, "No session: the frame tree is the running client's.", p);
        return;
    }
    // Read when shown, then again after input, a target, the game's
    // settings, `hidden` or the refresh button: a large UI's whole tree
    // read on a timer cost its frames (SuuUI's: 200 ms each).
    if state.poll(now, "tree", f64::INFINITY) && !state.waiting(&Asked::Tree) {
        let hidden = state.hidden;
        state.ask(system, Asked::Tree, GameRequest::FrameTree { hidden });
    }
    forms::padded(ui, 10, 8, |ui| {
        let width = ui.available_width();
        forms::text_input(ui, &mut state.query, width, "filter frames");
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if forms::checkbox(ui, &mut state.hidden, "hidden", None) {
                state.polled.remove("tree");
            }
            let mut blizzards = !state.mine_only;
            if forms::checkbox(ui, &mut blizzards, "Blizzard's", None) {
                state.mine_only = !blizzards;
            }
            if forms::text_button(ui, "refresh").clicked() {
                state.polled.remove("tree");
            }
        });
    });
    rule(ui, p);
    let selected = state.selected;
    let mut toggled = None;
    let mut picked = None;
    state.rows();
    // (taken while drawn, put back after: not copied each frame)
    let (key, rows) = state.rows.take().unwrap_or_default();
    if rows.is_empty() {
        state.rows = Some((key, rows));
        let text = if state.query.is_empty() {
            "Reading the frame tree…".to_owned()
        } else {
            format!(
                "No frame's name, type or text contains \"{}\".",
                state.query.trim()
            )
        };
        empty(ui, &text, p);
        return;
    }
    let open = state.open.clone();
    egui::ScrollArea::vertical()
        .id_salt("game-frames")
        .auto_shrink(false)
        .show_rows(ui, row_h(), rows.len(), |ui, range| {
            for row in &rows[range] {
                let (rect, response) = ui
                    .allocate_exact_size(Vec2::new(ui.available_width(), row_h()), Sense::click());
                if selected == Some(row.id) {
                    ui.painter()
                        .rect_filled(rect, CornerRadius::ZERO, p.amber_soft);
                } else if response.hovered() {
                    ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_200);
                }
                let x = rect.left() + 10.0 + row.depth as f32 * 14.0;
                let y = rect.center().y;
                if row.children {
                    let twist = if open.contains(&row.id) { "−" } else { "+" };
                    ui.painter().text(
                        Pos2::new(x, y),
                        Align2::LEFT_CENTER,
                        twist,
                        Face::Plex.at(12.0),
                        p.ink_muted,
                    );
                    let twist_rect =
                        Rect::from_center_size(Pos2::new(x + 4.0, y), Vec2::splat(16.0));
                    if response.clicked()
                        && response
                            .interact_pointer_pos()
                            .is_some_and(|q| twist_rect.contains(q))
                    {
                        toggled = Some(row.id);
                        continue;
                    }
                }
                let color = if row.shown { p.ink } else { p.ink_faint };
                let r = ui.painter().text(
                    Pos2::new(x + 16.0, y),
                    Align2::LEFT_CENTER,
                    &row.name,
                    Face::Plex.at(12.5),
                    color,
                );
                ui.painter().text(
                    Pos2::new(r.right() + 8.0, y),
                    Align2::LEFT_CENTER,
                    &row.class,
                    Face::Plex.at(11.0),
                    p.ink_faint,
                );
                if response.clicked() {
                    picked = Some(row.id);
                }
            }
        });
    state.rows = Some((key, rows));
    if let Some(id) = toggled
        && !state.open.remove(&id)
    {
        state.open.insert(id);
    }
    if let Some(id) = picked {
        state.select(system, id);
    }
}

// ------------------------------------------------------------------- units

/// The world's units (the units panel): a click targets one.
pub fn units(ui: &mut Ui, state: &mut GameState, system: &dyn System) {
    let p = palette(ui.ctx());
    let now = ui.input(|i| i.time);
    if !state.running() {
        empty(ui, "No session: the units are the running world's.", p);
        return;
    }
    if state.poll(now, "units", 1.0) && !state.waiting(&Asked::Units) {
        state.ask(system, Asked::Units, GameRequest::Units);
    }
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_secs(1));
    let units = state.units.clone();
    let mut target = None;
    egui::ScrollArea::vertical()
        .id_salt("game-units")
        .auto_shrink(false)
        .show_rows(ui, 38.0, units.len(), |ui, range| {
            for (i, u) in units[range.clone()].iter().enumerate() {
                let index = range.start + i;
                let (rect, response) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 38.0), Sense::click());
                if u["target"].as_bool() == Some(true) {
                    ui.painter()
                        .rect_filled(rect, CornerRadius::ZERO, p.amber_soft);
                } else if response.hovered() {
                    ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_200);
                }
                let color = match u["reaction"].as_str() {
                    Some("hostile") => p.error,
                    Some("neutral") => p.warning,
                    _ => p.success,
                };
                let x = rect.left() + 12.0;
                ui.painter()
                    .circle_filled(Pos2::new(x + 4.0, rect.top() + 12.0), 4.0, color);
                ui.painter().text(
                    Pos2::new(x + 16.0, rect.top() + 12.0),
                    Align2::LEFT_CENTER,
                    u["name"].as_str().unwrap_or("?"),
                    Face::Plex.at(12.5),
                    p.ink,
                );
                let tokens: Vec<&str> = u["tokens"]
                    .as_array()
                    .map(|t| t.iter().filter_map(|v| v.as_str()).take(3).collect())
                    .unwrap_or_default();
                let sub = format!(
                    "{} · {} · {}",
                    u["class"].as_str().unwrap_or(""),
                    u["level"].as_f64().unwrap_or(0.0),
                    tokens.join(", ")
                );
                ui.painter().text(
                    Pos2::new(x + 16.0, rect.top() + 27.0),
                    Align2::LEFT_CENTER,
                    sub,
                    Face::Plex.at(11.0),
                    p.ink_muted,
                );
                if let Some(d) = u["distance"].as_f64() {
                    ui.painter().text(
                        Pos2::new(rect.right() - 12.0, rect.top() + 12.0),
                        Align2::RIGHT_CENTER,
                        format!("{d:.0} yd"),
                        Face::Plex.at(11.0),
                        p.ink_muted,
                    );
                }
                if response.clicked() {
                    target = Some(u["id"].as_u64().map_or(index, |i| i as usize));
                }
            }
        });
    if let Some(t) = target {
        state.ask(system, Asked::Target, GameRequest::Target(Some(t)));
        state.polled.remove("units");
    }
}

// --------------------------------------------------------------- inspector

/// The picked frame's details (the inspector panel).
pub fn inspector(
    ui: &mut Ui,
    state: &mut GameState,
    workspace: &Workspace,
    system: &dyn System,
) -> Option<Action> {
    let p = palette(ui.ctx());
    let mut action = None;
    let Some(id) = state.selected else {
        empty(
            ui,
            "Pick a frame on the screen (inspect), or in the frame tree.",
            p,
        );
        return None;
    };
    if state.inspected.is_none() && !state.waiting(&Asked::Inspect(id)) {
        state.ask(system, Asked::Inspect(id), GameRequest::Inspect(id));
    }
    // (taken while drawn, put back after: not copied each frame)
    let Some(info) = state.inspected.take() else {
        empty(ui, "Reading the frame…", p);
        return None;
    };
    let num = |v: &Json| v.as_f64().map_or("—".to_owned(), trim);
    egui::ScrollArea::vertical()
        .id_salt("game-inspector")
        .auto_shrink(false)
        .show(ui, |ui| {
            forms::padded(ui, 12, 10, |ui| {
                ui.label(
                    egui::RichText::new(info["label"].as_str().unwrap_or("?"))
                        .font(Face::Plex.at(15.0))
                        .color(p.ink),
                );
                ui.label(
                    egui::RichText::new(info["type"].as_str().unwrap_or(""))
                        .font(Face::Plex.at(12.0))
                        .color(p.ink_muted),
                );
                ui.add_space(8.0);
                let rect = &info["rect"];
                let mut rows: Vec<(&str, String)> = vec![
                    (
                        "parent",
                        info["parent"]["label"].as_str().unwrap_or("—").to_owned(),
                    ),
                    (
                        "shown · visible",
                        format!(
                            "{} · {}",
                            info["shown"].as_bool().unwrap_or(false),
                            info["visible"].as_bool().unwrap_or(false)
                        ),
                    ),
                ];
                if info["strata"].is_string() {
                    rows.push((
                        "strata · level",
                        format!(
                            "{} · {}",
                            info["strata"].as_str().unwrap_or(""),
                            num(&info["level"])
                        ),
                    ));
                } else {
                    rows.push((
                        "layer · sublevel",
                        format!(
                            "{} · {}",
                            info["layer"].as_str().unwrap_or(""),
                            num(&info["sublevel"])
                        ),
                    ));
                }
                rows.push((
                    "rect",
                    if rect.is_object() {
                        format!(
                            "{}, {} · {} × {}",
                            num(&rect["x"]),
                            num(&rect["y"]),
                            num(&rect["width"]),
                            num(&rect["height"])
                        )
                    } else {
                        "not laid out".to_owned()
                    },
                ));
                rows.push((
                    "alpha · scale",
                    format!(
                        "{} ({} drawn) · {} ({} in all)",
                        num(&info["alpha"]),
                        num(&info["effectiveAlpha"]),
                        num(&info["scale"]),
                        num(&info["effectiveScale"])
                    ),
                ));
                rows.push((
                    "mouse",
                    if info["mouse"].as_bool() == Some(true) {
                        "takes it"
                    } else {
                        "passes it"
                    }
                    .to_owned(),
                ));
                let created = place_text(&info["created"]);
                rows.push((
                    "created at",
                    created.clone().unwrap_or_else(|| "the client's own".into()),
                ));
                forms::key_values(ui, &rows);
                section(ui, "points", p);
                for pt in info["points"].as_array().into_iter().flatten() {
                    let text = format!(
                        "{} → {} {} ({}, {})",
                        pt["point"].as_str().unwrap_or(""),
                        pt["relativeTo"].as_str().unwrap_or("its parent"),
                        pt["relativePoint"].as_str().unwrap_or(""),
                        num(&pt["x"]),
                        num(&pt["y"]),
                    );
                    line(ui, &text, p);
                }
                section(ui, "scripts", p);
                for sc in info["scripts"].as_array().into_iter().flatten() {
                    let at = place_text(&sc["at"]).unwrap_or_default();
                    let hook = if sc["hook"].as_bool() == Some(true) {
                        " (hook)"
                    } else {
                        ""
                    };
                    let r = line(
                        ui,
                        &format!("{}{hook}  {at}", sc["name"].as_str().unwrap_or("")),
                        p,
                    );
                    if let Some(place) = place_of(&sc["at"], workspace, system, &mut state.files)
                        && r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
                    {
                        action = Some(Action::Open(place.0, place.1));
                    }
                }
                let events: Vec<&str> = info["events"]
                    .as_array()
                    .map(|e| e.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();
                if !events.is_empty() {
                    section(ui, "events", p);
                    for e in events {
                        line(ui, e, p);
                    }
                }
                section(ui, "regions", p);
                for r in info["regions"].as_array().into_iter().flatten() {
                    let shows = r["atlas"]
                        .as_str()
                        .map(|a| format!("atlas {a}"))
                        .or_else(|| r["texture"].as_str().map(str::to_owned))
                        .or_else(|| r["texture"].as_f64().map(|n| format!("file {n}")))
                        .or_else(|| r["text"].as_str().map(|t| format!("\"{t}\"")))
                        .unwrap_or_default();
                    line(
                        ui,
                        &format!(
                            "{} {} {}  {shows}",
                            r["type"].as_str().unwrap_or(""),
                            r["layer"].as_str().unwrap_or(""),
                            num(&r["sublevel"])
                        ),
                        p,
                    );
                }
                let children = info["children"].as_array().cloned().unwrap_or_default();
                section(ui, &format!("children · {}", children.len()), p);
                for c in children.iter().take(50) {
                    if line(ui, c["label"].as_str().unwrap_or("?"), p)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                        && let Some(cid) = c["id"].as_u64()
                    {
                        state.select(system, cid as usize);
                    }
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let code = place_of(&info["created"], workspace, system, &mut state.files);
                    if ui
                        .add_enabled_ui(code.is_some(), |ui| {
                            widgets::button(ui, ButtonKind::Secondary, "go to code", true, None)
                        })
                        .inner
                        .clicked()
                        && let Some((path, line)) = code
                    {
                        action = Some(Action::Open(path, line));
                    }
                    // (by its global name: a frame without one can't be named)
                    let named = info["name"].as_str().map(str::to_owned);
                    let scripts: Vec<String> = info["scripts"]
                        .as_array()
                        .map(|l| {
                            l.iter()
                                .filter_map(|s| s["name"].as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default();
                    let can = named.is_some() && !scripts.is_empty();
                    let r = ui.add_enabled_ui(can, |ui| {
                        widgets::button(
                            ui,
                            ButtonKind::Tertiary,
                            "break on its scripts",
                            true,
                            None,
                        )
                    });
                    if r.inner.clicked()
                        && let Some(n) = &named
                    {
                        let mut names: Vec<String> =
                            scripts.iter().map(|s| format!("script:{n}:{s}")).collect();
                        names.dedup();
                        action = Some(Action::BreakOn(names));
                    }
                    if !can {
                        r.response.on_disabled_hover_text(if named.is_none() {
                            "a frame without a global name can't be named in a breakpoint"
                        } else {
                            "it has no scripts"
                        });
                    }
                    if widgets::button(ui, ButtonKind::Tertiary, "screenshot it", true, None)
                        .clicked()
                    {
                        shoot(state, system, Some(id));
                    }
                });
            });
        });
    // (another frame picked meanwhile reads its own)
    if state.selected == Some(id) && state.inspected.is_none() {
        state.inspected = Some(info);
    }
    action
}

/// `file:line` of a place (`{ file, line }`).
fn place_text(v: &Json) -> Option<String> {
    let file = v["file"].as_str()?;
    Some(format!("{file}:{}", v["line"].as_u64().unwrap_or(0)))
}

/// A place in the workspace's files, when it is one (a package's source,
/// by its path from the package's root or absolute).
fn place_of(
    v: &Json,
    workspace: &Workspace,
    system: &dyn System,
    found: &mut HashMap<String, Option<PathBuf>>,
) -> Option<(PathBuf, usize)> {
    let file = v["file"].as_str()?;
    let line = (v["line"].as_u64().unwrap_or(1) as usize).max(1);
    let full = found
        .entry(file.to_owned())
        .or_insert_with(|| {
            let path = Path::new(file);
            if path.is_absolute() {
                return system.exists(path).then(|| path.to_path_buf());
            }
            std::iter::once(&workspace.root)
                .chain(workspace.members.iter().map(|m| &m.dir))
                .map(|root| root.join(path))
                .find(|full| system.exists(full))
        })
        .clone()?;
    Some((full, line))
}

fn section(ui: &mut Ui, title: &str, p: &Palette) {
    ui.add_space(10.0);
    ui.label(
        egui::RichText::new(title)
            .font(Face::Plex.at(11.0))
            .color(p.ink_faint),
    );
    ui.add_space(2.0);
}

fn line(ui: &mut Ui, text: &str, p: &Palette) -> egui::Response {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .font(Face::Plex.at(12.0))
                .color(p.ink),
        )
        .truncate()
        .sense(Sense::click()),
    )
}

fn trim(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        let s = format!("{n:.3}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

// ------------------------------------------------- events, input, screenshots

/// The events told (the events panel), newest last.
pub fn events(ui: &mut Ui, state: &mut GameState, system: &dyn System) {
    let p = palette(ui.ctx());
    let now = ui.input(|i| i.time);
    if !state.running() {
        empty(ui, "No session: the events are the running client's.", p);
        return;
    }
    if state.poll(now, "events", 0.25) && !state.waiting(&Asked::Events) {
        state.ask(system, Asked::Events, GameRequest::Events);
    }
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(250));
    let header = ["event", "arguments", "frames", "in"];
    let widths = [0.30, 0.38, 0.22, 0.10];
    let width = ui.available_width();
    let (hrect, _) = ui.allocate_exact_size(Vec2::new(width, row_h()), Sense::hover());
    let mut x = hrect.left() + 10.0;
    for (h, w) in header.iter().zip(widths) {
        ui.painter().text(
            Pos2::new(x, hrect.center().y),
            Align2::LEFT_CENTER,
            *h,
            Face::Plex.at(11.0),
            p.ink_faint,
        );
        x += width * w;
    }
    let events = &state.events;
    egui::ScrollArea::vertical()
        .id_salt("game-events")
        .auto_shrink(false)
        .stick_to_bottom(true)
        .show_rows(ui, row_h(), events.len(), |ui, range| {
            for e in events.range(range) {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(width, row_h()), Sense::hover());
                let cells = [&e.event, &e.args, &e.frames, &e.time];
                let mut x = rect.left() + 10.0;
                for (i, (c, w)) in cells.iter().zip(widths).enumerate() {
                    let clip = Rect::from_min_size(
                        Pos2::new(x, rect.top()),
                        Vec2::new(width * w - 8.0, row_h()),
                    );
                    ui.painter().with_clip_rect(clip).text(
                        Pos2::new(x, rect.center().y),
                        Align2::LEFT_CENTER,
                        c,
                        Face::Plex.at(12.0),
                        if i == 0 { p.ink } else { p.ink_muted },
                    );
                    x += width * w;
                }
            }
        });
}

/// The input given (the input panel), newest last.
pub fn input(ui: &mut Ui, state: &mut GameState) {
    let p = palette(ui.ctx());
    if state.given.is_empty() {
        empty(
            ui,
            "Input goes to the client as a user's: pick click, hover, drag, type or key on the \
             screen's toolbar, or run a slash command.",
            p,
        );
        return;
    }
    let given = &state.given;
    egui::ScrollArea::vertical()
        .id_salt("game-input")
        .auto_shrink(false)
        .stick_to_bottom(true)
        .show_rows(ui, row_h(), given.len(), |ui, range| {
            for g in &given[range] {
                let (rect, _) = ui
                    .allocate_exact_size(Vec2::new(ui.available_width(), row_h()), Sense::hover());
                let y = rect.center().y;
                ui.painter().text(
                    Pos2::new(rect.left() + 10.0, y),
                    Align2::LEFT_CENTER,
                    g.kind,
                    Face::Plex.at(11.5),
                    p.ink_faint,
                );
                ui.painter().text(
                    Pos2::new(rect.left() + 70.0, y),
                    Align2::LEFT_CENTER,
                    &g.text,
                    Face::Plex.at(12.0),
                    p.ink,
                );
                let (meta, color) = match &g.result {
                    None => ("…".to_owned(), p.ink_faint),
                    Some(Ok(())) => ("ok".to_owned(), p.success),
                    Some(Err(e)) => (e.clone(), p.error),
                };
                ui.painter().text(
                    Pos2::new(rect.right() - 10.0, y),
                    Align2::RIGHT_CENTER,
                    meta,
                    Face::Plex.at(11.5),
                    color,
                );
            }
        });
}

/// The screenshots taken (the screenshots panel).
pub fn screenshots(ui: &mut Ui, state: &mut GameState, system: &dyn System) {
    let p = palette(ui.ctx());
    forms::padded(ui, 12, 8, |ui| {
        ui.horizontal(|ui| {
            ui.add_enabled_ui(state.running(), |ui| {
                if widgets::button(
                    ui,
                    ButtonKind::Secondary,
                    "screenshot the screen",
                    true,
                    None,
                )
                .clicked()
                {
                    shoot(state, system, None);
                }
            });
        });
        ui.add_space(6.0);
        forms::paragraph(
            ui,
            "Screenshots save as PNG into dist/screenshots. Tests compare the UI only \
             (toMatchScreenshot).",
            12.0,
            18.0,
            p.ink_muted,
        );
        ui.add_space(6.0);
        for s in state.shots.iter().rev() {
            line(ui, &s.display().to_string(), p);
        }
    });
}

fn empty(ui: &mut Ui, text: &str, p: &Palette) {
    forms::padded(ui, 12, 12, |ui| {
        forms::paragraph(ui, text, 12.5, 19.0, p.ink_muted);
    });
}

fn rule(ui: &mut Ui, p: &Palette) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::ZERO, p.line);
}

#[cfg(test)]
mod unit {
    use super::*;
    use crate::system::NoSystem;
    use serde_json::json;

    /// Without Blizzard's frames: a Blizzard frame that holds the addon's
    /// stays (the way to them), one that holds none goes.
    #[test]
    fn the_tree_leaves_blizzards_frames_out() {
        let tree = json!([
            { "id": 1, "name": "UIParent", "type": "Frame", "blizzard": true, "children": [
                { "id": 2, "name": "MyMeter", "type": "Frame" },
                { "id": 3, "name": "MinimapCluster", "type": "Frame", "blizzard": true }
            ] },
            { "id": 4, "name": "WorldFrame", "type": "Frame", "blizzard": true }
        ]);
        let open: BTreeSet<usize> = [1].into_iter().collect();
        let names = |mine_only: bool| -> Vec<String> {
            let mut out = Vec::new();
            for n in tree.as_array().unwrap() {
                if mine_only && !holds_mine(n) {
                    continue;
                }
                flatten(n, 0, &open, "", mine_only, &mut out);
            }
            out.into_iter().map(|r| r.name).collect()
        };
        assert_eq!(
            names(false),
            ["UIParent", "MyMeter", "MinimapCluster", "WorldFrame"]
        );
        assert_eq!(names(true), ["UIParent", "MyMeter"]);
    }

    #[test]
    fn keys_are_named_as_bindings_name_them() {
        let none = egui::Modifiers::NONE;
        assert_eq!(wow_key(Key::Escape, none).as_deref(), Some("ESCAPE"));
        assert_eq!(
            wow_key(Key::A, egui::Modifiers::SHIFT).as_deref(),
            Some("SHIFT-A")
        );
        let both = egui::Modifiers {
            alt: true,
            ctrl: true,
            ..none
        };
        assert_eq!(wow_key(Key::Num1, both).as_deref(), Some("ALT-CTRL-1"));
        assert_eq!(wow_key(Key::F5, none).as_deref(), Some("F5"));
        assert_eq!(wow_key(Key::Comma, none), None);
    }

    fn tree() -> Json {
        json!([
            { "id": 1, "type": "Frame", "name": "UIParent", "shown": true, "children": [
                { "id": 7, "type": "Frame", "name": "DemoBox", "shown": true, "text": null },
                { "id": 9, "type": "Button", "shown": false, "children": [
                    { "id": 10, "type": "FontString", "text": "Hello", "shown": true }
                ] }
            ] },
            { "id": 2, "type": "Frame", "name": "WorldFrame", "shown": true }
        ])
    }

    /// A session as the page sees it: started, the tree asked and read.
    fn running() -> GameState {
        let mut state = GameState::default();
        let s = state.begin();
        state.event(
            s,
            GameEvent::Started {
                addon: "Demo".into(),
                flavor: "camelot".into(),
                installation: None,
            },
            0.0,
        );
        state
    }

    #[test]
    fn the_tree_shows_open_nodes_and_every_match() {
        let mut state = running();
        state.ask(
            &NoSystem,
            Asked::Tree,
            GameRequest::FrameTree { hidden: false },
        );
        state.event(
            1,
            GameEvent::Reply {
                id: 1,
                result: Ok(tree()),
            },
            0.0,
        );
        let names = |s: &mut GameState| -> Vec<(usize, String)> {
            s.rows().iter().map(|r| (r.depth, r.name.clone())).collect()
        };
        assert_eq!(
            names(&mut state),
            [(0, "UIParent".to_owned()), (0, "WorldFrame".to_owned())]
        );
        state.open.insert(1);
        assert_eq!(
            names(&mut state),
            [
                (0, "UIParent".to_owned()),
                (1, "DemoBox".to_owned()),
                (1, "Button#9".to_owned()),
                (0, "WorldFrame".to_owned())
            ]
        );
        // A query: the matches, flat, by name, type or text.
        state.query = "hello".into();
        assert_eq!(names(&mut state), [(0, "FontString#10".to_owned())]);
    }

    #[test]
    fn a_pick_selects_its_frame_and_the_end_clears_the_session() {
        let mut state = running();
        assert!(state.running());
        state.pick(&NoSystem, 960.0, 540.0);
        state.event(
            1,
            GameEvent::Reply {
                id: 1,
                result: Ok(json!({ "frame": 7, "region": 8 })),
            },
            0.0,
        );
        assert_eq!(state.selected, Some(7));
        // A reply nobody asked for, or another session's, is not heard.
        state.event(
            1,
            GameEvent::Reply {
                id: 99,
                result: Ok(json!({ "frame": 3 })),
            },
            0.0,
        );
        state.event(2, GameEvent::Ended { error: None }, 0.0);
        assert_eq!(state.selected, Some(7));
        assert!(state.running());
        state.event(
            1,
            GameEvent::Ended {
                error: Some("boom".into()),
            },
            0.0,
        );
        assert!(!state.running());
        assert_eq!(state.ended, Some(Some("boom".into())));
    }

    #[test]
    fn settings_are_sent_as_the_game_writes_them() {
        let mut state = running();
        state.size = (1280, 720);
        state.use_ui_scale = true;
        state.ui_scale = 0.8;
        // (f32's 0.8 is 0.800000011920929: the CVar the game writes is 0.8)
        let sent = (f64::from(state.ui_scale) * 100.0).round() / 100.0;
        assert_eq!(sent, 0.8);
        state.apply_settings(&NoSystem);
        assert!(state.waiting(&Asked::Settings));
    }
}
