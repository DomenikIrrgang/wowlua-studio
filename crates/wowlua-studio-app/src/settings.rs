//! The settings page (M10, S062): Studio's own settings on this computer
//! by section, found by name; `wowlua.json` per package; a package's
//! dependencies. A setting changes as it is set and is kept with the
//! others (`settings.json` in Studio's data).

use std::path::PathBuf;

use egui::{CornerRadius, Sense, Ui, Vec2};
use wowlua_studio_ui::forms::{self, CalloutKind};
use wowlua_studio_ui::widgets::{self, BadgeKind, ButtonKind};
use wowlua_studio_ui::{Face, Palette, palette};

use crate::deps::DepsState;
use crate::state::Settings;
use crate::system::{GithubSignIn, Installation, SettingsInfo, System, UpdateCheck, Workspace};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Section {
    Appearance,
    Editor,
    Completion,
    LanguageServer,
    #[default]
    Debugger,
    GameScreen,
    Installation,
    Github,
    Claude,
    Keybindings,
    Layouts,
    About,
}

impl Section {
    pub const ALL: [Section; 12] = [
        Section::Appearance,
        Section::Editor,
        Section::Completion,
        Section::LanguageServer,
        Section::Debugger,
        Section::GameScreen,
        Section::Installation,
        Section::Github,
        Section::Claude,
        Section::Keybindings,
        Section::Layouts,
        Section::About,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Section::Appearance => "appearance",
            Section::Editor => "editor",
            Section::Completion => "completion",
            Section::LanguageServer => "language server",
            Section::Debugger => "debugger",
            Section::GameScreen => "game screen",
            Section::Installation => "installation",
            Section::Github => "github",
            Section::Claude => "claude",
            Section::Keybindings => "keybindings",
            Section::Layouts => "layouts",
            Section::About => "about",
        }
    }
}

/// What the page shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Studio(Section),
    /// `wowlua.json`'s section by its index.
    Manifest(usize),
    Dependencies,
}

impl Default for View {
    fn default() -> Self {
        View::Studio(Section::default())
    }
}

/// What the page asks of the rest of Studio.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// A setting changed: keep the settings and apply them.
    Changed,
    PickInstallation,
    /// Reset every page's layout to its preset.
    ResetLayouts,
    OpenLog,
}

#[derive(Default)]
pub struct SettingsState {
    pub view: View,
    pub query: String,
    pub deps: DepsState,
    info: Option<SettingsInfo>,
    asked: bool,
    updates: Option<UpdateCheck>,
    checking_updates: bool,
    github: Option<GithubSignIn>,
    claude_key: String,
    /// The wowlua built into Studio (the host's).
    pub wowlua_version: String,
    /// Text being typed into a text setting, by its label.
    buffers: std::collections::HashMap<&'static str, String>,
}

impl SettingsState {
    pub fn info(&mut self, info: SettingsInfo) {
        self.info = Some(info);
    }

    pub fn updates(&mut self, check: UpdateCheck) {
        self.checking_updates = false;
        self.updates = Some(check);
    }

    pub fn github(&mut self, event: GithubSignIn) {
        if matches!(event, GithubSignIn::SignedIn(_)) {
            self.asked = false;
        }
        self.github = Some(event);
    }

    /// The page was opened: what it shows of this computer is read again.
    pub fn opened(&mut self) {
        self.asked = false;
        self.deps.opened();
    }
}

/// How a setting is set.
enum Kind {
    Flag(fn(&Settings) -> bool, fn(&mut Settings, bool)),
    Choice(
        &'static [&'static str],
        fn(&Settings) -> String,
        fn(&mut Settings, &str),
    ),
    Text(fn(&Settings) -> String, fn(&mut Settings, &str)),
    /// A value of this computer and its button.
    Action(Act),
    /// A key to save, never shown again.
    Secret,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Act {
    Installation,
    Wowlua,
    BlizzardUi,
    CheckCache,
    ArtCache,
    Github,
    Drafts,
    Version,
    Log,
    ResetLayouts,
}

struct Row {
    section: Section,
    label: &'static str,
    desc: &'static str,
    kind: Kind,
}

fn rows() -> Vec<Row> {
    use Kind::*;
    use Section::*;
    let row = |section, label, desc, kind| Row {
        section,
        label,
        desc,
        kind,
    };
    vec![
        row(
            Appearance,
            "theme",
            "wowlua's dark and light themes; system follows the OS. Also in the command palette: > theme.",
            Choice(
                &["dark", "light", "system"],
                |s| s.theme.clone(),
                |s, v| s.theme = v.into(),
            ),
        ),
        row(
            Appearance,
            "interface size",
            "Studio draws its own interface on the GPU: one scale for all of it, in percent.",
            Text(
                |s| format!("{}%", s.interface_size),
                |s, v| {
                    if let Ok(n) = v.trim().trim_end_matches('%').trim().parse::<u32>() {
                        s.interface_size = n.clamp(50, 200);
                    }
                },
            ),
        ),
        row(
            Appearance,
            "density",
            "Row height of trees, tables and lists.",
            Choice(
                &["compact", "comfortable"],
                |s| s.density.clone(),
                |s, v| s.density = v.into(),
            ),
        ),
        row(
            Appearance,
            "reduce motion",
            "No blinking caret, no animated panels.",
            Flag(|s| s.reduce_motion, |s, v| s.reduce_motion = v),
        ),
        row(
            Editor,
            "font size",
            "IBM Plex Mono, as everywhere in wowlua.",
            Text(
                |s| format!("{}", s.font_size),
                |s, v| {
                    if let Ok(n) = v.trim().parse::<f32>() {
                        s.font_size = n.clamp(8.0, 32.0);
                    }
                },
            ),
        ),
        row(
            Editor,
            "format on save",
            "wowlua fmt with the package's fmt settings.",
            Flag(|s| s.format_on_save, |s, v| s.format_on_save = v),
        ),
        row(
            Editor,
            "inlay hints",
            "Inferred types after names, as in : number | Secret<number>?",
            Flag(|s| s.inlay_hints, |s, v| s.inlay_hints = v),
        ),
        row(
            Editor,
            "code lenses",
            "References, tests and debug above functions and tests.",
            Flag(|s| s.code_lenses, |s, v| s.code_lenses = v),
        ),
        row(
            Editor,
            "coverage in the gutter",
            "Lines the last coverage run never ran.",
            Flag(|s| s.coverage_gutter, |s, v| s.coverage_gutter = v),
        ),
        row(
            Editor,
            "word wrap",
            "Long lines wrap at the package's fmt line width or at the window's edge.",
            Choice(
                &["off", "at the line width", "at the window"],
                |s| match s.word_wrap.as_str() {
                    "line width" => "at the line width".into(),
                    "window" => "at the window".into(),
                    _ => "off".into(),
                },
                |s, v| {
                    s.word_wrap = match v {
                        "at the line width" => "line width".into(),
                        "at the window" => "window".into(),
                        _ => "off".into(),
                    }
                },
            ),
        ),
        row(
            Completion,
            "suggest while typing",
            "Locals, members, imports, events, anchors, frame types, decorators.",
            Flag(
                |s| s.suggest_while_typing,
                |s, v| s.suggest_while_typing = v,
            ),
        ),
        row(
            Completion,
            "accept with",
            "",
            Choice(
                &["tab", "enter", "tab and enter"],
                |s| s.accept_with.clone(),
                |s, v| s.accept_with = v.into(),
            ),
        ),
        row(
            Completion,
            "add imports",
            "Completing an export of another file adds its import.",
            Flag(|s| s.add_imports, |s, v| s.add_imports = v),
        ),
        row(
            Completion,
            "documentation beside the list",
            "The game API's documentation for your flavor, and your doc comments.",
            Flag(|s| s.completion_docs, |s, v| s.completion_docs = v),
        ),
        row(
            Completion,
            "signature help",
            "Parameters as you type a call.",
            Flag(|s| s.signature_help, |s, v| s.signature_help = v),
        ),
        row(
            LanguageServer,
            "check",
            "The open file and its imports first, then the workspace.",
            Choice(
                &["while typing", "when you pause", "on save"],
                |s| s.check_when.clone(),
                |s, v| s.check_when = v.into(),
            ),
        ),
        row(
            LanguageServer,
            "worker threads",
            "For per-file work; 1: none.",
            Text(
                |s| {
                    if s.check_threads == 0 {
                        "all cores".into()
                    } else {
                        s.check_threads.to_string()
                    }
                },
                |s, v| {
                    s.check_threads = v.trim().parse::<u32>().unwrap_or(0);
                },
            ),
        ),
        row(
            LanguageServer,
            "check with profile",
            "Diagnostics follow a build profile.",
            Choice(
                &["dev", "test", "release"],
                |s| s.check_profile.clone(),
                |s, v| s.check_profile = v.into(),
            ),
        ),
        row(
            LanguageServer,
            "check cache",
            "The check cache, reused between runs.",
            Action(Act::CheckCache),
        ),
        row(
            Debugger,
            "default clock",
            "For new configurations. Manual: time moves only when you advance it.",
            Choice(
                &["manual", "realtime"],
                |s| s.default_clock.clone(),
                |s, v| s.default_clock = v.into(),
            ),
        ),
        row(
            Debugger,
            "stop on uncaught errors",
            "Errors that reach the error handler.",
            Flag(|s| s.stops_on("uncaught"), |s, v| stop(s, "uncaught", v)),
        ),
        row(
            Debugger,
            "stop on every error",
            "Caught ones too: pcall, xpcall, your own error handlers.",
            Flag(|s| s.stops_on("error"), |s, v| stop(s, "error", v)),
        ),
        row(
            Debugger,
            "stop on taint",
            "When secure execution becomes tainted.",
            Flag(|s| s.stops_on("taint"), |s, v| stop(s, "taint", v)),
        ),
        row(
            Debugger,
            "stop on blocked actions",
            "A protected action refused (ADDON_ACTION_BLOCKED, ADDON_ACTION_FORBIDDEN).",
            Flag(|s| s.stops_on("blocked"), |s, v| stop(s, "blocked", v)),
        ),
        row(
            Debugger,
            "step into Blizzard's UI",
            "Off: Blizzard's code is stepped over and never stopped in.",
            Flag(|s| s.step_into_blizzard, |s, v| s.step_into_blizzard = v),
        ),
        row(
            Debugger,
            "show dispatch frames",
            "Events, scripts, OnUpdate, timers and game frames in the call stack.",
            Flag(|s| s.show_dispatch, |s, v| s.show_dispatch = v),
        ),
        row(
            Debugger,
            "inline values",
            "Values beside the code while paused; secrets marked.",
            Flag(|s| s.inline_values, |s, v| s.inline_values = v),
        ),
        row(
            Debugger,
            "show what secrets hide",
            "Off: a secret shows only as secret, as the game does.",
            Flag(|s| s.reveal_secrets, |s, v| s.reveal_secrets = v),
        ),
        row(
            GameScreen,
            "screen size",
            "For sessions; tests use test.screen.",
            Choice(
                &["1920 × 1080", "2560 × 1440", "3840 × 2160"],
                |s| s.screen_size.clone(),
                |s, v| s.screen_size = v.into(),
            ),
        ),
        row(
            GameScreen,
            "use the installation's art",
            "Textures, atlases, fonts and models from your WoW folder; never shipped.",
            Flag(|s| s.installation_art, |s, v| s.installation_art = v),
        ),
        row(
            GameScreen,
            "answer from the installation's data",
            "Items, creatures, appearances where a profile defines nothing. Off: the same result on every machine.",
            Flag(|s| s.installation_data, |s, v| s.installation_data = v),
        ),
        row(
            GameScreen,
            "art cache",
            "Read once from the installation, then kept on disk.",
            Action(Act::ArtCache),
        ),
        row(
            Installation,
            "wow",
            "Where Studio reads art and installs AddOns, kept as wow.path in ~/.wowlua/config.json.",
            Action(Act::Installation),
        ),
        row(
            Installation,
            "wowlua",
            "The wowlua built into Studio: its checker, test runner, debugger and client.",
            Action(Act::Wowlua),
        ),
        row(
            Installation,
            "blizzard ui",
            "Blizzard's UI code for test.blizzard-ui.",
            Action(Act::BlizzardUi),
        ),
        row(
            Github,
            "account",
            "GitHub's device sign-in: a code to enter on github.com. The token is kept in the OS credential store.",
            Action(Act::Github),
        ),
        row(
            Github,
            "drafts",
            "Online workspaces keep your uncommitted edits here until you commit or discard them.",
            Action(Act::Drafts),
        ),
        row(
            Github,
            "load an online repository",
            "Opened files first; the rest follows so checks and tests see the whole package.",
            Choice(
                &["opened files first", "everything at once"],
                |s| s.github_load.clone(),
                |s, v| s.github_load = v.into(),
            ),
        ),
        row(
            Github,
            "watch branches on GitHub",
            "Every 5 minutes: a branch that moved under your drafts or folder says so.",
            Flag(|s| s.github_watch, |s, v| s.github_watch = v),
        ),
        row(
            Github,
            "notify when a run finishes",
            "For runs of your pushes and pull requests.",
            Flag(|s| s.github_notify, |s, v| s.github_notify = v),
        ),
        row(
            Claude,
            "api key",
            "Your Anthropic API key, kept in the OS credential store; Studio never shows it again.",
            Secret,
        ),
        row(
            Claude,
            "approvals",
            "What Claude may do without asking.",
            Choice(
                &[
                    "ask every time",
                    "edits ask, the rest runs",
                    "only reading runs",
                ],
                |s| s.claude_approvals.clone(),
                |s, v| s.claude_approvals = v.into(),
            ),
        ),
        row(
            Claude,
            "debug and game tools",
            "Start sessions, screenshots, clicks, world changes, breakpoints (wowlua mcp).",
            Flag(|s| s.claude_tools, |s, v| s.claude_tools = v),
        ),
        row(
            Claude,
            "run commands",
            "wowlua test, check, build; never publish without asking.",
            Flag(|s| s.claude_shell, |s, v| s.claude_shell = v),
        ),
        row(
            Layouts,
            "page presets",
            "Each page opens in its preset until you move a panel.",
            Flag(|s| s.follow_presets, |s, v| s.follow_presets = v),
        ),
        row(
            Layouts,
            "keep layouts per workspace",
            "Each workspace keeps its own panels.",
            Flag(
                |s| s.layouts_per_workspace,
                |s, v| s.layouts_per_workspace = v,
            ),
        ),
        row(
            Layouts,
            "floating panels as OS windows",
            "A panel dragged out becomes its own window, on any screen.",
            Flag(|s| s.float_windows, |s, v| s.float_windows = v),
        ),
        row(
            Layouts,
            "reset layouts",
            "Back to the presets for every page.",
            Action(Act::ResetLayouts),
        ),
        row(
            About,
            "version",
            "Studio and the wowlua built into it.",
            Action(Act::Version),
        ),
        row(
            About,
            "logs",
            "Studio's own log, for bug reports.",
            Action(Act::Log),
        ),
    ]
}

fn stop(s: &mut Settings, kind: &str, on: bool) {
    s.stop_on.retain(|k| k != kind);
    if on {
        s.stop_on.push(kind.into());
    }
}

/// The left panel: Studio's sections, `wowlua.json`'s, the dependencies.
pub fn sections(ui: &mut Ui, state: &mut SettingsState, manifest_sections: &[&str]) {
    let p = palette(ui.ctx());
    forms::padded(ui, 14, 6, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Studio")
                    .font(Face::PlexMedium.at(11.0))
                    .color(p.ink_muted),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new("this computer")
                        .font(Face::Plex.at(11.0))
                        .color(p.ink_faint),
                );
            });
        });
    });
    for section in Section::ALL {
        let current = state.query.trim().is_empty() && state.view == View::Studio(section);
        if nav(ui, section.label(), current, &p) {
            state.view = View::Studio(section);
            state.query.clear();
        }
    }
    ui.add_space(8.0);
    forms::padded(ui, 14, 6, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("wowlua.json")
                    .font(Face::PlexMedium.at(11.0))
                    .color(p.ink_muted),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new("per package")
                        .font(Face::Plex.at(11.0))
                        .color(p.ink_faint),
                );
            });
        });
    });
    for (i, name) in manifest_sections.iter().enumerate() {
        if nav(ui, name, state.view == View::Manifest(i), &p) {
            state.view = View::Manifest(i);
        }
    }
    ui.add_space(8.0);
    if nav(ui, "dependencies", state.view == View::Dependencies, &p) {
        state.view = View::Dependencies;
        state.deps.opened();
    }
}

fn nav(ui: &mut Ui, label: &str, current: bool, p: &Palette) -> bool {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 28.0), Sense::click());
    let painter = ui.painter();
    if current {
        painter.rect_filled(rect, CornerRadius::ZERO, p.bg_400);
        let bar = egui::Rect::from_min_size(rect.min, Vec2::new(2.0, rect.height()));
        painter.rect_filled(bar, CornerRadius::ZERO, p.amber);
    } else if response.hovered() {
        painter.rect_filled(rect, CornerRadius::ZERO, p.bg_300);
    }
    painter.text(
        rect.left_center() + Vec2::new(24.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        Face::Plex.at(12.5),
        p.ink,
    );
    response.clicked()
}

/// What this computer's facts say for an action row, and its button.
fn act_value(
    act: Act,
    state: &SettingsState,
    installation: Option<&Installation>,
    flavor: &str,
) -> (String, &'static str) {
    let info = state.info.clone().unwrap_or_default();
    match act {
        Act::Installation => (
            installation
                .and_then(|i| i.path.as_ref())
                .map_or_else(|| "not found".into(), |p| p.to_string_lossy().into_owned()),
            "change",
        ),
        Act::Wowlua => (format!("wowlua {} · built in", state.wowlua_version), ""),
        Act::BlizzardUi => (format!("from the installation · {flavor}"), "refresh"),
        Act::CheckCache => (
            info.check_cache.map_or("empty".into(), |b| {
                format!("~/.wowlua/cache/check · {}", crate::ship::size(b))
            }),
            "clear",
        ),
        Act::ArtCache => (
            info.art_cache.map_or("empty".into(), |(b, n)| {
                format!("{n} files · {}", crate::ship::size(b))
            }),
            "clear",
        ),
        Act::Github => match (&state.github, &info.github) {
            (Some(GithubSignIn::Code { .. }), _) => ("waiting for GitHub…".into(), "cancel"),
            (_, Some(login)) => (format!("signed in as {login}"), "sign out"),
            _ => ("not signed in".into(), "sign in with GitHub"),
        },
        Act::Drafts => (
            "Studio's data on this machine · none yet (online workspaces come with M12)".into(),
            "",
        ),
        Act::Version => (
            format!(
                "wowlua studio {} · wowlua {}",
                env!("CARGO_PKG_VERSION"),
                state.wowlua_version
            ),
            "check for updates",
        ),
        Act::Log => (
            info.log
                .map_or("no log yet".into(), |p| p.to_string_lossy().into_owned()),
            "open",
        ),
        Act::ResetLayouts => (String::new(), "reset layouts"),
    }
}

/// The main panel for Studio's settings: a section, or what matches.
#[allow(clippy::too_many_arguments)]
pub fn studio(
    ui: &mut Ui,
    state: &mut SettingsState,
    settings: &mut Settings,
    system: &dyn System,
    workspace: Option<&Workspace>,
    installation: Option<&Installation>,
    keys: impl FnOnce(&mut Ui, &mut Settings) -> bool,
) -> Option<Action> {
    let p = palette(ui.ctx());
    if !state.asked {
        state.asked = true;
        system.settings_info();
    }
    let section = match state.view {
        View::Studio(s) => s,
        _ => Section::default(),
    };
    let query = state.query.trim().to_lowercase();
    let mut action = None;
    forms::padded(ui, 16, 8, |ui| {
        ui.horizontal(|ui| {
            let title = if query.is_empty() {
                section.label().to_owned()
            } else {
                format!("settings matching \"{}\"", state.query.trim())
            };
            ui.label(
                egui::RichText::new(title)
                    .font(Face::PlexMedium.at(12.5))
                    .color(p.ink),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                forms::text_input(ui, &mut state.query, 260.0, "find a setting");
            });
        });
    });
    let flavor = workspace
        .and_then(|w| w.flavors.first().cloned())
        .unwrap_or_else(|| "camelot".into());
    egui::ScrollArea::vertical()
        .id_salt("settings-rows")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            forms::padded(ui, 24, 8, |ui| {
                ui.set_width(ui.available_width());
                if query.is_empty() {
                    match section {
                        Section::Claude => {
                            forms::callout(ui, CalloutKind::Plain, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    widgets::badge(ui, "M14", BadgeKind::Planned);
                                    forms::paragraph(ui, "Claude in Studio is the last part to build. Your key and these settings are kept now; they take effect when it is.", 12.5, 19.0, p.ink);
                                });
                            });
                        }
                        Section::Github => {
                            forms::callout(ui, CalloutKind::Plain, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    widgets::badge(ui, "M12", BadgeKind::Planned);
                                    forms::paragraph(ui, "Signing in works now; the pull requests, online workspaces and runs these settings steer come with the GitHub pages.", 12.5, 19.0, p.ink);
                                });
                            });
                            if let Some(GithubSignIn::Code { code, url }) = &state.github {
                                forms::callout(ui, CalloutKind::Note, |ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        forms::paragraph(ui, &format!("Enter {code} at {url} · waiting for GitHub…"), 12.5, 19.0, p.ink);
                                        if forms::text_button(ui, "copy the code").clicked() {
                                            ui.ctx().copy_text(code.clone());
                                        }
                                    });
                                });
                            }
                            if let Some(GithubSignIn::Failed(why)) = &state.github {
                                forms::callout(ui, CalloutKind::Error, |ui| forms::paragraph(ui, why, 12.5, 19.0, p.ink));
                            }
                        }
                        Section::Keybindings => {
                            if keys(ui, settings) {
                                action = Some(Action::Changed);
                            }
                            return;
                        }
                        Section::About => {
                            match &state.updates {
                                Some(UpdateCheck::Newest) => caption(ui, "This is the newest Studio.", &p),
                                Some(UpdateCheck::Newer { version, url }) => {
                                    forms::callout(ui, CalloutKind::Note, |ui| {
                                        forms::paragraph(ui, &format!("wowlua studio {version} is out: {url}"), 12.5, 19.0, p.ink);
                                    });
                                }
                                Some(UpdateCheck::Failed(why)) => caption(ui, why, &p),
                                None if state.checking_updates => caption(ui, "checking…", &p),
                                None => {}
                            }
                        }
                        _ => {}
                    }
                }
                let all = rows();
                let shown: Vec<&Row> = all
                    .iter()
                    .filter(|r| {
                        if query.is_empty() {
                            r.section == section
                        } else {
                            format!("{} {}", r.label, r.desc).to_lowercase().contains(&query)
                        }
                    })
                    .collect();
                if shown.is_empty() {
                    caption(ui, &format!("No setting matches \"{}\".", state.query.trim()), &p);
                }
                for r in shown {
                    let where_ = (!query.is_empty()).then(|| r.section.label());
                    let a = row(ui, r, where_, state, settings, system, installation, &flavor, &p);
                    if a.is_some() {
                        action = a;
                    }
                }
            });
        });
    action
}

fn caption(ui: &mut Ui, text: &str, p: &Palette) {
    ui.label(
        egui::RichText::new(text)
            .font(Face::Plex.at(11.5))
            .color(p.ink_muted),
    );
}

/// A setting's row: its label and description, its control on the right.
#[allow(clippy::too_many_arguments)]
fn row(
    ui: &mut Ui,
    r: &Row,
    section: Option<&str>,
    state: &mut SettingsState,
    settings: &mut Settings,
    system: &dyn System,
    installation: Option<&Installation>,
    flavor: &str,
    p: &Palette,
) -> Option<Action> {
    let mut action = None;
    let width = ui.available_width();
    let control = 360.0_f32.min(width * 0.5);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(width - control - 16.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(r.label)
                        .font(Face::PlexMedium.at(12.5))
                        .color(p.ink),
                );
                if let Some(s) = section {
                    ui.label(
                        egui::RichText::new(format!("· {s}"))
                            .font(Face::Plex.at(11.0))
                            .color(p.ink_faint),
                    );
                }
            });
            if !r.desc.is_empty() {
                ui.label(
                    egui::RichText::new(r.desc)
                        .font(Face::Plex.at(11.5))
                        .color(p.ink_muted),
                );
            }
        });
        ui.vertical(|ui| {
            ui.set_width(control);
            match &r.kind {
                Kind::Flag(get, set) => {
                    let mut on = get(settings);
                    let word = if on { "on" } else { "off" };
                    if forms::checkbox(ui, &mut on, word, None) {
                        set(settings, on);
                        action = Some(Action::Changed);
                    }
                }
                Kind::Choice(options, get, set) => {
                    let now = get(settings);
                    let at = options.iter().position(|o| *o == now).unwrap_or(0);
                    let id = format!("set-{}", r.label);
                    if let Some(i) = widgets::seg(ui, &id, options, at, true) {
                        set(settings, options[i]);
                        action = Some(Action::Changed);
                    }
                }
                Kind::Text(get, set) => {
                    let now = get(settings);
                    let buffer = state.buffers.entry(r.label).or_insert_with(|| now.clone());
                    let response = forms::text_input(ui, buffer, control, "");
                    if response.lost_focus() {
                        set(settings, buffer);
                        *buffer = get(settings);
                        action = Some(Action::Changed);
                    } else if !response.has_focus() {
                        *buffer = now;
                    }
                }
                Kind::Secret => {
                    let saved = state.info.as_ref().is_some_and(|i| i.claude_key);
                    ui.horizontal(|ui| {
                        if saved {
                            caption(ui, "a key is saved", p);
                            if widgets::button(ui, ButtonKind::Secondary, "remove", true, None)
                                .clicked()
                            {
                                system.claude_key(None);
                                state.asked = false;
                            }
                        } else {
                            ui.add(
                                egui::TextEdit::singleline(&mut state.claude_key)
                                    .password(true)
                                    .desired_width(control - 80.0)
                                    .font(Face::Plex.at(12.0))
                                    .hint_text("sk-ant-…"),
                            );
                            if widgets::button(ui, ButtonKind::Secondary, "save", true, None)
                                .clicked()
                                && !state.claude_key.trim().is_empty()
                            {
                                system.claude_key(Some(state.claude_key.trim().to_owned()));
                                state.claude_key.clear();
                                state.asked = false;
                            }
                        }
                    });
                }
                Kind::Action(act) => {
                    let (value, button) = act_value(*act, state, installation, flavor);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(value)
                                .font(Face::Plex.at(11.5))
                                .color(p.ink_muted),
                        );
                        if !button.is_empty()
                            && widgets::button(ui, ButtonKind::Secondary, button, true, None)
                                .clicked()
                        {
                            match act {
                                Act::Installation => action = Some(Action::PickInstallation),
                                Act::BlizzardUi => system.refresh_ui(flavor),
                                Act::CheckCache => {
                                    system.clear_check_cache();
                                    state.asked = false;
                                }
                                Act::ArtCache => {
                                    system.clear_art_cache();
                                    state.asked = false;
                                }
                                Act::Github => match (
                                    &state.github,
                                    state.info.as_ref().and_then(|i| i.github.as_ref()),
                                ) {
                                    (Some(GithubSignIn::Code { .. }), _) => {
                                        system.github_sign_in(false);
                                        state.github = None;
                                    }
                                    (_, Some(_)) => {
                                        system.github_sign_in(false);
                                        state.github = None;
                                        state.asked = false;
                                    }
                                    _ => system.github_sign_in(true),
                                },
                                Act::Version => {
                                    state.checking_updates = true;
                                    state.updates = None;
                                    system.check_updates();
                                }
                                Act::Log => action = Some(Action::OpenLog),
                                Act::ResetLayouts => action = Some(Action::ResetLayouts),
                                Act::Wowlua | Act::Drafts => {}
                            }
                        }
                    });
                }
            }
        });
    });
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
    action
}

/// The rows' labels, for checks.
pub fn labels() -> Vec<(&'static str, &'static str)> {
    rows()
        .into_iter()
        .map(|r| (r.section.label(), r.label))
        .collect()
}

#[allow(dead_code)]
fn _unused(_: PathBuf) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every section has its rows (the keybindings have their table), and
    /// every choice can show what its setting holds.
    #[test]
    fn every_section_has_settings() {
        let all = rows();
        for section in Section::ALL {
            let n = all.iter().filter(|r| r.section == section).count();
            assert!(
                n > 0 || section == Section::Keybindings,
                "{}",
                section.label()
            );
        }
        let s = Settings::default();
        for r in &all {
            if let Kind::Choice(options, get, _) = &r.kind {
                assert!(
                    options.contains(&get(&s).as_str()),
                    "{}: {}",
                    r.label,
                    get(&s)
                );
            }
        }
    }

    #[test]
    fn settings_set_and_read_back() {
        let mut s = Settings::default();
        for r in rows() {
            match r.kind {
                Kind::Flag(get, set) => {
                    let was = get(&s);
                    set(&mut s, !was);
                    assert_eq!(get(&s), !was, "{}", r.label);
                }
                Kind::Choice(options, get, set) => {
                    set(&mut s, options[options.len() - 1]);
                    assert_eq!(get(&s), options[options.len() - 1], "{}", r.label);
                }
                _ => {}
            }
        }
        let size = rows()
            .into_iter()
            .find(|r| r.label == "interface size")
            .unwrap();
        if let Kind::Text(get, set) = size.kind {
            set(&mut s, "125%");
            assert_eq!(get(&s), "125%");
            set(&mut s, "900");
            assert_eq!(s.interface_size, 200, "clamped");
        }
    }
}
