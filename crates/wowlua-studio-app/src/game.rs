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
use wowlua_studio_ui::{Face, Icon, Palette, icons, palette};

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
    /// Plays: the key bindings move the character, the mouse turns the
    /// camera and targets (S079).
    Play,
    Click,
    Hover,
    Drag,
    Type,
    Key,
}

const MODES: [(Mode, &str); 7] = [
    (Mode::Inspect, "inspect"),
    (Mode::Play, "play"),
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

/// The chat lines kept (S077).
const CHAT_KEPT: usize = 1000;

/// The chat's channels: the chat type and its name.
const CHANNELS: [(&str, &str); 6] = [
    ("SAY", "say"),
    ("PARTY", "party"),
    ("RAID", "raid"),
    ("GUILD", "guild"),
    ("YELL", "yell"),
    ("WHISPER", "whisper"),
];

/// What a chat line is, for the chat's filter: the game's yellow system
/// lines, lines with no colour (an addon's `print`), the rest chat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChatKind {
    Chat,
    System,
    Addon,
}

/// A line of the game's chat windows.
#[derive(Clone, Debug, PartialEq)]
struct ChatRow {
    text: String,
    color: Option<[f32; 3]>,
    kind: ChatKind,
}

impl ChatRow {
    fn of(line: &Json) -> ChatRow {
        let color = line["color"].as_array().and_then(|c| {
            let v: Vec<f32> = c
                .iter()
                .filter_map(|x| x.as_f64())
                .map(|x| x as f32)
                .collect();
            (v.len() == 3).then(|| [v[0], v[1], v[2]])
        });
        let kind = match color {
            None => ChatKind::Addon,
            Some([r, g, b]) if r > 0.95 && g > 0.95 && b < 0.05 => ChatKind::System,
            Some(_) => ChatKind::Chat,
        };
        ChatRow {
            text: line["text"].as_str().unwrap_or("").to_owned(),
            color,
            kind,
        }
    }
}

/// A chat line's text as the game shows it (S077): `|cffRRGGBB…|r` its
/// colours, a link (`|H…|htext|h`) its text, textures and atlases
/// (`|T…|t`, `|A…|a`) left out, `|n` a new line, `||` a bar; each span
/// with its colour (none: the line's).
fn chat_spans(text: &str) -> Vec<(String, Option<[f32; 3]>)> {
    let mut spans: Vec<(String, Option<[f32; 3]>)> = Vec::new();
    let mut color: Option<[f32; 3]> = None;
    let mut cur = String::new();
    let flush = |spans: &mut Vec<(String, Option<[f32; 3]>)>, cur: &mut String, color| {
        if !cur.is_empty() {
            spans.push((std::mem::take(cur), color));
        }
    };
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    // (skips to just after `|` + `end`)
    let skip_to = |i: usize, end: char| {
        let mut j = i;
        while j + 1 < chars.len() {
            if chars[j] == '|' && chars[j + 1] == end {
                return j + 2;
            }
            j += 1;
        }
        chars.len()
    };
    while i < chars.len() {
        let c = chars[i];
        if c != '|' || i + 1 >= chars.len() {
            cur.push(c);
            i += 1;
            continue;
        }
        match chars[i + 1] {
            '|' => {
                cur.push('|');
                i += 2;
            }
            'n' => {
                cur.push('\n');
                i += 2;
            }
            'c' => {
                flush(&mut spans, &mut cur, color);
                let hex: String = chars.iter().skip(i + 2).take(8).collect();
                if hex.len() == 8 && hex.chars().all(|h| h.is_ascii_hexdigit()) {
                    let byte = |k: usize| {
                        u8::from_str_radix(&hex[k..k + 2], 16).unwrap_or(255) as f32 / 255.0
                    };
                    color = Some([byte(2), byte(4), byte(6)]);
                    i += 10;
                } else {
                    // (a named colour, `|cnNAME:`: the line's)
                    let mut j = i + 2;
                    while j < chars.len() && chars[j] != ':' {
                        j += 1;
                    }
                    color = None;
                    i = (j + 1).min(chars.len());
                }
            }
            'r' => {
                flush(&mut spans, &mut cur, color);
                color = None;
                i += 2;
            }
            // (a link's data up to `|h`; its text follows, closed by `|h`)
            'H' => i = skip_to(i + 2, 'h'),
            'h' => i += 2,
            'T' => i = skip_to(i + 2, 't'),
            'A' => i = skip_to(i + 2, 'a'),
            _ => {
                cur.push(c);
                i += 1;
            }
        }
    }
    flush(&mut spans, &mut cur, color);
    spans
}

/// The chat box's own commands: `/p hi` is (PARTY, none, "hi"), `/w Name
/// hi` (WHISPER, Name, "hi"); other lines none.
pub fn chat_slash(line: &str) -> Option<(String, Option<String>, String)> {
    let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
    let chat_type = match cmd.to_lowercase().as_str() {
        "/s" | "/say" => "SAY",
        "/y" | "/yell" | "/sh" | "/shout" => "YELL",
        "/p" | "/party" => "PARTY",
        "/ra" | "/raid" => "RAID",
        "/g" | "/guild" => "GUILD",
        "/o" | "/officer" => "OFFICER",
        "/i" | "/instance" => "INSTANCE_CHAT",
        "/e" | "/em" | "/me" | "/emote" => "EMOTE",
        "/w" | "/whisper" | "/t" | "/tell" => "WHISPER",
        _ => return None,
    };
    let rest = rest.trim();
    if chat_type == "WHISPER" {
        let (name, message) = rest.split_once(' ')?;
        let message = message.trim();
        return (!message.is_empty()).then(|| {
            (
                chat_type.to_owned(),
                Some(name.to_owned()),
                message.to_owned(),
            )
        });
    }
    (!rest.is_empty()).then(|| (chat_type.to_owned(), None, rest.to_owned()))
}

/// A request asked, by its id: what its answer is for.
#[derive(Clone, Debug, PartialEq)]
enum Asked {
    Tree,
    Inspect(usize),
    Pick,
    Units,
    Target,
    Events,
    Chat,
    Advance,
    Settings,
    WorldPoint,
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
    /// Input changed the game since the frame tree was read (S086).
    tree_stale: bool,
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
    // The chat (S077).
    chat: VecDeque<ChatRow>,
    chat_next: u64,
    pub chat_filter: usize,
    chat_text: String,
    chat_chan: usize,
    /// Full screen (S077): the game's window size before it.
    pub full: Option<(u32, u32)>,
    /// Lines the screenshot harness types into the chat once it runs.
    pub chat_later: Vec<String>,
    /// A unit being placed in the game view (S080): the world page's ask,
    /// and the places clicked so far.
    pub placing: Option<(crate::world::Placing, Vec<[f64; 3]>)>,
    /// A placing done: for the app to write into its profile.
    pub placed: Option<(crate::world::Placing, Vec<[f64; 3]>)>,
    /// A key the screenshot harness holds in play mode once the game runs,
    /// for so many seconds; and until when it is held.
    pub hold_later: Option<(String, f64)>,
    held_until: Option<(String, f64)>,
    /// The session's clock is manual (the debugger's): a chat line moves
    /// it a frame, for the server's answer to come.
    pub manual: bool,
    started_at: Option<f64>,
    /// The keys held down in play mode, as WoW names each (its up goes
    /// by the same name).
    held: std::collections::HashMap<Key, String>,
    /// The mouse button held down in play mode (true: the right one),
    /// pressed on the screen (S084).
    play_held: Option<bool>,
    /// The 3D world behind the UI (M13, S078); the UI draws over black
    /// without it.
    pub world: bool,
    /// What the session was told of the world last (none: nothing yet).
    world_sent: Option<bool>,
    /// Whether the session was told the game is in the foreground (S085).
    foreground_sent: Option<bool>,
    /// Play mode runs the session's clock in real time over a manual one
    /// (S079): whether it does now.
    clock_run: bool,
    /// The session reads the installation's game data: the world can be
    /// shown (Settings › game screen › answer from the installation's data).
    pub world_ready: bool,
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
    /// The game screen over the whole display (S077).
    FullScreen,
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
            self.full,
        );
        let next = self.next_session;
        let (world, world_ready) = (self.world, self.world_ready);
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
            full: keep.7,
            shots: std::mem::take(&mut self.shots),
            chat_later: std::mem::take(&mut self.chat_later),
            placing: self.placing.take(),
            hold_later: self.hold_later.take(),
            world,
            world_ready,
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
            (Asked::WorldPoint, Ok(at)) => {
                let point = (|| Some([at["y"].as_f64()?, at["x"].as_f64()?, at["z"].as_f64()?]))();
                match (point, self.placing.as_mut()) {
                    (Some(p), Some((placing, points))) => {
                        points.push(p);
                        if !placing.path {
                            self.placed = self.placing.take();
                        }
                    }
                    (None, Some(_)) => {
                        self.toast = Some(("no ground there: click the world".into(), now));
                    }
                    _ => {}
                }
            }
            (Asked::Chat, Ok(data)) => {
                for line in data["lines"].as_array().into_iter().flatten() {
                    if self.chat.len() == CHAT_KEPT {
                        self.chat.pop_front();
                    }
                    self.chat.push_back(ChatRow::of(line));
                }
                if let Some(next) = data["next"].as_u64() {
                    self.chat_next = next;
                }
            }
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
                self.tree_stale = true;
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

    /// Sends the chat line typed (S077): the chat box's own commands
    /// (`/p hi`, `/w Name hi`) and a line on the channel picked as chat,
    /// another `/command` as a slash command.
    fn send_chat(&mut self, system: &dyn System) {
        let text = self.chat_text.trim().to_owned();
        if text.is_empty() {
            return;
        }
        self.chat_text.clear();
        let given = self.given.len();
        let chat = |chat_type: &str, target: Option<String>, message: String| GameInput::Chat {
            text: message,
            chat_type: chat_type.to_owned(),
            target,
        };
        if let Some((chat_type, target, message)) = chat_slash(&text) {
            self.give(system, "chat", text, chat(&chat_type, target, message));
        } else if text.starts_with('/') {
            self.give(system, "slash", text.clone(), GameInput::Slash(text));
        } else {
            let chat_type = CHANNELS[self.chat_chan.min(CHANNELS.len() - 1)].0;
            if chat_type == "WHISPER" {
                // ("Name message")
                match text.split_once(' ') {
                    Some((name, message)) => {
                        let input =
                            chat(chat_type, Some(name.to_owned()), message.trim().to_owned());
                        self.give(system, "chat", text, input);
                    }
                    None => self.chat_text = text,
                }
            } else {
                self.give(system, "chat", text.clone(), chat(chat_type, None, text));
            }
        }
        // (a chat line given: its answer comes on the next frame)
        if self.given.len() > given && self.given.last().is_some_and(|g| g.kind == "chat") {
            self.answer_comes(system);
        }
    }

    /// The screenshot harness's chat lines, once the game ran a while
    /// (its login told).
    pub fn send_later(&mut self, system: &dyn System, now: f64, focused: bool) {
        // (the game in the foreground while Studio's window is: its frame
        // rate the game's settings', in the background `maxFPSBk`, S085)
        if self.running() && self.foreground_sent != Some(focused) {
            self.ask(
                system,
                Asked::Settings,
                GameRequest::SetForeground { on: focused },
            );
            self.foreground_sent = Some(focused);
        }
        // (the world as the checkbox has it, each session told once a
        // change; S078)
        if !self.world_ready {
            self.world = false;
        }
        if self.running() && self.world_sent != Some(self.world) {
            let on = self.world;
            self.ask(system, Asked::Settings, GameRequest::ShowWorld { on });
            self.world_sent = Some(on);
        }
        // (play mode on a manual clock: time runs while playing, and the
        // clock is manual again after, S079)
        let run = self.mode == Mode::Play && self.manual;
        if self.running() && run != self.clock_run {
            self.ask(
                system,
                Asked::Settings,
                GameRequest::SetClock { manual: !run },
            );
            self.clock_run = run;
        }
        if let Some((key, until)) = self.held_until.clone()
            && now >= until
        {
            self.play(system, GameInput::KeyUp(key));
            self.held_until = None;
        }
        if self.running()
            && self.hold_later.is_some()
            && now - *self.started_at.get_or_insert(now) >= 8.0
            && let Some((key, seconds)) = self.hold_later.take()
        {
            self.mode = Mode::Play;
            self.play(system, GameInput::KeyDown(key.clone()));
            self.held_until = Some((key, now + seconds));
        }
        if self.chat_later.is_empty() || !self.running() {
            return;
        }
        let since = *self.started_at.get_or_insert(now);
        if now - since < 8.0 {
            return;
        }
        for line in std::mem::take(&mut self.chat_later) {
            self.chat_text = line;
            self.send_chat(system);
        }
    }

    /// A chat line's answer comes on the next frame (the server's): on a
    /// manual clock the game moves that frame.
    fn answer_comes(&mut self, system: &dyn System) {
        if self.manual {
            self.ask(
                system,
                Asked::Advance,
                GameRequest::Advance {
                    frames: Some(2),
                    seconds: None,
                },
            );
        }
    }

    /// Full screen (S077): the game's window as large as the display, as
    /// the game is.
    pub fn enter_full(&mut self, system: &dyn System, display: (u32, u32)) {
        if self.full.is_some() {
            return;
        }
        if self.size == (0, 0) {
            self.size = (1920, 1080);
            self.ui_scale = 1.0;
        }
        self.full = Some(self.size);
        self.size = display;
        self.apply_settings(system);
    }

    /// Back from full screen: the window's size as it was.
    pub fn leave_full(&mut self, system: &dyn System) {
        if let Some(size) = self.full.take() {
            self.size = size;
            self.apply_settings(system);
        }
    }

    /// Play mode's input: many a second, so not in the input panel's list.
    fn play(&mut self, system: &dyn System, input: GameInput) {
        self.ask(system, Asked::Settings, GameRequest::Input(input));
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
    if toolbar(ui, state, system, p) {
        action = Some(Action::FullScreen);
    }
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
    // A unit being placed (S080): what to click, and done for a path.
    if let Some((placing, points)) = &state.placing {
        let (label, path, n) = (placing.label.clone(), placing.path, points.len());
        let mut done = false;
        let mut cancel = false;
        forms::padded(ui, 12, 6, |ui| {
            forms::callout(ui, CalloutKind::Note, |ui| {
                ui.horizontal(|ui| {
                    let text = match (path, state.running()) {
                        (_, false) => format!("start the game to place {label}"),
                        (false, true) => format!("click where {label} stands"),
                        (true, true) => format!("click {label}'s waypoints, in order · {n} so far"),
                    };
                    forms::paragraph(ui, &text, 12.0, 18.0, p.ink);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::button(ui, ButtonKind::Tertiary, "cancel", true, None).clicked()
                        {
                            cancel = true;
                        }
                        if path
                            && widgets::button(ui, ButtonKind::Primary, "done", n >= 2, None)
                                .clicked()
                        {
                            done = true;
                        }
                    });
                });
            });
        });
        if cancel {
            state.placing = None;
        } else if done {
            state.placed = state.placing.take();
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

/// The screen's toolbar; returns whether full screen was asked for.
fn toolbar(ui: &mut Ui, state: &mut GameState, system: &dyn System, p: &Palette) -> bool {
    let mut full = false;
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
            // The 3D world behind the UI: a toggle of the screen (S076,
            // S078), with the installation's game data.
            world_checkbox(ui, state);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(state.running(), |ui| {
                    if widgets::button(ui, ButtonKind::Tertiary, "screenshot", true, None).clicked()
                    {
                        shoot(state, system, None);
                    }
                });
                if widgets::icon_button(
                    ui,
                    Icon::FullScreen,
                    "full screen: the game alone, as in the game · alt enter leaves it",
                    false,
                )
                .clicked()
                {
                    full = true;
                }
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
    full
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
    // (placing a unit: a click is a place of the world, S080)
    if state.placing.is_some() {
        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let (x, y) = at(pos);
            state.ask(system, Asked::WorldPoint, GameRequest::WorldPoint { x, y });
        }
        return;
    }
    match state.mode {
        Mode::Inspect => {
            if response.clicked()
                && let Some(pos) = response.interact_pointer_pos()
            {
                let (x, y) = at(pos);
                state.ask(system, Asked::Pick, GameRequest::Pick { x, y });
            }
        }
        Mode::Play => {
            // (the mouse as the game takes it, S084: where it is, its
            // buttons and wheel; the game decides what the frame or the
            // world there does: hover, click, drag, the camera)
            let (pos, moved, scroll) = ui.input(|i| {
                (
                    i.pointer.latest_pos(),
                    i.pointer.delta() != egui::Vec2::ZERO,
                    i.smooth_scroll_delta().y,
                )
            });
            if let Some(pos) = pos {
                let inside = response.rect.contains(pos);
                let (x, y) = at(pos);
                if moved && (inside || state.play_held.is_some()) {
                    state.play(system, GameInput::MouseMove { x, y });
                }
                for (button, right) in [
                    (egui::PointerButton::Primary, false),
                    (egui::PointerButton::Secondary, true),
                ] {
                    let (pressed, released) = ui.input(|i| {
                        (
                            i.pointer.button_pressed(button),
                            i.pointer.button_released(button),
                        )
                    });
                    if pressed && inside && state.play_held.is_none() {
                        state.play_held = Some(right);
                        state.play(system, GameInput::MouseDown { x, y, right });
                        response.request_focus();
                    }
                    if released && state.play_held == Some(right) {
                        state.play_held = None;
                        state.play(system, GameInput::MouseUp { x, y, right });
                    }
                }
                if inside && scroll != 0.0 {
                    // (a notch, about 50 points)
                    let delta = f64::from(scroll) / 50.0;
                    state.play(system, GameInput::Wheel { x, y, delta });
                }
            }
            keys(ui, state, system, response);
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
        Mode::Type | Mode::Key => keys(ui, state, system, response),
    }
}

/// The keyboard on the screen while it has the focus (a click gives it):
/// text typed, keys pressed, or in play mode keys held.
fn keys(ui: &mut Ui, state: &mut GameState, system: &dyn System, response: &egui::Response) {
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
                // (play: text into the focused edit box, S084)
                (Mode::Play, egui::Event::Text(t)) => {
                    state.play(system, GameInput::Text(t));
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
                // (play: held down until let go, by the same name)
                (
                    Mode::Play,
                    egui::Event::Key {
                        key,
                        pressed,
                        repeat: false,
                        modifiers,
                        ..
                    },
                ) => {
                    if pressed {
                        if let Some(name) = wow_key(key, modifiers) {
                            state.held.insert(key, name.clone());
                            state.play(system, GameInput::KeyDown(name));
                        }
                    } else if let Some(name) = state.held.remove(&key) {
                        state.play(system, GameInput::KeyUp(name));
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
    // Read when shown, then again after a target, the game's settings,
    // `hidden` or the refresh button, and after input at most once a
    // second: a large UI's whole tree read on a timer cost its frames
    // (SuuUI's: 200 ms each), and while playing every mouse move is input
    // (S086).
    let every = if state.tree_stale { 1.0 } else { f64::INFINITY };
    if state.poll(now, "tree", every) && !state.waiting(&Asked::Tree) {
        state.tree_stale = false;
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
             screen's toolbar, or type in the chat tab.",
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

/// The game screen's "3D world" checkbox (S078): the zone behind the UI,
/// with the installation's game data; without, why not.
fn world_checkbox(ui: &mut Ui, state: &mut GameState) {
    let ready = state.world_ready;
    let r = ui
        .add_enabled_ui(ready, |ui| {
            forms::checkbox(ui, &mut state.world, "3D world", None);
        })
        .response;
    if !ready {
        r.on_disabled_hover_text(
            "the 3D world reads your installation's game data: turn on Settings › game screen › answer from the installation's data",
        );
    }
}

/// The game's chat (the chat panel, S077): its chat windows' lines in
/// their colours, a filter, a line to type a message or a /command in.
pub fn chat(ui: &mut Ui, state: &mut GameState, system: &dyn System) {
    let p = palette(ui.ctx());
    let now = ui.input(|i| i.time);
    if !state.running() {
        empty(ui, "No session: the chat is the running client's.", p);
        return;
    }
    if state.poll(now, "chat", 0.25) && !state.waiting(&Asked::Chat) {
        let since = state.chat_next;
        state.ask(system, Asked::Chat, GameRequest::ChatLines { since });
    }
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(250));
    forms::padded(ui, 10, 4, |ui| {
        ui.horizontal(|ui| {
            let labels = ["all", "chat", "system", "addons"];
            if let Some(i) = widgets::seg(ui, "game-chat-filter", &labels, state.chat_filter, false)
            {
                state.chat_filter = i;
            }
        });
    });
    // (the game's colours; on a light ground, darker)
    let light = p.bg_000.r() > 128;
    let ink = |row: &ChatRow| match row.color {
        None => p.ink,
        Some([r, g, b]) => {
            let k = if light { 0.55 } else { 1.0 };
            egui::Color32::from_rgb(
                (r * k * 255.0) as u8,
                (g * k * 255.0) as u8,
                (b * k * 255.0) as u8,
            )
        }
    };
    let filter = state.chat_filter;
    let shown: Vec<&ChatRow> = state
        .chat
        .iter()
        .filter(|r| match filter {
            1 => r.kind == ChatKind::Chat,
            2 => r.kind == ChatKind::System,
            3 => r.kind == ChatKind::Addon,
            _ => true,
        })
        .collect();
    let list_h = (ui.available_height() - 38.0).max(40.0);
    ui.allocate_ui(Vec2::new(ui.available_width(), list_h), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("game-chat")
            .auto_shrink(false)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                forms::padded(ui, 10, 2, |ui| {
                    if shown.is_empty() {
                        ui.label(
                            egui::RichText::new("Nothing in the chat yet.")
                                .font(Face::Plex.at(12.0))
                                .color(p.ink_faint),
                        );
                    }
                    let width = ui.available_width();
                    for row in &shown {
                        let mut job = egui::text::LayoutJob::default();
                        job.wrap.max_width = width;
                        for (text, color) in chat_spans(&row.text) {
                            let color = color.map_or_else(
                                || ink(row),
                                |[r, g, b]| {
                                    ink(&ChatRow {
                                        text: String::new(),
                                        color: Some([r, g, b]),
                                        kind: row.kind,
                                    })
                                },
                            );
                            job.append(
                                &text,
                                0.0,
                                egui::TextFormat {
                                    font_id: Face::Plex.at(12.0),
                                    color,
                                    ..Default::default()
                                },
                            );
                        }
                        ui.label(job);
                    }
                });
            });
    });
    let mut send = false;
    forms::padded(ui, 10, 4, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let options: Vec<(&str, bool)> = CHANNELS.iter().map(|(_, l)| (*l, true)).collect();
            forms::select_sized(
                ui,
                "game-chat-channel",
                &options,
                &mut state.chat_chan,
                100.0,
                26.0,
            );
            let send_w = widgets::button_width(ui, "send", true);
            let r = forms::text_input(
                ui,
                &mut state.chat_text,
                (ui.available_width() - send_w - 8.0).max(80.0),
                "a message, or a /command: /suu, /reload, /w Healer hi",
            );
            if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                send = true;
            }
            if widgets::button(ui, ButtonKind::Secondary, "send", true, None).clicked() {
                send = true;
            }
        });
    });
    if send {
        state.send_chat(system);
    }
}

/// The game screen alone over the whole display (S077): the screen at the
/// display's size, and a bar floating over it that the user moves.
/// Returns whether to leave full screen.
pub fn full_screen(ui: &mut Ui, state: &mut GameState, system: &dyn System) -> bool {
    let p = palette(ui.ctx());
    let area = ui.max_rect();
    ui.painter()
        .rect_filled(area, CornerRadius::ZERO, egui::Color32::BLACK);
    let mut leave = false;
    if let Some(screen) = system.game_screen().filter(|_| state.running()) {
        let size = Vec2::new(screen.size[0] as f32, screen.size[1] as f32);
        let scale = (area.width() / size.x)
            .min(area.height() / size.y)
            .max(0.05);
        let rect = Rect::from_center_size(area.center(), size * scale);
        let response = ui.interact(
            rect,
            ui.make_persistent_id("game-full-screen"),
            Sense::click_and_drag(),
        );
        ui.painter().image(
            screen.texture,
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        pointer(ui, state, system, &response, rect, scale);
    }
    // The bar: dragged by its ground, it stays where it is left.
    egui::Area::new(egui::Id::unique("game-full-screen-bar"))
        .movable(true)
        .order(egui::Order::Foreground)
        .default_pos(area.right_bottom() - Vec2::new(560.0, 60.0))
        .show(ui.ctx(), |ui| {
            egui::Frame::new()
                .fill(p.bg_200.gamma_multiply(0.92))
                .stroke(egui::Stroke::new(1.0, p.line_strong))
                .inner_margin(egui::Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        icons::paint(
                            ui.painter(),
                            Rect::from_min_size(
                                ui.cursor().min + Vec2::new(0.0, 6.0),
                                Vec2::splat(14.0),
                            ),
                            Icon::Grip,
                            p.ink_faint,
                        );
                        ui.add_space(16.0);
                        let labels: Vec<&str> = MODES.iter().map(|(_, l)| *l).collect();
                        let at = MODES
                            .iter()
                            .position(|(m, _)| *m == state.mode)
                            .unwrap_or(0);
                        if let Some(i) = widgets::seg(ui, "game-full-mode", &labels, at, false) {
                            state.mode = MODES[i].0;
                        }
                        world_checkbox(ui, state);
                        ui.add_enabled_ui(state.running(), |ui| {
                            if widgets::icon_button(ui, Icon::Snapshot, "screenshot", false)
                                .clicked()
                            {
                                shoot(state, system, None);
                            }
                        });
                        if widgets::button(
                            ui,
                            ButtonKind::Secondary,
                            "exit full screen",
                            true,
                            Some(Icon::ExitFullScreen),
                        )
                        .on_hover_text("alt enter too")
                        .clicked()
                        {
                            leave = true;
                        }
                    });
                });
        });
    leave
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

#[cfg(test)]
mod chat_tests {
    use super::*;

    fn placing(path: bool) -> crate::world::Placing {
        crate::world::Placing {
            profile: "wolves".into(),
            unit: crate::world::json::Key::Index(0),
            label: "Young Wolf".into(),
            path,
            movement: None,
        }
    }

    /// Placing a unit (S080): a place is its first point; a path's points
    /// gather until done; no ground there leaves it as it was.
    #[test]
    fn a_unit_is_placed_by_the_points_clicked() {
        let mut state = GameState {
            placing: Some((placing(false), Vec::new())),
            ..GameState::default()
        };
        state.answer(Asked::WorldPoint, Ok(Json::Null), 0.0);
        assert!(state.placing.is_some() && state.placed.is_none());
        let point = serde_json::json!({ "y": 1.0, "x": 2.0, "z": 3.0 });
        state.answer(Asked::WorldPoint, Ok(point), 0.0);
        let (p, points) = state.placed.take().expect("placed");
        assert_eq!((p.path, points), (false, vec![[1.0, 2.0, 3.0]]));
        assert!(state.placing.is_none());
        state.placing = Some((placing(true), Vec::new()));
        for i in 0..3 {
            let v = f64::from(i);
            let point = serde_json::json!({ "y": v, "x": v, "z": 0.0 });
            state.answer(Asked::WorldPoint, Ok(point), 0.0);
        }
        assert!(state.placed.is_none());
        assert_eq!(state.placing.as_ref().map(|(_, p)| p.len()), Some(3));
    }

    #[test]
    fn the_chat_box_s_own_commands_are_chat() {
        assert_eq!(
            chat_slash("/p pulling"),
            Some(("PARTY".into(), None, "pulling".into()))
        );
        assert_eq!(
            chat_slash("/w Healer behind it"),
            Some(("WHISPER".into(), Some("Healer".into()), "behind it".into()))
        );
        assert_eq!(
            chat_slash("/SAY hi"),
            Some(("SAY".into(), None, "hi".into()))
        );
        assert_eq!(chat_slash("/w Healer"), None, "a whisper needs a message");
        assert_eq!(chat_slash("/p"), None);
        assert_eq!(chat_slash("/suu modules"), None, "a slash command");
    }

    #[test]
    fn a_chat_line_s_markup_is_its_colours_and_its_links_text() {
        let spans = chat_spans(
            "Code of |HurlIndex:64|h|cnVISIBLE_URL_DEFAULT_CHAT_LINK_COLOR:https://x.example|r|h for more",
        );
        let text: String = spans.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(text, "Code of https://x.example for more");
        let spans = chat_spans("|cffff0000red|r and |T134400:0|ticon || bar");
        assert_eq!(spans[0], ("red".to_owned(), Some([1.0, 0.0, 0.0])));
        let text: String = spans.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(text, "red and icon | bar");
    }

    #[test]
    fn a_chat_line_is_chat_system_or_an_addon_s_by_its_colour() {
        let row = |color: Json| ChatRow::of(&serde_json::json!({ "text": "x", "color": color }));
        assert_eq!(row(Json::Null).kind, ChatKind::Addon);
        assert_eq!(
            row(serde_json::json!([1.0, 1.0, 0.0])).kind,
            ChatKind::System
        );
        assert_eq!(
            row(serde_json::json!([0.67, 0.67, 1.0])).kind,
            ChatKind::Chat
        );
    }
}
