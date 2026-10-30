//! The pull requests page (M12, the canvas's pull request board): the
//! repository's open pull requests and Actions runs at the left; a pull
//! request's conversation, checks and files, a run's jobs, steps, log and
//! annotations, or the new pull request's form in the main view; the
//! review dialog over it.

use std::collections::BTreeSet;

use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, Ui, Vec2};
use wowlua_studio_ui::forms::{self, CalloutKind};
use wowlua_studio_ui::widgets::{self, ButtonKind};
use wowlua_studio_ui::{Face, Icon, palette};

use super::model::{Comment, GithubError, Job, MergeMethod, Pending, PullDetail, ReviewEvent, Run};
use super::{GithubState, Selected};
use crate::git::model::LineKind;
use crate::git::page::{age, caption, diff_row, fit, hunk_head, section};
use crate::system::{GithubRequest, System};

/// What the page asks of the rest of Studio.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// A file of the workspace at a line (1-based), from an annotation.
    Open(String, u64),
    /// The settings' GitHub sign-in.
    SignIn,
    /// The git page (to push a branch).
    ShowGit,
}

/// A pull request's tab.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Conversation,
    Checks,
    Files,
}

/// What the page's controls hold between frames.
#[derive(Default)]
pub struct View {
    pub tab: Tab,
    /// The workflow the runs list shows (none: every one).
    pub workflow: Option<String>,
    pub new_text: String,
    /// The thread being replied to (its first comment), and the reply.
    pub replying: Option<u64>,
    pub reply_text: String,
    /// The line a comment is being written on (path, line), and it.
    pub draft: Option<(String, u64)>,
    pub draft_text: String,
    /// The file the files tab shows.
    pub file: usize,
    pub inline: bool,
    pub method: usize,
    /// The jobs open in a run, the step whose log shows (job, step).
    pub open_jobs: BTreeSet<u64>,
    pub open_step: Option<(u64, u64)>,
    /// The review dialog: open, its summary and choice.
    pub reviewing: bool,
    pub review_text: String,
    pub review_event: usize,
    /// The new pull request's form.
    pub base: Option<String>,
    pub head: Option<String>,
    pub title: String,
    pub title_from: Option<(String, String)>,
    pub body: String,
    pub draft_pull: bool,
}

const METHODS: [(&str, MergeMethod); 3] = [
    ("squash and merge", MergeMethod::Squash),
    ("merge commit", MergeMethod::Merge),
    ("rebase and merge", MergeMethod::Rebase),
];

/// A run's mark and its colour: ✓ passed, × failed, ◌ running, · queued.
fn run_mark(
    status: &str,
    conclusion: Option<&str>,
    p: &wowlua_studio_ui::Palette,
) -> (&'static str, Color32) {
    match (status, conclusion) {
        ("completed", Some("success")) => ("✓", p.success),
        ("completed", Some("skipped")) => ("-", p.ink_faint),
        ("completed", Some("cancelled")) => ("×", p.ink_muted),
        ("completed", _) => ("×", p.error),
        ("in_progress", _) => ("◌", p.amber_ink),
        _ => ("·", p.ink_muted),
    }
}

/// A list row: a mark, a title over a line of detail, a note at the right;
/// whether it was clicked.
fn row(
    ui: &mut Ui,
    mark: (&str, Color32),
    title: &str,
    sub: &str,
    note: &str,
    selected: bool,
) -> bool {
    let p = palette(ui.ctx());
    let (rect, r) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::click());
    if selected {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_400);
    } else if r.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_300);
    }
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, p.line),
    );
    ui.painter().text(
        egui::pos2(rect.left() + 18.0, rect.top() + 13.0),
        Align2::CENTER_CENTER,
        mark.0,
        Face::PlexSemiBold.at(12.0),
        mark.1,
    );
    let note_w = ui
        .painter()
        .text(
            egui::pos2(rect.right() - 12.0, rect.top() + 13.0),
            Align2::RIGHT_CENTER,
            note,
            Face::Plex.at(11.0),
            p.ink_muted,
        )
        .width();
    let room = rect.width() - 34.0 - note_w - 20.0;
    ui.painter().text(
        egui::pos2(rect.left() + 30.0, rect.top() + 13.0),
        Align2::LEFT_CENTER,
        fit(ui, title, &Face::Plex.at(12.5), room),
        Face::Plex.at(12.5),
        p.ink,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 30.0, rect.top() + 29.0),
        Align2::LEFT_CENTER,
        fit(ui, sub, &Face::Plex.at(11.0), rect.width() - 44.0),
        Face::Plex.at(11.0),
        p.ink_muted,
    );
    r.clicked()
}

/// `you` for the signed-in user.
fn who<'a>(state: &'a GithubState, login: &'a str) -> &'a str {
    if state.login() == Some(login) {
        "you"
    } else {
        login
    }
}

// ---------------------------------------------------------- the list --

/// The left column: when it was read, the open pull requests, the runs.
pub fn list(ui: &mut Ui, state: &mut GithubState, system: &dyn System, now: f64) -> Option<Action> {
    let p = palette(ui.ctx());
    let action = None;
    let repo = match &state.repo {
        Some(Ok((r, _, _, _))) => Some(r.full()),
        _ => None,
    };
    forms::padded(ui, 12, 8, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(repo.clone().unwrap_or_else(|| "GitHub".into()))
                    .font(Face::PlexMedium.at(12.5))
                    .color(p.ink),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::icon_button(ui, Icon::Refresh, "read again from GitHub", false)
                    .clicked()
                {
                    state.refresh(system, now);
                }
            });
        });
        if repo.is_some() {
            let read = state.read_at.map_or("reading…".to_owned(), |at| {
                let s = (now - at).max(0.0) as i64;
                if s < 60 {
                    "read just now".to_owned()
                } else {
                    format!("read {} min ago", s / 60)
                }
            });
            let watching = matches!(&state.runs, Some(Ok(r)) if r.iter().any(Run::running));
            let rule = if watching {
                "every 15 s while a run goes"
            } else {
                "every 5 min, with git's fetch"
            };
            caption(ui, &format!("{read} · {rule}"));
        }
    });
    if repo.is_none() {
        return action;
    }
    egui::ScrollArea::vertical()
        .id_salt("github-list")
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let pulls = match &state.pulls {
                Some(Ok(p)) => p.clone(),
                _ => Vec::new(),
            };
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                caption(ui, &format!("pull requests · open {}", pulls.len()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(8.0);
                    if widgets::button(ui, ButtonKind::Inline, "new", true, Some(Icon::Add)).clicked() {
                        state.selected = Some(Selected::NewPull);
                    }
                });
            });
            if pulls.is_empty() {
                forms::padded(ui, 12, 6, |ui| caption(ui, "No open pull requests."));
            }
            for q in &pulls {
                let selected = state.selected == Some(Selected::Pull(q.number));
                let mark = if q.draft { ("○", p.ink_muted) } else { ("●", p.success) };
                let sub = format!("#{} · {} → {}", q.number, q.head, q.base);
                let author = who(state, &q.author).to_owned();
                if row(ui, mark, &q.title, &sub, &author, selected) {
                    state.selected = Some(Selected::Pull(q.number));
                    state.view.tab = Tab::Conversation;
                }
            }
            // The runs, by workflow.
            let runs = match &state.runs {
                Some(Ok(r)) => r.clone(),
                _ => Vec::new(),
            };
            let mut workflows: Vec<String> = runs.iter().map(|r| r.workflow.clone()).collect();
            workflows.sort();
            workflows.dedup();
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                caption(ui, "actions");
                if !workflows.is_empty() {
                    let mut names = vec!["every workflow".to_owned()];
                    names.extend(workflows.iter().cloned());
                    let mut index = state
                        .view
                        .workflow
                        .as_ref()
                        .and_then(|w| workflows.iter().position(|x| x == w))
                        .map_or(0, |i| i + 1);
                    let opts: Vec<(&str, bool)> = names.iter().map(|n| (n.as_str(), true)).collect();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(8.0);
                        let w = (ui.available_width() - 8.0).clamp(90.0, 160.0);
                        if forms::select_sized(ui, "github-workflow", &opts, &mut index, w, 26.0) {
                            state.view.workflow = index.checked_sub(1).and_then(|i| workflows.get(i).cloned());
                        }
                    });
                }
            });
            ui.add_space(4.0);
            let shown: Vec<&Run> = runs
                .iter()
                .filter(|r| state.view.workflow.as_ref().is_none_or(|w| *w == r.workflow))
                .take(20)
                .collect();
            if shown.is_empty() {
                forms::padded(ui, 12, 6, |ui| caption(ui, "No runs yet."));
            }
            for r in shown {
                let selected = state.selected == Some(Selected::Run(r.id));
                let sub = format!("{} · {} · {}", r.workflow, r.branch, r.event.replace('_', " "));
                if row(ui, run_mark(&r.status, r.conclusion.as_deref(), p), &r.title, &sub, &age(r.created), selected) {
                    state.selected = Some(Selected::Run(r.id));
                }
            }
            forms::padded(ui, 12, 8, |ui| {
                caption(ui, "The workflow's last 20 runs. A run of your push or pull request sends a notification when it finishes.");
            });
        });
    action
}

// ---------------------------------------------------------- the main --

/// The main view: what the left column picked, the signed-out and
/// no-remote pages, what the last change said.
pub fn main(
    ui: &mut Ui,
    state: &mut GithubState,
    system: &dyn System,
    branch: Option<&str>,
) -> Option<Action> {
    let p = palette(ui.ctx());
    let mut action = None;
    match &state.repo {
        None => {
            forms::padded(ui, 20, 18, |ui| {
                caption(ui, "Reading the repository on GitHub…")
            });
            return None;
        }
        Some(Err(GithubError::SignedOut)) => {
            forms::padded(ui, 24, 22, |ui| {
                ui.label(
                    egui::RichText::new("signed out of GitHub")
                        .font(Face::PlexSemiBold.at(15.0))
                        .color(p.ink),
                );
                forms::paragraph(
                    ui,
                    "Pull requests and Actions come from GitHub with your sign-in. Git keeps working without it: commit, pull and push use your own git setup.",
                    12.5,
                    19.0,
                    p.ink_muted,
                );
                ui.add_space(8.0);
                if widgets::button(ui, ButtonKind::Primary, "sign in to GitHub", true, None)
                    .clicked()
                {
                    action = Some(Action::SignIn);
                }
            });
            return action;
        }
        Some(Err(e)) => {
            let text = e.to_string();
            forms::padded(ui, 24, 22, |ui| {
                let remote = text.contains("no remote");
                ui.label(
                    egui::RichText::new(if remote {
                        "no GitHub remote"
                    } else {
                        "GitHub can't show this repository"
                    })
                    .font(Face::PlexSemiBold.at(15.0))
                    .color(p.ink),
                );
                forms::paragraph(
                    ui,
                    if remote {
                        "None of this folder's git remotes is on github.com, so there are no pull requests or Actions runs to show. Add a remote on github.com with git, and this page fills."
                    } else {
                        &text
                    },
                    12.5,
                    19.0,
                    p.ink_muted,
                );
                ui.add_space(8.0);
                if widgets::button(ui, ButtonKind::Secondary, "open the git page", true, None)
                    .clicked()
                {
                    action = Some(Action::ShowGit);
                }
            });
            return action;
        }
        Some(Ok(_)) => {}
    }
    if let Some(busy) = &state.busy {
        forms::padded(ui, 20, 6, |ui| caption(ui, busy));
    }
    match state.said.clone() {
        Some(Ok(said)) => forms::callout(ui, CalloutKind::Ok, |ui| {
            forms::paragraph(ui, &said, 12.5, 19.0, p.ink);
        }),
        Some(Err(e)) => forms::callout(ui, CalloutKind::Error, |ui| {
            forms::paragraph(ui, &e.to_string(), 12.5, 19.0, p.ink);
        }),
        None => {}
    }
    match state.selected.clone() {
        Some(Selected::Pull(n)) => {
            state.want_pull(system, n);
            if let Some(a) = pull_view(ui, state, system, n) {
                action = Some(a);
            }
        }
        Some(Selected::Run(id)) => {
            let run = match &state.runs {
                Some(Ok(r)) => r.iter().find(|r| r.id == id).cloned(),
                _ => None,
            };
            if let Some(run) = run {
                run_header(ui, state, &run);
                egui::ScrollArea::vertical()
                    .id_salt(("github-run", id))
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        if let Some(a) = run_body(ui, state, system, &run) {
                            action = Some(a);
                        }
                    });
            }
        }
        Some(Selected::NewPull) => {
            if let Some(a) = new_pull(ui, state, system, branch) {
                action = Some(a);
            }
        }
        None => {
            forms::padded(ui, 20, 18, |ui| {
                caption(ui, "Pick a pull request or a run at the left.")
            });
        }
    }
    action
}

// ------------------------------------------------------ a pull request --

fn pull_view(ui: &mut Ui, state: &mut GithubState, system: &dyn System, n: u64) -> Option<Action> {
    let p = palette(ui.ctx());
    let mut action = None;
    let Some(d) = state.details.get(&n).cloned() else {
        forms::padded(ui, 20, 18, |ui| caption(ui, &format!("Reading #{n}…")));
        return None;
    };
    let pull = &d.pull;
    let mine = state.login() == Some(pull.summary.author.as_str());
    // The header.
    forms::padded(ui, 20, 12, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(&pull.summary.title)
                    .font(Face::PlexSemiBold.at(15.0))
                    .color(p.ink),
            );
            ui.label(
                egui::RichText::new(format!("#{n}"))
                    .font(Face::Plex.at(15.0))
                    .color(p.ink_muted),
            );
            let (word, color) = if pull.merged {
                ("merged", p.note)
            } else if pull.state == "closed" {
                ("closed", p.error)
            } else if pull.summary.draft {
                ("draft", p.ink_muted)
            } else {
                ("open", p.success)
            };
            ui.label(
                egui::RichText::new(word)
                    .font(Face::PlexMedium.at(11.5))
                    .color(color),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::button(
                    ui,
                    ButtonKind::Tertiary,
                    "open on GitHub",
                    true,
                    Some(Icon::Open),
                )
                .clicked()
                {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(&pull.url));
                }
                let open = pull.state == "open";
                let review = ui.add_enabled_ui(open, |ui| {
                    widgets::button(ui, ButtonKind::Secondary, "review", true, None)
                });
                if review.inner.clicked() {
                    state.view.reviewing = true;
                    state.view.review_event = 0;
                }
            });
        });
        caption(
            ui,
            &format!(
                "{} → {} · {} commits · {} files · +{} −{} · opened by {} {} ago",
                pull.summary.head,
                pull.summary.base,
                pull.commits,
                pull.changed_files,
                pull.additions,
                pull.deletions,
                who(state, &pull.summary.author),
                age(pull.created)
            ),
        );
    });
    // The tabs.
    let checks: Vec<Run> = match &state.runs {
        Some(Ok(r)) => r
            .iter()
            .filter(|r| r.head_sha == pull.head_sha || r.pulls.contains(&n))
            .cloned()
            .collect(),
        _ => Vec::new(),
    };
    let conversation_n = d.comments.len()
        + d.reviews
            .iter()
            .filter(|r| !r.body.is_empty() || r.state != "COMMENTED")
            .count();
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        let labels = [
            format!("conversation {conversation_n}"),
            format!("checks {}", checks.len()),
            format!("files {}", d.files.len()),
        ];
        let tabs = [Tab::Conversation, Tab::Checks, Tab::Files];
        let at = tabs.iter().position(|t| *t == state.view.tab).unwrap_or(0);
        let names: Vec<&str> = labels.iter().map(String::as_str).collect();
        if let Some(i) = widgets::seg(ui, "github-tabs", &names, at, false) {
            state.view.tab = tabs[i];
        }
    });
    ui.add_space(6.0);
    egui::ScrollArea::vertical()
        .id_salt(("github-pull", n))
        .auto_shrink(false)
        .show(ui, |ui| match state.view.tab {
            Tab::Conversation => conversation(ui, state, system, &d),
            Tab::Checks => {
                if checks.is_empty() {
                    forms::padded(ui, 20, 10, |ui| {
                        caption(ui, "No runs for this pull request's head yet.")
                    });
                }
                for run in &checks {
                    if let Some(a) = run_card(ui, state, system, run) {
                        action = Some(a);
                    }
                    ui.add_space(10.0);
                }
            }
            Tab::Files => files(ui, state, system, &d),
        });
    if state.view.reviewing {
        review_dialog(ui.ctx(), state, system, &d, mine);
    }
    action
}

/// A card: a head line and a body.
fn card(ui: &mut Ui, head: impl FnOnce(&mut Ui), body: impl FnOnce(&mut Ui)) {
    card_of(ui, head, Some(body));
}

/// A card, its head alone without a body.
fn card_of<B: FnOnce(&mut Ui)>(ui: &mut Ui, head: impl FnOnce(&mut Ui), body: Option<B>) {
    let p = palette(ui.ctx());
    egui::Frame::new()
        .stroke(Stroke::new(1.0, p.line))
        .fill(p.bg_100)
        .outer_margin(egui::Margin::symmetric(20, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::Frame::new()
                .fill(p.bg_200)
                .inner_margin(egui::Margin::symmetric(12, 6))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(head);
                });
            if let Some(body) = body {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        body(ui);
                    });
            }
        });
}

fn who_line(ui: &mut Ui, author: &str, when: &str) {
    let p = palette(ui.ctx());
    ui.label(
        egui::RichText::new(author)
            .font(Face::PlexMedium.at(12.0))
            .color(p.ink),
    );
    ui.label(
        egui::RichText::new(format!("· {when}"))
            .font(Face::Plex.at(11.5))
            .color(p.ink_muted),
    );
}

/// A text box and its buttons; the buttons clicked (index).
fn composer(
    ui: &mut Ui,
    id: &str,
    text: &mut String,
    hint: &str,
    buttons: &[(&str, ButtonKind)],
) -> Option<usize> {
    let mut clicked = None;
    ui.add(
        egui::TextEdit::multiline(text)
            .id_salt(id)
            .desired_rows(3)
            .desired_width(ui.available_width())
            .font(Face::Plex.at(12.5))
            .hint_text(hint),
    );
    ui.add_space(4.0);
    let empty = text.trim().is_empty();
    ui.horizontal(|ui| {
        for (i, (label, kind)) in buttons.iter().enumerate() {
            let enabled = !empty || *label == "cancel";
            if ui
                .add_enabled_ui(enabled, |ui| widgets::button(ui, *kind, label, true, None))
                .inner
                .clicked()
            {
                clicked = Some(i);
            }
        }
    });
    clicked
}

/// A review thread: where, its comments, reply, resolve.
fn thread(
    ui: &mut Ui,
    state: &mut GithubState,
    system: &dyn System,
    number: u64,
    first: &Comment,
    replies: &[&Comment],
    d: &PullDetail,
) {
    let p = palette(ui.ctx());
    let t = d.threads.iter().find(|t| t.first == first.id).cloned();
    let resolved = t.as_ref().is_some_and(|t| t.resolved);
    let on = first.on.clone().unwrap_or_else(|| super::model::OnLine {
        path: String::new(),
        line: None,
        side: "RIGHT".into(),
        reply_to: None,
        hunk: String::new(),
    });
    let where_ = format!(
        "{}:{}",
        on.path,
        on.line.map_or("?".into(), |l| l.to_string())
    );
    card(
        ui,
        |ui| {
            ui.label(
                egui::RichText::new(&where_)
                    .font(Face::PlexMedium.at(12.0))
                    .color(p.ink),
            );
            if resolved {
                ui.label(
                    egui::RichText::new("resolved")
                        .font(Face::Plex.at(11.5))
                        .color(p.success),
                );
            }
        },
        |ui| {
            // (the line it is on, as GitHub quotes it)
            if !resolved && let Some(last) = on.hunk.lines().last() {
                let (sign, kind) = match last.chars().next() {
                    Some('+') => ("+", p.success_soft),
                    Some('-') => ("−", p.error_soft),
                    _ => (" ", Color32::TRANSPARENT),
                };
                diff_row(
                    ui,
                    system,
                    sign,
                    on.line.map(|l| l as u32),
                    last.get(1..).unwrap_or(""),
                    kind,
                );
                ui.add_space(6.0);
            }
            if resolved {
                return;
            }
            for c in std::iter::once(first).chain(replies.iter().copied()) {
                ui.horizontal(|ui| {
                    who_line(
                        ui,
                        who(state, &c.author),
                        &format!("{} ago", age(c.created)),
                    )
                });
                forms::paragraph(ui, &c.body, 12.5, 19.0, p.ink);
                ui.add_space(6.0);
            }
            if state.view.replying == Some(first.id) {
                let mut text = std::mem::take(&mut state.view.reply_text);
                match composer(
                    ui,
                    &format!("reply-{}", first.id),
                    &mut text,
                    "reply in this thread",
                    &[
                        ("reply", ButtonKind::Primary),
                        ("cancel", ButtonKind::Tertiary),
                    ],
                ) {
                    Some(0) => {
                        state.change(
                            system,
                            "replying…",
                            GithubRequest::Reply {
                                number,
                                to: first.id,
                                body: text.trim().to_owned(),
                            },
                        );
                        state.view.replying = None;
                        text.clear();
                    }
                    Some(_) => {
                        state.view.replying = None;
                        text.clear();
                    }
                    None => {}
                }
                state.view.reply_text = text;
            } else {
                ui.horizontal(|ui| {
                    if widgets::button(ui, ButtonKind::Inline, "reply", true, Some(Icon::Reply))
                        .clicked()
                    {
                        state.view.replying = Some(first.id);
                        state.view.reply_text.clear();
                    }
                    if let Some(t) = &t
                        && widgets::button(
                            ui,
                            ButtonKind::Inline,
                            "resolve",
                            true,
                            Some(Icon::Confirm),
                        )
                        .clicked()
                    {
                        state.change(
                            system,
                            "resolving…",
                            GithubRequest::Resolve {
                                thread: t.id.clone(),
                                resolve: true,
                            },
                        );
                    }
                });
            }
        },
    );
    if resolved && let Some(t) = &t {
        ui.horizontal(|ui| {
            ui.add_space(20.0);
            if widgets::button(ui, ButtonKind::Inline, "open it again", true, None).clicked() {
                state.change(
                    system,
                    "opening the thread…",
                    GithubRequest::Resolve {
                        thread: t.id.clone(),
                        resolve: false,
                    },
                );
            }
        });
    }
}

/// The conversation: the description, reviews, comments and threads, the
/// merge, a new comment.
fn conversation(ui: &mut Ui, state: &mut GithubState, system: &dyn System, d: &PullDetail) {
    let p = palette(ui.ctx());
    let pull = &d.pull;
    let n = pull.summary.number;
    card(
        ui,
        |ui| {
            who_line(
                ui,
                who(state, &pull.summary.author),
                &format!("{} ago", age(pull.created)),
            )
        },
        |ui| {
            let body = if pull.body.trim().is_empty() {
                "No description."
            } else {
                pull.body.as_str()
            };
            forms::paragraph(
                ui,
                body,
                12.5,
                19.0,
                if pull.body.trim().is_empty() {
                    p.ink_muted
                } else {
                    p.ink
                },
            );
        },
    );
    // The timeline: reviews and comments by time, a review's threads with it.
    enum Item<'a> {
        Review(&'a super::model::Review),
        Comment(&'a Comment),
    }
    let mut items: Vec<(i64, Item<'_>)> = Vec::new();
    for r in &d.reviews {
        items.push((r.submitted, Item::Review(r)));
    }
    for c in &d.comments {
        // (a reply shows in its thread)
        if c.on.as_ref().is_some_and(|o| o.reply_to.is_some()) {
            continue;
        }
        items.push((c.created, Item::Comment(c)));
    }
    items.sort_by_key(|(t, _)| *t);
    for (_, item) in items {
        match item {
            Item::Review(r) => {
                let (word, color) = match r.state.as_str() {
                    "APPROVED" => ("approved", p.success),
                    "CHANGES_REQUESTED" => ("asked for changes", p.error),
                    "DISMISSED" => ("review dismissed", p.ink_muted),
                    _ => ("reviewed", p.ink_muted),
                };
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    ui.label(
                        egui::RichText::new(who(state, &r.author))
                            .font(Face::PlexMedium.at(12.0))
                            .color(p.ink),
                    );
                    ui.label(
                        egui::RichText::new(word)
                            .font(Face::Plex.at(12.0))
                            .color(color),
                    );
                    ui.label(
                        egui::RichText::new(format!("· {} ago", age(r.submitted)))
                            .font(Face::Plex.at(11.5))
                            .color(p.ink_muted),
                    );
                });
                if !r.body.trim().is_empty() {
                    card(
                        ui,
                        |ui| who_line(ui, who(state, &r.author), "review"),
                        |ui| {
                            forms::paragraph(ui, &r.body, 12.5, 19.0, p.ink);
                        },
                    );
                }
            }
            Item::Comment(c) if c.on.is_some() => {
                let replies: Vec<&Comment> = d
                    .comments
                    .iter()
                    .filter(|r| r.on.as_ref().and_then(|o| o.reply_to) == Some(c.id))
                    .collect();
                thread(ui, state, system, n, c, &replies, d);
            }
            Item::Comment(c) => {
                card(
                    ui,
                    |ui| {
                        who_line(
                            ui,
                            who(state, &c.author),
                            &format!("{} ago", age(c.created)),
                        )
                    },
                    |ui| {
                        forms::paragraph(ui, &c.body, 12.5, 19.0, p.ink);
                    },
                );
            }
        }
    }
    // The merge.
    if pull.state == "open" {
        let (can, note) = match (
            pull.summary.draft,
            pull.mergeable,
            pull.mergeable_state.as_str(),
        ) {
            (true, _, _) => (
                false,
                "A draft can't merge: mark it ready for review on GitHub first.".to_owned(),
            ),
            (_, Some(false), _) | (_, _, "dirty") => (
                false,
                "It conflicts with the base: merge the base into the branch first.".to_owned(),
            ),
            (_, _, "blocked") => (
                false,
                "Blocked by the branch's rules: required reviews or checks.".to_owned(),
            ),
            (_, _, "behind") => (true, "The branch is behind the base.".to_owned()),
            (_, _, "unstable") => (true, "Some checks failed or are still running.".to_owned()),
            (_, None, _) => (
                false,
                "GitHub is still working out whether it can merge.".to_owned(),
            ),
            _ => (true, "Ready to merge.".to_owned()),
        };
        card(
            ui,
            |ui| {
                ui.label(
                    egui::RichText::new("merge")
                        .font(Face::PlexMedium.at(12.0))
                        .color(p.ink),
                );
            },
            |ui| {
                ui.horizontal(|ui| {
                    let names: Vec<(&str, bool)> = METHODS.iter().map(|(n, _)| (*n, can)).collect();
                    let mut m = state.view.method;
                    ui.add_enabled_ui(can, |ui| {
                        if forms::select_sized(
                            ui,
                            "github-merge-method",
                            &names,
                            &mut m,
                            170.0,
                            28.0,
                        ) {
                            state.view.method = m;
                        }
                    });
                    let word = METHODS[state.view.method].0;
                    let go = ui.add_enabled_ui(can && state.busy.is_none(), |ui| {
                        widgets::button(
                            ui,
                            ButtonKind::Primary,
                            word,
                            true,
                            Some(Icon::PullRequest),
                        )
                    });
                    if go.inner.clicked() {
                        state.change(
                            system,
                            "merging…",
                            GithubRequest::Merge {
                                number: n,
                                method: METHODS[state.view.method].1,
                            },
                        );
                    }
                });
                caption(ui, &note);
            },
        );
    } else if pull.merged {
        card(
            ui,
            |ui| {
                ui.label(
                    egui::RichText::new("merged")
                        .font(Face::PlexMedium.at(12.0))
                        .color(p.note),
                );
            },
            |ui| {
                caption(ui, &format!("{} can be deleted.", pull.summary.head));
            },
        );
    }
    // A new comment.
    card(
        ui,
        |ui| {
            ui.label(
                egui::RichText::new(format!("comment on #{n}"))
                    .font(Face::PlexMedium.at(12.0))
                    .color(p.ink),
            );
        },
        |ui| {
            let mut text = std::mem::take(&mut state.view.new_text);
            if composer(
                ui,
                "github-new-comment",
                &mut text,
                "a comment for everyone on the pull request",
                &[("comment", ButtonKind::Primary)],
            ) == Some(0)
            {
                state.change(
                    system,
                    "commenting…",
                    GithubRequest::Comment {
                        number: n,
                        body: text.trim().to_owned(),
                    },
                );
                text.clear();
            }
            state.view.new_text = text;
            caption(
                ui,
                "A comment on a line: press + on the line in the files tab.",
            );
        },
    );
}

/// The files: a list, then the picked one's diff with a comment on any
/// line of the new side.
fn files(ui: &mut Ui, state: &mut GithubState, system: &dyn System, d: &PullDetail) {
    let p = palette(ui.ctx());
    let n = d.pull.summary.number;
    ui.spacing_mut().item_spacing.y = 0.0;
    for (i, f) in d.files.iter().enumerate() {
        let comments = d
            .comments
            .iter()
            .filter(|c| c.on.as_ref().is_some_and(|o| o.path == f.path))
            .count();
        let note = format!(
            "+{} −{}{}",
            f.additions,
            f.deletions,
            if comments > 0 {
                format!(" · {comments} comments")
            } else {
                String::new()
            }
        );
        let change = match f.status.as_str() {
            "added" => crate::git::model::Change::Added,
            "removed" => crate::git::model::Change::Deleted,
            "renamed" => crate::git::model::Change::Renamed,
            _ => crate::git::model::Change::Modified,
        };
        let (click, _) =
            crate::git::page::file_row(ui, &f.path, &note, None, change, i == state.view.file);
        if click {
            state.view.file = i;
            state.view.draft = None;
        }
    }
    let Some(f) = d.files.get(state.view.file).cloned() else {
        return;
    };
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        ui.label(
            egui::RichText::new(&f.path)
                .font(Face::PlexMedium.at(12.5))
                .color(p.ink),
        );
        caption(
            ui,
            &format!("· {} ← {}", d.pull.summary.base, d.pull.summary.head),
        );
    });
    if f.binary {
        forms::padded(ui, 20, 10, |ui| {
            caption(
                ui,
                "No lines to show: a binary file, or too large for GitHub's diff.",
            )
        });
        return;
    }
    let pending: Vec<Pending> = state.pending.get(&n).cloned().unwrap_or_default();
    for (hi, h) in f.hunks.iter().enumerate() {
        hunk_head(ui, h, hi + 1, f.hunks.len(), None);
        for l in &h.lines {
            let (sign, ground) = match l.kind {
                LineKind::Context => (" ", Color32::TRANSPARENT),
                LineKind::Added => ("+", p.success_soft),
                LineKind::Removed => ("−", p.error_soft),
            };
            let number = if l.kind == LineKind::Removed {
                l.old
            } else {
                l.new
            };
            let top = ui.cursor().top();
            diff_row(ui, system, sign, number, &l.text, ground);
            // (a comment on a line of the new side: + at its right)
            if let Some(line) = l.new {
                let rect = Rect::from_min_size(
                    egui::pos2(ui.max_rect().right() - 28.0, top + 1.0),
                    Vec2::splat(18.0),
                );
                let r = ui.interact(
                    rect,
                    egui::Id::unique(("github-line", n, f.path.clone(), line)),
                    Sense::click(),
                );
                if r.hovered()
                    || state.view.draft.as_ref() == Some(&(f.path.clone(), u64::from(line)))
                {
                    ui.painter().rect_filled(rect, CornerRadius::ZERO, p.amber);
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        "+",
                        Face::PlexSemiBold.at(12.0),
                        p.on_amber,
                    );
                }
                if r.on_hover_text("comment on this line").clicked() {
                    state.view.draft = Some((f.path.clone(), u64::from(line)));
                    state.view.draft_text.clear();
                }
                // Threads on this line, pending comments, the draft.
                for c in d.comments.iter().filter(|c| {
                    c.on.as_ref().is_some_and(|o| {
                        o.path == f.path && o.line == Some(u64::from(line)) && o.reply_to.is_none()
                    })
                }) {
                    let replies: Vec<&Comment> = d
                        .comments
                        .iter()
                        .filter(|r| r.on.as_ref().and_then(|o| o.reply_to) == Some(c.id))
                        .collect();
                    thread(ui, state, system, n, c, &replies, d);
                }
                for pc in pending
                    .iter()
                    .filter(|pc| pc.path == f.path && pc.line == u64::from(line))
                {
                    card(
                        ui,
                        |ui| {
                            ui.label(
                                egui::RichText::new("pending · in your review")
                                    .font(Face::Plex.at(11.5))
                                    .color(p.amber_ink),
                            );
                        },
                        |ui| {
                            forms::paragraph(ui, &pc.body, 12.5, 19.0, p.ink);
                        },
                    );
                }
                if state.view.draft.as_ref() == Some(&(f.path.clone(), u64::from(line))) {
                    let mut text = std::mem::take(&mut state.view.draft_text);
                    let mut close = false;
                    card(
                        ui,
                        |ui| {
                            ui.label(
                                egui::RichText::new(format!("comment on {}:{line}", f.path))
                                    .font(Face::PlexMedium.at(12.0))
                                    .color(p.ink),
                            );
                        },
                        |ui| {
                            match composer(
                                ui,
                                &format!("draft-{}-{line}", f.path),
                                &mut text,
                                &format!("a comment on line {line}"),
                                &[
                                    ("comment", ButtonKind::Primary),
                                    ("add to your review", ButtonKind::Secondary),
                                    ("cancel", ButtonKind::Tertiary),
                                ],
                            ) {
                                Some(0) => {
                                    state.change(
                                        system,
                                        "commenting…",
                                        GithubRequest::LineComment {
                                            number: n,
                                            commit: d.pull.head_sha.clone(),
                                            path: f.path.clone(),
                                            line: u64::from(line),
                                            side: "RIGHT".into(),
                                            body: text.trim().to_owned(),
                                        },
                                    );
                                    close = true;
                                }
                                Some(1) => {
                                    state.pending.entry(n).or_default().push(Pending {
                                        path: f.path.clone(),
                                        line: u64::from(line),
                                        side: "RIGHT".into(),
                                        body: text.trim().to_owned(),
                                    });
                                    close = true;
                                }
                                Some(_) => close = true,
                                None => {}
                            }
                            caption(
                                ui,
                                "comment: posted now, for everyone · add to your review: kept as pending until you submit the review",
                            );
                        },
                    );
                    if close {
                        state.view.draft = None;
                        text.clear();
                    }
                    state.view.draft_text = text;
                }
            }
        }
    }
    forms::padded(ui, 20, 10, |ui| {
        caption(
            ui,
            &format!(
                "Point at a line and press + to comment on it. The diff is the pull request's: {} against {} as GitHub has them, not your working tree.",
                d.pull.summary.head, d.pull.summary.base
            ),
        );
    });
}

/// The review dialog: a summary, comment / approve / request changes, the
/// pending line comments.
fn review_dialog(
    ctx: &egui::Context,
    state: &mut GithubState,
    system: &dyn System,
    d: &PullDetail,
    mine: bool,
) {
    let p = palette(ctx);
    let n = d.pull.summary.number;
    let pending = state.pending.get(&n).cloned().unwrap_or_default();
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::unique("wowlua-studio-github-review")).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.spacing_mut().item_spacing.y = 10.0;
        ui.label(
            egui::RichText::new(format!("review #{n}"))
                .font(Face::PlexSemiBold.at(13.0))
                .color(p.ink),
        );
        caption(
            ui,
            &format!("{} · {}", d.pull.summary.title, d.pull.summary.head),
        );
        forms::label(ui, "summary");
        ui.add(
            egui::TextEdit::multiline(&mut state.view.review_text)
                .desired_rows(4)
                .desired_width(ui.available_width())
                .font(Face::Plex.at(12.5))
                .hint_text("what you think of the changes as a whole"),
        );
        let options = [
            ("comment", "Feedback without approving or blocking.", true),
            (
                "approve",
                if mine {
                    "Not on your own pull request."
                } else {
                    "The changes can merge."
                },
                !mine,
            ),
            (
                "request changes",
                if mine {
                    "Not on your own pull request."
                } else {
                    "The changes need work before they merge."
                },
                !mine,
            ),
        ];
        for (i, (title, why, ok)) in options.iter().enumerate() {
            if crate::git::page::option(ui, title, why, state.view.review_event == i, *ok) {
                state.view.review_event = i;
            }
        }
        if !pending.is_empty() {
            caption(
                ui,
                &format!("{} pending line comments go with it:", pending.len()),
            );
            for pc in &pending {
                caption(ui, &format!("{}:{} · {}", pc.path, pc.line, pc.body));
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            let event = [
                ReviewEvent::Comment,
                ReviewEvent::Approve,
                ReviewEvent::RequestChanges,
            ][state.view.review_event];
            let needs_text = event != ReviewEvent::Approve && pending.is_empty();
            let ready = !(needs_text && state.view.review_text.trim().is_empty());
            let go = ui.add_enabled_ui(ready, |ui| {
                widgets::button(
                    ui,
                    ButtonKind::Primary,
                    "submit the review",
                    true,
                    Some(Icon::Send),
                )
            });
            if go.inner.clicked() {
                state.change(
                    system,
                    "sending the review…",
                    GithubRequest::Review {
                        number: n,
                        event,
                        body: state.view.review_text.trim().to_owned(),
                        comments: pending.clone(),
                    },
                );
                state.pending.remove(&n);
                state.view.review_text.clear();
                close = true;
            }
            if widgets::button(ui, ButtonKind::Tertiary, "cancel", true, None).clicked() {
                close = true;
            }
        });
    });
    if close || modal.should_close() {
        state.view.reviewing = false;
    }
}

// ---------------------------------------------------------------- runs --

fn run_header(ui: &mut Ui, state: &mut GithubState, run: &Run) {
    let p = palette(ui.ctx());
    forms::padded(ui, 20, 12, |ui| {
        ui.horizontal(|ui| {
            let (mark, color) = run_mark(&run.status, run.conclusion.as_deref(), p);
            ui.label(
                egui::RichText::new(mark)
                    .font(Face::PlexSemiBold.at(15.0))
                    .color(color),
            );
            ui.label(
                egui::RichText::new(&run.title)
                    .font(Face::PlexSemiBold.at(15.0))
                    .color(p.ink),
            );
            ui.label(
                egui::RichText::new(format!("· {}", run.workflow))
                    .font(Face::Plex.at(13.0))
                    .color(p.ink_muted),
            );
            if let Some(&pr) = run.pulls.first() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::button(
                        ui,
                        ButtonKind::Secondary,
                        &format!("open pull request #{pr}"),
                        true,
                        Some(Icon::Open),
                    )
                    .clicked()
                    {
                        state.selected = Some(Selected::Pull(pr));
                        state.view.tab = Tab::Checks;
                    }
                });
            }
        });
        caption(
            ui,
            &format!(
                "{} · {} by {} · {} · {} ago",
                run.branch,
                run.event.replace('_', " "),
                who(state, &run.actor),
                run.head_sha.chars().take(7).collect::<String>(),
                age(run.created)
            ),
        );
    });
}

/// A run's card on a pull request's checks tab, with its jobs.
fn run_card(
    ui: &mut Ui,
    state: &mut GithubState,
    system: &dyn System,
    run: &Run,
) -> Option<Action> {
    let p = palette(ui.ctx());
    let (mark, color) = run_mark(&run.status, run.conclusion.as_deref(), p);
    ui.horizontal(|ui| {
        ui.add_space(20.0);
        ui.label(
            egui::RichText::new(mark)
                .font(Face::PlexSemiBold.at(13.0))
                .color(color),
        );
        ui.label(
            egui::RichText::new(&run.workflow)
                .font(Face::PlexMedium.at(12.5))
                .color(p.ink),
        );
        caption(ui, &format!("· {} · {} ago", run.title, age(run.created)));
        if widgets::button(
            ui,
            ButtonKind::Inline,
            "open the run",
            true,
            Some(Icon::Open),
        )
        .clicked()
        {
            state.selected = Some(Selected::Run(run.id));
        }
    });
    run_body(ui, state, system, run)
}

/// A run's controls, jobs, steps, the log of the step opened, the
/// annotations.
fn run_body(
    ui: &mut Ui,
    state: &mut GithubState,
    system: &dyn System,
    run: &Run,
) -> Option<Action> {
    let p = palette(ui.ctx());
    let mut action = None;
    state.want_jobs(system, run.id);
    ui.horizontal(|ui| {
        ui.add_space(20.0);
        let failed = run.conclusion.as_deref() == Some("failure");
        if run.running() {
            if widgets::button(ui, ButtonKind::Secondary, "cancel the run", true, Some(Icon::Stop)).clicked() {
                state.change(system, "cancelling…", GithubRequest::Cancel(run.id));
            }
            caption(ui, "watching · read every 15 s while it runs · a notification says when it finishes, on any page");
        } else {
            if failed
                && widgets::button(ui, ButtonKind::Secondary, "re-run failed jobs", true, Some(Icon::Refresh)).clicked()
            {
                state.change(system, "re-running…", GithubRequest::Rerun { run: run.id, failed_only: true });
            }
            if widgets::button(ui, ButtonKind::Tertiary, "re-run all jobs", true, Some(Icon::Refresh)).clicked() {
                state.change(system, "re-running…", GithubRequest::Rerun { run: run.id, failed_only: false });
            }
        }
        if widgets::button(ui, ButtonKind::Tertiary, "open on GitHub", true, Some(Icon::Open)).clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(&run.url));
        }
    });
    let Some(jobs) = state.jobs.get(&run.id).cloned() else {
        forms::padded(ui, 20, 6, |ui| caption(ui, "Reading the jobs…"));
        return None;
    };
    for job in &jobs {
        if let Some(a) = job_card(ui, state, system, job) {
            action = Some(a);
        }
    }
    // The annotations of the failed jobs, mapped to the user's files.
    let notes: Vec<super::model::Annotation> = jobs
        .iter()
        .filter_map(|j| state.logs.get(&j.id))
        .flat_map(|(_, a)| a.iter().cloned())
        .filter(|a| !a.path.is_empty() && a.path != ".github")
        .collect();
    if !notes.is_empty() {
        card(
            ui,
            |ui| {
                ui.label(
                    egui::RichText::new("annotations")
                        .font(Face::PlexMedium.at(12.0))
                        .color(p.ink),
                );
                caption(ui, "· from --message-format github, mapped to your files");
            },
            |ui| {
                for a in &notes {
                    ui.horizontal(|ui| {
                        let color = if a.level == "failure" {
                            p.error
                        } else {
                            p.warning
                        };
                        ui.label(
                            egui::RichText::new(if a.title.is_empty() {
                                &a.level
                            } else {
                                &a.title
                            })
                            .font(Face::PlexSemiBold.at(12.0))
                            .color(color),
                        );
                        ui.label(
                            egui::RichText::new(&a.message)
                                .font(Face::Plex.at(12.0))
                                .color(p.ink),
                        );
                        caption(ui, &format!("· {}:{}", a.path, a.line));
                        if widgets::button(ui, ButtonKind::Inline, "open", true, None).clicked() {
                            action = Some(Action::Open(a.path.clone(), a.line));
                        }
                    });
                }
            },
        );
    }
    action
}

fn job_card(
    ui: &mut Ui,
    state: &mut GithubState,
    system: &dyn System,
    job: &Job,
) -> Option<Action> {
    let p = palette(ui.ctx());
    let failed = job.conclusion.as_deref() == Some("failure");
    // (a failed job opens by itself, its log read)
    if failed && !state.view.open_jobs.contains(&job.id) && state.view.open_step.is_none() {
        state.view.open_jobs.insert(job.id);
        if let Some(s) = job
            .steps
            .iter()
            .find(|s| s.conclusion.as_deref() == Some("failure"))
        {
            state.view.open_step = Some((job.id, s.number));
        }
    }
    let open = state.view.open_jobs.contains(&job.id);
    let (mark, color) = run_mark(&job.status, job.conclusion.as_deref(), p);
    let took = job
        .completed
        .map(|c| format!("{} s", (c - job.started).max(0)))
        .unwrap_or_default();
    let done = job.steps.iter().filter(|s| s.status == "completed").count();
    let mut toggle = false;
    card_of(
        ui,
        |ui| {
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(format!(
                            "{} {mark} {}",
                            if open { "−" } else { "+" },
                            job.name
                        ))
                        .font(Face::PlexMedium.at(12.5))
                        .color(color),
                    )
                    .frame(false),
                )
                .clicked()
            {
                toggle = true;
            }
            caption(
                ui,
                &format!("· {done} of {} steps · {took}", job.steps.len()),
            );
        },
        open.then_some(|ui: &mut Ui| {
            for s in &job.steps {
                let (m, c) = run_mark(&s.status, s.conclusion.as_deref(), p);
                let took = match (s.started, s.completed) {
                    (Some(a), Some(b)) => format!("{} s", (b - a).max(0)),
                    _ => String::new(),
                };
                let (rect, r) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::click());
                if r.hovered() {
                    ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_300);
                }
                ui.painter().text(
                    egui::pos2(rect.left() + 8.0, rect.center().y),
                    Align2::CENTER_CENTER,
                    m,
                    Face::PlexSemiBold.at(12.0),
                    c,
                );
                ui.painter().text(
                    egui::pos2(rect.left() + 22.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    &s.name,
                    Face::Plex.at(12.5),
                    p.ink,
                );
                ui.painter().text(
                    egui::pos2(rect.right() - 6.0, rect.center().y),
                    Align2::RIGHT_CENTER,
                    took,
                    Face::Plex.at(11.5),
                    p.ink_muted,
                );
                if r.clicked() {
                    state.view.open_step = if state.view.open_step == Some((job.id, s.number)) {
                        None
                    } else {
                        Some((job.id, s.number))
                    };
                }
                if state.view.open_step == Some((job.id, s.number)) {
                    state.want_log(system, job.id);
                    match state.logs.get(&job.id) {
                        Some((log, _)) => {
                            let text = step_log(log, &s.name);
                            egui::Frame::new()
                                .fill(p.bg_000)
                                .stroke(Stroke::new(1.0, p.line))
                                .inner_margin(egui::Margin::symmetric(10, 8))
                                .show(ui, |ui| {
                                    egui::ScrollArea::vertical()
                                        .id_salt(("github-log", job.id, s.number))
                                        .max_height(320.0)
                                        .stick_to_bottom(true)
                                        .show(ui, |ui| {
                                            ui.label(
                                                egui::RichText::new(text)
                                                    .font(Face::Plex.at(11.5))
                                                    .color(p.ink_muted),
                                            );
                                        });
                                });
                        }
                        None => caption(ui, "Reading the log…"),
                    }
                }
            }
        }),
    );
    if toggle {
        if open {
            state.view.open_jobs.remove(&job.id);
        } else {
            state.view.open_jobs.insert(job.id);
        }
    }
    None
}

/// A step's part of a job's log: between its `##[group]Run …` and the
/// next step's; the whole log's end when it can't be told apart.
fn step_log(log: &str, step: &str) -> String {
    let lines: Vec<&str> = log.lines().collect();
    let name = step.trim_start_matches("Run ").trim();
    let start = lines
        .iter()
        .position(|l| l.starts_with("##[group]") && l.contains(name));
    match start {
        Some(s) => {
            let end = lines[s + 1..]
                .iter()
                .position(|l| l.starts_with("##[group]Run ") || l.starts_with("Post job cleanup"))
                .map_or(lines.len(), |e| s + 1 + e);
            lines[s..end].join("\n")
        }
        None => lines[lines.len().saturating_sub(200)..].join("\n"),
    }
}

// ------------------------------------------------- a new pull request --

fn new_pull(
    ui: &mut Ui,
    state: &mut GithubState,
    system: &dyn System,
    branch: Option<&str>,
) -> Option<Action> {
    let p = palette(ui.ctx());
    let mut action = None;
    if state.branches.is_empty() {
        state.want_branches(system);
    }
    let default = match &state.repo {
        Some(Ok((_, d, _, _))) => d.clone(),
        _ => "main".into(),
    };
    let base = state.view.base.clone().unwrap_or(default);
    // (the branch checked out, unless it is the base: then the first other)
    let head = state
        .view
        .head
        .clone()
        .or_else(|| branch.filter(|b| *b != base).map(str::to_owned))
        .or_else(|| state.branches.iter().find(|b| **b != base).cloned())
        .unwrap_or_default();
    let pushed = state.branches.contains(&head);
    let open = state.pull_of(&head).map(|q| q.number);
    let compare = if pushed && open.is_none() && head != base {
        state.want_compare(system, &base, &head);
        state.compares.get(&(base.clone(), head.clone())).cloned()
    } else {
        None
    };
    // (the title from the first commit, until it is changed)
    if let Some(Ok(c)) = &compare
        && state.view.title_from.as_ref() != Some(&(base.clone(), head.clone()))
    {
        state.view.title = c.commits.first().map(|c| c.1.clone()).unwrap_or_default();
        state.view.title_from = Some((base.clone(), head.clone()));
    }
    egui::ScrollArea::vertical().id_salt("github-new").auto_shrink(false).show(ui, |ui| {
        forms::padded(ui, 24, 18, |ui| {
            ui.label(egui::RichText::new("new pull request").font(Face::PlexSemiBold.at(15.0)).color(p.ink));
            caption(ui, "From a branch on origin. The title is its first commit's; change it as you like.");
            ui.add_space(10.0);
            let names: Vec<(&str, bool)> = state.branches.iter().map(|b| (b.as_str(), true)).collect();
            ui.horizontal(|ui| {
                forms::label(ui, "base");
                let mut bi = state.branches.iter().position(|b| *b == base).unwrap_or(0);
                if forms::select(ui, "github-new-base", &names, &mut bi, 200.0) {
                    state.view.base = state.branches.get(bi).cloned();
                }
                ui.label(egui::RichText::new("←").font(Face::Plex.at(14.0)).color(p.ink_muted));
                forms::label(ui, "head");
                let mut hi = state.branches.iter().position(|b| *b == head).unwrap_or(0);
                let mut head_names = names.clone();
                if !pushed && !head.is_empty() {
                    head_names.push((head.as_str(), true));
                    hi = head_names.len() - 1;
                }
                if forms::select(ui, "github-new-head", &head_names, &mut hi, 240.0) {
                    state.view.head = head_names.get(hi).map(|(h, _)| (*h).to_owned());
                }
            });
            ui.add_space(8.0);
            if let Some(n) = open {
                forms::callout(ui, CalloutKind::Note, |ui| {
                    forms::paragraph(ui, &format!("already open: #{n} is the pull request of {head}. A branch has one open pull request at a time."), 12.5, 19.0, p.ink);
                    if widgets::button(ui, ButtonKind::Secondary, &format!("open #{n}"), true, Some(Icon::Open)).clicked() {
                        state.selected = Some(Selected::Pull(n));
                    }
                });
                return;
            }
            if !pushed {
                forms::callout(ui, CalloutKind::Note, |ui| {
                    forms::paragraph(ui, &format!("not on origin: {head} has never been pushed, so GitHub doesn't know it. Push it from the git page, then come back."), 12.5, 19.0, p.ink);
                    if widgets::button(ui, ButtonKind::Secondary, "open the git page", true, None).clicked() {
                        action = Some(Action::ShowGit);
                    }
                });
                return;
            }
            if head == base {
                forms::callout(ui, CalloutKind::Note, |ui| {
                    forms::paragraph(ui, &format!("the same branch: {head} is the base too. Pick the branch with the changes as the head."), 12.5, 19.0, p.ink);
                });
                return;
            }
            forms::label(ui, "title");
            forms::text_input(ui, &mut state.view.title, ui.available_width().min(720.0), "what the pull request does");
            ui.add_space(6.0);
            forms::label(ui, "description");
            ui.add(
                egui::TextEdit::multiline(&mut state.view.body)
                    .desired_rows(6)
                    .desired_width(ui.available_width().min(720.0))
                    .font(Face::Plex.at(12.5))
                    .hint_text("what changes, and how to try it"),
            );
            forms::checkbox(ui, &mut state.view.draft_pull, "a draft", Some("not ready for review, and it can't merge yet; the checks run either way"));
            match &compare {
                Some(Ok(c)) if c.commits.is_empty() => {
                    caption(ui, &format!("Nothing to merge: {head} has no commits that {base} doesn't."));
                }
                Some(Ok(c)) => {
                    section(ui, &format!("{} commits", c.commits.len()), c.commits.len(), p.ink_muted, None);
                    for (hash, msg, when) in &c.commits {
                        caption(ui, &format!("{hash}  {msg} · {} ago", age(*when)));
                    }
                    section(ui, &format!("{} files", c.files.len()), c.files.len(), p.ink_muted, None);
                    for f in &c.files {
                        caption(ui, &format!("{}  +{} −{}", f.path, f.additions, f.deletions));
                    }
                }
                Some(Err(e)) => caption(ui, &e.to_string()),
                None => caption(ui, "Reading what the branch brings…"),
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let word = if state.view.draft_pull { "create the draft" } else { "create the pull request" };
                let ready = !state.view.title.trim().is_empty() && matches!(&compare, Some(Ok(c)) if !c.commits.is_empty()) && state.busy.is_none();
                if ui.add_enabled_ui(ready, |ui| widgets::button(ui, ButtonKind::Primary, word, true, Some(Icon::PullRequest))).inner.clicked() {
                    state.change(system, "creating the pull request…", GithubRequest::Create {
                        title: state.view.title.trim().to_owned(),
                        body: state.view.body.trim().to_owned(),
                        head: head.clone(),
                        base: base.clone(),
                        draft: state.view.draft_pull,
                    });
                }
                if widgets::button(ui, ButtonKind::Tertiary, "cancel", true, None).clicked() {
                    state.selected = None;
                }
            });
        });
    });
    action
}
