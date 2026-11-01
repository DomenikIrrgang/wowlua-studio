//! The status bar's branch (M11, the canvas's branch component): the
//! branch checked out with ↑ and ↓, and a click's list of every branch to
//! switch to, find by name or make one from.

use egui::{Align2, CornerRadius, Id, Order, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use wowlua_studio_ui::forms;
use wowlua_studio_ui::{Face, Icon, palette};

use super::{Dialog, GitState};
use crate::system::{GitRequest, System};

const LIST_W: f32 = 400.0;

/// What the list asks of the rest of Studio.
pub enum Action {
    ShowGit,
}

/// The branch's words and ↑↓: `feat/x`, `↑2 ↓1`.
fn words(state: &GitState) -> Option<(String, String)> {
    let (label, ahead, behind) = state.branch_label()?;
    let ab = match (ahead, behind) {
        (0, 0) => String::new(),
        (a, 0) => format!("↑{a}"),
        (0, b) => format!("↓{b}"),
        (a, b) => format!("↑{a} ↓{b}"),
    };
    Some((label, ab))
}

/// The room the branch takes in the bar (none outside a repository).
pub fn width(ui: &Ui, state: &GitState) -> f32 {
    let Some((label, ab)) = words(state) else {
        return 0.0;
    };
    let font = Face::Plex.at(11.5);
    let w = |s: &str| {
        ui.fonts_mut(|f| {
            f.layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::WHITE)
                .size()
                .x
        })
    };
    let ab_w = if ab.is_empty() { 0.0 } else { w(&ab) + 6.0 };
    6.0 + 14.0 + 6.0 + w(&label) + ab_w + 6.0
}

/// The branch in the bar at `left`; a click opens its list.
pub fn button(ui: &mut Ui, state: &mut GitState, system: &dyn System, left: f32, cy: f32) {
    let Some((label, ab)) = words(state) else {
        return;
    };
    let p = palette(ui.ctx());
    let w = width(ui, state);
    let rect = Rect::from_min_size(Pos2::new(left, cy - 10.0), Vec2::new(w, 20.0));
    let r = ui.interact(rect, Id::unique("wowlua-studio-branch"), Sense::click());
    if r.hovered() || state.bar_open {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_300);
        ui.painter().rect_stroke(
            rect,
            CornerRadius::ZERO,
            Stroke::new(1.0, p.line_strong),
            egui::StrokeKind::Inside,
        );
    }
    let icon = Rect::from_min_size(Pos2::new(rect.left() + 6.0, cy - 7.0), Vec2::splat(14.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(icon));
    wowlua_studio_ui::widgets::icon(&mut child, Icon::Git, 14.0, p.ink_muted);
    let font = Face::Plex.at(11.5);
    let t = ui.painter().text(
        Pos2::new(icon.right() + 6.0, cy),
        Align2::LEFT_CENTER,
        &label,
        font.clone(),
        p.ink,
    );
    if !ab.is_empty() {
        ui.painter().text(
            Pos2::new(t.right() + 6.0, cy),
            Align2::LEFT_CENTER,
            &ab,
            font,
            p.ink_muted,
        );
    }
    if r.on_hover_text("switch branch").clicked() {
        state.bar_open = !state.bar_open;
        state.bar_query.clear();
        if state.bar_open
            && let Some(root) = state.root()
        {
            system.git(root, GitRequest::Branches);
        }
    }
}

/// A branch of the list; whether it was clicked.
fn option(ui: &mut Ui, check: bool, name: &str, meta: &str) -> bool {
    let p = palette(ui.ctx());
    let (rect, r) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::click());
    if r.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_300);
    }
    if check {
        ui.painter().text(
            Pos2::new(rect.left() + 16.0, rect.center().y),
            Align2::CENTER_CENTER,
            "✓",
            Face::PlexSemiBold.at(12.0),
            p.amber_ink,
        );
    }
    ui.painter().text(
        Pos2::new(rect.left() + 32.0, rect.center().y),
        Align2::LEFT_CENTER,
        name,
        Face::Plex.at(12.5),
        p.ink,
    );
    ui.painter().text(
        Pos2::new(rect.right() - 10.0, rect.center().y),
        Align2::RIGHT_CENTER,
        meta,
        Face::Plex.at(11.5),
        p.ink_muted,
    );
    r.clicked()
}

fn head(ui: &mut Ui, text: &str) {
    let p = palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
    ui.painter().text(
        Pos2::new(rect.left() + 10.0, rect.bottom() - 4.0),
        Align2::LEFT_BOTTOM,
        text,
        Face::Plex.at(11.0),
        p.ink_muted,
    );
}

/// The list over the bar, from `left`.
pub fn list(
    ctx: &egui::Context,
    state: &mut GitState,
    system: &dyn System,
    left: f32,
    bar_top: f32,
) -> Option<Action> {
    if !state.bar_open {
        return None;
    }
    let p = palette(ctx);
    let mut action = None;
    let mut close = false;
    let mut switch_to: Option<String> = None;
    let mut new_branch = false;
    let current = state.status.as_ref().and_then(|s| s.branch.clone());
    egui::Area::new(Id::unique("wowlua-studio-branch-list"))
        .order(Order::Foreground)
        .pivot(Align2::LEFT_BOTTOM)
        .fixed_pos(Pos2::new(left, bar_top - 4.0))
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(p.bg_200)
                .stroke(Stroke::new(1.0, p.line_strong))
                .shadow(ui.visuals().popup_shadow)
                .inner_margin(egui::Margin::symmetric(0, 4))
                .show(ui, |ui| {
                    ui.set_width(LIST_W);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    forms::padded(ui, 6, 2, |ui| {
                        let r = forms::text_input(ui, &mut state.bar_query, LIST_W - 12.0, "find a branch");
                        r.request_focus();
                    });
                    let q = state.bar_query.to_lowercase();
                    let matches = |n: &str| q.is_empty() || n.to_lowercase().contains(&q);
                    let ab = |a: u32, b: u32| match (a, b) {
                        (0, 0) => String::new(),
                        (a, 0) => format!("↑{a} · "),
                        (0, b) => format!("↓{b} · "),
                        (a, b) => format!("↑{a} ↓{b} · "),
                    };
                    let local: Vec<_> = state.branches.iter().filter(|b| !b.remote && matches(&b.name)).cloned().collect();
                    let remote: Vec<_> = state.branches.iter().filter(|b| b.remote && matches(&b.name)).cloned().collect();
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        if !local.is_empty() {
                            head(ui, "local");
                        }
                        for b in &local {
                            let meta = format!("{}{} · {}", ab(b.ahead, b.behind), b.author, super::page::age(b.time));
                            if option(ui, b.current, &b.name, &meta) && !b.current {
                                switch_to = Some(b.name.clone());
                            }
                        }
                        if !remote.is_empty() {
                            head(ui, "remotes");
                        }
                        for b in &remote {
                            let meta = format!("{} · {}", b.author, super::page::age(b.time));
                            if option(ui, false, &b.name, &meta) {
                                switch_to = Some(b.name.clone());
                            }
                        }
                        if local.is_empty() && remote.is_empty() {
                            forms::padded(ui, 10, 8, |ui| {
                                ui.label(egui::RichText::new(if state.branches.is_empty() { "Reading the branches…" } else { "No branch has that in its name." }).font(Face::Plex.at(12.0)).color(p.ink_muted));
                            });
                        }
                    });
                    ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top() + 2.0, Stroke::new(1.0, p.line));
                    ui.add_space(4.0);
                    let from = current.clone().unwrap_or_else(|| "this commit".into());
                    if option(ui, false, &format!("new branch from {from}…"), "") {
                        new_branch = true;
                    }
                    if option(ui, false, "open the git page", "") {
                        action = Some(Action::ShowGit);
                    }
                    forms::padded(ui, 10, 6, |ui| {
                        forms::paragraph(
                            ui,
                            "Switching with uncommitted changes asks first: carry them, stash them or cancel. A remote's branch is checked out as a local branch that tracks it.",
                            11.5,
                            16.0,
                            p.ink_muted,
                        );
                    });
                });
        });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close = true;
    }
    if let Some(target) = switch_to {
        close = true;
        super::page::start_switch(state, system, target);
    }
    if new_branch {
        close = true;
        state.dialog = Some(Dialog::NewBranch {
            from: None,
            name: String::new(),
            switch: true,
            bad: None,
        });
    }
    if close || action.is_some() {
        state.bar_open = false;
    }
    action
}
