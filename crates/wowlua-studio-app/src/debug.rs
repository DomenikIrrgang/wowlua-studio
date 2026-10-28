//! The debugger page (the debugger and sessions boards, S056): the session
//! the game page runs, under wowlua's debugger (one per window), started
//! from a launch configuration (`.wowlua/launch.json`, VS Code's wowlua
//! entries); breakpoints of every kind, kept per workspace; the stop with
//! its stack, variables, watches and console; the clock; the live world.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};

use egui::{Align2, CornerRadius, Key, Pos2, Sense, Ui, Vec2};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use wowlua_studio_ui::forms::{self, CalloutKind};
use wowlua_studio_ui::widgets::{self, ButtonKind};
use wowlua_studio_ui::{Face, Palette, palette};

use crate::editor::view::{BpKind, BpMark};
use crate::system::{FunctionBp, GameEvent, GameRequest, Launch, LineBp, System, Workspace};

/// The debugger's requests' ids: apart from the game page's.
pub const IDS: u64 = 1 << 40;
const ROW_H: f32 = 22.0;
/// The most console lines kept.
const CONSOLE_KEPT: usize = 2000;

/// What stops the session, and the board's words for it.
pub const STOP_ON: [(&str, &str, bool); 4] = [
    ("uncaught", "uncaught errors", true),
    ("all", "every error, caught or not", false),
    ("taint", "secure code tainted", false),
    ("blocked", "actions blocked by taint", true),
];

/// A launch configuration (`.wowlua/launch.json`'s, VS Code's).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Config {
    pub name: String,
    /// A test file (from the package's root); none: the addon.
    pub test: Option<String>,
    pub filters: Vec<String>,
    pub flavor: Option<String>,
    pub profile: Option<String>,
    pub manual: bool,
    pub stop_on_entry: bool,
    pub debug_blizzard_ui: bool,
    /// Read from VS Code's `launch.json` (Studio writes its own).
    pub vscode: bool,
}

impl Config {
    /// The configuration for running the addon.
    pub fn addon(name: &str) -> Config {
        Config {
            name: name.to_owned(),
            ..Config::default()
        }
    }

    /// One test (or a test file) of `file`.
    pub fn test(file: &str, test: Option<&str>) -> Config {
        Config {
            name: match test {
                Some(t) => format!("{file} · {t}"),
                None => file.to_owned(),
            },
            test: Some(file.to_owned()),
            filters: test.map(|t| vec![t.to_owned()]).unwrap_or_default(),
            ..Config::default()
        }
    }

    pub fn launch(&self) -> Launch {
        Launch {
            test: self
                .test
                .as_ref()
                .map(|t| (t.clone(), self.filters.clone())),
            flavor: self.flavor.clone(),
            profile: self.profile.clone(),
            manual: self.manual,
            stop_on_entry: self.stop_on_entry,
            debug_blizzard_ui: self.debug_blizzard_ui,
        }
    }

    /// Its launch options as `launch.json` writes them.
    fn json(&self) -> Json {
        let mut j = json!({ "type": "wowlua", "request": "launch", "name": self.name });
        match &self.test {
            Some(t) => {
                j["test"] = json!(t);
                if !self.filters.is_empty() {
                    j["filter"] = json!(self.filters);
                }
            }
            None => j["addon"] = json!(true),
        }
        if let Some(f) = &self.flavor {
            j["flavor"] = json!(f);
        }
        if let Some(p) = &self.profile {
            j["profile"] = json!(p);
        }
        j["clock"] = json!(if self.manual { "manual" } else { "realtime" });
        if self.stop_on_entry {
            j["stopOnEntry"] = json!(true);
        }
        if self.debug_blizzard_ui {
            j["debugBlizzardUI"] = json!(true);
        }
        j
    }

    fn of(j: &Json, vscode: bool) -> Option<Config> {
        if j["type"].as_str().is_some_and(|t| t != "wowlua") {
            return None;
        }
        let filters = match &j["filter"] {
            Json::String(s) => vec![s.clone()],
            Json::Array(a) => a
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect(),
            _ => Vec::new(),
        };
        Some(Config {
            name: j["name"].as_str().unwrap_or("wowlua").to_owned(),
            test: j["test"].as_str().map(str::to_owned),
            filters,
            flavor: j["flavor"].as_str().map(str::to_owned),
            profile: j["profile"].as_str().map(str::to_owned),
            manual: j["clock"].as_str() == Some("manual"),
            stop_on_entry: j["stopOnEntry"].as_bool().unwrap_or(false),
            debug_blizzard_ui: j["debugBlizzardUI"].as_bool().unwrap_or(false),
            vscode,
        })
    }
}

/// `launch.json`'s text without its comments and trailing commas (VS Code
/// writes JSON with comments).
pub fn strip_jsonc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_str = false;
    while let Some(c) = chars.next() {
        if in_str {
            out.push(c);
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match (c, chars.peek()) {
            ('"', _) => {
                in_str = true;
                out.push(c);
            }
            ('/', Some('/')) => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut last = ' ';
                for n in chars.by_ref() {
                    if last == '*' && n == '/' {
                        break;
                    }
                    last = n;
                }
            }
            _ => out.push(c),
        }
    }
    // Trailing commas before a closing bracket.
    let mut cleaned = String::with_capacity(out.len());
    let bytes: Vec<char> = out.chars().collect();
    let mut in_str = false;
    for (i, &c) in bytes.iter().enumerate() {
        if in_str {
            cleaned.push(c);
            if c == '"' && bytes.get(i.wrapping_sub(1)) != Some(&'\\') {
                in_str = false;
            }
            continue;
        }
        if c == '"' {
            in_str = true;
        }
        if c == ',' {
            let next = bytes[i + 1..].iter().find(|n| !n.is_whitespace());
            if matches!(next, Some('}') | Some(']')) {
                continue;
            }
        }
        cleaned.push(c);
    }
    cleaned
}

/// A workspace's configurations: `.wowlua/launch.json`'s, then VS Code's
/// wowlua entries.
pub fn read_configs(root: &Path, system: &dyn System) -> Vec<Config> {
    let mut out = Vec::new();
    for (file, vscode) in [
        (".wowlua/launch.json", false),
        (".vscode/launch.json", true),
    ] {
        let Ok(text) = system.read_file(&root.join(file)) else {
            continue;
        };
        let Ok(j) = serde_json::from_str::<Json>(&strip_jsonc(&text)) else {
            continue;
        };
        for c in j["configurations"].as_array().into_iter().flatten() {
            if let Some(c) = Config::of(c, vscode) {
                out.push(c);
            }
        }
    }
    out
}

/// What a workspace keeps between starts (Studio's store).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Kept {
    /// Line breakpoints by file.
    pub lines: BTreeMap<PathBuf, Vec<LineBp>>,
    pub functions: Vec<FunctionBp>,
    pub stop_on: BTreeSet<String>,
    pub watches: Vec<String>,
    pub history: Vec<String>,
    /// Recent sessions: the configuration, how it ended, when.
    pub recent: Vec<(String, String, u64)>,
    pub chosen: Option<String>,
}

/// A request asked, by its id.
#[derive(Clone, Debug, PartialEq)]
enum Asked {
    Lines(PathBuf, Vec<u32>),
    Functions,
    Stack,
    Scopes(u32),
    Vars(u32),
    Watch(usize),
    Eval,
    Hits,
    Units,
    SaveWorld(PathBuf),
    Plain,
}

/// The stop: why, its text, an exception's details.
#[derive(Clone, Debug, PartialEq)]
pub struct Stop {
    pub reason: String,
    pub text: Option<String>,
    pub detail: Option<Json>,
}

/// How a session ended, as the sessions board sums it up.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ended {
    pub config: String,
    pub error: Option<String>,
    pub time: f64,
    pub frames: u64,
    pub stops: u32,
    pub logs: u32,
    pub errors: u32,
}

#[derive(Default)]
pub struct DebugState {
    pub kept: Kept,
    pub configs: Vec<Config>,
    pub configs_read: bool,
    /// The configuration being edited (the dialog), its place (none: new).
    pub editing: Option<(Option<usize>, Config)>,
    /// The line breakpoint being edited (the dialog): file, line.
    pub editing_bp: Option<(PathBuf, u32, LineBp)>,
    // The session.
    pub session: Option<u64>,
    pub config: Option<Config>,
    pub stopped: Option<Stop>,
    frames: Vec<Json>,
    pub frame: Option<u32>,
    scopes: Vec<Json>,
    vars: HashMap<u32, Vec<Json>>,
    open: BTreeSet<u32>,
    watch_values: Vec<Option<Result<String, String>>>,
    /// Breakpoints' ids: line breakpoints' file and line, functions' index.
    ids: HashMap<u32, (Option<PathBuf>, u32)>,
    /// By id: placed, the line it took, hits.
    states: HashMap<u32, (bool, Option<u32>, u32)>,
    console: VecDeque<(String, String)>,
    input: String,
    back: usize,
    units: Vec<Json>,
    world: String,
    event: String,
    args: String,
    profile_name: String,
    function: String,
    watch_input: String,
    pub ended: Option<Ended>,
    stops: u32,
    logs: u32,
    errors: u32,
    next: u64,
    asked: HashMap<u64, Asked>,
    polled: f64,
    /// The source to show at the stop: file and line (lib.rs opens it).
    pub show: Option<(PathBuf, usize)>,
    /// The game's time and frames as last seen.
    pub clock: (f64, u64),
    pub changed: bool,
}

/// What the debugger page asks Studio to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Start a session of a configuration.
    Start(Config),
    Stop,
    Restart,
    /// Open a file at a line (from 1).
    Open(PathBuf, usize),
    /// Save the running world as `tests/profiles/<name>.json` (its JSON).
    SaveProfile(PathBuf, String),
    /// Write `.wowlua/launch.json` with these configurations.
    SaveConfigs(Vec<Config>),
}

impl DebugState {
    /// The kept state of a workspace.
    pub fn restore(&mut self, kept: Kept) {
        self.kept = kept;
        if self.kept.stop_on.is_empty() {
            self.kept.stop_on = STOP_ON
                .iter()
                .filter(|(_, _, on)| *on)
                .map(|(k, _, _)| (*k).to_owned())
                .collect();
        }
        self.watch_values = vec![None; self.kept.watches.len()];
    }

    pub fn running(&self) -> bool {
        self.session.is_some()
    }

    /// A new session's state; the requests to ask before it runs (its
    /// breakpoints, each with its id).
    pub fn begin(
        &mut self,
        session: u64,
        config: Config,
        workspace: &Workspace,
    ) -> Vec<(u64, GameRequest)> {
        self.session = Some(session);
        self.config = Some(config);
        self.stopped = None;
        self.frames.clear();
        self.frame = None;
        self.scopes.clear();
        self.vars.clear();
        self.ids.clear();
        self.states.clear();
        self.ended = None;
        self.stops = 0;
        self.logs = 0;
        self.errors = 0;
        self.console.clear();
        self.asked.clear();
        let mut setup = Vec::new();
        let files: Vec<PathBuf> = self.kept.lines.keys().cloned().collect();
        for file in files {
            if let Some(r) = self.lines_request(&file, workspace) {
                setup.push(r);
            }
        }
        let f = self.functions_request();
        setup.push(f);
        let id = self.id(Asked::Plain);
        setup.push((
            id,
            GameRequest::SetStopOn(self.kept.stop_on.iter().cloned().collect()),
        ));
        setup
    }

    fn id(&mut self, asked: Asked) -> u64 {
        self.next += 1;
        let id = IDS + self.next;
        self.asked.insert(id, asked);
        id
    }

    fn lines_request(&mut self, file: &Path, workspace: &Workspace) -> Option<(u64, GameRequest)> {
        let lines = self.kept.lines.get(file).cloned().unwrap_or_default();
        let source = crate::tests::rel_of(workspace, file);
        let id = self.id(Asked::Lines(
            file.to_path_buf(),
            lines.iter().map(|b| b.line).collect(),
        ));
        Some((id, GameRequest::SetBreakpoints { source, lines }))
    }

    fn functions_request(&mut self) -> (u64, GameRequest) {
        let id = self.id(Asked::Functions);
        (
            id,
            GameRequest::SetFunctionBreakpoints(self.kept.functions.clone()),
        )
    }

    fn ask(&mut self, system: &dyn System, asked: Asked, request: GameRequest) {
        if self.session.is_none() {
            return;
        }
        let id = self.id(asked);
        system.game_request(id, request);
    }

    /// The request to send: a step, the clock, the world (the bar's).
    pub fn send(&mut self, system: &dyn System, request: GameRequest) {
        self.ask(system, Asked::Plain, request);
    }

    /// Sets or removes a line's breakpoint; a running session hears it.
    pub fn toggle(&mut self, system: &dyn System, workspace: &Workspace, file: &Path, line: u32) {
        let list = self.kept.lines.entry(file.to_path_buf()).or_default();
        if let Some(i) = list.iter().position(|b| b.line == line) {
            list.remove(i);
        } else {
            list.push(LineBp {
                line,
                ..LineBp::default()
            });
            list.sort_by_key(|b| b.line);
        }
        if list.is_empty() {
            self.kept.lines.remove(file);
        }
        self.changed = true;
        self.sync_file(system, workspace, file);
    }

    /// A line breakpoint changed (the editor dialog's).
    pub fn set_line(
        &mut self,
        system: &dyn System,
        workspace: &Workspace,
        file: &Path,
        bp: LineBp,
    ) {
        let list = self.kept.lines.entry(file.to_path_buf()).or_default();
        match list.iter_mut().find(|b| b.line == bp.line) {
            Some(b) => *b = bp,
            None => {
                list.push(bp);
                list.sort_by_key(|b| b.line);
            }
        }
        self.changed = true;
        self.sync_file(system, workspace, file);
    }

    fn sync_file(&mut self, system: &dyn System, workspace: &Workspace, file: &Path) {
        if self.session.is_none() {
            return;
        }
        if let Some((id, r)) = self.lines_request(file, workspace) {
            system.game_request(id, r);
        }
    }

    fn sync_functions(&mut self, system: &dyn System) {
        self.changed = true;
        if self.session.is_some() {
            let (id, r) = self.functions_request();
            system.game_request(id, r);
        }
    }

    /// Function breakpoints added (the inspector's "break on its
    /// scripts").
    pub fn add_functions(&mut self, system: &dyn System, names: Vec<String>) {
        for name in names {
            if !self.kept.functions.iter().any(|f| f.name == name) {
                self.kept.functions.push(FunctionBp {
                    name,
                    condition: None,
                });
            }
        }
        self.sync_functions(system);
    }

    /// The breakpoints of a file, as the editor's gutter draws them.
    pub fn marks(&self, file: &Path) -> Vec<BpMark> {
        let Some(list) = self.kept.lines.get(file) else {
            return Vec::new();
        };
        list.iter()
            .map(|b| {
                let state = self.ids.iter().find_map(|(id, (f, l))| {
                    (f.as_deref() == Some(file) && *l == b.line)
                        .then(|| self.states.get(id))
                        .flatten()
                });
                BpMark {
                    line: b.line.saturating_sub(1) as usize,
                    // (without a session: as set)
                    verified: self.session.is_none() || state.is_some_and(|s| s.0),
                    kind: if b.log_message.is_some() {
                        BpKind::Log
                    } else if b.condition.is_some() || b.hit_condition.is_some() {
                        BpKind::Conditional
                    } else {
                        BpKind::Plain
                    },
                }
            })
            .collect()
    }

    /// The line (from 0) of `file` the selected frame of the stop is at.
    pub fn stopped_line(&self, file: &Path, workspace: &Workspace) -> Option<usize> {
        self.stopped.as_ref()?;
        let f = self.selected_frame()?;
        let path = frame_path(f, workspace)?;
        (path == file).then(|| f["line"].as_u64().unwrap_or(1).saturating_sub(1) as usize)
    }

    fn selected_frame(&self) -> Option<&Json> {
        let id = self.frame?;
        self.frames
            .iter()
            .find(|f| f["id"].as_u64() == Some(u64::from(id)))
    }

    /// A session's news (its replies have ids from [`IDS`]).
    pub fn event(
        &mut self,
        session: u64,
        event: &GameEvent,
        system: &dyn System,
        workspace: &Workspace,
    ) {
        if self.session != Some(session) {
            return;
        }
        match event {
            GameEvent::Started { .. } => {}
            GameEvent::Stopped {
                reason,
                text,
                detail,
                ..
            } => {
                self.stops += 1;
                if reason == "exception" {
                    self.errors += 1;
                }
                self.stopped = Some(Stop {
                    reason: reason.clone(),
                    text: text.clone(),
                    detail: detail.clone(),
                });
                self.frames.clear();
                self.vars.clear();
                self.ask(system, Asked::Stack, GameRequest::StackTrace);
            }
            GameEvent::Continued => {
                self.stopped = None;
                self.frames.clear();
                self.frame = None;
                self.scopes.clear();
                self.vars.clear();
            }
            GameEvent::Breakpoint(b) => self.state_of(b),
            GameEvent::Reloaded => {
                self.push_console("console", "the UI reloaded: /reload\n");
            }
            GameEvent::Output { category, text } => {
                if category == "console" && !text.starts_with('[') {
                    self.logs += 1;
                }
                if category == "stderr" {
                    self.errors += 1;
                }
                self.push_console(category, text);
            }
            GameEvent::Ended { error } => {
                let config = self
                    .config
                    .as_ref()
                    .map(|c| c.name.clone())
                    .unwrap_or_default();
                let result = match error {
                    Some(e) => format!("can't start: {e}"),
                    None => format!("{} stops", self.stops),
                };
                self.kept
                    .recent
                    .insert(0, (config.clone(), result, system.now()));
                self.kept.recent.truncate(8);
                self.changed = true;
                self.ended = Some(Ended {
                    config,
                    error: error.clone(),
                    time: self.clock.0,
                    frames: self.clock.1,
                    stops: self.stops,
                    logs: self.logs,
                    errors: self.errors,
                });
                self.session = None;
                self.stopped = None;
                self.frames.clear();
            }
            GameEvent::Reply { id, result } => {
                let Some(asked) = self.asked.remove(id) else {
                    return;
                };
                self.answer(asked, result.clone(), system, workspace);
            }
            GameEvent::Status(_) => {}
        }
    }

    fn push_console(&mut self, category: &str, text: &str) {
        if self.console.len() == CONSOLE_KEPT {
            self.console.pop_front();
        }
        self.console
            .push_back((category.to_owned(), text.trim_end_matches('\n').to_owned()));
    }

    fn state_of(&mut self, b: &Json) {
        let Some(id) = b["id"].as_u64() else { return };
        self.states.insert(
            id as u32,
            (
                b["verified"].as_bool().unwrap_or(false),
                b["line"].as_u64().map(|l| l as u32),
                b["hits"].as_u64().unwrap_or(0) as u32,
            ),
        );
    }

    fn answer(
        &mut self,
        asked: Asked,
        result: Result<Json, String>,
        system: &dyn System,
        workspace: &Workspace,
    ) {
        let data = match result {
            Ok(d) => d,
            Err(e) => {
                match asked {
                    Asked::Watch(i) => {
                        if let Some(v) = self.watch_values.get_mut(i) {
                            *v = Some(Err(e));
                        }
                    }
                    _ => self.push_console("stderr", &e),
                }
                return;
            }
        };
        match asked {
            Asked::Lines(file, lines) => {
                self.ids.retain(|_, (f, _)| f.as_deref() != Some(&file));
                for (b, line) in data["breakpoints"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .zip(lines)
                {
                    if let Some(id) = b["id"].as_u64() {
                        self.ids.insert(id as u32, (Some(file.clone()), line));
                        self.state_of(b);
                    }
                }
            }
            Asked::Functions => {
                self.ids.retain(|_, (f, _)| f.is_some());
                for (i, b) in data["breakpoints"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .enumerate()
                {
                    if let Some(id) = b["id"].as_u64() {
                        self.ids.insert(id as u32, (None, i as u32));
                        self.state_of(b);
                    }
                }
            }
            Asked::Hits => {
                for b in data["breakpoints"].as_array().into_iter().flatten() {
                    self.state_of(b);
                }
            }
            Asked::Stack => {
                self.frames = data["frames"].as_array().cloned().unwrap_or_default();
                // The innermost frame of the package's (else the first).
                let top = self
                    .frames
                    .iter()
                    .find(|f| f["source"].is_string())
                    .or_else(|| self.frames.first())
                    .cloned();
                if let Some(f) = top {
                    self.pick_frame(system, workspace, &f);
                }
            }
            Asked::Scopes(_) => {
                self.scopes = data["scopes"].as_array().cloned().unwrap_or_default();
                let refs: Vec<u32> = self
                    .scopes
                    .iter()
                    .filter(|s| !s["expensive"].as_bool().unwrap_or(false))
                    .filter_map(|s| s["reference"].as_u64().map(|r| r as u32))
                    .collect();
                for r in refs {
                    self.open.insert(r);
                    self.ask(system, Asked::Vars(r), GameRequest::Variables(r));
                }
            }
            Asked::Vars(r) => {
                self.vars
                    .insert(r, data["variables"].as_array().cloned().unwrap_or_default());
            }
            Asked::Watch(i) => {
                let text = value_text(&data["variable"]);
                if let Some(v) = self.watch_values.get_mut(i) {
                    *v = Some(Ok(text));
                }
            }
            Asked::Eval => {
                let text = value_text(&data["variable"]);
                self.push_console("result", &format!("= {text}"));
            }
            Asked::Units => self.units = data.as_array().cloned().unwrap_or_default(),
            Asked::SaveWorld(path) => {
                let text = serde_json::to_string_pretty(&data).unwrap_or_default();
                match system.write_file(&path, &(text + "\n")) {
                    Ok(()) => self.push_console("console", &format!("saved {}", path.display())),
                    Err(e) => self.push_console("stderr", &e),
                }
            }
            Asked::Plain => {}
        }
    }

    /// The frame shown: its source opened, its scopes and the watches read.
    fn pick_frame(&mut self, system: &dyn System, workspace: &Workspace, f: &Json) {
        let Some(id) = f["id"].as_u64().map(|i| i as u32) else {
            return;
        };
        self.frame = Some(id);
        self.scopes.clear();
        self.vars.clear();
        self.open.clear();
        if let Some(path) = frame_path(f, workspace) {
            self.show = Some((path, f["line"].as_u64().unwrap_or(1) as usize));
        }
        self.ask(system, Asked::Scopes(id), GameRequest::Scopes(id));
        self.read_watches(system);
    }

    fn read_watches(&mut self, system: &dyn System) {
        self.watch_values = vec![None; self.kept.watches.len()];
        let frame = self.frame;
        for (i, w) in self.kept.watches.clone().into_iter().enumerate() {
            self.ask(
                system,
                Asked::Watch(i),
                GameRequest::Evaluate {
                    expression: w,
                    frame,
                    watch: true,
                },
            );
        }
    }

    /// Asks what the shown panels show (every second while running).
    pub fn poll(&mut self, system: &dyn System, now: f64, units: bool) {
        if self.session.is_none() || now - self.polled < 1.0 {
            return;
        }
        self.polled = now;
        if !self.asked.values().any(|a| *a == Asked::Hits) {
            self.ask(system, Asked::Hits, GameRequest::Breakpoints);
        }
        if units && !self.asked.values().any(|a| *a == Asked::Units) {
            self.ask(system, Asked::Units, GameRequest::Units);
        }
    }
}

/// A frame's file in the workspace: the package's source, or a file on
/// disk (Blizzard's UI, another addon).
fn frame_path(f: &Json, workspace: &Workspace) -> Option<PathBuf> {
    if let Some(s) = f["source"].as_str() {
        return Some(workspace.root.join(s));
    }
    f["file"].as_str().map(PathBuf::from)
}

/// A variable's value as shown: its text, secret and taint marked.
fn value_text(v: &Json) -> String {
    let mut s = v["value"].as_str().unwrap_or("nil").to_owned();
    if v["secret"].as_bool() == Some(true) {
        s.push_str("  (secret)");
    }
    s
}

// --------------------------------------------------------------- the bar

/// The session's bar over the editor (the debugger board's): what runs,
/// continue or pause, steps, stop, restart, the clock, `/reload`; and the
/// stop's banner.
pub fn bar(ui: &mut Ui, state: &mut DebugState, system: &dyn System) -> Option<Action> {
    let p = palette(ui.ctx());
    let mut action = None;
    let Some(config) = state.config.clone().filter(|_| state.running()) else {
        return None;
    };
    let stopped = state.stopped.is_some();
    forms::padded(ui, 10, 6, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if stopped {
                if widgets::button(ui, ButtonKind::Primary, "continue", true, None).clicked() {
                    state.send(system, GameRequest::Continue);
                }
            } else if widgets::button(ui, ButtonKind::Secondary, "pause", true, None).clicked() {
                state.send(system, GameRequest::Pause);
            }
            ui.add_enabled_ui(stopped, |ui| {
                for (label, r) in [
                    ("step over", GameRequest::Next),
                    ("step into", GameRequest::StepIn),
                    ("step out", GameRequest::StepOut),
                ] {
                    if widgets::button(ui, ButtonKind::Ghost, label, true, None).clicked() {
                        state.send(system, r);
                    }
                }
            });
            if widgets::button(ui, ButtonKind::Ghost, "restart", true, None).clicked() {
                action = Some(Action::Restart);
            }
            if widgets::button(ui, ButtonKind::Ghost, "stop", true, None).clicked() {
                action = Some(Action::Stop);
            }
            if forms::text_button(ui, "/reload").clicked() {
                state.send(system, GameRequest::World("C_UI.Reload()".into()));
            }
        });
        // What runs; the clock at the right.
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.label(
                egui::RichText::new(describe(&config))
                    .font(Face::Plex.at(11.5))
                    .color(p.ink_muted),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{:.1} s · frame {}",
                        state.clock.0, state.clock.1
                    ))
                    .font(Face::Plex.at(11.5))
                    .color(p.ink_muted),
                );
                let manual = state.config.as_ref().is_some_and(|c| c.manual);
                if manual {
                    for (label, frames, seconds) in [
                        ("+1 s", None, Some(1.0)),
                        ("+100 ms", None, Some(0.1)),
                        ("+1 frame", Some(1), None),
                    ] {
                        if widgets::button(ui, ButtonKind::Ghost, label, true, None).clicked() {
                            state.send(system, GameRequest::Advance { frames, seconds });
                        }
                    }
                }
                if let Some(i) = widgets::seg(
                    ui,
                    "debug-clock",
                    &["realtime", "manual"],
                    usize::from(manual),
                    false,
                ) {
                    let manual = i == 1;
                    if let Some(c) = state.config.as_mut() {
                        c.manual = manual;
                    }
                    state.send(system, GameRequest::SetClock { manual });
                }
            });
        });
    });
    if let Some(stop) = state.stopped.clone() {
        banner(ui, &stop, &p);
    }
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::ZERO, p.line);
    action
}

/// What a configuration runs, as the bar says it.
pub fn describe(c: &Config) -> String {
    let mut parts = vec![match &c.test {
        Some(t) => format!("test {t}"),
        None => "the addon".to_owned(),
    }];
    if let Some(f) = &c.flavor {
        parts.push(f.clone());
    }
    if let Some(pr) = &c.profile {
        parts.push(format!("world {pr}"));
    }
    parts.push(if c.manual { "manual clock" } else { "realtime" }.to_owned());
    parts.join(" · ")
}

/// The stop's banner: why, the details, a hint.
fn banner(ui: &mut Ui, stop: &Stop, p: &Palette) {
    let (kind, word) = match stop.reason.as_str() {
        "exception" => (CalloutKind::Error, "stopped on an error"),
        "breakpoint" => (CalloutKind::Warning, "breakpoint"),
        "functionBreakpoint" => (CalloutKind::Warning, "function breakpoint"),
        "pause" => (CalloutKind::Note, "paused"),
        "entry" => (CalloutKind::Note, "stopped on entry"),
        _ => (CalloutKind::Note, "stepped"),
    };
    let detail = stop.detail.as_ref();
    let (text, more, help) = match detail.and_then(|d| d["kind"].as_str()) {
        Some("error") => {
            let d = detail.unwrap_or(&Json::Null);
            let caught = if d["caught"].as_bool() == Some(true) {
                "caught by a handler"
            } else {
                "uncaught"
            };
            (
                format!("{} ({caught})", d["message"].as_str().unwrap_or("")),
                d["stack"].as_str().unwrap_or("").to_owned(),
                None,
            )
        }
        Some("blocked") => {
            let d = detail.unwrap_or(&Json::Null);
            (
                format!(
                    "{} was blocked from {}",
                    d["addon"].as_str().unwrap_or("?"),
                    d["action"].as_str().unwrap_or("?")
                ),
                d["report"].as_str().unwrap_or("").to_owned(),
                Some(
                    "protected actions need untainted code: in combat, only Blizzard's secure \
                     code may run them. The chain shows where the taint came from.",
                ),
            )
        }
        Some("taint") => {
            let d = detail.unwrap_or(&Json::Null);
            (
                format!("secure code tainted by {}", d["by"].as_str().unwrap_or("?")),
                d["report"].as_str().unwrap_or("").to_owned(),
                Some(
                    "change the world, not Blizzard's globals: a replaced global taints the \
                     secure code that reads it.",
                ),
            )
        }
        _ => (stop.text.clone().unwrap_or_default(), String::new(), None),
    };
    forms::padded(ui, 10, 4, |ui| {
        forms::callout(ui, kind, |ui| {
            ui.label(
                egui::RichText::new(word)
                    .font(Face::Plex.at(12.0))
                    .strong()
                    .color(p.ink),
            );
            if !text.is_empty() {
                forms::paragraph(ui, &text, 12.0, 18.0, p.ink);
            }
            if !more.is_empty() {
                egui::ScrollArea::vertical()
                    .id_salt("debug-banner")
                    .max_height(90.0)
                    .show(ui, |ui| {
                        forms::paragraph(ui, &more, 11.5, 17.0, p.ink_muted);
                    });
            }
            if let Some(h) = help {
                forms::paragraph(ui, &format!("help: {h}"), 11.5, 17.0, p.ink_muted);
            }
        });
    });
}

// ----------------------------------------------------------- the call stack

/// The call stack panel; without a session, run and debug: the
/// configurations, the last session's end, the recent ones.
pub fn call_stack(
    ui: &mut Ui,
    state: &mut DebugState,
    workspace: &Workspace,
    system: &dyn System,
    status: Option<&str>,
) -> Option<Action> {
    let p = palette(ui.ctx());
    if !state.configs_read {
        state.configs_read = true;
        state.configs = read_configs(&workspace.root, system);
    }
    if !state.running() {
        return run_and_debug(ui, state, workspace, &p);
    }
    if let Some(s) = status {
        empty(ui, &format!("{s}…"), &p);
        return None;
    }
    if state.stopped.is_none() {
        empty(ui, "running · pause (f6) to see where", &p);
        return None;
    }
    let frames = state.frames.clone();
    let selected = state.frame;
    let mut picked = None;
    egui::ScrollArea::vertical()
        .id_salt("debug-stack")
        .auto_shrink(false)
        .show(ui, |ui| {
            for f in &frames {
                let (rect, resp) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::click());
                let id = f["id"].as_u64().map(|i| i as u32);
                if selected == id {
                    ui.painter()
                        .rect_filled(rect, CornerRadius::ZERO, p.amber_soft);
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_200);
                }
                let ours = f["source"].is_string();
                let dispatch = f["kind"].as_str() == Some("dispatch");
                let color = if ours {
                    p.ink
                } else if dispatch {
                    p.amber_ink
                } else {
                    p.ink_muted
                };
                ui.painter().text(
                    Pos2::new(rect.left() + 12.0, rect.top() + 10.0),
                    Align2::LEFT_CENTER,
                    f["name"].as_str().unwrap_or("?"),
                    Face::Plex.at(12.5),
                    color,
                );
                let where_ = match (
                    f["source"].as_str().or(f["chunk"].as_str()),
                    f["line"].as_u64(),
                ) {
                    (Some(s), Some(l)) => format!("{s}:{l}"),
                    _ => f["kind"].as_str().unwrap_or("").to_owned(),
                };
                ui.painter().text(
                    Pos2::new(rect.left() + 12.0, rect.top() + 25.0),
                    Align2::LEFT_CENTER,
                    where_,
                    Face::Plex.at(11.0),
                    p.ink_faint,
                );
                if resp.clicked() {
                    picked = Some(f.clone());
                }
            }
        });
    if let Some(f) = picked {
        state.pick_frame(system, workspace, &f);
    }
    None
}

fn run_and_debug(
    ui: &mut Ui,
    state: &mut DebugState,
    workspace: &Workspace,
    p: &Palette,
) -> Option<Action> {
    let mut action = None;
    let mut configs = state.configs.clone();
    if configs.is_empty() {
        configs.push(Config::addon(&format!("{} · the addon", workspace.name)));
    }
    let chosen = state
        .kept
        .chosen
        .as_ref()
        .and_then(|n| configs.iter().position(|c| &c.name == n))
        .unwrap_or(0);
    egui::ScrollArea::vertical()
        .id_salt("debug-run")
        .auto_shrink(false)
        .show(ui, |ui| {
            forms::padded(ui, 12, 10, |ui| {
                ui.label(
                    egui::RichText::new("configurations")
                        .font(Face::Plex.at(11.0))
                        .color(p.ink_faint),
                );
                ui.add_space(4.0);
                for (i, c) in configs.iter().enumerate() {
                    let (rect, resp) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 36.0), Sense::click());
                    if i == chosen {
                        ui.painter()
                            .rect_filled(rect, CornerRadius::ZERO, p.amber_soft);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_200);
                    }
                    ui.painter().text(
                        Pos2::new(rect.left() + 8.0, rect.top() + 11.0),
                        Align2::LEFT_CENTER,
                        &c.name,
                        Face::Plex.at(12.5),
                        p.ink,
                    );
                    let sub = if c.vscode {
                        format!("{} · from VS Code", describe(c))
                    } else {
                        describe(c)
                    };
                    ui.painter().text(
                        Pos2::new(rect.left() + 8.0, rect.top() + 26.0),
                        Align2::LEFT_CENTER,
                        sub,
                        Face::Plex.at(11.0),
                        p.ink_muted,
                    );
                    if resp.clicked() {
                        state.kept.chosen = Some(c.name.clone());
                        state.changed = true;
                    }
                    if resp.double_clicked() {
                        state.editing =
                            Some((Some(i).filter(|_| !state.configs.is_empty()), c.clone()));
                    }
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if widgets::button(ui, ButtonKind::Primary, "start", true, None).clicked() {
                        action = Some(Action::Start(configs[chosen].clone()));
                    }
                    ui.label(
                        egui::RichText::new("f5")
                            .font(Face::Plex.at(11.0))
                            .color(p.ink_faint),
                    );
                    if forms::text_button(ui, "edit").clicked() {
                        let at = Some(chosen).filter(|_| !state.configs.is_empty());
                        state.editing = Some((at, configs[chosen].clone()));
                    }
                    if forms::text_button(ui, "new").clicked() {
                        state.editing = Some((None, Config::addon("a new configuration")));
                    }
                });
                if let Some(e) = &state.ended {
                    ui.add_space(12.0);
                    match &e.error {
                        Some(err) => {
                            forms::callout(ui, CalloutKind::Error, |ui| {
                                forms::paragraph(
                                    ui,
                                    &format!("error: the session can't start: {err}"),
                                    12.0,
                                    18.0,
                                    p.ink,
                                );
                            });
                        }
                        None => {
                            forms::callout(ui, CalloutKind::Plain, |ui| {
                                ui.label(
                                    egui::RichText::new(format!("Session ended · {}", e.config))
                                        .font(Face::Plex.at(12.5))
                                        .color(p.ink),
                                );
                                forms::key_values(
                                    ui,
                                    &[
                                        (
                                            "game time",
                                            format!("{:.1} s · {} frames", e.time, e.frames),
                                        ),
                                        ("stops", e.stops.to_string()),
                                        ("log points", format!("{} lines", e.logs)),
                                        ("errors", e.errors.to_string()),
                                    ],
                                );
                            });
                        }
                    }
                }
                if !state.kept.recent.is_empty() {
                    ui.add_space(12.0);
                    ui.label(
                        egui::RichText::new("recent sessions")
                            .font(Face::Plex.at(11.0))
                            .color(p.ink_faint),
                    );
                    for (name, result, _) in &state.kept.recent {
                        ui.label(
                            egui::RichText::new(format!("{name} · {result}"))
                                .font(Face::Plex.at(12.0))
                                .color(p.ink_muted),
                        );
                    }
                }
                ui.add_space(12.0);
                forms::paragraph(
                    ui,
                    "Start a session from a configuration, a test's lens (debug test), or the \
                     run menu (f5). One session at a time: the game page shows it too.",
                    11.5,
                    17.0,
                    p.ink_faint,
                );
            });
        });
    action
}

// ------------------------------------------------------------ breakpoints

/// The breakpoints panel: every line and function breakpoint, with its
/// hits; what else stops the session.
pub fn breakpoints(
    ui: &mut Ui,
    state: &mut DebugState,
    workspace: &Workspace,
    system: &dyn System,
) -> Option<Action> {
    let p = palette(ui.ctx());
    let mut action = None;
    let mut remove: Option<(PathBuf, u32)> = None;
    let mut remove_fn = None;
    egui::ScrollArea::vertical()
        .id_salt("debug-bps")
        .auto_shrink(false)
        .show(ui, |ui| {
            forms::padded(ui, 10, 8, |ui| {
                for (file, list) in &state.kept.lines {
                    let rel = crate::tests::rel_of(workspace, file);
                    for b in list {
                        let state_ = state.ids.iter().find_map(|(id, (f, l))| {
                            (f.as_ref() == Some(file) && *l == b.line)
                                .then(|| state.states.get(id))
                                .flatten()
                        });
                        let detail = b
                            .log_message
                            .as_ref()
                            .map(|m| format!("log: {m}"))
                            .or_else(|| b.condition.as_ref().map(|c| format!("if {c}")))
                            .or_else(|| b.hit_condition.as_ref().map(|h| format!("hit {h}")))
                            .unwrap_or_default();
                        let hits = state_.map_or(0, |s| s.2);
                        let unplaced = state.running() && !state_.is_some_and(|s| s.0);
                        ui.horizontal(|ui| {
                            let color = if unplaced { p.ink_faint } else { p.error };
                            let (r, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                            ui.painter().circle_filled(r.center(), 4.0, color);
                            let text = format!("{rel}:{}  {detail}", b.line);
                            if ui
                                .add(
                                    egui::Label::new(
                                        egui::RichText::new(text)
                                            .font(Face::Plex.at(12.0))
                                            .color(p.ink),
                                    )
                                    .truncate()
                                    .sense(Sense::click()),
                                )
                                .clicked()
                            {
                                action = Some(Action::Open(file.clone(), b.line as usize));
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if forms::text_button(ui, "×").clicked() {
                                        remove = Some((file.clone(), b.line));
                                    }
                                    if hits > 0 {
                                        ui.label(
                                            egui::RichText::new(format!("{hits} hits"))
                                                .font(Face::Plex.at(11.0))
                                                .color(p.ink_muted),
                                        );
                                    }
                                },
                            );
                        });
                        if unplaced {
                            ui.label(
                                egui::RichText::new(
                                    "   not loaded yet, or no code at or after this line",
                                )
                                .font(Face::Plex.at(11.0))
                                .color(p.ink_faint),
                            );
                        }
                    }
                }
                for (i, f) in state.kept.functions.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                        ui.painter().circle_filled(r.center(), 4.0, p.warning);
                        ui.label(
                            egui::RichText::new(&f.name)
                                .font(Face::Plex.at(12.0))
                                .color(p.ink),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if forms::text_button(ui, "×").clicked() {
                                remove_fn = Some(i);
                            }
                        });
                    });
                }
                if state.kept.lines.is_empty() && state.kept.functions.is_empty() {
                    forms::paragraph(
                        ui,
                        "Click left of a line number to set a breakpoint; right-click it for a \
                         condition, a hit count or a log message.",
                        11.5,
                        17.0,
                        p.ink_faint,
                    );
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let r = forms::text_input(
                        ui,
                        &mut state.function,
                        (ui.available_width() - 50.0).max(80.0),
                        "a function, event:NAME, script:OnClick, timer",
                    );
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    if (forms::text_button(ui, "add").clicked() || enter)
                        && !state.function.trim().is_empty()
                    {
                        let name = state.function.trim().to_owned();
                        state.function.clear();
                        state.add_functions(system, vec![name]);
                    }
                });
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new("stop on")
                        .font(Face::Plex.at(11.0))
                        .color(p.ink_faint),
                );
                let mut changed = false;
                for (key, label, _) in STOP_ON {
                    let mut on = state.kept.stop_on.contains(key);
                    if forms::checkbox(ui, &mut on, label, None) {
                        changed = true;
                        if on {
                            state.kept.stop_on.insert(key.to_owned());
                        } else {
                            state.kept.stop_on.remove(key);
                        }
                    }
                }
                if changed {
                    state.changed = true;
                    let filters = state.kept.stop_on.iter().cloned().collect();
                    state.send(system, GameRequest::SetStopOn(filters));
                }
            });
        });
    if let Some((file, line)) = remove {
        state.toggle(system, workspace, &file, line);
    }
    if let Some(i) = remove_fn {
        state.kept.functions.remove(i);
        state.sync_functions(system);
    }
    action
}

// ---------------------------------------------------- variables and watches

/// The selected frame's variables: its scopes, opened on a click.
pub fn variables(ui: &mut Ui, state: &mut DebugState, system: &dyn System) {
    let p = palette(ui.ctx());
    if state.stopped.is_none() {
        empty(ui, "The variables of the frame the session stopped in.", &p);
        return;
    }
    let scopes = state.scopes.clone();
    let mut toggle = None;
    egui::ScrollArea::vertical()
        .id_salt("debug-vars")
        .auto_shrink(false)
        .show(ui, |ui| {
            for s in &scopes {
                let r = s["reference"].as_u64().unwrap_or(0) as u32;
                let open = state.open.contains(&r);
                if row(
                    ui,
                    0,
                    open,
                    true,
                    s["name"].as_str().unwrap_or(""),
                    "",
                    "",
                    &p,
                )
                .clicked()
                {
                    toggle = Some(r);
                }
                if open {
                    var_rows(ui, state, r, 1, &mut toggle, &p);
                }
            }
        });
    if let Some(r) = toggle {
        if !state.open.remove(&r) {
            state.open.insert(r);
            if !state.vars.contains_key(&r) {
                state.ask(system, Asked::Vars(r), GameRequest::Variables(r));
            }
        }
    }
}

fn var_rows(
    ui: &mut Ui,
    state: &DebugState,
    r: u32,
    depth: usize,
    toggle: &mut Option<u32>,
    p: &Palette,
) {
    let Some(list) = state.vars.get(&r) else {
        return;
    };
    for v in list {
        let child = v["reference"].as_u64().unwrap_or(0) as u32;
        let open = child != 0 && state.open.contains(&child);
        let mut meta = v["type"].as_str().unwrap_or("").to_owned();
        if let Some(t) = v["taint"]
            .as_str()
            .filter(|t| *t != "secure" && !t.is_empty())
        {
            meta.push_str(&format!(" · {t}"));
        }
        if row(
            ui,
            depth,
            open,
            child != 0,
            v["name"].as_str().unwrap_or(""),
            &value_text(v),
            &meta,
            p,
        )
        .clicked()
            && child != 0
        {
            *toggle = Some(child);
        }
        if open && depth < 12 {
            var_rows(ui, state, child, depth + 1, toggle, p);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn row(
    ui: &mut Ui,
    depth: usize,
    open: bool,
    has_children: bool,
    name: &str,
    value: &str,
    meta: &str,
    p: &Palette,
) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_200);
    }
    let x = rect.left() + 10.0 + depth as f32 * 14.0;
    let y = rect.center().y;
    if has_children {
        ui.painter().text(
            Pos2::new(x, y),
            Align2::LEFT_CENTER,
            if open { "−" } else { "+" },
            Face::Plex.at(12.0),
            p.ink_muted,
        );
    }
    let painter = ui.painter().with_clip_rect(rect);
    let r = painter.text(
        Pos2::new(x + 14.0, y),
        Align2::LEFT_CENTER,
        name,
        Face::Plex.at(12.0),
        p.syn_function,
    );
    let r = if value.is_empty() {
        r
    } else {
        painter.text(
            Pos2::new(r.right() + 6.0, y),
            Align2::LEFT_CENTER,
            format!("= {value}"),
            Face::Plex.at(12.0),
            p.ink,
        )
    };
    if !meta.is_empty() {
        painter.text(
            Pos2::new(r.right() + 8.0, y),
            Align2::LEFT_CENTER,
            meta,
            Face::Plex.at(11.0),
            p.ink_faint,
        );
    }
    resp
}

/// The watches: expressions read in the selected frame at each stop.
pub fn watch(ui: &mut Ui, state: &mut DebugState, system: &dyn System) {
    let p = palette(ui.ctx());
    let mut remove = None;
    forms::padded(ui, 10, 8, |ui| {
        for (i, w) in state.kept.watches.iter().enumerate() {
            let value = match state.watch_values.get(i).cloned().flatten() {
                Some(Ok(v)) => v,
                Some(Err(e)) => e,
                None if state.stopped.is_some() => "…".into(),
                None => "—".into(),
            };
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(w)
                        .font(Face::Plex.at(12.0))
                        .color(p.syn_function),
                );
                ui.label(
                    egui::RichText::new(format!("= {value}"))
                        .font(Face::Plex.at(12.0))
                        .color(p.ink),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if forms::text_button(ui, "×").clicked() {
                        remove = Some(i);
                    }
                });
            });
        }
        ui.add_space(4.0);
        let mut text = std::mem::take(&mut state.watch_input);
        let r = forms::text_input(ui, &mut text, ui.available_width(), "add a watch");
        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !text.trim().is_empty() {
            state.kept.watches.push(text.trim().to_owned());
            state.watch_values.push(None);
            state.changed = true;
            text.clear();
            if state.stopped.is_some() {
                state.read_watches(system);
            }
        }
        state.watch_input = text;
        ui.add_space(4.0);
        forms::paragraph(
            ui,
            "Watches run in the frame selected at each stop. Note: a watch that calls a function \
             runs it, side effects too.",
            11.0,
            16.0,
            p.ink_faint,
        );
    });
    if let Some(i) = remove {
        state.kept.watches.remove(i);
        if i < state.watch_values.len() {
            state.watch_values.remove(i);
        }
        state.changed = true;
    }
}

// ------------------------------------------------------------- the console

/// The console: the session's output, and expressions evaluated in the
/// selected frame (or slash commands, `/…`).
pub fn console(ui: &mut Ui, state: &mut DebugState, system: &dyn System) {
    let p = palette(ui.ctx());
    let height = ui.available_height() - 40.0;
    let lines = &state.console;
    egui::ScrollArea::vertical()
        .id_salt("debug-console")
        .auto_shrink(false)
        .max_height(height.max(40.0))
        .stick_to_bottom(true)
        .show_rows(ui, 18.0, lines.len(), |ui, range| {
            for (category, text) in lines.range(range) {
                let color = match category.as_str() {
                    "stderr" => p.error,
                    "result" => p.syn_string,
                    "stdout" => p.ink,
                    _ => p.ink_muted,
                };
                let first = text.lines().next().unwrap_or("");
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(first)
                            .font(Face::Plex.at(12.0))
                            .color(color),
                    )
                    .truncate(),
                );
            }
        });
    forms::padded(ui, 8, 4, |ui| {
        let hint = if state.stopped.is_some() {
            "evaluate in the selected frame, or a slash command"
        } else {
            "evaluate in the running client, or a slash command"
        };
        let r = forms::text_input(ui, &mut state.input, ui.available_width(), hint);
        if r.has_focus() {
            let (up, down) =
                ui.input(|i| (i.key_pressed(Key::ArrowUp), i.key_pressed(Key::ArrowDown)));
            let n = state.kept.history.len();
            if up && state.back < n {
                state.back += 1;
                state.input = state.kept.history[n - state.back].clone();
            } else if down && state.back > 0 {
                state.back -= 1;
                state.input = if state.back == 0 {
                    String::new()
                } else {
                    state.kept.history[n - state.back].clone()
                };
            }
        }
        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            let text = state.input.trim().to_owned();
            state.input.clear();
            state.back = 0;
            if !text.is_empty() {
                state.kept.history.push(text.clone());
                if state.kept.history.len() > 200 {
                    state.kept.history.remove(0);
                }
                state.changed = true;
                state.push_console("input", &format!("> {text}"));
                if text.starts_with('/') {
                    state.send(
                        system,
                        GameRequest::Input(crate::system::GameInput::Slash(text)),
                    );
                } else {
                    let frame = state.stopped.as_ref().and(state.frame);
                    state.ask(
                        system,
                        Asked::Eval,
                        GameRequest::Evaluate {
                            expression: text,
                            frame,
                            watch: false,
                        },
                    );
                }
            }
            r.request_focus();
        }
    });
}

// ------------------------------------------------------------ the live world

/// The live world: the running world's units, Lua run in it with
/// `wowlua:mock`, events fired, the world saved as a profile.
pub fn live_world(
    ui: &mut Ui,
    state: &mut DebugState,
    workspace: &Workspace,
    system: &dyn System,
) -> Option<Action> {
    let p = palette(ui.ctx());
    if !state.running() {
        empty(
            ui,
            "No session: the live world is the running client's.",
            &p,
        );
        return None;
    }
    let now = ui.input(|i| i.time);
    state.poll(system, now, true);
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_secs(1));
    let action = None;
    egui::ScrollArea::vertical()
        .id_salt("debug-world")
        .auto_shrink(false)
        .show(ui, |ui| {
            forms::padded(ui, 10, 8, |ui| {
                let units = state.units.clone();
                for u in &units {
                    ui.horizontal(|ui| {
                        let threat = match &u["threat"] {
                            Json::Object(t) => format!(
                                "{} · {:.0}%",
                                t.get("status").and_then(Json::as_f64).unwrap_or(0.0),
                                t.get("percent").and_then(Json::as_f64).unwrap_or(0.0)
                            ),
                            _ => "—".into(),
                        };
                        let tokens: Vec<&str> = u["tokens"]
                            .as_array()
                            .map(|t| t.iter().filter_map(|v| v.as_str()).take(2).collect())
                            .unwrap_or_default();
                        ui.label(
                            egui::RichText::new(format!(
                                "{:<10} {}  {} · {}  {}  threat {threat}",
                                tokens.join(","),
                                u["name"].as_str().unwrap_or("?"),
                                u["class"].as_str().unwrap_or(""),
                                u["level"].as_f64().unwrap_or(0.0),
                                u["reaction"].as_str().unwrap_or(""),
                            ))
                            .font(Face::Plex.at(12.0))
                            .color(p.ink),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if forms::text_button(ui, "target").clicked() {
                                let i = u["id"].as_u64().map(|i| i as usize);
                                state.send(system, GameRequest::Target(i));
                            }
                        });
                    });
                }
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("run in the world · wowlua:mock")
                        .font(Face::Plex.at(11.0))
                        .color(p.ink_faint),
                );
                ui.horizontal(|ui| {
                    forms::text_input(
                        ui,
                        &mut state.world,
                        (ui.available_width() - 60.0).max(80.0),
                        "mock.player:setMoney(500)",
                    );
                    if forms::text_button(ui, "apply").clicked() && !state.world.trim().is_empty() {
                        let script = state.world.trim().to_owned();
                        state.push_console("input", &format!("world> {script}"));
                        state.send(system, GameRequest::World(script));
                    }
                });
                ui.label(
                    egui::RichText::new("fire an event")
                        .font(Face::Plex.at(11.0))
                        .color(p.ink_faint),
                );
                ui.horizontal(|ui| {
                    forms::text_input(ui, &mut state.event, 160.0, "UNIT_HEALTH");
                    forms::text_input(
                        ui,
                        &mut state.args,
                        (ui.available_width() - 50.0).max(60.0),
                        "\"player\"",
                    );
                    if forms::text_button(ui, "fire").clicked() && !state.event.trim().is_empty() {
                        let args = parse_args(&state.args);
                        let event = state.event.trim().to_owned();
                        state.send(system, GameRequest::FireEvent { event, args });
                    }
                });
                ui.label(
                    egui::RichText::new("save as a profile")
                        .font(Face::Plex.at(11.0))
                        .color(p.ink_faint),
                );
                ui.horizontal(|ui| {
                    forms::text_input(ui, &mut state.profile_name, 160.0, "raid-pull");
                    if forms::text_button(ui, "save").clicked()
                        && !state.profile_name.trim().is_empty()
                    {
                        let name = state.profile_name.trim().to_owned();
                        let path = workspace
                            .root
                            .join("tests")
                            .join("profiles")
                            .join(format!("{name}.json"));
                        state.ask(system, Asked::SaveWorld(path), GameRequest::SaveWorld);
                    }
                });
                forms::paragraph(
                    ui,
                    "The profile keeps the character, money, place, units and changed CVars; \
                     items, bags, spells, auras and saved variables are not written yet.",
                    11.0,
                    16.0,
                    p.ink_faint,
                );
            });
        });
    action
}

/// An event's arguments as typed: `"player", 2, true, nil`.
pub fn parse_args(text: &str) -> Vec<Json> {
    text.split(',')
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .map(|a| match a {
            "nil" => Json::Null,
            "true" => json!(true),
            "false" => json!(false),
            _ => a
                .parse::<f64>()
                .map(|n| json!(n))
                .unwrap_or_else(|_| json!(a.trim_matches('"').trim_matches('\''))),
        })
        .collect()
}

// --------------------------------------------------------------- dialogs

/// The configuration's and the line breakpoint's editors, over the page.
pub fn dialogs(
    ctx: &egui::Context,
    state: &mut DebugState,
    workspace: &Workspace,
    system: &dyn System,
) -> Option<Action> {
    let mut action = None;
    if let Some((at, mut c)) = state.editing.take() {
        let mut open = true;
        let mut done = None;
        egui::Window::new("configuration")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                forms::label(ui, "name");
                forms::text_input(ui, &mut c.name, 320.0, "SuuUI · addon");
                forms::label(ui, "run");
                let mut kind = usize::from(c.test.is_some());
                if let Some(k) =
                    widgets::seg(ui, "cfg-run", &["the addon", "a test file"], kind, false)
                {
                    kind = k;
                    c.test = (k == 1).then(String::new);
                }
                if kind == 1 {
                    let mut t = c.test.clone().unwrap_or_default();
                    forms::text_input(ui, &mut t, 320.0, "tests/unit/x.test.wowlua");
                    c.test = Some(t);
                    let mut f = c.filters.join(", ");
                    forms::label(ui, "only tests whose names contain");
                    forms::text_input(ui, &mut f, 320.0, "secret");
                    c.filters = f
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .collect();
                }
                let mut flavor = c.flavor.clone().unwrap_or_default();
                forms::label(ui, "flavor (empty: the package's first)");
                forms::text_input(ui, &mut flavor, 320.0, "camelot");
                c.flavor = Some(flavor.trim().to_owned()).filter(|f| !f.is_empty());
                let mut profile = c.profile.clone().unwrap_or_default();
                forms::label(ui, "world profile (empty: test.profile)");
                forms::text_input(ui, &mut profile, 320.0, "raid-pull");
                c.profile = Some(profile.trim().to_owned()).filter(|f| !f.is_empty());
                forms::checkbox(
                    ui,
                    &mut c.manual,
                    "manual clock",
                    Some("time moves only when you advance it"),
                );
                forms::checkbox(
                    ui,
                    &mut c.stop_on_entry,
                    "stop on entry",
                    Some("pause at the package's first line"),
                );
                forms::checkbox(
                    ui,
                    &mut c.debug_blizzard_ui,
                    "debug Blizzard's UI",
                    Some("step into and stop in Blizzard's own code"),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if widgets::button(ui, ButtonKind::Primary, "save", true, None).clicked() {
                        done = Some(true);
                    }
                    if widgets::button(ui, ButtonKind::Ghost, "cancel", true, None).clicked() {
                        done = Some(false);
                    }
                    if at.is_some()
                        && widgets::button(ui, ButtonKind::Ghost, "remove", true, None).clicked()
                    {
                        done = Some(false);
                        if let Some(i) = at {
                            let mut list: Vec<Config> = state
                                .configs
                                .iter()
                                .filter(|c| !c.vscode)
                                .cloned()
                                .collect();
                            if i < list.len() {
                                list.remove(i);
                            }
                            state.configs = list.clone();
                            action = Some(Action::SaveConfigs(list));
                        }
                    }
                });
                forms::paragraph(
                    ui,
                    "Saved to .wowlua/launch.json (the launch options VS Code's wowlua \
                     debugger takes).",
                    11.0,
                    16.0,
                    palette(ui.ctx()).ink_faint,
                );
            });
        match done {
            Some(true) => {
                let mut list: Vec<Config> = state
                    .configs
                    .iter()
                    .filter(|c| !c.vscode)
                    .cloned()
                    .collect();
                c.vscode = false;
                match at.filter(|i| *i < list.len()) {
                    Some(i) => list[i] = c.clone(),
                    None => list.push(c.clone()),
                }
                state.kept.chosen = Some(c.name.clone());
                state.changed = true;
                let vscode: Vec<Config> =
                    state.configs.iter().filter(|c| c.vscode).cloned().collect();
                state.configs = list.iter().cloned().chain(vscode).collect();
                action = Some(Action::SaveConfigs(list));
            }
            Some(false) => {}
            None if open => state.editing = Some((at, c)),
            None => {}
        }
    }
    if let Some((file, line, mut bp)) = state.editing_bp.take() {
        let mut open = true;
        let mut done = None;
        egui::Window::new(format!("breakpoint · line {line}"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                let mut cond = bp.condition.clone().unwrap_or_default();
                forms::label(ui, "condition");
                forms::text_input(ui, &mut cond, 320.0, "s == 2");
                forms::help(ui, "Lua in the line's frame: stops when it is true.");
                let mut hit = bp.hit_condition.clone().unwrap_or_default();
                forms::label(ui, "hit count");
                forms::text_input(ui, &mut hit, 320.0, ">5, %10, 3");
                let mut log = bp.log_message.clone().unwrap_or_default();
                forms::label(ui, "log instead of stopping");
                forms::text_input(ui, &mut log, 320.0, "rows {#rows}");
                forms::help(ui, "{expressions} are evaluated; it goes to the console.");
                bp.condition = Some(cond.trim().to_owned()).filter(|s| !s.is_empty());
                bp.hit_condition = Some(hit.trim().to_owned()).filter(|s| !s.is_empty());
                bp.log_message = Some(log.trim().to_owned()).filter(|s| !s.is_empty());
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if widgets::button(ui, ButtonKind::Primary, "save", true, None).clicked() {
                        done = Some(true);
                    }
                    if widgets::button(ui, ButtonKind::Ghost, "cancel", true, None).clicked() {
                        done = Some(false);
                    }
                });
            });
        match done {
            Some(true) => state.set_line(system, workspace, &file, bp),
            Some(false) => {}
            None if open => state.editing_bp = Some((file, line, bp)),
            None => {}
        }
    }
    action
}

/// `.wowlua/launch.json`'s text for these configurations.
pub fn configs_json(list: &[Config]) -> String {
    let j = json!({
        "version": "0.2.0",
        "configurations": list.iter().map(Config::json).collect::<Vec<_>>(),
    });
    serde_json::to_string_pretty(&j).unwrap_or_default() + "\n"
}

fn empty(ui: &mut Ui, text: &str, p: &Palette) {
    forms::padded(ui, 12, 12, |ui| {
        forms::paragraph(ui, text, 12.5, 19.0, p.ink_muted);
    });
}
