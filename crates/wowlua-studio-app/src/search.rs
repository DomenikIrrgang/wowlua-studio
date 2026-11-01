//! The search panel (the design's search board): the references found, a
//! rename's changes to check before they are made, and text search with
//! replace across the workspace.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use egui::{Align2, CornerRadius, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use wowlua_studio_ui::forms::{self, CalloutKind};
use wowlua_studio_ui::widgets::{self, ButtonKind};
use wowlua_studio_ui::{Face, Palette, palette};

use crate::lsp::{Location, TextEdit, Wire};
use crate::system::{Found, Search};

/// A row, at the settings' density (S062).
fn row_h() -> f32 {
    wowlua_studio_ui::row(24.0)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    References,
    Rename,
    Text,
}

/// A change of a rename, to be made or left: where, the line as it is.
#[derive(Clone, Debug)]
pub struct Change {
    pub edit: TextEdit,
    pub line: String,
    pub checked: bool,
}

/// A rename the server worked out, before it is made.
#[derive(Clone, Debug, Default)]
pub struct Rename {
    pub old: String,
    pub new: String,
    pub files: BTreeMap<PathBuf, Vec<Change>>,
    /// Why the server refused it.
    pub refused: Option<String>,
}

#[derive(Default)]
pub struct SearchState {
    pub tab: Tab,
    /// The name, and each reference with its line's text.
    pub references: Option<(String, Vec<(Location, String)>)>,
    pub rename: Option<Rename>,
    pub search: Search,
    pub replace: String,
    /// The search being run, and what the last one found.
    pub running: Option<Search>,
    pub found: Option<(Search, Found)>,
    /// Focus the query field.
    pub focus: bool,
}

/// What the panel asks Studio to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Show a place (as a preview tab).
    Show(Location),
    Search(Search),
    /// Make the rename's checked changes.
    Rename(BTreeMap<PathBuf, Vec<TextEdit>>),
    /// Replace every match of the last search.
    ReplaceAll(BTreeMap<PathBuf, Vec<TextEdit>>),
}

pub fn show(ui: &mut Ui, state: &mut SearchState, root: &Path) -> Option<Action> {
    let p = palette(ui.ctx());
    let mut action = None;
    // The tabs: references, rename, text.
    let (bar, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::hover());
    ui.painter()
        .hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, p.line));
    let mut x = bar.left();
    for (tab, label) in [
        (Tab::References, "references"),
        (Tab::Rename, "rename"),
        (Tab::Text, "text"),
    ] {
        let g = widgets::galley(ui, label, Face::PlexMedium.at(12.0), p.ink, 0.24);
        let r = Rect::from_min_size(
            Pos2::new(x, bar.top()),
            Vec2::new(g.size().x + 24.0, bar.height()),
        );
        let resp = ui.interact(
            r,
            egui::Id::unique(("wowlua-studio-search-tab", label)),
            Sense::click(),
        );
        let on = state.tab == tab;
        let fg = if on || resp.hovered() {
            p.ink
        } else {
            p.ink_muted
        };
        ui.painter().galley(r.center() - g.size() / 2.0, g, fg);
        if on {
            ui.painter().rect_filled(
                Rect::from_min_max(Pos2::new(r.left(), r.bottom() - 2.0), r.max),
                CornerRadius::ZERO,
                p.amber,
            );
        }
        if resp.clicked() {
            state.tab = tab;
            state.focus = tab == Tab::Text;
        }
        x = r.right();
    }
    match state.tab {
        Tab::References => {
            if let Some(l) = references(ui, state.references.as_ref(), root, p) {
                action = Some(Action::Show(l));
            }
        }
        Tab::Rename => action = rename(ui, state, root, p),
        Tab::Text => action = text(ui, state, root, p),
    }
    action
}

const SECTION_H: f32 = 28.0;

/// A long list in a scroll area, drawing only the rows on screen: each
/// row's height, and `draw` for row `i` in its place.
fn rows(ui: &mut Ui, heights: &[f32], mut draw: impl FnMut(&mut Ui, usize, Rect)) {
    let total: f32 = heights.iter().sum();
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show_viewport(ui, |ui, viewport| {
            let width = ui.available_width();
            let (all, _) = ui.allocate_exact_size(Vec2::new(width, total), Sense::hover());
            let mut y = 0.0;
            for (i, h) in heights.iter().enumerate() {
                if y > viewport.max.y {
                    break;
                }
                if y + h >= viewport.min.y {
                    let r = Rect::from_min_size(all.min + Vec2::new(0.0, y), Vec2::new(width, *h));
                    draw(ui, i, r);
                }
                y += h;
            }
        });
}

fn section(ui: &mut Ui, r: Rect, text: &str, count: usize, p: &Palette) {
    let painter = ui.painter().with_clip_rect(r);
    painter.text(
        Pos2::new(r.left() + 12.0, r.bottom() - 9.0),
        Align2::LEFT_CENTER,
        text,
        Face::Plex.at(11.5),
        p.ink_muted,
    );
    painter.text(
        Pos2::new(r.right() - 12.0, r.bottom() - 9.0),
        Align2::RIGHT_CENTER,
        count.to_string(),
        Face::Plex.at(11.5),
        p.ink_muted,
    );
}

fn empty(ui: &mut Ui, text: &str, p: &Palette) {
    forms::padded(ui, 12, 12, |ui| {
        forms::paragraph(ui, text, 12.5, 19.0, p.ink_muted);
    });
}

/// A row of a match in `r`: its line number, the line with the match
/// marked.
#[allow(clippy::too_many_arguments)]
fn hit_row(
    ui: &mut Ui,
    r: Rect,
    id: egui::Id,
    line: usize,
    text: &str,
    start: usize,
    end: usize,
    p: &Palette,
) -> egui::Response {
    let resp = ui.interact(r, id, Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::ZERO, p.bg_300);
    }
    let painter = ui.painter().with_clip_rect(r);
    painter.text(
        Pos2::new(r.left() + 22.0, r.center().y),
        Align2::LEFT_CENTER,
        (line + 1).to_string(),
        Face::Plex.at(11.5),
        p.ink_muted,
    );
    // The line from a little before the match, trimmed.
    let chars: Vec<char> = text.chars().take(end + 300).collect();
    let from = start.saturating_sub(24).max(
        chars
            .iter()
            .take_while(|c| c.is_whitespace())
            .count()
            .min(start),
    );
    let before: String = chars[from..start.min(chars.len())].iter().collect();
    let hit: String = chars[start.min(chars.len())..end.min(chars.len())]
        .iter()
        .collect();
    // No more than a wide panel shows.
    let after: String = chars[end.min(chars.len())..].iter().take(300).collect();
    let font = Face::Plex.at(12.0);
    let mut x = r.left() + 58.0;
    let a = painter.text(
        Pos2::new(x, r.center().y),
        Align2::LEFT_CENTER,
        &before,
        font.clone(),
        p.ink,
    );
    x = a.right();
    let g = painter.layout_no_wrap(hit, font.clone(), p.ink);
    let hit_rect = Rect::from_min_size(Pos2::new(x, r.center().y - g.size().y / 2.0), g.size());
    painter.rect_filled(hit_rect, CornerRadius::ZERO, p.amber_soft);
    painter.rect_stroke(
        hit_rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, p.amber_ink),
        egui::StrokeKind::Inside,
    );
    painter.galley(hit_rect.min, g, p.ink);
    painter.text(
        Pos2::new(hit_rect.right(), r.center().y),
        Align2::LEFT_CENTER,
        &after,
        font,
        p.ink,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn references(
    ui: &mut Ui,
    found: Option<&(String, Vec<(Location, String)>)>,
    root: &Path,
    p: &Palette,
) -> Option<Location> {
    let Some((name, list)) = found else {
        empty(
            ui,
            "References show here: shift f12 on a name in the editor, or click a lens over a function.",
            p,
        );
        return None;
    };
    let mut files: BTreeMap<&Path, Vec<&(Location, String)>> = BTreeMap::new();
    for l in list {
        files.entry(l.0.path.as_path()).or_default().push(l);
    }
    forms::padded(ui, 12, 10, |ui| {
        forms::paragraph(
            ui,
            &format!(
                "{name} · {} references in {} files of the workspace",
                list.len(),
                files.len()
            ),
            12.0,
            18.0,
            p.ink_muted,
        );
    });
    // The rows: a file's section, then its references.
    let mut list_rows: Vec<(&Path, Option<&(Location, String)>)> = Vec::new();
    for (path, locs) in &files {
        list_rows.push((path, None));
        list_rows.extend(locs.iter().map(|l| (*path, Some(*l))));
    }
    let heights: Vec<f32> = list_rows
        .iter()
        .map(|(_, l)| if l.is_some() { row_h() } else { SECTION_H })
        .collect();
    let mut picked = None;
    rows(ui, &heights, |ui, i, r| match list_rows[i] {
        (path, None) => section(ui, r, &rel(path, root), files[path].len(), p),
        (_, Some((l, text))) => {
            let start = utf16_to_chars(text, l.start.character);
            let end = if l.end.line == l.start.line {
                utf16_to_chars(text, l.end.character)
            } else {
                start
            };
            let id = egui::Id::unique(("wowlua-studio-reference", i));
            if hit_row(ui, r, id, l.start.line, text, start, end, p).clicked() {
                picked = Some(l.clone());
            }
        }
    });
    picked
}

fn rename(ui: &mut Ui, state: &mut SearchState, root: &Path, p: &Palette) -> Option<Action> {
    let Some(r) = &mut state.rename else {
        empty(
            ui,
            "A rename shows its changes here first: f2 on a name in the editor.",
            p,
        );
        return None;
    };
    if let Some(why) = &r.refused {
        forms::padded(ui, 12, 12, |ui| {
            forms::callout(ui, CalloutKind::Warning, |ui| {
                forms::paragraph(
                    ui,
                    &format!("can't rename {}: {why} Nothing was changed.", r.old),
                    12.0,
                    18.0,
                    p.ink,
                );
            });
        });
        return None;
    }
    let total: usize = r.files.values().map(Vec::len).sum();
    let checked: usize = r.files.values().flatten().filter(|c| c.checked).count();
    forms::padded(ui, 12, 10, |ui| {
        forms::paragraph(
            ui,
            &format!(
                "rename {} to {} · {total} changes in {} files",
                r.old,
                r.new,
                r.files.len()
            ),
            12.0,
            18.0,
            p.ink_muted,
        );
    });
    let mut action = None;
    let mut cancel = false;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .max_height(ui.available_height() - 52.0)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (path, changes) in &mut r.files {
                let (row, _) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), SECTION_H),
                    Sense::hover(),
                );
                section(ui, row, &rel(path, root), changes.len(), p);
                for c in changes.iter_mut() {
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        let label = format!("{}", c.edit.start.line + 1);
                        forms::checkbox(ui, &mut c.checked, &label, None);
                        let start = utf16_to_chars(&c.line, c.edit.start.character);
                        let end = utf16_to_chars(&c.line, c.edit.end.character);
                        let chars: Vec<char> = c.line.chars().collect();
                        let s = |a: usize, b: usize| {
                            chars[a.min(chars.len())..b.min(chars.len())]
                                .iter()
                                .collect::<String>()
                        };
                        let mut job = egui::text::LayoutJob::default();
                        let font = Face::Plex.at(12.0);
                        let plain = egui::text::TextFormat::simple(font.clone(), p.ink);
                        job.append(s(0, start).trim_start(), 0.0, plain.clone());
                        let mut old = egui::text::TextFormat::simple(font.clone(), p.ink);
                        old.background = p.error_soft;
                        old.strikethrough = Stroke::new(1.0, p.ink_muted);
                        job.append(&s(start, end), 0.0, old);
                        let mut new = egui::text::TextFormat::simple(font.clone(), p.ink);
                        new.background = p.success_soft;
                        job.append(&c.edit.text, 0.0, new);
                        job.append(&s(end, chars.len()), 0.0, plain);
                        job.wrap.max_rows = 1;
                        let g = ui.fonts_mut(|f| f.layout_job(job));
                        let (rect, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), row_h()),
                            Sense::hover(),
                        );
                        ui.painter().with_clip_rect(rect).galley(
                            Pos2::new(rect.left(), rect.center().y - g.size().y / 2.0),
                            g,
                            p.ink,
                        );
                    });
                }
            }
        });
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.add_space(12.0);
        let label = format!("rename {checked}");
        let go = ui
            .add_enabled_ui(checked > 0, |ui| {
                widgets::button(ui, ButtonKind::Primary, &label, true, None)
            })
            .inner;
        if go.clicked() {
            let edits = r
                .files
                .iter()
                .map(|(path, changes)| {
                    (
                        path.clone(),
                        changes
                            .iter()
                            .filter(|c| c.checked)
                            .map(|c| c.edit.clone())
                            .collect::<Vec<_>>(),
                    )
                })
                .filter(|(_, e)| !e.is_empty())
                .collect();
            action = Some(Action::Rename(edits));
        }
        cancel = widgets::button(ui, ButtonKind::Tertiary, "cancel", true, None).clicked();
    });
    if cancel || action.is_some() {
        state.rename = None;
        state.tab = Tab::References;
    }
    action
}

fn chip(ui: &mut Ui, label: &str, on: &mut bool, p: &Palette) -> bool {
    let g = widgets::galley(ui, label, Face::Plex.at(11.5), p.ink, 0.0);
    let (r, resp) = ui.allocate_exact_size(Vec2::new(g.size().x + 16.0, 22.0), Sense::click());
    let (bg, border) = if *on {
        (p.amber_soft, p.amber_ink)
    } else if resp.hovered() {
        (p.bg_300, p.line_strong)
    } else {
        (p.bg_100, p.line)
    };
    ui.painter().rect_filled(r, CornerRadius::ZERO, bg);
    ui.painter().rect_stroke(
        r,
        CornerRadius::ZERO,
        Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(r.center() - g.size() / 2.0, g, p.ink);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, *on, label));
    if resp.clicked() {
        *on = !*on;
        return true;
    }
    false
}

fn text(ui: &mut Ui, state: &mut SearchState, root: &Path, p: &Palette) -> Option<Action> {
    let mut action = None;
    let mut again = false;
    forms::padded(ui, 12, 12, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        let w = ui.available_width();
        let query = forms::text_input(ui, &mut state.search.query, w, "search text");
        if std::mem::take(&mut state.focus) {
            query.request_focus();
        }
        if query.changed() {
            again = true;
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            again |= chip(ui, "Aa case", &mut state.search.case, p);
            again |= chip(ui, "whole word", &mut state.search.word, p);
            again |= chip(ui, ".* regex", &mut state.search.regex, p);
        });
        forms::text_input(ui, &mut state.replace, w, "replace");
    });
    if again && !state.search.query.is_empty() {
        action = Some(Action::Search(state.search.clone()));
    }
    let Some((search, found)) = &state.found else {
        if state.running.is_some() {
            empty(ui, "searching…", p);
        } else {
            empty(
                ui,
                "Text in every file of the workspace that Git does not ignore.",
                p,
            );
        }
        return action;
    };
    if let Some(error) = &found.error {
        forms::padded(ui, 12, 4, |ui| {
            forms::callout(ui, CalloutKind::Error, |ui| {
                forms::paragraph(ui, error, 12.0, 18.0, p.ink)
            });
        });
        return action;
    }
    let matches: usize = found.files.iter().map(|f| f.1.len()).sum();
    if !state.replace.is_empty() && matches > 0 {
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            let label = format!("replace {matches}");
            if widgets::button(ui, ButtonKind::Secondary, &label, true, None).clicked() {
                let mut edits: BTreeMap<PathBuf, Vec<TextEdit>> = BTreeMap::new();
                for (path, hits) in &found.files {
                    for h in hits {
                        let (start, end) = wires(h);
                        edits.entry(path.clone()).or_default().push(TextEdit {
                            start,
                            end,
                            text: state.replace.clone(),
                        });
                    }
                }
                action = Some(Action::ReplaceAll(edits));
            }
        });
    }
    let _ = search;
    // The rows: a file's section, then its matches; last the summary.
    let mut list_rows: Vec<(usize, Option<usize>)> = Vec::new();
    for (f, (_, hits)) in found.files.iter().enumerate() {
        list_rows.push((f, None));
        list_rows.extend((0..hits.len()).map(|h| (f, Some(h))));
    }
    let mut heights: Vec<f32> = list_rows
        .iter()
        .map(|(_, h)| if h.is_some() { row_h() } else { SECTION_H })
        .collect();
    heights.push(36.0);
    let cut = if found.cut { " (the first 5000)" } else { "" };
    let summary = format!(
        "{matches} matches{cut} in {} files · {} files searched in {} ms",
        found.files.len(),
        found.searched,
        found.millis
    );
    let mut picked = None;
    rows(ui, &heights, |ui, i, r| {
        let Some(&(f, hit)) = list_rows.get(i) else {
            ui.painter().with_clip_rect(r).text(
                Pos2::new(r.left() + 12.0, r.center().y),
                Align2::LEFT_CENTER,
                &summary,
                Face::Plex.at(11.5),
                p.ink_muted,
            );
            return;
        };
        let (path, hits) = &found.files[f];
        match hit {
            None => section(ui, r, &rel(path, root), hits.len(), p),
            Some(h) => {
                let h = &hits[h];
                let id = egui::Id::unique(("wowlua-studio-search-hit", i));
                if hit_row(ui, r, id, h.line, &h.text, h.start, h.end, p).clicked() {
                    let (start, end) = wires(h);
                    picked = Some(Location {
                        path: path.clone(),
                        start,
                        end,
                    });
                }
            }
        }
    });
    if let Some(l) = picked {
        action = Some(Action::Show(l));
    }
    action
}

/// A match's place, as the language server counts.
fn wires(h: &crate::system::Hit) -> (Wire, Wire) {
    let at = |character| Wire {
        line: h.line,
        character,
    };
    (at(h.at.0), at(h.at.1))
}

/// `path` from the workspace's root, with `/`.
fn rel(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn utf16_to_chars(text: &str, units: usize) -> usize {
    let mut seen = 0;
    for (i, c) in text.chars().enumerate() {
        if seen >= units {
            return i;
        }
        seen += c.len_utf16();
    }
    text.chars().count()
}

/// The output panel: what the language server logs.
pub fn output(ui: &mut Ui, lines: &[String]) {
    let p = palette(ui.ctx());
    if lines.is_empty() {
        empty(ui, "What the language server logs shows here.", p);
        return;
    }
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .stick_to_bottom(true)
        .show_rows(ui, 18.0, lines.len(), |ui, range| {
            for line in &lines[range] {
                let (r, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
                ui.painter().with_clip_rect(r).text(
                    Pos2::new(r.left() + 12.0, r.center().y),
                    Align2::LEFT_CENTER,
                    line,
                    Face::Plex.at(12.0),
                    p.ink,
                );
            }
        });
}
