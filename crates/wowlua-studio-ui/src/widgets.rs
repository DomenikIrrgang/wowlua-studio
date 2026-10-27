//! The design system's controls as egui widgets: square, flat, lowercase,
//! a 2 px focus ring offset 2 px, amber only for the primary action and
//! the current item.

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Galley, Pos2, Rect, Response, Role, Sense,
    Stroke, StrokeKind, Ui, Vec2, WidgetInfo,
    text::{LayoutJob, TextFormat},
};
use std::sync::Arc;

use crate::{Face, Icon, icons, palette};

/// Lays out one line of text; `spacing` is the extra letter spacing in px
/// (labels take 0.02 em).
pub fn galley(ui: &Ui, text: &str, font: FontId, color: Color32, spacing: f32) -> Arc<Galley> {
    let mut job = LayoutJob::single_section(
        text.to_owned(),
        TextFormat {
            font_id: font,
            color,
            extra_letter_spacing: spacing,
            ..Default::default()
        },
    );
    job.wrap.max_rows = 1;
    ui.fonts_mut(|f| f.layout_job(job))
}

/// A label's font: Plex 500 at `size`, with its letter spacing.
pub fn label_font(size: f32) -> (FontId, f32) {
    (Face::PlexMedium.at(size), size * 0.02)
}

/// The focus ring: 2 px of `focus`, 2 px outside `rect`.
pub fn focus_ring(ui: &Ui, rect: Rect, response: &Response) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            CornerRadius::ZERO,
            Stroke::new(2.0, palette(ui.ctx()).focus()),
            StrokeKind::Outside,
        );
    }
}

/// An icon in the text colour, 16 px (or 14 px small).
pub fn icon(ui: &mut Ui, icon: Icon, size: f32, color: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    icons::paint(ui.painter(), rect, icon, color);
    response
}

/// A 28 px square button showing only an icon; `label` names it for
/// screen readers and its tooltip. `on` draws it pressed.
pub fn icon_button(ui: &mut Ui, icon: Icon, label: &str, on: bool) -> Response {
    let p = palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(Role::Button, ui.is_enabled(), label));
    let enabled = ui.is_enabled();
    let (bg, fg) = if !enabled {
        (Color32::TRANSPARENT, p.ink_faint)
    } else if on {
        (p.bg_400, p.ink)
    } else if response.hovered() {
        (p.bg_300, p.ink)
    } else {
        (Color32::TRANSPARENT, p.ink_muted)
    };
    ui.painter().rect_filled(rect, CornerRadius::ZERO, bg);
    icons::paint(
        ui.painter(),
        Rect::from_center_size(rect.center(), Vec2::splat(16.0)),
        icon,
        fg,
    );
    focus_ring(ui, rect, &response);
    response
        .on_hover_text(label)
        .on_hover_cursor(CursorIcon::PointingHand)
}

/// A panel's `…` menu button: 24 px, `ink-muted`.
pub fn menu_button(ui: &mut Ui, label: &str) -> Response {
    let p = palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(Role::Button, ui.is_enabled(), label));
    let (bg, fg) = if response.hovered() {
        (p.bg_300, p.ink)
    } else {
        (Color32::TRANSPARENT, p.ink_muted)
    };
    ui.painter().rect_filled(rect, CornerRadius::ZERO, bg);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        "…",
        Face::Plex.at(13.0),
        fg,
    );
    focus_ring(ui, rect, &response);
    response
        .on_hover_text(label)
        .on_hover_cursor(CursorIcon::PointingHand)
}

/// A button's role.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonKind {
    /// The one action a view exists for: amber.
    Primary,
    /// Its alternatives: a `line-strong` frame.
    Secondary,
    /// Small inline actions: `[ label ]`.
    Ghost,
}

/// A check mark centred on `center` (the fonts have none: it is drawn).
pub fn check_mark(painter: &egui::Painter, center: egui::Pos2, color: Color32) {
    painter.line(
        vec![
            center + Vec2::new(-4.0, 0.0),
            center + Vec2::new(-1.25, 2.75),
            center + Vec2::new(4.0, -2.5),
        ],
        egui::Stroke::new(1.75, color),
    );
}

/// A command button: 36 px, or 28 px `small`; an optional leading icon.
pub fn button(
    ui: &mut Ui,
    kind: ButtonKind,
    label: &str,
    small: bool,
    icon: Option<Icon>,
) -> Response {
    let p = palette(ui.ctx());
    let enabled = ui.is_enabled();
    let (font, spacing) = label_font(12.0);
    let color = match (enabled, kind) {
        (false, _) => p.ink_faint,
        (true, ButtonKind::Primary) => p.on_amber,
        (true, _) => p.ink,
    };
    let text = galley(ui, label, font.clone(), color, spacing);
    let bracket = galley(ui, "[", font.clone(), p.ink_muted, 0.0);
    let height = if small { 28.0 } else { 36.0 };
    let pad = if small { 10.0 } else { 16.0 };
    let icon_w = if icon.is_some() { 16.0 + 6.0 } else { 0.0 };
    let brackets_w = if kind == ButtonKind::Ghost {
        2.0 * (bracket.size().x + 4.0)
    } else {
        0.0
    };
    let width = pad * 2.0 + icon_w + brackets_w + text.size().x;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, label));
    let hovered = enabled && response.hovered();
    let pressed = enabled && response.is_pointer_button_down_on();
    let painter = ui.painter();
    match kind {
        ButtonKind::Primary if enabled => {
            let fill = if hovered {
                p.amber.gamma_multiply(1.08)
            } else {
                p.amber
            };
            let body = if pressed {
                rect.translate(Vec2::Y)
            } else {
                rect
            };
            if !pressed {
                // The press edge: 2 px of darker amber under the block.
                painter.rect_filled(
                    Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - 2.0), rect.max),
                    CornerRadius::ZERO,
                    p.amber_ink.gamma_multiply(0.7),
                );
            }
            painter.rect_filled(
                body.with_max_y(body.bottom() - 2.0 * f32::from(!pressed)),
                CornerRadius::ZERO,
                fill,
            );
        }
        ButtonKind::Primary | ButtonKind::Secondary => {
            if hovered {
                painter.rect_filled(rect, CornerRadius::ZERO, p.bg_300);
            }
            let border = if enabled { p.line_strong } else { p.line };
            painter.rect_stroke(
                rect,
                CornerRadius::ZERO,
                Stroke::new(1.0, border),
                StrokeKind::Inside,
            );
        }
        ButtonKind::Ghost => {
            if hovered {
                painter.rect_filled(rect, CornerRadius::ZERO, p.bg_300);
            }
        }
    }
    let mut x = rect.left() + pad;
    let cy = rect.center().y
        + if pressed && kind == ButtonKind::Primary {
            1.0
        } else {
            0.0
        };
    if kind == ButtonKind::Ghost {
        painter.galley(
            Pos2::new(x, cy - bracket.size().y / 2.0),
            bracket.clone(),
            p.ink_muted,
        );
        x += bracket.size().x + 4.0;
    }
    if let Some(i) = icon {
        icons::paint(
            painter,
            Rect::from_min_size(Pos2::new(x, cy - 8.0), Vec2::splat(16.0)),
            i,
            color,
        );
        x += icon_w;
    }
    painter.galley(Pos2::new(x, cy - text.size().y / 2.0), text.clone(), color);
    if kind == ButtonKind::Ghost {
        let close = galley(ui, "]", font, p.ink_muted, 0.0);
        x += text.size().x + 4.0;
        ui.painter()
            .galley(Pos2::new(x, cy - close.size().y / 2.0), close, p.ink_muted);
    }
    focus_ring(ui, rect, &response);
    if enabled {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response.on_hover_cursor(CursorIcon::NotAllowed)
    }
}

/// A key cap: `ctrl`, `k`.
pub fn kbd(ui: &mut Ui, key: &str) -> Response {
    let size = Vec2::new(kbd_width(ui, key), 16.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    paint_kbd(ui, rect.left(), rect.center().y, key);
    response
}

/// A key cap's width.
pub fn kbd_width(ui: &Ui, key: &str) -> f32 {
    galley(ui, key, Face::Plex.at(10.5), Color32::PLACEHOLDER, 0.0)
        .size()
        .x
        + 8.0
}

/// Paints a key cap from `left`, centred on `center_y`, outside any
/// layout; returns its rectangle.
pub fn paint_kbd(ui: &Ui, left: f32, center_y: f32, key: &str) -> Rect {
    let p = palette(ui.ctx());
    let text = galley(ui, key, Face::Plex.at(10.5), p.ink, 0.0);
    let rect = Rect::from_min_size(
        Pos2::new(left, (center_y - 8.0).round()),
        Vec2::new(text.size().x + 8.0, 16.0),
    );
    let painter = ui.painter();
    painter.rect_filled(rect.translate(Vec2::Y), CornerRadius::same(2), p.key_edge);
    painter.rect_filled(rect, CornerRadius::same(2), p.bg_200);
    painter.rect_stroke(
        rect,
        CornerRadius::same(2),
        Stroke::new(1.0, p.line_strong),
        StrokeKind::Inside,
    );
    painter.galley(rect.center() - text.size() / 2.0, text, p.ink);
    rect
}

/// Paints a shortcut's key caps ending at `right`, centred on
/// `center_y`; returns where they start.
pub fn paint_keys_right(ui: &Ui, right: f32, center_y: f32, keys: &[impl AsRef<str>]) -> f32 {
    let mut x = right;
    for key in keys.iter().rev() {
        let w = kbd_width(ui, key.as_ref());
        paint_kbd(ui, x - w, center_y, key.as_ref());
        x -= w + 4.0;
    }
    x + 4.0
}

/// A badge's look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadgeKind {
    Neutral,
    Error,
    Warning,
    Done,
    /// Future work: a dashed frame.
    Planned,
}

/// A small fact: one or two lowercase words.
pub fn badge(ui: &mut Ui, text: &str, kind: BadgeKind) -> Response {
    let p = palette(ui.ctx());
    let (fg, bg, border) = match kind {
        BadgeKind::Neutral => (p.ink, p.bg_200, p.line_strong),
        BadgeKind::Error => (p.error, p.error_soft, p.error),
        BadgeKind::Warning => (p.warning, p.warning_soft, p.warning),
        BadgeKind::Done => (p.success, p.success_soft, p.success),
        BadgeKind::Planned => (p.ink_muted, Color32::TRANSPARENT, p.line_strong),
    };
    let (font, spacing) = label_font(11.0);
    let text = galley(ui, text, font, fg, spacing);
    let size = Vec2::new(text.size().x + 12.0, 18.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(2), bg);
    if kind == BadgeKind::Planned {
        dashed_rect(ui, rect, border);
    } else {
        painter.rect_stroke(
            rect,
            CornerRadius::same(2),
            Stroke::new(1.0, border),
            StrokeKind::Inside,
        );
    }
    ui.painter()
        .galley(rect.center() - text.size() / 2.0, text, fg);
    response
}

/// A 1 px dashed frame inside `rect`.
pub fn dashed_rect(ui: &Ui, rect: Rect, color: Color32) {
    let r = rect.shrink(0.5);
    let stroke = Stroke::new(1.0, color);
    let corners = [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
        r.left_top(),
    ];
    for pair in corners.windows(2) {
        ui.painter().extend(egui::Shape::dashed_line(
            &[pair[0], pair[1]],
            stroke,
            3.0,
            2.0,
        ));
    }
}

/// A segmented control; returns the option picked this frame.
pub fn seg(ui: &mut Ui, id: &str, options: &[&str], selected: usize, large: bool) -> Option<usize> {
    let p = palette(ui.ctx());
    let (font_size, height, pad) = if large {
        (12.0, 26.0, 12.0)
    } else {
        (11.0, 18.0, 8.0)
    };
    let galleys: Vec<_> = options
        .iter()
        .map(|o| galley(ui, o, Face::Plex.at(font_size), p.ink, 0.0))
        .collect();
    let inner_w: f32 = galleys.iter().map(|g| g.size().x + 2.0 * pad).sum();
    let outer = Vec2::new(inner_w + 4.0, height + 4.0);
    let (rect, _) = ui.allocate_exact_size(outer, Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_000);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, p.line_strong),
        StrokeKind::Inside,
    );
    let mut picked = None;
    let mut x = rect.left() + 2.0;
    for (i, g) in galleys.into_iter().enumerate() {
        let w = g.size().x + 2.0 * pad;
        let r = Rect::from_min_size(Pos2::new(x, rect.top() + 2.0), Vec2::new(w, height));
        let resp = ui.interact(r, ui.scope_id().with((id, i)), Sense::click());
        let on = i == selected;
        resp.widget_info(|| WidgetInfo::selected(Role::RadioButton, true, on, options[i]));
        let (bg, fg) = if on {
            (p.bg_400, p.ink)
        } else if resp.hovered() {
            (p.bg_300, p.ink)
        } else {
            (Color32::TRANSPARENT, p.ink_muted)
        };
        ui.painter().rect_filled(r, CornerRadius::ZERO, bg);
        ui.painter().galley(r.center() - g.size() / 2.0, g, fg);
        focus_ring(ui, r, &resp);
        if resp.clicked() {
            picked = Some(i);
        }
        resp.on_hover_cursor(CursorIcon::PointingHand);
        x += w;
    }
    picked
}

/// Text placed at `pos` by `anchor`, in one line.
pub fn text_at(
    ui: &Ui,
    pos: Pos2,
    anchor: Align2,
    text: &str,
    font: FontId,
    color: Color32,
) -> Rect {
    ui.painter().text(pos, anchor, text, font, color)
}
