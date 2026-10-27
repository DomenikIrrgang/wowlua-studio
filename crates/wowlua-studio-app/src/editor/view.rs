//! Drawing the editor and taking its input (the design's editor board):
//! the tabs, the path, the changed-on-disk banner, and the code with its
//! gutter. Only the lines on screen are laid out; positions come from each
//! line's laid-out text, so wide characters and tabs place the caret right.

use std::sync::Arc;

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, Event, EventFilter, FontId, Galley, Id, Key,
    Modifiers, Pos2, Rect, Sense, Stroke, Ui, UiBuilder, Vec2,
    epaint::text::cursor::CCursor,
    text::{LayoutJob, TextFormat},
};
use wowlua_studio_ui::forms::{self, CalloutKind};
use wowlua_studio_ui::widgets::{self, ButtonKind};
use wowlua_studio_ui::{Face, Icon, Palette, icons, palette};

use super::diff;
use super::doc::{Document, EditKind, Pos, Selection};
use super::{Editors, Tab};
use crate::system::{Syntax, System, Workspace};

pub const LINE_H: f32 = 21.0;
const PAD_TOP: f32 = 6.0;
/// The breakpoint column and the line numbers.
const GUTTER: f32 = 20.0 + 46.0;
const TABS_H: f32 = 34.0;
const CRUMBS_H: f32 = 26.0;
const TAB_STOP: usize = 4;
const FONT_SIZE: f32 = 13.0;

/// What the editor asks Studio to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Save(usize),
    /// Close the tab; Studio asks first when it has unsaved edits.
    Close(usize),
}

/// What the editor needs from Studio.
pub struct Cx<'a> {
    pub system: &'a dyn System,
    pub workspace: &'a Workspace,
    pub mac: bool,
}

/// A line as it is drawn: tabs to the next stop of four. With it each
/// character's display column, and the end's.
pub fn display(line: &str) -> (String, Vec<usize>) {
    let mut out = String::with_capacity(line.len());
    let mut map = Vec::with_capacity(line.len() + 1);
    let mut col = 0;
    for c in line.chars() {
        map.push(col);
        if c == '\t' {
            let n = TAB_STOP - col % TAB_STOP;
            out.extend(std::iter::repeat_n(' ', n));
            col += n;
        } else {
            out.push(c);
            col += 1;
        }
    }
    map.push(col);
    (out, map)
}

/// Draws the editor into the panel.
pub fn show(ui: &mut Ui, editors: &mut Editors, cx: &Cx<'_>) -> Vec<Action> {
    let p = palette(ui.ctx());
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_000);
    let mut actions = Vec::new();
    if editors.tabs.is_empty() {
        let text = "Open a file from the explorer.";
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            text,
            Face::Plex.at(12.5),
            p.ink_muted,
        );
        return actions;
    }
    editors.active = editors.active.min(editors.tabs.len() - 1);

    let tabs_rect = Rect::from_min_size(rect.min, Vec2::new(rect.width(), TABS_H));
    tab_bar(ui, tabs_rect, editors, &mut actions);
    let crumbs_rect = Rect::from_min_size(
        Pos2::new(rect.left(), tabs_rect.bottom()),
        Vec2::new(rect.width(), CRUMBS_H),
    );
    let focus = std::mem::take(&mut editors.focus);
    let active = editors.active;
    let tab = &mut editors.tabs[active];
    crumbs(ui, crumbs_rect, &tab.doc, cx.workspace);

    let mut top = crumbs_rect.bottom();
    if tab.clash.is_some() {
        let banner = Rect::from_min_max(
            Pos2::new(rect.left(), top),
            Pos2::new(rect.right(), rect.bottom()),
        );
        let mut child = ui.new_child(UiBuilder::new().max_rect(banner));
        clash_banner(&mut child, tab);
        top = child.min_rect().bottom();
    }
    let code_rect = Rect::from_min_max(Pos2::new(rect.left(), top), rect.max);
    if let Some(clash) = &tab.clash
        && clash.comparing
    {
        let mine = tab.doc.text();
        let theirs = clash.theirs.replace("\r\n", "\n");
        compare(ui, code_rect, &mine, &theirs, &tab.doc.path);
        return actions;
    }
    code(ui, code_rect, tab, cx, focus, active, &mut actions);
    actions
}

fn tab_bar(ui: &mut Ui, rect: Rect, editors: &mut Editors, actions: &mut Vec<Action>) {
    let p = palette(ui.ctx());
    let painter = ui.painter().clone();
    painter.rect_filled(rect, CornerRadius::ZERO, p.bg_200);
    painter.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, p.line),
    );
    let mut x = rect.left();
    let clip = painter.with_clip_rect(rect);
    for (i, tab) in editors.tabs.iter().enumerate() {
        let name = file_name(&tab.doc);
        let on = i == editors.active;
        let g = widgets::galley(ui, &name, Face::Plex.at(12.0), p.ink, 0.0);
        let w = 14.0 + 14.0 + 8.0 + g.size().x + 8.0 + 16.0 + 14.0;
        let r = Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(w, TABS_H - 1.0));
        let resp = ui.interact(
            r,
            Id::unique(("wowlua-studio-etab", &tab.doc.path)),
            Sense::click(),
        );
        let hovered = resp.hovered();
        let (bg, fg) = if on {
            (p.bg_000, p.ink)
        } else if hovered {
            (p.bg_300, p.ink)
        } else {
            (Color32::TRANSPARENT, p.ink_muted)
        };
        clip.rect_filled(r, CornerRadius::ZERO, bg);
        clip.vline(r.right() - 0.5, r.y_range(), Stroke::new(1.0, p.line));
        let cy = r.center().y;
        let (icon, color) = super::super::explorer::file_icon(&tab.doc.path, &name, p);
        icons::paint(
            &clip,
            Rect::from_min_size(Pos2::new(r.left() + 14.0, cy - 7.0), Vec2::splat(14.0)),
            icon,
            color,
        );
        clip.galley(
            Pos2::new(r.left() + 36.0, cy - g.size().y / 2.0),
            g.clone(),
            fg,
        );
        // The close button, or the dot of unsaved edits.
        let close =
            Rect::from_center_size(Pos2::new(r.right() - 14.0 - 8.0, cy), Vec2::splat(16.0));
        let close_resp = ui.interact(
            close,
            Id::unique(("wowlua-studio-etab-close", &tab.doc.path)),
            Sense::click(),
        );
        if tab.doc.is_dirty() && !close_resp.hovered() {
            clip.circle_filled(close.center(), 3.5, fg);
        } else if on || hovered || close_resp.hovered() {
            if close_resp.hovered() {
                clip.rect_filled(close, CornerRadius::ZERO, p.bg_400);
            }
            icons::paint(&clip, close.shrink(2.0), Icon::Close, fg);
        }
        if close_resp.clicked() || resp.middle_clicked() {
            actions.push(Action::Close(i));
        } else if resp.clicked() {
            editors.active = i;
            editors.focus = true;
        }
        resp.on_hover_text(tab.doc.path.to_string_lossy());
        x = r.right();
    }
}

fn file_name(doc: &Document) -> String {
    doc.path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The path: the package, its folders, the file (`st-crumbs`).
fn crumbs(ui: &Ui, rect: Rect, doc: &Document, workspace: &Workspace) {
    let p = palette(ui.ctx());
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, CornerRadius::ZERO, p.bg_000);
    painter.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, p.line),
    );
    // The member holding the file: the deepest folder that contains it.
    let member = workspace
        .members
        .iter()
        .filter(|m| doc.path.starts_with(&m.dir))
        .max_by_key(|m| m.dir.components().count());
    let mut parts: Vec<String> = Vec::new();
    let rel = match member {
        Some(m) => {
            parts.push(m.name.clone());
            doc.path.strip_prefix(&m.dir).unwrap_or(&doc.path)
        }
        None => doc.path.strip_prefix(&workspace.root).unwrap_or(&doc.path),
    };
    parts.extend(rel.iter().map(|c| c.to_string_lossy().into_owned()));
    let mut job = LayoutJob::default();
    let font = Face::Plex.at(11.5);
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            job.append(" › ", 0.0, TextFormat::simple(font.clone(), p.ink_muted));
        }
        let last = i + 1 == parts.len();
        let (f, c) = if last {
            (Face::PlexMedium.at(11.5), p.ink)
        } else {
            (font.clone(), p.ink_muted)
        };
        job.append(part, 0.0, TextFormat::simple(f, c));
    }
    let g = ui.fonts_mut(|f| f.layout_job(job));
    painter.galley(
        Pos2::new(rect.left() + 12.0, rect.center().y - g.size().y / 2.0),
        g,
        p.ink,
    );
}

/// The file changed on disk while it had unsaved edits (S049).
fn clash_banner(ui: &mut Ui, tab: &mut Tab) {
    let p = palette(ui.ctx());
    let Some(clash) = &tab.clash else {
        return;
    };
    let name = file_name(&tab.doc);
    let text = if clash.on_save {
        format!("{name} changed on disk since Studio read it; saving would write over it.")
    } else if clash.checkout {
        format!("{name} changed on disk (a branch was checked out) while you had unsaved edits.")
    } else {
        format!("{name} changed on disk while you had unsaved edits.")
    };
    let comparing = clash.comparing;
    let mut choice = None;
    forms::callout(ui, CalloutKind::Note, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let width = (ui.available_width() - 330.0).max(200.0);
            ui.allocate_ui(Vec2::new(width, 28.0), |ui| {
                let g = rich_note(ui, &text, width, p);
                let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
                ui.painter().galley(r.min, g, p.ink);
            });
            let compare = if comparing {
                "back to editing"
            } else {
                "compare"
            };
            if widgets::button(ui, ButtonKind::Secondary, compare, true, None).clicked() {
                choice = Some(0);
            }
            if widgets::button(ui, ButtonKind::Ghost, "keep mine", true, None).clicked() {
                choice = Some(1);
            }
            if widgets::button(ui, ButtonKind::Ghost, "take theirs", true, None).clicked() {
                choice = Some(2);
            }
        });
    });
    match choice {
        Some(0) => {
            if let Some(c) = &mut tab.clash {
                c.comparing = !c.comparing;
            }
        }
        Some(1) => tab.keep_mine(),
        Some(2) => tab.take_theirs(),
        _ => {}
    }
}

fn rich_note(ui: &Ui, text: &str, width: f32, p: &Palette) -> Arc<Galley> {
    let mut job = LayoutJob::default();
    job.append(
        "note: ",
        0.0,
        TextFormat::simple(Face::PlexSemiBold.at(12.0), p.note),
    );
    job.append(text, 0.0, TextFormat::simple(Face::Plex.at(12.0), p.ink));
    job.wrap.max_width = width;
    ui.fonts_mut(|f| f.layout_job(job))
}

/// Mine against the disk's, read-only: removed lines are mine, added
/// lines theirs (`st-ln--del`, `st-ln--add`).
fn compare(ui: &mut Ui, rect: Rect, mine: &str, theirs: &str, path: &std::path::Path) {
    let p = palette(ui.ctx());
    let font = Face::Plex.at(FONT_SIZE);
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
    child.set_clip_rect(rect);
    let Some(lines) = diff::lines(mine, theirs) else {
        child.painter().text(
            rect.left_top() + Vec2::new(16.0, 16.0),
            Align2::LEFT_TOP,
            "The two differ too much to compare here.",
            Face::Plex.at(12.5),
            p.ink_muted,
        );
        return;
    };
    egui::ScrollArea::both()
        .id_salt(("wowlua-studio-compare", path))
        .auto_shrink(false)
        .show_rows(&mut child, LINE_H, lines.len(), |ui, range| {
            for line in &lines[range] {
                let (sign, text, bg) = match line {
                    diff::Line::Same(t) => (" ", *t, Color32::TRANSPARENT),
                    diff::Line::Removed(t) => ("-", *t, p.error_soft),
                    diff::Line::Added(t) => ("+", *t, p.success_soft),
                };
                let (shown, _) = display(text);
                let width = ui.available_width().max(rect.width());
                let (r, _) = ui.allocate_exact_size(Vec2::new(width, LINE_H), Sense::hover());
                ui.painter().rect_filled(r, CornerRadius::ZERO, bg);
                ui.painter().text(
                    Pos2::new(r.left() + 12.0, r.center().y),
                    Align2::LEFT_CENTER,
                    sign,
                    font.clone(),
                    p.ink_muted,
                );
                ui.painter().text(
                    Pos2::new(r.left() + 32.0, r.center().y),
                    Align2::LEFT_CENTER,
                    shown,
                    font.clone(),
                    p.ink,
                );
            }
        });
}

/// The code: the gutter and the visible lines; the keyboard and pointer.
fn code(
    ui: &mut Ui,
    rect: Rect,
    tab: &mut Tab,
    cx: &Cx<'_>,
    focus: bool,
    index: usize,
    actions: &mut Vec<Action>,
) {
    let p = palette(ui.ctx());
    let ctx = ui.ctx().clone();
    let id = Id::unique(("wowlua-studio-editor", &tab.doc.path));
    let font = Face::Plex.at(FONT_SIZE);
    let char_w = ui.fonts_mut(|f| f.glyph_width(&font, 'M'));
    let text_rect = Rect::from_min_max(Pos2::new(rect.left() + GUTTER, rect.top()), rect.max);

    if focus {
        ctx.memory_mut(|m| m.request_focus(id));
    }
    let has_focus = ctx.memory(|m| m.has_focus(id));
    if has_focus {
        ctx.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: false,
                },
            );
        });
        let changed = keyboard(ui, &mut tab.doc, cx.mac, (rect.height() / LINE_H) as usize);
        if changed.save {
            actions.push(Action::Save(index));
        }
        if changed.moved {
            tab.reveal = true;
            let now = ctx.input(|i| i.time);
            ctx.data_mut(|d| d.insert_temp(id.with("typed"), now));
        }
    }

    let widest = tab.widest();
    tab.highlights(cx.system);
    let highlights: &[Vec<(usize, usize, Syntax)>] =
        tab.highlights.as_ref().map_or(&[], |(_, h)| h.as_slice());
    let doc = &mut tab.doc;
    let lines = doc.line_count();
    let content = Vec2::new(
        GUTTER.max(widest as f32 * char_w + 48.0),
        PAD_TOP + lines as f32 * LINE_H + 24.0 + rect.height() * 0.5,
    );
    let mut child = ui.new_child(UiBuilder::new().max_rect(text_rect));
    child.set_clip_rect(text_rect);
    let reveal_now = std::mem::take(&mut tab.reveal);
    let mut reveal = reveal_now;
    let mut caret_screen = None;
    let output = egui::ScrollArea::both()
        .id_salt(("wowlua-studio-code", &doc.path))
        .auto_shrink(false)
        .show_viewport(&mut child, |ui, viewport| {
            ui.set_min_size(content);
            let origin = ui.min_rect().min;
            let full = Rect::from_min_size(origin, content.max(text_rect.size()));
            let response = ui.interact(full, id, Sense::click_and_drag());
            if response.hovered() {
                ctx.set_cursor_icon(CursorIcon::Text);
            }
            let first = (((viewport.min.y - PAD_TOP) / LINE_H).floor().max(0.0)) as usize;
            let last = ((((viewport.max.y - PAD_TOP) / LINE_H).ceil()) as usize + 1).min(lines);
            let line_y = |line: usize| origin.y + PAD_TOP + line as f32 * LINE_H;
            let galley_of = |ui: &Ui, text: &str, line: usize| {
                layout(
                    ui,
                    text,
                    highlights.get(line).map_or(&[][..], |h| h.as_slice()),
                    &font,
                    p,
                )
            };

            // The pointer: place the caret, select by dragging, words and
            // lines by double and triple clicks.
            if let Some(pointer) = response.interact_pointer_pos() {
                let line = (((pointer.y - origin.y - PAD_TOP) / LINE_H).floor().max(0.0) as usize)
                    .min(lines - 1);
                let g = galley_of(ui, doc.line(line), line);
                let at = Pos::new(
                    line,
                    char_col(
                        doc.line(line),
                        g.cursor_from_pos(Vec2::new(pointer.x - origin.x, 0.0))
                            .index
                            .0,
                    ),
                );
                let pressed = ui.input(|i| i.pointer.primary_pressed());
                let shift = ui.input(|i| i.modifiers.shift);
                if pressed && response.is_pointer_button_down_on() {
                    ctx.memory_mut(|m| m.request_focus(id));
                    doc.selection = if shift {
                        Selection {
                            anchor: doc.selection.anchor,
                            head: at,
                        }
                    } else {
                        Selection::caret(at)
                    };
                    doc.goal_col = None;
                } else if response.dragged() {
                    doc.selection.head = at;
                    reveal = true;
                }
                if response.triple_clicked() {
                    let end = if line + 1 < lines {
                        Pos::new(line + 1, 0)
                    } else {
                        Pos::new(line, doc.line_len(line))
                    };
                    doc.selection = Selection {
                        anchor: Pos::new(line, 0),
                        head: end,
                    };
                } else if response.double_clicked() {
                    let (start, end) = word_at(doc.line(line), at.col);
                    doc.selection = Selection {
                        anchor: Pos::new(line, start),
                        head: Pos::new(line, end),
                    };
                }
                let now = ctx.input(|i| i.time);
                ctx.data_mut(|d| d.insert_temp(id.with("typed"), now));
            }

            let (sel_start, sel_end) = doc.selection.range();
            let head = doc.selection.head;
            for line in first..last {
                let y = line_y(line);
                let g = galley_of(ui, doc.line(line), line);
                let row = Rect::from_min_size(
                    Pos2::new(viewport.min.x + origin.x, y),
                    Vec2::new(viewport.width().max(full.width()), LINE_H),
                );
                if line == head.line && doc.selection.is_empty() {
                    ui.painter().rect_filled(row, CornerRadius::ZERO, p.bg_100);
                }
                // The selection on this line.
                if line >= sel_start.line && line <= sel_end.line && sel_start != sel_end {
                    let from = if line == sel_start.line {
                        sel_start.col
                    } else {
                        0
                    };
                    let to_end = line < sel_end.line;
                    let to = if to_end {
                        doc.line_len(line)
                    } else {
                        sel_end.col
                    };
                    let x0 = x_of(&g, doc.line(line), from);
                    let mut x1 = x_of(&g, doc.line(line), to);
                    if to_end {
                        x1 += char_w * 0.6;
                    }
                    if x1 > x0 {
                        ui.painter().rect_filled(
                            Rect::from_min_max(
                                Pos2::new(origin.x + x0, y),
                                Pos2::new(origin.x + x1, y + LINE_H),
                            ),
                            CornerRadius::ZERO,
                            p.amber_soft,
                        );
                    }
                }
                let text_y = y + (LINE_H - g.size().y) / 2.0;
                ui.painter()
                    .galley(Pos2::new(origin.x, text_y), g.clone(), p.ink);
                if line == head.line {
                    let x = origin.x + x_of(&g, doc.line(line), head.col);
                    caret_screen = Some(Rect::from_min_size(
                        Pos2::new(x, y + 2.0),
                        Vec2::new(2.0, LINE_H - 4.0),
                    ));
                }
            }
            if reveal {
                let g = galley_of(ui, doc.line(head.line), head.line);
                let x = origin.x + x_of(&g, doc.line(head.line), head.col);
                let y = line_y(head.line);
                ui.scroll_to_rect(
                    Rect::from_min_size(
                        Pos2::new(x - 24.0, y - LINE_H),
                        Vec2::new(48.0, LINE_H * 3.0),
                    ),
                    None,
                );
            }
        });

    // The caret: 2 px of amber, blinking while nothing is typed.
    if has_focus && let Some(caret) = caret_screen {
        let now = ctx.input(|i| i.time);
        let typed: f64 = ctx.data(|d| d.get_temp(id.with("typed"))).unwrap_or(now);
        let phase = (now - typed) % 1.0;
        if phase < 0.6 && text_rect.intersects(caret) {
            ui.painter()
                .with_clip_rect(text_rect)
                .rect_filled(caret, CornerRadius::ZERO, p.amber);
        }
        let next = if phase < 0.6 {
            0.6 - phase
        } else {
            1.0 - phase
        };
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(next));
        ctx.output_mut(|o| {
            o.ime = Some(egui::output::IMEOutput {
                purpose: egui::IMEPurpose::Normal,
                rect: text_rect,
                cursor_rect: caret,
                should_interrupt_composition: false,
            });
        });
    }

    // The gutter, beside the scrolled lines.
    let offset = output.state.clamped_offset().y;
    let gutter = Rect::from_min_max(rect.min, Pos2::new(rect.left() + GUTTER, rect.bottom()));
    let painter = ui.painter().with_clip_rect(gutter);
    painter.rect_filled(gutter, CornerRadius::ZERO, p.bg_000);
    let first = ((offset - PAD_TOP) / LINE_H).floor().max(0.0) as usize;
    let last = ((offset + rect.height()) / LINE_H).ceil() as usize + 1;
    let number_font = Face::Plex.at(FONT_SIZE);
    for line in first..last.min(lines) {
        let y = rect.top() + PAD_TOP + line as f32 * LINE_H - offset + LINE_H / 2.0;
        let color = if line == doc_head_line(tab) {
            p.ink
        } else {
            p.ink_muted
        };
        painter.text(
            Pos2::new(gutter.right() - 16.0, y),
            Align2::RIGHT_CENTER,
            (line + 1).to_string(),
            number_font.clone(),
            color,
        );
    }
}

fn doc_head_line(tab: &Tab) -> usize {
    tab.doc.selection.head.line
}

/// A line laid out in its colours.
fn layout(
    ui: &Ui,
    line: &str,
    highlights: &[(usize, usize, Syntax)],
    font: &FontId,
    p: &Palette,
) -> Arc<Galley> {
    let (shown, _) = display(line);
    let mut job = LayoutJob::default();
    let plain = TextFormat::simple(font.clone(), p.ink);
    let mut at = 0;
    let byte = |col: usize| {
        shown
            .char_indices()
            .nth(col)
            .map_or(shown.len(), |(b, _)| b)
    };
    for &(from, to, syntax) in highlights {
        let (from, to) = (byte(from.max(at)), byte(to));
        let at_b = byte(at);
        if from > at_b {
            job.append(&shown[at_b..from], 0.0, plain.clone());
        }
        if to > from {
            job.append(&shown[from..to], 0.0, style(syntax, font, p));
        }
        at = shown[..to].chars().count();
    }
    let at_b = byte(at);
    if at_b < shown.len() {
        job.append(&shown[at_b..], 0.0, plain);
    }
    job.wrap.max_rows = 1;
    ui.fonts_mut(|f| f.layout_job(job))
}

/// The design system's syntax colours (`tok-*`).
fn style(syntax: Syntax, font: &FontId, p: &Palette) -> TextFormat {
    let (color, face) = match syntax {
        Syntax::Keyword => (p.syn_keyword, None),
        Syntax::Type => (p.syn_type, None),
        Syntax::String => (p.syn_string, None),
        Syntax::Number => (p.syn_number, None),
        Syntax::Decorator => (p.syn_decorator, None),
        Syntax::Function => (p.syn_function, Some(Face::PlexSemiBold)),
        Syntax::Comment => (p.syn_comment, Some(Face::PlexItalic)),
        Syntax::Punct => (p.syn_punct(), None),
    };
    TextFormat::simple(
        face.map_or_else(|| font.clone(), |f| f.at(font.size)),
        color,
    )
}

/// The x of character `col` in a line's layout.
fn x_of(galley: &Galley, line: &str, col: usize) -> f32 {
    let (_, map) = display(line);
    let dcol = map[col.min(map.len() - 1)];
    galley.pos_from_cursor(CCursor::new(dcol)).min.x
}

/// The character at display column `dcol` (the nearest one's start).
fn char_col(line: &str, dcol: usize) -> usize {
    let (_, map) = display(line);
    map.iter().position(|&d| d >= dcol).unwrap_or(map.len() - 1)
}

/// The word around `col`: letters, digits and `_`, or a run of others.
fn word_at(line: &str, col: usize) -> (usize, usize) {
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() {
        return (0, 0);
    }
    let at = col.min(chars.len() - 1);
    let kind = class(chars[at]);
    let mut start = at;
    while start > 0 && class(chars[start - 1]) == kind {
        start -= 1;
    }
    let mut end = at;
    while end < chars.len() && class(chars[end]) == kind {
        end += 1;
    }
    (start, end)
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Class {
    Word,
    Space,
    Other,
}

fn class(c: char) -> Class {
    if c.is_alphanumeric() || c == '_' {
        Class::Word
    } else if c.is_whitespace() {
        Class::Space
    } else {
        Class::Other
    }
}

/// What the keyboard did.
#[derive(Default)]
struct Changed {
    /// The caret moved or the text changed: show the caret, solid.
    moved: bool,
    save: bool,
}

/// The editor's keys and text input.
fn keyboard(ui: &Ui, doc: &mut Document, mac: bool, page: usize) -> Changed {
    let mut changed = Changed::default();
    let events = ui.input(|i| i.events.clone());
    for event in events {
        match event {
            Event::Text(text) => {
                doc.insert(&text, EditKind::Typing);
                changed.moved = true;
            }
            Event::Ime(egui::ImeEvent::Commit(text)) if !text.is_empty() => {
                doc.insert(&text, EditKind::Typing);
                changed.moved = true;
            }
            Event::Copy | Event::Cut => {
                let cut = event == Event::Cut;
                let text = if doc.selection.is_empty() {
                    // Without a selection, the whole line.
                    let line = doc.selection.head.line;
                    if cut {
                        doc.selection = Selection {
                            anchor: Pos::new(line, 0),
                            head: if line + 1 < doc.line_count() {
                                Pos::new(line + 1, 0)
                            } else {
                                Pos::new(line, doc.line_len(line))
                            },
                        };
                    }
                    format!("{}\n", doc.line(line))
                } else {
                    doc.selected()
                };
                ui.ctx().copy_text(text);
                if cut {
                    doc.insert("", EditKind::Other);
                    changed.moved = true;
                }
            }
            Event::Paste(text) => {
                doc.insert(&text.replace("\r\n", "\n"), EditKind::Other);
                changed.moved = true;
            }
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } if key_press(doc, key, modifiers, mac, page) => {
                changed.moved = true;
            }
            _ => {}
        }
    }
    changed
}

/// One key; returns whether it did something.
fn key_press(doc: &mut Document, key: Key, m: Modifiers, mac: bool, page: usize) -> bool {
    let word = if mac { m.alt } else { m.ctrl };
    let line_edge = mac && m.mac_cmd;
    let head = doc.selection.head;
    match key {
        Key::A if m.command => {
            doc.selection = Selection {
                anchor: Pos::default(),
                head: doc.end(),
            };
        }
        Key::Z if m.command && m.shift => {
            doc.redo();
        }
        Key::Z if m.command => {
            doc.undo();
        }
        Key::Y if m.command && !mac => {
            doc.redo();
        }
        Key::ArrowLeft
        | Key::ArrowRight
        | Key::ArrowUp
        | Key::ArrowDown
        | Key::Home
        | Key::End
        | Key::PageUp
        | Key::PageDown => {
            let doc_edge = (mac && m.mac_cmd && matches!(key, Key::ArrowUp | Key::ArrowDown))
                || (!mac && m.ctrl && matches!(key, Key::Home | Key::End));
            // Without shift a selection collapses to its side first.
            if !m.shift
                && !doc.selection.is_empty()
                && matches!(key, Key::ArrowLeft | Key::ArrowRight)
                && !word
                && !line_edge
            {
                let (start, end) = doc.selection.range();
                doc.selection = Selection::caret(if key == Key::ArrowLeft { start } else { end });
                doc.goal_col = None;
                return true;
            }
            let vertical = matches!(
                key,
                Key::ArrowUp | Key::ArrowDown | Key::PageUp | Key::PageDown
            ) && !doc_edge;
            let to = moved(doc, head, key, word, line_edge, doc_edge, page);
            if !vertical {
                doc.goal_col = None;
            }
            doc.selection = if m.shift {
                Selection {
                    anchor: doc.selection.anchor,
                    head: to,
                }
            } else {
                Selection::caret(to)
            };
        }
        Key::Backspace | Key::Delete => {
            if doc.selection.is_empty() {
                let to = if key == Key::Backspace {
                    if line_edge {
                        Pos::new(head.line, 0)
                    } else if word {
                        word_left(doc, head)
                    } else {
                        left(doc, head)
                    }
                } else if line_edge {
                    Pos::new(head.line, doc.line_len(head.line))
                } else if word {
                    word_right(doc, head)
                } else {
                    right(doc, head)
                };
                doc.selection = Selection { anchor: to, head };
            }
            doc.insert("", EditKind::Deleting);
        }
        Key::Enter => {
            // The new line keeps the indentation of the one it splits.
            let indent: String = doc
                .line(head.line)
                .chars()
                .take(head.col)
                .take_while(|c| *c == ' ' || *c == '\t')
                .collect();
            doc.insert(&format!("\n{indent}"), EditKind::Other);
        }
        Key::Tab => {
            let unit = indent_unit(doc);
            let (start, end) = doc.selection.range();
            if start.line != end.line || m.shift {
                indent_lines(doc, &unit, m.shift);
            } else {
                doc.insert(&unit, EditKind::Typing);
            }
        }
        _ => return false,
    }
    true
}

/// Where a movement key takes the caret.
fn moved(
    doc: &mut Document,
    head: Pos,
    key: Key,
    word: bool,
    line_edge: bool,
    doc_edge: bool,
    page: usize,
) -> Pos {
    match key {
        Key::ArrowUp | Key::ArrowDown if doc_edge => {
            if key == Key::ArrowUp {
                Pos::default()
            } else {
                doc.end()
            }
        }
        Key::Home | Key::End if doc_edge => {
            if key == Key::Home {
                Pos::default()
            } else {
                doc.end()
            }
        }
        Key::ArrowLeft if line_edge => smart_home(doc, head),
        Key::ArrowRight if line_edge => Pos::new(head.line, doc.line_len(head.line)),
        Key::Home => smart_home(doc, head),
        Key::End => Pos::new(head.line, doc.line_len(head.line)),
        Key::ArrowLeft if word => word_left(doc, head),
        Key::ArrowRight if word => word_right(doc, head),
        Key::ArrowLeft => left(doc, head),
        Key::ArrowRight => right(doc, head),
        Key::ArrowUp | Key::ArrowDown | Key::PageUp | Key::PageDown => {
            let rows = if matches!(key, Key::PageUp | Key::PageDown) {
                page.max(1)
            } else {
                1
            };
            let up = matches!(key, Key::ArrowUp | Key::PageUp);
            let here = display(doc.line(head.line)).1[head.col.min(doc.line_len(head.line))];
            let goal = *doc.goal_col.get_or_insert(here);
            let line = if up {
                if head.line == 0 {
                    return Pos::default();
                }
                head.line.saturating_sub(rows)
            } else {
                if head.line + 1 >= doc.line_count() {
                    return doc.end();
                }
                (head.line + rows).min(doc.line_count() - 1)
            };
            Pos::new(line, char_col(doc.line(line), goal))
        }
        _ => head,
    }
}

fn left(doc: &Document, at: Pos) -> Pos {
    if at.col > 0 {
        Pos::new(at.line, at.col - 1)
    } else if at.line > 0 {
        Pos::new(at.line - 1, doc.line_len(at.line - 1))
    } else {
        at
    }
}

fn right(doc: &Document, at: Pos) -> Pos {
    if at.col < doc.line_len(at.line) {
        Pos::new(at.line, at.col + 1)
    } else if at.line + 1 < doc.line_count() {
        Pos::new(at.line + 1, 0)
    } else {
        at
    }
}

fn word_left(doc: &Document, at: Pos) -> Pos {
    if at.col == 0 {
        return left(doc, at);
    }
    let chars: Vec<char> = doc.line(at.line).chars().collect();
    let mut col = at.col;
    while col > 0 && class(chars[col - 1]) == Class::Space {
        col -= 1;
    }
    if col > 0 {
        let kind = class(chars[col - 1]);
        while col > 0 && class(chars[col - 1]) == kind {
            col -= 1;
        }
    }
    Pos::new(at.line, col)
}

fn word_right(doc: &Document, at: Pos) -> Pos {
    let chars: Vec<char> = doc.line(at.line).chars().collect();
    if at.col >= chars.len() {
        return right(doc, at);
    }
    let mut col = at.col;
    while col < chars.len() && class(chars[col]) == Class::Space {
        col += 1;
    }
    if col < chars.len() {
        let kind = class(chars[col]);
        while col < chars.len() && class(chars[col]) == kind {
            col += 1;
        }
    }
    Pos::new(at.line, col)
}

/// Home: the line's first character after its indentation, or its start
/// when the caret is there already.
fn smart_home(doc: &Document, at: Pos) -> Pos {
    let indent = doc
        .line(at.line)
        .chars()
        .take_while(|c| c.is_whitespace())
        .count();
    Pos::new(at.line, if at.col == indent { 0 } else { indent })
}

/// The file's indentation: a tab when a line starts with one, else four
/// spaces.
fn indent_unit(doc: &Document) -> String {
    let tabs = (0..doc.line_count().min(2000)).any(|i| doc.line(i).starts_with('\t'));
    if tabs { "\t".into() } else { "    ".into() }
}

/// Indents (or, `out`, outdents) the selected lines by one unit.
fn indent_lines(doc: &mut Document, unit: &str, out: bool) {
    let (start, end) = doc.selection.range();
    let last = if end.col == 0 && end.line > start.line {
        end.line - 1
    } else {
        end.line
    };
    let mut lines: Vec<String> = (start.line..=last)
        .map(|i| doc.line(i).to_owned())
        .collect();
    for line in &mut lines {
        if out {
            let strip = if line.starts_with('\t') {
                1
            } else {
                line.chars()
                    .take(TAB_STOP)
                    .take_while(|c| *c == ' ')
                    .count()
            };
            line.drain(..strip);
        } else if !line.is_empty() {
            line.insert_str(0, unit);
        }
    }
    let new = lines.join("\n");
    let end_pos = Pos::new(last, doc.line_len(last));
    doc.edit(Pos::new(start.line, 0), end_pos, &new, EditKind::Other);
    doc.selection = Selection {
        anchor: Pos::new(start.line, 0),
        head: Pos::new(last, doc.line_len(last)),
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn doc(text: &str) -> Document {
        Document::new(PathBuf::from("a.wowlua"), text)
    }

    #[test]
    fn tabs_are_drawn_to_the_next_stop() {
        let (shown, map) = display("\ta\tb");
        assert_eq!(shown, "    a   b");
        assert_eq!(map, [0, 4, 5, 8, 9]);
        assert_eq!(char_col("\ta", 2), 1);
    }

    #[test]
    fn words_and_home() {
        let d = doc("    local fooBar = 1");
        assert_eq!(word_left(&d, Pos::new(0, 16)), Pos::new(0, 10));
        assert_eq!(word_right(&d, Pos::new(0, 4)), Pos::new(0, 9));
        assert_eq!(smart_home(&d, Pos::new(0, 12)), Pos::new(0, 4));
        assert_eq!(smart_home(&d, Pos::new(0, 4)), Pos::new(0, 0));
        assert_eq!(word_at(d.line(0), 12), (10, 16));
    }

    #[test]
    fn enter_keeps_the_indentation_and_tab_indents_lines() {
        let mut d = doc("\tif x then");
        d.selection = Selection::caret(Pos::new(0, 10));
        key_press(&mut d, Key::Enter, Modifiers::NONE, false, 10);
        assert_eq!(d.text(), "\tif x then\n\t");
        d.selection = Selection {
            anchor: Pos::new(0, 0),
            head: Pos::new(1, 1),
        };
        key_press(&mut d, Key::Tab, Modifiers::NONE, false, 10);
        assert_eq!(d.text(), "\t\tif x then\n\t\t");
        key_press(&mut d, Key::Tab, Modifiers::SHIFT, false, 10);
        assert_eq!(d.text(), "\tif x then\n\t");
    }

    #[test]
    fn moving_down_keeps_the_column() {
        let mut d = doc("abcdef\nab\nabcdef");
        d.selection = Selection::caret(Pos::new(0, 5));
        key_press(&mut d, Key::ArrowDown, Modifiers::NONE, false, 10);
        assert_eq!(d.selection.head, Pos::new(1, 2));
        key_press(&mut d, Key::ArrowDown, Modifiers::NONE, false, 10);
        assert_eq!(d.selection.head, Pos::new(2, 5));
    }
}
