//! wowlua studio's application: the window's chrome, docking, the command
//! palette and the pages, drawn with egui. It knows nothing of the system
//! it runs on: the host gives it the platform, wowlua's version, a place
//! to keep its files and the system ([`system::System`]), so it builds for
//! the browser too (S002).

pub mod chrome;
pub mod commands;
pub mod dock;
pub mod editor;
pub mod explorer;
pub mod jobs;
pub mod lsp;
pub mod notify;
pub mod pages;
pub mod palette;
pub mod panels;
pub mod problems;
pub mod recents;
pub mod search;
pub mod start;
pub mod state;
pub mod system;
pub mod terminal;

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
        commands::entries(
            self.host.platform,
            self.settings.follow_presets,
            self.settings.frame_stats,
            self.workspace.is_some(),
        )
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

        // Closing the window with unsaved edits asks first.
        if ctx.input(|i| i.viewport().close_requested())
            && !self.quitting
            && self.editors.any_dirty()
        {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.pending = Some(Pending::Quit);
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
        }
        self.lsp
            .sync(self.editors.tabs.iter().map(|t| &t.doc), &*self.host.system);

        let entries = self.entries();
        let mut commands = std::mem::take(&mut self.queued);
        if !self.palette.open && self.ask.is_none() && self.pending.is_none() {
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
        let key = workspace.root.to_string_lossy().into_owned();
        let layout = layout_for(&mut self.layouts, &key, self.layout_page);
        let explorer = &mut self.explorer;
        let editors = &mut self.editors;
        let lsp_state = &self.lsp;
        let search_state = &mut self.search;
        let term_state = &mut self.terminal;
        let problems_this_file = &mut self.problems_this_file;
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
                let cx = editor::view::Cx {
                    system,
                    workspace,
                    mac,
                    diagnostics,
                };
                actions.extend(editor::view::show(ui, editors, &cx));
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
            _ => dock::placeholder(ui, panel),
        };
        let changed = dock::show(ui, area, layout, &mut self.dock, &mut content);
        if changed {
            self.touch(ctx);
        }
        if let Some(path) = opened {
            self.open_file(&path, ctx);
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
            A::Hover(at) => self.lsp.hover(doc, at, system),
            A::Complete(at) => self.lsp.complete(doc, at, system),
            A::Signature(at) => self.lsp.signature(doc, at, system),
            A::Definition(at) => self.lsp.definition(doc, at, system),
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
                    self.editors.tabs[i].completion = Some(editor::Completion {
                        at,
                        items,
                        selected: 0,
                        incomplete,
                    });
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
                    self.lsp.decorations(&tab.doc, &*self.host.system);
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
        // Format on save: the server's formatting first, then the write.
        if self.lsp.ready
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
            Ok(true) => self.lsp.saved(&path, &*self.host.system),
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
                self.lsp.start(&workspace.root, &*self.host.system);
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
        self.branch = self.host.system.branch(&workspace.root);
        self.reload_recents();
        self.recents.opened(&workspace, self.host.system.now());
        self.save_recents();
        self.workspace = Some(workspace);
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
    fn layout_key(&self) -> String {
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
