//! The git page (M11, the canvas's git board): the changes and their
//! commit, the history, and the main view: a file's hunks, staged one by
//! one, or a commit's files and diff.

use egui::text::LayoutJob;
use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, Ui, Vec2};
use wowlua_studio_ui::forms::{self, CalloutKind};
use wowlua_studio_ui::widgets::{self, ButtonKind};
use wowlua_studio_ui::{Face, Icon, Palette, palette};

use super::model::{Apply, Carry, Change, DiffOf, Hunk, LineKind, RemoteError, Side};
use super::{Callout, Dialog, GitState, Selected};
use crate::system::{CommitStop, GitRequest, System};

/// What the page asks of the rest of Studio.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// A file of the repository in the editor, at a line (1-based).
    Open(std::path::PathBuf, Option<u32>),
    /// The settings' GitHub sign-in.
    SignIn,
}

const ROW_H: f32 = 24.0;

/// How long ago, shortly: `now`, `40 min`, `2 h`, `3 d`, `5 w`.
pub fn age(time: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let s = (now - time).max(0);
    match s {
        0..60 => "now".into(),
        60..3600 => format!("{} min", s / 60),
        3600..86_400 => format!("{} h", s / 3600),
        86_400..1_209_600 => format!("{} d", s / 86_400),
        _ => format!("{} w", s / 604_800),
    }
}

/// The colour of a change's letter.
fn change_color(change: Change, p: &Palette) -> Color32 {
    match change {
        Change::Modified | Change::Renamed => p.warning,
        Change::Added | Change::Untracked => p.success,
        Change::Deleted | Change::Conflicted => p.error,
    }
}

fn caption(ui: &mut Ui, text: &str) {
    let p = palette(ui.ctx());
    ui.label(
        egui::RichText::new(text)
            .font(Face::Plex.at(11.5))
            .color(p.ink_muted),
    );
}

/// A section's head: its name and count.
fn section(ui: &mut Ui, name: &str, count: usize, color: Color32) {
    let p = palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
    ui.painter().text(
        egui::pos2(rect.left() + 12.0, rect.bottom() - 6.0),
        Align2::LEFT_BOTTOM,
        name,
        Face::Plex.at(11.5),
        color,
    );
    ui.painter().text(
        egui::pos2(rect.right() - 12.0, rect.bottom() - 6.0),
        Align2::RIGHT_BOTTOM,
        count.to_string(),
        Face::Plex.at(11.5),
        p.ink_muted,
    );
}

/// A file's row: its name and folder, a note, the stage or unstage button,
/// its letter. (clicked, button clicked)
fn file_row(
    ui: &mut Ui,
    path: &str,
    note: &str,
    button: Option<(Icon, &str)>,
    change: Change,
    selected: bool,
) -> (bool, bool) {
    let p = palette(ui.ctx());
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::click());
    let hovered = response.hovered();
    if selected {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_400);
    } else if hovered {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_300);
    }
    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
    let font = Face::Plex.at(12.5);
    let name_w = ui
        .painter()
        .text(
            egui::pos2(rect.left() + 12.0, rect.center().y),
            Align2::LEFT_CENTER,
            name,
            font,
            if change == Change::Deleted {
                p.ink_muted
            } else {
                p.ink
            },
        )
        .width();
    ui.painter().text(
        egui::pos2(rect.left() + 20.0 + name_w, rect.center().y),
        Align2::LEFT_CENTER,
        dir,
        Face::Plex.at(11.5),
        p.ink_muted,
    );
    // The letter, the button left of it, the note left of that.
    let letter = egui::pos2(rect.right() - 16.0, rect.center().y);
    ui.painter().text(
        letter,
        Align2::CENTER_CENTER,
        change.letter(),
        Face::PlexSemiBold.at(11.0),
        change_color(change, p),
    );
    let mut clicked_button = false;
    let mut right = rect.right() - 28.0;
    if let Some((icon, label)) = button {
        let b =
            Rect::from_center_size(egui::pos2(right - 10.0, rect.center().y), Vec2::splat(20.0));
        let r = ui.interact(
            b,
            egui::Id::unique(("wowlua-studio-git-row", path, label)),
            Sense::click(),
        );
        if hovered || r.hovered() {
            if r.hovered() {
                ui.painter().rect_filled(b, CornerRadius::ZERO, p.bg_400);
            }
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(b.shrink(3.0)));
            widgets::icon(
                &mut child,
                icon,
                14.0,
                if r.hovered() { p.ink } else { p.ink_muted },
            );
        }
        clicked_button = r.on_hover_text(label).clicked();
        right -= 24.0;
    }
    if !note.is_empty() {
        ui.painter().text(
            egui::pos2(right - 4.0, rect.center().y),
            Align2::RIGHT_CENTER,
            note,
            Face::Plex.at(11.0),
            p.ink_muted,
        );
    }
    (response.clicked() && !clicked_button, clicked_button)
}

// ------------------------------------------------------------ changes --

/// The changes panel: the branch, the commit's message and buttons, the
/// conflicts, the staged files and the others.
pub fn changes(ui: &mut Ui, state: &mut GitState, system: &dyn System) -> Option<Action> {
    let p = palette(ui.ctx());
    let Some(status) = state.status.clone() else {
        forms::padded(ui, 12, 12, |ui| {
            let text = match &state.error {
                Some(e) if e.contains("not a git repository") => {
                    "This workspace is no Git repository: run git init in the terminal, or start from the start page's new addon.".to_owned()
                }
                Some(e) => e.clone(),
                None => "Reading the repository…".to_owned(),
            };
            forms::paragraph(ui, &text, 12.0, 18.0, p.ink_muted);
        });
        return None;
    };
    let action = None;
    let merging = status.merging.is_some();
    let conflicts: Vec<&super::model::File> = status.conflicts().collect();
    // The branch and pulling and pushing.
    forms::padded(ui, 12, 8, |ui| {
        ui.horizontal(|ui| {
            let mut names: Vec<String> = Vec::new();
            let current = status.branch_label();
            names.push(current.clone());
            for b in state.branches.iter().filter(|b| !b.remote && !b.current) {
                let ab = match (b.ahead, b.behind) {
                    (0, 0) => String::new(),
                    (a, 0) => format!(" · ↑{a}"),
                    (0, d) => format!(" · ↓{d}"),
                    (a, d) => format!(" · ↑{a} ↓{d}"),
                };
                names.push(format!("{}{ab}", b.name));
            }
            names.push("+ new branch from here…".into());
            let mut index = 0;
            let opts: Vec<(&str, bool)> = names.iter().map(|n| (n.as_str(), !merging)).collect();
            let w = ui.available_width() - 34.0;
            if forms::select(ui, "git-branch", &opts, &mut index, w) && index > 0 {
                if index == names.len() - 1 {
                    state.dialog = Some(Dialog::NewBranch {
                        from: None,
                        name: String::new(),
                        switch: true,
                        bad: None,
                    });
                } else {
                    let target = names[index].split(" · ").next().unwrap_or("").to_owned();
                    start_switch(state, system, target);
                }
            }
            let sync = widgets::icon_button(ui, Icon::Sync, "pull, then push", false)
                .on_hover_text("pull (as your git config says), then push");
            if sync.clicked() && state.busy.is_none() {
                state.then_push = true;
                state.change(system, "pulling…", GitRequest::Pull);
            }
        });
    });
    // The message and the commit.
    let staged: Vec<&super::model::File> = status
        .files
        .iter()
        .filter(|f| f.staged.is_some() && f.staged != Some(Change::Conflicted))
        .collect();
    let unstaged: Vec<&super::model::File> = status
        .files
        .iter()
        .filter(|f| f.unstaged.is_some() && f.unstaged != Some(Change::Conflicted))
        .collect();
    forms::padded(ui, 12, 6, |ui| {
        let hint = match &status.merging {
            Some(m) => format!(
                "Merge {m} into {}",
                status.branch.as_deref().unwrap_or("HEAD")
            ),
            None => "a commit message".to_owned(),
        };
        ui.add(
            egui::TextEdit::multiline(&mut state.message)
                .desired_rows(3)
                .desired_width(ui.available_width())
                .font(Face::Plex.at(12.5))
                .hint_text(hint.clone()),
        );
        ui.add_space(4.0);
        let message = if state.message.trim().is_empty() && merging {
            hint.clone()
        } else {
            state.message.trim().to_owned()
        };
        let can = state.busy.is_none()
            && !message.is_empty()
            && conflicts.is_empty()
            && (!staged.is_empty() || merging);
        ui.horizontal(|ui| {
            let label = if merging {
                "commit the merge".to_owned()
            } else {
                format!("commit {}", staged.len())
            };
            let commit = ui.add_enabled_ui(can, |ui| {
                widgets::button(ui, ButtonKind::Primary, &label, true, Some(Icon::Commit))
            });
            let push = ui.add_enabled_ui(can, |ui| {
                widgets::button(
                    ui,
                    ButtonKind::Secondary,
                    "commit and push",
                    true,
                    Some(Icon::Commit),
                )
            });
            for (r, push) in [(commit.inner, false), (push.inner, true)] {
                if r.clicked() {
                    let format_and_check = state.format_and_check;
                    state.change(
                        system,
                        if format_and_check {
                            "formatting and checking the staged files…"
                        } else {
                            "committing…"
                        },
                        GitRequest::Commit {
                            message: message.clone(),
                            format_and_check,
                            push,
                        },
                    );
                }
            }
        });
        forms::checkbox(
            ui,
            &mut state.format_and_check,
            "format and check first",
            None,
        );
        caption(
            ui,
            "the staged files: wowlua fmt, then wowlua check; hooks after",
        );
    });
    ui.add_space(4.0);
    ui.painter().hline(
        ui.max_rect().x_range(),
        ui.cursor().top(),
        Stroke::new(1.0, p.line),
    );
    // The files.
    let mut pick: Option<Selected> = None;
    let mut request: Option<(&str, GitRequest)> = None;
    egui::ScrollArea::vertical()
        .id_salt("git-changes")
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if !conflicts.is_empty() {
                section(ui, "conflicts", conflicts.len(), p.error);
                for f in &conflicts {
                    let sel =
                        state.selected == Some(Selected::File(f.path.clone(), DiffOf::Unstaged));
                    let (click, _) = file_row(
                        ui,
                        &f.path,
                        "resolve in the editor",
                        None,
                        Change::Conflicted,
                        sel,
                    );
                    if click {
                        pick = Some(Selected::File(f.path.clone(), DiffOf::Unstaged));
                    }
                }
            }
            section(ui, "staged", staged.len(), p.ink_muted);
            for f in &staged {
                let sel = state.selected == Some(Selected::File(f.path.clone(), DiffOf::Staged));
                let note = if f.unstaged.is_some() { "part" } else { "" };
                let (click, button) = file_row(
                    ui,
                    &f.path,
                    note,
                    Some((Icon::Minimize, "unstage")),
                    f.staged.unwrap_or(Change::Modified),
                    sel,
                );
                if click {
                    pick = Some(Selected::File(f.path.clone(), DiffOf::Staged));
                }
                if button {
                    request = Some(("unstaging…", GitRequest::Unstage(vec![f.path.clone()])));
                }
            }
            section(ui, "changes", unstaged.len(), p.ink_muted);
            for f in &unstaged {
                let sel = state.selected == Some(Selected::File(f.path.clone(), DiffOf::Unstaged));
                let note = if f.staged.is_some() { "part" } else { "" };
                let (click, button) = file_row(
                    ui,
                    &f.path,
                    note,
                    Some((Icon::Add, "stage")),
                    f.unstaged.unwrap_or(Change::Modified),
                    sel,
                );
                if click {
                    pick = Some(Selected::File(f.path.clone(), DiffOf::Unstaged));
                }
                if button {
                    request = Some(("staging…", GitRequest::Stage(vec![f.path.clone()])));
                }
            }
            if status.files.is_empty() {
                forms::padded(ui, 12, 10, |ui| {
                    caption(
                        ui,
                        &format!(
                            "Nothing to commit: the working tree matches {}.",
                            status.branch.as_deref().unwrap_or(&status.head)
                        ),
                    );
                });
            }
        });
    if let Some(s) = pick {
        state.selected = Some(s);
        state.commit_file = 0;
    }
    if let Some((busy, r)) = request {
        state.change(system, busy, r);
    }
    action
}

/// A switch asked for: straight away without changes, else the dialog.
fn start_switch(state: &mut GitState, system: &dyn System, target: String) {
    let has_changes = state.status.as_ref().is_some_and(|s| !s.files.is_empty());
    if has_changes {
        if let Some(root) = state.root() {
            system.git(root, GitRequest::Blocking(target.clone()));
        }
        state.dialog = Some(Dialog::Switch {
            target,
            blocking: None,
            carry: Carry::Carry,
        });
    } else {
        state.change(
            system,
            "switching…",
            GitRequest::Switch {
                target,
                carry: Carry::Carry,
            },
        );
    }
}

// ------------------------------------------------------------ history --

/// The history panel: the branch's commits, newest first; one opens its
/// view.
pub fn history(ui: &mut Ui, state: &mut GitState, _system: &dyn System) -> Option<Action> {
    let p = palette(ui.ctx());
    let mut pick = None;
    egui::ScrollArea::vertical()
        .id_salt("git-history")
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if state.log.is_empty() {
                forms::padded(ui, 12, 10, |ui| caption(ui, "No commits yet."));
            }
            for (i, c) in state.log.iter().enumerate() {
                let width = ui.available_width();
                let (rect, response) =
                    ui.allocate_exact_size(Vec2::new(width, 26.0), Sense::click());
                let selected = state.selected == Some(Selected::Commit(c.hash.clone()));
                if selected {
                    ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_400);
                } else if response.hovered() {
                    ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_300);
                }
                let node = egui::pos2(rect.left() + 20.0, rect.center().y);
                if i == 0 {
                    ui.painter().circle_filled(node, 4.5, p.amber);
                } else {
                    ui.painter()
                        .circle_stroke(node, 3.5, Stroke::new(2.0, p.ink_muted));
                }
                let meta = format!("{} · {}", c.author, age(c.time));
                let meta_w = ui
                    .painter()
                    .text(
                        egui::pos2(rect.right() - 12.0, rect.center().y),
                        Align2::RIGHT_CENTER,
                        &meta,
                        Face::Plex.at(11.0),
                        p.ink_muted,
                    )
                    .width();
                let refs: Vec<&str> = c
                    .refs
                    .iter()
                    .map(|r| r.trim_start_matches("HEAD -> ").trim_start_matches("tag: "))
                    .filter(|r| *r != "HEAD")
                    .collect();
                let mut job = LayoutJob::default();
                job.append(
                    &c.subject,
                    0.0,
                    egui::TextFormat::simple(Face::Plex.at(12.5), p.ink),
                );
                if !refs.is_empty() {
                    job.append(
                        &refs.join(" · "),
                        8.0,
                        egui::TextFormat::simple(Face::Plex.at(11.0), p.ink_muted),
                    );
                }
                job.wrap.max_width = (rect.width() - 44.0 - meta_w - 12.0).max(40.0);
                job.wrap.max_rows = 1;
                job.wrap.break_anywhere = true;
                let galley = ui.fonts_mut(|f| f.layout_job(job));
                ui.painter().galley(
                    egui::pos2(rect.left() + 34.0, rect.center().y - galley.size().y / 2.0),
                    galley,
                    p.ink,
                );
                if response.clicked() {
                    pick = Some(c.hash.clone());
                }
            }
        });
    if let Some(hash) = pick {
        state.selected = Some(Selected::Commit(hash));
        state.commit_file = 0;
    }
    None
}

// --------------------------------------------------------------- main --

/// The main view: what the last change did, the merge under way, then a
/// file's hunks or a commit.
pub fn main(ui: &mut Ui, state: &mut GitState, system: &dyn System) -> Option<Action> {
    let p = palette(ui.ctx());
    let mut action = None;
    state.page_shown(system);
    // A merge under way.
    if let Some(status) = state.status.clone()
        && let Some(merging) = &status.merging
    {
        let left: Vec<String> = status.conflicts().map(|f| f.path.clone()).collect();
        forms::callout(ui, CalloutKind::Warning, |ui| {
            let text = if left.is_empty() {
                format!(
                    "merging {merging}: no conflicts left. Commit the merge in the changes panel."
                )
            } else {
                format!(
                    "merge conflict: merging {merging} into {} conflicts in {}. Resolve each in the editor, conflict by conflict, then commit the merge.",
                    status.branch.as_deref().unwrap_or("HEAD"),
                    left.join(", ")
                )
            };
            forms::paragraph(ui, &text, 12.5, 19.0, p.ink);
            ui.horizontal_wrapped(|ui| {
                let target = match &state.selected {
                    Some(Selected::File(path, _)) if left.contains(path) => Some(path.clone()),
                    _ => left.first().cloned(),
                };
                if let Some(path) = &target {
                    let name = path.rsplit('/').next().unwrap_or(path);
                    if widgets::button(ui, ButtonKind::Primary, &format!("open {name}"), true, None)
                        .clicked()
                    {
                        action = Some(Action::Open(status.root.join(path), None));
                    }
                    if widgets::button(
                        ui,
                        ButtonKind::Secondary,
                        &format!("take mine for {name}"),
                        true,
                        None,
                    )
                    .clicked()
                    {
                        state.change(
                            system,
                            "taking yours…",
                            GitRequest::TakeSide {
                                path: path.clone(),
                                side: Side::Mine,
                            },
                        );
                    }
                    if widgets::button(
                        ui,
                        ButtonKind::Secondary,
                        &format!("take {merging}'s for {name}"),
                        true,
                        None,
                    )
                    .clicked()
                    {
                        state.change(
                            system,
                            "taking theirs…",
                            GitRequest::TakeSide {
                                path: path.clone(),
                                side: Side::Theirs,
                            },
                        );
                    }
                }
                if widgets::button(ui, ButtonKind::Tertiary, "abort the merge", true, None)
                    .clicked()
                {
                    state.change(system, "aborting the merge…", GitRequest::Abort);
                }
            });
        });
    }
    if let Some(busy) = &state.busy {
        forms::padded(ui, 16, 6, |ui| caption(ui, busy));
    }
    if let Some(c) = state.callout.clone()
        && let Some(a) = callout(ui, state, system, &c)
    {
        action = Some(a);
    }
    match state.selected.clone() {
        Some(Selected::File(path, of)) => file_view(ui, state, system, &path, &of, &mut action),
        Some(Selected::Commit(hash)) => commit_view(ui, state, system, &hash),
        None => {
            forms::padded(ui, 16, 16, |ui| {
                caption(
                    ui,
                    "Pick a changed file for its diff, or a commit of the history.",
                );
            });
        }
    }
    action
}

/// What the last commit, push or change did.
fn callout(ui: &mut Ui, state: &mut GitState, system: &dyn System, c: &Callout) -> Option<Action> {
    let p = palette(ui.ctx());
    let mut action = None;
    match c {
        Callout::Committed(done) => forms::callout(ui, CalloutKind::Ok, |ui| {
            let mut parts = vec![format!("committed \"{}\" ({})", done.message, done.hash)];
            if !done.formatted.is_empty() {
                parts.push(format!(
                    "formatted {} and staged again",
                    done.formatted.join(", ")
                ));
            }
            if !done.not_formatted.is_empty() {
                parts.push(format!(
                    "not formatted (unstaged changes too): {}",
                    done.not_formatted.join(", ")
                ));
            }
            if done.checked {
                parts.push(format!(
                    "wowlua check: 0 errors, {} warnings",
                    done.warnings
                ));
            }
            if !done.hooks.trim().is_empty() {
                parts.push(format!(
                    "hooks: {}",
                    done.hooks.lines().last().unwrap_or("").trim()
                ));
            }
            match &done.pushed {
                Some(Ok(said)) => parts.push(said.clone()),
                Some(Err(e)) => parts.push(format!("not pushed: {e}")),
                None => {}
            }
            forms::paragraph(ui, &parts.join(" · "), 12.5, 19.0, p.ink);
        }),
        Callout::Stopped(CommitStop::Errors { formatted, errors }) => {
            forms::callout(ui, CalloutKind::Error, |ui| {
                let mut text = format!(
                    "commit stopped: wowlua check found {} error{} in the staged files",
                    errors.len(),
                    if errors.len() == 1 { "" } else { "s" }
                );
                if !formatted.is_empty() {
                    text.push_str(&format!(
                        " ({} formatted and staged again first)",
                        formatted.join(", ")
                    ));
                }
                forms::paragraph(ui, &text, 12.5, 19.0, p.ink);
                for (file, line, code, message) in errors.iter().take(8) {
                    ui.horizontal(|ui| {
                        caption(ui, &format!("{file}:{line} · {code} {message}"));
                        if widgets::button(ui, ButtonKind::Inline, "open", true, None).clicked()
                            && let Some(st) = &state.status
                        {
                            action = Some(Action::Open(st.root.join(file), Some(*line)));
                        }
                    });
                }
            });
        }
        Callout::Stopped(CommitStop::Failed(e)) | Callout::Failed(e) => {
            forms::callout(ui, CalloutKind::Error, |ui| {
                forms::paragraph(ui, e, 12.5, 19.0, p.ink);
            });
        }
        Callout::Remote(RemoteError::Credentials(e)) => {
            forms::callout(ui, CalloutKind::Note, |ui| {
                forms::paragraph(
                    ui,
                    "push needs credentials: the remote asked for them. Studio pushes with your GitHub sign-in, and with your own git setup (an SSH key, a credential manager) when you're signed out. Your commits stay local until a push works.",
                    12.5,
                    19.0,
                    p.ink,
                );
                caption(ui, e);
                ui.horizontal(|ui| {
                    if widgets::button(ui, ButtonKind::Secondary, "sign in to GitHub", true, None)
                        .clicked()
                    {
                        action = Some(Action::SignIn);
                    }
                    if widgets::button(
                        ui,
                        ButtonKind::Tertiary,
                        "push again",
                        true,
                        Some(Icon::Publish),
                    )
                    .clicked()
                    {
                        state.change(system, "pushing…", GitRequest::Push);
                    }
                });
            })
        }
        Callout::Remote(RemoteError::Rejected(e)) => forms::callout(ui, CalloutKind::Error, |ui| {
            forms::paragraph(
                ui,
                "push rejected: the remote has commits this branch doesn't. Pull first, then push again.",
                12.5,
                19.0,
                p.ink,
            );
            caption(ui, e);
            if widgets::button(ui, ButtonKind::Secondary, "pull", true, None).clicked() {
                state.change(system, "pulling…", GitRequest::Pull);
            }
        }),
        Callout::Remote(RemoteError::Other(e)) => forms::callout(ui, CalloutKind::Error, |ui| {
            forms::paragraph(ui, e, 12.5, 19.0, p.ink);
        }),
        Callout::Done(said) => forms::callout(ui, CalloutKind::Ok, |ui| {
            forms::paragraph(ui, said, 12.5, 19.0, p.ink);
        }),
    }
    action
}

/// A line's text in the editor's syntax colours.
fn code_line(ui: &Ui, system: &dyn System, text: &str, color: Color32) -> LayoutJob {
    let p = palette(ui.ctx());
    let font = Face::Plex.at(crate::editor::view::font_size());
    let (shown, _) = crate::editor::view::display(text);
    let mut job = LayoutJob::default();
    if shown != text {
        job.append(&shown, 0.0, egui::TextFormat::simple(font, color));
        return job;
    }
    let mut at = 0;
    for h in system.highlight(text) {
        if h.start > at {
            job.append(
                &text[at..h.start],
                0.0,
                egui::TextFormat::simple(font.clone(), color),
            );
        }
        if h.end > h.start && h.end <= text.len() {
            job.append(
                &text[h.start..h.end],
                0.0,
                crate::editor::view::style(h.syntax, &font, p),
            );
            at = h.end;
        }
    }
    if at < text.len() {
        job.append(&text[at..], 0.0, egui::TextFormat::simple(font, color));
    }
    job
}

/// A row of a diff: the sign, the line's number, its text on a ground.
fn diff_row(
    ui: &mut Ui,
    system: &dyn System,
    sign: &str,
    number: Option<u32>,
    text: &str,
    ground: Color32,
) {
    let p = palette(ui.ctx());
    let h = crate::editor::view::line_h();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, ground);
    let font = Face::Plex.at(crate::editor::view::font_size());
    ui.painter().text(
        egui::pos2(rect.left() + 10.0, rect.center().y),
        Align2::CENTER_CENTER,
        sign,
        font.clone(),
        p.amber_ink,
    );
    if let Some(n) = number {
        ui.painter().text(
            egui::pos2(rect.left() + 56.0, rect.center().y),
            Align2::RIGHT_CENTER,
            n.to_string(),
            font,
            p.ink_muted,
        );
    }
    let job = code_line(ui, system, text, p.ink);
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let painter = ui.painter().with_clip_rect(rect);
    painter.galley(
        egui::pos2(rect.left() + 72.0, rect.center().y - galley.size().y / 2.0),
        galley,
        p.ink,
    );
}

/// A hunk's head: where it is, which of how many, staged or not, its
/// buttons.
fn hunk_head(ui: &mut Ui, hunk: &Hunk, n: usize, of: usize, staged: Option<bool>) -> Option<Apply> {
    let p = palette(ui.ctx());
    let mut out = None;
    let frame = egui::Frame::new()
        .fill(p.bg_200)
        .stroke(Stroke::new(1.0, p.line))
        .inner_margin(egui::Margin::symmetric(12, 3));
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let state = match staged {
                Some(true) => " · staged",
                Some(false) => " · not staged",
                None => "",
            };
            caption(
                ui,
                &format!(
                    "@@ {}–{} · hunk {n} of {of}{state}{}",
                    hunk.new_start,
                    hunk.new_start + hunk.new_lines.saturating_sub(1),
                    if hunk.section.is_empty() {
                        String::new()
                    } else {
                        format!(" · {}", hunk.section)
                    }
                ),
            );
            ui.with_layout(
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| match staged {
                    Some(true) => {
                        if widgets::button(
                            ui,
                            ButtonKind::Inline,
                            "unstage hunk",
                            true,
                            Some(Icon::Minimize),
                        )
                        .clicked()
                        {
                            out = Some(Apply::Unstage);
                        }
                    }
                    Some(false) => {
                        if widgets::button(ui, ButtonKind::Inline, "discard hunk", true, None)
                            .clicked()
                        {
                            out = Some(Apply::Discard);
                        }
                        if widgets::button(ui, ButtonKind::Inline, "stage hunk", true, None)
                            .clicked()
                        {
                            out = Some(Apply::Stage);
                        }
                    }
                    None => {}
                },
            );
        });
    });
    out
}

/// A hunk's lines: side by side (before | after) or inline.
fn hunk_lines(ui: &mut Ui, system: &dyn System, hunk: &Hunk, side_by_side: bool) {
    let p = palette(ui.ctx());
    if !side_by_side {
        for l in &hunk.lines {
            let (sign, n, ground) = match l.kind {
                LineKind::Context => (" ", l.new, Color32::TRANSPARENT),
                LineKind::Added => ("+", l.new, p.success_soft),
                LineKind::Removed => ("−", None, p.error_soft),
            };
            diff_row(ui, system, sign, n, &l.text, ground);
        }
        return;
    }
    // Side by side: the removed lines beside the added ones of a change.
    let mut rows: Vec<(Option<&super::model::Line>, Option<&super::model::Line>)> = Vec::new();
    let mut i = 0;
    while i < hunk.lines.len() {
        let l = &hunk.lines[i];
        if l.kind == LineKind::Context {
            rows.push((Some(l), Some(l)));
            i += 1;
            continue;
        }
        let removed: Vec<&super::model::Line> = hunk.lines[i..]
            .iter()
            .take_while(|l| l.kind == LineKind::Removed)
            .collect();
        i += removed.len();
        let added: Vec<&super::model::Line> = hunk.lines[i..]
            .iter()
            .take_while(|l| l.kind == LineKind::Added)
            .collect();
        i += added.len();
        for k in 0..removed.len().max(added.len()) {
            rows.push((removed.get(k).copied(), added.get(k).copied()));
        }
    }
    ui.columns(2, |cols| {
        for (left, right) in &rows {
            for (col, line, old) in [(0, left, true), (1, right, false)] {
                let ui = &mut cols[col];
                ui.spacing_mut().item_spacing.y = 0.0;
                match line {
                    Some(l) => {
                        let (sign, ground) = match l.kind {
                            LineKind::Context => (" ", Color32::TRANSPARENT),
                            LineKind::Added => ("+", p.success_soft),
                            LineKind::Removed => ("−", p.error_soft),
                        };
                        diff_row(
                            ui,
                            system,
                            sign,
                            if old { l.old } else { l.new },
                            &l.text,
                            ground,
                        );
                    }
                    None => diff_row(ui, system, "", None, "", p.bg_100),
                }
            }
        }
    });
}

/// A changed file's diff, hunk by hunk.
fn file_view(
    ui: &mut Ui,
    state: &mut GitState,
    system: &dyn System,
    path: &str,
    of: &DiffOf,
    action: &mut Option<Action>,
) {
    let p = palette(ui.ctx());
    let file = state.status.as_ref().and_then(|s| s.file(path)).cloned();
    let mode = match of {
        DiffOf::Staged => "staged against HEAD",
        DiffOf::Unstaged => "working tree against the index",
        DiffOf::Commit(_) => "",
    };
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new(path)
                .font(Face::PlexMedium.at(12.5))
                .color(p.ink),
        );
        caption(ui, &format!("· {mode}"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(8.0);
            let picked = widgets::seg(
                ui,
                "git-diff-view",
                &["side by side", "inline"],
                if state.side_by_side { 0 } else { 1 },
                false,
            );
            if let Some(i) = picked {
                state.side_by_side = i == 0;
            }
            if widgets::button(ui, ButtonKind::Inline, "open the file", true, None).clicked()
                && let Some(st) = &state.status
            {
                *action = Some(Action::Open(st.root.join(path), None));
            }
            if let Some(f) = &file
                && *of == DiffOf::Unstaged
                && f.unstaged.is_some()
                && f.unstaged != Some(Change::Conflicted)
                && widgets::button(
                    ui,
                    ButtonKind::Inline,
                    "discard the file's changes",
                    true,
                    None,
                )
                .clicked()
            {
                state.dialog = Some(Dialog::Discard {
                    what: format!("the changes to {path}"),
                    request: GitRequest::Discard(vec![path.to_owned()]),
                });
            }
        });
    });
    let Some(diff) = state.diff(path, of).cloned() else {
        forms::padded(ui, 16, 10, |ui| caption(ui, "Reading the diff…"));
        return;
    };
    if diff.binary {
        forms::padded(ui, 16, 10, |ui| {
            caption(ui, "A binary file: no lines to show.")
        });
        return;
    }
    if file
        .as_ref()
        .is_some_and(|f| f.change() == Change::Conflicted)
    {
        forms::padded(ui, 16, 10, |ui| {
            caption(
                ui,
                "In conflict: open it in the editor to take each conflict's side, or take a whole side above.",
            );
        });
    }
    let n = diff.hunks.len();
    let staged = match of {
        DiffOf::Staged => Some(true),
        DiffOf::Unstaged
            if !diff.new_file
                && file
                    .as_ref()
                    .is_some_and(|f| f.change() != Change::Conflicted) =>
        {
            Some(false)
        }
        _ => None,
    };
    let mut apply: Option<(Hunk, Apply)> = None;
    egui::ScrollArea::vertical()
        .id_salt(("git-diff", path, format!("{of:?}")))
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (i, hunk) in diff.hunks.iter().enumerate() {
                if let Some(how) = hunk_head(ui, hunk, i + 1, n, staged) {
                    apply = Some((hunk.clone(), how));
                }
                hunk_lines(ui, system, hunk, state.side_by_side);
            }
            let (added, removed) = diff
                .hunks
                .iter()
                .fold((0, 0), |(a, r), h| (a + h.added(), r + h.removed()));
            forms::padded(ui, 16, 10, |ui| {
                caption(ui, &format!("{n} hunks · +{added} −{removed}"));
            });
        });
    if let Some((hunk, how)) = apply {
        let request = GitRequest::Hunk {
            path: path.to_owned(),
            hunk,
            how,
        };
        match how {
            Apply::Discard => {
                state.dialog = Some(Dialog::Discard {
                    what: format!("a hunk of {path}"),
                    request,
                });
            }
            Apply::Stage => state.change(system, "staging the hunk…", request),
            Apply::Unstage => state.change(system, "unstaging the hunk…", request),
        }
    }
}

/// A commit's view: its message, who and when, what to do with it, its
/// files and the picked one's diff.
fn commit_view(ui: &mut Ui, state: &mut GitState, system: &dyn System, hash: &str) {
    let p = palette(ui.ctx());
    let Some(c) = state.log.iter().find(|c| c.hash == hash).cloned() else {
        return;
    };
    forms::padded(ui, 20, 14, |ui| {
        ui.label(
            egui::RichText::new(&c.subject)
                .font(Face::PlexSemiBold.at(14.0))
                .color(p.ink),
        );
        if !c.body.is_empty() {
            forms::paragraph(ui, &c.body, 12.5, 19.0, p.ink);
        }
        let parent = c
            .parents
            .first()
            .map_or("none".to_owned(), |h| h.chars().take(7).collect());
        caption(
            ui,
            &format!(
                "{} <{}> · {} ago · {} · parent {parent}{}",
                c.author,
                c.email,
                age(c.time),
                c.short,
                if c.refs.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", c.refs.join(", "))
                }
            ),
        );
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if widgets::button(
                ui,
                ButtonKind::Secondary,
                "switch to it",
                true,
                Some(Icon::Git),
            )
            .clicked()
            {
                state.change(
                    system,
                    "switching…",
                    GitRequest::SwitchCommit(c.hash.clone()),
                );
            }
            if widgets::button(
                ui,
                ButtonKind::Secondary,
                "new branch from it",
                true,
                Some(Icon::Git),
            )
            .clicked()
            {
                state.dialog = Some(Dialog::NewBranch {
                    from: Some(c.hash.clone()),
                    name: String::new(),
                    switch: true,
                    bad: None,
                });
            }
            if widgets::button(ui, ButtonKind::Tertiary, "revert it", true, None).clicked() {
                state.change(system, "reverting…", GitRequest::Revert(c.hash.clone()));
            }
            if widgets::button(ui, ButtonKind::Tertiary, "copy hash", true, None).clicked() {
                ui.ctx().copy_text(c.hash.clone());
            }
        });
        caption(
            ui,
            "Switching to a commit leaves no branch checked out (git's detached HEAD) until you make one from it. Reverting adds a commit that undoes this one.",
        );
    });
    let Some(files) = state.commit_files(hash).map(<[_]>::to_vec) else {
        forms::padded(ui, 20, 6, |ui| caption(ui, "Reading the commit…"));
        return;
    };
    let (added, removed) = files
        .iter()
        .fold((0, 0), |(a, r), f| (a + f.added, r + f.removed));
    egui::ScrollArea::vertical()
        .id_salt(("git-commit", hash))
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            section(
                ui,
                &format!("{} files · +{added} −{removed}", files.len()),
                files.len(),
                p.ink_muted,
            );
            for (i, f) in files.iter().enumerate() {
                let note = format!("+{} −{}", f.added, f.removed);
                let (click, _) = file_row(
                    ui,
                    &f.path,
                    &note,
                    None,
                    Change::Modified,
                    i == state.commit_file,
                );
                if click {
                    state.commit_file = i;
                }
            }
            if let Some(f) = files.get(state.commit_file) {
                let of = DiffOf::Commit(hash.to_owned());
                match state.diff(&f.path, &of).cloned() {
                    Some(d) => {
                        let n = d.hunks.len();
                        for (i, h) in d.hunks.iter().enumerate() {
                            hunk_head(ui, h, i + 1, n, None);
                            hunk_lines(ui, system, h, false);
                        }
                    }
                    None => forms::padded(ui, 16, 10, |ui| caption(ui, "Reading the diff…")),
                }
            }
        });
}

// ------------------------------------------------------------ dialogs --

/// The page's dialogs: switching with changes, a new branch, discarding.
pub fn dialogs(ctx: &egui::Context, state: &mut GitState, system: &dyn System) {
    let Some(dialog) = state.dialog.clone() else {
        return;
    };
    let p = palette(ctx);
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::unique("wowlua-studio-git-dialog")).show(ctx, |ui| {
        ui.set_width(520.0);
        ui.spacing_mut().item_spacing.y = 12.0;
        let title = |ui: &mut Ui, text: &str| {
            ui.label(egui::RichText::new(text).font(Face::PlexSemiBold.at(13.0)).color(p.ink));
        };
        match dialog {
            Dialog::Switch { target, blocking, carry } => {
                let changed = state.status.as_ref().map_or(0, |s| s.files.len());
                let from = state
                    .status
                    .as_ref()
                    .and_then(|s| s.branch.clone())
                    .unwrap_or_else(|| "HEAD".into());
                title(ui, &format!("switch to {target}"));
                caption(ui, &format!("{changed} changed files on {from}"));
                let can_carry = blocking.as_ref().is_some_and(Vec::is_empty);
                let carry_text = match &blocking {
                    None => "Checking whether git can carry them…".to_owned(),
                    Some(b) if b.is_empty() => "They come along as they are: none of them differs between the branches.".to_owned(),
                    Some(b) => format!("Not possible: {} differs on {target}, so git would have to overwrite your changes.", b.join(", ")),
                };
                let mut pick = carry;
                if option(ui, &format!("carry them to {target}"), &carry_text, carry == Carry::Carry, can_carry) {
                    pick = Carry::Carry;
                }
                if option(
                    ui,
                    "stash them",
                    &format!("Kept as {from}'s stash and put back when you switch back to it."),
                    carry == Carry::Stash,
                    true,
                ) {
                    pick = Carry::Stash;
                }
                if pick != carry
                    && let Some(Dialog::Switch { carry, .. }) = &mut state.dialog
                {
                    *carry = pick;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    let ready = blocking.is_some() && (pick == Carry::Stash || can_carry);
                    let go = ui.add_enabled_ui(ready, |ui| {
                        widgets::button(ui, ButtonKind::Primary, &format!("switch to {target}"), true, Some(Icon::Git))
                    });
                    if go.inner.clicked() {
                        close = true;
                        state.change(system, "switching…", GitRequest::Switch { target: target.clone(), carry: pick });
                    }
                    if widgets::button(ui, ButtonKind::Tertiary, "cancel", true, None).clicked() {
                        close = true;
                    }
                });
            }
            Dialog::NewBranch { from, name, switch, bad } => {
                title(ui, "new branch");
                let from_text = match &from {
                    Some(h) => format!("from commit {}", h.chars().take(7).collect::<String>()),
                    None => format!(
                        "from {}",
                        state.status.as_ref().map_or("HEAD".into(), |s| s.branch_label())
                    ),
                };
                caption(ui, &from_text);
                let mut typed = name.clone();
                forms::label(ui, "name");
                if forms::text_input(ui, &mut typed, 480.0, "feat/threat-colors").changed() {
                    if let Some(root) = state.root() {
                        system.git(root, GitRequest::BranchName(typed.clone()));
                    }
                    if let Some(Dialog::NewBranch { name, bad, .. }) = &mut state.dialog {
                        *name = typed.clone();
                        *bad = None;
                    }
                }
                let mut sw = switch;
                if forms::checkbox(ui, &mut sw, "switch to it (your changes come along)", None)
                    && let Some(Dialog::NewBranch { switch, .. }) = &mut state.dialog
                {
                    *switch = sw;
                }
                if let Some(why) = &bad {
                    ui.label(egui::RichText::new(why).font(Face::Plex.at(11.5)).color(p.error));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    let ready = !typed.trim().is_empty() && bad.is_none();
                    let go = ui.add_enabled_ui(ready, |ui| {
                        widgets::button(ui, ButtonKind::Primary, "create the branch", true, Some(Icon::Git))
                    });
                    if go.inner.clicked() {
                        close = true;
                        state.change(
                            system,
                            "making the branch…",
                            GitRequest::NewBranch { name: typed.trim().to_owned(), from: from.clone(), switch: sw },
                        );
                    }
                    if widgets::button(ui, ButtonKind::Tertiary, "cancel", true, None).clicked() {
                        close = true;
                    }
                });
            }
            Dialog::Discard { what, request } => {
                title(ui, "discard changes");
                forms::paragraph(
                    ui,
                    &format!("Discard {what}? Git can't bring discarded changes back."),
                    13.0,
                    21.0,
                    p.ink,
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if widgets::button(ui, ButtonKind::Primary, "discard", true, None).clicked() {
                        close = true;
                        state.change(system, "discarding…", request.clone());
                    }
                    if widgets::button(ui, ButtonKind::Tertiary, "cancel", true, None).clicked() {
                        close = true;
                    }
                });
            }
        }
    });
    if close || modal.should_close() {
        state.dialog = None;
    }
}

/// A choice of a dialog: a title and why, picked or not; whether it was
/// clicked.
fn option(ui: &mut Ui, title: &str, text: &str, on: bool, enabled: bool) -> bool {
    let p = palette(ui.ctx());
    let (fill, stroke) = if on && enabled {
        (p.amber_soft, p.amber_ink)
    } else {
        (Color32::TRANSPARENT, p.line)
    };
    let r = egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, stroke))
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let ink = if enabled { p.ink } else { p.ink_faint };
            ui.label(
                egui::RichText::new(title)
                    .font(Face::PlexMedium.at(12.5))
                    .color(ink),
            );
            ui.label(
                egui::RichText::new(text)
                    .font(Face::Plex.at(11.5))
                    .color(if enabled { p.ink_muted } else { p.ink_faint }),
            );
        })
        .response;
    let r = ui.interact(
        r.rect,
        egui::Id::unique(("wowlua-studio-git-option", title)),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    enabled && r.clicked()
}
