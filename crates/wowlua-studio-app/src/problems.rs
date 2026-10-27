//! The panels the language server fills: problems (every file's, as the
//! problems panel lists them), the outline of the active file, and the
//! references found (shown in the search panel, the search board).

use std::path::{Path, PathBuf};

use egui::{Align2, CornerRadius, Pos2, Rect, Sense, Ui, Vec2};
use wowlua_studio_ui::{Face, Palette, palette};

use crate::lsp::{Diag, Location, Symbol, Wire};

const ROW_H: f32 = 24.0;

/// The problems of every file; returns the one clicked.
pub fn problems<'a>(
    ui: &mut Ui,
    diagnostics: impl Iterator<Item = (&'a PathBuf, &'a Vec<Diag>)>,
    root: &Path,
) -> Option<Location> {
    let p = palette(ui.ctx());
    let mut rows: Vec<Row<'a>> = Vec::new();
    for (path, list) in diagnostics.filter(|(_, l)| !l.is_empty()) {
        rows.push(Row::File(path, list.len()));
        for d in list {
            rows.push(Row::Diag(path, d));
        }
    }
    if rows.is_empty() {
        empty(
            ui,
            "No problems. The checker's errors and warnings show here.",
        );
        return None;
    }
    let mut picked = None;
    egui::ScrollArea::vertical().auto_shrink(false).show_rows(
        ui,
        ROW_H,
        rows.len(),
        |ui, range| {
            for row in &rows[range] {
                let (r, resp) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(r, CornerRadius::ZERO, p.bg_300);
                }
                let painter = ui.painter().with_clip_rect(r);
                let cy = r.center().y;
                match row {
                    Row::File(path, n) => {
                        let rel = path.strip_prefix(root).unwrap_or(path);
                        painter.text(
                            Pos2::new(r.left() + 12.0, cy),
                            Align2::LEFT_CENTER,
                            rel.to_string_lossy(),
                            Face::PlexSemiBold.at(12.0),
                            p.ink,
                        );
                        painter.text(
                            Pos2::new(r.right() - 12.0, cy),
                            Align2::RIGHT_CENTER,
                            n.to_string(),
                            Face::Plex.at(11.5),
                            p.ink_muted,
                        );
                    }
                    Row::Diag(path, d) => {
                        let (word, color) = if d.is_error() {
                            ("error", p.error)
                        } else {
                            ("warning", p.warning)
                        };
                        let head = match &d.code {
                            Some(code) => format!("{word}[{code}]"),
                            None => word.to_owned(),
                        };
                        let x = r.left() + 28.0;
                        let w = painter
                            .text(
                                Pos2::new(x, cy),
                                Align2::LEFT_CENTER,
                                &head,
                                Face::PlexSemiBold.at(12.0),
                                color,
                            )
                            .width();
                        let first = d.message.lines().next().unwrap_or("");
                        painter.text(
                            Pos2::new(x + w + 6.0, cy),
                            Align2::LEFT_CENTER,
                            first,
                            Face::Plex.at(12.0),
                            p.ink,
                        );
                        painter.text(
                            Pos2::new(r.right() - 12.0, cy),
                            Align2::RIGHT_CENTER,
                            format!("{}:{}", d.start.line + 1, d.start.character + 1),
                            Face::Plex.at(11.5),
                            p.ink_muted,
                        );
                        if resp.clicked() {
                            picked = Some(Location {
                                path: (*path).clone(),
                                start: d.start,
                                end: d.end,
                            });
                        }
                    }
                }
            }
        },
    );
    picked
}

enum Row<'a> {
    File(&'a PathBuf, usize),
    Diag(&'a PathBuf, &'a Diag),
}

/// The active file's symbols; returns the place of the one clicked.
pub fn outline(ui: &mut Ui, symbols: Option<&[Symbol]>) -> Option<Wire> {
    let p = palette(ui.ctx());
    let Some(symbols) = symbols.filter(|s| !s.is_empty()) else {
        empty(ui, "The outline of the file in the editor shows here.");
        return None;
    };
    let mut picked = None;
    egui::ScrollArea::vertical().auto_shrink(false).show_rows(
        ui,
        ROW_H,
        symbols.len(),
        |ui, range| {
            for s in &symbols[range] {
                let (r, resp) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(r, CornerRadius::ZERO, p.bg_300);
                }
                let painter = ui.painter().with_clip_rect(r);
                let x = r.left() + 18.0 + s.depth as f32 * 14.0;
                let cy = r.center().y;
                painter.text(
                    Pos2::new(x, cy),
                    Align2::LEFT_CENTER,
                    symbol_word(s.kind),
                    Face::Plex.at(11.0),
                    p.ink_muted,
                );
                let w = painter
                    .text(
                        Pos2::new(x + 44.0, cy),
                        Align2::LEFT_CENTER,
                        &s.name,
                        Face::Plex.at(12.5),
                        p.ink,
                    )
                    .width();
                if !s.detail.is_empty() {
                    painter.text(
                        Pos2::new(x + 44.0 + w + 8.0, cy),
                        Align2::LEFT_CENTER,
                        &s.detail,
                        Face::Plex.at(11.5),
                        p.ink_muted,
                    );
                }
                if resp.clicked() {
                    picked = Some(s.start);
                }
            }
        },
    );
    picked
}

/// The references found; returns the one clicked.
pub fn references(
    ui: &mut Ui,
    found: Option<&(String, Vec<Location>)>,
    root: &Path,
) -> Option<Location> {
    let p = palette(ui.ctx());
    let Some((name, list)) = found else {
        empty(
            ui,
            "References show here: shift f12 on a name in the editor.",
        );
        return None;
    };
    let (head, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::hover());
    ui.painter().text(
        Pos2::new(head.left() + 12.0, head.center().y),
        Align2::LEFT_CENTER,
        format!("{} references to {name}", list.len()),
        Face::Plex.at(11.5),
        p.ink_muted,
    );
    let mut picked = None;
    egui::ScrollArea::vertical().auto_shrink(false).show_rows(
        ui,
        ROW_H,
        list.len(),
        |ui, range| {
            for l in &list[range] {
                let (r, resp) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(r, CornerRadius::ZERO, p.bg_300);
                }
                let rel = l.path.strip_prefix(root).unwrap_or(&l.path);
                ui.painter().with_clip_rect(r).text(
                    Pos2::new(r.left() + 12.0, r.center().y),
                    Align2::LEFT_CENTER,
                    format!("{}:{}", rel.to_string_lossy(), l.start.line + 1),
                    Face::Plex.at(12.0),
                    p.ink,
                );
                if resp.clicked() {
                    picked = Some(l.clone());
                }
            }
        },
    );
    picked
}

fn empty(ui: &mut Ui, text: &str) {
    let p: &Palette = palette(ui.ctx());
    let rect = ui.max_rect().shrink2(Vec2::new(16.0, 16.0));
    let g = ui.painter().layout(
        text.to_owned(),
        Face::Plex.at(12.5),
        p.ink_muted,
        rect.width().max(40.0),
    );
    ui.painter().galley(
        Rect::from_min_size(rect.min, Vec2::ZERO).min,
        g,
        p.ink_muted,
    );
}

/// The word for an LSP symbol kind.
fn symbol_word(kind: u8) -> &'static str {
    match kind {
        5 => "class",
        6 | 12 => "fn",
        8 => "field",
        13 => "local",
        14 => "const",
        23 => "type",
        11 => "type",
        2 => "module",
        _ => "",
    }
}
