//! The design system's form controls (fields, selects, check boxes, text
//! buttons) and the boxes pages are built from (cards, callouts, terminal
//! blocks, key-value lists).

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Frame, Margin, Pos2, Rect, Response,
    RichText, Role, Sense, Shape, Stroke, StrokeKind, TextEdit, Ui, UiBuilder, Vec2, WidgetInfo,
    text::{LayoutJob, TextFormat},
};

use crate::{Face, palette, widgets};

/// `size-control`: inputs, selects and large buttons.
pub const CONTROL_H: f32 = 36.0;

/// Text at the available width, wrapped: `size` px Plex on `line` px
/// lines.
pub fn paragraph(ui: &mut Ui, text: &str, size: f32, line: f32, color: Color32) -> Response {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .font(Face::Plex.at(size))
                .line_height(Some(line))
                .color(color),
        )
        .wrap(),
    )
}

/// A field's label: `label` in `ink`.
pub fn label(ui: &mut Ui, text: &str) -> Response {
    let p = palette(ui.ctx());
    let (font, spacing) = widgets::label_font(12.0);
    let g = widgets::galley(ui, text, font, p.ink, spacing);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(g.size().x, 16.0), Sense::hover());
    ui.painter().galley(
        Pos2::new(rect.left(), rect.center().y - g.size().y / 2.0),
        g,
        p.ink,
    );
    response
}

/// A field's help under it: 12 px `ink-muted`.
pub fn help(ui: &mut Ui, text: &str) -> Response {
    let p = palette(ui.ctx());
    paragraph(ui, text, 12.0, 18.0, p.ink_muted)
}

/// A page's bar: one row `CONTROL_H` tall, its title and controls centred
/// on it (a plain horizontal row centres only on its first item's height).
pub fn bar<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), CONTROL_H),
        egui::Layout::left_to_right(egui::Align::Center),
        add,
    )
    .inner
}

/// A row of two columns, `left` and `control` wide, centred on each other:
/// the shorter pads by half of what it lacks, by their heights of the last
/// frame (one more frame when a height changes). `id` names the row.
pub fn centred_row(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    left: f32,
    control: f32,
    add_left: impl FnOnce(&mut Ui),
    add_control: impl FnOnce(&mut Ui),
) {
    let id = egui::Id::unique(("wowlua-studio-centred-row", id));
    let (left_h, control_h): (f32, f32) = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let tall = left_h.max(control_h);
    let mut measured = (0.0, 0.0);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(left);
            ui.add_space(((tall - left_h) / 2.0).max(0.0));
            let top = ui.cursor().top();
            add_left(ui);
            measured.0 = ui.min_rect().bottom() - top;
        });
        ui.vertical(|ui| {
            ui.set_width(control);
            ui.set_max_width(control);
            ui.add_space(((tall - control_h) / 2.0).max(0.0));
            let top = ui.cursor().top();
            add_control(ui);
            measured.1 = ui.min_rect().bottom() - top;
        });
    });
    if (measured.0 - left_h).abs() > 0.5 || (measured.1 - control_h).abs() > 0.5 {
        ui.data_mut(|d| d.insert_temp(id, measured));
        ui.ctx().request_repaint();
    }
}

/// A one-line text input, `width` wide; `hint` shows while it is empty.
pub fn text_input(ui: &mut Ui, text: &mut String, width: f32, hint: &str) -> Response {
    let p = palette(ui.ctx());
    let enabled = ui.is_enabled();
    let (rect, frame) = ui.allocate_exact_size(Vec2::new(width, CONTROL_H), Sense::click());
    let ground = ui.painter().add(Shape::Noop);
    let font = Face::Plex.at(13.5);
    let row = ui.fonts_mut(|f| f.row_height(&font));
    let cy = rect.center().y;
    let inner = Rect::from_min_max(
        Pos2::new(rect.left() + 12.0, cy - row / 2.0),
        Pos2::new(rect.right() - 12.0, cy + row / 2.0),
    );
    let mut child = ui.new_child(UiBuilder::new().max_rect(inner));
    let output = TextEdit::singleline(text)
        .frame(Frame::NONE)
        .margin(Margin::ZERO)
        .font(font.clone())
        .text_color(if enabled { p.ink } else { p.ink_faint })
        .hint_text(RichText::new(hint).font(font).color(p.ink_faint))
        .desired_width(inner.width())
        .show(&mut child);
    let response = output.response.response;
    if frame.clicked() {
        response.request_focus();
    }
    let focused = response.has_focus();
    let (fill, border) = if !enabled {
        (p.bg_200, p.line)
    } else if focused {
        (p.bg_000, p.amber_ink)
    } else {
        (p.bg_000, p.line_strong)
    };
    ui.painter()
        .set(ground, Shape::rect_filled(rect, CornerRadius::ZERO, fill));
    ui.painter().rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(if focused { 2.0 } else { 1.0 }, border),
        StrokeKind::Inside,
    );
    frame.on_hover_cursor(CursorIcon::Text);
    response
}

/// A select: the chosen option and a chevron; a click lists the options.
/// `options` are its texts and whether each can be picked. Returns
/// whether the choice changed.
pub fn select(
    ui: &mut Ui,
    id: &str,
    options: &[(&str, bool)],
    selected: &mut usize,
    width: f32,
) -> bool {
    select_sized(ui, id, options, selected, width, CONTROL_H)
}

/// [`select`] at a height of its own: a small one (under 32 px, the title
/// bar's) has smaller text and a tighter chevron.
pub fn select_sized(
    ui: &mut Ui,
    id: &str,
    options: &[(&str, bool)],
    selected: &mut usize,
    width: f32,
    height: f32,
) -> bool {
    let p = palette(ui.ctx());
    let enabled = ui.is_enabled();
    let small = height < 32.0;
    let (pad, chevron, size) = if small {
        (8.0, 12.0, 12.0)
    } else {
        (12.0, 18.0, 13.5)
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let current = options.get(*selected).map_or("", |o| o.0);
    response.widget_info(|| WidgetInfo::labeled(Role::ComboBox, enabled, current));
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
    let fill = if !enabled {
        p.bg_200
    } else if response.hovered() || open {
        p.bg_300
    } else {
        p.bg_000
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::ZERO, fill);
    painter.rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, if enabled { p.line_strong } else { p.line }),
        StrokeKind::Inside,
    );
    let ink = if enabled { p.ink } else { p.ink_faint };
    let text_rect = Rect::from_min_max(
        rect.min,
        Pos2::new(rect.right() - chevron * 2.0, rect.bottom()),
    );
    painter.with_clip_rect(text_rect).text(
        Pos2::new(rect.left() + pad, rect.center().y),
        Align2::LEFT_CENTER,
        current,
        Face::Plex.at(size),
        ink,
    );
    // The chevron: a 6 px corner turned down.
    let c = Pos2::new(rect.right() - chevron, rect.center().y - 1.5);
    painter.line(
        vec![
            c + Vec2::new(-3.5, -1.75),
            c + Vec2::new(0.0, 1.75),
            c + Vec2::new(3.5, -1.75),
        ],
        Stroke::new(1.5, p.ink_muted),
    );
    widgets::focus_ring(ui, rect, &response);
    let mut changed = false;
    // (the list is as wide as its longest option, never narrower than the
    // box)
    let widest = options
        .iter()
        .map(|(text, _)| {
            ui.fonts_mut(|f| {
                f.layout_no_wrap((*text).to_owned(), Face::Plex.at(13.0), p.ink)
                    .size()
                    .x
            })
        })
        .fold(0.0_f32, f32::max);
    let list_w = width.max(widest + 8.0 + 16.0);
    egui::Popup::menu(&response)
        .id(egui::Id::unique(("wowlua-studio-select", id)))
        .width(list_w)
        .show(|ui| {
            ui.set_min_width(list_w - 8.0);
            for (i, &(text, can)) in options.iter().enumerate() {
                let w = ui.available_width();
                let (r, row) = ui.allocate_exact_size(
                    Vec2::new(w, 28.0),
                    if can { Sense::click() } else { Sense::hover() },
                );
                let on = i == *selected;
                if can && (row.hovered() || on) {
                    ui.painter().rect_filled(
                        r,
                        CornerRadius::ZERO,
                        if row.hovered() { p.bg_300 } else { p.bg_400 },
                    );
                }
                ui.painter().text(
                    Pos2::new(r.left() + 8.0, r.center().y),
                    Align2::LEFT_CENTER,
                    text,
                    Face::Plex.at(13.0),
                    if can { p.ink } else { p.ink_faint },
                );
                if row.clicked() {
                    changed = i != *selected;
                    *selected = i;
                    ui.close();
                }
            }
        });
    if enabled {
        response.on_hover_cursor(CursorIcon::PointingHand);
    }
    changed
}

/// A check box with its label and, under the label, its help. Returns
/// whether it was toggled.
pub fn checkbox(ui: &mut Ui, checked: &mut bool, label: &str, help: Option<&str>) -> bool {
    let p = palette(ui.ctx());
    let enabled = ui.is_enabled();
    let label_font = Face::Plex.at(13.5);
    let help_font = Face::Plex.at(12.0);
    let ink = if enabled { p.ink } else { p.ink_faint };
    let text = widgets::galley(ui, label, label_font, ink, 0.0);
    let help_g = help.map(|h| widgets::galley(ui, h, help_font, p.ink_muted, 0.0));
    let text_w = text
        .size()
        .x
        .max(help_g.as_ref().map_or(0.0, |g| g.size().x));
    let height = 22.0 + if help_g.is_some() { 18.0 + 2.0 } else { 0.0 };
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(18.0 + 12.0 + text_w, height), Sense::click());
    response.widget_info(|| WidgetInfo::selected(Role::CheckBox, enabled, *checked, label));
    let toggled = enabled && response.clicked();
    if toggled {
        *checked = !*checked;
    }
    let boxr = Rect::from_min_size(Pos2::new(rect.left(), rect.top() + 2.0), Vec2::splat(18.0));
    let painter = ui.painter();
    let (fill, border) = match (enabled, *checked) {
        (false, true) => (p.line_strong, p.line_strong),
        (false, false) => (p.bg_200, p.line),
        (true, true) => (p.amber, p.amber),
        (true, false) if response.hovered() => (p.bg_000, p.ink_muted),
        (true, false) => (p.bg_000, p.line_strong),
    };
    painter.rect_filled(boxr, CornerRadius::ZERO, fill);
    painter.rect_stroke(
        boxr,
        CornerRadius::ZERO,
        Stroke::new(1.0, border),
        StrokeKind::Inside,
    );
    if *checked {
        painter.rect_filled(
            Rect::from_center_size(boxr.center(), Vec2::splat(8.0)),
            CornerRadius::ZERO,
            p.on_amber,
        );
    }
    let x = boxr.right() + 12.0;
    painter.galley(
        Pos2::new(x, rect.top() + 11.0 - text.size().y / 2.0),
        text,
        ink,
    );
    if let Some(g) = help_g {
        painter.galley(
            Pos2::new(x, rect.top() + 24.0 + 9.0 - g.size().y / 2.0),
            g,
            p.ink_muted,
        );
    }
    widgets::focus_ring(ui, boxr, &response);
    let cursor = if enabled {
        CursorIcon::PointingHand
    } else {
        CursorIcon::NotAllowed
    };
    response.on_hover_cursor(cursor);
    toggled
}

/// An inline action (Option D): its verb's icon, then the label,
/// underlined under the pointer; a removing one is danger.
pub fn text_button(ui: &mut Ui, label: &str) -> Response {
    widgets::button(ui, widgets::ButtonKind::Inline, label, true, None)
}

/// A card: a `bg-200` box with a 36 px head (`title`, then `head` from the
/// right) over `body`, which sets its own padding.
pub fn card<R>(
    ui: &mut Ui,
    title: &str,
    head: impl FnOnce(&mut Ui),
    body: impl FnOnce(&mut Ui) -> R,
) -> R {
    let p = palette(ui.ctx());
    let width = ui.available_width();
    let ground = ui.painter().add(Shape::Noop);
    let top = ui.cursor().min;
    let head_rect = Rect::from_min_size(top, Vec2::new(width, 36.0));
    ui.allocate_rect(head_rect, Sense::hover());
    let (font, spacing) = widgets::label_font(12.0);
    let t = widgets::galley(ui, title, font, p.ink_muted, spacing);
    ui.painter().galley(
        Pos2::new(
            head_rect.left() + 14.0,
            head_rect.center().y - t.size().y / 2.0,
        ),
        t,
        p.ink_muted,
    );
    let mut right = ui.new_child(
        UiBuilder::new()
            .max_rect(head_rect.shrink2(Vec2::new(14.0, 4.0)))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    head(&mut right);
    ui.painter().hline(
        head_rect.x_range(),
        head_rect.bottom() - 0.5,
        Stroke::new(1.0, p.line),
    );
    let mut inner = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_size(
                Pos2::new(top.x, head_rect.bottom()),
                Vec2::new(width, f32::INFINITY),
            ))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    inner.set_width(width);
    let out = body(&mut inner);
    let bottom = inner.min_rect().bottom().max(head_rect.bottom());
    let rect = Rect::from_min_max(top, Pos2::new(top.x + width, bottom));
    ui.allocate_rect(rect, Sense::hover());
    ui.painter().set(
        ground,
        Shape::rect_filled(rect, CornerRadius::ZERO, p.bg_200),
    );
    ui.painter().rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, p.line),
        StrokeKind::Inside,
    );
    out
}

/// Padding inside a box: `x` and `y` px around `add`.
pub fn padded<R>(ui: &mut Ui, x: i8, y: i8, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::NONE
        .inner_margin(Margin::symmetric(x, y))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// A callout's look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalloutKind {
    Plain,
    Error,
    Warning,
    Note,
    Ok,
}

/// A callout: a framed note in its kind's colours, at the full width.
pub fn callout<R>(ui: &mut Ui, kind: CalloutKind, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = palette(ui.ctx());
    let (fill, border) = match kind {
        CalloutKind::Plain => (p.bg_200, p.line),
        CalloutKind::Error => (p.error_soft, p.error),
        CalloutKind::Warning => (p.warning_soft, p.warning),
        CalloutKind::Note => (p.note_soft, p.note),
        CalloutKind::Ok => (p.success_soft, p.success),
    };
    Frame::NONE
        .fill(fill)
        .stroke(Stroke::new(1.0, border))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// A line of a terminal block.
pub enum TermLine<'a> {
    /// `$ command`.
    Command(&'a str),
    /// Output, `ink-muted`.
    Out(&'a str),
    /// A status line: its word in `success`, then the rest.
    Status(&'a str, &'a str),
}

/// A terminal block: an optional bar (its left and right text), then the
/// lines in 13.5 px Plex on 22 px, wrapped at the block's width.
pub fn terminal(ui: &mut Ui, bar: Option<(&str, &str)>, lines: &[TermLine<'_>]) {
    let p = palette(ui.ctx());
    let width = ui.available_width();
    let font = Face::Plex.at(13.5);
    let bold = Face::PlexSemiBold.at(13.5);
    let section = |job: &mut LayoutJob, text: &str, font: &FontId, color: Color32| {
        job.append(
            text,
            0.0,
            TextFormat {
                font_id: font.clone(),
                color,
                line_height: Some(22.0),
                ..Default::default()
            },
        );
    };
    let galleys: Vec<_> = lines
        .iter()
        .map(|line| {
            let mut job = LayoutJob::default();
            match line {
                TermLine::Command(c) => {
                    section(&mut job, "$ ", &font, p.ink_muted);
                    section(&mut job, c, &font, p.ink);
                }
                TermLine::Out(o) => section(&mut job, o, &font, p.ink_muted),
                TermLine::Status(word, rest) => {
                    section(&mut job, &format!("{word:>12} "), &bold, p.success);
                    section(&mut job, rest, &font, p.ink_muted);
                }
            }
            job.wrap.max_width = width - 32.0;
            ui.fonts_mut(|f| f.layout_job(job))
        })
        .collect();
    let bar_h = if bar.is_some() { 32.0 } else { 0.0 };
    let body_h: f32 = galleys.iter().map(|g| g.size().y).sum();
    let height = bar_h + 12.0 * 2.0 + body_h;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, CornerRadius::ZERO, p.bg_000);
    painter.rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, p.line),
        StrokeKind::Inside,
    );
    if let Some((left, right)) = bar {
        let cy = rect.top() + 16.0;
        let small = Face::Plex.at(12.0);
        painter.text(
            Pos2::new(rect.left() + 12.0, cy),
            Align2::LEFT_CENTER,
            left,
            small.clone(),
            p.ink_muted,
        );
        painter.text(
            Pos2::new(rect.right() - 12.0, cy),
            Align2::RIGHT_CENTER,
            right,
            small,
            p.ink_muted,
        );
        painter.hline(
            rect.x_range(),
            rect.top() + bar_h - 0.5,
            Stroke::new(1.0, p.line),
        );
    }
    let mut y = rect.top() + bar_h + 12.0;
    for galley in galleys {
        let h = galley.size().y;
        painter.galley(Pos2::new(rect.left() + 16.0, y), galley, p.ink);
        y += h;
    }
}

/// A key-value list: keys in a 128 px `ink-muted` column, values wrapped
/// beside them.
pub fn key_values(ui: &mut Ui, rows: &[(&str, String)]) {
    let p = palette(ui.ctx());
    let width = ui.available_width();
    for (key, value) in rows {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            let (r, _) = ui.allocate_exact_size(Vec2::new(128.0, 20.0), Sense::hover());
            ui.painter().text(
                Pos2::new(r.left(), r.center().y),
                Align2::LEFT_CENTER,
                *key,
                Face::Plex.at(12.0),
                p.ink_muted,
            );
            ui.allocate_ui(Vec2::new(width - 140.0, 20.0), |ui| {
                paragraph(ui, value, 12.0, 20.0, p.ink);
            });
        });
    }
}
