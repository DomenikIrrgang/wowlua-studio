//! The claude page (M14): Claude Code as Studio found it and signing in
//! through it (S090, S095), and the conversation (S097): your messages,
//! Claude's text as it streams, each tool call with its answer, and what
//! Studio's tools ask first (S096), answered in place; the tools panel
//! holds the conversation's permissions.

use std::collections::BTreeMap;

use egui::{ScrollArea, Ui};
use serde::{Deserialize, Serialize};
use wowlua_studio_ui::widgets::{self, ButtonKind};
use wowlua_studio_ui::{Face, Palette, forms, palette};

use crate::settings::SettingsState;
use crate::state::{CLAUDE_PERMISSIONS, CLAUDE_TOOLS, Settings};
use crate::system::{ClaudeChanges, ClaudeCode, ClaudeReply, ConversationEvent, System};

/// What the page asks the app to do.
pub enum Action {
    /// Runs Claude Code's sign-in in the terminal: this line.
    SignIn(String),
    /// The user's message: starts Claude Code, or goes to it.
    Send(String),
    /// Ends Claude Code's turn (the conversation goes on).
    Stop,
    /// A new conversation.
    New,
    /// Open a kept conversation (S101).
    Open(u64),
    /// Forget a kept conversation.
    Forget(u64),
    /// Stage Claude's changed files and commit them on the git page.
    Commit(Vec<String>),
}

/// One part of the conversation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Entry {
    You(String),
    /// Claude's text (`streaming`: more to come).
    Claude {
        text: String,
        streaming: bool,
    },
    /// A tool call and its answer (its text, whether it failed, whether it
    /// held an image).
    Tool {
        id: String,
        name: String,
        input: String,
        answer: Option<(String, bool, bool)>,
    },
    /// What a tool asks first; the user's answer once given.
    Ask {
        id: u64,
        group: String,
        what: String,
        answer: Option<bool>,
    },
    /// What Studio tells: an error, the end of Claude Code.
    Note(String),
}

/// The conversation on the page.
#[derive(Default)]
pub struct Conversation {
    /// When it started (seconds since the epoch): its key among the kept
    /// ones (S101).
    pub id: u64,
    /// Its model and effort (from the settings at its start; changed in
    /// its footer).
    pub model_choice: Option<String>,
    pub effort_choice: Option<String>,
    pub entries: Vec<Entry>,
    pub draft: String,
    /// Claude Code runs (its process).
    pub running: bool,
    /// A turn is under way.
    pub busy: bool,
    /// Claude Code's session, to go on with.
    pub session: Option<String>,
    pub model: Option<String>,
    /// The conversation's cost so far, as Claude Code estimates it.
    pub cost: f64,
    /// The plan's limit: reached until (seconds since the epoch).
    pub limited: Option<Option<u64>>,
    pub used: Option<f64>,
    /// Each tool group's permission in this conversation (from the
    /// settings at its start).
    pub permissions: BTreeMap<String, String>,
    /// What Claude changed (S098), as last read; read again when stale
    /// (an edit tool answered, a hunk undone).
    pub changes: Option<ClaudeChanges>,
    pub stale: bool,
    /// The file the changes panel shows.
    pub file: usize,
    /// What Claude is told with the next message: the files went back to
    /// a checkpoint.
    pub notice: Option<String>,
    /// What goes with the next message besides the open file (S099).
    pub chips: Vec<Chip>,
}

/// Context the user adds to a message (`@` chips, S099).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Chip {
    /// The open file's whole text (its name and problems always go).
    OpenFile,
    /// The tests that failed in the last run, with their messages.
    FailingTests,
    /// Where the debugger stopped.
    Stop,
    /// What another page attached (S100): its label and text.
    Text { label: String, text: String },
}

impl Chip {
    pub const ADDABLE: [Chip; 3] = [Chip::OpenFile, Chip::FailingTests, Chip::Stop];

    pub fn label(&self) -> &str {
        match self {
            Chip::OpenFile => "the open file",
            Chip::FailingTests => "the failing tests",
            Chip::Stop => "the debugger's stop",
            Chip::Text { label, .. } => label,
        }
    }
}

/// "Ask Claude" from another page (S100): the message it starts with, and
/// what the user looks at, attached as a chip.
#[derive(Clone, Debug, PartialEq)]
pub struct Prompt {
    pub draft: String,
    pub label: String,
    pub context: String,
}

/// A conversation kept in Studio's data (S101).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Kept {
    pub id: u64,
    pub title: String,
    pub session: Option<String>,
    pub entries: Vec<Entry>,
    pub permissions: BTreeMap<String, String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub cost: f64,
    /// When it was last kept.
    pub at: u64,
}

impl Conversation {
    /// What is kept of it (none while empty).
    pub fn kept(&self, now: u64) -> Option<Kept> {
        let title = self.entries.iter().find_map(|e| match e {
            Entry::You(t) => Some(short(t, 80)),
            _ => None,
        })?;
        Some(Kept {
            id: self.id,
            title,
            session: self.session.clone(),
            entries: self.entries.clone(),
            permissions: self.permissions.clone(),
            model: self.model_choice.clone(),
            effort: self.effort_choice.clone(),
            cost: self.cost,
            at: now,
        })
    }

    /// A kept conversation, to go on with.
    pub fn from_kept(k: Kept) -> Conversation {
        let mut entries = k.entries;
        // (questions left open then cannot be answered now)
        for e in &mut entries {
            if let Entry::Ask { answer, .. } = e
                && answer.is_none()
            {
                *answer = Some(false);
            }
        }
        Conversation {
            id: k.id,
            model_choice: k.model,
            effort_choice: k.effort,
            entries,
            session: k.session,
            cost: k.cost,
            permissions: k.permissions,
            ..Conversation::default()
        }
    }

    /// The permissions, model and effort it starts with: the settings'.
    pub fn begin(&mut self, settings: &Settings, now: u64) {
        if self.id == 0 {
            self.id = now;
        }
        if self.model_choice.is_none() {
            self.model_choice = Some(settings.claude_model.clone());
        }
        if self.effort_choice.is_none() {
            self.effort_choice = Some(settings.claude_effort.clone());
        }
        if self.permissions.is_empty() {
            self.permissions = CLAUDE_TOOLS
                .iter()
                .map(|t| (t.0.to_owned(), settings.claude_permission(t.0)))
                .collect();
        }
    }

    pub fn event(&mut self, event: ConversationEvent) {
        match event {
            ConversationEvent::Started { session, model } => {
                self.session = Some(session);
                self.model = Some(model);
            }
            ConversationEvent::TextDelta(t) => match self.entries.last_mut() {
                Some(Entry::Claude {
                    text,
                    streaming: true,
                }) => text.push_str(&t),
                _ => self.entries.push(Entry::Claude {
                    text: t,
                    streaming: true,
                }),
            },
            ConversationEvent::Text(t) => match self.entries.last_mut() {
                Some(Entry::Claude { text, streaming }) if *streaming => {
                    *text = t;
                    *streaming = false;
                }
                _ => self.entries.push(Entry::Claude {
                    text: t,
                    streaming: false,
                }),
            },
            ConversationEvent::ToolUse { id, name, input } => {
                self.entries.push(Entry::Tool {
                    id,
                    name,
                    input,
                    answer: None,
                });
            }
            ConversationEvent::ToolResult {
                id,
                text,
                error,
                image,
            } => {
                for e in self.entries.iter_mut().rev() {
                    if let Entry::Tool {
                        id: i,
                        name,
                        answer,
                        ..
                    } = e
                        && *i == id
                    {
                        // (Claude's changes are read again after an edit)
                        if matches!(
                            name.as_str(),
                            "write_file" | "edit_file" | "delete_file" | "fmt"
                        ) {
                            self.stale = true;
                        }
                        *answer = Some((text, error, image));
                        break;
                    }
                }
            }
            ConversationEvent::TurnDone {
                cost,
                error,
                denied,
            } => {
                self.busy = false;
                self.cost = cost;
                if let Some(e) = error {
                    self.entries.push(Entry::Note(e));
                }
                if denied > 0 {
                    self.entries
                        .push(Entry::Note(format!("Claude Code denied {denied} calls")));
                }
            }
            ConversationEvent::Limit {
                rejected,
                resets_at,
                utilization,
            } => {
                self.limited = rejected.then_some(resets_at);
                self.used = utilization;
            }
            ConversationEvent::Ended(error) => {
                self.running = false;
                self.busy = false;
                if let Some(e) = error {
                    self.entries
                        .push(Entry::Note(format!("Claude Code ended: {e}")));
                }
            }
        }
    }

    /// A question of Studio's tools.
    pub fn ask(&mut self, id: u64, group: String, what: String) {
        self.entries.push(Entry::Ask {
            id,
            group,
            what,
            answer: None,
        });
    }

    /// A question was answered: `always` makes the group allowed for the
    /// rest of the conversation.
    fn answer(&mut self, id: u64, allowed: bool, always: bool, system: &dyn System) {
        for e in &mut self.entries {
            if let Entry::Ask {
                id: i,
                group,
                answer,
                ..
            } = e
                && *i == id
            {
                *answer = Some(allowed);
                if always {
                    self.permissions.insert(group.clone(), "allow".into());
                    system.claude_permission(group, "allow");
                }
            }
        }
        system.claude_reply(id, ClaudeReply::Allowed(allowed));
    }

    /// Questions still open (a new conversation answers them no).
    pub fn open_asks(&self) -> Vec<u64> {
        self.entries
            .iter()
            .filter_map(|e| match e {
                Entry::Ask {
                    id, answer: None, ..
                } => Some(*id),
                _ => None,
            })
            .collect()
    }
}

/// The conversation's panel: Claude Code's state until it is ready, then
/// the conversation (`waiting`: a sign-in runs in the terminal).
pub fn chat(
    ui: &mut Ui,
    settings: &mut SettingsState,
    system: &dyn System,
    waiting: bool,
    c: &mut Conversation,
) -> Option<Action> {
    let p = palette(ui.ctx());
    settings.read(system);
    let claude = settings.claude().cloned();
    let ready = claude
        .as_ref()
        .is_some_and(|c| c.path.is_some() && c.account.is_some());
    if !ready {
        return not_ready(ui, settings, claude.as_ref(), waiting, p);
    }
    let mut action = None;
    let input_h = 146.0;
    let rect = ui.max_rect();
    let talk = egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.max.y - input_h));
    ui.scope_builder(egui::UiBuilder::new().max_rect(talk), |ui| {
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                forms::padded(ui, 24, 18, |ui| {
                    ui.set_max_width(760.0);
                    if c.entries.is_empty() {
                        forms::paragraph(
                            ui,
                            "Ask Claude to build, fix, test or explain. It works through Studio's tools: the workspace's files, git, and wowlua's checks, tests, game and debugger. What it may do without asking is in the tools panel.",
                            12.5,
                            19.0,
                            p.ink_muted,
                        );
                    }
                    let mut answered = None;
                    for e in &c.entries {
                        entry(ui, e, p, &mut answered);
                        ui.add_space(10.0);
                    }
                    if let Some((id, allowed, always)) = answered {
                        c.answer(id, allowed, always, system);
                    }
                    if c.busy {
                        forms::paragraph(ui, "Claude is working…", 11.5, 17.0, p.ink_faint);
                    }
                });
            });
    });
    // The input: the message, send, stop, a new conversation.
    let bottom = egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.max.y - input_h), rect.max);
    ui.scope_builder(egui::UiBuilder::new().max_rect(bottom), |ui| {
        ui.painter().line_segment(
            [bottom.left_top(), bottom.right_top()],
            egui::Stroke::new(1.0, p.line),
        );
        forms::padded(ui, 24, 10, |ui| {
            let limited = c.limited.is_some();
            // the chips: what goes with the message
            ui.horizontal_wrapped(|ui| {
                let mut gone = None;
                for (i, chip) in c.chips.iter().enumerate() {
                    if ui
                        .button(
                            egui::RichText::new(format!("@ {} ×", chip.label()))
                                .font(Face::Plex.at(11.5)),
                        )
                        .on_hover_text("leave it out")
                        .clicked()
                    {
                        gone = Some(i);
                    }
                }
                if let Some(i) = gone {
                    c.chips.remove(i);
                }
                ui.menu_button(
                    egui::RichText::new("+ add context").font(Face::Plex.at(11.5)),
                    |ui| {
                        for chip in Chip::ADDABLE {
                            if !c.chips.contains(&chip) && ui.button(chip.label()).clicked() {
                                c.chips.push(chip);
                                ui.close();
                            }
                        }
                    },
                );
                ui.label(
                    egui::RichText::new("the open file's name and problems always go")
                        .font(Face::Plex.at(11.0))
                        .color(p.ink_faint),
                );
            });
            ui.add(
                egui::TextEdit::multiline(&mut c.draft)
                    .desired_rows(3)
                    .desired_width(ui.available_width())
                    .font(Face::Plex.at(12.5))
                    .hint_text("Ask Claude to build, fix, test or explain…"),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let can = !c.busy && !limited && !c.draft.trim().is_empty();
                if widgets::button(ui, ButtonKind::Primary, "send", can, None).clicked() {
                    action = Some(Action::Send(std::mem::take(&mut c.draft).trim().to_owned()));
                }
                if c.busy
                    && widgets::button(ui, ButtonKind::Secondary, "stop", true, None).clicked()
                {
                    action = Some(Action::Stop);
                }
                if widgets::button(
                    ui,
                    ButtonKind::Tertiary,
                    "new conversation",
                    !c.entries.is_empty(),
                    None,
                )
                .clicked()
                {
                    action = Some(Action::New);
                }
                // this conversation's model and effort (S101)
                let mut model = c.model_choice.clone().unwrap_or_else(|| "fable".into());
                let models = ["fable", "opus", "sonnet", "haiku"];
                let mut at = models.iter().position(|m| *m == model).unwrap_or(0);
                let opts: Vec<(&str, bool)> = models.iter().map(|m| (*m, true)).collect();
                if forms::select(ui, "claude-model", &opts, &mut at, 110.0) {
                    model = models[at].to_owned();
                    c.model_choice = Some(model);
                    if c.running && !c.busy {
                        // (the next message goes on with it)
                        system.claude_stop();
                        c.running = false;
                    }
                }
                let efforts = [
                    crate::state::CLAUDE_EFFORT_DEFAULT,
                    "low",
                    "medium",
                    "high",
                    "xhigh",
                    "max",
                ];
                let effort = c
                    .effort_choice
                    .clone()
                    .unwrap_or_else(|| crate::state::CLAUDE_EFFORT_DEFAULT.into());
                let mut at = efforts.iter().position(|e| *e == effort).unwrap_or(0);
                let opts: Vec<(&str, bool)> = efforts.iter().map(|e| (*e, true)).collect();
                if forms::select(ui, "claude-effort", &opts, &mut at, 190.0) {
                    c.effort_choice = Some(efforts[at].to_owned());
                    if c.running && !c.busy {
                        system.claude_stop();
                        c.running = false;
                    }
                }
                let mut state = Vec::new();
                if let Some(m) = &c.model {
                    state.push(m.clone());
                }
                if c.cost > 0.0 {
                    state.push(format!("≈ ${:.2} so far", c.cost));
                }
                if let Some(u) = c.used {
                    state.push(format!("plan {:.0}% used", u * 100.0));
                }
                if let Some(reset) = c.limited {
                    state.push(match reset {
                        Some(at) => {
                            format!("paused: the plan's limit is reached until {}", clock(at))
                        }
                        None => "paused: the plan's limit is reached".into(),
                    });
                }
                ui.label(
                    egui::RichText::new(state.join(" · "))
                        .font(Face::Plex.at(11.0))
                        .color(p.ink_faint),
                );
            });
        });
    });
    action
}

/// One entry of the conversation; a question's buttons set `answered`.
fn entry(ui: &mut Ui, e: &Entry, p: &Palette, answered: &mut Option<(u64, bool, bool)>) {
    match e {
        Entry::You(t) => {
            egui::Frame::new()
                .fill(p.bg_300)
                .stroke(egui::Stroke::new(1.0, p.line))
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| forms::paragraph(ui, t, 12.5, 19.0, p.ink));
        }
        Entry::Claude { text, .. } => {
            forms::paragraph(ui, text, 12.5, 19.0, p.ink);
        }
        Entry::Tool {
            name,
            input,
            answer,
            ..
        } => {
            let (mark, color) = match answer {
                None => ("…", p.ink_faint),
                Some((_, true, _)) => ("✕", p.error),
                Some(_) => ("✓", p.success),
            };
            egui::Frame::new()
                .fill(p.bg_200)
                .stroke(egui::Stroke::new(1.0, p.line))
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            egui::RichText::new(mark)
                                .font(Face::PlexMedium.at(12.0))
                                .color(color),
                        );
                        ui.label(
                            egui::RichText::new(name)
                                .font(Face::PlexMedium.at(12.0))
                                .color(p.ink),
                        );
                        ui.label(
                            egui::RichText::new(short(input, 120))
                                .font(Face::Plex.at(11.5))
                                .color(p.ink_muted),
                        );
                    });
                    if let Some((text, failed, image)) = answer {
                        let mut first = short(text.lines().next().unwrap_or(""), 160);
                        if *image {
                            first = format!("[image] {first}");
                        }
                        if !first.is_empty() {
                            ui.label(
                                egui::RichText::new(first)
                                    .font(Face::Plex.at(11.5))
                                    .color(if *failed { p.error } else { p.ink_faint }),
                            );
                        }
                    }
                });
        }
        Entry::Ask {
            id,
            group,
            what,
            answer,
        } => {
            egui::Frame::new()
                .fill(p.bg_200)
                .stroke(egui::Stroke::new(1.0, p.amber_ink))
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    forms::paragraph(ui, &format!("Claude asks: {what}"), 12.5, 19.0, p.ink);
                    match answer {
                        Some(true) => {
                            forms::paragraph(ui, "allowed", 11.5, 17.0, p.success);
                        }
                        Some(false) => {
                            forms::paragraph(ui, "denied", 11.5, 17.0, p.error);
                        }
                        None => {
                            ui.horizontal_wrapped(|ui| {
                                if widgets::button(ui, ButtonKind::Primary, "allow", true, None)
                                    .clicked()
                                {
                                    *answered = Some((*id, true, false));
                                }
                                let always =
                                    format!("allow {} in this conversation", group_label(group));
                                if widgets::button(ui, ButtonKind::Secondary, &always, true, None)
                                    .clicked()
                                {
                                    *answered = Some((*id, true, true));
                                }
                                if widgets::button(ui, ButtonKind::Tertiary, "deny", true, None)
                                    .clicked()
                                {
                                    *answered = Some((*id, false, false));
                                }
                            });
                        }
                    }
                });
        }
        Entry::Note(t) => {
            forms::paragraph(ui, t, 11.5, 17.0, p.warning);
        }
    }
}

/// The changes panel (S098): the checkpoints, each file Claude changed
/// with its hunks against how it was, undo and redo per hunk, undo all,
/// commit.
pub fn changes(ui: &mut Ui, c: &mut Conversation, system: &dyn System) -> Option<Action> {
    let p = palette(ui.ctx());
    if c.stale || c.changes.is_none() {
        c.changes = system.claude_changes();
        c.stale = false;
    }
    let Some(view) = c.changes.clone() else {
        forms::padded(ui, 14, 12, |ui| {
            forms::paragraph(
                ui,
                "Claude has changed nothing yet.",
                12.5,
                19.0,
                p.ink_muted,
            );
        });
        return None;
    };
    let mut action = None;
    let mut redo_all = false;
    let mut problem: Option<String> = None;
    let mut back: Option<usize> = None;
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            forms::padded(ui, 14, 12, |ui| {
                let (added, removed): (usize, usize) = view
                    .files
                    .iter()
                    .fold((0, 0), |(a, r), f| (a + f.added, r + f.removed));
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new(if view.files.is_empty() {
                            "no changes".to_string()
                        } else {
                            format!("{} files · +{added} −{removed}", view.files.len())
                        })
                        .font(Face::PlexMedium.at(12.5))
                        .color(p.ink),
                    );
                    let any = !view.files.is_empty();
                    if widgets::button(
                        ui,
                        ButtonKind::Secondary,
                        "undo all of Claude's changes",
                        any,
                        None,
                    )
                    .clicked()
                    {
                        back = Some(0);
                    }
                    if widgets::button(ui, ButtonKind::Primary, "commit these changes", any, None)
                        .clicked()
                    {
                        action = Some(Action::Commit(
                            view.files.iter().map(|f| f.path.clone()).collect(),
                        ));
                    }
                });
                ui.add_space(10.0);
                if !view.checkpoints.is_empty() {
                    ui.label(
                        egui::RichText::new("checkpoints")
                            .font(Face::PlexMedium.at(12.0))
                            .color(p.ink_muted),
                    );
                    for (k, cp) in view.checkpoints.iter().enumerate() {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "before \"{}\" · then {} files",
                                    short(&cp.label, 60),
                                    cp.files
                                ))
                                .font(Face::Plex.at(12.0))
                                .color(p.ink),
                            );
                            if cp.files > 0
                                && widgets::button(
                                    ui,
                                    ButtonKind::Tertiary,
                                    "go back to here",
                                    true,
                                    None,
                                )
                                .clicked()
                            {
                                back = Some(k);
                            }
                        });
                    }
                    ui.add_space(10.0);
                }
                for (i, f) in view.files.iter().enumerate() {
                    let what = if f.created {
                        " · made by Claude"
                    } else if f.deleted {
                        " · deleted"
                    } else {
                        ""
                    };
                    let shown = c.file == i;
                    if ui
                        .selectable_label(
                            shown,
                            egui::RichText::new(format!(
                                "{}  +{} −{}{what}",
                                f.path, f.added, f.removed
                            ))
                            .font(Face::PlexMedium.at(12.0)),
                        )
                        .clicked()
                    {
                        c.file = i;
                    }
                    if !shown {
                        continue;
                    }
                    for h in &f.hunks {
                        egui::Frame::new()
                            .fill(p.bg_200)
                            .stroke(egui::Stroke::new(1.0, p.line))
                            .inner_margin(egui::Margin::symmetric(8, 6))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!("line {}", h.new_start))
                                            .font(Face::Plex.at(11.0))
                                            .color(p.ink_faint),
                                    );
                                    if widgets::button(
                                        ui,
                                        ButtonKind::Tertiary,
                                        "undo this hunk",
                                        true,
                                        None,
                                    )
                                    .clicked()
                                    {
                                        if let Err(e) = system.claude_undo(&f.path, h) {
                                            problem = Some(e);
                                        }
                                        c.stale = true;
                                    }
                                });
                                for l in &h.old {
                                    ui.label(
                                        egui::RichText::new(format!("− {l}"))
                                            .font(Face::Plex.at(11.5))
                                            .color(p.error),
                                    );
                                }
                                for l in &h.new {
                                    ui.label(
                                        egui::RichText::new(format!("+ {l}"))
                                            .font(Face::Plex.at(11.5))
                                            .color(p.success),
                                    );
                                }
                            });
                        ui.add_space(6.0);
                    }
                }
                if !view.undone.is_empty() {
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new("undone")
                            .font(Face::PlexMedium.at(12.0))
                            .color(p.ink_muted),
                    );
                    for (i, (path, h)) in view.undone.iter().enumerate() {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{path}: line {}, {} lines",
                                    h.old_start,
                                    h.new.len()
                                ))
                                .font(Face::Plex.at(12.0))
                                .color(p.ink),
                            );
                            if widgets::button(ui, ButtonKind::Tertiary, "redo", true, None)
                                .clicked()
                                && !redo_all
                            {
                                if let Err(e) = system.claude_redo(i) {
                                    problem = Some(e);
                                }
                                redo_all = true;
                                c.stale = true;
                            }
                        });
                    }
                }
                if let Some(e) = &problem {
                    forms::paragraph(ui, e, 11.5, 17.0, p.warning);
                }
            });
        });
    if let Some(k) = back {
        let label = view.checkpoints.get(k).map(|cp| cp.label.clone());
        match system.claude_back_to(k) {
            Ok(kept) => {
                let to = match &label {
                    Some(l) if k > 0 => format!("before \"{}\"", short(l, 60)),
                    _ => "the conversation's start".into(),
                };
                let mut note = format!("The files are back to {to}.");
                if !kept.is_empty() {
                    note.push_str(&format!(
                        " Kept, as you changed them after Claude: {}.",
                        kept.join(", ")
                    ));
                }
                c.entries.push(Entry::Note(note.clone()));
                c.notice = Some(format!(
                    "(Studio: the user went back: {note} Read the files again before changing them.)"
                ));
            }
            Err(e) => c.entries.push(Entry::Note(e)),
        }
        c.stale = true;
    }
    action
}

/// The conversations panel (S101): a new one, and the kept ones of the
/// workspace, newest first.
pub fn conversations(ui: &mut Ui, kept: &[Kept], current: u64, busy: bool) -> Option<Action> {
    let p = palette(ui.ctx());
    let mut action = None;
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            forms::padded(ui, 10, 10, |ui| {
                if widgets::button(ui, ButtonKind::Secondary, "new conversation", !busy, None)
                    .clicked()
                {
                    action = Some(Action::New);
                }
                ui.add_space(8.0);
                if kept.is_empty() {
                    forms::paragraph(
                        ui,
                        "Your conversations in this workspace are kept here.",
                        11.5,
                        17.0,
                        p.ink_muted,
                    );
                }
                let mut list: Vec<&Kept> = kept.iter().collect();
                list.sort_by_key(|k| std::cmp::Reverse(k.at));
                for k in list {
                    let here = k.id == current;
                    let turns = k
                        .entries
                        .iter()
                        .filter(|e| matches!(e, Entry::You(_)))
                        .count();
                    let resp = ui.selectable_label(
                        here,
                        egui::RichText::new(&k.title).font(Face::Plex.at(12.0)),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "{turns} message{} · ≈ ${:.2}",
                            if turns == 1 { "" } else { "s" },
                            k.cost
                        ))
                        .font(Face::Plex.at(11.0))
                        .color(p.ink_faint),
                    );
                    if resp.clicked() && !here && !busy {
                        action = Some(Action::Open(k.id));
                    }
                    resp.context_menu(|ui| {
                        if ui.button("forget it").clicked() {
                            action = Some(Action::Forget(k.id));
                            ui.close();
                        }
                    });
                    ui.add_space(6.0);
                }
                if busy {
                    forms::paragraph(
                        ui,
                        "Claude is working: another conversation opens after.",
                        11.0,
                        16.0,
                        p.ink_faint,
                    );
                }
            });
        });
    action
}

/// The tools panel: this conversation's permission per tool group.
pub fn tools(ui: &mut Ui, c: &mut Conversation, settings: &Settings, system: &dyn System) {
    let p = palette(ui.ctx());
    c.begin(settings, system.now());
    ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        forms::padded(ui, 14, 12, |ui| {
            forms::paragraph(
                ui,
                "What Claude may do in this conversation. A new conversation starts from the settings. Publishing and your WoW folder stay yours.",
                11.5,
                17.0,
                p.ink_muted,
            );
            ui.add_space(8.0);
            for (key, label, _, _) in CLAUDE_TOOLS {
                ui.label(egui::RichText::new(*label).font(Face::Plex.at(12.0)).color(p.ink));
                let now = c.permissions.get(*key).cloned().unwrap_or_else(|| "ask".into());
                let at = CLAUDE_PERMISSIONS.iter().position(|o| *o == now).unwrap_or(1);
                if let Some(i) = widgets::seg(ui, &format!("claude-{key}"), CLAUDE_PERMISSIONS, at, true) {
                    c.permissions.insert((*key).to_owned(), CLAUDE_PERMISSIONS[i].to_owned());
                    system.claude_permission(key, CLAUDE_PERMISSIONS[i]);
                }
                ui.add_space(6.0);
            }
        });
    });
}

/// A tool group's label (its key's, S095).
fn group_label(group: &str) -> &str {
    CLAUDE_TOOLS
        .iter()
        .find(|t| t.0 == group)
        .map_or(group, |t| t.1)
}

/// At most `n` characters, on one line.
fn short(s: &str, n: usize) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= n {
        one
    } else {
        format!("{}…", one.chars().take(n).collect::<String>())
    }
}

/// A time of day (UTC) from seconds since the epoch.
fn clock(at: u64) -> String {
    let s = at % 86_400;
    format!("{:02}:{:02} UTC", s / 3600, s % 3600 / 60)
}

/// Before Claude Code is ready: not installed, not signed in.
fn not_ready(
    ui: &mut Ui,
    settings: &mut SettingsState,
    claude: Option<&ClaudeCode>,
    waiting: bool,
    p: &Palette,
) -> Option<Action> {
    let mut action = None;
    forms::padded(ui, 24, 20, |ui| {
        ui.set_max_width(580.0);
        let heading = |ui: &mut Ui, text: &str| {
            ui.label(
                egui::RichText::new(text)
                    .font(Face::PlexMedium.at(15.0))
                    .color(p.ink),
            );
            ui.add_space(6.0);
        };
        let text = |ui: &mut Ui, text: &str| {
            forms::paragraph(ui, text, 12.5, 19.0, p.ink_muted);
        };
        match claude {
            None => text(ui, "Looking for Claude Code…"),
            Some(c) if c.path.is_none() => {
                heading(ui, "Claude Code is not installed");
                text(
                    ui,
                    "Studio runs the Claude Code you install, on your Claude plan. Install it from claude.com/claude-code, then check again.",
                );
                ui.add_space(12.0);
                if widgets::button(ui, ButtonKind::Secondary, "check again", true, None).clicked() {
                    settings.read_again();
                }
            }
            Some(c) => {
                heading(ui, "Claude Code is not signed in");
                text(
                    ui,
                    &format!(
                        "Studio runs the Claude Code you installed ({}), on your Claude plan. Claude sees the workspace you open and only acts through Studio's tools; you approve what it changes.",
                        found(c)
                    ),
                );
                if let Some(problem) = &c.problem {
                    ui.add_space(4.0);
                    forms::paragraph(ui, problem, 11.5, 17.0, p.ink_faint);
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if widgets::button(
                        ui,
                        ButtonKind::Primary,
                        "sign in through Claude Code",
                        !waiting,
                        None,
                    )
                    .clicked()
                    {
                        action = Some(Action::SignIn(c.sign_in.clone()));
                    }
                    if widgets::button(ui, ButtonKind::Secondary, "check again", true, None)
                        .clicked()
                    {
                        settings.read_again();
                    }
                });
                ui.add_space(8.0);
                forms::paragraph(
                    ui,
                    if waiting {
                        "Finish signing in in the terminal below; Studio notices when you are done."
                    } else {
                        "Runs Claude Code's own sign-in in the terminal, which opens your browser. Studio never sees or keeps your sign-in."
                    },
                    11.5,
                    17.0,
                    p.ink_faint,
                );
            }
        }
    });
    action
}

/// `Claude Code 2.1.314 at ~/.local/bin/claude`.
pub fn found(c: &ClaudeCode) -> String {
    let version = c
        .version
        .as_ref()
        .map(|v| format!("Claude Code {v}"))
        .unwrap_or_else(|| "Claude Code".into());
    match &c.path {
        Some(path) => format!("{version} at {}", path.display()),
        None => version,
    }
}

/// `signed in as a@b.c (max)`.
pub fn signed_in(c: &ClaudeCode) -> String {
    match &c.account {
        Some(a) if a.plan.is_empty() => format!("signed in as {}", a.email),
        Some(a) => format!("signed in as {} ({})", a.email, a.plan),
        None => "not signed in".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_conversation_follows_claude_code() {
        let mut c = Conversation::default();
        for e in [
            ConversationEvent::Started {
                session: "s1".into(),
                model: "claude-fable-5-1".into(),
            },
            ConversationEvent::TextDelta("I'll ".into()),
            ConversationEvent::TextDelta("read".into()),
            ConversationEvent::Text("I'll read it.".into()),
            ConversationEvent::ToolUse {
                id: "t1".into(),
                name: "read_file".into(),
                input: "{}".into(),
            },
            ConversationEvent::ToolResult {
                id: "t1".into(),
                text: "1 print".into(),
                error: false,
                image: false,
            },
            ConversationEvent::TurnDone {
                cost: 0.5,
                error: None,
                denied: 0,
            },
        ] {
            c.event(e);
        }
        assert_eq!(c.session.as_deref(), Some("s1"));
        assert_eq!(c.cost, 0.5);
        assert_eq!(
            c.entries,
            vec![
                Entry::Claude {
                    text: "I'll read it.".into(),
                    streaming: false
                },
                Entry::Tool {
                    id: "t1".into(),
                    name: "read_file".into(),
                    input: "{}".into(),
                    answer: Some(("1 print".into(), false, false))
                },
            ]
        );
        c.ask(
            7,
            "files.edit".into(),
            "write src/a.wowlua (3 lines)".into(),
        );
        assert_eq!(c.open_asks(), vec![7]);
        c.event(ConversationEvent::Limit {
            rejected: true,
            resets_at: Some(3600 * 16),
            utilization: Some(1.0),
        });
        assert_eq!(c.limited, Some(Some(57_600)));
        assert_eq!(clock(57_600), "16:00 UTC");
    }

    /// A conversation kept and opened again (S101): its title, its
    /// session to go on with; a question left open is answered no.
    #[test]
    fn a_conversation_is_kept_and_opened_again() {
        let mut c = Conversation::default();
        assert!(c.kept(1).is_none(), "nothing to keep");
        c.begin(&Settings::default(), 100);
        assert_eq!(c.id, 100);
        assert_eq!(c.model_choice.as_deref(), Some("fable"));
        c.entries
            .push(Entry::You("a pull countdown on the threat meter".into()));
        c.session = Some("s1".into());
        c.ask(3, "files.edit".into(), "write a".into());
        let k = c.kept(200).unwrap();
        assert_eq!(k.title, "a pull countdown on the threat meter");
        let json = serde_json::to_string(&k).unwrap();
        let back: Kept = serde_json::from_str(&json).unwrap();
        let again = Conversation::from_kept(back);
        assert_eq!(again.id, 100);
        assert_eq!(again.session.as_deref(), Some("s1"));
        assert!(!again.running);
        assert!(again.open_asks().is_empty());
    }
}
