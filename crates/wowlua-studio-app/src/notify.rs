//! Notifications (the notifications board, S047): a toast over the status
//! bar's end, then the tray at its end. The tray keeps this window's
//! notifications until it closes.

use std::path::PathBuf;

use egui::{Align2, Color32, CornerRadius, Id, Order, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use wowlua_studio_ui::forms;
use wowlua_studio_ui::widgets::{self, ButtonKind};
use wowlua_studio_ui::{Face, Icon, Palette, icons, palette};

/// How long a toast shows, in seconds.
const TOAST_FOR: f64 = 6.0;
const TOAST_W: f32 = 400.0;
const TRAY_W: f32 = 460.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Note,
    Ok,
    Warning,
    Error,
}

/// What a notification's button does.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// Writes a file deleted outside back and opens it (S048).
    Restore { path: PathBuf, text: String },
    /// Opens a file in the editor.
    Open(PathBuf),
}

#[derive(Clone, Debug)]
pub struct Notification {
    id: u64,
    pub kind: Kind,
    pub title: String,
    pub text: String,
    /// Its buttons: a label and what it does; the first is the toast's
    /// primary one.
    pub acts: Vec<(String, Act)>,
    /// When it came, on egui's clock.
    at: f64,
    read: bool,
}

impl Notification {
    pub fn new(kind: Kind, title: impl Into<String>, text: impl Into<String>) -> Notification {
        Notification {
            id: 0,
            kind,
            title: title.into(),
            text: text.into(),
            acts: Vec::new(),
            at: 0.0,
            read: false,
        }
    }

    pub fn act(mut self, label: &str, act: Act) -> Notification {
        self.acts.push((label.to_owned(), act));
        self
    }
}

#[derive(Default)]
pub struct Notifications {
    /// Newest first.
    list: Vec<Notification>,
    next: u64,
    /// The toast shown, until when (errors: until dismissed).
    toast: Option<(u64, Option<f64>)>,
    pub open: bool,
}

impl Notifications {
    pub fn push(&mut self, mut n: Notification, now: f64) {
        self.next += 1;
        n.id = self.next;
        n.at = now;
        let until = (n.kind != Kind::Error).then_some(now + TOAST_FOR);
        self.toast = Some((n.id, until));
        self.list.insert(0, n);
    }

    pub fn unread(&self) -> usize {
        self.list.iter().filter(|n| !n.read).count()
    }

    fn dismiss(&mut self, id: u64) {
        self.list.retain(|n| n.id != id);
        if self.toast.is_some_and(|(t, _)| t == id) {
            self.toast = None;
        }
    }

    /// The room the tray's button takes at the status bar's end.
    pub fn tray_width(&self) -> f32 {
        let unread = self.unread();
        let count = if unread > 0 {
            4.0 + (unread.to_string().len() as f32 * 7.0).max(8.0) + 6.0
        } else {
            0.0
        };
        6.0 + 14.0 + count + 6.0 + 10.0
    }

    /// The tray's button at the status bar's end; returns its width.
    pub fn tray_button(&mut self, ui: &mut Ui, status: Rect) -> f32 {
        let p = palette(ui.ctx());
        let unread = self.unread();
        let error = self.list.iter().any(|n| !n.read && n.kind == Kind::Error);
        let count = (unread > 0).then(|| {
            widgets::galley(
                ui,
                &unread.to_string(),
                Face::PlexSemiBold.at(11.0),
                p.on_amber,
                0.0,
            )
        });
        let w = 6.0
            + 14.0
            + count
                .as_ref()
                .map_or(0.0, |g| 4.0 + g.size().x.max(8.0) + 6.0)
            + 6.0;
        let r = Rect::from_min_size(
            Pos2::new(status.right() - 10.0 - w, status.center().y - 10.0),
            Vec2::new(w, 20.0),
        );
        let resp = ui.interact(r, Id::unique("wowlua-studio-tray"), Sense::click());
        resp.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::Role::Button,
                true,
                format!("notifications: {} in all, {unread} unread", self.list.len()),
            )
        });
        let painter = ui.painter();
        if resp.hovered() || self.open {
            painter.rect_filled(r, CornerRadius::ZERO, p.bg_300);
            painter.rect_stroke(
                r,
                CornerRadius::ZERO,
                Stroke::new(1.0, p.line_strong),
                egui::StrokeKind::Inside,
            );
        }
        let ink = if unread > 0 || resp.hovered() || self.open {
            p.ink
        } else {
            p.ink_muted
        };
        icons::paint(
            painter,
            Rect::from_min_size(
                Pos2::new(r.left() + 6.0, r.center().y - 7.0),
                Vec2::splat(14.0),
            ),
            Icon::Notification,
            ink,
        );
        if let Some(g) = count {
            let badge = Rect::from_min_size(
                Pos2::new(r.left() + 6.0 + 14.0 + 4.0, r.center().y - 7.0),
                Vec2::new(g.size().x.max(8.0) + 6.0, 14.0),
            );
            painter.rect_filled(
                badge,
                CornerRadius::same(2),
                if error { p.error } else { p.amber },
            );
            painter.galley(
                badge.center() - g.size() / 2.0,
                g,
                if error { p.on_error } else { p.on_amber },
            );
        }
        if resp
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            self.open = !self.open;
            if self.open {
                self.toast = None;
                for n in &mut self.list {
                    n.read = true;
                }
            }
        }
        w + 10.0
    }

    /// The toast and the tray, over the window; returns a button's act.
    pub fn show(&mut self, ui: &mut Ui, status: Rect) -> Option<Act> {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        let mut picked = None;
        if self.open {
            picked = self.tray(&ctx, status, now);
        } else if let Some((id, until)) = self.toast {
            if until.is_some_and(|u| now > u) {
                self.toast = None;
            } else if let Some(n) = self.list.iter().find(|n| n.id == id).cloned() {
                let (act, dismissed, hovered) = toast(&ctx, status, &n, now);
                if let Some(u) = until {
                    // Longer while the pointer is on it.
                    let until = if hovered { u.max(now + 2.0) } else { u };
                    self.toast = Some((id, Some(until)));
                    ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                        (until - now).max(0.05),
                    ));
                }
                if dismissed {
                    self.toast = None;
                }
                if act.is_some() {
                    self.toast = None;
                    picked = act;
                }
            }
        }
        picked
    }

    fn tray(&mut self, ctx: &egui::Context, status: Rect, now: f64) -> Option<Act> {
        let p = palette(ctx);
        let mut picked = None;
        let mut dismissed = None;
        let mut clear = false;
        let mut close = false;
        let height_guess = 400.0;
        let pos = Pos2::new(
            status.right() - 10.0 - TRAY_W,
            status.top() - 4.0 - height_guess,
        );
        let area = egui::Area::new(Id::unique("wowlua-studio-tray-pop"))
            .order(Order::Foreground)
            .pivot(Align2::LEFT_BOTTOM)
            .fixed_pos(Pos2::new(pos.x, status.top() - 4.0))
            .show(ctx, |ui| {
                ui.set_width(TRAY_W);
                egui::Frame::NONE
                    .fill(p.bg_200)
                    .stroke(Stroke::new(1.0, p.line_strong))
                    .shadow(ui.visuals().popup_shadow)
                    .show(ui, |ui| {
                        ui.set_width(TRAY_W);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        // The head.
                        ui.horizontal(|ui| {
                            ui.set_height(36.0);
                            ui.add_space(12.0);
                            let (font, spacing) = widgets::label_font(12.0);
                            let g = widgets::galley(ui, "notifications", font, p.ink, spacing);
                            let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
                            ui.painter().galley(r.min, g, p.ink);
                            if !self.list.is_empty() {
                                ui.label(
                                    egui::RichText::new(format!("{} in this window", self.list.len()))
                                        .font(Face::Plex.at(11.5))
                                        .color(p.ink_muted),
                                );
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.add_space(6.0);
                                close = widgets::icon_button(ui, Icon::Close, "close", false).clicked();
                                clear = forms::text_button(ui, "clear all").clicked();
                            });
                        });
                        line(ui, p);
                        if self.list.is_empty() {
                            forms::padded(ui, 16, 20, |ui| {
                                forms::paragraph(
                                    ui,
                                    "Nothing new. Files deleted outside Studio, finished jobs, notes about your workspace and new releases show here.",
                                    12.5,
                                    19.0,
                                    p.ink_muted,
                                );
                            });
                            line(ui, p);
                        }
                        egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                            for n in &self.list {
                                let (act, gone) = row(ui, n, now, p);
                                if act.is_some() {
                                    picked = act;
                                }
                                if gone {
                                    dismissed = Some(n.id);
                                }
                                line(ui, p);
                            }
                        });
                        forms::padded(ui, 12, 8, |ui| {
                            forms::paragraph(
                                ui,
                                "This window's notifications, until it closes. A toast shows each for 6 seconds, longer while the pointer is on it; errors stay until you dismiss them.",
                                11.5,
                                16.0,
                                p.ink_muted,
                            );
                        });
                    });
            });
        let _ = area;
        if let Some(id) = dismissed {
            self.dismiss(id);
        }
        if clear {
            self.list.clear();
        }
        if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.open = false;
        }
        picked
    }
}

fn line(ui: &mut Ui, p: &Palette) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .hline(r.x_range(), r.center().y, Stroke::new(1.0, p.line));
}

fn mark(kind: Kind, p: &Palette) -> (&'static str, Color32) {
    match kind {
        Kind::Note => ("·", p.ink_muted),
        Kind::Ok => ("✓", p.success),
        Kind::Warning => ("!", p.warning),
        Kind::Error => ("×", p.error),
    }
}

/// When it came: `just now`, `4 min`, `2 h`.
fn ago(now: f64, at: f64) -> String {
    let s = (now - at).max(0.0);
    if s < 60.0 {
        "just now".into()
    } else if s < 3600.0 {
        format!("{} min", (s / 60.0) as u64)
    } else {
        format!("{} h", (s / 3600.0) as u64)
    }
}

/// A notification's mark and text (the check mark drawn: the fonts have
/// none).
fn mark_at(ui: &Ui, at: Pos2, kind: Kind, p: &Palette) {
    let (sign, color) = mark(kind, p);
    if kind == Kind::Ok {
        ui.painter().line(
            vec![
                at + Vec2::new(-4.0, 0.0),
                at + Vec2::new(-1.25, 2.75),
                at + Vec2::new(4.0, -2.5),
            ],
            Stroke::new(1.75, color),
        );
    } else {
        ui.painter().text(
            at,
            Align2::CENTER_CENTER,
            sign,
            Face::PlexSemiBold.at(12.5),
            color,
        );
    }
}

/// A row of the tray; returns a button's act and whether it was dismissed.
fn row(ui: &mut Ui, n: &Notification, now: f64, p: &Palette) -> (Option<Act>, bool) {
    let mut act = None;
    let mut gone = false;
    let top = ui.cursor().min;
    forms::padded(ui, 12, 8, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let (r, _) = ui.allocate_exact_size(Vec2::new(14.0, 17.0), Sense::hover());
            mark_at(ui, r.center(), n.kind, p);
            let text_w = ui.available_width() - 90.0;
            ui.allocate_ui(Vec2::new(text_w, 17.0), |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    forms::paragraph(ui, &n.title, 12.0, 17.0, p.ink);
                    if !n.text.is_empty() {
                        forms::paragraph(ui, &n.text, 11.5, 16.0, p.ink_muted);
                    }
                    if !n.acts.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            for (label, a) in &n.acts {
                                if forms::text_button(ui, label).clicked() {
                                    act = Some(a.clone());
                                    gone = true;
                                }
                            }
                        });
                    }
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                if widgets::icon_button(ui, Icon::Close, "dismiss", false).clicked() {
                    gone = true;
                }
                ui.label(
                    egui::RichText::new(ago(now, n.at))
                        .font(Face::Plex.at(11.5))
                        .color(p.ink_muted),
                );
            });
        });
    });
    if !n.read {
        let bottom = ui.cursor().min.y;
        ui.painter().rect_filled(
            Rect::from_min_max(top, Pos2::new(top.x + 2.0, bottom)),
            CornerRadius::ZERO,
            p.amber,
        );
    }
    (act, gone)
}

/// The toast over the status bar's end; returns a button's act, whether
/// it was dismissed and whether the pointer is on it.
fn toast(
    ctx: &egui::Context,
    status: Rect,
    n: &Notification,
    now: f64,
) -> (Option<Act>, bool, bool) {
    let p = palette(ctx);
    let mut act = None;
    let mut dismissed = false;
    let border = if n.kind == Kind::Error {
        p.error
    } else {
        p.line_strong
    };
    let area = egui::Area::new(Id::unique("wowlua-studio-toast"))
        .order(Order::Foreground)
        .pivot(Align2::RIGHT_BOTTOM)
        .fixed_pos(Pos2::new(status.right() - 10.0, status.top() - 8.0))
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(p.bg_200)
                .stroke(Stroke::new(1.0, border))
                .shadow(ui.visuals().popup_shadow)
                .inner_margin(egui::Margin {
                    left: 12,
                    right: 6,
                    top: 10,
                    bottom: 10,
                })
                .show(ui, |ui| {
                    ui.set_width(TOAST_W - 18.0);
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let (r, _) = ui.allocate_exact_size(Vec2::new(14.0, 17.0), Sense::hover());
                        mark_at(ui, r.center(), n.kind, p);
                        ui.allocate_ui(
                            Vec2::new(TOAST_W - 18.0 - 14.0 - 8.0 - 24.0 - 8.0, 17.0),
                            |ui| {
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 4.0;
                                    forms::paragraph(ui, &n.title, 12.0, 17.0, p.ink);
                                    if !n.text.is_empty() {
                                        forms::paragraph(ui, &n.text, 11.5, 16.0, p.ink_muted);
                                    }
                                    if !n.acts.is_empty() {
                                        ui.horizontal(|ui| {
                                            for (i, (label, a)) in n.acts.iter().enumerate() {
                                                let clicked = if i == 0 {
                                                    widgets::button(
                                                        ui,
                                                        ButtonKind::Primary,
                                                        label,
                                                        true,
                                                        None,
                                                    )
                                                    .clicked()
                                                } else {
                                                    forms::text_button(ui, label).clicked()
                                                };
                                                if clicked {
                                                    act = Some(a.clone());
                                                }
                                            }
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    ui.label(
                                                        egui::RichText::new(ago(now, n.at))
                                                            .font(Face::Plex.at(11.5))
                                                            .color(p.ink_muted),
                                                    );
                                                },
                                            );
                                        });
                                    }
                                });
                            },
                        );
                        dismissed =
                            widgets::icon_button(ui, Icon::Close, "dismiss", false).clicked();
                    });
                });
        });
    let hovered = area.response.contains_pointer();
    (act, dismissed, hovered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_stay_and_the_rest_go() {
        let mut n = Notifications::default();
        n.push(Notification::new(Kind::Note, "a", ""), 0.0);
        assert_eq!(n.toast.unwrap().1, Some(TOAST_FOR));
        n.push(Notification::new(Kind::Error, "b", ""), 1.0);
        assert_eq!(n.toast.unwrap().1, None);
        assert_eq!(n.unread(), 2);
        assert_eq!(n.list[0].title, "b", "newest first");
        assert_eq!(ago(200.0, 0.0), "3 min");
    }
}
