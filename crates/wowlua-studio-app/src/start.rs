//! The start page (the design's start board, S024): what a window shows
//! with no workspace. Open a folder, make a new addon or adopt one, the
//! recent workspaces; this computer (wowlua, the WoW installation,
//! Blizzard's UI) and what's new.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Galley, Pos2, Rect, Sense, Stroke,
    StrokeKind, Ui, UiBuilder, Vec2,
    text::{LayoutJob, TextFormat},
};
use wowlua_studio_ui::forms::{self, CalloutKind, TermLine};
use wowlua_studio_ui::widgets::{self, BadgeKind, ButtonKind};
use wowlua_studio_ui::{Face, Icon, Palette, icons, palette, wordmark};

use crate::recents::{self, Recents};
use crate::system::{Adopt, AdoptPreview, Event, Installation, NewAddon, System, UiState};

/// The form under the three ways to start.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Panel {
    #[default]
    Open,
    New,
    Adopt,
}

/// What the start page asks Studio to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// The system's folder picker, then the folder.
    PickFolder,
    Open(PathBuf),
    /// Finds a recent workspace's folder that moved, then opens it.
    Locate(PathBuf),
    Forget(PathBuf),
}

/// What the start page needs from Studio.
pub struct Cx<'a> {
    pub system: &'a dyn System,
    pub recents: &'a Recents,
    pub home: Option<&'a Path>,
    /// The version of wowlua built in.
    pub version: &'a str,
}

#[derive(Default)]
pub struct StartPage {
    pub panel: Panel,
    pub new: NewForm,
    pub adopt: AdoptForm,
    /// `None` while it is looked for.
    installation: Option<Installation>,
    /// The installation was just picked: its problem is a warning.
    picked: bool,
    ui_code: BTreeMap<String, UiState>,
    whats_new: Vec<String>,
    /// The folder being opened.
    pub opening: Option<PathBuf>,
    /// The folder that did not open, and why.
    pub failed: Option<(PathBuf, String)>,
    /// Recent workspaces whose folder is gone.
    gone: Vec<PathBuf>,
    started: bool,
}

/// The new-addon form (`wowlua new`, S026).
pub struct NewForm {
    pub name: String,
    kind: usize,
    /// `strict` or `basic`.
    check: usize,
    camelot: bool,
    folder: String,
    /// The folder was typed: it no longer follows the name.
    folder_typed: bool,
    /// `a new repository in the folder` or `no git`.
    git: usize,
    ci: bool,
    profile: bool,
    pub creating: bool,
    pub error: Option<String>,
    /// Whether the folder's parent is in a Git repository, per parent.
    in_git: Option<(PathBuf, bool)>,
}

impl Default for NewForm {
    fn default() -> Self {
        NewForm {
            name: String::new(),
            kind: 0,
            check: 0,
            camelot: true,
            folder: String::new(),
            folder_typed: false,
            git: 0,
            ci: true,
            profile: true,
            creating: false,
            error: None,
            in_git: None,
        }
    }
}

/// The adopt form (`wowlua init`, S027).
#[derive(Default)]
pub struct AdoptForm {
    pub folder: String,
    /// `.pkgmeta and TOC files` or `TOC files only`.
    from: usize,
    /// What adopting the folder would take, for the folder it was read for.
    preview: Option<(PathBuf, AdoptPreview)>,
    /// The folder was opened, and has no `wowlua.json` (S032).
    pub no_manifest: bool,
    pub adopting: bool,
    pub error: Option<String>,
}

const KINDS: [&str; 4] = ["addon", "library", "lua-library", "types"];
const CHECKS: [&str; 2] = ["strict", "basic"];

impl StartPage {
    /// Shows the adopt form for a folder with no `wowlua.json`.
    pub fn offer_adopt(&mut self, dir: &Path, home: Option<&Path>) {
        self.panel = Panel::Adopt;
        self.adopt.folder = recents::tilde(dir, home);
        self.adopt.preview = None;
        self.adopt.no_manifest = true;
        self.adopt.error = None;
    }

    /// The WoW installation, once found (Studio looks for it at start).
    pub fn installation(&self) -> Option<&Installation> {
        self.installation.as_ref()
    }

    /// Recent workspaces whose folder is gone, checked again.
    pub fn refresh(&mut self, system: &dyn System, recents: &Recents) {
        self.gone = recents
            .list
            .iter()
            .filter(|r| !system.exists(&r.path))
            .map(|r| r.path.clone())
            .collect();
    }

    /// Takes the events that are the start page's.
    pub fn event(&mut self, event: &Event, system: &dyn System) {
        match event {
            Event::Installation(found) => {
                for flavor in flavors(found) {
                    system.check_ui(&flavor);
                }
                self.installation = Some(found.clone());
            }
            Event::Ui { flavor, state } => {
                self.ui_code.insert(flavor.clone(), state.clone());
            }
            _ => {}
        }
    }

    pub fn show(&mut self, ui: &mut Ui, cx: &Cx<'_>) -> Vec<Action> {
        if !self.started {
            self.started = true;
            self.whats_new = cx.system.whats_new();
            self.refresh(cx.system, cx.recents);
        }
        let mut actions = Vec::new();
        let p = palette(ui.ctx());
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                let full = ui.max_rect();
                // 1200 px at most, 24 px either side, 36 above, 48 below.
                let width = (full.width() - 48.0).clamp(200.0, 1152.0);
                let left = full.left() + (full.width() - width) / 2.0;
                let top = full.top() + 36.0;
                // The aside sits beside the main column where both fit
                // (600 and 340 px, 32 apart), under it otherwise.
                let beside = width >= 600.0 + 32.0 + 340.0;
                let main_w = if beside { width - 32.0 - 340.0 } else { width };
                let main_rect = Rect::from_min_size(Pos2::new(left, top), Vec2::new(main_w, 0.0));
                let main_bottom = column(ui, main_rect, |ui| self.main(ui, p, cx, &mut actions));
                let aside_rect = if beside {
                    Rect::from_min_size(Pos2::new(left + main_w + 32.0, top), Vec2::new(340.0, 0.0))
                } else {
                    Rect::from_min_size(Pos2::new(left, main_bottom + 32.0), Vec2::new(width, 0.0))
                };
                let aside_bottom = column(ui, aside_rect, |ui| self.aside(ui, p, cx));
                let bottom = main_bottom.max(aside_bottom) + 48.0;
                ui.allocate_rect(
                    Rect::from_min_max(full.min, Pos2::new(full.right(), bottom)),
                    Sense::hover(),
                );
            });
        actions
    }

    fn main(&mut self, ui: &mut Ui, p: &Palette, cx: &Cx<'_>, actions: &mut Vec<Action>) {
        let width = ui.available_width();
        // The wordmark, 28 px high.
        let (r, _) = ui.allocate_exact_size(Vec2::new(wordmark::width(28.0), 28.0), Sense::hover());
        wordmark::paint_wordmark(ui.ctx(), ui.painter(), r, p);
        ui.add_space(16.0);
        let heading = widgets::galley(
            ui,
            "Open an AddOn.",
            Face::MartianBold.at(28.0),
            p.ink,
            -0.56,
        );
        let (r, _) = ui.allocate_exact_size(Vec2::new(heading.size().x, 36.0), Sense::hover());
        ui.painter().galley(
            Pos2::new(r.left(), r.center().y - heading.size().y / 2.0),
            heading,
            p.ink,
        );
        ui.add_space(10.0);
        ui.scope(|ui| {
            ui.set_max_width(width.min(560.0));
            forms::paragraph(
                ui,
                "A package folder on this computer, a new AddOn, or an existing one to adopt. \
                 Studio checks it, runs its tests and runs it in the emulated client.",
                13.0,
                21.0,
                p.ink_muted,
            );
        });
        ui.add_space(22.0);

        // The three ways to start.
        let ways = [
            (
                Panel::Open,
                Icon::Folder,
                "open folder",
                "a package or workspace on disk",
            ),
            (
                Panel::New,
                Icon::Add,
                "new addon",
                "wowlua new, from a template",
            ),
            (
                Panel::Adopt,
                Icon::Package,
                "adopt addon",
                "TOC and .pkgmeta to wowlua.json",
            ),
        ];
        let cols = if width >= 480.0 { 3 } else { 1 };
        let col_w = (width - 16.0 * (cols as f32 - 1.0)) / cols as f32;
        let mut x = ui.cursor().left();
        let mut y = ui.cursor().top();
        for (i, (panel, icon, title, caption)) in ways.into_iter().enumerate() {
            let r = Rect::from_min_size(Pos2::new(x, y), Vec2::new(col_w, 90.0));
            if way(ui, r, icon, title, caption, self.panel == panel) {
                self.panel = panel;
                if panel == Panel::New && !self.new.folder_typed {
                    self.new.folder.clear();
                }
            }
            if (i + 1) % cols == 0 {
                x = ui.cursor().left();
                y += 90.0 + 16.0;
            } else {
                x += col_w + 16.0;
            }
        }
        let rows = 3usize.div_ceil(cols) as f32;
        ui.allocate_space(Vec2::new(width, rows * 90.0 + (rows - 1.0) * 16.0));
        ui.add_space(22.0);

        match self.panel {
            Panel::Open => self.recent(ui, p, cx, actions),
            Panel::New => self.new_form(ui, p, cx),
            Panel::Adopt => self.adopt_form(ui, p, cx, actions),
        }
    }

    fn recent(&mut self, ui: &mut Ui, p: &Palette, cx: &Cx<'_>, actions: &mut Vec<Action>) {
        let opening = self.opening.clone();
        let failed = self.failed.clone();
        let mut dismiss = false;
        let mut pick = false;
        forms::card(
            ui,
            "recent",
            |ui| {
                pick =
                    widgets::button(ui, ButtonKind::Primary, "open folder…", true, None).clicked();
            },
            |ui| {
                if let Some(path) = &opening {
                    forms::padded(ui, 14, 10, |ui| {
                        forms::paragraph(
                            ui,
                            &format!("opening {}…", recents::tilde(path, cx.home)),
                            12.5,
                            19.0,
                            p.ink_muted,
                        );
                    });
                }
                if let Some((path, message)) = &failed {
                    let text = format!("{} did not open. {message}", recents::tilde(path, cx.home));
                    if notice_row(ui, p, Notice::Error, "did not open", &text, &["dismiss"])
                        == Some(0)
                    {
                        dismiss = true;
                    }
                }
                if cx.recents.list.is_empty() {
                    forms::padded(ui, 16, 24, |ui| {
                        ui.spacing_mut().item_spacing.y = 10.0;
                        let width = ui.available_width();
                        let galley = rich(
                            ui,
                            &[
                                (
                                    "Nothing opened yet. Open the folder that holds a package's ",
                                    Face::Plex.at(12.5),
                                    p.ink_muted,
                                ),
                                ("wowlua.json", Face::Plex.at(11.5), p.ink),
                                (
                                    ", or drop it onto this window.",
                                    Face::Plex.at(12.5),
                                    p.ink_muted,
                                ),
                            ],
                            width,
                            19.0,
                        );
                        let (r, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
                        ui.painter().galley(r.min, galley, p.ink_muted);
                        forms::paragraph(
                            ui,
                            "A workspace opens with all of its members: a root package and its libs or plugins.",
                            11.5,
                            16.0,
                            p.ink_muted,
                        );
                    });
                    return;
                }
                let now = cx.system.now();
                for recent in &cx.recents.list {
                    let gone = self.gone.contains(&recent.path);
                    if gone {
                        let text = format!(
                            "{} is gone. It was moved or deleted. Locate it, or remove it from the list.",
                            recents::tilde(&recent.path, cx.home)
                        );
                        match notice_row(
                            ui,
                            p,
                            Notice::Error,
                            &recent.name,
                            &text,
                            &["locate", "remove"],
                        ) {
                            Some(0) => actions.push(Action::Locate(recent.path.clone())),
                            Some(_) => actions.push(Action::Forget(recent.path.clone())),
                            None => {}
                        }
                    } else if let Some(why) = &recent.cannot_check {
                        let text = format!(
                            "{} can't be checked: {why}. It opens; checks stay off until you change the requirement.",
                            recent.name
                        );
                        if notice_row(ui, p, Notice::Warning, &recent.name, &text, &["open"])
                            .is_some()
                        {
                            actions.push(Action::Open(recent.path.clone()));
                        }
                    } else if recent_row(
                        ui,
                        p,
                        &recent.name,
                        recent.members.len() > 1,
                        &recents::detail(recent, cx.home),
                        &recents::when(now, recent.opened),
                    ) {
                        actions.push(Action::Open(recent.path.clone()));
                    }
                }
            },
        );
        if dismiss {
            self.failed = None;
        }
        if pick {
            actions.push(Action::PickFolder);
        }
    }

    fn new_form(&mut self, ui: &mut Ui, p: &Palette, cx: &Cx<'_>) {
        let form = &mut self.new;
        let base = cx
            .recents
            .list
            .first()
            .and_then(|r| r.path.parent().map(Path::to_path_buf))
            .or_else(|| cx.home.map(Path::to_path_buf))
            .unwrap_or_default();
        if !form.folder_typed {
            form.folder = recents::tilde(&base.join(&form.name), cx.home);
        }
        let path = expand(&form.folder, cx.home);
        let names = cx.system.names(&form.name);
        let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
        if form.in_git.as_ref().is_none_or(|(dir, _)| *dir != parent) {
            form.in_git = Some((parent.clone(), cx.system.in_git(&parent)));
        }
        let parent_in_git = form.in_git.as_ref().is_some_and(|(_, git)| *git);
        let kind = KINDS[form.kind];
        let tests = matches!(kind, "addon" | "library");
        let folder_name = path
            .file_name()
            .filter(|_| !form.name.trim().is_empty() || form.folder_typed)
            .map_or_else(|| "<name>".to_owned(), |n| n.to_string_lossy().into_owned());
        // `--name` only when the folder's name gives other names.
        let name_flag = (!form.name.is_empty()
            && cx.system.names(&folder_name).package != names.package)
            .then(|| names.addon.clone());

        let mut submit = None;
        forms::card(
            ui,
            "new addon",
            |_| {},
            |ui| {
                forms::padded(ui, 14, 14, |ui| {
                    ui.spacing_mut().item_spacing.y = 16.0;
                    let w = ui.available_width();
                    grid(ui, w, 3, |ui, col, cw| match col {
                        0 => field(ui, "name", |ui| {
                            if forms::text_input(ui, &mut form.name, cw, "pull-timer").changed() {
                                form.error = None;
                            }
                            if kind == "addon" && !form.name.trim().is_empty() {
                                forms::help(ui, &format!("AddOn folder: {}", names.addon));
                            }
                        }),
                        1 => field(ui, "kind", |ui| {
                            let options: Vec<(&str, bool)> =
                                KINDS.iter().map(|k| (*k, true)).collect();
                            forms::select(ui, "new-kind", &options, &mut form.kind, cw);
                        }),
                        _ => field(ui, "type checking", |ui| {
                            forms::select(
                                ui,
                                "new-check",
                                &[("strict", true), ("basic", true)],
                                &mut form.check,
                                cw,
                            );
                        }),
                    });
                    field(ui, "flavors", |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 20.0;
                            forms::checkbox(ui, &mut form.camelot, "camelot", Some("WoW Forever"));
                            // The emulated client runs camelot only (S034).
                            ui.add_enabled_ui(false, |ui| {
                                forms::checkbox(ui, &mut false, "retail", Some("planned"));
                                forms::checkbox(ui, &mut false, "vanilla", Some("planned"));
                            });
                        });
                    });
                    grid(ui, w, 2, |ui, col, cw| match col {
                        0 => field(ui, "folder", |ui| {
                            if forms::text_input(ui, &mut form.folder, cw, "~/Projects/pull-timer")
                                .changed()
                            {
                                form.folder_typed = true;
                                form.error = None;
                            }
                        }),
                        _ => field(ui, "git", |ui| {
                            forms::select(
                                ui,
                                "new-git",
                                &[("a new repository in the folder", true), ("no git", true)],
                                &mut form.git,
                                cw,
                            );
                            if parent_in_git && form.git == 0 {
                                forms::help(
                                    ui,
                                    "The folder is in a Git repository already: no new one is made.",
                                );
                            }
                        }),
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 24.0;
                        forms::checkbox(
                            ui,
                            &mut form.ci,
                            "github actions workflow",
                            Some("--ci github: check, test, publish on tags"),
                        );
                        ui.add_enabled_ui(tests, |ui| {
                            let mut on = form.profile && tests;
                            let help = if tests {
                                "tests/profiles/default.json"
                            } else {
                                "addon and library packages: they have tests"
                            };
                            if forms::checkbox(ui, &mut on, "a world profile", Some(help)) {
                                form.profile = on;
                            }
                        });
                    });

                    let mut command = format!("wowlua new {folder_name} --kind {kind}");
                    if form.camelot {
                        command.push_str(" --flavors camelot");
                    }
                    if form.check == 0 {
                        command.push_str(" --check strict");
                    }
                    if form.ci {
                        command.push_str(" --ci github");
                    }
                    if form.profile && tests {
                        command.push_str(" --world-profile");
                    }
                    if form.git == 1 {
                        command.push_str(" --no-git");
                    }
                    if let Some(name) = &name_flag {
                        command.push_str(&format!(" --name {name}"));
                    }
                    let created = format!("{kind} package in {folder_name}");
                    forms::terminal(
                        ui,
                        Some((&recents::tilde(&parent, cx.home), "preview")),
                        &[
                            TermLine::Command(&command),
                            TermLine::Status("Created", &created),
                        ],
                    );
                    if let Some(error) = &form.error {
                        error_callout(ui, p, error);
                    }
                    let ready = !form.name.trim().is_empty() && form.camelot && !form.creating;
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        let label = if form.creating {
                            "creating…"
                        } else {
                            "create and open"
                        };
                        let clicked = ui
                            .add_enabled_ui(ready, |ui| {
                                widgets::button(ui, ButtonKind::Primary, label, true, None)
                            })
                            .inner
                            .clicked();
                        if clicked {
                            submit = Some(NewAddon {
                                path: path.clone(),
                                name: name_flag.clone(),
                                kind: kind.to_owned(),
                                flavors: vec!["camelot".to_owned()],
                                check: CHECKS[form.check].to_owned(),
                                git: form.git == 0,
                                ci_github: form.ci,
                                world_profile: form.profile && tests,
                            });
                        }
                    });
                });
            },
        );
        if let Some(request) = submit {
            form.creating = true;
            form.error = None;
            cx.system.new_addon(request);
        }
    }

    fn adopt_form(&mut self, ui: &mut Ui, p: &Palette, cx: &Cx<'_>, actions: &mut Vec<Action>) {
        let form = &mut self.adopt;
        let dir = expand(&form.folder, cx.home);
        let mut submit = None;
        forms::card(
            ui,
            "adopt an addon",
            |_| {},
            |ui| {
                forms::padded(ui, 14, 14, |ui| {
                    ui.spacing_mut().item_spacing.y = 14.0;
                    if form.no_manifest {
                        forms::callout(ui, CalloutKind::Note, |ui| {
                            forms::paragraph(
                                ui,
                                &format!(
                                    "{} has no wowlua.json. Adopt it to open it as a package: wowlua init makes one from its TOC files.",
                                    form.folder
                                ),
                                12.0,
                                18.0,
                                p.ink,
                            );
                        });
                    }
                    let w = ui.available_width();
                    let mut folder_input = None;
                    grid(ui, w, 2, |ui, col, cw| match col {
                        0 => field(ui, "folder", |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                let choose_w = 84.0;
                                let input = forms::text_input(
                                    ui,
                                    &mut form.folder,
                                    cw - choose_w - 8.0,
                                    "~/Projects/MyAddon",
                                );
                                if input.changed() {
                                    form.no_manifest = false;
                                    form.error = None;
                                }
                                folder_input = Some(input);
                                if widgets::button(
                                    ui,
                                    ButtonKind::Secondary,
                                    "choose…",
                                    false,
                                    None,
                                )
                                .clicked()
                                    && let Some(dir) = cx.system.pick_folder("adopt an addon")
                                {
                                    form.folder = recents::tilde(&dir, cx.home);
                                    form.no_manifest = false;
                                    form.error = None;
                                }
                            });
                        }),
                        _ => field(ui, "convert from", |ui| {
                            let pkgmeta = form
                                .preview
                                .as_ref()
                                .is_some_and(|(_, preview)| preview.pkgmeta);
                            if !pkgmeta {
                                form.from = 1;
                            }
                            forms::select(
                                ui,
                                "adopt-from",
                                &[
                                    (".pkgmeta and TOC files", pkgmeta),
                                    ("TOC files only", true),
                                ],
                                &mut form.from,
                                cw,
                            );
                        }),
                    });
                    // The folder is read once it is chosen, not at each key.
                    let typing = folder_input.is_some_and(|r| r.has_focus());
                    if !typing
                        && !form.folder.trim().is_empty()
                        && form.preview.as_ref().is_none_or(|(d, _)| *d != dir)
                    {
                        let preview = cx.system.adopt_preview(&dir);
                        form.from = usize::from(!preview.pkgmeta);
                        form.preview = Some((dir.clone(), preview));
                    }
                    let preview = form
                        .preview
                        .as_ref()
                        .filter(|(d, _)| *d == dir && !form.folder.trim().is_empty())
                        .map(|(_, preview)| preview.clone());
                    let Some(preview) = preview else {
                        return;
                    };
                    let pkgmeta = form.from == 0;
                    forms::key_values(
                        ui,
                        &[
                            (
                                "toc files",
                                if preview.tocs.is_empty() {
                                    "none".to_owned()
                                } else {
                                    preview.tocs.join(", ")
                                },
                            ),
                            (
                                "sources",
                                format!("{} files: .lua and .xml", preview.sources),
                            ),
                            (
                                ".pkgmeta",
                                match (preview.pkgmeta, pkgmeta) {
                                    (false, _) => "none".to_owned(),
                                    (true, true) => {
                                        "externals become dependencies (git), locked in wowlua.lock"
                                            .to_owned()
                                    }
                                    (true, false) => "left as it is".to_owned(),
                                },
                            ),
                            (
                                "git",
                                if preview.git {
                                    "on a new branch wowlua/adopt, so the change is one commit to review"
                                    .to_owned()
                                } else {
                                    "not a Git repository: the files change in place".to_owned()
                                },
                            ),
                        ],
                    );
                    let command = match (preview.pkgmeta, pkgmeta) {
                        (true, true) => "wowlua init --from-pkgmeta",
                        (true, false) => "wowlua init --no-pkgmeta",
                        (false, _) => "wowlua init",
                    };
                    let out = match preview.tocs.first() {
                        Some(toc) => format!("    wowlua.json from {toc}; sources into src/"),
                        None => "    wowlua.json from the TOC files; sources into src/".to_owned(),
                    };
                    forms::terminal(ui, None, &[TermLine::Command(command), TermLine::Out(&out)]);

                    let mut blocked = None;
                    if preview.has_manifest {
                        blocked = Some(format!(
                            "{} has a wowlua.json already: open it instead.",
                            form.folder
                        ));
                    } else if preview.tocs.is_empty() {
                        blocked = Some(format!(
                            "{} has no TOC file: wowlua init adopts an AddOn from its TOC files.",
                            form.folder
                        ));
                    }
                    if let Some(text) = &blocked {
                        let kind = if preview.has_manifest {
                            CalloutKind::Note
                        } else {
                            CalloutKind::Error
                        };
                        forms::callout(ui, kind, |ui| {
                            forms::paragraph(ui, text, 12.0, 18.0, p.ink);
                            if preview.has_manifest && forms::text_button(ui, "open").clicked() {
                                actions.push(Action::Open(dir.clone()));
                            }
                        });
                    }
                    if let Some(error) = &form.error {
                        error_callout(ui, p, error);
                    }
                    let ready = blocked.is_none() && !form.adopting;
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        let label = match (form.adopting, preview.git) {
                            (true, _) => "adopting…",
                            (false, true) => "adopt on a new branch",
                            (false, false) => "adopt",
                        };
                        let clicked = ui
                            .add_enabled_ui(ready, |ui| {
                                widgets::button(ui, ButtonKind::Primary, label, true, None)
                            })
                            .inner
                            .clicked();
                        if clicked {
                            submit = Some(Adopt {
                                dir: dir.clone(),
                                pkgmeta,
                                branch: preview.git,
                            });
                        }
                    });
                });
            },
        );
        if let Some(request) = submit {
            form.adopting = true;
            form.error = None;
            cx.system.adopt(request);
        }
    }

    fn aside(&mut self, ui: &mut Ui, p: &Palette, cx: &Cx<'_>) {
        ui.spacing_mut().item_spacing.y = 16.0;
        let mut pick = false;
        let mut download = None;
        forms::card(
            ui,
            "this computer",
            |_| {},
            |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                check_row(
                    ui,
                    p,
                    Mark::Ok,
                    &format!("wowlua {}", cx.version),
                    "built in · language server, debugger, emulated client, renderer",
                    None,
                    true,
                );
                let (mark, text, action) = match &self.installation {
                None => (Mark::Pending, "looking for it…".to_owned(), None),
                Some(Installation {
                    path: Some(path),
                    clients,
                    ..
                }) => {
                    // The folder, then its client folders and their flavors.
                    let mut text = format!("found: {}\n", recents::tilde(path, cx.home));
                    let found: Vec<String> = clients
                        .iter()
                        .map(|c| format!("{} ({})", c.folder, c.flavor))
                        .collect();
                    if found.is_empty() {
                        text.push_str("no client of a flavor wowlua knows");
                    } else {
                        text.push_str(&found.join(", "));
                    }
                    (Mark::Ok, text, Some("change"))
                }
                Some(Installation { problem, .. }) => match problem {
                    Some(problem) if self.picked => (Mark::Warning, problem.clone(), Some("locate")),
                    _ => (
                        Mark::Pending,
                        "not found yet: pick the World of Warcraft folder for textures, fonts, models and game data"
                            .to_owned(),
                        Some("locate"),
                    ),
                },
            };
                let flavors = self.installation.as_ref().map(flavors).unwrap_or_default();
                if check_row(
                    ui,
                    p,
                    mark,
                    "WoW installation",
                    &text,
                    action,
                    flavors.is_empty(),
                ) {
                    pick = true;
                }
                for (i, flavor) in flavors.iter().enumerate() {
                    let last = i + 1 == flavors.len();
                    let (mark, text, action) = match self.ui_code.get(flavor) {
                        None => (Mark::Pending, "looking in wowlua's cache…".to_owned(), None),
                        Some(UiState::Cached) => (
                            Mark::Ok,
                            "in wowlua's cache, for tests and runs".to_owned(),
                            None,
                        ),
                        Some(UiState::Missing) => (
                            Mark::Pending,
                            "not downloaded yet: tests and runs download it the first time"
                                .to_owned(),
                            Some("download"),
                        ),
                        Some(UiState::Downloading) => {
                            (Mark::Pending, "downloading…".to_owned(), None)
                        }
                        Some(UiState::Failed(e)) => (
                            Mark::Warning,
                            format!("could not download it: {e}"),
                            Some("retry"),
                        ),
                    };
                    let title = format!("Blizzard's UI for {flavor}");
                    if check_row(ui, p, mark, &title, &text, action, last) {
                        download = Some(flavor.clone());
                    }
                }
            },
        );
        if pick && let Some(path) = cx.system.pick_folder("the World of Warcraft folder") {
            self.installation = None;
            self.picked = true;
            cx.system.set_installation(&path);
        }
        if let Some(flavor) = download {
            cx.system.download_ui(&flavor);
        }
        if !self.whats_new.is_empty() {
            // The version built in, as its changelog section (S030).
            forms::card(
                ui,
                &format!("what's new in {}", cx.version),
                |_| {},
                |ui| {
                    forms::padded(ui, 14, 14, |ui| {
                        ui.spacing_mut().item_spacing.y = 8.0;
                        for line in &self.whats_new {
                            forms::paragraph(ui, line, 12.5, 19.0, p.ink);
                        }
                    });
                },
            );
        }
    }
}

/// The flavors of the installation's clients, each once (S029).
fn flavors(found: &Installation) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for client in &found.clients {
        if !out.contains(&client.flavor) {
            out.push(client.flavor.clone());
        }
    }
    out
}

/// Lays out a column from `rect`'s top; returns its bottom.
fn column(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui)) -> f32 {
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_size(
                rect.min,
                Vec2::new(rect.width(), f32::INFINITY),
            ))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_width(rect.width());
    add(&mut child);
    child.min_rect().bottom()
}

/// `n` equal columns, 16 px apart (`st-grid2`, `st-grid3`); one under the
/// other when narrow.
fn grid(ui: &mut Ui, width: f32, n: usize, mut cell: impl FnMut(&mut Ui, usize, f32)) {
    if width < 480.0 {
        for col in 0..n {
            cell(ui, col, width);
        }
        return;
    }
    let cw = (width - 16.0 * (n as f32 - 1.0)) / n as f32;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 16.0;
        for col in 0..n {
            ui.allocate_ui_with_layout(
                Vec2::new(cw, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(cw);
                    cell(ui, col, cw);
                },
            );
        }
    });
}

/// A field: its label, 6 px, its control and help.
fn field(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        forms::label(ui, label);
        add(ui);
    });
}

/// One of the three ways to start (`sx-act`); returns whether it was
/// picked.
fn way(ui: &mut Ui, r: Rect, icon: Icon, title: &str, caption: &str, on: bool) -> bool {
    let p = palette(ui.ctx());
    let response = ui.interact(
        r,
        egui::Id::unique(("wowlua-studio-way", title)),
        Sense::click(),
    );
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, title));
    let painter = ui.painter();
    painter.rect_filled(
        r,
        CornerRadius::ZERO,
        if response.hovered() {
            p.bg_300
        } else {
            p.bg_200
        },
    );
    let (border, w) = if on {
        (p.amber_ink, 2.0)
    } else {
        (p.line, 1.0)
    };
    painter.rect_stroke(
        r,
        CornerRadius::ZERO,
        Stroke::new(w, border),
        StrokeKind::Inside,
    );
    let x = r.left() + 14.0;
    icons::paint(
        painter,
        Rect::from_min_size(Pos2::new(x, r.top() + 14.0), Vec2::splat(16.0)),
        icon,
        p.ink,
    );
    let clip = painter.with_clip_rect(r.shrink(1.0));
    clip.text(
        Pos2::new(x, r.top() + 14.0 + 16.0 + 6.0 + 9.0),
        Align2::LEFT_CENTER,
        title,
        Face::PlexSemiBold.at(13.0),
        p.ink,
    );
    clip.text(
        Pos2::new(x, r.top() + 14.0 + 16.0 + 6.0 + 18.0 + 6.0 + 8.0),
        Align2::LEFT_CENTER,
        caption,
        Face::Plex.at(11.5),
        p.ink_muted,
    );
    widgets::focus_ring(ui, r, &response);
    response.on_hover_cursor(CursorIcon::PointingHand).clicked()
}

/// Text in sections of their own fonts and colours, wrapped at `width`.
fn rich(ui: &Ui, parts: &[(&str, FontId, Color32)], width: f32, line: f32) -> Arc<Galley> {
    let mut job = LayoutJob::default();
    for (text, font, color) in parts {
        job.append(
            text,
            0.0,
            TextFormat {
                font_id: font.clone(),
                color: *color,
                line_height: Some(line),
                ..Default::default()
            },
        );
    }
    job.wrap.max_width = width;
    ui.fonts_mut(|f| f.layout_job(job))
}

/// A recent workspace (`st-row--tall`); returns whether it was clicked.
fn recent_row(
    ui: &mut Ui,
    p: &Palette,
    name: &str,
    workspace: bool,
    detail: &str,
    when: &str,
) -> bool {
    let width = ui.available_width();
    let when_g = widgets::galley(ui, when, Face::Plex.at(11.5), p.ink_muted, 0.0);
    let text_w = width - 14.0 - 16.0 - 10.0 - when_g.size().x - 10.0 - 14.0;
    let name_g = widgets::galley(ui, name, Face::PlexSemiBold.at(12.5), p.ink, 0.0);
    let detail_g = rich(
        ui,
        &[(detail, Face::Plex.at(11.5), p.ink_muted)],
        text_w,
        16.0,
    );
    let height = (8.0 + 18.0 + 2.0 + detail_g.size().y + 8.0).max(40.0);
    let (r, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(r, CornerRadius::ZERO, p.bg_300);
    }
    painter.hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, p.line));
    let x = r.left() + 14.0;
    icons::paint(
        painter,
        Rect::from_min_size(Pos2::new(x, r.top() + 10.0), Vec2::splat(16.0)),
        Icon::Folder,
        p.ink,
    );
    let tx = x + 16.0 + 10.0;
    let name_w = name_g.size().x;
    painter.galley(
        Pos2::new(tx, r.top() + 8.0 + 9.0 - name_g.size().y / 2.0),
        name_g,
        p.ink,
    );
    painter.galley(
        Pos2::new(tx, r.top() + 8.0 + 18.0 + 2.0),
        detail_g,
        p.ink_muted,
    );
    painter.galley(
        Pos2::new(
            r.right() - 14.0 - when_g.size().x,
            r.top() + 8.0 + 9.0 - when_g.size().y / 2.0,
        ),
        when_g,
        p.ink_muted,
    );
    if workspace {
        let badge = Rect::from_min_size(
            Pos2::new(tx + name_w + 6.0, r.top() + 8.0),
            Vec2::new(0.0, 18.0),
        );
        let mut child = ui.new_child(UiBuilder::new().max_rect(badge.with_max_x(r.right())));
        widgets::badge(&mut child, "workspace", BadgeKind::Neutral);
    }
    response.on_hover_cursor(CursorIcon::PointingHand).clicked()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Notice {
    Error,
    Warning,
}

/// A recent workspace that needs a word (`error:`, `warning:`), with text
/// buttons; returns the button clicked.
fn notice_row(
    ui: &mut Ui,
    p: &Palette,
    kind: Notice,
    name: &str,
    text: &str,
    buttons: &[&str],
) -> Option<usize> {
    let width = ui.available_width();
    let (sign, word, ink, soft) = match kind {
        Notice::Error => ("×", "error: ", p.error, p.error_soft),
        Notice::Warning => ("!", "warning: ", p.warning, p.warning_soft),
    };
    let buttons_w = 140.0;
    let text_w = width - 14.0 - 16.0 - 10.0 - buttons_w - 14.0;
    let name_g = widgets::galley(ui, name, Face::PlexSemiBold.at(12.5), p.ink, 0.0);
    let body = rich(
        ui,
        &[
            (word, Face::PlexSemiBold.at(11.5), ink),
            (text, Face::Plex.at(11.5), p.ink_muted),
        ],
        text_w,
        16.0,
    );
    let height = (8.0 + 18.0 + 2.0 + body.size().y + 8.0).max(40.0);
    let (r, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(r, CornerRadius::ZERO, soft);
    painter.hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, p.line));
    let x = r.left() + 14.0;
    painter.text(
        Pos2::new(x + 8.0, r.top() + 8.0 + 9.0),
        Align2::CENTER_CENTER,
        sign,
        Face::PlexSemiBold.at(13.0),
        ink,
    );
    let tx = x + 16.0 + 10.0;
    painter.galley(
        Pos2::new(tx, r.top() + 8.0 + 9.0 - name_g.size().y / 2.0),
        name_g,
        p.ink,
    );
    painter.galley(Pos2::new(tx, r.top() + 8.0 + 18.0 + 2.0), body, p.ink_muted);
    let mut clicked = None;
    let area = Rect::from_min_max(
        Pos2::new(r.right() - 14.0 - buttons_w, r.top() + 6.0),
        Pos2::new(r.right() - 14.0, r.top() + 30.0),
    );
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(area)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = 8.0;
    for (i, label) in buttons.iter().enumerate().rev() {
        if forms::text_button(&mut child, label).clicked() {
            clicked = Some(i);
        }
    }
    clicked
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    Ok,
    Pending,
    Warning,
}

/// A row of "this computer" (`sx-check`): its mark, a title over a
/// caption, a text button; returns whether the button was clicked.
fn check_row(
    ui: &mut Ui,
    p: &Palette,
    mark: Mark,
    title: &str,
    caption: &str,
    action: Option<&str>,
    last: bool,
) -> bool {
    let width = ui.available_width();
    let action_w = action.map_or(0.0, |a| {
        widgets::galley(
            ui,
            &format!("[ {a} ]"),
            Face::PlexMedium.at(12.0),
            p.ink,
            0.24,
        )
        .size()
        .x + 10.0
    });
    let text_w = width - 14.0 - 18.0 - 10.0 - action_w - 14.0;
    let title_g = rich(ui, &[(title, Face::Plex.at(12.5), p.ink)], text_w, 19.0);
    let caption_g = rich(
        ui,
        &[(caption, Face::Plex.at(11.5), p.ink_muted)],
        text_w,
        16.0,
    );
    let height = 10.0 + title_g.size().y + caption_g.size().y + 10.0;
    let (r, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter();
    if !last {
        painter.hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, p.line));
    }
    let x = r.left() + 14.0;
    let center = Pos2::new(x + 9.0, r.top() + 10.0 + 9.5);
    match mark {
        // The fonts have no check mark: it is drawn.
        Mark::Ok => {
            painter.line(
                vec![
                    center + Vec2::new(-4.0, 0.0),
                    center + Vec2::new(-1.25, 2.75),
                    center + Vec2::new(4.0, -2.5),
                ],
                Stroke::new(1.75, p.success),
            );
        }
        Mark::Pending | Mark::Warning => {
            let (sign, color) = if mark == Mark::Pending {
                ("·", p.ink_muted)
            } else {
                ("!", p.warning)
            };
            painter.text(
                center,
                Align2::CENTER_CENTER,
                sign,
                Face::PlexSemiBold.at(12.5),
                color,
            );
        }
    }
    let tx = x + 18.0 + 10.0;
    let title_h = title_g.size().y;
    painter.galley(Pos2::new(tx, r.top() + 10.0), title_g, p.ink);
    painter.galley(
        Pos2::new(tx, r.top() + 10.0 + title_h),
        caption_g,
        p.ink_muted,
    );
    let Some(action) = action else {
        return false;
    };
    let area = Rect::from_min_max(
        Pos2::new(r.right() - 14.0 - action_w, r.top() + 8.0),
        Pos2::new(r.right() - 14.0, r.top() + 30.0),
    );
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(area)
            .layout(egui::Layout::right_to_left(egui::Align::Min)),
    );
    forms::text_button(&mut child, action).clicked()
}

fn error_callout(ui: &mut Ui, p: &Palette, error: &str) {
    forms::callout(ui, CalloutKind::Error, |ui| {
        let width = ui.available_width();
        let g = rich(
            ui,
            &[
                ("error: ", Face::PlexSemiBold.at(12.0), p.error),
                (error, Face::Plex.at(12.0), p.ink),
            ],
            width,
            18.0,
        );
        let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
        ui.painter().galley(r.min, g, p.ink);
    });
}

/// A folder as typed, `~` for the home folder.
fn expand(typed: &str, home: Option<&Path>) -> PathBuf {
    let typed = typed.trim();
    match (typed.strip_prefix('~'), home) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            home.join(rest.trim_start_matches(['/', '\\']))
        }
        _ => PathBuf::from(typed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_folder_expands_the_home_folder() {
        let home = Path::new("/home/me");
        assert_eq!(
            expand("~/Projects/a", Some(home)),
            Path::new("/home/me/Projects/a")
        );
        assert_eq!(expand("~", Some(home)), Path::new("/home/me"));
        assert_eq!(expand("~x/a", Some(home)), Path::new("~x/a"));
        assert_eq!(expand(" /abs ", Some(home)), Path::new("/abs"));
    }
}
