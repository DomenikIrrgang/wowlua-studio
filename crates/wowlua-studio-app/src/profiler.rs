//! The profiler page (the profiler board, S057): a recording of the
//! running session, a test file's profile, a profile kept in the
//! workspace (`.wowlua/profiles`) or another speedscope file, and the
//! recordings the dev window exported in the game. Its game frames (a
//! game recording's seconds), its functions, the call tree and flame
//! graph of the frame picked (wowlua decision T495), its dispatches; a
//! game recording's slow calls and hints; another profile to compare
//! with.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use egui::{Align2, CornerRadius, Pos2, Rect, Sense, Ui, Vec2};
use serde_json::Value as Json;
use wowlua_studio_ui::forms;
use wowlua_studio_ui::widgets::{self, ButtonKind};
use wowlua_studio_ui::{Face, Palette, palette};

use crate::system::{
    GameEvent, GameRecordings, GameRequest, Profile, SavedProfile, System, Workspace,
};

/// The profiler's requests' ids: apart from the debugger's.
pub const IDS: u64 = 1 << 41;
/// A row, at the settings' density (S062).
fn row_h() -> f32 {
    wowlua_studio_ui::row(22.0)
}
const FLAME_ROW: f32 = 20.0;
/// A function's own share that marks a spike's cause in a frame's tree.
const BAR_H: f32 = 56.0;

/// What the tables, the tree and the flame graph measure.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Measure {
    #[default]
    Instructions,
    Bytes,
    Time,
    Calls,
}

impl Measure {
    /// The measures a profile has: wowlua's VM counts instructions; the
    /// game only times.
    fn all(game: bool) -> &'static [Measure] {
        if game {
            &[Measure::Time, Measure::Bytes, Measure::Calls]
        } else {
            &[
                Measure::Instructions,
                Measure::Bytes,
                Measure::Time,
                Measure::Calls,
            ]
        }
    }

    fn label(self, game: bool) -> &'static str {
        match (self, game) {
            (Measure::Instructions, _) => "instructions",
            (Measure::Bytes, true) => "garbage",
            (Measure::Bytes, false) => "bytes",
            (Measure::Time, true) => "time",
            (Measure::Time, false) => "host time",
            (Measure::Calls, _) => "calls",
        }
    }

    /// The cost's index (instructions, bytes, nanoseconds); none: calls.
    fn index(self) -> Option<usize> {
        match self {
            Measure::Instructions => Some(0),
            Measure::Bytes => Some(1),
            Measure::Time => Some(2),
            Measure::Calls => None,
        }
    }
}

/// A view of the profile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum View {
    #[default]
    Functions,
    Tree,
    Flame,
    Dispatches,
    Slow,
    Hints,
}

impl View {
    fn all(game: bool) -> &'static [View] {
        if game {
            &[
                View::Functions,
                View::Tree,
                View::Flame,
                View::Dispatches,
                View::Slow,
                View::Hints,
            ]
        } else {
            &[View::Functions, View::Tree, View::Flame, View::Dispatches]
        }
    }

    fn label(self, game: bool) -> &'static str {
        match self {
            View::Functions => "functions",
            View::Tree => "call tree",
            View::Flame => "flame",
            View::Dispatches if game => "by event",
            View::Dispatches => "dispatches",
            View::Slow => "slow",
            View::Hints => "hints",
        }
    }
}

/// A function or a dispatch.
#[derive(Clone, Debug, Default, PartialEq)]
struct Key {
    name: String,
    /// Its source (from the package's root) and the line it is defined on.
    file: Option<String>,
    line: u32,
    /// A dispatch's kind.
    kind: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Node {
    key: u32,
    parent: u32,
    calls: u64,
    /// Own cost: instructions, bytes, nanoseconds.
    cost: [u64; 3],
}

/// A function's or dispatch's sums.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Flat {
    key: u32,
    calls: u64,
    own: [u64; 3],
    total: [u64; 3],
    /// Its slowest call (a game recording's), nanoseconds.
    max: u64,
}

/// A game frame (a game recording's second).
#[derive(Clone, Debug, Default, PartialEq)]
struct Bar {
    label: String,
    /// Its cost (a second's: the slowest frame's time, the garbage), and
    /// its calls (a second's: frames).
    cost: [u64; 3],
    calls: u64,
    /// A second's frames over the budget.
    over: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Dispatch {
    name: String,
    kind: String,
    times: u64,
    total: [u64; 3],
    /// Its costliest run.
    worst: [u64; 3],
}

/// An event, a script, a timer or a slash command a game recording
/// watched: how often, how long in all and at worst, the frames over the
/// budget it was in.
#[derive(Clone, Debug, Default, PartialEq)]
struct Watched {
    kind: &'static str,
    name: String,
    count: u64,
    total: u64,
    worst: u64,
    over: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Slow {
    ms: f64,
    name: String,
    stack: String,
    at: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Hint {
    code: String,
    subject: String,
    evidence: String,
    fix: String,
}

/// A profile as the page shows it: wowlua's (a session's, a test file's,
/// a file's) or a game recording.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
    pub name: String,
    /// What it is, under the bar.
    pub note: String,
    /// A game recording: time and garbage, per second.
    pub game: bool,
    /// Its file, to export.
    pub path: Option<PathBuf>,
    keys: Vec<Key>,
    /// The call tree, the root first.
    nodes: Vec<Node>,
    children: Vec<Vec<u32>>,
    totals: Vec<[u64; 3]>,
    flat: Vec<Flat>,
    bars: Vec<Bar>,
    /// Each bar's tree (a session's newest frames): node, calls, cost.
    trees: Vec<Vec<(u32, u64, [u64; 3])>>,
    dispatches: Vec<Dispatch>,
    slow: Vec<Slow>,
    hints: Vec<Hint>,
    /// The frame budget (a game recording's), nanoseconds.
    budget: u64,
    /// A game recording's events, scripts, timers and commands.
    watched: Vec<Watched>,
    /// A game recording's memory: seconds from its start, kilobytes.
    memory: Vec<(f64, f64)>,
    /// Functions named by their sources' outlines (S057), by key.
    names: HashMap<u32, String>,
    /// The sources asked for names.
    named: bool,
}

fn add(to: &mut [u64; 3], c: [u64; 3]) {
    for i in 0..3 {
        to[i] += c[i];
    }
}

impl Model {
    /// wowlua's profile.
    pub fn of_profile(p: &Profile, path: Option<PathBuf>) -> Model {
        let keys: Vec<Key> = p
            .keys
            .iter()
            .map(|k| Key {
                name: k.name.clone(),
                file: k.file.clone(),
                line: k.line,
                kind: k.kind.clone(),
            })
            .collect();
        let nodes: Vec<Node> = p
            .nodes
            .iter()
            .map(|n| Node {
                key: n.key,
                parent: n.parent,
                calls: n.calls,
                cost: n.cost,
            })
            .collect();
        let bars = p
            .frames
            .iter()
            .map(|f| Bar {
                label: f.name.clone(),
                cost: f.cost,
                calls: f.nodes.iter().map(|n| n.1).sum(),
                over: 0,
            })
            .collect();
        let trees = p.frames.iter().map(|f| f.nodes.clone()).collect();
        // A kept profile by its file's name (`main 2026-10-28 10.20.33`).
        let name = path
            .as_ref()
            .and_then(|p| p.file_name()?.to_str()?.strip_suffix(".speedscope.json"))
            .map_or_else(|| p.name.clone(), str::to_owned);
        let mut m = Model {
            name,
            note: String::new(),
            game: false,
            path,
            keys,
            nodes,
            bars,
            trees,
            ..Model::default()
        };
        m.finish();
        m.dispatches = m.dispatches_of_tree();
        m.note = format!(
            "{} · {} functions · {} game frames · the counts come from the VM, the same on any \
             machine; host time is this one's",
            m.name,
            m.flat
                .iter()
                .filter(|f| m.keys[f.key as usize].kind.is_none())
                .count(),
            m.bars.len()
        );
        m
    }

    /// A recording the dev window exported in the game (wowlua's export
    /// format), from `client`'s `account`.
    pub fn of_game(rec: &Json, client: &str, account: &str) -> Model {
        let ms = |v: &Json| (v.as_f64().unwrap_or(0.0) * 1e6) as u64;
        let kb = |v: &Json| (v.as_f64().unwrap_or(0.0) * 1024.0) as u64;
        let num = |v: &Json| v.as_f64().unwrap_or(0.0) as u64;
        let mut m = Model {
            name: rec["name"].as_str().unwrap_or("recording").to_owned(),
            game: true,
            budget: ms(&rec["frameBudget"]).max(1),
            ..Model::default()
        };
        m.keys.push(Key {
            name: "(root)".into(),
            ..Key::default()
        });
        m.nodes.push(Node::default());
        let mut key_of: HashMap<String, u32> = HashMap::new();
        let mut key = |m: &mut Model, name: &str, place: Option<&str>, kind: Option<&str>| -> u32 {
            if let Some(&k) = key_of.get(name) {
                return k;
            }
            let (file, line) = place
                .and_then(|p| p.rsplit_once(':'))
                .and_then(|(f, l)| Some((f.to_owned(), l.parse().ok()?)))
                .map_or((None, 0), |(f, l)| (Some(f), l));
            m.keys.push(Key {
                name: name.to_owned(),
                file,
                line,
                kind: kind.map(str::to_owned),
            });
            let k = (m.keys.len() - 1) as u32;
            key_of.insert(name.to_owned(), k);
            k
        };
        // The functions' sums, as the game counted them.
        let mut flat = Vec::new();
        for (name, s) in obj(&rec["functions"]) {
            let k = key(&mut m, name, s["location"].as_str(), None);
            flat.push(Flat {
                key: k,
                calls: num(&s["calls"]),
                own: [0, kb(&s["selfGarbage"]), ms(&s["self"])],
                total: [0, kb(&s["garbage"]), ms(&s["total"])],
                max: ms(&s["max"]),
            });
        }
        for (label, r) in obj(&rec["roots"]) {
            let kind = r["kind"].as_str().unwrap_or("other").to_owned();
            key(&mut m, label, None, Some(&kind));
            m.dispatches.push(Dispatch {
                name: label.clone(),
                kind,
                times: num(&r["calls"]),
                total: [0, 0, ms(&r["total"])],
                worst: [0, 0, ms(&r["max"])],
            });
        }
        // The call paths (with call paths on), else each function once.
        let paths = list(&rec["paths"]);
        let mut child: HashMap<(u32, u32), u32> = HashMap::new();
        if paths.is_empty() {
            for f in &flat {
                m.nodes.push(Node {
                    key: f.key,
                    parent: 0,
                    calls: f.calls,
                    cost: f.own,
                });
            }
        } else {
            for p in &paths {
                let mut at = 0u32;
                for name in list(&p["path"]) {
                    let name = name.as_str().unwrap_or("?");
                    let k = key(&mut m, name, None, None);
                    at = *child.entry((at, k)).or_insert_with(|| {
                        m.nodes.push(Node {
                            key: k,
                            parent: at,
                            ..Node::default()
                        });
                        (m.nodes.len() - 1) as u32
                    });
                }
                let n = &mut m.nodes[at as usize];
                n.calls += num(&p["calls"]);
                n.cost[2] += ms(&p["self"]);
            }
        }
        let s = &rec["seconds"];
        let (frames, max, over, garbage) = (
            list(&s["frames"]),
            list(&s["max"]),
            list(&s["over"]),
            list(&s["garbage"]),
        );
        for (i, f) in frames.iter().enumerate() {
            let get = |l: &[Json]| l.get(i).cloned().unwrap_or(Json::Null);
            m.bars.push(Bar {
                label: format!("{}:{:02}", i / 60, i % 60),
                cost: [0, kb(&get(&garbage)), ms(&get(&max))],
                calls: num(f),
                over: num(&get(&over)),
            });
        }
        for (field, kind) in [
            ("events", "event"),
            ("scripts", "script"),
            ("timers", "timer"),
            ("commands", "command"),
        ] {
            for (name, s) in obj(&rec[field]) {
                m.watched.push(Watched {
                    kind,
                    name: name.clone(),
                    count: num(&s["count"]),
                    total: ms(&s["total"]),
                    worst: ms(&s["max"]),
                    over: num(&s["over"]),
                });
            }
        }
        m.watched
            .sort_by(|a, b| b.total.cmp(&a.total).then(a.name.cmp(&b.name)));
        for s in list(&rec["memory"]) {
            m.memory.push((
                s["at"].as_f64().unwrap_or(0.0),
                s["kb"].as_f64().unwrap_or(0.0),
            ));
        }
        for s in list(&rec["slow"]) {
            m.slow.push(Slow {
                ms: s["ms"].as_f64().unwrap_or(0.0),
                name: s["name"].as_str().unwrap_or("?").to_owned(),
                stack: s["stack"]
                    .as_str()
                    .or_else(|| s["location"].as_str())
                    .unwrap_or("")
                    .to_owned(),
                at: s["at"].as_f64().unwrap_or(0.0),
            });
        }
        for h in list(&rec["hints"]) {
            m.hints.push(Hint {
                code: h["code"].as_str().unwrap_or("").to_owned(),
                subject: h["subject"].as_str().unwrap_or("").to_owned(),
                evidence: h["evidence"].as_str().unwrap_or("").to_owned(),
                fix: h["fix"].as_str().unwrap_or("").to_owned(),
            });
        }
        m.finish();
        // The game's own sums over the tree's.
        m.flat = flat;
        let detail = rec["detail"]
            .as_str()
            .map(|d| format!(" · {d}"))
            .unwrap_or_default();
        let duration = rec["duration"].as_f64().unwrap_or(0.0);
        m.note = format!(
            "{}{detail} · recorded in the game on {client} (account {account}){} · {} s · its \
             times are the game's, on that computer; instructions and bytes are only counted in \
             the emulated client, so this recording shows time, garbage and calls.{}",
            m.name,
            rec["build"]
                .as_str()
                .map(|b| format!(" with dev build {b}"))
                .unwrap_or_default(),
            duration.round(),
            if rec["callPaths"].as_bool() == Some(true) {
                " Call paths were on: the call tree and the flame graph have them."
            } else {
                " Call paths were off: the call tree is each function once."
            },
        );
        m
    }

    /// The children, the totals and the functions' sums (a recursive
    /// call's total counted once, at its outermost).
    fn finish(&mut self) {
        let n = self.nodes.len();
        self.children = vec![Vec::new(); n];
        for i in 1..n {
            let p = self.nodes[i].parent as usize;
            if p < n {
                self.children[p].push(i as u32);
            }
        }
        self.totals = self.nodes.iter().map(|n| n.cost).collect();
        for i in (1..n).rev() {
            let p = self.nodes[i].parent as usize;
            let t = self.totals[i];
            if p < n {
                add(&mut self.totals[p], t);
            }
        }
        let mut flat: Vec<Flat> = (0..self.keys.len())
            .map(|k| Flat {
                key: k as u32,
                ..Flat::default()
            })
            .collect();
        let mut path: Vec<u32> = Vec::new();
        let mut stack: Vec<(u32, bool)> = vec![(0, false)];
        while let Some((i, done)) = stack.pop() {
            let node = self.nodes[i as usize];
            if done {
                path.pop();
                continue;
            }
            if let Some(e) = flat.get_mut(node.key as usize) {
                e.calls += node.calls;
                add(&mut e.own, node.cost);
                if !path.contains(&node.key) {
                    add(&mut e.total, self.totals[i as usize]);
                }
            }
            path.push(node.key);
            stack.push((i, true));
            for &c in self.children[i as usize].iter().rev() {
                stack.push((c, false));
            }
        }
        flat.retain(|f| f.key != 0 && f.calls > 0);
        self.flat = flat;
    }

    /// A session's dispatches: each dispatch's node sums, its worst frame.
    fn dispatches_of_tree(&self) -> Vec<Dispatch> {
        let mut out: BTreeMap<u32, Dispatch> = BTreeMap::new();
        for f in &self.flat {
            let k = &self.keys[f.key as usize];
            let Some(kind) = &k.kind else { continue };
            if kind == "frame" {
                continue;
            }
            out.insert(
                f.key,
                Dispatch {
                    name: k.name.clone(),
                    kind: kind.clone(),
                    times: f.calls,
                    total: f.total,
                    worst: [0; 3],
                },
            );
        }
        for i in 0..self.trees.len() {
            for (node, total) in self.frame_totals(i) {
                let key = self.nodes[node as usize].key;
                if let Some(d) = out.get_mut(&key) {
                    for (w, t) in d.worst.iter_mut().zip(total) {
                        *w = (*w).max(t);
                    }
                }
            }
        }
        let mut list: Vec<Dispatch> = out.into_values().collect();
        list.sort_by(|a, b| {
            b.total[0]
                .cmp(&a.total[0])
                .then(b.total[2].cmp(&a.total[2]))
        });
        list
    }

    /// A kept frame's nodes with their totals in it.
    fn frame_totals(&self, frame: usize) -> Vec<(u32, [u64; 3])> {
        let Some(tree) = self.trees.get(frame) else {
            return Vec::new();
        };
        let mut out: Vec<(u32, [u64; 3])> = tree.iter().map(|n| (n.0, n.2)).collect();
        for i in (0..out.len()).rev() {
            let parent = self.nodes.get(out[i].0 as usize).map_or(0, |n| n.parent);
            if parent == out[i].0 {
                continue;
            }
            if let Ok(at) = out.binary_search_by_key(&parent, |e| e.0) {
                let t = out[i].1;
                add(&mut out[at].1, t);
            }
        }
        out
    }

    /// The sources of the package's functions, to name.
    fn sources(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .keys
            .iter()
            .filter(|k| k.kind.is_none() && k.line > 0)
            .filter_map(|k| k.file.clone())
            .filter(|f| !f.starts_with("Interface/") && !f.starts_with('['))
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// Names its functions: (source, line defined, name).
    fn name(&mut self, names: &[(String, u32, String)]) {
        for (i, k) in self.keys.iter().enumerate() {
            let Some(f) = &k.file else { continue };
            if let Some((_, _, n)) = names.iter().find(|(nf, nl, _)| nf == f && *nl == k.line) {
                self.names.insert(i as u32, n.clone());
            }
        }
    }

    /// A function's place, as the tables show it.
    fn place(&self, key: u32) -> String {
        let k = &self.keys[key as usize];
        match &k.file {
            Some(f) if k.line > 0 => format!("{f}:{}", k.line),
            Some(f) => f.clone(),
            None => k.kind.clone().unwrap_or_default(),
        }
    }

    /// A function's name, as the tables show it: its outline's, else its
    /// place (a profile names Lua functions by where they are defined).
    fn title(&self, key: u32) -> String {
        if let Some(n) = self.names.get(&key) {
            return n.clone();
        }
        let k = &self.keys[key as usize];
        k.name
            .strip_prefix("function ")
            .map(str::to_owned)
            .unwrap_or_else(|| k.name.clone())
    }

    /// Whether a function is the package's own code.
    fn own(&self, key: u32) -> bool {
        self.keys[key as usize].file.as_deref().is_some_and(|f| {
            !f.starts_with("Interface/") && !f.starts_with('[') && !f.contains("Blizzard_")
        })
    }

    fn value(&self, cost: [u64; 3], calls: u64, m: Measure) -> u64 {
        match m.index() {
            Some(i) => cost[i],
            None => calls,
        }
    }
}

fn obj(v: &Json) -> Vec<(&String, &Json)> {
    let mut out: Vec<(&String, &Json)> = v
        .as_object()
        .map(|o| o.iter().collect())
        .unwrap_or_default();
    out.sort_by(|a, b| a.0.cmp(b.0));
    out
}

/// A Lua array as the export writes it: an array, or numbered keys.
fn list(v: &Json) -> Vec<Json> {
    match v {
        Json::Array(a) => a.iter().filter(|v| !v.is_null()).cloned().collect(),
        Json::Object(o) => {
            let mut items: Vec<(usize, &Json)> = o
                .iter()
                .filter_map(|(k, v)| Some((k.parse().ok()?, v)))
                .collect();
            items.sort_by_key(|e| e.0);
            items.into_iter().map(|(_, v)| v.clone()).collect()
        }
        _ => Vec::new(),
    }
}

/// A measure's amount as the page writes it.
fn amount(v: u64, m: Measure) -> String {
    let v = v as f64;
    match m {
        Measure::Time => {
            let ms = v / 1e6;
            if ms >= 100.0 {
                format!("{ms:.0} ms")
            } else if ms >= 10.0 {
                format!("{ms:.1} ms")
            } else {
                format!("{ms:.2} ms")
            }
        }
        Measure::Bytes => {
            if v >= 1024.0 * 1024.0 {
                format!("{:.1} MB", v / 1024.0 / 1024.0)
            } else if v >= 1024.0 {
                format!("{:.0} KB", v / 1024.0)
            } else {
                format!("{v:.0} B")
            }
        }
        Measure::Instructions | Measure::Calls => {
            if v >= 1e9 {
                format!("{:.2} G", v / 1e9)
            } else if v >= 1e6 {
                format!("{:.2} M", v / 1e6)
            } else if v >= 1e4 {
                format!("{:.0} k", v / 1e3)
            } else {
                format!("{v:.0}")
            }
        }
    }
}

/// Where a profile comes from, in the picker.
#[derive(Clone, Debug, PartialEq)]
enum Pick {
    File(PathBuf),
    Game(usize, usize),
}

/// What a profile read is for.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Target {
    Current,
    Baseline,
}

/// The page's state.
#[derive(Default)]
pub struct ProfilerState {
    /// The session is being recorded.
    pub recording: bool,
    next: u64,
    stop_id: Option<u64>,
    /// A test run's first profile is shown when it comes.
    pub awaiting_test: bool,
    pub current: Option<Model>,
    current_pick: Option<Pick>,
    baseline: Option<Model>,
    baseline_pick: Option<Pick>,
    compare: bool,
    measure: Measure,
    view: View,
    frame: Option<usize>,
    selected: Option<u32>,
    open: HashSet<u32>,
    zoom: Option<u32>,
    saved: Vec<SavedProfile>,
    saved_read: bool,
    game: Vec<GameRecordings>,
    game_read: Option<Result<(), String>>,
    reading: Vec<(PathBuf, Target)>,
    error: Option<String>,
    /// The functions' order, by the measure (cached).
    order: Option<(Measure, Vec<usize>)>,
    /// The hints whose explanation is open, by code: the text.
    explained: HashMap<String, Option<String>>,
}

/// What the page asks of Studio.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Open a source (from the package's root) at a line (from 1).
    Open(String, u32),
    /// A function breakpoint (by definition, `src/x.wowlua:12`).
    BreakOn(String),
}

impl ProfilerState {
    fn send(&mut self, system: &dyn System, request: GameRequest) -> u64 {
        self.next += 1;
        let id = IDS + self.next;
        system.game_request(id, request);
        id
    }

    /// Reads the workspace's profiles and the game's recordings (the page
    /// shown, or after a change).
    pub fn refresh(&mut self, workspace: &Workspace, system: &dyn System) {
        self.saved = system.profiles(&workspace.root);
        self.saved_read = true;
        if self.game_read.is_none() {
            self.game_read = Some(Ok(()));
            system.game_recordings(&workspace.root);
        }
    }

    /// A session's news.
    pub fn event(&mut self, event: &GameEvent, workspace: &Workspace, system: &dyn System) {
        match event {
            GameEvent::Reply { id, result } if Some(*id) == self.stop_id => {
                self.stop_id = None;
                self.recording = false;
                match result {
                    Ok(data) => {
                        let name = system.branch(&workspace.root).map_or_else(
                            || workspace.name.clone(),
                            |b| format!("{b} {}", workspace.name),
                        );
                        match system.profile_keep(&workspace.root, &name, &data["speedscope"]) {
                            Ok((path, profile)) => {
                                self.show(
                                    Model::of_profile(&profile, Some(path.clone())),
                                    Some(Pick::File(path)),
                                );
                                self.saved = system.profiles(&workspace.root);
                            }
                            Err(e) => self.error = Some(e),
                        }
                    }
                    Err(e) => self.error = Some(e.clone()),
                }
            }
            GameEvent::Reply { id, result: Err(e) } if *id > IDS => {
                self.recording = false;
                self.error = Some(e.clone());
            }
            GameEvent::Ended { .. } => {
                self.recording = false;
                self.stop_id = None;
            }
            _ => {}
        }
    }

    /// A profile read.
    pub fn profile_read(&mut self, path: &Path, result: Result<Profile, String>) {
        let Some(at) = self.reading.iter().position(|(p, _)| p == path) else {
            return;
        };
        let (_, target) = self.reading.remove(at);
        match result {
            Ok(p) => {
                let model = Model::of_profile(&p, Some(path.to_path_buf()));
                match target {
                    Target::Current => self.show(model, Some(Pick::File(path.to_path_buf()))),
                    Target::Baseline => {
                        self.baseline = Some(model);
                        self.baseline_pick = Some(Pick::File(path.to_path_buf()));
                    }
                }
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// The game's recordings read.
    pub fn game_recordings(&mut self, result: Result<Vec<GameRecordings>, String>) {
        match result {
            Ok(list) => {
                self.game = list;
                self.game_read = Some(Ok(()));
            }
            Err(e) => {
                self.game.clear();
                self.game_read = Some(Err(e));
            }
        }
    }

    /// Functions named: the profiles shown take them.
    pub fn function_names(&mut self, names: Vec<(String, u32, String)>) {
        for m in [self.current.as_mut(), self.baseline.as_mut()]
            .into_iter()
            .flatten()
        {
            m.name(&names);
        }
    }

    /// A profile kept by a test run: shown if the page waits for one.
    pub fn test_profiled(&mut self, path: &Path, workspace: &Workspace, system: &dyn System) {
        self.saved = system.profiles(&workspace.root);
        if std::mem::take(&mut self.awaiting_test) {
            self.read(path, Target::Current, system);
        }
    }

    /// Starts recording the running session.
    pub fn record(&mut self, system: &dyn System) {
        self.send(system, GameRequest::ProfileStart);
        self.recording = true;
        self.error = None;
    }

    /// Stops recording: the profile is kept and shown.
    pub fn stop(&mut self, system: &dyn System) {
        if self.recording && self.stop_id.is_none() {
            self.stop_id = Some(self.send(system, GameRequest::ProfileStop));
        }
    }

    /// Picks a function by its rank in the functions' table.
    pub fn select_rank(&mut self, rank: usize) {
        let order = self.order();
        if let (Some(m), Some(&i)) = (&self.current, order.get(rank)) {
            self.selected = Some(m.flat[i].key);
        }
    }

    /// Shows a profile (the frame budget's, the screenshots').
    pub fn open_profile(&mut self, profile: &Profile, path: Option<PathBuf>) {
        let pick = path.clone().map(Pick::File);
        self.show(Model::of_profile(profile, path), pick);
    }

    /// Shows a view, a frame picked (none: all).
    pub fn set_view(&mut self, view: View, frame: Option<usize>) {
        self.view = view;
        self.frame = frame;
    }

    /// Reads a profile from a file and shows it (a screenshot's step).
    pub fn open_file(&mut self, path: &Path, system: &dyn System) {
        self.read(path, Target::Current, system);
    }

    fn read(&mut self, path: &Path, target: Target, system: &dyn System) {
        self.reading.push((path.to_path_buf(), target));
        system.profile_read(path);
    }

    fn show(&mut self, model: Model, pick: Option<Pick>) {
        let game = model.game;
        if !Measure::all(game).contains(&self.measure) {
            self.measure = Measure::all(game)[0];
        }
        if !View::all(game).contains(&self.view) {
            self.view = View::Functions;
        }
        if self.baseline.as_ref().is_some_and(|b| b.game != game) {
            self.baseline = None;
            self.baseline_pick = None;
        }
        self.frame = None;
        self.zoom = None;
        self.selected = None;
        self.order = None;
        // The hottest paths open.
        self.open.clear();
        let mut at = 0u32;
        for _ in 0..4 {
            let Some(&c) = model.children.get(at as usize).and_then(|c| {
                c.iter()
                    .max_by_key(|&&c| model.totals[c as usize][if game { 2 } else { 0 }])
            }) else {
                break;
            };
            self.open.insert(c);
            at = c;
        }
        self.current = Some(model);
        self.current_pick = pick;
        self.error = None;
    }

    /// The functions in the measure's order.
    fn order(&mut self) -> Vec<usize> {
        let Some(m) = &self.current else {
            return Vec::new();
        };
        if let Some((measure, order)) = &self.order
            && *measure == self.measure
        {
            return order.clone();
        }
        // Functions: the dispatches have their own view.
        let mut order: Vec<usize> = (0..m.flat.len())
            .filter(|&i| m.keys[m.flat[i].key as usize].kind.is_none())
            .collect();
        let measure = self.measure;
        order.sort_by(|&a, &b| {
            let (fa, fb) = (&m.flat[a], &m.flat[b]);
            m.value(fb.total, fb.calls, measure)
                .cmp(&m.value(fa.total, fa.calls, measure))
                .then(fa.key.cmp(&fb.key))
        });
        self.order = Some((measure, order.clone()));
        order
    }
}

// ------------------------------------------------------------------ page

/// The profiler panel: the bar, the frames, the views.
pub fn show(
    ui: &mut Ui,
    state: &mut ProfilerState,
    workspace: &Workspace,
    system: &dyn System,
    running: bool,
) -> Option<Action> {
    let p = palette(ui.ctx());
    if !state.saved_read {
        state.refresh(workspace, system);
    }
    let mut action = None;
    // Its functions named by their outlines, asked once a profile.
    if let Some(m) = state.current.as_mut()
        && !m.game
        && !m.named
    {
        m.named = true;
        let files = m.sources();
        if !files.is_empty() {
            system.function_names(&workspace.root, files);
        }
    }
    bar(ui, state, workspace, system, running, p);
    if let Some(e) = state.error.clone() {
        forms::padded(ui, 12, 6, |ui| {
            forms::callout(ui, forms::CalloutKind::Error, |ui| {
                ui.label(
                    egui::RichText::new(e)
                        .font(Face::Plex.at(12.0))
                        .color(p.ink),
                );
            });
        });
    }
    if state.current.is_none() {
        empty(ui, state, running, p);
        return None;
    }
    note(ui, state, p);
    frames(ui, state, p);
    views(ui, state, p);
    let rest = ui.available_rect_before_wrap();
    ui.scope_builder(egui::UiBuilder::new().max_rect(rest), |ui| {
        action = match state.view {
            View::Functions => functions(ui, state, p),
            View::Tree => tree(ui, state, p),
            View::Flame => flame(ui, state, p),
            View::Dispatches => {
                dispatches(ui, state, p);
                None
            }
            View::Slow => {
                slow(ui, state, p);
                None
            }
            View::Hints => {
                hints(ui, state, system, p);
                None
            }
        };
    });
    action
}

/// The bar: record, the profile, the measure, compare, export.
fn bar(
    ui: &mut Ui,
    state: &mut ProfilerState,
    workspace: &Workspace,
    system: &dyn System,
    running: bool,
    p: &Palette,
) {
    forms::padded(ui, 10, 6, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if state.recording {
                let busy = state.stop_id.is_some();
                if ui
                    .add_enabled_ui(!busy, |ui| {
                        widgets::button(ui, ButtonKind::Primary, "stop and show", true, None)
                    })
                    .inner
                    .clicked()
                {
                    state.stop(system);
                }
            } else if ui
                .add_enabled_ui(running, |ui| {
                    widgets::button(ui, ButtonKind::Secondary, "record the session", true, None)
                })
                .inner
                .on_disabled_hover_text("start a session first: run the addon, or debug it")
                .clicked()
            {
                state.record(system);
            }
            // The profiles to pick from.
            let picks = picks(state);
            let mut labels: Vec<(String, bool)> =
                picks.iter().map(|(l, _)| (l.clone(), true)).collect();
            labels.push(("open a speedscope file…".into(), true));
            let current = state
                .current_pick
                .as_ref()
                .and_then(|c| picks.iter().position(|(_, p)| p == c));
            let mut chosen = current.unwrap_or(labels.len());
            if current.is_none() {
                labels.push((
                    state
                        .current
                        .as_ref()
                        .map_or_else(|| "no profile".to_owned(), |m| m.name.clone()),
                    false,
                ));
            }
            let options: Vec<(&str, bool)> = labels.iter().map(|(l, e)| (l.as_str(), *e)).collect();
            if forms::select(ui, "prof-pick", &options, &mut chosen, 300.0) {
                if chosen == picks.len() {
                    if let Some(path) = system.pick_file("open a speedscope file", &["json"]) {
                        state.read(&path, Target::Current, system);
                    }
                } else if let Some((_, pick)) = picks.get(chosen) {
                    open_pick(state, pick.clone(), Target::Current, system);
                }
            }
            let game = state.current.as_ref().is_some_and(|m| m.game);
            let measures = Measure::all(game);
            let names: Vec<&str> = measures.iter().map(|m| m.label(game)).collect();
            let at = measures
                .iter()
                .position(|m| *m == state.measure)
                .unwrap_or(0);
            if state.current.is_some()
                && let Some(i) = widgets::seg(ui, "prof-measure", &names, at, false)
            {
                state.measure = measures[i];
            }
        });
        // The second row: what to compare with; the export at the right.
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let game = state.current.as_ref().is_some_and(|m| m.game);
            let mut options: Vec<(String, Pick)> = picks(state)
                .into_iter()
                .filter(|(_, p)| Some(p) != state.current_pick.as_ref())
                .filter(|(_, p)| matches!(p, Pick::Game(..)) == game)
                .collect();
            if state.current.is_some() && !options.is_empty() {
                let mut on = state.compare;
                let toggled = forms::checkbox(ui, &mut on, "compare with", None);
                let mut base = state
                    .baseline_pick
                    .as_ref()
                    .and_then(|b| options.iter().position(|(_, p)| p == b))
                    .unwrap_or(0);
                let labels: Vec<(&str, bool)> =
                    options.iter().map(|(l, _)| (l.as_str(), true)).collect();
                let picked = ui
                    .add_enabled_ui(on, |ui| {
                        forms::select(ui, "prof-base", &labels, &mut base, 300.0)
                    })
                    .inner;
                state.compare = on;
                if on && (toggled || picked || state.baseline_pick.is_none()) {
                    let (_, pick) = options.swap_remove(base);
                    open_pick(state, pick, Target::Baseline, system);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let exportable = state.current.as_ref().and_then(|m| m.path.clone());
                if ui
                    .add_enabled_ui(exportable.is_some(), |ui| {
                        widgets::button(ui, ButtonKind::Ghost, "export speedscope", true, None)
                    })
                    .inner
                    .clicked()
                    && let Some(from) = exportable
                    && let Some(dir) = system.pick_folder("export the speedscope file to")
                    && let Some(name) = from.file_name()
                    && let Err(e) = system.copy_file(&from, &dir.join(name))
                {
                    state.error = Some(e);
                }
                let _ = workspace;
            });
        });
    });
    rule(ui, p);
}

/// The profiles to pick: kept in the workspace, the game's recordings.
fn picks(state: &ProfilerState) -> Vec<(String, Pick)> {
    let mut out: Vec<(String, Pick)> = state
        .saved
        .iter()
        .map(|s| (s.name.clone(), Pick::File(s.path.clone())))
        .collect();
    for (i, g) in state.game.iter().enumerate() {
        for (j, r) in g.recordings.iter().enumerate() {
            let name = r["name"].as_str().unwrap_or("recording");
            out.push((
                format!("{name} · from the game · {}", g.client),
                Pick::Game(i, j),
            ));
        }
    }
    out
}

fn open_pick(state: &mut ProfilerState, pick: Pick, target: Target, system: &dyn System) {
    match &pick {
        Pick::File(path) => {
            // The baseline is picked now; its profile comes when read.
            if target == Target::Baseline {
                state.baseline_pick = Some(pick.clone());
            }
            state.read(path, target, system);
        }
        Pick::Game(i, j) => {
            let Some(g) = state.game.get(*i) else { return };
            let Some(r) = g.recordings.get(*j) else {
                return;
            };
            let model = Model::of_game(r, &g.client, &g.account);
            match target {
                Target::Current => state.show(model, Some(pick)),
                Target::Baseline => {
                    state.baseline = Some(model);
                    state.baseline_pick = Some(pick);
                }
            }
        }
    }
}

fn empty(ui: &mut Ui, state: &ProfilerState, running: bool, p: &Palette) {
    forms::padded(ui, 16, 16, |ui| {
        ui.label(
            egui::RichText::new(if state.recording {
                "Recording"
            } else {
                "No profile"
            })
            .font(Face::MartianMedium.at(15.0))
            .color(p.ink),
        );
        ui.add_space(6.0);
        let text = if state.recording {
            "Every function, dispatch and game frame of the session is counted: its calls, \
             instructions, bytes allocated and host time. Stop to see them."
        } else if running {
            "Record the running session: every function, dispatch and game frame with its calls, \
             instructions, bytes allocated and host time. Or profile a test file from its lens or \
             the tests page. Every recording is kept in .wowlua/profiles, to compare with later; a \
             speedscope file opens here too."
        } else {
            "Start a session (run the addon, or debug it) and record it here. Or profile a test \
             file from its lens or the tests page. Every recording is kept in .wowlua/profiles, \
             to compare with later; a speedscope file opens here too."
        };
        forms::paragraph(ui, text, 12.5, 19.0, p.ink_muted);
        ui.add_space(6.0);
        let game = match &state.game_read {
            Some(Err(e)) => format!("The game's recordings could not be read: {e}"),
            _ if state.game.is_empty() => "Recordings from the game show here too: in the dev \
                window's performance tab, right-click a combat or a boss pull, mark it for export \
                and /reload. Studio finds it in your installation's SavedVariables."
                .to_owned(),
            _ => format!(
                "{} recordings from the game are in the picker.",
                state.game.iter().map(|g| g.recordings.len()).sum::<usize>()
            ),
        };
        forms::paragraph(ui, &game, 12.0, 18.0, p.ink_faint);
    });
}

fn note(ui: &mut Ui, state: &ProfilerState, p: &Palette) {
    let Some(m) = &state.current else { return };
    forms::padded(ui, 12, 4, |ui| {
        let text = match (&state.baseline, state.compare) {
            (Some(b), true) => format!("{} · compared with {}", m.note, b.name),
            _ => m.note.clone(),
        };
        forms::paragraph(ui, &text, 11.5, 17.0, p.ink_muted);
    });
}

/// The game frames (a game recording's seconds): a bar each, the picked
/// one the call tree's and flame graph's scope.
fn frames(ui: &mut Ui, state: &mut ProfilerState, p: &Palette) {
    let Some(m) = &state.current else { return };
    let measure = if m.game && state.measure == Measure::Calls {
        Measure::Calls
    } else if m.game {
        match state.measure {
            Measure::Bytes => Measure::Bytes,
            _ => Measure::Time,
        }
    } else {
        state.measure
    };
    let n = m.bars.len();
    let title = if m.game { "seconds" } else { "game frames" };
    let what = if m.game {
        format!(
            "the recording's {n} seconds · {} · the budget {}",
            match measure {
                Measure::Bytes => "the garbage of each second",
                Measure::Calls => "the frames of each second",
                _ => "the slowest frame of each second",
            },
            amount(m.budget, Measure::Time)
        )
    } else {
        format!("the last {n} · {} per frame", measure.label(false))
    };
    let picked = state.frame.filter(|&i| i < n);
    let picked_text = picked.map(|i| {
        let b = &m.bars[i];
        if m.game {
            format!(
                "{} · slowest frame {} · {} frames, {} over the budget · {} garbage",
                b.label,
                amount(b.cost[2], Measure::Time),
                b.calls,
                b.over,
                amount(b.cost[1], Measure::Bytes)
            )
        } else {
            format!(
                "{} · {} · {} · {}",
                b.label,
                amount(b.cost[0], Measure::Instructions),
                amount(b.cost[1], Measure::Bytes),
                amount(b.cost[2], Measure::Time)
            )
        }
    });
    let mut clicked = None;
    forms::padded(ui, 12, 4, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(title)
                    .font(Face::Plex.at(12.0))
                    .color(p.ink),
            );
            ui.label(
                egui::RichText::new(&what)
                    .font(Face::Plex.at(11.5))
                    .color(p.ink_muted),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(t) = &picked_text {
                    if forms::text_button(ui, "all frames").clicked() {
                        clicked = Some(None);
                    }
                    ui.label(
                        egui::RichText::new(t)
                            .font(Face::Plex.at(11.5))
                            .color(p.ink_muted),
                    );
                }
            });
        });
        let (rect, resp) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), BAR_H), Sense::click());
        ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_000);
        if n == 0 {
            return;
        }
        let value = |b: &Bar| match measure.index() {
            Some(i) => b.cost[i],
            None => b.calls,
        };
        let top = m.bars.iter().map(value).max().unwrap_or(1).max(
            if m.game && measure == Measure::Time {
                m.budget
            } else {
                1
            },
        );
        // A column per pixel at most: its bars' highest.
        let cols = (rect.width().floor() as usize).max(1);
        let per = n.div_ceil(cols).max(1);
        let w = rect.width() / n.div_ceil(per) as f32;
        let hover = resp.hover_pos();
        for (c, chunk) in m.bars.chunks(per).enumerate() {
            let v = chunk.iter().map(value).max().unwrap_or(0);
            let h = (v as f32 / top as f32 * (BAR_H - 4.0)).max(1.0);
            let x = rect.left() + c as f32 * w;
            let bar = Rect::from_min_max(
                Pos2::new(x, rect.bottom() - h),
                Pos2::new(x + (w - 1.0).max(1.0), rect.bottom()),
            );
            let index = c * per;
            let hot = m.game && measure == Measure::Time && v > m.budget;
            let color = if picked.is_some_and(|i| i / per == c) {
                p.amber
            } else if hot {
                p.error
            } else if hover.is_some_and(|h| h.x >= x && h.x < x + w) {
                p.ink
            } else {
                p.ink_faint
            };
            ui.painter().rect_filled(bar, CornerRadius::ZERO, color);
            if resp.clicked() && hover.is_some_and(|h| h.x >= x && h.x < x + w) {
                clicked = Some(Some(index));
            }
        }
        if m.game && measure == Measure::Time {
            let y = rect.bottom() - (m.budget as f32 / top as f32 * (BAR_H - 4.0));
            ui.painter()
                .hline(rect.x_range(), y, egui::Stroke::new(1.0, p.warning));
        }
        // A game recording's memory: a line over its seconds, and its
        // start, end and peak.
        if m.game && m.memory.len() > 1 {
            let (first, last) = (m.memory[0].1, m.memory[m.memory.len() - 1].1);
            let peak = m.memory.iter().map(|s| s.1).fold(0.0, f64::max);
            let low = m.memory.iter().map(|s| s.1).fold(f64::INFINITY, f64::min);
            let end = m.memory[m.memory.len() - 1].0.max(1.0);
            let (line, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
            let span = (peak - low).max(1.0);
            let points: Vec<Pos2> = m
                .memory
                .iter()
                .map(|&(at, kb)| {
                    Pos2::new(
                        line.left() + (at / end) as f32 * line.width(),
                        line.bottom() - 2.0 - ((kb - low) / span) as f32 * (line.height() - 4.0),
                    )
                })
                .collect();
            ui.painter()
                .add(egui::Shape::line(points, egui::Stroke::new(1.0, p.note)));
            let mb = |kb: f64| format!("{:.1} MB", kb / 1024.0);
            ui.label(
                egui::RichText::new(format!(
                    "memory {} → {} · peak {}",
                    mb(first),
                    mb(last),
                    mb(peak)
                ))
                .font(Face::Plex.at(11.0))
                .color(p.ink_muted),
            );
        }
    });
    if let Some(c) = clicked {
        state.frame = c;
        state.zoom = None;
    }
    rule(ui, p);
}

fn views(ui: &mut Ui, state: &mut ProfilerState, p: &Palette) {
    let game = state.current.as_ref().is_some_and(|m| m.game);
    let views = View::all(game);
    let labels: Vec<String> = views
        .iter()
        .map(|v| match (v, &state.current) {
            (View::Hints, Some(m)) => format!("hints {}", m.hints.len()),
            _ => v.label(game).to_owned(),
        })
        .collect();
    let names: Vec<&str> = labels.iter().map(String::as_str).collect();
    let at = views.iter().position(|v| *v == state.view).unwrap_or(0);
    forms::padded(ui, 12, 6, |ui| {
        ui.horizontal(|ui| {
            if let Some(i) = widgets::seg(ui, "prof-view", &names, at, false) {
                state.view = views[i];
            }
            let scope = match (state.view, state.frame, game) {
                (View::Tree | View::Flame, _, true) => {
                    "the whole recording: the game keeps no tree per frame".to_owned()
                }
                (View::Tree | View::Flame, Some(f), false) => {
                    let m = state.current.as_ref().expect("a profile");
                    if m.trees.get(f).is_some_and(|t| !t.is_empty()) {
                        m.bars[f].label.clone()
                    } else {
                        "the whole profile: this frame's tree was not kept".to_owned()
                    }
                }
                (_, _, true) => "the whole recording".to_owned(),
                _ => "the whole profile".to_owned(),
            };
            ui.label(
                egui::RichText::new(scope)
                    .font(Face::Plex.at(11.5))
                    .color(p.ink_muted),
            );
        });
    });
}

/// The function table: by the measure's total.
fn functions(ui: &mut Ui, state: &mut ProfilerState, p: &Palette) -> Option<Action> {
    let order = state.order();
    let Some(m) = &state.current else { return None };
    let measure = state.measure;
    let root_total = match measure.index() {
        Some(i) => m.totals.first().map_or(1, |t| t[i]).max(1),
        None => m.flat.iter().map(|f| f.calls).max().unwrap_or(1).max(1),
    };
    let base = state.baseline.as_ref().filter(|_| state.compare);
    let base_by: HashMap<&str, &Flat> = base
        .map(|b| {
            b.flat
                .iter()
                .map(|f| (b.keys[f.key as usize].name.as_str(), f))
                .collect()
        })
        .unwrap_or_default();
    let mut action = None;
    let mut select = None;
    header(
        ui,
        &[
            "function",
            "calls",
            "self",
            "total",
            "share",
            if base.is_some() { "vs baseline" } else { "" },
        ],
        p,
    );
    egui::ScrollArea::vertical()
        .id_salt("prof-fns")
        .auto_shrink(false)
        .show_rows(ui, row_h(), order.len(), |ui, range| {
            for &i in &order[range] {
                let f = &m.flat[i];
                let (rect, resp) = ui
                    .allocate_exact_size(Vec2::new(ui.available_width(), row_h()), Sense::click());
                let sel = state.selected == Some(f.key);
                if sel {
                    ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_400);
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_200);
                }
                let cols = columns(rect);
                let y = rect.center().y;
                let name = m.title(f.key);
                let color = if m.own(f.key) { p.ink } else { p.ink_muted };
                let r = ui.painter().with_clip_rect(cols[0]).text(
                    Pos2::new(cols[0].left() + 10.0, y),
                    Align2::LEFT_CENTER,
                    &name,
                    Face::Plex.at(12.0),
                    color,
                );
                // A named function's place after its name.
                if m.names.contains_key(&f.key) {
                    ui.painter().with_clip_rect(cols[0]).text(
                        Pos2::new(r.right() + 8.0, y),
                        Align2::LEFT_CENTER,
                        m.place(f.key),
                        Face::Plex.at(11.0),
                        p.ink_faint,
                    );
                }
                let total = m.value(f.total, f.calls, measure);
                for (c, text) in [
                    (1, amount(f.calls, Measure::Calls)),
                    (
                        2,
                        measure
                            .index()
                            .map_or(String::new(), |j| amount(f.own[j], measure)),
                    ),
                    (3, amount(total, measure)),
                ] {
                    ui.painter().text(
                        Pos2::new(cols[c].right() - 8.0, y),
                        Align2::RIGHT_CENTER,
                        text,
                        Face::Plex.at(12.0),
                        p.ink,
                    );
                }
                let share = (total as f32 / root_total as f32).clamp(0.0, 1.0);
                let meter = Rect::from_min_size(
                    Pos2::new(cols[4].left() + 4.0, y - 3.0),
                    Vec2::new((cols[4].width() - 12.0) * share, 6.0),
                );
                ui.painter().rect_filled(
                    meter,
                    CornerRadius::ZERO,
                    if m.own(f.key) { p.amber } else { p.bg_400 },
                );
                if base.is_some() {
                    let text = match base_by.get(m.keys[f.key as usize].name.as_str()) {
                        Some(b) => {
                            let was = base.map_or(0, |bm| bm.value(b.total, b.calls, measure));
                            delta(total, was)
                        }
                        None => "new".to_owned(),
                    };
                    let worse = text.starts_with('+') || text == "new";
                    ui.painter().text(
                        Pos2::new(cols[5].right() - 8.0, y),
                        Align2::RIGHT_CENTER,
                        text,
                        Face::Plex.at(12.0),
                        if worse { p.error } else { p.success },
                    );
                }
                if resp.double_clicked() {
                    action = open_action(m, f.key);
                }
                if resp.clicked() {
                    select = Some(f.key);
                }
            }
        });
    if let Some(k) = select {
        state.selected = Some(k);
    }
    action
}

fn delta(now: u64, was: u64) -> String {
    if was == 0 {
        return if now == 0 { "0%".into() } else { "new".into() };
    }
    let d = (now as f64 - was as f64) / was as f64 * 100.0;
    if d.abs() < 0.5 {
        "0%".into()
    } else {
        format!("{d:+.0}%")
    }
}

fn open_action(m: &Model, key: u32) -> Option<Action> {
    let k = &m.keys[key as usize];
    k.file
        .as_ref()
        .map(|f| Action::Open(f.clone(), k.line.max(1)))
}

/// A table's columns: the name, then numbers.
fn columns(rect: Rect) -> [Rect; 6] {
    let w = rect.width();
    let widths = [0.0, 80.0, 90.0, 90.0, 110.0, 90.0];
    let numbers: f32 = widths.iter().sum();
    let name_w = (w - numbers).max(120.0);
    let mut x = rect.left();
    let mut out = [Rect::NOTHING; 6];
    for (i, cw) in widths.iter().enumerate() {
        let cw = if i == 0 { name_w } else { *cw };
        out[i] = Rect::from_min_max(Pos2::new(x, rect.top()), Pos2::new(x + cw, rect.bottom()));
        x += cw;
    }
    out
}

fn header(ui: &mut Ui, titles: &[&str; 6], p: &Palette) {
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h()), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_200);
    let cols = columns(rect);
    for (i, t) in titles.iter().enumerate() {
        let (pos, align) = if i == 0 {
            (
                Pos2::new(cols[i].left() + 10.0, rect.center().y),
                Align2::LEFT_CENTER,
            )
        } else {
            (
                Pos2::new(cols[i].right() - 8.0, rect.center().y),
                Align2::RIGHT_CENTER,
            )
        };
        ui.painter()
            .text(pos, align, *t, Face::Plex.at(11.0), p.ink_faint);
    }
}

/// The scope's nodes: the picked frame's (its kept tree), else all; each
/// with its calls and total there.
fn scope(state: &ProfilerState) -> Option<HashMap<u32, (u64, [u64; 3])>> {
    let m = state.current.as_ref()?;
    let f = state.frame?;
    if m.game || m.trees.get(f).is_none_or(|t| t.is_empty()) {
        return None;
    }
    let calls: HashMap<u32, u64> = m.trees[f].iter().map(|n| (n.0, n.1)).collect();
    Some(
        m.frame_totals(f)
            .into_iter()
            .map(|(n, t)| (n, (calls.get(&n).copied().unwrap_or(0), t)))
            .collect(),
    )
}

/// The call tree: its rows opened by the user, the hottest first.
fn tree(ui: &mut Ui, state: &mut ProfilerState, p: &Palette) -> Option<Action> {
    let scope = scope(state);
    let m = state.current.as_ref()?;
    let measure = state.measure;
    let at = |n: u32| -> Option<(u64, [u64; 3])> {
        match &scope {
            Some(s) => s.get(&n).copied(),
            None => Some((m.nodes[n as usize].calls, m.totals[n as usize])),
        }
    };
    let own = |n: u32| -> u64 {
        match (&scope, measure.index()) {
            (Some(_), Some(i)) => {
                let total = at(n).map_or(0, |a| a.1[i]);
                let kids: u64 = m.children[n as usize]
                    .iter()
                    .filter_map(|&c| at(c))
                    .map(|a| a.1[i])
                    .sum();
                total.saturating_sub(kids)
            }
            (None, Some(i)) => m.nodes[n as usize].cost[i],
            (_, None) => at(n).map_or(0, |a| a.0),
        }
    };
    let value = |n: u32| at(n).map_or(0, |(calls, t)| m.value(t, calls, measure));
    // The rows shown: depth-first through the open nodes.
    let mut rows: Vec<(u32, usize)> = Vec::new();
    let mut stack: Vec<(u32, usize)> = Vec::new();
    let sorted = |n: u32| {
        let mut c: Vec<u32> = m.children[n as usize]
            .iter()
            .copied()
            .filter(|&c| at(c).is_some())
            .collect();
        c.sort_by_key(|&c| std::cmp::Reverse(value(c)));
        c
    };
    for c in sorted(0).into_iter().rev() {
        stack.push((c, 0));
    }
    while let Some((n, depth)) = stack.pop() {
        rows.push((n, depth));
        if state.open.contains(&n) {
            for c in sorted(n).into_iter().rev() {
                stack.push((c, depth + 1));
            }
        }
    }
    let mut toggle = None;
    let mut select = None;
    let mut action = None;
    header(ui, &["function", "calls", "self", "total", "", ""], p);
    egui::ScrollArea::vertical()
        .id_salt("prof-tree")
        .auto_shrink(false)
        .show_rows(ui, row_h(), rows.len(), |ui, range| {
            for &(n, depth) in &rows[range] {
                let key = m.nodes[n as usize].key;
                let (rect, resp) = ui
                    .allocate_exact_size(Vec2::new(ui.available_width(), row_h()), Sense::click());
                if state.selected == Some(key) {
                    ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_400);
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_200);
                }
                let cols = columns(rect);
                let y = rect.center().y;
                let x = cols[0].left() + 10.0 + depth as f32 * 14.0;
                let has_kids = m.children[n as usize].iter().any(|&c| at(c).is_some());
                if has_kids {
                    let twisty = if state.open.contains(&n) { "−" } else { "+" };
                    ui.painter().text(
                        Pos2::new(x, y),
                        Align2::LEFT_CENTER,
                        twisty,
                        Face::Plex.at(12.0),
                        p.ink_muted,
                    );
                }
                let color = if m.own(key) {
                    p.amber_ink
                } else if m.keys[key as usize].kind.is_some() {
                    p.ink_muted
                } else {
                    p.ink
                };
                ui.painter().with_clip_rect(cols[0]).text(
                    Pos2::new(x + 14.0, y),
                    Align2::LEFT_CENTER,
                    m.title(key),
                    Face::Plex.at(12.0),
                    color,
                );
                let (calls, _) = at(n).unwrap_or_default();
                for (c, text) in [
                    (1, amount(calls, Measure::Calls)),
                    (
                        2,
                        if measure == Measure::Calls {
                            String::new()
                        } else {
                            amount(own(n), measure)
                        },
                    ),
                    (3, amount(value(n), measure)),
                ] {
                    ui.painter().text(
                        Pos2::new(cols[c].right() - 8.0, y),
                        Align2::RIGHT_CENTER,
                        text,
                        Face::Plex.at(12.0),
                        p.ink,
                    );
                }
                if resp.double_clicked() {
                    action = open_action(m, key);
                } else if resp.clicked() {
                    if has_kids && resp.interact_pointer_pos().is_some_and(|q| q.x < x + 12.0) {
                        toggle = Some(n);
                    } else {
                        select = Some(key);
                        if has_kids {
                            toggle = Some(n);
                        }
                    }
                }
            }
        });
    if let Some(n) = toggle
        && !state.open.remove(&n)
    {
        state.open.insert(n);
    }
    if let Some(k) = select {
        state.selected = Some(k);
    }
    action
}

/// A box of the flame graph: its node, its row from the bottom, where it
/// is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
struct FlameBox {
    node: u32,
    depth: usize,
    rect: Rect,
}

/// The flame graph's boxes in `rect`, from `base` (the tree's root, not a
/// box itself, or a node zoomed to): each child's width its share of its
/// parent's, a parent's total at least its children's (a frame's scope
/// has no root of its own). Boxes narrower than a pixel, or above the
/// top, are left out.
fn flame_layout(m: &Model, total: &dyn Fn(u32) -> u64, base: u32, rect: Rect) -> Vec<FlameBox> {
    let mut out = Vec::new();
    let mut todo = vec![(base, rect.left(), rect.width(), 0usize)];
    while let Some((n, x, w, depth)) = todo.pop() {
        let drawn = n != 0 || base != 0;
        if drawn {
            let y = rect.bottom() - (depth as f32 + 1.0) * FLAME_ROW;
            if y < rect.top() || w < 1.0 {
                continue;
            }
            out.push(FlameBox {
                node: n,
                depth,
                rect: Rect::from_min_size(Pos2::new(x, y), Vec2::new(w - 1.0, FLAME_ROW - 1.0)),
            });
        }
        let next = if drawn { depth + 1 } else { depth };
        let mut kids: Vec<u32> = m.children[n as usize]
            .iter()
            .copied()
            .filter(|&c| total(c) > 0)
            .collect();
        kids.sort_by_key(|&c| std::cmp::Reverse(total(c)));
        let sum: u64 = kids.iter().map(|&c| total(c)).sum();
        let whole = total(n).max(sum).max(1);
        let mut left = x;
        for c in kids {
            let cw = w * (total(c) as f32 / whole as f32);
            todo.push((c, left, cw, next));
            left += cw;
        }
    }
    out
}

/// The flame graph: the game called in at the bottom; a box's width its
/// total by the measure. A click picks a function and zooms to it.
fn flame(ui: &mut Ui, state: &mut ProfilerState, p: &Palette) -> Option<Action> {
    let scope = scope(state);
    let m = state.current.as_ref()?;
    let measure = match state.measure {
        Measure::Calls if m.game => Measure::Time,
        Measure::Calls => Measure::Instructions,
        other => other,
    };
    let i = measure.index().unwrap_or(0);
    let total = |n: u32| -> u64 {
        match &scope {
            Some(s) => s.get(&n).map_or(0, |a| a.1[i]),
            None => m.totals[n as usize][i],
        }
    };
    let mut zoom_to = None;
    let mut select = None;
    let mut action = None;
    forms::padded(ui, 12, 4, |ui| {
        ui.horizontal(|ui| {
            forms::paragraph(
                ui,
                &format!(
                    "Width is {}; the bottom row is where the game called in. Amber: the package's code; grey: the game's and Blizzard's. Click to zoom.",
                    measure.label(m.game)
                ),
                11.5,
                17.0,
                p.ink_muted,
            );
            if state.zoom.is_some() && forms::text_button(ui, "all").clicked() {
                zoom_to = Some(None);
            }
        });
    });
    let rect = ui.available_rect_before_wrap();
    let resp = ui.allocate_rect(rect, Sense::click());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_000);
    let base = state
        .zoom
        .filter(|&z| (z as usize) < m.nodes.len())
        .unwrap_or(0);
    let boxes = flame_layout(m, &total, base, rect);
    let base_total = boxes
        .iter()
        .filter(|b| b.depth == 0)
        .map(|b| total(b.node))
        .sum::<u64>()
        .max(1);
    let hover = resp.hover_pos();
    let mut hovered_text = None;
    for b in &boxes {
        let key = m.nodes[b.node as usize].key;
        let r = b.rect;
        let fill = if state.selected == Some(key) {
            p.amber
        } else if m.own(key) {
            p.amber_soft
        } else {
            p.bg_300
        };
        ui.painter().rect_filled(r, CornerRadius::ZERO, fill);
        if r.width() > 30.0 {
            ui.painter().with_clip_rect(r.shrink(3.0)).text(
                Pos2::new(r.left() + 4.0, r.center().y),
                Align2::LEFT_CENTER,
                m.title(key),
                Face::Plex.at(11.0),
                p.ink,
            );
        }
        if hover.is_some_and(|h| r.contains(h)) {
            hovered_text = Some(format!(
                "{} · {} · {:.1}%",
                m.title(key),
                amount(total(b.node), measure),
                total(b.node) as f64 / base_total as f64 * 100.0
            ));
            if resp.clicked() {
                select = Some(key);
                zoom_to = Some(Some(b.node));
            }
            if resp.double_clicked() {
                action = open_action(m, key);
            }
        }
    }
    if let Some(t) = hovered_text {
        resp.on_hover_text(t);
    }
    if let Some(z) = zoom_to {
        state.zoom = z;
    }
    if let Some(k) = select {
        state.selected = Some(k);
    }
    action
}

fn dispatches(ui: &mut Ui, state: &mut ProfilerState, p: &Palette) {
    let Some(m) = &state.current else { return };
    if m.game && !m.watched.is_empty() {
        let half = (ui.available_height() / 2.0).max(row_h() * 4.0);
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(Rect::from_min_size(
                ui.available_rect_before_wrap().min,
                Vec2::new(ui.available_width(), half),
            )),
            |ui| dispatch_table(ui, m, state.measure, p),
        );
        ui.add_space(half);
        watched(ui, m, p);
        return;
    }
    dispatch_table(ui, m, state.measure, p);
}

/// A game recording's events, scripts, timers and commands.
fn watched(ui: &mut Ui, m: &Model, p: &Palette) {
    header(
        ui,
        &[
            "event, script, timer, command",
            "kind",
            "times",
            "total",
            "worst",
            "over budget",
        ],
        p,
    );
    egui::ScrollArea::vertical()
        .id_salt("prof-watched")
        .auto_shrink(false)
        .show_rows(ui, row_h(), m.watched.len(), |ui, range| {
            for w in &m.watched[range] {
                let (rect, resp) = ui
                    .allocate_exact_size(Vec2::new(ui.available_width(), row_h()), Sense::hover());
                if resp.hovered() {
                    ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_200);
                }
                let cols = columns(rect);
                let y = rect.center().y;
                ui.painter().with_clip_rect(cols[0]).text(
                    Pos2::new(cols[0].left() + 10.0, y),
                    Align2::LEFT_CENTER,
                    &w.name,
                    Face::Plex.at(12.0),
                    p.ink,
                );
                for (c, text, color) in [
                    (1, w.kind.to_owned(), p.ink_muted),
                    (2, amount(w.count, Measure::Calls), p.ink),
                    (3, amount(w.total, Measure::Time), p.ink),
                    (
                        4,
                        amount(w.worst, Measure::Time),
                        if w.worst > m.budget { p.error } else { p.ink },
                    ),
                    (
                        5,
                        amount(w.over, Measure::Calls),
                        if w.over > 0 { p.error } else { p.ink_muted },
                    ),
                ] {
                    ui.painter().text(
                        Pos2::new(cols[c].right() - 8.0, y),
                        Align2::RIGHT_CENTER,
                        text,
                        Face::Plex.at(12.0),
                        color,
                    );
                }
            }
        });
}

/// The dispatches: their times, total, worst run and share.
fn dispatch_table(ui: &mut Ui, m: &Model, measure: Measure, p: &Palette) {
    let measure = match measure {
        Measure::Calls => {
            if m.game {
                Measure::Time
            } else {
                Measure::Instructions
            }
        }
        Measure::Bytes if m.game => Measure::Time,
        other => other,
    };
    let i = measure.index().unwrap_or(0);
    let all: u64 = m.dispatches.iter().map(|d| d.total[i]).sum::<u64>().max(1);
    let mut rows: Vec<&Dispatch> = m.dispatches.iter().collect();
    rows.sort_by_key(|d| std::cmp::Reverse(d.total[i]));
    header(
        ui,
        &["dispatch", "kind", "times", "total", "worst", "share"],
        p,
    );
    egui::ScrollArea::vertical()
        .id_salt("prof-dispatch")
        .auto_shrink(false)
        .show_rows(ui, row_h(), rows.len(), |ui, range| {
            for d in &rows[range] {
                let (rect, resp) = ui
                    .allocate_exact_size(Vec2::new(ui.available_width(), row_h()), Sense::hover());
                if resp.hovered() {
                    ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_200);
                }
                let cols = columns(rect);
                let y = rect.center().y;
                ui.painter().with_clip_rect(cols[0]).text(
                    Pos2::new(cols[0].left() + 10.0, y),
                    Align2::LEFT_CENTER,
                    &d.name,
                    Face::Plex.at(12.0),
                    p.ink,
                );
                for (c, text) in [
                    (1, d.kind.clone()),
                    (2, amount(d.times, Measure::Calls)),
                    (3, amount(d.total[i], measure)),
                    (4, amount(d.worst[i], measure)),
                ] {
                    ui.painter().text(
                        Pos2::new(cols[c].right() - 8.0, y),
                        Align2::RIGHT_CENTER,
                        text,
                        Face::Plex.at(12.0),
                        if c == 1 { p.ink_muted } else { p.ink },
                    );
                }
                let share = (d.total[i] as f32 / all as f32).clamp(0.0, 1.0);
                let meter = Rect::from_min_size(
                    Pos2::new(cols[5].left() + 4.0, y - 3.0),
                    Vec2::new((cols[5].width() - 12.0) * share, 6.0),
                );
                ui.painter()
                    .rect_filled(meter, CornerRadius::ZERO, p.ink_faint);
            }
        });
}

fn slow(ui: &mut Ui, state: &mut ProfilerState, p: &Palette) {
    let Some(m) = &state.current else { return };
    egui::ScrollArea::vertical()
        .id_salt("prof-slow")
        .auto_shrink(false)
        .show(ui, |ui| {
            forms::padded(ui, 12, 8, |ui| {
                if m.slow.is_empty() {
                    forms::paragraph(ui, "No call took longer than the profile's slow-call.", 12.0, 18.0, p.ink_muted);
                }
                for s in &m.slow {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(format!("{:.1} ms", s.ms)).font(Face::Plex.at(12.0)).color(p.error));
                        ui.label(egui::RichText::new(&s.name).font(Face::Plex.at(12.0)).color(p.ink));
                        ui.label(egui::RichText::new(format!("at {}:{:02}", (s.at / 60.0) as u64, s.at as u64 % 60)).font(Face::Plex.at(11.5)).color(p.ink_faint));
                    });
                    forms::paragraph(ui, &s.stack, 11.5, 17.0, p.ink_muted);
                    ui.add_space(8.0);
                }
                forms::paragraph(ui, "Calls longer than the recording's slow-call, newest first; the game kept the last 50.", 11.5, 17.0, p.ink_faint);
            });
        });
}

fn hints(ui: &mut Ui, state: &mut ProfilerState, system: &dyn System, p: &Palette) {
    let Some(m) = &state.current else { return };
    let mut toggle = None;
    egui::ScrollArea::vertical()
        .id_salt("prof-hints")
        .auto_shrink(false)
        .show(ui, |ui| {
            forms::padded(ui, 12, 8, |ui| {
                if m.hints.is_empty() {
                    forms::paragraph(
                        ui,
                        "The dev window found nothing to point out in this recording.",
                        12.0,
                        18.0,
                        p.ink_muted,
                    );
                }
                for h in &m.hints {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(&h.code)
                                .font(Face::Plex.at(12.0))
                                .color(p.warning),
                        );
                        ui.label(
                            egui::RichText::new(&h.subject)
                                .font(Face::Plex.at(12.0))
                                .color(p.ink),
                        );
                    });
                    forms::paragraph(ui, &h.evidence, 12.0, 18.0, p.ink_muted);
                    forms::paragraph(ui, &format!("= help: {}", h.fix), 12.0, 18.0, p.ink_muted);
                    // wowlua's explanation of the code, opened under it.
                    let open = state.explained.get(&h.code);
                    let label = if open.is_some() {
                        format!("hide {}", h.code)
                    } else {
                        format!("explain {}", h.code)
                    };
                    if forms::text_button(ui, &label).clicked() {
                        toggle = Some(h.code.clone());
                    }
                    if let Some(text) = open {
                        let text = text
                            .as_deref()
                            .unwrap_or("wowlua has no explanation of this code.");
                        ui.add_space(4.0);
                        forms::paragraph(ui, text, 12.0, 18.0, p.ink);
                    }
                    ui.add_space(10.0);
                }
            });
        });
    if let Some(code) = toggle
        && state.explained.remove(&code).is_none()
    {
        let text = system.explain(&code);
        state.explained.insert(code, text);
    }
}

/// The function panel: the picked function's sums, its callers and
/// callees, its place.
pub fn function(ui: &mut Ui, state: &mut ProfilerState, p: &Palette) -> Option<Action> {
    let m = state.current.as_ref()?;
    let Some(key) = state.selected else {
        forms::padded(ui, 12, 12, |ui| {
            forms::paragraph(
                ui,
                "Pick a function in a table, the call tree or the flame graph.",
                12.5,
                19.0,
                p.ink_muted,
            );
        });
        return None;
    };
    let f = m
        .flat
        .iter()
        .find(|f| f.key == key)
        .copied()
        .unwrap_or(Flat {
            key,
            ..Flat::default()
        });
    // Callers and callees: over the nodes of this function.
    let mut by: BTreeMap<u32, u64> = BTreeMap::new();
    let mut to: BTreeMap<u32, u64> = BTreeMap::new();
    for (i, n) in m.nodes.iter().enumerate() {
        if n.key != key {
            continue;
        }
        if i != 0 {
            *by.entry(m.nodes[n.parent as usize].key).or_default() += n.calls;
        }
        for &c in &m.children[i] {
            *to.entry(m.nodes[c as usize].key).or_default() += m.nodes[c as usize].calls;
        }
    }
    let mut action = None;
    egui::ScrollArea::vertical()
        .id_salt("prof-function")
        .auto_shrink(false)
        .show(ui, |ui| {
            forms::padded(ui, 12, 10, |ui| {
                ui.label(
                    egui::RichText::new(m.title(key))
                        .font(Face::Plex.at(13.0))
                        .color(p.ink),
                );
                // A Lua function's name is its place already.
                let place = m.place(key);
                if place != m.title(key) {
                    ui.label(
                        egui::RichText::new(place)
                            .font(Face::Plex.at(11.5))
                            .color(p.ink_muted),
                    );
                }
                ui.add_space(8.0);
                let mut rows: Vec<(&str, String)> =
                    vec![("calls", amount(f.calls, Measure::Calls))];
                if !m.game {
                    rows.push(("instructions", amount(f.total[0], Measure::Instructions)));
                }
                rows.push((
                    if m.game { "garbage" } else { "bytes allocated" },
                    amount(f.total[1], Measure::Bytes),
                ));
                rows.push((
                    if m.game { "time" } else { "host time" },
                    amount(f.total[2], Measure::Time),
                ));
                if let Some(per) = f.total[if m.game { 2 } else { 0 }].checked_div(f.calls) {
                    let per = if m.game {
                        amount(per, Measure::Time)
                    } else {
                        format!("{} instructions", amount(per, Measure::Instructions))
                    };
                    rows.push(("per call", per));
                }
                if m.game && f.max > 0 {
                    rows.push(("slowest call", amount(f.max, Measure::Time)));
                }
                forms::key_values(ui, &rows);
                for (title, list) in [("called by", &by), ("calls", &to)] {
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(title)
                            .font(Face::Plex.at(11.0))
                            .color(p.ink_faint),
                    );
                    if list.is_empty() {
                        ui.label(
                            egui::RichText::new("nothing")
                                .font(Face::Plex.at(12.0))
                                .color(p.ink_muted),
                        );
                    }
                    let mut items: Vec<(&u32, &u64)> = list.iter().collect();
                    items.sort_by_key(|e| std::cmp::Reverse(*e.1));
                    for (k, calls) in items.into_iter().take(12) {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(if *k == 0 {
                                    "the game".to_owned()
                                } else {
                                    m.title(*k)
                                })
                                .font(Face::Plex.at(12.0))
                                .color(p.ink),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(format!("{calls}"))
                                            .font(Face::Plex.at(12.0))
                                            .color(p.ink_muted),
                                    );
                                },
                            );
                        });
                    }
                }
                if let Some(b) = state.baseline.as_ref().filter(|_| state.compare) {
                    let name = &m.keys[key as usize].name;
                    let was = b
                        .flat
                        .iter()
                        .find(|bf| &b.keys[bf.key as usize].name == name);
                    let i = state.measure.index().unwrap_or(2);
                    ui.add_space(10.0);
                    let text = match was {
                        Some(w) => format!(
                            "{} {} against {}.",
                            delta(f.total[i], w.total[i]),
                            state.measure.label(m.game),
                            b.name
                        ),
                        None => format!("Not in {}.", b.name),
                    };
                    forms::paragraph(ui, &text, 12.0, 18.0, p.ink_muted);
                }
                ui.add_space(12.0);
                let k = &m.keys[key as usize];
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled_ui(k.file.is_some(), |ui| {
                            widgets::button(ui, ButtonKind::Secondary, "go to code", true, None)
                        })
                        .inner
                        .clicked()
                    {
                        action = open_action(m, key);
                    }
                    let can_break = k
                        .file
                        .as_deref()
                        .is_some_and(|f| f.ends_with(".lua") || f.ends_with(".wowlua"))
                        && k.line > 0;
                    if ui
                        .add_enabled_ui(can_break, |ui| {
                            widgets::button(ui, ButtonKind::Ghost, "break on it", true, None)
                        })
                        .inner
                        .clicked()
                        && let Some(file) = &k.file
                    {
                        action = Some(Action::BreakOn(format!("{file}:{}", k.line)));
                    }
                });
            });
        });
    action
}

fn rule(ui: &mut Ui, p: &Palette) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::ZERO, p.line);
}

#[cfg(test)]
mod unit {
    use super::*;
    use crate::system::{ProfileFrame, ProfileKey, ProfileNode};

    /// A session's profile: two frames, the second with a spike.
    pub fn profile() -> Profile {
        let key = |name: &str, file: Option<&str>, line: u32, kind: Option<&str>| ProfileKey {
            name: name.into(),
            file: file.map(str::to_owned),
            line,
            kind: kind.map(str::to_owned),
        };
        let node = |key, parent, calls, i: u64| ProfileNode {
            key,
            parent,
            calls,
            cost: [i, i / 10, i * 3],
        };
        Profile {
            name: "demo".into(),
            keys: vec![
                key("(root)", None, 0, None),
                key("frame game frame", None, 0, Some("frame")),
                key("script MyFrame:OnUpdate", None, 0, Some("script")),
                key("function src/tick.lua:1", Some("src/tick.lua"), 1, None),
                key("function src/tick.lua:5", Some("src/tick.lua"), 5, None),
            ],
            nodes: vec![
                node(0, 0, 0, 0),
                node(1, 0, 2, 10),
                node(2, 1, 2, 20),
                node(3, 2, 3, 300),
                node(4, 2, 1, 5000),
            ],
            frames: vec![
                ProfileFrame {
                    name: "frame 1".into(),
                    cost: [215, 21, 645],
                    nodes: vec![
                        (1, 1, [5, 0, 15]),
                        (2, 1, [10, 1, 30]),
                        (3, 1, [200, 20, 600]),
                    ],
                },
                ProfileFrame {
                    name: "frame 2".into(),
                    cost: [5115, 511, 15345],
                    nodes: vec![
                        (1, 1, [5, 0, 15]),
                        (2, 1, [10, 1, 30]),
                        (3, 2, [100, 10, 300]),
                        (4, 1, [5000, 500, 15000]),
                    ],
                },
            ],
        }
    }

    #[test]
    fn a_session_profile_sums_its_functions() {
        let m = Model::of_profile(&profile(), None);
        let tick = m.flat.iter().find(|f| f.key == 3).expect("tick");
        assert_eq!(tick.calls, 3);
        assert_eq!(tick.total[0], 300);
        let update = m.flat.iter().find(|f| f.key == 2).expect("the script");
        assert_eq!(update.total[0], 20 + 300 + 5000);
        assert_eq!(m.dispatches.len(), 1, "the frame is not a dispatch");
        assert_eq!(m.dispatches[0].worst[0], 10 + 100 + 5000);
        // The spike's frame: its tree's totals.
        let totals: HashMap<u32, [u64; 3]> = m.frame_totals(1).into_iter().collect();
        assert_eq!(totals[&1][0], 5115);
        assert_eq!(m.title(3), "src/tick.lua:1");
        assert!(m.own(3));
    }

    #[test]
    fn a_game_recording_reads_as_a_model() {
        let rec = serde_json::json!({
            "name": "pull 1", "detail": "Lucifron", "duration": 161, "frameBudget": 1, "callPaths": true,
            "functions": {
                "ThreatModule:Update": { "calls": 600, "total": 145.7, "self": 12.0, "garbage": 400, "selfGarbage": 10, "max": 6.8, "location": "src/Units/Threat.wowlua:467" },
                "readRows": { "calls": 600, "total": 60.0, "self": 60.0, "garbage": 390, "selfGarbage": 390, "max": 2.0 }
            },
            "paths": [
                { "path": ["OnUpdate MyFrame", "ThreatModule:Update"], "calls": 600, "total": 145.7, "self": 85.7 },
                { "path": ["OnUpdate MyFrame", "ThreatModule:Update", "readRows"], "calls": 600, "total": 60.0, "self": 60.0 }
            ],
            "roots": { "OnUpdate MyFrame": { "kind": "update", "calls": 600, "total": 145.7, "max": 6.8 } },
            "seconds": { "frames": [60, 60], "time": [10, 12], "max": [0.4, 6.8], "over": [0, 3], "garbage": [38, 412] },
            "slow": [{ "at": 112, "ms": 6.8, "name": "ThreatModule:Update", "stack": "readRows ← ThreatModule:Update" }],
            "hints": [{ "code": "P7301", "subject": "ThreatModule:Update", "evidence": "took 6.8 ms", "fix": "spread the work" }],
            "events": { "UNIT_HEALTH": { "count": 1480, "total": 24.1, "max": 0.3, "over": 0 } },
            "scripts": { "SuuUIFrame:OnUpdate": { "count": 9660, "total": 145.7, "max": 6.8, "over": 3 } },
            "memory": [{ "at": 0, "kb": 10240 }, { "at": 60, "kb": 12288 }]
        });
        let m = Model::of_game(&rec, "_classic_beta_", "ACCOUNT");
        assert!(m.game);
        assert_eq!(m.bars.len(), 2);
        assert_eq!(m.bars[1].over, 3);
        assert_eq!(m.bars[1].cost[2], 6_800_000);
        let update = m
            .flat
            .iter()
            .find(|f| m.keys[f.key as usize].name == "ThreatModule:Update")
            .unwrap();
        assert_eq!(update.total[2], 145_700_000);
        assert_eq!(m.place(update.key), "src/Units/Threat.wowlua:467");
        // The call paths: the dispatch over the update over readRows.
        let root_kids = &m.children[0];
        assert_eq!(root_kids.len(), 1);
        assert_eq!(m.dispatches[0].worst[2], 6_800_000);
        assert_eq!((m.slow.len(), m.hints.len()), (1, 1));
        assert!(m.note.contains("Call paths were on"), "{}", m.note);
        // Its events and scripts, the costliest first; its memory.
        assert_eq!(
            m.watched
                .iter()
                .map(|w| (w.kind, w.over))
                .collect::<Vec<_>>(),
            [("script", 3), ("event", 0)]
        );
        assert_eq!(m.memory, [(0.0, 10240.0), (60.0, 12288.0)]);
    }

    /// In a frame's scope (no root of its own) the boxes fill the width:
    /// siblings side by side, each its share.
    #[test]
    fn a_frames_flame_fills_the_width() {
        let m = Model::of_profile(&profile(), None);
        let totals: HashMap<u32, [u64; 3]> = m.frame_totals(1).into_iter().collect();
        let total = |n: u32| totals.get(&n).map_or(0, |t| t[0]);
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 400.0));
        let boxes = flame_layout(&m, &total, 0, rect);
        let at = |n: u32| {
            boxes
                .iter()
                .find(|b| b.node == n)
                .map(|b| b.rect)
                .expect("a box")
        };
        assert!((at(1).width() - 999.0).abs() < 0.5, "{:?}", at(1));
        // The script's two functions side by side, by their totals.
        let (a, b) = (at(4), at(3));
        assert!(
            (a.left() - 0.0).abs() < 0.5 && (b.left() - a.right() - 1.0).abs() < 0.5,
            "{a:?} {b:?}"
        );
        assert!(b.right() <= 1000.0);
        assert!(a.width() > b.width());
    }

    #[test]
    fn amounts_read_well() {
        assert_eq!(amount(3_210_000, Measure::Instructions), "3.21 M");
        assert_eq!(amount(6_800_000, Measure::Time), "6.80 ms");
        assert_eq!(amount(412 * 1024, Measure::Bytes), "412 KB");
        assert_eq!(delta(120, 100), "+20%");
        assert_eq!(delta(80, 100), "-20%");
    }
}
