//! wowlua studio's application: the window's chrome, docking, the command
//! palette and the pages, drawn with egui. It knows nothing of the system
//! it runs on: the host gives it the platform, wowlua's version and a
//! place to keep its files, so it builds for the browser too (S002).

pub mod chrome;
pub mod commands;
pub mod dock;
pub mod pages;
pub mod palette;
pub mod panels;
pub mod state;

use egui::{Context, Rect, Ui, Vec2, ViewportCommand};
use wowlua_studio_ui::{Face, Theme, mark, palette as colors};

use commands::{Command, Entry};
use dock::{DockState, Layout, Target};
use pages::Page;
use palette::PaletteState;
use state::{LAYOUTS_FILE, Layouts, SETTINGS_FILE, Settings};

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
}

/// Studio's whole state.
pub struct Studio {
    host: Host,
    settings: Settings,
    layouts: Layouts,
    /// The page shown.
    page: Page,
    /// The page whose layout is shown: the page itself, unless layouts do
    /// not follow pages.
    layout_page: Page,
    dock: DockState,
    palette: PaletteState,
    about: bool,
    /// Commands from outside the frame (macOS's menu bar).
    queued: Vec<Command>,
    /// When the layouts or settings changed and are not saved yet.
    unsaved_since: Option<f64>,
}

/// The key of the layouts while no workspace is open (M2 opens them).
const NO_WORKSPACE: &str = "";

impl Studio {
    pub fn new(ctx: &Context, host: Host) -> Self {
        let settings: Settings = state::parse(host.store.load(SETTINGS_FILE));
        let layouts: Layouts = state::parse(host.store.load(LAYOUTS_FILE));
        wowlua_studio_ui::install(ctx, settings.theme());
        Studio {
            host,
            settings,
            layouts,
            page: Page::Files,
            layout_page: Page::Files,
            dock: DockState::default(),
            palette: PaletteState::default(),
            about: false,
            queued: Vec::new(),
            unsaved_since: None,
        }
    }

    pub fn platform(&self) -> Platform {
        self.host.platform
    }

    /// Every command, as the menus list them.
    pub fn entries(&self) -> Vec<Entry> {
        commands::entries(self.host.platform, self.settings.follow_presets)
    }

    /// Runs a command at the next frame (the macOS menu bar's).
    pub fn queue(&mut self, command: Command) {
        self.queued.push(command);
    }

    /// Draws a frame.
    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let entries = self.entries();
        let mut commands = std::mem::take(&mut self.queued);
        if !self.palette.open {
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

        let full = ui.max_rect();
        let title = Rect::from_min_size(full.min, Vec2::new(full.width(), chrome::TITLE_H));
        let status = Rect::from_min_max(
            egui::pos2(full.left(), full.bottom() - chrome::STATUS_H),
            full.max,
        );
        let rail = Rect::from_min_max(
            egui::pos2(full.left(), title.bottom()),
            egui::pos2(full.left() + chrome::RAIL_W, status.top()),
        );
        let area = Rect::from_min_max(
            egui::pos2(rail.right(), title.bottom()),
            egui::pos2(full.right(), status.top()),
        );

        chrome::title_bar(ui, title, self.host.platform, &entries, &mut commands);
        chrome::rail(ui, rail, self.page, &mut commands);
        chrome::status_bar(ui, status, &self.host.wowlua_version);

        let layout = layout_for(&mut self.layouts, self.layout_page);
        let changed = dock::show(ui, area, layout, &mut self.dock, &mut dock::placeholder);
        if changed {
            self.touch(&ctx);
        }

        if let Some(c) = palette::show(ui, &mut self.palette, &entries, self.host.platform) {
            commands.push(c);
        }
        if self.about {
            self.about_dialog(&ctx);
        }
        if self.host.platform != Platform::MacOs && self.host.platform != Platform::Web {
            chrome::resize_edges(ui, full);
        }

        for command in commands {
            self.run(&ctx, command);
        }
        self.save_when_due(&ctx);
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
                *layout_for(&mut self.layouts, self.layout_page) = Layout::preset(self.layout_page);
                self.dock.maximized = None;
                self.touch(ctx);
            }
            Command::MovePanel => {
                self.dock.open_menu = self.dock.focused.or_else(|| {
                    let layout = layout_for(&mut self.layouts, self.layout_page);
                    layout
                        .main
                        .first()
                        .map(|g| g.tabs[g.active.min(g.tabs.len() - 1)])
                });
            }
            Command::ShowPanel(panel, home) => {
                let layout = layout_for(&mut self.layouts, self.layout_page);
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
                self.touch(ctx);
            }
            Command::OpenDocs => ctx.open_url(egui::OpenUrl::new_tab("https://wowlua.dev")),
            Command::About => self.about = true,
            Command::Quit => ctx.send_viewport_cmd(ViewportCommand::Close),
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
            wowlua_studio_ui::widgets::button(ui, wowlua_studio_ui::widgets::ButtonKind::Secondary, "close", true, None)
                .clicked()
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

fn layout_for(layouts: &mut Layouts, page: Page) -> &mut Layout {
    layouts
        .workspaces
        .entry(NO_WORKSPACE.to_owned())
        .or_default()
        .entry(page)
        .or_insert_with(|| Layout::preset(page))
}
