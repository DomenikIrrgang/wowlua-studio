//! wowlua studio's application: the window's chrome, docking, the command
//! palette and the pages, drawn with egui. It knows nothing of the system
//! it runs on: the host gives it the platform, wowlua's version, a place
//! to keep its files and the system ([`system::System`]), so it builds for
//! the browser too (S002).

pub mod chrome;
pub mod commands;
pub mod dock;
pub mod explorer;
pub mod pages;
pub mod palette;
pub mod panels;
pub mod recents;
pub mod start;
pub mod state;
pub mod system;

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use egui::{Context, Pos2, Rect, Ui, UiBuilder, Vec2, ViewportCommand};
use wowlua_studio_ui::forms::{self, CalloutKind};
use wowlua_studio_ui::widgets::{self, ButtonKind};
use wowlua_studio_ui::{Face, Theme, mark, palette as colors};

use commands::{Command, Entry};
use dock::{DockState, Layout, Target};
use explorer::Explorer;
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
    /// A note over the window: what just happened.
    notice: Option<(CalloutKind, String)>,
    /// Commands from outside the frame (macOS's menu bar).
    queued: Vec<Command>,
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
            notice: None,
            queued: Vec::new(),
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

        let entries = self.entries();
        let mut commands = std::mem::take(&mut self.queued);
        if !self.palette.open && self.ask.is_none() {
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
        let session = chrome::Session {
            text: match (&self.workspace, &checks_off) {
                (None, _) => "no folder open",
                (Some(_), Some(text)) => text,
                (Some(_), None) => "",
            },
            warning: checks_off.is_some(),
            right: &right,
        };
        chrome::status_bar(ui, status, &self.host.wowlua_version, &session);

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
        self.notice(
            ui,
            Rect::from_min_max(egui::pos2(full.left(), area_top), full.max),
        );

        if let Some(c) = palette::show(ui, &mut self.palette, &entries, self.host.platform) {
            commands.push(c);
        }
        if self.about {
            self.about_dialog(&ctx);
        }
        if self.ask.is_some() {
            self.ask_dialog(&ctx);
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
        let system = &*self.host.system;
        let mut content = |ui: &mut Ui, panel: Panel| match panel {
            Panel::Explorer => explorer.show(ui, workspace, system),
            _ => dock::placeholder(ui, panel),
        };
        let changed = dock::show(ui, area, layout, &mut self.dock, &mut content);
        if changed {
            self.touch(ctx);
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
                        self.notice = Some((
                            CalloutKind::Error,
                            format!("{folder} did not open. {message}"),
                        ));
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
                    self.notice = Some((CalloutKind::Ok, lines.join("\n")));
                    self.open(&dir);
                }
                Event::Adopted(Err(error)) => {
                    self.start.adopt.adopting = false;
                    self.start.adopt.error = Some(error);
                }
                Event::FilesChanged(paths) => {
                    self.explorer.changed(&paths);
                    // A changed manifest can change the members: the
                    // workspace is read again.
                    let manifest = paths
                        .iter()
                        .any(|p| p.file_name().is_some_and(|n| n == "wowlua.json"));
                    if manifest && let Some(w) = &self.workspace {
                        self.host.system.open(&w.root);
                    }
                }
                Event::Installation(_) | Event::Ui { .. } => {}
            }
            ctx.request_repaint();
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
            Command::CloseFolder => self.close_workspace(),
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

    /// The note over the window's top, until it is dismissed.
    fn notice(&mut self, ui: &mut Ui, area: Rect) {
        let Some((kind, text)) = &self.notice else {
            return;
        };
        let width = 560.0f32.min(area.width() - 32.0);
        let rect = Rect::from_min_size(
            Pos2::new(area.center().x - width / 2.0, area.top() + 16.0),
            Vec2::new(width, 0.0),
        );
        let mut dismissed = false;
        egui::Area::new(egui::Id::unique("wowlua-studio-notice"))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .show(ui.ctx(), |ui| {
                ui.set_width(width);
                let p = colors(ui.ctx());
                forms::callout(ui, *kind, |ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    for line in text.lines() {
                        forms::paragraph(ui, line, 12.0, 18.0, p.ink);
                    }
                    dismissed = forms::text_button(ui, "dismiss").clicked();
                });
            });
        if dismissed {
            self.notice = None;
        }
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
                    self.notice =
                        Some((CalloutKind::Ok, format!("{opened} opens in a new window.")));
                } else {
                    self.open(&ask.path);
                }
            }
            Some(false) => self.ask = None,
            None => {}
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

fn layout_for<'a>(layouts: &'a mut Layouts, workspace: &str, page: Page) -> &'a mut Layout {
    layouts
        .workspaces
        .entry(workspace.to_owned())
        .or_default()
        .entry(page)
        .or_insert_with(|| Layout::preset(page))
}
