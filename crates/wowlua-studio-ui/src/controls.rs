//! The settings' controls (the design's Controls board): a slider, a
//! stepper with presets, toggle chips and tags with suggestions, so no
//! value starts from a blank field (S065).

use egui::{
    CornerRadius, CursorIcon, Frame, Id, Margin, Pos2, Rect, Response, Sense, Stroke, StrokeKind,
    TextEdit, Ui, UiBuilder, Vec2,
};

use crate::icons::{self, Icon};
use crate::{Face, palette, widgets};

/// The height of a control.
const H: f32 = 28.0;

/// `v` on the step's grid from `min`, within `min` and `max`.
fn snap(v: f64, min: f64, max: f64, step: f64) -> f64 {
    let on = if step > 0.0 {
        ((v - min) / step).round() * step + min
    } else {
        v
    };
    (on.clamp(min, max) * 1e6).round() / 1e6
}

/// `v` with as many decimals as `step` has.
pub fn number(v: f64, step: f64) -> String {
    let decimals = if step >= 1.0 || step <= 0.0 {
        0
    } else {
        (-step.log10()).ceil() as usize
    };
    format!("{v:.decimals$}")
}

/// A box the number is typed into: what it shows while not focused, and
/// what was typed while it is (kept by `id`). The typed number when it was
/// entered.
fn number_box(
    ui: &mut Ui,
    id: Id,
    shown: &str,
    width: f32,
    frame: bool,
) -> (Response, Option<f64>) {
    let p = palette(ui.ctx());
    let (rect, frame_response) = ui.allocate_exact_size(Vec2::new(width, H - 2.0), Sense::click());
    let mut text: String = ui
        .data(|d| d.get_temp(id))
        .unwrap_or_else(|| shown.to_owned());
    let font = Face::Plex.at(12.5);
    // (one row of the font, centred in the box, as forms::text_input)
    let row = ui.fonts_mut(|f| f.row_height(&font));
    let inner = Rect::from_center_size(rect.center(), Vec2::new(rect.width() - 16.0, row));
    let mut child = ui.new_child(UiBuilder::new().max_rect(inner));
    let output = TextEdit::singleline(&mut text)
        .id(id)
        .frame(Frame::NONE)
        .margin(Margin::ZERO)
        .font(font)
        .text_color(p.ink)
        // (in the middle, at the top of the row: the theme makes a text
        // edit taller than the row, and centring it up and down would put
        // the text under the row's middle)
        .align(egui::Align2::CENTER_TOP)
        .desired_width(inner.width())
        .show(&mut child);
    let response = output.response.response;
    if frame_response.clicked() {
        response.request_focus();
    }
    let focused = response.has_focus();
    if frame {
        let border = if focused { p.amber_ink } else { p.line_strong };
        ui.painter().rect_stroke(
            rect,
            CornerRadius::ZERO,
            Stroke::new(1.0, border),
            StrokeKind::Inside,
        );
    }
    let entered = response.lost_focus();
    let typed = entered
        .then(|| {
            text.trim()
                .trim_end_matches(|c: char| !c.is_ascii_digit() && c != '.')
                .trim()
                .parse::<f64>()
                .ok()
        })
        .flatten();
    if focused {
        ui.data_mut(|d| d.insert_temp(id, text));
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
    }
    (response, typed)
}

/// A number on a slider from `min` to `max` in `step`s, the value in a box
/// beside it (shown times `scale`, then `suffix`: 0.8 as `80%`) and `ticks`
/// under it. True when it changed.
#[allow(clippy::too_many_arguments)]
pub fn slider(
    ui: &mut Ui,
    id: &str,
    value: &mut f64,
    min: f64,
    max: f64,
    step: f64,
    scale: f64,
    suffix: &str,
    ticks: &[(f64, &str)],
    width: f32,
) -> bool {
    let p = palette(ui.ctx());
    let enabled = ui.is_enabled();
    let id = Id::unique(("wowlua-studio-slider", id));
    let mut changed = false;
    let track_w = (width - 72.0 - 10.0).max(80.0);
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(track_w, 20.0), Sense::click_and_drag());
            if enabled
                && let Some(pos) = response.interact_pointer_pos()
                && (response.dragged() || response.clicked())
            {
                let t = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64;
                let v = snap(min + t * (max - min), min, max, step);
                if (v - *value).abs() > 1e-9 {
                    *value = v;
                    changed = true;
                }
            }
            let t = if max > min {
                ((*value - min) / (max - min)).clamp(0.0, 1.0) as f32
            } else {
                0.0
            };
            let painter = ui.painter();
            let cy = rect.center().y;
            let x = rect.left() + t * rect.width();
            let (fill, rest) = if enabled {
                (p.amber_ink, p.line_strong)
            } else {
                (p.ink_faint, p.line)
            };
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(rect.left(), cy - 2.0), Pos2::new(x, cy + 2.0)),
                CornerRadius::ZERO,
                fill,
            );
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(x, cy - 2.0), Pos2::new(rect.right(), cy + 2.0)),
                CornerRadius::ZERO,
                rest,
            );
            let thumb = Rect::from_center_size(Pos2::new(x, cy), Vec2::new(10.0, 16.0));
            painter.rect_filled(
                thumb,
                CornerRadius::ZERO,
                if enabled { p.amber } else { p.ink_faint },
            );
            painter.rect_stroke(
                thumb,
                CornerRadius::ZERO,
                Stroke::new(1.0, p.bg_000),
                StrokeKind::Inside,
            );
            widgets::focus_ring(ui, rect, &response);
            if enabled {
                response.on_hover_cursor(CursorIcon::PointingHand);
            }
            let shown = format!("{}{suffix}", number(*value * scale, step * scale));
            let (_, typed) = number_box(ui, id.with("value"), &shown, 72.0, true);
            if let Some(n) = typed {
                let v = snap(n / scale, min, max, step);
                if (v - *value).abs() > 1e-9 {
                    *value = v;
                    changed = true;
                }
            }
        });
        if !ticks.is_empty() {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(track_w, 14.0), Sense::hover());
            for (at, label) in ticks {
                let t = if max > min {
                    ((at - min) / (max - min)).clamp(0.0, 1.0) as f32
                } else {
                    0.0
                };
                let align = if t <= 0.01 {
                    egui::Align2::LEFT_TOP
                } else if t >= 0.99 {
                    egui::Align2::RIGHT_TOP
                } else {
                    egui::Align2::CENTER_TOP
                };
                ui.painter().text(
                    Pos2::new(rect.left() + t * rect.width(), rect.top()),
                    align,
                    label,
                    Face::Plex.at(10.5),
                    p.ink_muted,
                );
            }
        }
    });
    changed
}

/// A number with − and + (one `step` each) and its `unit` after it,
/// typed too; within `min` and `max` when given. True when it changed.
pub fn stepper(
    ui: &mut Ui,
    id: &str,
    value: &mut f64,
    min: Option<f64>,
    max: Option<f64>,
    step: f64,
    unit: &str,
) -> bool {
    let p = palette(ui.ctx());
    let id = Id::unique(("wowlua-studio-stepper", id));
    let lo = min.unwrap_or(f64::MIN);
    let hi = max.unwrap_or(f64::MAX);
    let mut changed = false;
    let unit_w = if unit.is_empty() {
        0.0
    } else {
        ui.fonts_mut(|f| {
            f.layout_no_wrap(unit.to_owned(), Face::Plex.at(11.5), p.ink_muted)
                .size()
                .x
        }) + 10.0
    };
    let width = 26.0 + 52.0 + unit_w + 26.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, H), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_000);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, p.line_strong),
        StrokeKind::Inside,
    );
    let button = |ui: &mut Ui, r: Rect, icon: Icon, label: &str, enabled: bool| {
        let response = ui.interact(r, id.with(label), Sense::click());
        let enabled = enabled && ui.is_enabled();
        let ink = if !enabled {
            p.ink_faint
        } else if response.hovered() {
            ui.painter().rect_filled(r, CornerRadius::ZERO, p.bg_300);
            p.ink
        } else {
            p.ink_muted
        };
        icons::paint(
            ui.painter(),
            Rect::from_center_size(r.center(), Vec2::splat(12.0)),
            icon,
            ink,
        );
        enabled && response.clicked()
    };
    let minus = Rect::from_min_size(rect.min, Vec2::new(26.0, H));
    let plus = Rect::from_min_size(
        Pos2::new(rect.right() - 26.0, rect.top()),
        Vec2::new(26.0, H),
    );
    if button(ui, minus, Icon::Subtract, "less", *value > lo) {
        *value = (*value - step).max(lo);
        changed = true;
    }
    if button(ui, plus, Icon::Add, "more", *value < hi) {
        *value = (*value + step).min(hi);
        changed = true;
    }
    let middle = Rect::from_min_max(
        Pos2::new(minus.right(), rect.top()),
        Pos2::new(plus.left(), rect.bottom()),
    );
    for x in [middle.left(), middle.right()] {
        ui.painter()
            .vline(x, rect.y_range(), Stroke::new(1.0, p.line));
    }
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(middle)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let shown = number(*value, step);
    let (_, typed) = number_box(&mut child, id.with("value"), &shown, 52.0, false);
    if !unit.is_empty() {
        child.label(
            egui::RichText::new(unit)
                .font(Face::Plex.at(11.5))
                .color(p.ink_muted),
        );
    }
    if let Some(n) = typed {
        let v = n.clamp(lo, hi);
        if (v - *value).abs() > 1e-9 {
            *value = v;
            changed = true;
        }
    }
    changed
}

/// A chip that is on or off (the Controls board's toggle chip): a small
/// square, amber when on.
pub fn chip(ui: &mut Ui, label: &str, on: bool) -> Response {
    let p = palette(ui.ctx());
    let font = Face::Plex.at(12.0);
    let text_w = ui.fonts_mut(|f| {
        f.layout_no_wrap(label.to_owned(), font.clone(), p.ink)
            .size()
            .x
    });
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(8.0 + 8.0 + 6.0 + text_w + 8.0, 24.0),
        Sense::click(),
    );
    let enabled = ui.is_enabled();
    let hovered = enabled && response.hovered();
    let painter = ui.painter();
    let (border, fill, ink) = if on {
        (p.amber_ink, p.amber_soft, p.ink)
    } else if hovered {
        (p.line_strong, egui::Color32::TRANSPARENT, p.ink)
    } else {
        (p.line, egui::Color32::TRANSPARENT, p.ink_muted)
    };
    let ink = if enabled { ink } else { p.ink_faint };
    painter.rect_filled(rect, CornerRadius::ZERO, fill);
    painter.rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, border),
        StrokeKind::Inside,
    );
    let square = Rect::from_min_size(
        Pos2::new(rect.left() + 8.0, rect.center().y - 4.0),
        Vec2::splat(8.0),
    );
    if on {
        painter.rect_filled(square, CornerRadius::ZERO, p.amber_ink);
    } else {
        painter.rect_stroke(
            square,
            CornerRadius::ZERO,
            Stroke::new(1.0, p.line_strong),
            StrokeKind::Inside,
        );
    }
    painter.text(
        Pos2::new(square.right() + 6.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        font,
        ink,
    );
    widgets::focus_ring(ui, rect, &response);
    if enabled {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response
    }
}

/// Presets beside a control: the one `current` is, on. The value picked.
pub fn presets(ui: &mut Ui, options: &[(&str, f64)], current: f64) -> Option<f64> {
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (label, v) in options {
            if chip(ui, label, (current - v).abs() < 1e-9).clicked() {
                picked = Some(*v);
            }
        }
    });
    picked
}

/// Several of a known set (reporters, flavors): each a chip. True when the
/// selection changed.
pub fn toggle_chips(ui: &mut Ui, options: &[&str], selected: &mut Vec<String>) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for o in options {
            let on = selected.iter().any(|s| s == o);
            if chip(ui, o, on).clicked() {
                if on {
                    selected.retain(|s| s != o);
                } else {
                    selected.push((*o).to_owned());
                }
                changed = true;
            }
        }
    });
    changed
}

/// A list of strings as tags (globs, saved variables): each removed with
/// its ×, one added by typing it and Enter (or a comma); `meta` says
/// something of each (a glob's match count). True when the list changed.
pub fn tags(
    ui: &mut Ui,
    id: &str,
    items: &mut Vec<String>,
    placeholder: &str,
    width: f32,
    meta: &dyn Fn(&str) -> Option<(String, bool)>,
) -> bool {
    let p = palette(ui.ctx());
    let id = Id::unique(("wowlua-studio-tags", id));
    let mut changed = false;
    let mut remove = None;
    let mut typed: String = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let mut focused = false;
    let frame = Frame::NONE.fill(p.bg_000).inner_margin(Margin::same(3));
    let inner = frame.show(ui, |ui| {
        ui.set_width(width - 8.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
            for (i, item) in items.iter().enumerate() {
                let info = meta(item);
                let bad = info.as_ref().is_some_and(|(_, bad)| *bad);
                Frame::NONE
                    .fill(p.bg_300)
                    .stroke(Stroke::new(1.0, if bad { p.error } else { p.line }))
                    .inner_margin(Margin {
                        left: 7,
                        right: 2,
                        top: 2,
                        bottom: 2,
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            // (a tag no wider than the field: a long one is
                            // cut with …, its whole text under the pointer)
                            let text_w = ui.fonts_mut(|f| {
                                f.layout_no_wrap(item.clone(), Face::Plex.at(12.0), p.ink)
                                    .size()
                                    .x
                            });
                            let room = (width - 8.0 - 7.0 - 2.0 - 6.0 - 18.0 - 8.0).max(40.0);
                            ui.allocate_ui(Vec2::new(text_w.min(room), 18.0), |ui| {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(item)
                                            .font(Face::Plex.at(12.0))
                                            .color(p.ink),
                                    )
                                    .truncate(),
                                );
                            });
                            if let Some((m, _)) = &info {
                                let c = if bad { p.error } else { p.ink_muted };
                                ui.label(egui::RichText::new(m).font(Face::Plex.at(11.0)).color(c));
                            }
                            let (r, x) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
                            let ink = if x.hovered() {
                                ui.painter().rect_filled(r, CornerRadius::ZERO, p.bg_400);
                                p.ink
                            } else {
                                p.ink_muted
                            };
                            icons::paint(
                                ui.painter(),
                                Rect::from_center_size(r.center(), Vec2::splat(10.0)),
                                Icon::Close,
                                ink,
                            );
                            if x.clicked() {
                                remove = Some(i);
                            }
                        });
                    });
            }
            // The field to type in: one cell as tall as a tag, its text
            // and hint centred on it, in from the edge as in every field.
            let cell = Vec2::new(90.0_f32.max(ui.available_width() - 4.0), 22.0);
            let down = egui::Layout::left_to_right(egui::Align::Center);
            let row = ui.fonts_mut(|f| f.row_height(&Face::Plex.at(12.0)));
            let response = ui
                .allocate_ui_with_layout(cell, down, |ui| {
                    ui.set_min_size(cell);
                    ui.add_space(6.0);
                    TextEdit::singleline(&mut typed)
                        .id(id.with("input"))
                        .frame(Frame::NONE)
                        .margin(Margin::ZERO)
                        .min_size(Vec2::new(cell.x - 10.0, row))
                        .font(Face::Plex.at(12.0))
                        .text_color(p.ink)
                        .hint_text(
                            egui::RichText::new(placeholder)
                                .font(Face::Plex.at(12.0))
                                .color(p.ink_muted),
                        )
                        .desired_width(cell.x - 10.0)
                        .show(ui)
                        .response
                        .response
                })
                .inner;
            focused = response.has_focus();
            let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let comma = typed.ends_with(',');
            if (enter || comma) && !typed.trim_end_matches(',').trim().is_empty() {
                let new = typed.trim_end_matches(',').trim().to_owned();
                if !items.contains(&new) {
                    items.push(new);
                    changed = true;
                }
                typed.clear();
                if comma {
                    response.request_focus();
                }
            }
            if focused
                && typed.is_empty()
                && ui.input(|i| i.key_pressed(egui::Key::Backspace))
                && !items.is_empty()
            {
                items.pop();
                changed = true;
            }
        });
    });
    let border = if focused { p.amber_ink } else { p.line_strong };
    ui.painter().rect_stroke(
        inner.response.rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, border),
        StrokeKind::Inside,
    );
    if let Some(i) = remove {
        items.remove(i);
        changed = true;
    }
    ui.data_mut(|d| d.insert_temp(id, typed));
    changed
}

/// Suggestions under a control (`Media/**` and the like): each a chip
/// that adds itself. The one picked.
pub fn suggestions(ui: &mut Ui, options: &[String]) -> Option<String> {
    let p = palette(ui.ctx());
    let mut picked = None;
    if options.is_empty() {
        return None;
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(
            egui::RichText::new("add")
                .font(Face::Plex.at(11.5))
                .color(p.ink_muted),
        );
        for o in options {
            let font = Face::Plex.at(11.5);
            let w =
                ui.fonts_mut(|f| f.layout_no_wrap(o.clone(), font.clone(), p.ink).size().x) + 12.0;
            let (rect, response) = ui.allocate_exact_size(Vec2::new(w, 20.0), Sense::click());
            let (border, ink) = if response.hovered() {
                (p.amber_ink, p.amber_ink)
            } else {
                (p.line_strong, p.ink)
            };
            ui.painter().rect_stroke(
                rect,
                CornerRadius::ZERO,
                Stroke::new(1.0, border),
                StrokeKind::Inside,
            );
            ui.painter()
                .text(rect.center(), egui::Align2::CENTER_CENTER, o, font, ink);
            if response.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                picked = Some(o.clone());
            }
        }
    });
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_snap_to_their_steps() {
        assert_eq!(snap(0.8349, 0.65, 1.15, 0.01), 0.83);
        assert_eq!(snap(2.0, 0.65, 1.15, 0.01), 1.15);
        assert_eq!(snap(76.3, 0.0, 100.0, 0.5), 76.5);
        assert_eq!(number(0.8, 0.01), "0.80");
        assert_eq!(number(80.0, 1.0), "80");
    }
}
