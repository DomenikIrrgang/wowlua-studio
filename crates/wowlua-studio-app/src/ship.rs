//! The ship page (M9, S061): the build profiles and their settings (a
//! form over `wowlua.json`: the designed fields, then wowlua's schema's
//! for the others; an edit changes only that value's text), a build with
//! its progress and output, the installation and the dev loop, and
//! publishing: the checks, the targets and their tokens, a dry run, a
//! publish confirmed against a fresh dry run, the version tagged and
//! pushed after a confirm.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use egui::{CornerRadius, Sense, Ui, Vec2};
use wowlua_studio_ui::forms::{self, CalloutKind, TermLine};
use wowlua_studio_ui::widgets::{self, BadgeKind, ButtonKind};
use wowlua_studio_ui::{Face, Palette, palette};

use crate::system::{
    BuildDone, BuildEvent, BuildProfile, BuildStep, ChangeKind, DevBuild, DevEvent, InstallFolder,
    InstallState, Installation, PublishDone, PublishEvent, ShipBuild, ShipDiagnostic, ShipEvent,
    ShipFacts, ShipInstall, ShipPublish, ShipTarget, System, TagPush, TokenState, Workspace,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Build,
    Install,
    Publish,
}

/// A confirm before something that leaves the workspace.
#[derive(Clone, Debug, PartialEq)]
enum Dialog {
    /// Upload, once the dry run of this job shows what each target gets.
    Publish {
        job: u64,
    },
    Tag(TagPush),
    /// Install over folders something else changed, or not wowlua's.
    Install {
        profile: String,
        force: bool,
    },
}

/// What the page asks of the rest of Studio.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Save(PathBuf),
    /// A file at a line and column (1-based).
    GoTo(PathBuf, u32, u32),
    PickInstallation,
    /// A build profile's settings, in its wowlua.json's form (S064).
    EditProfile(PathBuf, String),
    /// A publish target's build profile, set in its wowlua.json (S067).
    SetTargetProfile {
        manifest: PathBuf,
        target: String,
        profile: String,
    },
}

/// A build (or install) running.
struct Running {
    job: u64,
    profile: String,
    install: bool,
    step: Option<BuildStep>,
}

enum DevLog {
    Watching(Vec<String>),
    Built(DevBuild),
    Stopped(String),
}

#[derive(Default)]
pub struct ShipState {
    pub tab: Tab,
    /// The profile the panel shows and the build tab builds.
    pub selected: Option<String>,
    facts: Option<Result<ShipFacts, String>>,
    reading: bool,
    read_at: Option<f64>,
    stale: bool,
    next_job: u64,
    running: Option<Running>,
    last: Option<BuildDone>,
    pub dev_on: bool,
    dev_link: bool,
    dev_log: Vec<DevLog>,
    dev_building: Option<Vec<String>>,
    /// Targets switched off for the next publish.
    off: BTreeSet<String>,
    /// The targets' profiles picked, shown until the package read again
    /// has them (wowlua.json's tab may be unsaved).
    target_profiles: HashMap<String, String>,
    tokens: HashMap<String, TokenState>,
    tokens_asked: bool,
    token_text: HashMap<String, String>,
    token_said: HashMap<String, Result<String, String>>,
    publish_job: Option<u64>,
    publish_dry: bool,
    publish_lines: Vec<(String, String)>,
    dry: Option<PublishDone>,
    published: Option<PublishDone>,
    dialog: Option<Dialog>,
    tagging: bool,
    tagged: Option<Result<String, String>>,
    /// The addon picked in a workspace without a package of its own: its
    /// folder.
    picked: Option<PathBuf>,
}

impl ShipState {
    /// The folder of the package the page works on: the workspace's root
    /// package, or the addon picked in a workspace without one (S061).
    pub fn dir(&self, workspace: &Workspace) -> PathBuf {
        self.picked
            .clone()
            .unwrap_or_else(|| workspace.root.clone())
    }

    fn job(&mut self) -> u64 {
        self.next_job += 1;
        self.next_job
    }

    pub fn facts(&self) -> Option<&ShipFacts> {
        self.facts.as_ref()?.as_ref().ok()
    }

    pub fn files_changed(&mut self) {
        self.stale = true;
    }

    pub fn event(&mut self, event: ShipEvent) {
        match event {
            ShipEvent::Read(read) => {
                let read = *read;
                self.reading = false;
                if let Ok(facts) = &read
                    && self
                        .selected
                        .as_ref()
                        .is_none_or(|s| !facts.profiles.iter().any(|p| &p.name == s))
                {
                    self.selected = facts
                        .profiles
                        .iter()
                        .find(|p| p.name == "release")
                        .or(facts.profiles.first())
                        .map(|p| p.name.clone());
                }
                if let Ok(facts) = &read {
                    self.target_profiles.retain(|name, profile| {
                        facts
                            .targets
                            .iter()
                            .any(|t| &t.name == name && &t.profile != profile)
                    });
                }
                self.facts = Some(read);
            }
            ShipEvent::Tokens(list) => self.tokens = list.into_iter().collect(),
            ShipEvent::TokenSaved { kind, result } => {
                if result.is_ok() {
                    self.token_text.remove(&kind);
                    self.tokens_asked = false;
                }
                self.token_said.insert(kind, result);
            }
            ShipEvent::Build { job, event } => match event {
                BuildEvent::Step(step) => {
                    if let Some(r) = self.running.as_mut().filter(|r| r.job == job) {
                        r.step = Some(step);
                    }
                }
                BuildEvent::Done(done) => {
                    if self.running.as_ref().is_some_and(|r| r.job == job) {
                        self.running = None;
                        self.last = Some(done);
                        self.stale = true;
                    }
                }
            },
            ShipEvent::Dev(event) => {
                match event {
                    DevEvent::Watching(names) => self.dev_log.push(DevLog::Watching(names)),
                    DevEvent::Building(changed) => self.dev_building = Some(changed),
                    DevEvent::Built(build) => {
                        self.dev_building = None;
                        self.dev_log.push(DevLog::Built(build));
                        self.stale = true;
                    }
                    DevEvent::Stopped(why) => {
                        self.dev_on = false;
                        self.dev_building = None;
                        if let Some(why) = why {
                            self.dev_log.push(DevLog::Stopped(why));
                        }
                    }
                }
                let over = self.dev_log.len().saturating_sub(200);
                self.dev_log.drain(..over);
            }
            ShipEvent::Publish { job, event } => {
                if self.publish_job != Some(job) {
                    return;
                }
                match event {
                    PublishEvent::Line { label, text } => self.publish_lines.push((label, text)),
                    PublishEvent::Done(done) => {
                        self.publish_job = None;
                        if done.dry_run {
                            self.dry = Some(done);
                        } else {
                            self.published = Some(done);
                            self.stale = true;
                        }
                    }
                }
            }
            ShipEvent::Tagged(result) => {
                self.tagging = false;
                self.tagged = Some(result);
                self.stale = true;
            }
        }
    }

    /// Reads the package again when it changed (or every 15 seconds), and
    /// the tokens when the publish tab first shows.
    fn refresh(&mut self, workspace: &Workspace, system: &dyn System, now: f64) {
        let due = self.facts.is_none()
            || (self.stale && self.read_at.is_none_or(|t| now - t > 1.0))
            || self.read_at.is_none_or(|t| now - t > 15.0);
        if due && !self.reading {
            self.reading = true;
            self.stale = false;
            self.read_at = Some(now);
            system.ship_read(&self.dir(workspace));
        }
        if self.tab == Tab::Publish
            && !self.tokens_asked
            && let Some(facts) = self.facts()
            && !facts.targets.is_empty()
        {
            let targets = facts.targets.clone();
            self.tokens_asked = true;
            system.ship_tokens(targets);
        }
    }

    fn build(
        &mut self,
        workspace: &Workspace,
        system: &dyn System,
        profile: &str,
        install: Option<ShipInstall>,
    ) {
        let job = self.job();
        self.running = Some(Running {
            job,
            profile: profile.to_owned(),
            install: install.is_some(),
            step: None,
        });
        self.last = None;
        system.ship_build(
            &self.dir(workspace),
            job,
            ShipBuild {
                profile: profile.to_owned(),
                install,
            },
        );
    }

    fn publish(&mut self, workspace: &Workspace, system: &dyn System, dry_run: bool) -> u64 {
        let job = self.job();
        let facts = self.facts().cloned().unwrap_or_default();
        let targets = facts
            .targets
            .iter()
            .filter(|t| t.enabled && !self.off.contains(&t.name))
            .map(|t| t.name.clone())
            .collect();
        self.publish_job = Some(job);
        self.publish_dry = dry_run;
        self.publish_lines.clear();
        if dry_run {
            self.dry = None;
        } else {
            self.published = None;
        }
        system.ship_publish(
            &self.dir(workspace),
            job,
            ShipPublish {
                dry_run,
                targets,
                tests: !dry_run,
                tag: facts.tag.clone(),
            },
        );
        job
    }
}

impl ShipState {
    /// Does what the screenshot harness's `shipdo:` asks, once the package
    /// is read: `build`, `install`, `dry`, `publish` (its confirm), `tag`
    /// (its confirm), `dev`. False: not yet.
    pub fn act(&mut self, what: &str, workspace: &Workspace, system: &dyn System) -> bool {
        let Some(facts) = self.facts().cloned() else {
            return false;
        };
        let profile = self.selected.clone().unwrap_or_else(|| "release".into());
        match what {
            "build" => self.build(workspace, system, &profile, None),
            "install" => {
                let how = ShipInstall {
                    link: false,
                    force: false,
                };
                self.build(workspace, system, "release", Some(how));
            }
            "dry" => {
                self.publish(workspace, system, true);
            }
            "publish" => {
                let job = self.publish(workspace, system, true);
                self.dialog = Some(Dialog::Publish { job });
            }
            "tag" => {
                self.dialog = Some(Dialog::Tag(TagPush {
                    tag: facts.tag.clone(),
                    message: format!("{} {}", facts.addon, facts.version),
                    remote: facts.remote.clone().unwrap_or_else(|| "origin".into()),
                    branch: facts.branch.clone().unwrap_or_else(|| "main".into()),
                }));
            }
            "dev" => {
                self.dev_on = true;
                self.dev_log.clear();
                system.ship_dev(
                    &self.dir(workspace),
                    Some(ShipInstall {
                        link: self.dev_link,
                        force: false,
                    }),
                );
            }
            _ => {}
        }
        true
    }
}

/// A workspace without a package of its own: which addon the page works
/// on.
fn pick_addon(ui: &mut Ui, state: &mut ShipState, facts: &ShipFacts, p: &Palette) {
    forms::paragraph(
        ui,
        "This workspace has no package of its own: pick the addon to build, install and publish.",
        12.5,
        19.0,
        p.ink_muted,
    );
    for (name, dir) in &facts.members {
        if forms::text_button(ui, name).clicked() {
            state.picked = Some(dir.clone());
            state.facts = None;
            state.selected = None;
            state.read_at = None;
            state.tokens_asked = false;
        }
    }
}

// ------------------------------------------------------------- helpers

fn caption(ui: &mut Ui, text: &str, p: &Palette) {
    ui.label(
        egui::RichText::new(text)
            .font(Face::Plex.at(11.5))
            .color(p.ink_muted),
    );
}

fn strong(ui: &mut Ui, text: &str, p: &Palette) {
    ui.label(
        egui::RichText::new(text)
            .font(Face::PlexMedium.at(12.5))
            .color(p.ink),
    );
}

/// A size as people read it: `1.84 MB`, `212 KB`, `90 B`.
pub fn size(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1_000_000.0 {
        format!("{:.2} MB", b / 1_000_000.0)
    } else if b >= 1_000.0 {
        format!("{:.0} KB", b / 1_000.0)
    } else {
        format!("{bytes} B")
    }
}

/// A build's progress, 0 to 1, from its step.
pub fn fraction(step: Option<&BuildStep>) -> f32 {
    let of = |done: usize, of: usize| done as f32 / of.max(1) as f32;
    match step {
        None => 0.0,
        Some(BuildStep::Read { .. }) => 0.05,
        Some(BuildStep::Parsed { done, of: n }) => 0.05 + 0.25 * of(*done, *n),
        Some(BuildStep::Linted { done, of: n }) => 0.30 + 0.15 * of(*done, *n),
        Some(BuildStep::Checking { index, count, .. }) => 0.45 + 0.35 * of(*index, *count),
        Some(BuildStep::Bundling) => 0.82,
        Some(BuildStep::Writing) => 0.92,
        Some(BuildStep::Installing) => 0.96,
    }
}

/// What a build is doing, as the page says it.
pub fn step_text(step: Option<&BuildStep>, files: Option<usize>) -> String {
    match step {
        None => "loading the package".into(),
        Some(BuildStep::Read { files }) => format!("read {files} files"),
        Some(BuildStep::Parsed { done, of }) => format!("parsing · {done} of {of}"),
        Some(BuildStep::Linted { done, of }) => format!("linting · {done} of {of}"),
        Some(BuildStep::Checking {
            flavor,
            index,
            count,
        }) => {
            let n = files.map_or(String::new(), |f| format!("{f} files"));
            match flavor {
                Some(f) if *count > 1 => format!("checking {n} for {f} · {} of {count}", index + 1),
                _ => format!("checking {n}"),
            }
        }
        Some(BuildStep::Bundling) => "bundling".into(),
        Some(BuildStep::Writing) => "writing the AddOn folder".into(),
        Some(BuildStep::Installing) => "installing".into(),
    }
}

/// A diagnostic as compiler output: `error[E2003]: … (src/x.wowlua:68:20)`.
pub fn diagnostic_line(d: &ShipDiagnostic, root: &Path) -> String {
    let head = match &d.code {
        Some(code) => format!("{}[{code}]", d.severity),
        None => d.severity.clone(),
    };
    let at = d.path.as_ref().map(|path| {
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        match (d.line, d.column) {
            (Some(l), Some(c)) => format!(" ({rel}:{l}:{c})"),
            _ => format!(" ({rel})"),
        }
    });
    format!("{head}: {}{}", d.message, at.unwrap_or_default())
}

fn meter(ui: &mut Ui, fraction: f32, p: &Palette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::ZERO, p.bg_300);
    let mut fill = rect;
    fill.set_width(rect.width() * fraction.clamp(0.0, 1.0));
    painter.rect_filled(fill, CornerRadius::ZERO, p.success);
}

/// The tag a target's token shows.
fn token_text(state: Option<&TokenState>) -> (&'static str, Option<String>) {
    match state {
        None => ("reading", None),
        Some(TokenState::Saved) => ("saved", None),
        Some(TokenState::Missing) => ("missing", None),
        Some(TokenState::FromEnv(var)) => ("from", Some(var.clone())),
        Some(TokenState::Unreadable(why)) => ("unreadable", Some(why.clone())),
    }
}

/// A profile's summary for its row: `bundle · minify max · optimized`.
pub fn summary(profile: &BuildProfile) -> String {
    let e = &profile.effective;
    let mut parts: Vec<String> = Vec::new();
    if let Some(parent) = profile.chain.iter().rev().nth(1) {
        parts.push(format!("inherits {parent}"));
    }
    if e["bundle"].as_bool() == Some(true) {
        parts.push("bundle".into());
    }
    match e["minify"].as_str() {
        Some("none") | None => {}
        Some(level) => parts.push(format!("minify {level}")),
    }
    let optimized = e["optimize"].as_object().is_some_and(|o| {
        o.values()
            .any(|v| v.as_bool() == Some(true) || v.as_str() == Some("safe"))
    });
    if optimized {
        parts.push("optimized".into());
    }
    if parts.is_empty() || (parts.len() == 1 && parts[0].starts_with("inherits")) {
        parts.push("files as written".into());
    }
    parts.join(" · ")
}

/// Who builds with a profile: the targets that use it, the tests, or
/// wowlua.
fn used_by(profile: &BuildProfile, facts: &ShipFacts) -> String {
    let targets: Vec<&str> = facts
        .targets
        .iter()
        .filter(|t| t.profile == profile.name)
        .map(|t| t.name.as_str())
        .collect();
    if !targets.is_empty() {
        targets.join(" · ")
    } else if profile.name == "test" {
        "tests".into()
    } else if profile.name == "dev" {
        "dev loop".into()
    } else if profile.builtin {
        "built in".into()
    } else {
        String::new()
    }
}

// --------------------------------------------------------- the profiles

/// The ship page's profiles: the one the build tab builds. Their settings
/// are edited in wowlua.json's build section (S064).
pub fn profiles(
    ui: &mut Ui,
    state: &mut ShipState,
    workspace: &Workspace,
    system: &dyn System,
) -> Option<Action> {
    let now = ui.input(|i| i.time);
    state.refresh(workspace, system, now);
    let p = palette(ui.ctx());
    let facts = match &state.facts {
        Some(Ok(f)) => f.clone(),
        Some(Err(why)) => {
            forms::padded(ui, 12, 10, |ui| {
                forms::paragraph(ui, why, 12.5, 19.0, p.ink_muted);
            });
            return None;
        }
        None => {
            forms::padded(ui, 12, 10, |ui| caption(ui, "reading the package…", p));
            return None;
        }
    };
    if !facts.members.is_empty() {
        forms::padded(ui, 12, 10, |ui| pick_addon(ui, state, &facts, p));
        return None;
    }
    let mut action = None;
    egui::ScrollArea::vertical()
        .id_salt("ship-profiles")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for profile in &facts.profiles {
                let selected = state.selected.as_deref() == Some(profile.name.as_str());
                if row(ui, p, profile, &used_by(profile, &facts), selected) {
                    state.selected = Some(profile.name.clone());
                }
            }
            let Some(profile) = state
                .selected
                .as_ref()
                .and_then(|s| facts.profiles.iter().find(|p| &p.name == s))
            else {
                return;
            };
            forms::padded(ui, 12, 10, |ui| {
                ui.set_width(ui.available_width());
                if let Some(why) = &profile.problem {
                    forms::callout(ui, CalloutKind::Error, |ui| {
                        forms::paragraph(ui, why, 12.0, 18.0, p.ink);
                    });
                }
                if widgets::button(
                    ui,
                    ButtonKind::Secondary,
                    &format!("edit {} in wowlua.json", profile.name),
                    true,
                    None,
                )
                .clicked()
                {
                    action = Some(Action::EditProfile(
                        facts.manifest.clone(),
                        profile.name.clone(),
                    ));
                }
                caption(
                    ui,
                    "A profile's settings, and new profiles, are edited in wowlua.json's build section, which opens in the editor.",
                    p,
                );
            });
        });
    action
}

/// A profile's row: its name, who uses it, its summary.
fn row(ui: &mut Ui, p: &Palette, profile: &BuildProfile, used: &str, selected: bool) -> bool {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 46.0), Sense::click());
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, CornerRadius::ZERO, p.bg_400);
        let bar = egui::Rect::from_min_size(rect.min, Vec2::new(2.0, rect.height()));
        painter.rect_filled(bar, CornerRadius::ZERO, p.amber);
    } else if response.hovered() {
        painter.rect_filled(rect, CornerRadius::ZERO, p.bg_300);
    }
    painter.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        egui::Stroke::new(1.0, p.line),
    );
    let name_color = if profile.problem.is_some() {
        p.error
    } else {
        p.ink
    };
    painter.text(
        rect.min + Vec2::new(12.0, 8.0),
        egui::Align2::LEFT_TOP,
        &profile.name,
        Face::PlexMedium.at(12.5),
        name_color,
    );
    painter.text(
        egui::pos2(rect.right() - 12.0, rect.top() + 8.0),
        egui::Align2::RIGHT_TOP,
        used,
        Face::Plex.at(11.0),
        p.ink_muted,
    );
    painter.text(
        rect.min + Vec2::new(12.0, 26.0),
        egui::Align2::LEFT_TOP,
        summary(profile),
        Face::Plex.at(11.0),
        p.ink_muted,
    );
    response.clicked()
}

// ------------------------------------------------------------- the page

pub fn ship(
    ui: &mut Ui,
    state: &mut ShipState,
    workspace: &Workspace,
    system: &dyn System,
    installation: Option<&Installation>,
) -> Option<Action> {
    let now = ui.input(|i| i.time);
    state.refresh(workspace, system, now);
    let p = palette(ui.ctx());
    let mut action = None;
    forms::padded(ui, 16, 8, |ui| {
        ui.horizontal(|ui| {
            let at = state.tab as usize;
            if let Some(i) = widgets::seg(
                ui,
                "ship-tab",
                &["build", "install and dev", "publish"],
                at,
                false,
            ) {
                state.tab = [Tab::Build, Tab::Install, Tab::Publish][i];
            }
            if let Some(f) = state.facts() {
                caption(ui, &format!("{} {}", f.addon, f.version), p);
            }
            if state.reading {
                caption(ui, "reading…", p);
            }
        });
    });
    let facts = match &state.facts {
        Some(Ok(f)) => f.clone(),
        Some(Err(why)) => {
            forms::padded(ui, 16, 8, |ui| {
                forms::callout(ui, CalloutKind::Error, |ui| {
                    forms::paragraph(ui, why, 12.5, 19.0, p.ink);
                });
            });
            return None;
        }
        None => return None,
    };
    if !facts.members.is_empty() {
        forms::padded(ui, 22, 14, |ui| pick_addon(ui, state, &facts, p));
        return None;
    }
    egui::ScrollArea::vertical()
        .id_salt("ship-page")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            forms::padded(ui, 22, 14, |ui| {
                ui.set_width(ui.available_width());
                let a = match state.tab {
                    Tab::Build => build_tab(ui, state, &facts, workspace, system, p),
                    Tab::Install => {
                        install_tab(ui, state, &facts, workspace, system, installation, p)
                    }
                    Tab::Publish => publish_tab(ui, state, &facts, workspace, system, p),
                };
                if a.is_some() {
                    action = a;
                }
            });
        });
    action
}

fn build_tab(
    ui: &mut Ui,
    state: &mut ShipState,
    facts: &ShipFacts,
    workspace: &Workspace,
    system: &dyn System,
    p: &Palette,
) -> Option<Action> {
    let mut action = None;
    let profile = state.selected.clone().unwrap_or_else(|| "release".into());
    let busy = state.running.is_some();
    ui.horizontal(|ui| {
        ui.add_enabled_ui(!busy, |ui| {
            if widgets::button(
                ui,
                ButtonKind::Primary,
                &format!("build {profile}"),
                true,
                None,
            )
            .clicked()
            {
                state.build(workspace, system, &profile, None);
            }
        });
        let out = facts
            .out
            .strip_prefix(&workspace.root)
            .unwrap_or(&facts.out);
        caption(
            ui,
            &format!(
                "wowlua build --profile {profile} · into {}/{profile}/{}",
                out.to_string_lossy(),
                facts.addon
            ),
            p,
        );
    });
    ui.add_space(10.0);
    if let Some(r) = &state.running {
        let files = match r.step {
            Some(BuildStep::Read { files }) => Some(files),
            _ => None,
        };
        caption(
            ui,
            &format!("{} · {}", r.profile, step_text(r.step.as_ref(), files)),
            p,
        );
        meter(ui, fraction(r.step.as_ref()), p);
        return None;
    }
    if let Some(done) = &state.last {
        let errors: Vec<&ShipDiagnostic> = done
            .diagnostics
            .iter()
            .filter(|d| d.severity == "error")
            .collect();
        if !done.ok {
            forms::callout(ui, CalloutKind::Error, |ui| {
                for d in errors.iter().take(3) {
                    ui.horizontal_wrapped(|ui| {
                        forms::paragraph(
                            ui,
                            &diagnostic_line(d, &workspace.root),
                            12.5,
                            19.0,
                            p.ink,
                        );
                        if let (Some(path), Some(l), Some(c)) = (&d.path, d.line, d.column)
                            && forms::text_button(ui, "go to the error").clicked()
                        {
                            action = Some(Action::GoTo(path.clone(), l, c));
                        }
                    });
                }
                let n = errors.len();
                caption(
                    ui,
                    &format!(
                        "the build stopped: {n} error{} · nothing was written to {}/{}",
                        if n == 1 { "" } else { "s" },
                        facts
                            .out
                            .strip_prefix(&workspace.root)
                            .unwrap_or(&facts.out)
                            .to_string_lossy(),
                        done.profile
                    ),
                    p,
                );
            });
            ui.add_space(10.0);
        }
    }
    // The selected profile's output and the sizes of each.
    let output = facts.outputs.iter().find(|o| o.profile == profile);
    ui.columns(2, |cols| {
        let ui = &mut cols[0];
        let title = format!("{}/{}", profile, facts.addon);
        forms::card(
            ui,
            &title,
            |ui| {
                if let Some(o) = output {
                    caption(
                        ui,
                        &o.modified
                            .map_or(String::new(), |m| format!("built {}", ago(m))),
                        p,
                    );
                }
            },
            |ui| {
                forms::padded(ui, 12, 10, |ui| match output {
                    None => caption(ui, "Not built yet.", p),
                    Some(o) => {
                        for (name, meta) in tree(&o.files) {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(name)
                                        .font(Face::Plex.at(12.5))
                                        .color(p.ink),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        caption(ui, &meta, p);
                                    },
                                );
                            });
                        }
                    }
                });
            },
        );
        let ui = &mut cols[1];
        forms::card(
            ui,
            "size by profile",
            |_| {},
            |ui| {
                forms::padded(ui, 12, 10, |ui| {
                    if facts.outputs.is_empty() {
                        caption(ui, "No profile is built yet.", p);
                    }
                    egui::Grid::new("ship-sizes")
                        .num_columns(4)
                        .spacing([16.0, 6.0])
                        .show(ui, |ui| {
                            for h in ["profile", "largest Lua file", "all", "built"] {
                                caption(ui, h, p);
                            }
                            ui.end_row();
                            for o in &facts.outputs {
                                let lua = o
                                    .files
                                    .iter()
                                    .filter(|(n, _)| n.ends_with(".lua"))
                                    .max_by_key(|(_, s)| *s);
                                let total: u64 = o.files.iter().map(|(_, s)| s).sum();
                                strong(ui, &o.profile, p);
                                caption(
                                    ui,
                                    &lua.map_or("—".into(), |(n, s)| {
                                        format!(
                                            "{} · {}",
                                            size(*s),
                                            n.rsplit('/').next().unwrap_or(n)
                                        )
                                    }),
                                    p,
                                );
                                caption(ui, &format!("{} · {}", size(total), o.files.len()), p);
                                caption(ui, &o.modified.map_or(String::new(), ago), p);
                                ui.end_row();
                            }
                        });
                });
            },
        );
    });
    // The compiler's output.
    if let Some(done) = &state.last {
        ui.add_space(10.0);
        let lines: Vec<String> = done
            .diagnostics
            .iter()
            .map(|d| diagnostic_line(d, &workspace.root))
            .collect();
        let finished = format!(
            "{} in {:.1} s{}",
            done.profile,
            done.ms as f64 / 1000.0,
            done.dir.as_ref().map_or(String::new(), |d| format!(
                " · {}",
                d.strip_prefix(&workspace.root)
                    .unwrap_or(d)
                    .to_string_lossy()
            ))
        );
        let mut term: Vec<TermLine<'_>> = lines.iter().map(|l| TermLine::Out(l)).collect();
        if done.ok {
            term.push(TermLine::Status("Finished", &finished));
        }
        forms::terminal(ui, None, &term);
    }
    action
}

/// The top of an AddOn folder: its files, and its folders with how many
/// files and bytes they hold.
pub fn tree(files: &[(String, u64)]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut folders: Vec<(String, usize, u64)> = Vec::new();
    for (path, size_of) in files {
        match path.split_once('/') {
            None => out.push((path.clone(), size(*size_of))),
            Some((dir, _)) => match folders.iter_mut().find(|(d, _, _)| d == dir) {
                Some(f) => {
                    f.1 += 1;
                    f.2 += size_of;
                }
                None => folders.push((dir.to_owned(), 1, *size_of)),
            },
        }
    }
    out.extend(folders.into_iter().map(|(d, n, b)| {
        (
            format!("{d}/"),
            format!("{n} file{} · {}", if n == 1 { "" } else { "s" }, size(b)),
        )
    }));
    out
}

/// How long ago a time (ms since the epoch) was.
fn ago(ms: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    ago_text(now.saturating_sub(ms) / 1000)
}

/// `just now`, `40 s ago`, `12 min ago`, `3 h ago`, `2 days ago`.
pub fn ago_text(secs: u64) -> String {
    match secs {
        0..10 => "just now".into(),
        10..60 => format!("{secs} s ago"),
        60..3_600 => format!("{} min ago", secs / 60),
        3_600..86_400 => format!("{} h ago", secs / 3_600),
        86_400..172_800 => "a day ago".into(),
        _ => format!("{} days ago", secs / 86_400),
    }
}

fn changed_files(folder: &InstallFolder) -> Option<&Vec<crate::system::InstallChange>> {
    match &folder.state {
        InstallState::Copied(Some(c)) if !c.is_empty() => Some(c),
        _ => None,
    }
}

fn install_tab(
    ui: &mut Ui,
    state: &mut ShipState,
    facts: &ShipFacts,
    workspace: &Workspace,
    system: &dyn System,
    installation: Option<&Installation>,
    p: &Palette,
) -> Option<Action> {
    let mut action = None;
    let busy = state.running.is_some();
    forms::card(
        ui,
        "your WoW installation",
        |ui| {
            if widgets::button(ui, ButtonKind::Tertiary, "change", true, None).clicked() {
                action = Some(Action::PickInstallation);
            }
        },
        |ui| {
            forms::padded(ui, 12, 10, |ui| {
                match installation.and_then(|i| i.path.as_ref()) {
                    Some(path) => {
                        let clients: Vec<String> = installation
                            .map(|i| {
                                i.clients
                                    .iter()
                                    .map(|c| format!("{} {}", c.folder, c.client))
                                    .collect()
                            })
                            .unwrap_or_default();
                        strong(ui, &path.to_string_lossy(), p);
                        caption(ui, &clients.join(" · "), p);
                    }
                    None => caption(
                        ui,
                        installation
                            .and_then(|i| i.problem.as_deref())
                            .unwrap_or("No installation found."),
                        p,
                    ),
                }
                caption(
                    ui,
                    "\"change\" picks the folder and keeps it as wow.path in ~/.wowlua/config.json: wowlua's commands, sessions and the game's art use the same installation.",
                    p,
                );
                ui.add_space(8.0);
                // Where the package is installed.
                for folder in &facts.installs {
                    ui.horizontal_wrapped(|ui| {
                        strong(ui, &folder.client, p);
                        let (text, kind) = match &folder.state {
                            InstallState::Absent => ("not installed".to_owned(), None),
                            InstallState::Linked(to) => (
                                format!(
                                    "linked to {}",
                                    to.strip_prefix(&workspace.root)
                                        .unwrap_or(to)
                                        .to_string_lossy()
                                ),
                                Some(BadgeKind::Neutral),
                            ),
                            InstallState::Copied(Some(c)) if !c.is_empty() => (
                                format!(
                                    "{} file{} changed outside Studio",
                                    c.len(),
                                    if c.len() == 1 { "" } else { "s" }
                                ),
                                Some(BadgeKind::Warning),
                            ),
                            InstallState::Copied(_) => {
                                ("installed".to_owned(), Some(BadgeKind::Done))
                            }
                            InstallState::Foreign => (
                                "not installed by wowlua (an addon manager's?)".to_owned(),
                                Some(BadgeKind::Error),
                            ),
                        };
                        match kind {
                            Some(k) => {
                                widgets::badge(ui, &text, k);
                            }
                            None => caption(ui, &text, p),
                        }
                    });
                }
                if let Some(why) = &facts.install_problem
                    && facts.installs.is_empty()
                {
                    caption(ui, why, p);
                }
            });
        },
    );
    // Folders something else changed.
    for folder in &facts.installs {
        let Some(changes) = changed_files(folder) else {
            continue;
        };
        ui.add_space(8.0);
        forms::callout(ui, CalloutKind::Warning, |ui| {
            forms::paragraph(
                ui,
                &format!(
                    "{}/Interface/AddOns/{} is not what wowlua installed: installing replaces it.",
                    folder.client, facts.addon
                ),
                12.5,
                19.0,
                p.ink,
            );
            for c in changes.iter().take(12) {
                let (what, meta) = match c.kind {
                    ChangeKind::Changed { was, now } => {
                        ("changed", format!("{} → {}", size(was), size(now)))
                    }
                    ChangeKind::Added { size: s } => ("added", size(s)),
                    ChangeKind::Removed => ("removed", String::new()),
                };
                let at = c
                    .modified
                    .map_or(String::new(), |m| format!(" · {}", ago(m)));
                caption(ui, &format!("{what:<8} {}  {meta}{at}", c.path), p);
            }
            if changes.len() > 12 {
                caption(ui, &format!("and {} more", changes.len() - 12), p);
            }
            if widgets::button(ui, ButtonKind::Tertiary, "show the folder", true, None).clicked() {
                system.reveal(&folder.path);
            }
        });
    }
    ui.add_space(10.0);
    let release = "release";
    ui.horizontal(|ui| {
        ui.add_enabled_ui(!busy, |ui| {
            if widgets::button(ui, ButtonKind::Primary, "install release", true, None).clicked() {
                let changed = facts.installs.iter().any(|f| changed_files(f).is_some());
                let foreign = facts.installs.iter().any(|f| f.state == InstallState::Foreign);
                if changed || foreign {
                    state.dialog = Some(Dialog::Install {
                        profile: release.into(),
                        force: foreign,
                    });
                } else {
                    state.build(workspace, system, release, Some(ShipInstall { link: false, force: false }));
                }
            }
        });
        caption(
            ui,
            &format!(
                "wowlua install --release · builds once into Interface/AddOns/{}, then /reload in game",
                facts.addon
            ),
            p,
        );
    });
    if let Some(r) = state.running.as_ref().filter(|r| r.install) {
        caption(ui, &step_text(r.step.as_ref(), None), p);
        meter(ui, fraction(r.step.as_ref()), p);
    } else if let Some(done) = state
        .last
        .as_ref()
        .filter(|d| !d.installed.is_empty() || !d.ok)
    {
        ui.add_space(6.0);
        if done.ok {
            forms::callout(ui, CalloutKind::Ok, |ui| {
                forms::paragraph(
                    ui,
                    &format!(
                        "installed {} ({}) into {} · type /reload in game to load it",
                        facts.addon,
                        done.profile,
                        done.installed
                            .iter()
                            .filter_map(|p| p
                                .parent()
                                .and_then(|a| a.parent())
                                .and_then(|i| i.parent()))
                            .filter_map(|c| c.file_name().map(|n| n.to_string_lossy().into_owned()))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    12.5,
                    19.0,
                    p.ink,
                );
            });
        } else {
            forms::callout(ui, CalloutKind::Error, |ui| {
                for d in done
                    .diagnostics
                    .iter()
                    .filter(|d| d.severity == "error")
                    .take(3)
                {
                    forms::paragraph(ui, &diagnostic_line(d, &workspace.root), 12.5, 19.0, p.ink);
                }
            });
        }
    }
    caption(
        ui,
        &format!(
            "Interface/AddOns/{} is build output: Studio never edits it. It remembers what it installed and, before the next install, lists what something else changed.",
            facts.addon
        ),
        p,
    );
    ui.add_space(12.0);
    // The dev loop.
    let mut on = state.dev_on;
    let mut toggled = false;
    forms::card(
        ui,
        "dev loop",
        |ui| {
            toggled = forms::checkbox(ui, &mut on, "on", None);
        },
        |ui| {
            forms::padded(ui, 12, 10, |ui| {
                caption(
                    ui,
                    "wowlua dev: builds the dev profile on every change on disk (a save here, another editor, a git checkout) and syncs it into AddOns, with wowlua's in-game profiling tools. You /reload in game.",
                    p,
                );
                ui.add_enabled_ui(!state.dev_on, |ui| {
                    forms::checkbox(
                        ui,
                        &mut state.dev_link,
                        "link the AddOn folder to the build instead of copying",
                        None,
                    );
                });
                if state.dev_on || !state.dev_log.is_empty() {
                    let lines = dev_lines(state, &workspace.root);
                    let term: Vec<TermLine<'_>> = lines.iter().map(|l| TermLine::Out(l)).collect();
                    forms::terminal(ui, None, &term);
                }
            });
        },
    );
    if toggled {
        state.dev_on = on;
        if on {
            state.dev_log.clear();
            let how = ShipInstall {
                link: state.dev_link,
                force: false,
            };
            system.ship_dev(&state.dir(workspace), Some(how));
        } else {
            system.ship_dev(&state.dir(workspace), None);
        }
    }
    action
}

/// The dev loop's log, as `wowlua dev` prints it.
pub fn dev_lines(state: &ShipState, root: &Path) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for entry in &state.dev_log {
        lines.push(match entry {
            DevLog::Watching(names) => format!("[dev] watching {}", names.join(", ")),
            DevLog::Stopped(why) => format!("[dev] stopped: {why}"),
            DevLog::Built(b) => {
                let what = match b.changed.as_slice() {
                    [] => "first build".to_owned(),
                    [one] => format!("{} changed", one.rsplit('/').next().unwrap_or(one)),
                    many => format!("{} files changed", many.len()),
                };
                if b.ok {
                    let synced = match b.synced.len() {
                        0 => "linked: nothing to sync".to_owned(),
                        1 => "synced".to_owned(),
                        n => format!("synced into {n} clients"),
                    };
                    format!(
                        "[dev] {} {what} · built dev in {:.1} s · {synced} · /reload in game",
                        b.clock,
                        b.ms as f64 / 1000.0
                    )
                } else {
                    let first = b
                        .diagnostics
                        .iter()
                        .find(|d| d.severity == "error")
                        .map_or("the build failed".to_owned(), |d| diagnostic_line(d, root));
                    format!("[dev] {} {what} · {first} · nothing synced", b.clock)
                }
            }
        });
    }
    if let Some(changed) = &state.dev_building {
        lines.push(format!(
            "[dev] building{}",
            if changed.is_empty() {
                String::new()
            } else {
                format!(" · {}", changed.join(", "))
            }
        ));
    }
    lines
}

/// The publish checks: whether each passes, its text and where it comes
/// from. `None`: told by the publish (the tests).
pub fn checks(facts: &ShipFacts) -> Vec<(Option<bool>, String, String)> {
    let branch = facts.branch.clone().unwrap_or_default();
    let mut out = vec![(
        Some(facts.clean),
        if facts.clean {
            "the working tree is clean".to_owned()
        } else {
            "the working tree has changes: a publish needs a clean tree".to_owned()
        },
        branch,
    )];
    out.push(match facts.tag_at_head {
        None => (
            Some(true),
            format!("package.version is new: {}", facts.version),
            "wowlua.json".into(),
        ),
        Some(true) => (
            Some(true),
            format!(
                "{} is tagged on this commit: publishing uploads it again",
                facts.tag
            ),
            "wowlua.json".into(),
        ),
        Some(false) => (
            Some(false),
            format!(
                "package.version {} is already released ({} is on another commit)",
                facts.version, facts.tag
            ),
            "raise it in wowlua.json".into(),
        ),
    });
    let file = |p: &Path| {
        p.file_name()
            .map_or(String::new(), |n| n.to_string_lossy().into_owned())
    };
    out.push(match &facts.changelog {
        Some((path, text)) if !text.trim().is_empty() => (
            Some(true),
            format!("{} has a section for this version", file(path)),
            "uploaded as the notes".into(),
        ),
        Some((path, _)) => (
            Some(false),
            format!("{} has no section for {}", file(path), facts.version),
            "the notes".into(),
        ),
        None => (
            Some(true),
            "no changelog: the sites get none".into(),
            String::new(),
        ),
    });
    out.push((None, "the tests pass".into(), "run before packaging".into()));
    out.push((
        Some(true),
        "a tag will match the version, or publish stops (E4319)".into(),
        facts.tag.clone(),
    ));
    out
}

/// A publish target's row in the targets table: published or not, its
/// name, project, build profile (written to wowlua.json, S067) and token.
fn target_row(
    ui: &mut Ui,
    state: &mut ShipState,
    facts: &ShipFacts,
    t: &ShipTarget,
    system: &dyn System,
    p: &Palette,
) -> Option<Action> {
    let mut action = None;
    let mut checked = t.enabled && !state.off.contains(&t.name);
    // (in a cell of the row's height, centred as the rest)
    ui.allocate_ui_with_layout(
        egui::Vec2::new(18.0, forms::CONTROL_H),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            if !t.enabled {
                ui.disable();
            }
            if forms::checkbox(ui, &mut checked, "", None) {
                if checked {
                    state.off.remove(&t.name);
                } else {
                    state.off.insert(t.name.clone());
                }
            }
        },
    );
    strong(ui, &t.name, p);
    caption(ui, &t.project, p);
    // Its build profile, written to wowlua.json.
    let shown = state
        .target_profiles
        .get(&t.name)
        .unwrap_or(&t.profile)
        .clone();
    let mut names: Vec<&str> = facts.profiles.iter().map(|p| p.name.as_str()).collect();
    if !names.contains(&shown.as_str()) {
        names.push(&shown);
    }
    let mut index = names.iter().position(|n| *n == shown).unwrap_or(0);
    let opts: Vec<(&str, bool)> = names.iter().map(|n| (*n, true)).collect();
    if forms::select(
        ui,
        &format!("ship-target-profile-{}", t.name),
        &opts,
        &mut index,
        112.0,
    ) && names[index] != shown
    {
        let profile = names[index].to_owned();
        state
            .target_profiles
            .insert(t.name.clone(), profile.clone());
        action = Some(Action::SetTargetProfile {
            manifest: facts.manifest.clone(),
            target: t.name.clone(),
            profile,
        });
    }
    ui.horizontal(|ui| {
        let (word, more) = token_text(state.tokens.get(&t.name));
        match word {
            "saved" => {
                ui.label(
                    egui::RichText::new("token saved")
                        .font(Face::Plex.at(12.0))
                        .color(p.success),
                );
                if forms::text_button(ui, "remove").clicked() {
                    system.ship_token(&t.kind, None);
                }
            }
            "from" => caption(ui, &format!("token from ${}", more.unwrap_or_default()), p),
            "reading" => caption(ui, "reading the token…", p),
            _ => {
                if let Some(why) = more {
                    caption(ui, &why, p);
                }
                let input = state.token_text.entry(t.kind.clone()).or_default();
                ui.add(
                    egui::TextEdit::singleline(input)
                        .password(true)
                        .desired_width(110.0)
                        .font(Face::Plex.at(12.0))
                        .hint_text("token"),
                );
                if widgets::button(ui, ButtonKind::Secondary, "save", true, None).clicked() {
                    let token = input.clone();
                    system.ship_token(&t.kind, Some(token));
                }
            }
        }
    });
    action
}

fn publish_tab(
    ui: &mut Ui,
    state: &mut ShipState,
    facts: &ShipFacts,
    workspace: &Workspace,
    system: &dyn System,
    p: &Palette,
) -> Option<Action> {
    let mut action = None;
    let checks = checks(facts);
    let failing = checks.iter().any(|(ok, _, _)| *ok == Some(false));
    let on: Vec<&ShipTarget> = facts
        .targets
        .iter()
        .filter(|t| t.enabled && !state.off.contains(&t.name))
        .collect();
    let tokenless = on.iter().any(|t| {
        matches!(
            state.tokens.get(&t.name),
            Some(TokenState::Missing | TokenState::Unreadable(_))
        )
    });
    let busy = state.publish_job.is_some() || state.tagging;
    ui.columns(2, |cols| {
        let ui = &mut cols[0];
        let title = format!("{} {}", facts.addon, facts.version);
        forms::card(
            ui,
            &title,
            |ui| {
                widgets::badge(ui, &facts.release, BadgeKind::Neutral);
            },
            |ui| {
                forms::padded(ui, 12, 8, |ui| {
                    for (ok, text, meta) in &checks {
                        ui.horizontal_wrapped(|ui| {
                            let (mark, color) = match ok {
                                Some(true) => ("✓", p.success),
                                Some(false) => ("×", p.error),
                                None => ("·", p.ink_faint),
                            };
                            ui.label(egui::RichText::new(mark).font(Face::PlexSemiBold.at(12.5)).color(color));
                            ui.label(egui::RichText::new(text).font(Face::Plex.at(12.5)).color(p.ink));
                            caption(ui, meta, p);
                        });
                    }
                });
            },
        );
        ui.add_space(12.0);
        forms::card(
            ui,
            "targets",
            |_| {},
            |ui| {
                forms::padded(ui, 12, 8, |ui| {
                    if facts.targets.is_empty() {
                        caption(ui, "No publish targets: add one under publish.targets in wowlua.json.", p);
                    }
                    if !facts.targets.is_empty() {
                        egui::Grid::new("ship-targets")
                            .num_columns(5)
                            .spacing([10.0, 6.0])
                            .show(ui, |ui| {
                                for h in ["", "target", "project", "profile", "token"] {
                                    caption(ui, h, p);
                                }
                                ui.end_row();
                                for t in &facts.targets {
                                    if let Some(a) = target_row(ui, state, facts, t, system, p) {
                                        action = Some(a);
                                    }
                                    ui.end_row();
                                }
                            });
                    }
                    for t in &facts.targets {
                        if let Some(said) = state.token_said.get(&t.kind) {
                            match said {
                                Ok(s) => caption(ui, &format!("{}: {s}", t.kind), p),
                                Err(e) => {
                                    ui.label(egui::RichText::new(format!("{}: {e}", t.kind)).font(Face::Plex.at(11.5)).color(p.error));
                                }
                            }
                        }
                    }
                    caption(
                        ui,
                        "Tokens are kept in the OS credential store, the one wowlua login uses: a token saved here works for wowlua publish too. Studio never shows a saved token.",
                        p,
                    );
                });
            },
        );
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(!busy && !on.is_empty(), |ui| {
                if widgets::button(ui, ButtonKind::Secondary, "dry run", true, None).clicked() {
                    state.publish(workspace, system, true);
                }
            });
            ui.add_enabled_ui(!busy && !failing && !tokenless && !on.is_empty(), |ui| {
                if widgets::button(ui, ButtonKind::Primary, "publish from Studio", true, None).clicked() {
                    let job = state.publish(workspace, system, true);
                    state.dialog = Some(Dialog::Publish { job });
                }
            });
            caption(ui, "or", p);
            let can_tag = facts.clean && facts.tag_at_head.is_none() && facts.branch.is_some() && facts.remote.is_some();
            ui.add_enabled_ui(!busy && can_tag && !failing, |ui| {
                if widgets::button(ui, ButtonKind::Secondary, &format!("tag {} and push", facts.tag), true, None).clicked() {
                    state.dialog = Some(Dialog::Tag(TagPush {
                        tag: facts.tag.clone(),
                        message: format!("{} {}", facts.addon, facts.version),
                        remote: facts.remote.clone().unwrap_or_default(),
                        branch: facts.branch.clone().unwrap_or_default(),
                    }));
                }
            });
            caption(
                ui,
                &match &facts.ci {
                    Some(ci) => format!("{ci} runs wowlua publish for the tag"),
                    None => "no CI workflow here runs wowlua publish".into(),
                },
                p,
            );
        });

        let ui = &mut cols[1];
        let notes_title = format!("changelog · {}", facts.version);
        forms::card(
            ui,
            &notes_title,
            |ui| caption(ui, "what every site receives", p),
            |ui| {
                forms::padded(ui, 12, 10, |ui| match &facts.changelog {
                    Some((_, text)) if !text.trim().is_empty() => {
                        ui.label(egui::RichText::new(text.trim()).font(Face::Plex.at(12.0)).color(p.ink));
                    }
                    _ => caption(ui, "No section for this version.", p),
                });
            },
        );
        // A run: its lines, then what each target got.
        if !state.publish_lines.is_empty() || state.publish_job.is_some() {
            ui.add_space(12.0);
            let command = if state.publish_dry { "wowlua publish --dry-run" } else { "wowlua publish" };
            let mut term: Vec<TermLine<'_>> = vec![TermLine::Command(command)];
            for (label, text) in &state.publish_lines {
                term.push(TermLine::Status(label, text));
            }
            forms::terminal(ui, None, &term);
        }
        let last = if state.publish_dry { state.dry.as_ref() } else { state.published.as_ref() };
        if let Some(done) = last.filter(|_| state.publish_job.is_none()) {
            ui.add_space(8.0);
            outcomes(ui, done, &workspace.root, p);
        }
        if let Some(tagged) = &state.tagged {
            ui.add_space(8.0);
            match tagged {
                Ok(_) => forms::callout(ui, CalloutKind::Note, |ui| {
                    forms::paragraph(
                        ui,
                        &format!("pushed {} · the CI run builds and publishes it", facts.tag),
                        12.5,
                        19.0,
                        p.ink,
                    );
                }),
                Err(e) => forms::callout(ui, CalloutKind::Error, |ui| {
                    forms::paragraph(ui, e, 12.5, 19.0, p.ink);
                }),
            }
        }
    });
    action
}

/// What each target got (or would get), and the run's problems.
fn outcomes(ui: &mut Ui, done: &PublishDone, root: &Path, p: &Palette) {
    let kind = if done.ok {
        CalloutKind::Ok
    } else {
        CalloutKind::Error
    };
    forms::callout(ui, kind, |ui| {
        for o in &done.outcomes {
            let word = match (o.ok, o.skipped, done.dry_run) {
                (true, true, _) => "skipped",
                (true, false, true) => "would upload",
                (true, false, false) => "uploaded",
                (false, _, _) => "failed",
            };
            forms::paragraph(
                ui,
                &format!("{} · {word} {} · {}", o.target, o.file, o.message),
                12.5,
                19.0,
                p.ink,
            );
        }
        for d in done.diagnostics.iter().filter(|d| d.severity == "error") {
            forms::paragraph(ui, &diagnostic_line(d, root), 12.5, 19.0, p.ink);
        }
        for n in &done.notes {
            caption(ui, n, p);
        }
        if done.dry_run && done.ok {
            caption(ui, "nothing uploaded", p);
        }
    });
}

/// The confirms (S061): a publish against its fresh dry run, the tag and
/// push with its commands, an install over changed folders.
pub fn dialogs(
    ctx: &egui::Context,
    state: &mut ShipState,
    workspace: &Workspace,
    system: &dyn System,
) {
    let Some(dialog) = state.dialog.clone() else {
        return;
    };
    let p = palette(ctx);
    let mut open = true;
    let mut close = false;
    let facts = state.facts().cloned().unwrap_or_default();
    match dialog {
        Dialog::Publish { job } => {
            let title = format!("publish {} {}", facts.addon, facts.version);
            egui::Window::new(title)
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .default_width(560.0)
                .show(ctx, |ui| {
                    let running = state.publish_job == Some(job);
                    let dry = state.dry.clone().filter(|_| !running);
                    match &dry {
                        None => caption(ui, "a dry run first: packaging, nothing uploaded…", p),
                        Some(done) => {
                            caption(ui, "a dry run just now · nothing uploaded yet", p);
                            outcomes(ui, done, &workspace.root, p);
                            caption(
                                ui,
                                &format!(
                                    "The notes are the changelog's {} section. The tests run again before packaging. A site keeps a file once it is uploaded: Studio can't take it back.",
                                    facts.version
                                ),
                                p,
                            );
                        }
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if widgets::button(ui, ButtonKind::Secondary, "cancel", false, None).clicked() {
                            close = true;
                        }
                        let n = dry.as_ref().map_or(0, |d| d.outcomes.iter().filter(|o| o.ok && !o.skipped).count());
                        let ready = dry.as_ref().is_some_and(|d| d.ok) && n > 0;
                        ui.add_enabled_ui(ready, |ui| {
                            let label = format!("upload to {n} site{}", if n == 1 { "" } else { "s" });
                            if widgets::button(ui, ButtonKind::Primary, &label, false, None).clicked() {
                                state.publish(workspace, system, false);
                                close = true;
                            }
                        });
                    });
                });
        }
        Dialog::Tag(tag) => {
            let title = format!("tag {} and push", tag.tag);
            egui::Window::new(title)
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .default_width(560.0)
                .show(ctx, |ui| {
                    caption(ui, "Studio runs these in the repository:", p);
                    let commands: Vec<String> = tag
                        .commands()
                        .iter()
                        .map(|c| {
                            c.iter()
                                .map(|a| if a.contains(' ') { format!("\"{a}\"") } else { a.clone() })
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                        .collect();
                    let term: Vec<TermLine<'_>> = commands.iter().map(|c| TermLine::Command(c)).collect();
                    forms::terminal(ui, None, &term);
                    caption(
                        ui,
                        "The pushed tag starts the repository's CI, which runs wowlua publish for it. A pushed tag stays: to take it back, delete it on the remote and locally.",
                        p,
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if widgets::button(ui, ButtonKind::Secondary, "cancel", false, None).clicked() {
                            close = true;
                        }
                        if widgets::button(ui, ButtonKind::Primary, "tag and push", false, None).clicked() {
                            state.tagging = true;
                            state.tagged = None;
                            system.ship_tag(&state.dir(workspace), tag.clone());
                            close = true;
                        }
                    });
                });
        }
        Dialog::Install { profile, force } => {
            egui::Window::new("install over the AddOn folder")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .default_width(520.0)
                .show(ctx, |ui| {
                    for folder in &facts.installs {
                        if let Some(c) = changed_files(folder) {
                            forms::paragraph(
                                ui,
                                &format!("{}: {} file{} changed outside Studio, which the install replaces.", folder.client, c.len(), if c.len() == 1 { "" } else { "s" }),
                                12.5,
                                19.0,
                                p.ink,
                            );
                        }
                        if folder.state == InstallState::Foreign {
                            forms::paragraph(
                                ui,
                                &format!("{}: the folder was not installed by wowlua (an addon manager's?): installing replaces it.", folder.client),
                                12.5,
                                19.0,
                                p.ink,
                            );
                        }
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if widgets::button(ui, ButtonKind::Secondary, "cancel", false, None).clicked() {
                            close = true;
                        }
                        if widgets::button(ui, ButtonKind::Primary, "install anyway", false, None).clicked() {
                            state.build(workspace, system, &profile, Some(ShipInstall { link: false, force }));
                            close = true;
                        }
                    });
                });
        }
    }
    if close || !open {
        state.dialog = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value as Json;

    fn facts() -> ShipFacts {
        ShipFacts {
            addon: "SuuUI".into(),
            version: "0.6.0-beta.2".into(),
            tag: "v0.6.0-beta.2".into(),
            clean: true,
            branch: Some("main".into()),
            changelog: Some((PathBuf::from("CHANGELOG.md"), "### New\n- a".into())),
            ..ShipFacts::default()
        }
    }

    #[test]
    fn the_checks_follow_the_repository() {
        let ok = checks(&facts());
        assert!(ok.iter().all(|(o, _, _)| *o != Some(false)), "{ok:?}");
        let mut f = facts();
        f.clean = false;
        f.tag_at_head = Some(false);
        f.changelog = Some((PathBuf::from("CHANGELOG.md"), String::new()));
        let bad = checks(&f);
        let failed: Vec<&str> = bad
            .iter()
            .filter(|(o, _, _)| *o == Some(false))
            .map(|(_, t, _)| t.as_str())
            .collect();
        assert_eq!(failed.len(), 3, "{failed:?}");
        assert!(failed[1].contains("already released"));
        assert!(failed[2].contains("has no section for 0.6.0-beta.2"));
    }

    #[test]
    fn progress_moves_forward() {
        let steps = [
            BuildStep::Read { files: 3 },
            BuildStep::Parsed { done: 1, of: 3 },
            BuildStep::Parsed { done: 3, of: 3 },
            BuildStep::Linted { done: 2, of: 2 },
            BuildStep::Checking {
                flavor: None,
                index: 0,
                count: 1,
            },
            BuildStep::Bundling,
            BuildStep::Writing,
            BuildStep::Installing,
        ];
        let f: Vec<f32> = steps.iter().map(|s| fraction(Some(s))).collect();
        assert!(f.windows(2).all(|w| w[0] <= w[1] + 1e-6), "{f:?}");
        assert!(f.iter().all(|x| (0.0..=1.0).contains(x)));
    }

    #[test]
    fn an_addon_folder_s_top() {
        let files = vec![
            ("Media/a.tga".to_owned(), 1_000),
            ("Media/b.tga".to_owned(), 2_000),
            ("SuuUI.lua".to_owned(), 1_840_000),
            ("SuuUI.toc".to_owned(), 300),
        ];
        assert_eq!(
            tree(&files),
            vec![
                ("SuuUI.lua".to_owned(), "1.84 MB".to_owned()),
                ("SuuUI.toc".to_owned(), "300 B".to_owned()),
                ("Media/".to_owned(), "2 files · 3 KB".to_owned()),
            ]
        );
    }

    #[test]
    fn a_profile_s_summary() {
        let profile = |chain: &[&str], effective: Json| BuildProfile {
            name: chain.last().unwrap().to_string(),
            builtin: false,
            chain: chain.iter().map(|s| s.to_string()).collect(),
            effective,
            problem: None,
        };
        let release = profile(
            &["release"],
            serde_json::json!({ "bundle": true, "minify": "max", "optimize": { "inline": true } }),
        );
        assert_eq!(summary(&release), "bundle · minify max · optimized");
        let readable = profile(
            &["release", "readable"],
            serde_json::json!({ "bundle": true, "minify": "none", "optimize": { "inline": false } }),
        );
        assert_eq!(summary(&readable), "inherits release · bundle");
        let dev = profile(
            &["dev"],
            serde_json::json!({ "bundle": false, "minify": "none" }),
        );
        assert_eq!(summary(&dev), "files as written");
    }

    #[test]
    fn times_ago() {
        assert_eq!(ago_text(3), "just now");
        assert_eq!(ago_text(42), "42 s ago");
        assert_eq!(ago_text(130), "2 min ago");
        assert_eq!(ago_text(7_300), "2 h ago");
        assert_eq!(ago_text(90_000), "a day ago");
        assert_eq!(ago_text(400_000), "4 days ago");
    }

    #[test]
    fn diagnostics_read_as_compiler_output() {
        let d = ShipDiagnostic {
            severity: "error".into(),
            code: Some("E2003".into()),
            message: "`ThreatEntry` has no field `roel`".into(),
            path: Some(PathBuf::from("/w/src/UI/ThreatMeter.wowlua")),
            line: Some(68),
            column: Some(20),
        };
        assert_eq!(
            diagnostic_line(&d, Path::new("/w")),
            "error[E2003]: `ThreatEntry` has no field `roel` (src/UI/ThreatMeter.wowlua:68:20)"
        );
    }
}
