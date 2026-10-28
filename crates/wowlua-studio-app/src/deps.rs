//! The dependencies view of the settings page (M10, S010, S062): a
//! package's dependencies and dev-dependencies with their source,
//! requirement, locked and newest version; newer ones looked up when the
//! view opens (`wowlua outdated`); the registry's search to add from; a
//! dependency's details, its versions, update, a new requirement, remove.
//! Everything runs in Studio's process through wowlua's library.

use std::collections::HashMap;
use std::path::PathBuf;

use egui::{CornerRadius, Sense, Ui, Vec2};
use wowlua_studio_ui::forms::{self, CalloutKind, TermLine};
use wowlua_studio_ui::widgets::{self, ButtonKind};
use wowlua_studio_ui::{Face, Palette, palette};

use crate::system::{
    DepFound, DepNewer, DepRow, DepsCommand, DepsEvent, DepsFacts, System, Workspace,
};

#[derive(Default)]
pub struct DepsState {
    /// The package's folder (none: the workspace's root package).
    pub picked: Option<PathBuf>,
    facts: Option<Result<DepsFacts, String>>,
    reading: bool,
    read_at: Option<f64>,
    stale: bool,
    newer: Option<Result<Vec<DepNewer>, String>>,
    checking: bool,
    query: String,
    searched: Option<(String, Result<Vec<DepFound>, String>)>,
    searching: bool,
    /// The dependency the details show: its name and whether it is a
    /// dev-dependency.
    selected: Option<(String, bool)>,
    versions: HashMap<String, Vec<(String, bool)>>,
    next_job: u64,
    running: Option<(u64, String)>,
    /// The last command, whether it worked, its lines.
    last: Option<(String, bool, Vec<String>)>,
    requirement: Option<String>,
}

impl DepsState {
    fn dir(&self, workspace: &Workspace) -> PathBuf {
        self.picked
            .clone()
            .unwrap_or_else(|| workspace.root.clone())
    }

    pub fn files_changed(&mut self) {
        self.stale = true;
    }

    pub fn event(&mut self, event: DepsEvent) {
        match event {
            DepsEvent::Read(read) => {
                self.reading = false;
                self.facts = Some(*read);
            }
            DepsEvent::Outdated(newer) => {
                self.checking = false;
                self.newer = Some(newer);
            }
            DepsEvent::Searched { query, found } => {
                self.searching = false;
                self.searched = Some((query, found));
            }
            DepsEvent::Versions { name, versions } => {
                self.versions.insert(name, versions);
            }
            DepsEvent::Ran {
                job,
                command,
                ok,
                output,
            } => {
                if self.running.as_ref().is_some_and(|(j, _)| *j == job) {
                    self.running = None;
                    self.last = Some((command, ok, output));
                    self.stale = true;
                    // (what is newer changed with the lockfile)
                    self.newer = None;
                }
            }
        }
    }

    /// Reads the package when it changed, and what is newer once.
    fn refresh(&mut self, workspace: &Workspace, system: &dyn System, now: f64) {
        let dir = self.dir(workspace);
        let due =
            self.facts.is_none() || (self.stale && self.read_at.is_none_or(|t| now - t > 1.0));
        if due && !self.reading {
            self.reading = true;
            self.stale = false;
            self.read_at = Some(now);
            system.deps_read(&dir);
        }
        if self.newer.is_none() && !self.checking {
            self.checking = true;
            system.deps_outdated(&dir);
        }
    }

    /// The view was opened: newer versions are looked up again.
    pub fn opened(&mut self) {
        if !self.checking {
            self.newer = None;
        }
    }

    fn run(&mut self, workspace: &Workspace, system: &dyn System, command: DepsCommand) {
        self.next_job += 1;
        let job = self.next_job;
        self.running = Some((job, command.line()));
        system.deps_run(&self.dir(workspace), job, command);
    }
}

fn caption(ui: &mut Ui, text: &str, p: &Palette) {
    ui.label(
        egui::RichText::new(text)
            .font(Face::Plex.at(11.5))
            .color(p.ink_muted),
    );
}

fn text(ui: &mut Ui, text: &str, color: egui::Color32) {
    ui.label(
        egui::RichText::new(text)
            .font(Face::Plex.at(12.5))
            .color(color),
    );
}

/// What the state column says, and its color.
pub fn state_of(row: &DepRow, newer: Option<&DepNewer>, p: &Palette) -> (String, egui::Color32) {
    if row.locked.is_empty() {
        return ("not resolved".into(), p.error);
    }
    if row.yanked {
        return ("W4322 yanked".into(), p.warning);
    }
    if row.source.starts_with("path") {
        return (
            if row.dev { "tests only" } else { "workspace" }.into(),
            p.ink_muted,
        );
    }
    match newer {
        Some(n) if n.action.contains("update") => (format!("update: {}", n.latest), p.amber_ink),
        Some(n) => (format!("newer: {}", n.latest), p.amber_ink),
        None if row.requirement.starts_with("tag ") => ("pinned".into(), p.ink_muted),
        None => (
            if row.dev { "tests only" } else { "up to date" }.into(),
            if row.dev { p.ink_muted } else { p.success },
        ),
    }
}

/// The package select: the workspace's packages, the root first.
fn package_select(ui: &mut Ui, state: &mut DepsState, workspace: &Workspace) {
    let mut members: Vec<(String, PathBuf)> = workspace
        .members
        .iter()
        .map(|m| (m.name.clone(), m.dir.clone()))
        .collect();
    members.sort_by_key(|(_, dir)| *dir != workspace.root);
    if !members.iter().any(|(_, d)| *d == workspace.root) {
        members.insert(0, (workspace.name.clone(), workspace.root.clone()));
    }
    let current = state.dir(workspace);
    let mut index = members.iter().position(|(_, d)| *d == current).unwrap_or(0);
    let labels: Vec<String> = members
        .iter()
        .map(|(n, d)| {
            if *d == workspace.root {
                format!("{n} · root")
            } else {
                n.clone()
            }
        })
        .collect();
    let opts: Vec<(&str, bool)> = labels.iter().map(|l| (l.as_str(), true)).collect();
    if forms::select(ui, "deps-package", &opts, &mut index, 180.0) {
        let dir = members[index].1.clone();
        state.picked = (dir != workspace.root).then_some(dir);
        state.facts = None;
        state.newer = None;
        state.selected = None;
        state.last = None;
    }
}

pub fn show(ui: &mut Ui, state: &mut DepsState, workspace: &Workspace, system: &dyn System) {
    let now = ui.input(|i| i.time);
    state.refresh(workspace, system, now);
    let p = palette(ui.ctx());
    // The bar.
    forms::padded(ui, 16, 8, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("dependencies")
                    .font(Face::PlexMedium.at(12.5))
                    .color(p.ink),
            );
            package_select(ui, state, workspace);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let busy = state.running.is_some();
                ui.add_enabled_ui(!busy, |ui| {
                    if widgets::button(ui, ButtonKind::Secondary, "update all", true, None)
                        .clicked()
                    {
                        state.run(workspace, system, DepsCommand::Update(Vec::new()));
                    }
                    if widgets::button(ui, ButtonKind::Secondary, "check again", true, None)
                        .clicked()
                        && !state.checking
                    {
                        state.newer = None;
                    }
                });
                let response =
                    forms::text_input(ui, &mut state.query, 240.0, "/ search the registry");
                let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if enter && !state.query.trim().is_empty() {
                    state.searching = true;
                    system.deps_search(&state.dir(workspace), state.query.trim());
                }
                if state.checking {
                    caption(ui, "checking for newer versions…", &p);
                }
            });
        });
    });
    let facts = match &state.facts {
        Some(Ok(f)) => f.clone(),
        Some(Err(why)) => {
            forms::padded(ui, 16, 8, |ui| {
                forms::callout(ui, CalloutKind::Error, |ui| {
                    forms::paragraph(ui, why, 12.5, 19.0, p.ink)
                });
            });
            return;
        }
        None => {
            forms::padded(ui, 16, 8, |ui| caption(ui, "reading the dependencies…", &p));
            return;
        }
    };
    if let Some(Err(why)) = &state.newer {
        forms::callout(ui, CalloutKind::Warning, |ui| {
            forms::paragraph(
                ui,
                &format!(
                    "warning: newer versions can't be looked up ({why}). Builds use the locked versions from the cache; searching, adding and updates wait for the network."
                ),
                12.5,
                19.0,
                p.ink,
            );
        });
    }
    let yanked: Vec<&DepRow> = facts.rows.iter().filter(|r| r.yanked).collect();
    for row in &yanked {
        forms::callout(ui, CalloutKind::Warning, |ui| {
            ui.horizontal_wrapped(|ui| {
                forms::paragraph(
                    ui,
                    &format!("warning[W4322]: {} {} in wowlua.lock was yanked by its author. It keeps working; a new resolution never picks it.", row.name, row.locked),
                    12.5,
                    19.0,
                    p.ink,
                );
                if widgets::button(ui, ButtonKind::Primary, &format!("update {}", row.name), true, None).clicked() {
                    state.run(workspace, system, DepsCommand::Update(vec![row.name.clone()]));
                }
            });
        });
    }
    for problem in facts.problems.iter().filter(|p| !p.contains("W4322")) {
        caption(ui, problem, &p);
    }
    let newer: Vec<DepNewer> = match &state.newer {
        Some(Ok(n)) => n.clone(),
        _ => Vec::new(),
    };
    if state.selected.is_none() {
        state.selected = facts.rows.first().map(|r| (r.name.clone(), r.dev));
    }
    let available = ui.available_rect_before_wrap();
    let detail_width = 340.0_f32.min(available.width() * 0.4);
    let list_rect = egui::Rect::from_min_max(
        available.min,
        egui::pos2(available.max.x - detail_width, available.max.y),
    );
    let detail_rect =
        egui::Rect::from_min_max(egui::pos2(list_rect.max.x, available.min.y), available.max);
    ui.painter().line_segment(
        [detail_rect.left_top(), detail_rect.left_bottom()],
        egui::Stroke::new(1.0, p.line),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(list_rect), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("deps-list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                list(ui, state, &facts, &newer, workspace, system, &p)
            });
    });
    ui.scope_builder(egui::UiBuilder::new().max_rect(detail_rect), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("deps-detail")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                forms::padded(ui, 14, 12, |ui| {
                    details(ui, state, &facts, &newer, workspace, system, &p)
                });
            });
    });
    ui.allocate_rect(available, Sense::hover());
}

fn list(
    ui: &mut Ui,
    state: &mut DepsState,
    facts: &DepsFacts,
    newer: &[DepNewer],
    workspace: &Workspace,
    system: &dyn System,
    p: &Palette,
) {
    // The registry's results.
    if let Some((query, found)) = state.searched.clone() {
        forms::padded(ui, 12, 8, |ui| {
            ui.horizontal(|ui| {
                caption(ui, &format!("registry.wowlua.dev · \"{query}\""), p);
                if forms::text_button(ui, "close").clicked() {
                    state.searched = None;
                }
            });
            match &found {
                Err(why) => caption(ui, why, p),
                Ok(list) if list.is_empty() => caption(ui, "Nothing found.", p),
                Ok(list) => {
                    egui::Grid::new("deps-found")
                        .num_columns(4)
                        .spacing([16.0, 6.0])
                        .show(ui, |ui| {
                            for f in list {
                                text(ui, &f.name, p.ink);
                                caption(ui, &f.newest, p);
                                caption(ui, &f.description, p);
                                if facts.rows.iter().any(|r| r.name == f.name) {
                                    text(ui, "✓ added", p.success);
                                } else {
                                    let requirement = caret(&f.newest);
                                    let label = format!("add {requirement}");
                                    if widgets::button(
                                        ui,
                                        ButtonKind::Secondary,
                                        &label,
                                        true,
                                        None,
                                    )
                                    .clicked()
                                    {
                                        state.run(
                                            workspace,
                                            system,
                                            DepsCommand::Add {
                                                name: f.name.clone(),
                                                requirement: Some(requirement),
                                                dev: false,
                                            },
                                        );
                                    }
                                }
                                ui.end_row();
                            }
                        });
                    caption(
                        ui,
                        "A requirement as Cargo writes them: ^1.2 takes any 1.x from 1.2.0 on, ~1.2.3 any 1.2.x from 1.2.3, =1.2.3 exactly that one. Change it in the details after adding.",
                        p,
                    );
                }
            }
        });
    }
    if state.searching {
        forms::padded(ui, 12, 8, |ui| caption(ui, "searching the registry…", p));
    }
    if facts.rows.is_empty() {
        forms::padded(ui, 12, 12, |ui| {
            text(
                ui,
                &format!("{} has no dependencies yet.", facts.package),
                p.ink,
            );
            caption(
                ui,
                "Search the registry above, or add a library from git, SVN, an archive or a folder in wowlua.json. wowlua fetches, locks and embeds them: no Libs/ folder to copy by hand.",
                p,
            );
        });
        return;
    }
    for (dev, title, note) in [
        (false, "dependencies", "embedded into the build"),
        (true, "dev-dependencies", "checks and tests only"),
    ] {
        let rows: Vec<&DepRow> = facts.rows.iter().filter(|r| r.dev == dev).collect();
        if rows.is_empty() {
            continue;
        }
        forms::padded(ui, 12, 6, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(title)
                        .font(Face::PlexMedium.at(11.0))
                        .color(p.ink_muted),
                );
                caption(ui, note, p);
            });
        });
        for row in rows {
            let n = newer.iter().find(|n| n.name == row.name);
            let selected = state
                .selected
                .as_ref()
                .is_some_and(|(s, d)| *s == row.name && *d == row.dev);
            if dep_row(ui, row, n, selected, p) {
                state.selected = Some((row.name.clone(), row.dev));
                state.requirement = None;
            }
        }
    }
    forms::padded(ui, 12, 10, |ui| {
        caption(
            ui,
            "Dependencies are written to wowlua.json, keeping its formatting, and locked with their checksums in wowlua.lock; commit both. Fetched code lives in the shared cache, ~/.wowlua/cache.",
            p,
        );
    });
}

/// `^1.2` for 1.2.3, `^0.3` for 0.3.1: the version and compatible ones.
pub fn caret(version: &str) -> String {
    let parts: Vec<&str> = version.split(['.', '-']).collect();
    match parts.as_slice() {
        [major, minor, ..] => format!("^{major}.{minor}"),
        _ => format!("^{version}"),
    }
}

/// A dependency's row: name, source, requirement, locked, newest, state.
fn dep_row(
    ui: &mut Ui,
    row: &DepRow,
    newer: Option<&DepNewer>,
    selected: bool,
    p: &Palette,
) -> bool {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 28.0), Sense::click());
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, CornerRadius::ZERO, p.bg_400);
        let bar = egui::Rect::from_min_size(rect.min, Vec2::new(2.0, rect.height()));
        painter.rect_filled(bar, CornerRadius::ZERO, p.amber);
    } else if response.hovered() {
        painter.rect_filled(rect, CornerRadius::ZERO, p.bg_300);
    }
    let (state, color) = state_of(row, newer, p);
    let newest = newer.map_or("—".to_owned(), |n| n.latest.clone());
    let columns: [(f32, &str, egui::Color32); 6] = [
        (0.0, &row.name, p.ink),
        (0.2, &row.source, p.ink_muted),
        (0.42, &row.requirement, p.ink),
        (0.58, &row.locked, p.ink),
        (0.72, &newest, p.ink_muted),
        (0.84, &state, color),
    ];
    for (at, label, color) in columns {
        painter.text(
            egui::pos2(rect.left() + 12.0 + (width - 24.0) * at, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            Face::Plex.at(12.0),
            color,
        );
    }
    response.clicked()
}

fn details(
    ui: &mut Ui,
    state: &mut DepsState,
    facts: &DepsFacts,
    newer: &[DepNewer],
    workspace: &Workspace,
    system: &dyn System,
    p: &Palette,
) {
    let row = state
        .selected
        .as_ref()
        .and_then(|(n, d)| facts.rows.iter().find(|r| &r.name == n && r.dev == *d))
        .cloned();
    let Some(row) = row else {
        caption(ui, "Pick a dependency, or add one from the registry.", p);
        return;
    };
    ui.label(
        egui::RichText::new(&row.name)
            .font(Face::PlexMedium.at(13.0))
            .color(p.ink),
    );
    if !row.description.is_empty() {
        text(ui, &row.description, p.ink);
    }
    let n = newer.iter().find(|n| n.name == row.name);
    forms::key_values(
        ui,
        &[
            ("source", row.source.clone()),
            ("kind", row.kind.clone()),
            ("requirement", row.requirement.clone()),
            (
                "locked",
                format!(
                    "{}{}",
                    row.locked,
                    if row.yanked { " · yanked" } else { "" }
                ),
            ),
            ("newest", n.map_or("—".into(), |n| n.latest.clone())),
        ],
    );
    if row.registry {
        if !state.versions.contains_key(&row.name) {
            state.versions.insert(row.name.clone(), Vec::new());
            system.deps_versions(&state.dir(workspace), &row.name);
        }
        let versions = state.versions.get(&row.name).cloned().unwrap_or_default();
        if !versions.is_empty() {
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new("versions")
                    .font(Face::PlexMedium.at(11.0))
                    .color(p.ink_muted),
            );
            for (v, yanked) in versions.iter().take(8) {
                ui.horizontal(|ui| {
                    text(ui, v, p.ink);
                    let locked = row.locked == *v;
                    let note = match (locked, yanked) {
                        (true, true) => "yanked: locked here",
                        (true, false) => "locked",
                        (false, true) => "yanked",
                        (false, false) => "",
                    };
                    if !note.is_empty() {
                        caption(ui, note, p);
                    }
                });
            }
        }
    }
    if let Some(n) = n {
        ui.add_space(6.0);
        caption(ui, &format!("Newer: {} · {}", n.latest, n.action), p);
    }
    ui.add_space(8.0);
    let busy = state.running.is_some();
    ui.add_enabled_ui(!busy, |ui| {
        ui.horizontal_wrapped(|ui| {
            if n.is_some_and(|n| n.action.contains("update"))
                && widgets::button(
                    ui,
                    ButtonKind::Primary,
                    &format!("update {}", row.name),
                    true,
                    None,
                )
                .clicked()
            {
                state.run(
                    workspace,
                    system,
                    DepsCommand::Update(vec![row.name.clone()]),
                );
            }
            if row.registry
                && widgets::button(ui, ButtonKind::Ghost, "change requirement", true, None)
                    .clicked()
            {
                state.requirement = Some(row.requirement.clone());
            }
            if widgets::button(ui, ButtonKind::Ghost, "remove", true, None).clicked() {
                state.run(
                    workspace,
                    system,
                    DepsCommand::Remove {
                        name: row.name.clone(),
                        dev: row.dev,
                    },
                );
                state.selected = None;
            }
        });
    });
    if let Some(requirement) = &mut state.requirement {
        ui.add_space(6.0);
        forms::label(ui, "requirement");
        forms::text_input(ui, requirement, 160.0, "^1.2");
        let new = requirement.trim().to_owned();
        ui.horizontal(|ui| {
            if widgets::button(ui, ButtonKind::Secondary, "change", true, None).clicked()
                && !new.is_empty()
            {
                state.run(
                    workspace,
                    system,
                    DepsCommand::Add {
                        name: row.name.clone(),
                        requirement: Some(new.clone()),
                        dev: row.dev,
                    },
                );
                state.requirement = None;
            }
            if forms::text_button(ui, "cancel").clicked() {
                state.requirement = None;
            }
        });
    }
    if !row.registry && !row.source.starts_with("path") {
        caption(
            ui,
            "Its tag, branch or revision is changed in wowlua.json.",
            p,
        );
    }
    // The last command.
    ui.add_space(10.0);
    let (command, lines): (String, Vec<String>) = match (&state.running, &state.last) {
        (Some((_, c)), _) => (c.clone(), vec!["    running…".into()]),
        (None, Some((c, _, lines))) => (c.clone(), lines.clone()),
        (None, None) => (
            "wowlua outdated".into(),
            vec!["    newer versions, tags, commits or revisions, and what to change".into()],
        ),
    };
    let mut term: Vec<TermLine<'_>> = vec![TermLine::Command(&command)];
    term.extend(lines.iter().map(|l| TermLine::Out(l)));
    forms::terminal(ui, None, &term);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requirements_for_a_version() {
        assert_eq!(caret("1.2.3"), "^1.2");
        assert_eq!(caret("0.3.1"), "^0.3");
        assert_eq!(caret("2.0.0-beta.1"), "^2.0");
    }

    #[test]
    fn a_row_s_state() {
        let p = wowlua_studio_ui::Theme::Dark.palette();
        let row = DepRow {
            name: "lib-tooltip".into(),
            dev: false,
            source: "registry".into(),
            registry: true,
            kind: "library".into(),
            requirement: "^1.2".into(),
            locked: "1.2.1".into(),
            yanked: false,
            description: String::new(),
        };
        assert_eq!(state_of(&row, None, &p).0, "up to date");
        let newer = DepNewer {
            name: "lib-tooltip".into(),
            latest: "1.2.3".into(),
            action: "`wowlua update`".into(),
        };
        assert_eq!(state_of(&row, Some(&newer), &p).0, "update: 1.2.3");
        let yanked = DepRow {
            yanked: true,
            ..row.clone()
        };
        assert_eq!(state_of(&yanked, None, &p).0, "W4322 yanked");
        let unresolved = DepRow {
            locked: String::new(),
            ..row
        };
        assert_eq!(state_of(&unresolved, None, &p).0, "not resolved");
    }
}
