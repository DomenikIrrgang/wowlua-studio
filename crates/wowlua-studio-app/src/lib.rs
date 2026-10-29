//! wowlua studio's application: the window's chrome, docking, the command
//! palette and the pages, drawn with egui. It knows nothing of the system
//! it runs on: the host gives it the platform, wowlua's version, a place
//! to keep its files and the system ([`system::System`]), so it builds for
//! the browser too (S002).

pub mod chrome;
pub mod commands;
pub mod coverage;
pub mod debug;
pub mod deps;
pub mod dock;
pub mod editor;
pub mod explorer;
pub mod game;
pub mod jobs;
pub mod keys;
pub mod lsp;
pub mod manifest;
pub mod notify;
pub mod pages;
pub mod palette;
pub mod panels;
pub mod problems;
pub mod profiler;
pub mod recents;
pub mod search;
pub mod settings;
pub mod ship;
pub mod start;
pub mod state;
pub mod system;
pub mod terminal;
pub mod tests;
pub mod world;

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use egui::{Context, Pos2, Rect, Ui, UiBuilder, Vec2, ViewportCommand};
use wowlua_studio_ui::forms;
use wowlua_studio_ui::widgets::{self, ButtonKind};
use wowlua_studio_ui::{Face, Theme, mark, palette as colors};

use commands::{Command, Entry};
use dock::{DockState, Layout, Target};
use editor::{DiskChange, Editors};
use explorer::Explorer;
use notify::{Act, Kind, Notification, Notifications};
use pages::Page;
use palette::PaletteState;
use panels::Panel;
use recents::{RECENTS_FILE, Recents};
use start::StartPage;
use state::{LAYOUTS_FILE, Layouts, SETTINGS_FILE, Settings};
use system::{Event, Opened, System, Workspace};

/// The system Studio runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Windows,
    Linux,
    Web,
}

/// Where Studio keeps its files: the system's folder for application data
/// natively (S012).
pub trait Store {
    fn load(&self, name: &str) -> Option<String>;
    fn save(&self, name: &str, text: &str);
}

/// What the host gives Studio.
pub struct Host {
    pub platform: Platform,
    /// The version of wowlua built in.
    pub wowlua_version: String,
    pub store: Box<dyn Store>,
    pub system: Box<dyn System>,
}

/// Studio's whole state: one window, one workspace (S025).
/// A session to start in a profile, the edits for its live world
/// (`path`, `json`), and whether it started.
type QueuedWorld = (String, Vec<(String, String)>, bool);

pub struct Studio {
    host: Host,
    home: Option<PathBuf>,
    settings: Settings,
    layouts: Layouts,
    /// Shared with the other windows through their file (S031).
    recents: Recents,
    workspace: Option<Workspace>,
    branch: Option<String>,
    explorer: Explorer,
    start: StartPage,
    /// The page shown.
    page: Page,
    /// The page whose layout is shown: the page itself, unless layouts do
    /// not follow pages.
    layout_page: Page,
    dock: DockState,
    palette: PaletteState,
    about: bool,
    /// A folder to open while this window has a workspace (S025).
    ask: Option<Ask>,
    /// The open files.
    editors: Editors,
    /// wowlua's language server, while a workspace that can be checked is
    /// open.
    lsp: lsp::Lsp,
    /// The search panel: references, a rename's changes, text search.
    search: search::SearchState,
    /// The schemas of `wowlua.json` and world profiles, once read (S046).
    schemas: std::collections::HashMap<editor::json::Schema, serde_json::Value>,
    /// The terminal panel's shell (S043).
    terminal: terminal::TerminalState,
    /// The problems panel shows the editor's file only.
    problems_this_file: bool,
    /// The tests page (S052).
    tests: tests::TestsState,
    /// The game page (S055).
    game: game::GameState,
    /// The debugger page (S056).
    debug: debug::DebugState,
    profiler: profiler::ProfilerState,
    world: world::WorldState,
    /// The ship page (M9, S061).
    ship: ship::ShipState,
    /// The settings page (M10, S062).
    settings_page: settings::SettingsState,
    /// The wowlua.json tabs' forms (S063).
    manifests: manifest::Manifests,
    /// A wowlua.json to open on a section, as json or not (the screenshot
    /// harness's `manifest:`).
    queued_manifest: Option<(String, bool)>,
    /// The theme and scale last applied (S062).
    applied: Option<(wowlua_studio_ui::Theme, f32)>,
    /// The settings last applied (S062).
    prefs: Option<state::Settings>,
    /// The debugger's stops and dispatch frames as last synced with the
    /// settings: a change on the debugger page is kept in the settings.
    debug_seen: (Vec<String>, bool),
    /// The coverage page's gutter switch as last synced with the settings.
    gutter_seen: bool,
    /// The workspace's world profiles (`tests/profiles/*.json`), by name.
    profiles: Vec<String>,
    /// Background jobs (S044); the checks running, by package.
    jobs: jobs::Jobs,
    checks: std::collections::HashMap<PathBuf, u64>,
    /// The active file's outline: its path, the version it is for.
    outline: Option<(PathBuf, u64, Vec<lsp::Symbol>)>,
    /// When the outline was asked for, and for which version.
    outline_asked: Option<(PathBuf, u64)>,
    /// Notifications: toasts and the tray (S047).
    notes: Notifications,
    /// Waits for an answer about unsaved edits.
    pending: Option<Pending>,
    /// Closing the window was answered: it closes.
    quitting: bool,
    /// When Git's `HEAD` changed: files changing with it changed by a
    /// checkout (S049).
    head_changed: Option<f64>,
    /// Commands from outside the frame (macOS's menu bar).
    queued: Vec<Command>,
    /// Files to open in the editor at the next frame.
    queued_files: Vec<PathBuf>,
    /// A query for the find bar, once a file is open.
    queued_find: Option<String>,
    /// Run every test once the workspace and its test files are there,
    /// with coverage or not (the screenshot's `runtests`, `runcoverage`).
    queued_run: Option<bool>,
    /// Run the addon on the game page once the workspace is open (the
    /// screenshot's `rungame` step).
    queued_game: bool,
    /// Pick what draws at a point once the addon runs (`gamepick:x,y`).
    queued_pick: Option<(f64, f64)>,
    /// The game's settings once the addon runs (`gamesize:WxH[@scale]`).
    queued_size: Option<((u32, u32), Option<f32>)>,
    /// Breakpoints to set and debugging to start once the workspace is
    /// open (the screenshot's `bp:file:line`, `debug`).
    queued_bps: Vec<(PathBuf, u32)>,
    queued_debug: bool,
    /// A test to debug once the workspace is open (`debugtest:file::test`).
    queued_debug_test: Option<(PathBuf, Option<String>)>,
    /// The profiler's steps (`profile:file`, `profiletest:file::test`,
    /// `record:seconds`, `profview:view@frame#rank`): a profile to show, a
    /// test to profile, the session to record for that long (and when it
    /// started), the view.
    queued_profile: Option<PathBuf>,
    queued_profile_test: Option<(PathBuf, Option<String>)>,
    queued_record: Option<(f64, Option<f64>)>,
    /// A session to start in a profile, then edits for its live world
    /// (`path=json`), once it ran a moment (the screenshot harness's).
    queued_world: Option<QueuedWorld>,
    /// What the ship page does once the package is read (the screenshot
    /// harness's `shipdo:`).
    queued_ship: Vec<String>,
    queued_prof_view: Option<(profiler::View, Option<usize>, Option<usize>)>,
    /// A tab of the tests page's results to show once it is open.
    queued_tab: Option<tests::Tab>,
    /// Show the coverage in the gutter once the tests page is there.
    queued_gutter: bool,
    /// The commands changed: the menu bar is made again.
    menus_changed: bool,
    /// When the layouts or settings changed and are not saved yet.
    unsaved_since: Option<f64>,
    focused: bool,
    /// The window's title as last set.
    title: String,
    /// When the frames of the last second were drawn (S041).
    frame_times: VecDeque<f64>,
    /// The last frame's time, building and drawing it, as the host
    /// measured it.
    frame_cpu: Option<f32>,
}

/// What waits for unsaved edits to be saved or dropped.
#[derive(Clone, Debug, PartialEq)]
enum Pending {
    CloseTab(usize),
    CloseFolder,
    OpenHere(PathBuf),
    Quit,
}

/// Opening a folder in a window that has a workspace: here, or in a new
/// window.
struct Ask {
    path: PathBuf,
    new_window: bool,
}

impl Studio {
    pub fn new(ctx: &Context, host: Host) -> Self {
        let settings: Settings = state::parse(host.store.load(SETTINGS_FILE));
        let layouts: Layouts = state::parse(host.store.load(LAYOUTS_FILE));
        let recents: Recents = state::parse(host.store.load(RECENTS_FILE));
        wowlua_studio_ui::install(ctx, settings.theme());
        host.system.find_installation();
        Studio {
            home: host.system.home(),
            host,
            settings,
            layouts,
            recents,
            workspace: None,
            branch: None,
            explorer: Explorer::default(),
            start: StartPage::default(),
            page: Page::Files,
            layout_page: Page::Files,
            dock: DockState::default(),
            palette: PaletteState::default(),
            about: false,
            ask: None,
            editors: Editors::default(),
            lsp: lsp::Lsp::default(),
            search: search::SearchState::default(),
            schemas: std::collections::HashMap::new(),
            terminal: terminal::TerminalState::default(),
            problems_this_file: false,
            tests: tests::TestsState::default(),
            game: game::GameState::default(),
            debug: debug::DebugState::default(),
            profiler: profiler::ProfilerState::default(),
            world: world::WorldState::default(),
            ship: ship::ShipState::default(),
            settings_page: settings::SettingsState::default(),
            manifests: manifest::Manifests::default(),
            queued_manifest: None,
            applied: None,
            prefs: None,
            debug_seen: (Vec::new(), false),
            gutter_seen: false,
            profiles: Vec::new(),
            jobs: jobs::Jobs::default(),
            checks: std::collections::HashMap::new(),
            outline: None,
            outline_asked: None,
            notes: Notifications::default(),
            pending: None,
            quitting: false,
            head_changed: None,
            queued: Vec::new(),
            queued_files: Vec::new(),
            queued_find: None,
            queued_run: None,
            queued_game: false,
            queued_pick: None,
            queued_size: None,
            queued_bps: Vec::new(),
            queued_debug: false,
            queued_debug_test: None,
            queued_profile: None,
            queued_profile_test: None,
            queued_record: None,
            queued_world: None,
            queued_ship: Vec::new(),
            queued_prof_view: None,
            queued_tab: None,
            queued_gutter: false,
            menus_changed: false,
            unsaved_since: None,
            focused: true,
            title: String::new(),
            frame_times: VecDeque::new(),
            frame_cpu: None,
        }
    }

    /// The last frame's time, building and drawing it, for the frame stats.
    pub fn set_frame_time(&mut self, seconds: f32) {
        self.frame_cpu = Some(seconds);
    }

    pub fn platform(&self) -> Platform {
        self.host.platform
    }

    /// Every command, as the menus list them.
    pub fn entries(&self) -> Vec<Entry> {
        let mut entries = commands::entries(
            self.host.platform,
            self.settings.follow_presets,
            self.settings.frame_stats,
            self.workspace.is_some(),
        );
        // The keys recorded in the settings over the commands' own (S062).
        for e in &mut entries {
            let name = keys::name_of(&e.command);
            if self.settings.keys.contains_key(&name) {
                e.key = keys::key_of(&self.settings, &name, e.key);
            }
        }
        entries
    }

    /// Whether the commands changed since the last call: the host's menu
    /// bar is made again.
    pub fn take_menus_changed(&mut self) -> bool {
        std::mem::take(&mut self.menus_changed)
    }

    /// Runs a command at the next frame (the macOS menu bar's).
    pub fn queue(&mut self, command: Command) {
        self.queued.push(command);
    }

    /// Opens a file in the editor at the next frame, once a workspace is
    /// open.
    pub fn edit(&mut self, path: &Path) {
        self.queued_files.push(path.to_path_buf());
    }

    /// Opens the find bar with its replace row, `query` in it, once a file
    /// is open (the screenshot's `find:` step).
    pub fn find(&mut self, query: &str) {
        self.queued_find = Some(query.to_owned());
    }

    /// Shows a tab of the tests page's results: `failure`, `snapshot`,
    /// `screenshot` or `output` (the screenshot's `testtab:` step).
    pub fn tests_tab(&mut self, name: &str) {
        self.queued_tab = Some(match name {
            "snapshot" => tests::Tab::Snapshot,
            "screenshot" => tests::Tab::Screenshot,
            "output" => tests::Tab::Output,
            _ => tests::Tab::Failure,
        });
    }

    /// Runs the addon on the game page once the workspace is open (the
    /// screenshot's `rungame` step).
    pub fn run_game(&mut self) {
        self.queued_game = true;
    }

    /// Picks what draws at a point of the game's screen once the addon
    /// runs (the screenshot's `gamepick:x,y` step).
    pub fn game_pick(&mut self, x: f64, y: f64) {
        self.queued_pick = Some((x, y));
    }

    /// Sets a breakpoint once the workspace is open (the screenshot's
    /// `bp:file:line` step).
    pub fn set_breakpoint(&mut self, path: &Path, line: u32) {
        self.queued_bps.push((path.to_path_buf(), line));
    }

    /// Debugs a test once the workspace is open (the screenshot's
    /// `debugtest:file::test` step).
    pub fn debug_test_later(&mut self, file: &Path, test: Option<&str>) {
        self.queued_debug_test = Some((file.to_path_buf(), test.map(str::to_owned)));
    }

    /// The profiler's screenshot steps: a speedscope file shown, a test
    /// profiled, the running session recorded for `seconds`, a view.
    pub fn profile_later(&mut self, file: &Path) {
        self.queued_profile = Some(file.to_path_buf());
    }

    pub fn profile_test_later(&mut self, file: &Path, test: Option<&str>) {
        self.queued_profile_test = Some((file.to_path_buf(), test.map(str::to_owned)));
    }

    pub fn world_live_later(&mut self, profile: &str, edits: Vec<(String, String)>) {
        self.queued_world = Some((profile.to_owned(), edits, false));
    }

    pub fn record_later(&mut self, seconds: f64) {
        self.queued_record = Some((seconds, None));
    }

    pub fn profile_view(
        &mut self,
        view: profiler::View,
        frame: Option<usize>,
        rank: Option<usize>,
    ) {
        self.queued_prof_view = Some((view, frame, rank));
    }

    /// Starts debugging once the workspace is open (the screenshot's
    /// `debug` step).
    pub fn start_debugging(&mut self) {
        self.queued_debug = true;
    }

    /// Sets the game's settings once the addon runs (the screenshot's
    /// `gamesize:1280x720@0.8` step).
    pub fn game_size(&mut self, size: (u32, u32), ui_scale: Option<f32>) {
        self.queued_size = Some((size, ui_scale));
    }

    /// Runs every test once the workspace is open (the screenshot's
    /// `runtests` step).
    pub fn run_tests(&mut self, coverage: bool) {
        self.queued_run = Some(coverage);
    }

    /// Shows the coverage in the editor's gutter (the screenshot's `gutter`
    /// step).
    pub fn coverage_gutter(&mut self) {
        self.queued_gutter = true;
    }

    /// Where the active editor's hover is shown for: a line and a column.
    pub fn hovered(&self) -> Option<(usize, usize)> {
        let hover = self.editors.active()?.hover.as_ref()?;
        Some((hover.at.line, hover.at.col))
    }

    /// The name underlined as a link (cmd held over a name with a
    /// definition): its line and characters.
    pub fn linked(&self) -> Option<(usize, usize, usize)> {
        let link = self.editors.active()?.link.as_ref()?;
        (link.found == Some(true)).then_some((link.line, link.start, link.end))
    }

    /// Opens the workspace around `path` in this window.
    pub fn open(&mut self, path: &Path) {
        self.start.opening = Some(path.to_path_buf());
        self.start.failed = None;
        self.host.system.open(path);
    }

    /// Draws a frame.
    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.events(&ctx);
        let focused = ctx.input(|i| i.focused);
        if focused && !self.focused {
            // Another window may have changed the recents, Git the branch.
            self.reload_recents();
            if let Some(w) = &self.workspace {
                self.branch = self.host.system.branch(&w.root);
            }
        }
        self.focused = focused;
        // The OS's theme can change under a `system` theme.
        self.apply_settings(&ctx);
        self.debug.reveal_secrets = self.settings.reveal_secrets;
        self.debug.default_manual = self.settings.default_clock == "manual";

        // Closing the window with unsaved edits asks first.
        if ctx.input(|i| i.viewport().close_requested())
            && !self.quitting
            && self.editors.any_dirty()
        {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.pending = Some(Pending::Quit);
        }

        if let Some(w) = &self.workspace
            && let Some((section, json)) = self.queued_manifest.take()
        {
            let path = w.root.join("wowlua.json");
            self.manifests.show_section(&path, &section);
            if json {
                self.editors.as_json.insert(path.clone());
            }
            self.queued_files.push(path);
        }
        if self.workspace.is_some() {
            for path in std::mem::take(&mut self.queued_files) {
                self.open_file(&path, &ctx);
            }
            if self.editors.active().is_some()
                && let Some(query) = self.queued_find.take()
            {
                self.editors.finder.show_with(&query, true);
            }
            if let Some(tab) = self.queued_tab.filter(|_| self.tests.files.is_some()) {
                self.queued_tab = None;
                self.tests.tab = tab;
            }
            if self.queued_gutter && self.tests.files.is_some() {
                self.queued_gutter = false;
                self.tests.cov.in_gutter = true;
            }
            if self.tests.files.is_some() && self.queued_game {
                self.queued_game = false;
                self.queue(Command::RunAddon);
            }
            if self.tests.files.is_some()
                && !self.queued_bps.is_empty()
                && let Some(w) = self.workspace.clone()
            {
                for (path, line) in std::mem::take(&mut self.queued_bps) {
                    self.debug.toggle(&*self.host.system, &w, &path, line);
                }
            }
            if self.tests.files.is_some()
                && self.queued_bps.is_empty()
                && let Some((file, test)) = self.queued_debug_test.take()
            {
                self.debug_test(&file, test.as_deref());
            }
            if self.tests.files.is_some() && self.queued_bps.is_empty() && self.queued_debug {
                self.queued_debug = false;
                self.queue(Command::StartDebugging);
            }
            if self.game.running()
                && let Some((x, y)) = self.queued_pick.take()
            {
                self.game.pick(&*self.host.system, x, y);
            }
            if self.game.running()
                && let Some((size, ui_scale)) = self.queued_size.take()
            {
                self.game.set_screen(&*self.host.system, size, ui_scale);
            }
            if self.tests.files.is_some()
                && let Some(path) = self.queued_profile.take()
            {
                self.profiler.open_file(&path, &*self.host.system);
                self.queue(Command::ShowPage(Page::Perf));
            }
            if self.tests.files.is_some()
                && let Some((file, test)) = self.queued_profile_test.take()
            {
                self.profile_test(&file, test.as_deref(), &ctx);
            }
            if !self.queued_ship.is_empty()
                && let Some(w) = self.workspace.clone()
            {
                let what = self.queued_ship[0].clone();
                if self.ship.act(&what, &w, &*self.host.system) {
                    self.queued_ship.remove(0);
                }
                ctx.request_repaint();
            }
            if let Some((profile, edits, started)) = self.queued_world.take() {
                if !started && self.tests.files.is_some() {
                    self.world_action(world::Action::Start(profile.clone()), &ctx);
                    self.queued_world = Some((profile, edits, true));
                } else if started && self.debug.running() && self.debug.clock.1 > 30 {
                    for (path, value) in edits {
                        let keys = path
                            .split('/')
                            .map(|k| match k.parse::<usize>() {
                                Ok(i) => world::json::Key::Index(i),
                                Err(_) => world::json::Key::Name(k.to_owned()),
                            })
                            .collect();
                        self.world.pending.push(world::Change {
                            path: keys,
                            value: serde_json::from_str(&value).ok(),
                            typing: false,
                        });
                    }
                    self.queue(Command::ShowPage(Page::World));
                } else {
                    self.queued_world = Some((profile, edits, started));
                    ctx.request_repaint();
                }
            }
            if let Some((seconds, started)) = self.queued_record
                && self.game.running()
            {
                let now = ctx.input(|i| i.time);
                match started {
                    None => {
                        self.profiler.record(&*self.host.system);
                        self.queued_record = Some((seconds, Some(now)));
                        self.queue(Command::ShowPage(Page::Perf));
                    }
                    Some(at) if now - at >= seconds => {
                        self.profiler.stop(&*self.host.system);
                        self.queued_record = None;
                    }
                    Some(_) => ctx.request_repaint(),
                }
            }
            if self.profiler.current.is_some()
                && let Some((view, frame, rank)) = self.queued_prof_view.take()
            {
                self.profiler.set_view(view, frame);
                if let Some(r) = rank {
                    self.profiler.select_rank(r);
                }
            }
            if self.tests.files.is_some()
                && let Some(coverage) = self.queued_run.take()
            {
                self.tests.coverage_on = coverage;
                self.queue(Command::RunAllTests);
            }
        }
        self.lsp
            .sync(self.editors.tabs.iter().map(|t| &t.doc), &*self.host.system);

        let entries = self.entries();
        let mut commands = std::mem::take(&mut self.queued);
        // (not while a key is recorded for a command, S062)
        let recording = self.settings_page.recording.is_some();
        if !self.palette.open && self.ask.is_none() && self.pending.is_none() && !recording {
            ctx.input_mut(|i| {
                for e in &entries {
                    if let Some(key) = &e.key
                        && i.consume_shortcut(key)
                    {
                        commands.push(e.command);
                    }
                }
            });
        }
        // A folder dropped onto the window opens (a browser gives no path).
        let dropped = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()));
        if let Some(path) = dropped.filter(|_| self.host.platform != Platform::Web) {
            self.open_or_ask(path);
        }

        let full = ui.max_rect();
        let title = Rect::from_min_size(full.min, Vec2::new(full.width(), chrome::TITLE_H));
        let status = Rect::from_min_max(
            egui::pos2(full.left(), full.bottom() - chrome::STATUS_H),
            full.max,
        );

        let chip = self.workspace.as_ref().map(|w| chrome::Chip {
            name: &w.name,
            branch: self.branch.as_deref(),
            folder: recents::tilde(&w.root, self.home.as_deref()),
        });
        chrome::title_bar(ui, title, self.host.platform, &entries, chip, &mut commands);
        let mut right = self.status_items();
        if let Some(stats) = self.frame_stats(&ctx) {
            right.push(stats);
        }
        let checks_off = self
            .workspace
            .as_ref()
            .and_then(|w| w.cannot_check.as_ref())
            .map(|why| format!("checks off: {why}"));
        let (errors, warnings) =
            self.lsp
                .diagnostics
                .values()
                .flatten()
                .fold((0, 0), |(e, w), d| {
                    if d.is_error() {
                        (e + 1, w)
                    } else if d.severity == 2 {
                        (e, w + 1)
                    } else {
                        (e, w)
                    }
                });
        let session = chrome::Session {
            text: match (&self.workspace, &checks_off) {
                (None, _) => "no folder open",
                (Some(_), Some(text)) => text,
                (Some(_), None) if self.lsp.ready => "language server ready",
                (Some(_), None) if self.lsp.running() => "starting the language server…",
                (Some(_), None) => "",
            },
            warning: checks_off.is_some(),
            right: &right,
            inset: self.notes.tray_width(),
        };
        let session_end = chrome::status_bar(ui, status, &self.host.wowlua_version, &session);
        let cy = status.center().y;
        let job_w = if self.workspace.is_some() {
            self.jobs.button(ui, session_end + 14.0, cy)
        } else {
            0.0
        };
        if self.lsp.running() {
            let x = session_end + 14.0 + if job_w > 0.0 { job_w + 14.0 } else { 0.0 };
            chrome::problems(ui, x, cy, errors, warnings);
        }
        if let Some(jobs::Action::Show(panel)) =
            self.jobs.list(&ctx, session_end + 14.0, status.top())
        {
            let key = self.layout_key();
            let layout = layout_for(&mut self.layouts, &key, self.layout_page);
            layout.reveal(panel, dock::Dock::Bottom);
        }
        self.notes.tray_button(ui, status);

        let area_top = title.bottom();
        if self.workspace.is_some() {
            let rail = Rect::from_min_max(
                egui::pos2(full.left(), area_top),
                egui::pos2(full.left() + chrome::RAIL_W, status.top()),
            );
            chrome::rail(ui, rail, self.page, &mut commands);
            let area = Rect::from_min_max(
                egui::pos2(rail.right(), area_top),
                egui::pos2(full.right(), status.top()),
            );
            self.shell(ui, area, &ctx);
        } else {
            let area = Rect::from_min_max(
                egui::pos2(full.left(), area_top),
                egui::pos2(full.right(), status.top()),
            );
            self.start_page(ui, area, &ctx);
        }
        if let Some(act) = self.notes.show(ui, status) {
            self.act(act, &ctx);
        }

        if let Some(c) = palette::show(ui, &mut self.palette, &entries, self.host.platform) {
            commands.push(c);
        }
        if self.about {
            self.about_dialog(&ctx);
        }
        if self.ask.is_some() {
            self.ask_dialog(&ctx);
        }
        if self.pending.is_some() {
            self.unsaved_dialog(&ctx);
        }
        if self.host.platform != Platform::MacOs && self.host.platform != Platform::Web {
            chrome::resize_edges(ui, full);
        }

        for command in commands {
            self.run(&ctx, command);
        }
        self.set_title(&ctx);
        self.save_when_due(&ctx);
    }

    /// The workspace's pages: the docked panels.
    fn shell(&mut self, ui: &mut Ui, area: Rect, ctx: &Context) {
        let Some(workspace) = &self.workspace else {
            return;
        };
        let key = self.layout_key();
        let layout = layout_for(&mut self.layouts, &key, self.layout_page);
        let explorer = &mut self.explorer;
        let editors = &mut self.editors;
        let lsp_state = &self.lsp;
        let search_state = &mut self.search;
        let term_state = &mut self.terminal;
        let problems_this_file = &mut self.problems_this_file;
        let tests_state = &mut self.tests;
        let game_state = &mut self.game;
        let mut game_action = None;
        let session_running = self.debug.running() || game_state.running();
        let debug_state = &mut self.debug;
        let mut debug_action = None;
        let profiler_state = &mut self.profiler;
        let mut profiler_action = None;
        let world_state = &mut self.world;
        let mut world_action = None;
        let ship_state = &mut self.ship;
        let mut ship_action = None;
        let settings_page = &mut self.settings_page;
        let manifests = &mut self.manifests;
        let mut manifest_action = None;
        let platform = self.host.platform;
        settings_page.wowlua_version = self.host.wowlua_version.clone();
        let wowlua_version = self.host.wowlua_version.clone();
        let studio_settings = &mut self.settings;
        let mut settings_action = None;
        let installation = self.start.installation();
        let on_world = self.layout_page == Page::World;
        let on_debug = self.layout_page == Page::Debug;
        let game_status = game_state.status.clone();
        let profiles = &self.profiles;
        let mut tests_action = None;
        let mut search_action = None;
        let outline = &self.outline;
        let system = &*self.host.system;
        let mac = self.host.platform == Platform::MacOs;
        let mut opened = None;
        let mut go_to: Option<lsp::Location> = None;
        let mut go_to_symbol: Option<lsp::Wire> = None;
        let mut actions = Vec::new();
        let mut content = |ui: &mut Ui, panel: Panel| match panel {
            Panel::Explorer => {
                if let Some(path) = explorer.show(ui, workspace, system) {
                    opened = Some(path);
                }
            }
            Panel::Editor => {
                // By the server's name of the file: a manifest's problems
                // too, though the server does not have it open.
                let diagnostics = editors
                    .active()
                    .and_then(|t| lsp_state.diagnostics.get(&t.canonical))
                    .map_or(&[][..], Vec::as_slice);
                // The tests' lenses and the coverage of the active file.
                let active = editors.active().map(|t| t.doc.path.clone());
                let lenses = active
                    .as_deref()
                    .map(|path| tests::lenses(tests_state, workspace, path))
                    .unwrap_or_default();
                let tests_at: Vec<usize> = active
                    .as_deref()
                    .and_then(|path| tests_state.discovered.get(path))
                    .map(|tests| tests.iter().map(|t| t.line).collect())
                    .unwrap_or_default();
                let breakpoints = active
                    .as_deref()
                    .map(|path| debug_state.marks(path))
                    .unwrap_or_default();
                let stopped = active
                    .as_deref()
                    .and_then(|path| debug_state.stopped_line(path, workspace));
                // The stop's values after its lines.
                let inline = match (active.as_deref(), editors.active()) {
                    (Some(path), Some(tab))
                        if stopped.is_some() && studio_settings.inline_values =>
                    {
                        let doc = &tab.doc;
                        debug_state.inline_values(path, workspace, &|l| {
                            (l < doc.line_count()).then(|| doc.line(l).to_owned())
                        })
                    }
                    _ => Vec::new(),
                };
                // The session's bar and the stop's banner (the debugger's).
                if on_debug && let Some(a) = debug::bar(ui, debug_state, system) {
                    debug_action = Some(a);
                }
                let coverage = active.as_deref().and_then(|path| {
                    tests_state
                        .coverage
                        .as_ref()
                        .filter(|_| tests_state.cov.in_gutter)
                        .and_then(|c| c.files.get(path))
                });
                let cx = editor::view::Cx {
                    system,
                    workspace,
                    mac,
                    diagnostics,
                    lenses: &lenses,
                    tests_at: &tests_at,
                    coverage,
                    breakpoints: &breakpoints,
                    stopped,
                    inline: &inline,
                };
                // (below the debugger's bar, when it has one)
                let rest = ui.available_rect_before_wrap();
                ui.scope_builder(UiBuilder::new().max_rect(rest), |ui| {
                    let mut shown = editor::view::show(ui, editors, &cx);
                    // A wowlua.json tab: its bar and form (S063).
                    let manifest = shown.iter().find_map(|a| match a {
                        editor::view::Action::Manifest { tab, bar, form } => {
                            Some((*tab, *bar, *form))
                        }
                        _ => None,
                    });
                    shown.retain(|a| !matches!(a, editor::view::Action::Manifest { .. }));
                    actions.extend(shown);
                    if let Some((tab, bar, form)) = manifest {
                        ui.scope_builder(UiBuilder::new().max_rect(bar), |ui| {
                            if let Some(a) = manifest::bar(ui, manifests, editors, tab, workspace) {
                                manifest_action = Some(a);
                            }
                        });
                        if let Some(rect) = form {
                            ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                                if let Some(a) = manifest::form(
                                    ui,
                                    manifests,
                                    editors,
                                    tab,
                                    workspace,
                                    system,
                                    &wowlua_version,
                                ) {
                                    manifest_action = Some(a);
                                }
                            });
                        }
                    }
                });
            }
            Panel::Problems => {
                let only = editors.active().map(|t| t.canonical.clone());
                if let Some(l) = problems::problems(
                    ui,
                    lsp_state.diagnostics.iter(),
                    &workspace.root,
                    problems_this_file,
                    only.as_deref(),
                ) {
                    go_to = Some(l);
                }
            }
            Panel::Outline => {
                let active = editors.active().map(|t| t.doc.path.clone());
                let symbols = outline
                    .as_ref()
                    .filter(|(p, _, _)| Some(p) == active.as_ref())
                    .map(|(_, _, s)| s.as_slice());
                if let Some(w) = problems::outline(ui, symbols) {
                    go_to_symbol = Some(w);
                }
            }
            Panel::Search => search_action = search::show(ui, search_state, &workspace.root),
            Panel::Output => search::output(ui, &lsp_state.log),
            Panel::Terminal => terminal::show(ui, term_state, system, &workspace.root, mac),
            Panel::TestExplorer => {
                if let Some(a) = tests::explorer(ui, tests_state, workspace) {
                    tests_action = Some(a);
                }
            }
            Panel::Coverage => {
                if !tests_state.coverage_loaded {
                    tests_state.coverage_loaded = true;
                    tests_state.coverage = system.coverage_report(&workspace.root);
                }
                if let Some(a) = coverage::show(ui, tests_state, workspace, system) {
                    tests_action = Some(a);
                }
            }
            Panel::TestResults => {
                if let Some(a) = tests::results(ui, tests_state, workspace, system, profiles) {
                    tests_action = Some(a);
                }
            }
            Panel::Game => {
                if let Some(a) = game::screen(ui, game_state, workspace, system) {
                    game_action = Some(a);
                }
            }
            Panel::Frames => game::frames(ui, game_state, system),
            Panel::Units => game::units(ui, game_state, system),
            Panel::Inspector => {
                if let Some(a) = game::inspector(ui, game_state, workspace, system) {
                    game_action = Some(a);
                }
            }
            Panel::Events => game::events(ui, game_state, system),
            Panel::Input => game::input(ui, game_state),
            Panel::Screenshots => game::screenshots(ui, game_state, system),
            Panel::CallStack => {
                if let Some(a) =
                    debug::call_stack(ui, debug_state, workspace, system, game_status.as_deref())
                {
                    debug_action = Some(a);
                }
            }
            Panel::Breakpoints => {
                if let Some(a) = debug::breakpoints(ui, debug_state, workspace, system) {
                    debug_action = Some(a);
                }
            }
            Panel::Variables => debug::variables(ui, debug_state, system),
            Panel::Watch => debug::watch(ui, debug_state, system),
            Panel::Console => debug::console(ui, debug_state, system),
            Panel::LiveWorld if on_world => {
                if let Some(a) = world::live(ui, world_state, debug_state, workspace, system) {
                    world_action = Some(a);
                }
            }
            Panel::LiveWorld => {
                if let Some(a) = debug::live_world(ui, debug_state, workspace, system) {
                    debug_action = Some(a);
                }
            }
            Panel::Profiles => {
                if let Some(a) = world::profiles(ui, world_state, editors, workspace, system) {
                    world_action = Some(a);
                }
            }
            Panel::Parts => world::parts_panel(ui, world_state, editors, workspace, system),
            Panel::BuildProfiles => {
                if let Some(a) = ship::profiles(ui, ship_state, editors, workspace, system) {
                    ship_action = Some(a);
                }
            }
            Panel::Ship => {
                if let Some(a) = ship::ship(ui, ship_state, workspace, system, installation) {
                    ship_action = Some(a);
                }
            }
            Panel::SettingsSections => settings::sections(ui, settings_page),
            Panel::Settings => {
                if let Some(a) = settings::studio(
                    ui,
                    settings_page,
                    studio_settings,
                    system,
                    Some(workspace),
                    installation,
                    platform,
                ) {
                    settings_action = Some(a);
                }
            }
            Panel::ProfileEditor => {
                let live = world::session_profile(debug_state, world_state);
                let (json, a) = world::editor(
                    ui,
                    world_state,
                    editors,
                    workspace,
                    system,
                    &lsp_state.diagnostics,
                    live.as_deref(),
                );
                if let Some(a) = a {
                    world_action = Some(a);
                }
                // The json view: the profile's document in the editor's code.
                if let Some((rect, i)) = json {
                    let text = editors.tabs[i].doc.text();
                    let diagnostics = world::diagnostics(world_state, &text);
                    let cx = editor::view::Cx {
                        system,
                        workspace,
                        mac,
                        diagnostics: &diagnostics,
                        lenses: &[],
                        tests_at: &[],
                        coverage: None,
                        breakpoints: &[],
                        stopped: None,
                        inline: &[],
                    };
                    let tab = &mut editors.tabs[i];
                    ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                        // (typing, selection and undo act on the tab itself)
                        let _ = editor::view::single(ui, tab, &cx, i);
                    });
                }
            }
            Panel::Profiler => {
                profiler_action =
                    profiler::show(ui, profiler_state, workspace, system, session_running);
            }
            Panel::Function => {
                let p = wowlua_studio_ui::palette(ui.ctx());
                if let Some(a) = profiler::function(ui, profiler_state, p) {
                    profiler_action = Some(a);
                }
            }
            _ => dock::placeholder(ui, panel),
        };
        let changed = dock::show(ui, area, layout, &mut self.dock, &mut content);
        if changed {
            self.touch(ctx);
        }
        if let Some(path) = opened {
            self.open_file(&path, ctx);
        }
        if let Some(w) = self.workspace.clone()
            && let Some(a) = debug::dialogs(ctx, &mut self.debug, &w, &*self.host.system)
        {
            debug_action = Some(a);
        }
        if let Some(a) = debug_action {
            self.debug_action(a, ctx);
        }
        if let Some(a) = profiler_action {
            self.profiler_action(a, ctx);
        }
        if let Some(a) = world_action {
            self.world_action(a, ctx);
        }
        if let Some(w) = self.workspace.clone() {
            ship::dialogs(ctx, &mut self.ship, &w, &*self.host.system);
        }
        if let Some(a) = ship_action {
            self.ship_action(a, ctx);
        }
        if let Some(a) = settings_action {
            self.settings_action(a, ctx);
        }
        if let Some(a) = manifest_action {
            match a {
                manifest::Action::Save(path) => {
                    if let Some(i) = self.editors.find(&path)
                        && let Err(e) = self.editors.save(i, &*self.host.system)
                    {
                        let note = Notification::new(Kind::Error, "save", e);
                        self.notify(note, ctx);
                    }
                }
                // (the form's own section)
                manifest::Action::Go(manifest::Go::Dependencies) => {}
                manifest::Action::Go(manifest::Go::Ship) => {
                    self.queue(Command::ShowPage(Page::Ship))
                }
            }
        }
        world::flush_live(&mut self.world, &mut self.debug, &*self.host.system);
        self.pull_debug_settings(ctx);
        self.sync_gutter(ctx);
        // The stop's source, shown once.
        if let Some((path, line)) = self.debug.show.take() {
            let at = lsp::Wire {
                line: line.saturating_sub(1),
                character: 0,
            };
            let place = lsp::Location {
                path,
                start: at,
                end: at,
            };
            self.show_place(&place, ctx);
        }
        if self.debug.running()
            && let Some(s) = self.host.system.game_screen()
        {
            self.debug.clock = (s.time, s.frames);
        }
        if std::mem::take(&mut self.debug.changed) {
            self.keep_debug();
        }
        match game_action {
            Some(game::Action::Start) => self.queue(Command::RunAddon),
            Some(game::Action::Stop) => self.queue(Command::StopSession),
            Some(game::Action::Open(path, line)) => {
                let at = lsp::Wire {
                    line: line.saturating_sub(1),
                    character: 0,
                };
                let place = lsp::Location {
                    path,
                    start: at,
                    end: at,
                };
                self.show_place(&place, ctx);
                self.queue(Command::ShowPage(Page::Files));
            }
            Some(game::Action::ShowInspector) => {
                self.queue(Command::ShowPanel(Panel::Inspector, dock::Dock::Right));
            }
            Some(game::Action::BreakOn(names)) => {
                self.debug.add_functions(&*self.host.system, names);
            }
            None => {}
        }
        match tests_action {
            Some(tests::Action::Run(request)) => self.start_tests(request, ctx),
            Some(tests::Action::Stop) => self.host.system.test_stop(),
            Some(tests::Action::Open(path, line)) => {
                let at = lsp::Wire { line, character: 0 };
                let place = lsp::Location {
                    path,
                    start: at,
                    end: at,
                };
                self.show_place(&place, ctx);
                self.queue(Command::ShowPage(Page::Files));
            }
            Some(tests::Action::Debug(file, test)) => self.debug_test(&file, test.as_deref()),
            Some(tests::Action::Profile(file, test)) => {
                self.profile_test(&file, test.as_deref(), ctx)
            }
            Some(tests::Action::ShowCoverage) => {
                self.queue(Command::ShowPanel(Panel::Coverage, dock::Dock::Main));
            }
            None => {}
        }
        if let Some(l) = go_to {
            self.go_to(&l, ctx);
        }
        match search_action {
            Some(search::Action::Show(l)) => self.show_place(&l, ctx),
            Some(search::Action::Search(s)) => {
                if let Some(w) = &self.workspace {
                    self.host.system.search(&w.root, s.clone());
                }
                self.search.running = Some(s);
            }
            Some(search::Action::Rename(edits)) => self.apply_edits(edits, ctx),
            Some(search::Action::ReplaceAll(edits)) => {
                self.apply_edits(edits, ctx);
                // What is left to find.
                if let (Some(w), Some((s, _))) = (&self.workspace, &self.search.found) {
                    self.host.system.search(&w.root, s.clone());
                    self.search.running = Some(s.clone());
                }
            }
            None => {}
        }
        if let (Some(w), Some(path)) = (
            go_to_symbol,
            self.editors.active().map(|t| t.doc.path.clone()),
        ) {
            self.go_to(
                &lsp::Location {
                    path,
                    start: w,
                    end: w,
                },
                ctx,
            );
        }
        for action in actions {
            self.editor_action(action, ctx);
        }
        self.ask_outline(ctx);
    }

    /// What the editor asked for.
    fn editor_action(&mut self, action: editor::view::Action, ctx: &Context) {
        use editor::view::Action as A;
        // The breakpoint column (S056).
        if let A::ToggleBreakpoint(line) | A::EditBreakpoint(line) = action {
            let (Some(w), Some(path)) = (
                self.workspace.clone(),
                self.editors.active().map(|t| t.doc.path.clone()),
            ) else {
                return;
            };
            let line = line as u32 + 1;
            if let A::ToggleBreakpoint(_) = action {
                self.debug.toggle(&*self.host.system, &w, &path, line);
            } else {
                let bp = self
                    .debug
                    .kept
                    .lines
                    .get(&path)
                    .and_then(|l| l.iter().find(|b| b.line == line))
                    .cloned()
                    .unwrap_or(system::LineBp {
                        line,
                        ..system::LineBp::default()
                    });
                self.debug.editing_bp = Some((path, line, bp));
            }
            return;
        }
        // A JSON file with a schema answers from the schema (S046).
        if let Some(kind) = self
            .editors
            .active()
            .and_then(|t| editor::json::schema_of(&t.doc.path))
        {
            if matches!(action, A::Complete(_) | A::Hover(_)) {
                self.json_action(kind, action);
                return;
            }
            if !matches!(action, A::Save(_) | A::Close(_)) {
                return;
            }
        }
        let system = &*self.host.system;
        // The server has the text the request is about: what was typed
        // this frame reaches it first (a `.` asks for members).
        self.lsp
            .sync(self.editors.tabs.iter().map(|t| &t.doc), system);
        let Some(tab) = self.editors.active() else {
            return;
        };
        let doc = &tab.doc;
        match action {
            A::Save(i) => self.save_tab(i, ctx),
            A::Close(i) => self.close_tab(i),
            // (taken above)
            A::ToggleBreakpoint(_) | A::EditBreakpoint(_) | A::Manifest { .. } => {}
            A::Hover(at) => self.lsp.hover(doc, at, system),
            A::Complete(at) => self.lsp.complete(doc, at, system),
            A::Resolve(label) => {
                let path = doc.path.clone();
                let item = tab
                    .completion
                    .as_ref()
                    .and_then(|c| c.items.iter().find(|i| i.label == label))
                    .cloned();
                if let Some(item) = item {
                    self.lsp.resolve(&path, &item, system);
                    // (asked once: until it answers, and an empty answer,
                    // nothing shows)
                    if let Some(t) = self.editors.active_mut()
                        && let Some(c) = &mut t.completion
                    {
                        for it in c.items.iter_mut().filter(|i| i.label == label) {
                            it.doc = Some(String::new());
                        }
                    }
                }
            }
            A::Signature(at) => self.lsp.signature(doc, at, system),
            A::Definition(at) => self.lsp.definition(doc, at, system),
            A::Linkable(at) => self.lsp.linkable(doc, at, system),
            A::References(at) => {
                let line = doc.line(at.line);
                let (start, end) = word_bounds(line, at.col);
                let name: String = line.chars().skip(start).take(end - start).collect();
                self.lsp.references(doc, at, name, system);
            }
            A::Rename(at, name) => {
                let line = doc.line(at.line);
                let (start, end) = word_bounds(line, at.col);
                let old: String = line.chars().skip(start).take(end - start).collect();
                self.lsp.rename(doc, at, old, &name, system);
            }
            A::Fixes(at) => {
                if !self.lsp.fixes(doc, at, system) {
                    self.notify(Notification::new(Kind::Note, "No quick fix here", ""), ctx);
                }
            }
            A::ApplyFix(fix) => self.apply_edits(fix.edits, ctx),
            A::Lens(click) => self.lens_clicked(click, ctx),
        }
    }

    /// Completion and hover in a JSON file, from its schema.
    fn json_action(&mut self, kind: editor::json::Schema, action: editor::view::Action) {
        use editor::view::Action as A;
        if !self.schemas.contains_key(&kind) {
            let world = kind == editor::json::Schema::WorldProfile;
            if let Some(schema) = self
                .host
                .system
                .schema(world)
                .and_then(|t| serde_json::from_str(&t).ok())
            {
                self.schemas.insert(kind, schema);
            }
        }
        let (Some(schema), Some(tab)) = (self.schemas.get(&kind), self.editors.active_mut()) else {
            return;
        };
        let text = tab.doc.text();
        // Characters to the byte offset in the whole text.
        let offset_of = |doc: &editor::Document, at: editor::Pos| -> usize {
            (0..at.line).map(|l| doc.line(l).len() + 1).sum::<usize>()
                + editor::doc::byte(doc.line(at.line), at.col)
        };
        match action {
            A::Complete(at) => {
                let offset = offset_of(&tab.doc, at);
                let place = editor::json::place(&text, offset);
                let items: Vec<lsp::Item> = editor::json::suggestions(schema, &place)
                    .into_iter()
                    .map(|s| {
                        // From the quote typed: the suggestion has its own.
                        let line_start: usize =
                            (0..at.line).map(|l| tab.doc.line(l).len() + 1).sum();
                        let start_col = tab.doc.line(at.line)[..place
                            .start
                            .saturating_sub(line_start)
                            .min(tab.doc.line(at.line).len())]
                            .chars()
                            .count();
                        let start = lsp::to_wire(&tab.doc, editor::Pos::new(at.line, start_col));
                        let end = lsp::to_wire(&tab.doc, at);
                        let label = s.label.trim_matches('"').to_owned();
                        lsp::Item {
                            edits: Vec::new(),
                            doc: None,
                            raw: serde_json::Value::Null,
                            sort: label.clone(),
                            label,
                            detail: s.detail,
                            kind: if place.on_key { 10 } else { 12 },
                            insert: s.insert,
                            range: Some((start, end)),
                        }
                    })
                    .collect();
                tab.completion = (!items.is_empty()).then_some(editor::Completion {
                    at,
                    items,
                    selected: 0,
                    incomplete: false,
                });
            }
            A::Hover(at) => {
                let offset = offset_of(&tab.doc, at);
                if let Some(text) = editor::json::explain(schema, &text, offset) {
                    // The key's path, in a text block: nothing to colour.
                    tab.hover = Some(editor::Hover {
                        at,
                        text,
                        code: Vec::new(),
                    });
                }
            }
            _ => {}
        }
    }

    /// Asks for the active file's outline when its text has rested.
    fn ask_outline(&mut self, ctx: &Context) {
        let Some(tab) = self.editors.active() else {
            return;
        };
        let key = (tab.doc.path.clone(), tab.doc.version);
        if !self.lsp.ready || self.outline_asked.as_ref() == Some(&key) || !lsp::served(&key.0) {
            return;
        }
        let typed: f64 = ctx
            .data(|d| d.get_temp(egui::Id::unique("wowlua-studio-outline-typed")))
            .unwrap_or(0.0);
        let now = ctx.input(|i| i.time);
        if self
            .outline_asked
            .as_ref()
            .is_some_and(|(p, _)| *p == key.0)
            && now - typed < 0.4
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(400));
            return;
        }
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique("wowlua-studio-outline-typed"), now));
        self.lsp.symbols(&tab.doc, &*self.host.system);
        self.outline_asked = Some(key);
    }

    /// The name of the package whose manifest is `manifest` (as the server
    /// names it).
    fn package_of(&self, manifest: &Path) -> String {
        let system = &*self.host.system;
        self.workspace
            .as_ref()
            .and_then(|w| {
                w.members
                    .iter()
                    .find(|m| system.canonical(&m.dir.join("wowlua.json")) == manifest)
            })
            .map(|m| m.name.clone())
            .unwrap_or_else(|| {
                manifest
                    .parent()
                    .and_then(|d| d.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            })
    }

    /// A line of a file: from its tab when it is open, else from disk.
    fn line_of(&self, path: &Path, line: usize) -> String {
        let open = self
            .editors
            .tabs
            .iter()
            .find(|t| t.doc.path == path || self.lsp.canonical_of(&t.doc.path) == Some(path));
        match open {
            Some(t) => t.doc.line(line).to_owned(),
            None => self
                .host
                .system
                .read_file(path)
                .ok()
                .and_then(|text| text.lines().nth(line).map(str::to_owned))
                .unwrap_or_default(),
        }
    }

    /// Shows the search panel.
    fn reveal_search(&mut self) {
        let key = self.layout_key();
        let layout = layout_for(&mut self.layouts, &key, self.layout_page);
        if !layout.contains(Panel::Search) {
            layout.reveal(Panel::Search, dock::Dock::Left);
        }
    }

    /// Shows a place from the search panel, in the preview tab.
    fn show_place(&mut self, l: &lsp::Location, ctx: &Context) {
        let path = self
            .editors
            .tabs
            .iter()
            .map(|t| t.doc.path.clone())
            .find(|p| self.lsp.canonical_of(p) == Some(l.path.as_path()))
            .unwrap_or_else(|| l.path.clone());
        if let Err(error) = self.editors.preview(&path, &*self.host.system) {
            self.notify(
                Notification::new(Kind::Error, "The file did not open", error),
                ctx,
            );
            return;
        }
        if let Some(tab) = self.editors.active_mut() {
            let at = lsp::from_wire(&tab.doc, l.start);
            tab.doc.selection = editor::Selection::caret(at);
            tab.reveal = true;
        }
    }

    /// Opens a file at a place the server named.
    fn go_to(&mut self, l: &lsp::Location, ctx: &Context) {
        // The server names files by their canonical paths: an open one is
        // found by its own.
        let path = self
            .editors
            .tabs
            .iter()
            .map(|t| t.doc.path.clone())
            .find(|p| self.lsp.canonical_of(p) == Some(l.path.as_path()))
            .unwrap_or_else(|| l.path.clone());
        self.open_file(&path, ctx);
        if let Some(tab) = self.editors.active_mut() {
            let at = lsp::from_wire(&tab.doc, l.start);
            tab.doc.selection = editor::Selection::caret(at);
            tab.reveal = true;
        }
    }

    /// An answer of the language server.
    fn lsp_reply(&mut self, reply: lsp::Reply, ctx: &Context) {
        use lsp::Reply as R;
        let find = |editors: &Editors, path: &Path| editors.find(path);
        match reply {
            R::Hover { path, at, text } => {
                if let Some(i) = find(&self.editors, &path) {
                    let hover = editor::Hover::new(at, text, &*self.host.system);
                    self.editors.tabs[i].hover = Some(hover);
                }
            }
            R::Linkable { path, at, found } => {
                if let Some(i) = find(&self.editors, &path)
                    && let Some(link) = &mut self.editors.tabs[i].link
                    && (link.line, link.start) == (at.line, at.col)
                {
                    link.found = Some(found);
                }
            }
            R::Completion {
                path,
                at,
                items,
                incomplete,
            } => {
                if let Some(i) = find(&self.editors, &path)
                    && !items.is_empty()
                    && self.editors.tabs[i].doc.selection.head.line == at.line
                {
                    // (auto-import's items only when imports are added, S062)
                    let mut items = items;
                    if !self.settings.add_imports {
                        items.retain(|item| item.edits.is_empty());
                    }
                    self.editors.tabs[i].completion = Some(editor::Completion {
                        at,
                        items,
                        selected: 0,
                        incomplete,
                    });
                }
            }
            R::Resolved { path, label, doc } => {
                if let Some(i) = find(&self.editors, &path)
                    && let Some(c) = &mut self.editors.tabs[i].completion
                {
                    for item in c.items.iter_mut().filter(|it| it.label == label) {
                        item.doc = Some(doc.clone());
                    }
                }
            }
            R::Signature { path, text, active } => {
                if let Some(i) = find(&self.editors, &path) {
                    self.editors.tabs[i].signature = (!text.is_empty()).then_some((text, active));
                }
            }
            R::Definition(list) => {
                if let Some(l) = list.first() {
                    self.go_to(l, ctx);
                }
            }
            R::References { name, locations } => {
                let list = locations
                    .into_iter()
                    .map(|l| {
                        let text = self.line_of(&l.path, l.start.line);
                        (l, text)
                    })
                    .collect();
                self.search.references = Some((name, list));
                self.search.tab = search::Tab::References;
                self.reveal_search();
            }
            R::Renamed { old, new, files } => {
                // The changes are shown first: checked, then made (the
                // search board's rename).
                let files = files
                    .into_iter()
                    .map(|(path, edits)| {
                        let changes = edits
                            .into_iter()
                            .map(|edit| search::Change {
                                line: self.line_of(&path, edit.start.line),
                                edit,
                                checked: true,
                            })
                            .collect();
                        (path, changes)
                    })
                    .collect();
                self.search.rename = Some(search::Rename {
                    old,
                    new,
                    files,
                    refused: None,
                });
                self.search.tab = search::Tab::Rename;
                self.reveal_search();
            }
            R::RenameRefused { old, message } => {
                self.search.rename = Some(search::Rename {
                    old,
                    refused: Some(message),
                    ..search::Rename::default()
                });
                self.search.tab = search::Tab::Rename;
                self.reveal_search();
            }
            R::Tests { path, tests } => self.tests.discovered(path, tests),
            R::Symbols { path, symbols } => {
                let version = self
                    .editors
                    .find(&path)
                    .map_or(0, |i| self.editors.tabs[i].doc.version);
                self.outline = Some((path, version, symbols));
            }
            R::Format {
                path,
                edits,
                then_save,
            } => {
                if let Some(i) = find(&self.editors, &path) {
                    let doc = &mut self.editors.tabs[i].doc;
                    let list = edits
                        .iter()
                        .map(|e| {
                            (
                                lsp::from_wire(doc, e.start),
                                lsp::from_wire(doc, e.end),
                                e.text.clone(),
                            )
                        })
                        .collect();
                    doc.edits(list);
                    if then_save {
                        self.write_tab(i, ctx);
                    }
                }
            }
            R::Failed { save, message } => match save.and_then(|p| self.editors.find(&p)) {
                // The save goes ahead without the formatting.
                Some(i) => self.write_tab(i, ctx),
                None => self.lsp.log.push(message),
            },
            R::Message(text) => self.notify(Notification::new(Kind::Note, text, ""), ctx),
            R::Checking {
                manifest,
                begin,
                first,
                failed,
            } => {
                let now = ctx.input(|i| i.time);
                if begin {
                    let package = self.package_of(&manifest);
                    let detail = if first {
                        "the open files and what they import"
                    } else {
                        "the whole package"
                    };
                    let id =
                        self.jobs
                            .begin("check", &package, detail, 2.0, Some(Panel::Problems), now);
                    if let Some(old) = self.checks.insert(manifest, id) {
                        self.jobs.end(old, jobs::State::Done, None, now);
                    }
                } else if let Some(id) = self.checks.remove(&manifest) {
                    if failed {
                        let package = self.package_of(&manifest);
                        self.jobs.end(
                            id,
                            jobs::State::Failed("the check failed: see the output".into()),
                            None,
                            now,
                        );
                        self.notify(
                            Notification::new(Kind::Error, format!("check failed · {package}"), "The language server's check did not finish. The output panel has what it logged."),
                            ctx,
                        );
                    } else {
                        self.jobs.end(id, jobs::State::Done, None, now);
                    }
                }
            }
            R::Published(path) => {
                // A check: the open file's hints and lenses again.
                if let Some(tab) = self.editors.active()
                    && self.lsp.canonical_of(&tab.doc.path) == Some(path.as_path())
                {
                    let (h, l) = (self.settings.inlay_hints, self.settings.code_lenses);
                    self.lsp.decorations(&tab.doc, &*self.host.system, h, l);
                }
            }
            R::Hints {
                path,
                version,
                hints,
            } => {
                if let Some(i) = find(&self.editors, &path) {
                    let tab = &mut self.editors.tabs[i];
                    // Newer than those shown: an older text's are moved
                    // on with the edits since (the editor does).
                    if tab.hints.as_ref().is_none_or(|(v, _)| *v < version) {
                        let list = hints
                            .into_iter()
                            .map(|(w, label)| (lsp::from_wire(&tab.doc, w), label))
                            .collect();
                        tab.hints = Some((version, list));
                    }
                }
            }
            R::Lenses {
                path,
                version,
                lenses,
            } => {
                if let Some(i) = find(&self.editors, &path) {
                    let tab = &mut self.editors.tabs[i];
                    if tab.lenses.as_ref().is_none_or(|(v, _)| *v < version) {
                        let list = lenses
                            .into_iter()
                            .map(|(line, title, w)| (line, title, lsp::from_wire(&tab.doc, w)))
                            .collect();
                        tab.lenses = Some((version, list));
                    }
                }
            }
            R::Fixes { path, fixes } => {
                if let Some(i) = find(&self.editors, &path) {
                    if fixes.is_empty() {
                        self.notify(Notification::new(Kind::Note, "No quick fix here", ""), ctx);
                    } else {
                        self.editors.tabs[i].fixes = Some((fixes, 0));
                    }
                }
            }
        }
    }

    /// A rename's edits: open files change in the editor (to be saved),
    /// the others on disk.
    fn apply_edits(
        &mut self,
        files: std::collections::BTreeMap<PathBuf, Vec<lsp::TextEdit>>,
        ctx: &Context,
    ) {
        for (path, edits) in files {
            let open = self.editors.tabs.iter().position(|t| {
                t.doc.path == path || self.lsp.canonical_of(&t.doc.path) == Some(path.as_path())
            });
            let apply = |doc: &mut editor::Document| {
                let list = edits
                    .iter()
                    .map(|e| {
                        (
                            lsp::from_wire(doc, e.start),
                            lsp::from_wire(doc, e.end),
                            e.text.clone(),
                        )
                    })
                    .collect();
                doc.edits(list);
            };
            match open {
                Some(i) => apply(&mut self.editors.tabs[i].doc),
                None => {
                    let system = &*self.host.system;
                    let result = system.read_file(&path).and_then(|text| {
                        let mut doc = editor::Document::new(path.clone(), &text);
                        apply(&mut doc);
                        system.write_file(&path, &doc.disk_text())
                    });
                    if let Err(error) = result {
                        self.notify(
                            Notification::new(
                                Kind::Error,
                                "The rename did not change a file",
                                error,
                            ),
                            ctx,
                        );
                    }
                }
            }
        }
    }

    /// Opens a file in the editor, showing the editor's panel.
    fn open_file(&mut self, path: &Path, ctx: &Context) {
        match self.editors.open(path, &*self.host.system) {
            Ok(()) => {
                let key = self.layout_key();
                let layout = layout_for(&mut self.layouts, &key, self.layout_page);
                if !layout.contains(Panel::Editor) {
                    layout.reveal(Panel::Editor, dock::Dock::Main);
                    self.touch(ctx);
                }
            }
            Err(error) => self.notify(
                Notification::new(Kind::Error, "The file did not open", error),
                ctx,
            ),
        }
    }

    fn save_tab(&mut self, i: usize, ctx: &Context) {
        let Some(tab) = self.editors.tabs.get(i) else {
            return;
        };
        // Format on save (a setting, S062): the server's formatting first,
        // then the write.
        if self.settings.format_on_save
            && self.lsp.ready
            && lsp::served(&tab.doc.path)
            && self.lsp.format(&tab.doc, true, &*self.host.system)
        {
            return;
        }
        self.write_tab(i, ctx);
    }

    fn write_tab(&mut self, i: usize, ctx: &Context) {
        let Some(path) = self.editors.tabs.get(i).map(|t| t.doc.path.clone()) else {
            return;
        };
        match self.editors.save(i, &*self.host.system) {
            Ok(true) => {
                self.lsp.saved(&path, &*self.host.system);
                self.saved_for_tests(&path, ctx);
            }
            // The disk changed: the banner asks first (S049).
            Ok(false) => {}
            Err(error) => self.notify(
                Notification::new(Kind::Error, "The file was not saved", error),
                ctx,
            ),
        }
    }

    fn close_tab(&mut self, i: usize) {
        if self.editors.tabs.get(i).is_some_and(|t| t.doc.is_dirty()) {
            self.pending = Some(Pending::CloseTab(i));
        } else {
            self.editors.close(i);
        }
    }

    fn notify(&mut self, n: Notification, ctx: &Context) {
        let now = ctx.input(|i| i.time);
        self.notes.push(n, now);
    }

    /// A notification's button.
    fn act(&mut self, act: Act, ctx: &Context) {
        match act {
            Act::Restore { path, text } => match self.host.system.write_file(&path, &text) {
                Ok(()) => self.open_file(&path, ctx),
                Err(error) => self.notify(
                    Notification::new(Kind::Error, "The file was not restored", error),
                    ctx,
                ),
            },
            Act::Open(path) => self.open_file(&path, ctx),
        }
    }

    fn start_page(&mut self, ui: &mut Ui, area: Rect, ctx: &Context) {
        let p = colors(ctx);
        ui.painter()
            .rect_filled(area, egui::CornerRadius::ZERO, p.bg_100);
        let cx = start::Cx {
            system: &*self.host.system,
            recents: &self.recents,
            home: self.home.as_deref(),
            version: &self.host.wowlua_version,
        };
        let start = &mut self.start;
        let actions = ui
            .scope_builder(UiBuilder::new().max_rect(area), |ui| {
                ui.set_clip_rect(area);
                start.show(ui, &cx)
            })
            .inner;
        for action in actions {
            match action {
                start::Action::PickFolder => self.pick_and_open(),
                start::Action::Open(path) => self.open_or_ask(path),
                start::Action::Locate(from) => {
                    if let Some(to) = self.host.system.pick_folder("locate the workspace") {
                        self.reload_recents();
                        self.recents.relocate(&from, to.clone());
                        self.save_recents();
                        self.open(&to);
                    }
                }
                start::Action::Forget(path) => {
                    self.reload_recents();
                    self.recents.forget(&path);
                    self.save_recents();
                }
            }
        }
    }

    /// Work finished off the UI thread.
    fn events(&mut self, ctx: &Context) {
        for event in self.host.system.poll() {
            self.start.event(&event, &*self.host.system);
            match event {
                Event::Opened(Opened::Workspace(workspace)) => self.opened(workspace),
                Event::Opened(Opened::NoManifest(dir)) => {
                    // A folder with no `wowlua.json` can be adopted (S032).
                    self.start.opening = None;
                    self.close_workspace();
                    self.start.offer_adopt(&dir, self.home.as_deref());
                }
                Event::Opened(Opened::Failed { path, message }) => {
                    self.start.opening = None;
                    if self.workspace.is_some() {
                        let folder = recents::tilde(&path, self.home.as_deref());
                        self.notify(
                            Notification::new(
                                Kind::Error,
                                format!("{folder} did not open"),
                                message,
                            ),
                            ctx,
                        );
                    } else {
                        self.start.panel = start::Panel::Open;
                        self.start.failed = Some((path, message));
                    }
                }
                Event::Created(Ok(path)) => {
                    self.start.new = start::NewForm::default();
                    self.open(&path);
                }
                Event::Created(Err(error)) => {
                    self.start.new.creating = false;
                    self.start.new.error = Some(error);
                }
                Event::Adopted(Ok((dir, lines))) => {
                    self.start.adopt = start::AdoptForm::default();
                    let name = dir
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.notify(
                        Notification::new(Kind::Ok, format!("{name} adopted"), lines.join("\n")),
                        ctx,
                    );
                    self.open(&dir);
                }
                Event::Adopted(Err(error)) => {
                    self.start.adopt.adopting = false;
                    self.start.adopt.error = Some(error);
                }
                Event::FilesChanged(paths) => {
                    self.explorer.changed(&paths);
                    self.ship.files_changed();
                    self.manifests.deps.files_changed();
                    if let Some(w) = &self.workspace
                        && paths.iter().any(|p| p.ends_with("wowlua.json"))
                    {
                        let root = w.root.clone();
                        self.read_line_width(&root);
                    }
                    self.files_changed(&paths, ctx);
                    // A changed manifest can change the members: the
                    // workspace is read again.
                    let manifest = paths
                        .iter()
                        .any(|p| p.file_name().is_some_and(|n| n == "wowlua.json"));
                    if manifest && let Some(w) = &self.workspace {
                        self.host.system.open(&w.root);
                    }
                }
                Event::Tests { run, event } => self.test_event(run, event, ctx),
                Event::Game { session, event } => {
                    let now = ctx.input(|i| i.time);
                    let profiling = matches!(&event, system::GameEvent::Reply { id, .. } if *id >= profiler::IDS);
                    let debugger = !profiling
                        && matches!(&event, system::GameEvent::Reply { id, .. } if *id >= debug::IDS);
                    if let Some(w) = self.workspace.clone() {
                        if profiling || matches!(event, system::GameEvent::Ended { .. }) {
                            self.profiler.event(&event, &w, &*self.host.system);
                        }
                        if debugger || !matches!(event, system::GameEvent::Reply { .. }) {
                            self.debug.event(session, &event, &*self.host.system, &w);
                        }
                    }
                    if profiling {
                        ctx.request_repaint();
                        continue;
                    }
                    if !debugger {
                        self.game.event(session, event, now);
                    }
                    ctx.request_repaint();
                }
                Event::Searched { search, found } => {
                    if self.search.running.as_ref() == Some(&search) {
                        self.search.running = None;
                        self.search.found = Some((search, found));
                    }
                }
                Event::Lsp(message) => {
                    if let Some(reply) = self.lsp.receive(&message, &*self.host.system) {
                        self.lsp_reply(reply, ctx);
                    }
                }
                Event::Profile { path, result } => self.profiler.profile_read(&path, result),
                Event::GameRecordings(result) => self.profiler.game_recordings(result),
                Event::FunctionNames(names) => self.profiler.function_names(names),
                Event::WorldSnapshots(found) => self.world.snapshots_read(found),
                Event::GameArt(art, image) => self.world.art.read(ctx, art, image),
                Event::Spells(query, found) => self.world.spells_found(query, found),
                Event::ClassSpells(class, found) => self.world.class_spells_found(class, found),
                Event::Ship(event) => self.ship.event(event),
                Event::Deps(event) => self.manifests.deps.event(event),
                Event::Settings(info) => self.settings_page.info(info),
                Event::Updates(check) => self.settings_page.updates(check),
                Event::Github(event) => {
                    // GitHub's code: its page opens in the browser, the code
                    // on the clipboard to paste there (S062).
                    if let system::GithubSignIn::Code { code, url } = &event {
                        ctx.open_url(egui::OpenUrl::new_tab(url));
                        ctx.copy_text(code.clone());
                    }
                    self.settings_page.github(event)
                }
                Event::Installation(_) | Event::Ui { .. } => {}
            }
            ctx.request_repaint();
        }
    }

    /// Files changed outside Studio: open ones reload, clash or close
    /// (S048, S049).
    fn files_changed(&mut self, paths: &[PathBuf], ctx: &Context) {
        let now = ctx.input(|i| i.time);
        if self.lsp.running() {
            let system = &*self.host.system;
            let changes = paths
                .iter()
                .filter(|p| lsp::served(p) || p.ends_with("wowlua.json"))
                .map(|p| {
                    let change = if system.exists(p) {
                        system::FileChange::Changed
                    } else {
                        system::FileChange::Deleted
                    };
                    (system.canonical(p), change)
                })
                .collect::<Vec<_>>();
            if !changes.is_empty() {
                system.lsp_files(changes);
            }
        }
        if paths.iter().any(|p| p.ends_with(".git/HEAD")) {
            self.head_changed = Some(now);
            if let Some(w) = &self.workspace {
                self.branch = self.host.system.branch(&w.root);
            }
        }
        let checkout = self.head_changed.is_some_and(|t| now - t < 3.0);
        for change in self
            .editors
            .disk_changed(paths, checkout, &*self.host.system)
        {
            if let DiskChange::Deleted { path, text } = change {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.notify(
                    Notification::new(
                        Kind::Warning,
                        format!("{name} was deleted"),
                        "Deleted outside wowlua studio, so its tab was closed. Restore writes your last version back and opens it.",
                    )
                    .act("restore", Act::Restore { path, text }),
                    ctx,
                );
            }
        }
    }

    /// A workspace opened in this window, or read again.
    fn opened(&mut self, workspace: Workspace) {
        self.start.opening = None;
        self.start.failed = None;
        let again = self
            .workspace
            .as_ref()
            .is_some_and(|w| w.root == workspace.root);
        if !again {
            self.explorer = Explorer::new(&workspace);
            self.editors = Editors::default();
            self.search = search::SearchState::default();
            self.outline = None;
            // A package whose requirement excludes this wowlua opens with
            // checks, hover and completion off (E4300).
            if workspace.cannot_check.is_none() {
                let options = lsp::options(&self.settings);
                self.lsp.start(&workspace.root, &*self.host.system, options);
            } else {
                self.lsp.stop(&*self.host.system);
                self.terminal.close(&*self.host.system);
            }
            self.host.system.watch(Some(&workspace.root));
            self.page = Page::Files;
            self.layout_page = Page::Files;
            self.dock = DockState::default();
            self.menus_changed = true;
        }
        if !again {
            self.open_tests(&workspace);
            // The ship page's package and its dev loop are the last
            // workspace's.
            if self.ship.dev_on {
                self.host.system.ship_dev(&workspace.root, None);
            }
            // (the tab and profile chosen stay)
            let (tab, selected) = (self.ship.tab, self.ship.selected.take());
            self.ship = ship::ShipState::default();
            self.ship.tab = tab;
            self.ship.selected = selected;
        }
        self.read_line_width(&workspace.root);
        self.branch = self.host.system.branch(&workspace.root);
        self.reload_recents();
        self.recents.opened(&workspace, self.host.system.now());
        self.save_recents();
        self.workspace = Some(workspace);
    }

    /// A workspace's tests: its test files listed and read by the server,
    /// the last results it keeps, its world profiles.
    fn open_tests(&mut self, workspace: &Workspace) {
        let system = &*self.host.system;
        self.tests = tests::TestsState::default();
        // (the gutter as the settings say, S062)
        self.tests.cov.in_gutter = self.settings.coverage_gutter;
        self.gutter_seen = self.settings.coverage_gutter;
        let mut files: Vec<PathBuf> = Vec::new();
        let roots: Vec<PathBuf> = if workspace.members.is_empty() {
            vec![workspace.root.clone()]
        } else {
            workspace.members.iter().map(|m| m.dir.clone()).collect()
        };
        for root in &roots {
            for f in system.test_files(root) {
                if !files.contains(&f) {
                    files.push(f);
                }
            }
        }
        for f in &files {
            self.lsp.tests(f, system);
        }
        self.tests.files = Some(files);
        if let Some(kept) = self
            .kept_tests()
            .remove(&workspace.root.to_string_lossy().into_owned())
        {
            self.tests.restore(kept);
        }
        // The debugger's breakpoints, watches and history (S056).
        self.debug = debug::DebugState::default();
        let kept = self
            .kept_debug()
            .remove(&workspace.root.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.debug.restore(kept);
        self.profiles = system
            .list(&workspace.root.join("tests").join("profiles"))
            .into_iter()
            .filter(|e| !e.is_dir && e.name.ends_with(".json"))
            .map(|e| e.name.trim_end_matches(".json").to_owned())
            .collect();
        // (the settings' stops, not the workspace's, S062)
        self.push_debug_settings();
    }

    /// The tests every workspace keeps (`tests.json`), by its root.
    fn kept_tests(&self) -> std::collections::BTreeMap<String, tests::Kept> {
        self.host
            .store
            .load("tests.json")
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Keeps this workspace's results (the last ten workspaces').
    fn keep_tests(&self) {
        let Some(w) = &self.workspace else {
            return;
        };
        let mut all = self.kept_tests();
        let key = w.root.to_string_lossy().into_owned();
        all.remove(&key);
        while all.len() >= 10 {
            let Some(first) = all.keys().next().cloned() else {
                break;
            };
            all.remove(&first);
        }
        all.insert(key, self.tests.kept());
        if let Ok(text) = serde_json::to_string(&all) {
            self.host.store.save("tests.json", &text);
        }
    }

    /// Debugs a test file, or one test of it, on the debugger page.
    fn debug_test(&mut self, file: &Path, test: Option<&str>) {
        let Some(w) = self.workspace.clone() else {
            return;
        };
        let rel = tests::rel_of(&w, file);
        let test = test.map(tests::declared);
        let config = debug::Config {
            manual: self.settings.default_clock == "manual",
            ..debug::Config::test(&rel, test)
        };
        self.start_session(config);
        self.queue(Command::ShowPage(Page::Debug));
    }

    /// Profiles a test file, or one test of it (S057): its profile shown
    /// on the profiler page when the run keeps it.
    fn profile_test(&mut self, file: &Path, test: Option<&str>, ctx: &Context) {
        let Some(w) = self.workspace.clone() else {
            return;
        };
        let package = tests::package_of(&w, file);
        let mut request = tests::request_for(&package, file, test, &self.tests);
        request.profiler = true;
        request.coverage = false;
        self.profiler.awaiting_test = true;
        self.start_tests(request, ctx);
    }

    /// What the profiler page asks: a function's code, a breakpoint on it.
    fn profiler_action(&mut self, action: profiler::Action, ctx: &Context) {
        let Some(w) = self.workspace.clone() else {
            return;
        };
        match action {
            profiler::Action::Open(rel, line) => {
                // A source from its package's root: the root's, else a
                // member's that has it.
                let path = std::iter::once(w.root.clone())
                    .chain(w.members.iter().map(|m| m.dir.clone()))
                    .map(|r| r.join(&rel))
                    .find(|p| self.host.system.exists(p))
                    .unwrap_or_else(|| w.root.join(&rel));
                let at = lsp::Wire {
                    line: line.saturating_sub(1) as usize,
                    character: 0,
                };
                let place = lsp::Location {
                    path,
                    start: at,
                    end: at,
                };
                self.show_place(&place, ctx);
                self.queue(Command::ShowPage(Page::Files));
            }
            profiler::Action::BreakOn(name) => {
                self.debug.add_functions(&*self.host.system, vec![name]);
                self.keep_debug();
            }
        }
    }

    /// The configuration chosen on the debugger page, else the addon's.
    fn chosen_config(&mut self) -> debug::Config {
        let name = self
            .workspace
            .as_ref()
            .map(|w| w.name.clone())
            .unwrap_or_default();
        if !self.debug.configs_read
            && let Some(w) = &self.workspace
        {
            self.debug.configs_read = true;
            self.debug.configs = debug::read_configs(&w.root, &*self.host.system);
        }
        let chosen = self.debug.kept.chosen.clone();
        self.debug
            .configs
            .iter()
            .find(|c| Some(&c.name) == chosen.as_ref())
            .or_else(|| self.debug.configs.first())
            .cloned()
            .unwrap_or_else(|| debug::Config {
                manual: self.settings.default_clock == "manual",
                ..debug::Config::addon(&format!("{name} · the addon"))
            })
    }

    /// Starts a session of `config`, the debugger's breakpoints set first;
    /// one at a time (S056).
    fn start_session(&mut self, config: debug::Config) {
        let Some(w) = self.workspace.clone() else {
            return;
        };
        let session = self.game.begin();
        let setup = self.debug.begin(session, config.clone(), &w);
        self.host
            .system
            .game_start(&w.root, session, self.launch_of(&config), setup);
    }

    /// What the debugger page asked for.
    /// The world profiles page on a profile, a part of it, its json view
    /// (the screenshot harness's `world:`).
    pub fn world_show(&mut self, profile: &str, part: Option<&str>, json: bool) {
        self.queue(Command::ShowPage(Page::World));
        self.world.selected = Some(profile.to_owned());
        if let Some(part) = part {
            self.world.part = part.to_owned();
        }
        self.world.json_view = json;
    }

    /// The ship page on a tab and a profile (the screenshot harness's
    /// `ship:`).
    pub fn ship_show(&mut self, tab: &str, profile: Option<&str>) {
        self.queue(Command::ShowPage(Page::Ship));
        self.ship.tab = match tab {
            "install" => ship::Tab::Install,
            "publish" => ship::Tab::Publish,
            _ => ship::Tab::Build,
        };
        if let Some(p) = profile {
            self.ship.selected = Some(p.to_owned());
        }
    }

    /// What the ship page does once the package is read (the screenshot
    /// harness's `shipdo:`).
    pub fn ship_later(&mut self, what: &str) {
        self.queued_ship.push(what.to_owned());
    }

    /// The settings page on a section by its label (the screenshot
    /// harness's `settings:`).
    pub fn settings_show(&mut self, what: &str) {
        self.queue(Command::ShowPage(Page::Settings));
        self.settings_page.section = settings::Section::ALL
            .into_iter()
            .find(|s| s.label() == what)
            .unwrap_or_default();
    }

    /// The workspace's wowlua.json in its tab, on a section, its form or
    /// its json (the screenshot harness's `manifest:<section>[@json]`).
    pub fn manifest_show(&mut self, section: &str, json: bool) {
        self.queued_manifest = Some((section.to_owned(), json));
    }

    /// The spells part's search typed (the screenshot harness's `spells:`).
    pub fn world_search(&mut self, query: &str) {
        self.world.spell_query = query.to_owned();
    }

    /// The package's `fmt.line-width`, for wrapping at the line width
    /// (S062): 100 when it sets none.
    fn read_line_width(&self, root: &std::path::Path) {
        let width = self
            .host
            .system
            .read_file(&root.join("wowlua.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|m| m["fmt"]["line-width"].as_u64())
            .unwrap_or(100);
        editor::view::set_line_width(width as usize);
    }

    /// What the settings page asks (S062).
    fn settings_action(&mut self, action: settings::Action, ctx: &Context) {
        match action {
            settings::Action::Changed => {
                self.apply_settings(ctx);
                self.touch(ctx);
            }
            settings::Action::PickInstallation => {
                let system = &*self.host.system;
                if let Some(path) = system.pick_folder("Pick your World of Warcraft folder") {
                    system.set_installation(&path);
                }
            }
            settings::Action::ResetLayouts => {
                let key = self.layout_key();
                self.layouts.workspaces.remove(&key);
                self.touch(ctx);
            }
            settings::Action::OpenLog => {
                if let Some(log) = self.host.system.log_file() {
                    self.host.system.reveal(&log);
                }
            }
        }
    }

    /// Applies the settings that change how Studio draws (S062): the
    /// theme (the OS's for `system`) and the interface's scale.
    fn apply_settings(&mut self, ctx: &Context) {
        if self.prefs.as_ref() != Some(&self.settings) {
            let before = self.prefs.replace(self.settings.clone());
            state::set_prefs(ctx, &self.settings);
            wowlua_studio_ui::set_density(self.settings.comfortable());
            editor::view::set_font_size(self.settings.font_size);
            if let Some(before) = before {
                self.settings_changed(&before);
            }
        }
        let theme = if self.settings.follows_system() {
            match ctx.system_theme() {
                Some(egui::Theme::Light) => wowlua_studio_ui::Theme::Light,
                _ => wowlua_studio_ui::Theme::Dark,
            }
        } else {
            self.settings.theme()
        };
        let zoom = self.settings.zoom();
        if self.applied == Some((theme, zoom)) {
            return;
        }
        self.applied = Some((theme, zoom));
        wowlua_studio_ui::set_theme(ctx, theme);
        if (ctx.zoom_factor() - zoom).abs() > f32::EPSILON {
            ctx.set_zoom_factor(zoom);
        }
    }

    /// A configuration's launch with the game screen's settings (S062).
    fn launch_of(&self, config: &debug::Config) -> system::Launch {
        let s = &self.settings;
        let screen = s
            .screen_size
            .split_once(" × ")
            .and_then(|(w, h)| Some((w.trim().parse().ok()?, h.trim().parse().ok()?)));
        let launch = config.launch();
        system::Launch {
            art: s.installation_art,
            game_data: s.installation_data,
            screen,
            // (on: every session steps into Blizzard's code; off: as the
            // configuration says)
            debug_blizzard_ui: launch.debug_blizzard_ui || s.step_into_blizzard,
            ..launch
        }
    }

    /// The debugger takes the settings' stops and dispatch frames (S062).
    fn push_debug_settings(&mut self) {
        let mut stops = self.settings.stop_on.clone();
        stops.sort();
        let hide = !self.settings.show_dispatch;
        let changed = self.debug.kept.stop_on.iter().cloned().collect::<Vec<_>>() != stops;
        self.debug.kept.stop_on = stops.iter().cloned().collect();
        self.debug.kept.hide_dispatch = hide;
        self.debug_seen = (stops.clone(), hide);
        if changed && self.debug.running() {
            self.debug
                .send(&*self.host.system, system::GameRequest::SetStopOn(stops));
        }
    }

    /// The coverage page's gutter switch and the setting are one (S062).
    fn sync_gutter(&mut self, ctx: &Context) {
        let page = self.tests.cov.in_gutter;
        if page != self.gutter_seen {
            // the coverage page changed it
            self.settings.coverage_gutter = page;
            self.touch(ctx);
        } else if self.settings.coverage_gutter != page {
            // the settings did
            self.tests.cov.in_gutter = self.settings.coverage_gutter;
        }
        self.gutter_seen = self.tests.cov.in_gutter;
    }

    /// A change on the debugger page goes into the settings (S062).
    fn pull_debug_settings(&mut self, ctx: &Context) {
        let stops: Vec<String> = self.debug.kept.stop_on.iter().cloned().collect();
        let hide = self.debug.kept.hide_dispatch;
        if (stops.clone(), hide) == self.debug_seen {
            return;
        }
        self.debug_seen = (stops.clone(), hide);
        self.settings.stop_on = stops;
        self.settings.show_dispatch = !hide;
        self.touch(ctx);
    }

    /// What a change of the settings changes (S062).
    fn settings_changed(&mut self, before: &state::Settings) {
        if before.keys != self.settings.keys {
            // (the menu bar shows the keys)
            self.menus_changed = true;
        }
        if (&before.stop_on, before.show_dispatch)
            != (&self.settings.stop_on, self.settings.show_dispatch)
        {
            self.push_debug_settings();
        }
        let system = &*self.host.system;
        let options = lsp::options(&self.settings);
        if lsp::options(before) != options {
            self.lsp.configure(system, options);
        }
        let (hints, lenses) = (self.settings.inlay_hints, self.settings.code_lenses);
        if (hints, lenses) != (before.inlay_hints, before.code_lenses) {
            for tab in &mut self.editors.tabs {
                if !hints {
                    tab.hints = None;
                }
                if !lenses {
                    tab.lenses = None;
                }
            }
            if let Some(tab) = self.editors.active() {
                let on = (hints && !before.inlay_hints, lenses && !before.code_lenses);
                self.lsp.decorations(&tab.doc, system, on.0, on.1);
            }
        }
    }

    /// What the ship page asks (S061).
    fn ship_action(&mut self, action: ship::Action, ctx: &Context) {
        match action {
            ship::Action::Save(path) => {
                if let Some(i) = self.editors.find(&path)
                    && let Err(e) = self.editors.save(i, &*self.host.system)
                {
                    let note = Notification::new(Kind::Error, "save", e);
                    self.notify(note, ctx);
                }
            }
            ship::Action::GoTo(path, line, column) => {
                let at = lsp::Wire {
                    line: (line as usize).saturating_sub(1),
                    character: (column as usize).saturating_sub(1),
                };
                let location = lsp::Location {
                    path,
                    start: at,
                    end: at,
                };
                self.go_to(&location, ctx);
            }
            ship::Action::PickInstallation => {
                let system = &*self.host.system;
                if let Some(path) = system.pick_folder("Pick your World of Warcraft folder") {
                    system.set_installation(&path);
                }
            }
        }
    }

    /// What the world profiles page asks (S060).
    fn world_action(&mut self, action: world::Action, ctx: &Context) {
        match action {
            world::Action::Open(path) => self.open_file(&path, ctx),
            world::Action::Save(path) => {
                if let Some(i) = self.editors.find(&path)
                    && let Err(e) = self.editors.save(i, &*self.host.system)
                {
                    let note = Notification::new(Kind::Error, "save", e);
                    self.notify(note, ctx);
                }
            }
            world::Action::Note(title, text) => {
                let note = Notification::new(Kind::Error, &title, text);
                self.notify(note, ctx);
            }
            world::Action::RunTests(name) => {
                let Some(w) = &self.workspace else {
                    return;
                };
                let uses = self.host.system.profile_uses(&w.root, &name);
                let everything = uses.iter().any(|u| u == "wowlua.json · test.profile");
                let filters: Vec<String> = uses
                    .iter()
                    .filter_map(|u| u.split(" · ").next())
                    .filter(|f| f.ends_with(".test.wowlua"))
                    .map(str::to_owned)
                    .collect();
                if !everything && filters.is_empty() {
                    let note = Notification::new(
                        Kind::Note,
                        "run its tests",
                        format!(
                            "No test starts in {name}: config({{ profile = \"{name}\" }}) in a test file names it."
                        ),
                    );
                    self.notify(note, ctx);
                    return;
                }
                let request = system::TestRequest {
                    filters: if everything { Vec::new() } else { filters },
                    coverage: self.tests.coverage_on,
                    ..system::TestRequest::default()
                };
                self.queue(Command::ShowPage(Page::Tests));
                self.start_tests(request, ctx);
            }
            world::Action::Start(name) => {
                let Some(w) = &self.workspace else {
                    return;
                };
                let addon = w
                    .members
                    .first()
                    .map_or("addon".to_owned(), |m| m.name.clone());
                let mut config = debug::Config::addon(&format!("{addon} · {name}"));
                config.profile = Some(name);
                self.queue(Command::ShowPage(Page::Debug));
                self.start_session(config);
            }
        }
    }

    fn debug_action(&mut self, action: debug::Action, ctx: &Context) {
        match action {
            debug::Action::Start(config) => {
                self.start_session(config);
            }
            debug::Action::Stop => self.host.system.game_stop(),
            debug::Action::Restart => self.queue(Command::RestartSession),
            debug::Action::Open(path, line) => {
                let at = lsp::Wire {
                    line: line.saturating_sub(1),
                    character: 0,
                };
                let place = lsp::Location {
                    path,
                    start: at,
                    end: at,
                };
                self.show_place(&place, ctx);
            }
            debug::Action::SaveProfile(path, text) => {
                let _ = self.host.system.write_file(&path, &text);
            }
            debug::Action::SaveConfigs(list) => {
                if let Some(w) = &self.workspace {
                    let path = w.root.join(".wowlua").join("launch.json");
                    if let Err(e) = self
                        .host
                        .system
                        .write_file(&path, &debug::configs_json(&list))
                    {
                        let now = ctx.input(|i| i.time);
                        let note = Notification::new(Kind::Error, ".wowlua/launch.json", e);
                        self.notes.push(note, now);
                    }
                }
            }
        }
    }

    /// The debugger's kept state, by workspace (the last ten).
    fn kept_debug(&self) -> std::collections::BTreeMap<String, debug::Kept> {
        self.host
            .store
            .load("debug.json")
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    fn keep_debug(&self) {
        let Some(w) = &self.workspace else {
            return;
        };
        let mut all = self.kept_debug();
        let key = w.root.to_string_lossy().into_owned();
        all.remove(&key);
        while all.len() >= 10 {
            let Some(first) = all.keys().next().cloned() else {
                break;
            };
            all.remove(&first);
        }
        all.insert(key, self.debug.kept.clone());
        if let Ok(text) = serde_json::to_string(&all) {
            self.host.store.save("debug.json", &text);
        }
    }

    /// Runs tests: a job in the status bar, the results as they come.
    fn start_tests(&mut self, request: system::TestRequest, ctx: &Context) {
        let Some(w) = &self.workspace else {
            return;
        };
        if self.tests.running.is_some() {
            return;
        }
        let run = self.tests.next_run();
        let now = ctx.input(|i| i.time);
        let detail = match request.filters.len() {
            0 => "the whole suite".to_owned(),
            1 => request.filters[0].clone(),
            n => format!("{n} tests or files"),
        };
        let job = self.jobs.begin(
            "tests",
            &w.name,
            &detail,
            0.0,
            Some(Panel::TestResults),
            now,
        );
        let root = w.root.clone();
        self.tests.begin(run, request.clone(), job);
        self.host.system.test_run(&root, run, request);
    }

    /// A test run's news: its job, its end.
    fn test_event(&mut self, run: u64, event: system::TestEvent, ctx: &Context) {
        let Some(job) = self
            .tests
            .running
            .as_ref()
            .filter(|r| r.run == run)
            .map(|r| r.job)
        else {
            return;
        };
        if let system::TestEvent::Profiled { path, .. } = &event
            && let Some(w) = self.workspace.clone()
        {
            let showing = self.profiler.awaiting_test;
            self.profiler.test_profiled(path, &w, &*self.host.system);
            if showing {
                self.queue(Command::ShowPage(Page::Perf));
            }
        }
        let done = self.tests.event(run, event);
        let now = ctx.input(|i| i.time);
        if let Some(r) = &self.tests.running {
            let progress = (r.planned > 0).then(|| r.done as f32 / r.planned as f32);
            let detail = if r.planned == 0 {
                r.status.clone()
            } else {
                format!(
                    "{} of {} files · {} failed so far",
                    r.done, r.planned, r.failed
                )
            };
            self.jobs.update(job, progress, &detail);
        }
        let Some(done) = done else {
            return;
        };
        let (passed, failed, _, files) = self.tests.counts();
        if let Some(error) = &done.error {
            self.jobs.end(
                job,
                jobs::State::Failed(format!("no test ran: {error}")),
                None,
                now,
            );
        } else if failed > 0 {
            self.jobs.end(
                job,
                jobs::State::Failed(format!("{failed} failed · {passed} passed")),
                None,
                now,
            );
        } else {
            let detail = format!("{passed} passed in {files} files");
            self.jobs.end(job, jobs::State::Done, Some(detail), now);
        }
        self.keep_tests();
    }

    /// A lens of the tests was clicked: run, the failure, the callers.
    fn lens_clicked(&mut self, click: editor::view::LensClick, ctx: &Context) {
        use editor::view::LensClick as C;
        let Some(w) = self.workspace.clone() else {
            return;
        };
        match click {
            C::Run { file, test } => {
                let package = tests::package_of(&w, &file);
                let request = tests::request_for(&package, &file, test.as_deref(), &self.tests);
                self.start_tests(request, ctx);
            }
            C::Failure { file, test } => {
                self.tests.selected = Some((file, Some(test)));
                self.tests.tab = tests::Tab::Failure;
                self.queue(Command::ShowPage(Page::Tests));
            }
            C::Callers { file, line } => {
                self.tests.pin_callers(&w, &file, line);
                self.queue(Command::ShowPage(Page::Tests));
            }
            C::Debug { file, test } => self.debug_test(&file, test.as_deref()),
            C::Profile { file, test } => self.profile_test(&file, test.as_deref(), ctx),
        }
    }

    /// A file saved: its tests read again, and with run on save the tests
    /// it affects run.
    fn saved_for_tests(&mut self, path: &Path, ctx: &Context) {
        let Some(w) = &self.workspace else {
            return;
        };
        let is_test = path.to_string_lossy().ends_with(".test.wowlua");
        if is_test {
            if let Some(files) = &mut self.tests.files
                && !files.iter().any(|f| f == path)
            {
                files.push(path.to_path_buf());
                files.sort();
            }
            self.lsp.tests(path, &*self.host.system);
        }
        if !self.tests.run_on_save
            || self.tests.running.is_some()
            || path.extension().is_none_or(|e| e != "wowlua")
        {
            return;
        }
        let affected = self.host.system.affected_tests(&w.root, path);
        if affected.is_empty() {
            return;
        }
        let filters = affected
            .iter()
            .map(|f| {
                let package = tests::package_of(w, f);
                f.strip_prefix(&package)
                    .unwrap_or(f)
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        let request = system::TestRequest {
            filters,
            coverage: self.tests.coverage_on,
            update_snapshots: false,
            profile: self.tests.profile.clone(),
            profiler: false,
        };
        self.start_tests(request, ctx);
    }

    fn close_workspace(&mut self) {
        if self.workspace.take().is_some() {
            self.host.system.watch(None);
            self.explorer = Explorer::default();
            self.editors = Editors::default();
            self.lsp.stop(&*self.host.system);
            self.search = search::SearchState::default();
            self.outline = None;
            self.branch = None;
            self.menus_changed = true;
            self.reload_recents();
        }
    }

    /// The system's folder picker, then the folder.
    fn pick_and_open(&mut self) {
        if let Some(path) = self.host.system.pick_folder("open a folder") {
            self.open_or_ask(path);
        }
    }

    /// Opens `path` here, or asks where when this window has a workspace
    /// (S025).
    fn open_or_ask(&mut self, path: PathBuf) {
        if self.workspace.is_some() {
            self.ask = Some(Ask {
                path,
                new_window: true,
            });
        } else {
            self.open(&path);
        }
    }

    fn reload_recents(&mut self) {
        self.recents = state::parse(self.host.store.load(RECENTS_FILE));
        self.start.refresh(&*self.host.system, &self.recents);
    }

    fn save_recents(&mut self) {
        self.host
            .store
            .save(RECENTS_FILE, &state::to_text(&self.recents));
        self.start.refresh(&*self.host.system, &self.recents);
    }

    /// The status bar's right: the workspace's flavors and the clients the
    /// installation has for them.
    fn status_items(&self) -> Vec<String> {
        let Some(w) = &self.workspace else {
            return Vec::new();
        };
        let mut items = vec![w.flavors.join(", ")];
        if let Some(found) = self.start.installation() {
            for client in found
                .clients
                .iter()
                .filter(|c| w.flavors.contains(&c.flavor))
            {
                items.push(format!("{} · {}", client.client, client.folder));
            }
        }
        items.retain(|i| !i.is_empty());
        items
    }

    /// `118 fps · 0.21 ms`: the frames drawn in the last second and the
    /// last one's time. Studio draws only when something changes, so it
    /// is 1 fps while idle: the counter wakes the window once a second.
    fn frame_stats(&mut self, ctx: &Context) -> Option<String> {
        if !self.settings.frame_stats {
            self.frame_times.clear();
            return None;
        }
        let now = ctx.input(|i| i.time);
        self.frame_times.push_back(now);
        while self.frame_times.front().is_some_and(|t| now - t > 1.0) {
            self.frame_times.pop_front();
        }
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
        let fps = self.frame_times.len();
        Some(match self.frame_cpu {
            Some(cpu) => format!("{fps} fps · {:.2} ms", cpu * 1000.0),
            None => format!("{fps} fps"),
        })
    }

    fn set_title(&mut self, ctx: &Context) {
        let title = match &self.workspace {
            Some(w) => format!("{} · wowlua studio", w.name),
            None => "wowlua studio".to_owned(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn run(&mut self, ctx: &Context, command: Command) {
        match command {
            Command::ShowPage(page) => {
                if page == Page::Settings && self.page != Page::Settings {
                    self.settings_page.opened();
                }
                self.page = page;
                if self.settings.follow_presets {
                    self.layout_page = page;
                }
                self.dock.maximized = None;
            }
            Command::SetTheme(theme) => {
                wowlua_studio_ui::set_theme(ctx, theme);
                self.settings.theme = theme.name().to_owned();
                self.touch(ctx);
            }
            Command::OpenPalette => {
                if self.palette.open {
                    self.palette.open = false;
                } else {
                    self.palette.open(palette::Mode::Commands);
                }
            }
            Command::ResetLayout => {
                let key = self.layout_key();
                *layout_for(&mut self.layouts, &key, self.layout_page) =
                    Layout::preset(self.layout_page);
                self.dock.maximized = None;
                self.touch(ctx);
            }
            Command::MovePanel => {
                let key = self.layout_key();
                self.dock.open_menu = self.dock.focused.or_else(|| {
                    let layout = layout_for(&mut self.layouts, &key, self.layout_page);
                    layout
                        .main
                        .first()
                        .map(|g| g.tabs[g.active.min(g.tabs.len() - 1)])
                });
            }
            Command::ShowPanel(panel, home) => {
                let key = self.layout_key();
                let layout = layout_for(&mut self.layouts, &key, self.layout_page);
                if layout.floating.iter().any(|f| f.panel == panel) {
                    layout.place(panel, Target::Dock(home));
                } else {
                    layout.reveal(panel, home);
                }
                self.dock.focused = Some(panel);
                self.touch(ctx);
            }
            Command::FollowPresets(on) => {
                self.settings.follow_presets = on;
                if on {
                    self.layout_page = self.page;
                }
                self.menus_changed = true;
                self.touch(ctx);
            }
            Command::FrameStats(on) => {
                self.settings.frame_stats = on;
                self.menus_changed = true;
                self.touch(ctx);
            }
            Command::OpenFolder => self.pick_and_open(),
            Command::NewWindow => self.host.system.open_window(None),
            Command::CloseFolder => {
                if self.editors.any_dirty() {
                    self.pending = Some(Pending::CloseFolder);
                } else {
                    self.close_workspace();
                }
            }
            Command::Save => {
                let i = self.editors.active;
                self.save_tab(i, ctx);
            }
            Command::CloseEditor => {
                let i = self.editors.active;
                self.close_tab(i);
            }
            Command::RunAddon => {
                let config = self.chosen_config();
                self.start_session(config);
                self.queue(Command::ShowPage(Page::Game));
            }
            Command::StopSession => {
                self.host.system.game_stop();
            }
            Command::StartDebugging => {
                if self.debug.stopped.is_some() {
                    self.debug
                        .send(&*self.host.system, system::GameRequest::Continue);
                } else if !self.debug.running() {
                    let config = self.chosen_config();
                    self.start_session(config);
                    self.queue(Command::ShowPage(Page::Debug));
                }
            }
            Command::Pause => {
                self.debug
                    .send(&*self.host.system, system::GameRequest::Pause);
            }
            Command::StepOver | Command::StepInto | Command::StepOut => {
                if self.debug.stopped.is_some() {
                    let step = match command {
                        Command::StepOver => system::GameRequest::Next,
                        Command::StepInto => system::GameRequest::StepIn,
                        _ => system::GameRequest::StepOut,
                    };
                    self.debug.send(&*self.host.system, step);
                }
            }
            Command::RestartSession => {
                if let Some(config) = self.debug.config.clone() {
                    self.start_session(config);
                }
            }
            Command::RunAllTests | Command::RunFailedTests => {
                let filters = if command == Command::RunFailedTests {
                    self.tests.failed_filters()
                } else {
                    Vec::new()
                };
                if command == Command::RunAllTests || !filters.is_empty() {
                    let request = system::TestRequest {
                        filters,
                        coverage: self.tests.coverage_on,
                        update_snapshots: false,
                        profile: self.tests.profile.clone(),
                        profiler: false,
                    };
                    self.start_tests(request, ctx);
                }
            }
            Command::Find | Command::Replace => {
                let editor::Editors {
                    tabs,
                    active,
                    finder,
                    ..
                } = &mut self.editors;
                finder.show(tabs.get(*active), command == Command::Replace);
            }
            Command::FindNext | Command::FindPrevious => {
                let editor::Editors {
                    tabs,
                    active,
                    finder,
                    ..
                } = &mut self.editors;
                if finder.search.query.is_empty() {
                    finder.show(tabs.get(*active), false);
                } else if let Some(tab) = tabs.get_mut(*active) {
                    let forward = command == Command::FindNext;
                    editor::find::step(tab, finder, &*self.host.system, forward);
                }
            }
            Command::NewAddon => {
                if self.workspace.is_none() {
                    self.start.panel = start::Panel::New;
                }
            }
            Command::AdoptAddon => {
                if self.workspace.is_none() {
                    self.start.panel = start::Panel::Adopt;
                }
            }
            Command::OpenDocs => ctx.open_url(egui::OpenUrl::new_tab("https://wowlua.dev")),
            Command::About => self.about = true,
            Command::Quit => ctx.send_viewport_cmd(ViewportCommand::Close),
        }
    }

    /// The key of the open workspace's layouts.
    /// Whose layouts: the workspace's, or one for all of them (the
    /// setting "keep layouts per workspace", S062).
    fn layout_key(&self) -> String {
        if !self.settings.layouts_per_workspace {
            return String::new();
        }
        self.workspace
            .as_ref()
            .map(|w| w.root.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// Opening another folder (the design's open-another-folder board).
    fn ask_dialog(&mut self, ctx: &Context) {
        let Some(ask) = &mut self.ask else {
            return;
        };
        let name = self
            .workspace
            .as_ref()
            .map_or("", |w| w.name.as_str())
            .to_owned();
        let folder = recents::tilde(&ask.path, self.home.as_deref());
        let p = colors(ctx);
        let mut decided = None;
        let modal = egui::Modal::new(egui::Id::unique("wowlua-studio-open-ask")).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.spacing_mut().item_spacing.y = 14.0;
            let (font, spacing) = widgets::label_font(12.0);
            let title = widgets::galley(ui, &format!("open {folder}"), font, p.ink, spacing);
            let (r, _) = ui.allocate_exact_size(title.size(), egui::Sense::hover());
            ui.painter().galley(r.min, title, p.ink);
            forms::paragraph(
                ui,
                &format!(
                    "This window has the {name} workspace open. A window holds one workspace: its layout, its session and its title are its own."
                ),
                13.0,
                21.0,
                p.ink,
            );
            ui.spacing_mut().item_spacing.y = 8.0;
            if choice(ui, "in a new window", &format!("{name} stays open here."), ask.new_window) {
                ask.new_window = true;
            }
            if choice(
                ui,
                "in this window",
                &format!("{name} closes here. Its layout is kept for the next time you open it."),
                !ask.new_window,
            ) {
                ask.new_window = false;
            }
            ui.add_space(6.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                if widgets::button(ui, ButtonKind::Primary, "open", true, None).clicked() {
                    decided = Some(true);
                }
                if widgets::button(ui, ButtonKind::Ghost, "cancel", true, None).clicked() {
                    decided = Some(false);
                }
            });
        });
        if modal.should_close() {
            decided = decided.or(Some(false));
        }
        match decided {
            Some(true) => {
                let ask = self.ask.take().expect("asked");
                if ask.new_window {
                    self.host.system.open_window(Some(&ask.path));
                    let opened = ask
                        .path
                        .file_name()
                        .map_or_else(|| folder.clone(), |n| n.to_string_lossy().into_owned());
                    self.notify(
                        Notification::new(
                            Kind::Note,
                            format!("{opened} opened in a new window"),
                            "",
                        ),
                        ctx,
                    );
                } else if self.editors.any_dirty() {
                    self.pending = Some(Pending::OpenHere(ask.path));
                } else {
                    self.open(&ask.path);
                }
            }
            Some(false) => self.ask = None,
            None => {}
        }
    }

    /// Unsaved edits before closing a tab, the folder or the window, or
    /// opening another folder here: save, don't save, cancel.
    fn unsaved_dialog(&mut self, ctx: &Context) {
        let Some(pending) = self.pending.clone() else {
            return;
        };
        let tabs: Vec<usize> = match pending {
            Pending::CloseTab(i) => vec![i],
            _ => (0..self.editors.tabs.len())
                .filter(|&i| self.editors.tabs[i].doc.is_dirty())
                .collect(),
        };
        let names: Vec<String> = tabs
            .iter()
            .filter_map(|&i| self.editors.tabs.get(i))
            .map(|t| {
                t.doc
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            })
            .collect();
        let p = colors(ctx);
        let mut decided = None;
        let modal = egui::Modal::new(egui::Id::unique("wowlua-studio-unsaved")).show(ctx, |ui| {
            ui.set_width(480.0);
            ui.spacing_mut().item_spacing.y = 14.0;
            let (font, spacing) = widgets::label_font(12.0);
            let title = widgets::galley(ui, "unsaved edits", font, p.ink, spacing);
            let (r, _) = ui.allocate_exact_size(title.size(), egui::Sense::hover());
            ui.painter().galley(r.min, title, p.ink);
            let what = match pending {
                Pending::CloseTab(_) => "Closing it",
                Pending::CloseFolder => "Closing the folder",
                Pending::OpenHere(_) => "Opening another folder here",
                Pending::Quit => "Closing the window",
            };
            let text = if names.len() == 1 {
                format!(
                    "{} has unsaved edits. {what} drops them unless you save.",
                    names[0]
                )
            } else {
                format!(
                    "{} files have unsaved edits: {}. {what} drops them unless you save.",
                    names.len(),
                    names.join(", ")
                )
            };
            forms::paragraph(ui, &text, 13.0, 21.0, p.ink);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                if widgets::button(ui, ButtonKind::Primary, "save", true, None).clicked() {
                    decided = Some(true);
                }
                if widgets::button(ui, ButtonKind::Ghost, "don't save", true, None).clicked() {
                    decided = Some(false);
                }
                if widgets::button(ui, ButtonKind::Ghost, "cancel", true, None).clicked() {
                    self.pending = None;
                }
            });
        });
        if modal.should_close() {
            self.pending = None;
        }
        let Some(save) = decided else {
            return;
        };
        self.pending = None;
        if save {
            for &i in &tabs {
                match self.editors.save(i, &*self.host.system) {
                    // The disk changed: its banner asks first (S049).
                    Ok(false) => {
                        self.editors.active = i;
                        return;
                    }
                    Ok(true) => {}
                    Err(error) => {
                        self.notify(
                            Notification::new(Kind::Error, "The file was not saved", error),
                            ctx,
                        );
                        return;
                    }
                }
            }
        }
        match pending {
            Pending::CloseTab(i) => self.editors.close(i),
            Pending::CloseFolder => self.close_workspace(),
            Pending::OpenHere(path) => {
                self.editors = Editors::default();
                self.open(&path);
            }
            Pending::Quit => {
                self.quitting = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
    }

    fn about_dialog(&mut self, ctx: &Context) {
        let p = colors(ctx);
        let modal = egui::Modal::new(egui::Id::unique("wowlua-studio-about")).show(ctx, |ui| {
            ui.set_width(360.0);
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(48.0), egui::Sense::hover());
            mark::paint_mark(ctx, ui.painter(), rect, p);
            ui.add_space(8.0);
            ui.label(egui::RichText::new("wowlua studio").font(Face::MartianBold.at(20.0)).color(p.ink));
            ui.label(
                egui::RichText::new(format!(
                    "{} · wowlua {} built in",
                    env!("CARGO_PKG_VERSION"),
                    self.host.wowlua_version
                ))
                .color(p.ink_muted),
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(
                    "MIT licence. Martian Mono and IBM Plex Mono under the SIL Open Font License 1.1.",
                )
                .font(Face::Plex.at(11.5))
                .color(p.ink_muted),
            );
            ui.add_space(12.0);
            widgets::button(ui, ButtonKind::Secondary, "close", true, None).clicked()
        });
        if modal.inner || modal.should_close() {
            self.about = false;
        }
    }

    /// Marks the settings and layouts as changed: they are saved a second
    /// after the last change.
    fn touch(&mut self, ctx: &Context) {
        self.unsaved_since = Some(ctx.input(|i| i.time));
        ctx.request_repaint_after(std::time::Duration::from_millis(1100));
    }

    fn save_when_due(&mut self, ctx: &Context) {
        let Some(since) = self.unsaved_since else {
            return;
        };
        if ctx.input(|i| i.time) - since < 1.0 {
            return;
        }
        self.save();
    }

    /// Writes the settings and layouts now.
    pub fn save(&mut self) {
        self.unsaved_since = None;
        self.host
            .store
            .save(SETTINGS_FILE, &state::to_text(&self.settings));
        self.host
            .store
            .save(LAYOUTS_FILE, &state::to_text(&self.layouts));
    }

    /// The current theme.
    pub fn theme(&self) -> Theme {
        self.settings.theme()
    }
}

/// One of the open-another-folder dialog's two choices (`ow-choice`);
/// returns whether it was clicked.
fn choice(ui: &mut Ui, title: &str, caption: &str, on: bool) -> bool {
    let p = colors(ui.ctx());
    let width = ui.available_width();
    let (r, response) = ui.allocate_exact_size(Vec2::new(width, 56.0), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, on, title));
    let painter = ui.painter();
    painter.rect_filled(
        r,
        egui::CornerRadius::ZERO,
        if response.hovered() {
            p.bg_300
        } else {
            p.bg_100
        },
    );
    let (border, w) = if on {
        (p.amber_ink, 2.0)
    } else {
        (p.line, 1.0)
    };
    painter.rect_stroke(
        r,
        egui::CornerRadius::ZERO,
        egui::Stroke::new(w, border),
        egui::StrokeKind::Inside,
    );
    painter.text(
        Pos2::new(r.left() + 14.0, r.top() + 12.0 + 9.0),
        egui::Align2::LEFT_CENTER,
        title,
        Face::PlexSemiBold.at(12.5),
        p.ink,
    );
    painter.text(
        Pos2::new(r.left() + 14.0, r.top() + 12.0 + 18.0 + 4.0 + 8.0),
        egui::Align2::LEFT_CENTER,
        caption,
        Face::Plex.at(11.5),
        p.ink_muted,
    );
    widgets::focus_ring(ui, r, &response);
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// The word around `col` (letters, digits, `_`).
fn word_bounds(line: &str, col: usize) -> (usize, usize) {
    let chars: Vec<char> = line.chars().collect();
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let mut start = col.min(chars.len());
    while start > 0 && word(chars[start - 1]) {
        start -= 1;
    }
    let mut end = col.min(chars.len());
    while end < chars.len() && word(chars[end]) {
        end += 1;
    }
    (start, end)
}

fn layout_for<'a>(layouts: &'a mut Layouts, workspace: &str, page: Page) -> &'a mut Layout {
    layouts
        .workspaces
        .entry(workspace.to_owned())
        .or_default()
        .entry(page)
        .or_insert_with(|| Layout::preset(page))
}
