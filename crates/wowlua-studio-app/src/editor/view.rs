//! Drawing the editor and taking its input (the design's editor board):
//! the tabs, the path, the changed-on-disk banner, and the code with its
//! gutter: folds, the lenses over functions, the types of locals as inlay
//! hints, the server's problems underlined, and its popups: hover,
//! completion, signature help, rename, quick fixes. Only the rows on screen
//! are laid out; positions come from each line's laid-out text, so wide
//! characters, tabs and hints place the caret right.

use std::collections::HashMap;
use std::sync::Arc;

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, Event, EventFilter, FontId, Galley, Id, Key,
    Modifiers, Order, Pos2, Rect, Sense, Shape, Stroke, Ui, UiBuilder, Vec2,
    epaint::text::cursor::CCursor,
    text::{LayoutJob, TextFormat},
};
use wowlua_studio_ui::forms::{self, CalloutKind};
use wowlua_studio_ui::widgets::{self, ButtonKind};
use wowlua_studio_ui::{Face, Icon, Palette, icons, palette};

use super::diff;
use super::doc::{Document, EditKind, Pos, Selection};
use super::{Completion, Editors, Tab};
use crate::lsp::{self, Diag, Fix};
use crate::system::{Highlight, Syntax, System, Workspace};

pub const LINE_H: f32 = 21.0;
/// A lens row over a function (`st-ln--lens`).
const LENS_H: f32 = 18.0;
const PAD_TOP: f32 = 6.0;
/// The breakpoint column and the line numbers.
const GUTTER: f32 = 20.0 + 46.0;
const TABS_H: f32 = 34.0;
const CRUMBS_H: f32 = 26.0;
const TAB_STOP: usize = 4;
const FONT_SIZE: f32 = 13.0;
/// How long the pointer rests before the hover is asked for.
const HOVER_AFTER: f64 = 0.45;
/// How long the hover stays once the pointer left its place: time to move
/// onto it, to scroll or select in it.
const HOVER_LEAVE: f64 = 0.4;
/// The hover's height at most: this, or the window's less a margin.
const HOVER_MAX_H: f32 = 1260.0;

/// What the editor asks Studio to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Save(usize),
    /// Close the tab; Studio asks first when it has unsaved edits.
    Close(usize),
    Hover(Pos),
    Complete(Pos),
    Signature(Pos),
    Definition(Pos),
    /// Whether the name at the place has a definition (cmd held over it).
    Linkable(Pos),
    References(Pos),
    Rename(Pos, String),
    /// The quick fixes of the problems at the place.
    Fixes(Pos),
    /// Apply a quick fix.
    ApplyFix(Fix),
}

/// What the editor needs from Studio.
pub struct Cx<'a> {
    pub system: &'a dyn System,
    pub workspace: &'a Workspace,
    pub mac: bool,
    /// The active file's problems.
    pub diagnostics: &'a [Diag],
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
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Open a file from the explorer.",
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
    // Esc in the code closes the find bar, when no popup takes it.
    let code_id = Id::unique(("wowlua-studio-editor", &tab.doc.path));
    if editors.finder.open
        && tab.completion.is_none()
        && tab.fixes.is_none()
        && tab.rename.is_none()
        && ui.memory(|m| m.has_focus(code_id))
        && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))
    {
        editors.finder.close();
    }

    let mut top = crumbs_rect.bottom();
    if tab.clash.is_some() {
        let banner = Rect::from_min_max(Pos2::new(rect.left(), top), rect.max);
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
    // The find bar's matches, marked while it is open.
    let found: Vec<(Pos, Pos)> = if editors.finder.open {
        super::find::matches(tab, &editors.finder.search, cx.system)
            .list
            .clone()
    } else {
        Vec::new()
    };
    code(ui, code_rect, tab, cx, focus, active, &found, &mut actions);
    if editors.finder.open {
        let p = palette(ui.ctx());
        super::find::ticks(ui, code_rect, tab.doc.line_count(), &found, p);
        if super::find::bar(ui, code_rect, &mut editors.finder, tab, cx.system, p) {
            editors.focus = true;
        }
    }
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
    for tab in &mut editors.tabs {
        if tab.preview && tab.doc.is_dirty() {
            tab.preview = false;
        }
    }
    for (i, tab) in editors.tabs.iter().enumerate() {
        let name = if tab.preview {
            format!("{} · preview", file_name(&tab.doc))
        } else {
            file_name(&tab.doc)
        };
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
        let (icon, color) = crate::explorer::file_icon(&tab.doc.path, &name, p);
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
        let (f, c) = if i + 1 == parts.len() {
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
                let g = rich(
                    ui,
                    &[
                        ("note: ", Face::PlexSemiBold.at(12.0), p.note),
                        (&text, Face::Plex.at(12.0), p.ink),
                    ],
                    width,
                );
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

/// Text in sections of their own fonts and colours, wrapped at `width`.
fn rich(ui: &Ui, parts: &[(&str, FontId, Color32)], width: f32) -> Arc<Galley> {
    let mut job = LayoutJob::default();
    for (text, font, color) in parts {
        job.append(text, 0.0, TextFormat::simple(font.clone(), *color));
    }
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

/// A row of the code: a line, or the lens over one.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Row {
    Line(usize),
    Lens(usize),
}

impl Row {
    fn line(self) -> usize {
        match self {
            Row::Line(l) | Row::Lens(l) => l,
        }
    }

    fn height(self) -> f32 {
        match self {
            Row::Line(_) => LINE_H,
            Row::Lens(_) => LENS_H,
        }
    }
}

/// The code's rows from the top: folded lines left out, lenses in.
struct Rows {
    rows: Vec<(Row, f32)>,
    height: f32,
}

impl Rows {
    fn new(lines: usize, folded: &[(usize, usize)], lens_lines: &[usize]) -> Rows {
        let mut rows = Vec::with_capacity(lines + lens_lines.len());
        let mut y = PAD_TOP;
        let mut line = 0;
        while line < lines {
            if lens_lines.binary_search(&line).is_ok() {
                rows.push((Row::Lens(line), y));
                y += LENS_H;
            }
            rows.push((Row::Line(line), y));
            y += LINE_H;
            line = match folded.iter().find(|f| f.0 == line) {
                Some(&(_, last)) => last + 1,
                None => line + 1,
            };
        }
        Rows { rows, height: y }
    }

    /// The top of a line's row (a hidden line: the fold's first).
    fn y_of(&self, line: usize) -> f32 {
        let i = self
            .rows
            .partition_point(|(r, _)| r.line() < line || matches!(r, Row::Lens(l) if *l == line));
        match self.rows.get(i) {
            Some((Row::Line(l), y)) if *l == line => *y,
            _ => self.rows[i.saturating_sub(1).min(self.rows.len() - 1)].1,
        }
    }

    /// The row at `y`.
    fn at(&self, y: f32) -> Row {
        let i = self
            .rows
            .partition_point(|(_, top)| *top <= y)
            .saturating_sub(1);
        self.rows[i.min(self.rows.len() - 1)].0
    }

    /// The rows between `top` and `bottom`.
    fn between(&self, top: f32, bottom: f32) -> &[(Row, f32)] {
        let first = self.rows.partition_point(|(r, y)| y + r.height() < top);
        let last = self.rows.partition_point(|(_, y)| *y <= bottom);
        &self.rows[first..last.max(first)]
    }
}

/// Lines longer than this (bytes) are laid out only where they are seen.
pub const LONG_LINE: usize = 4000;
/// A long line's characters are marked every so many.
const MARK_EVERY: usize = 1024;
/// The characters laid out on each side of a long line's part in view.
const MARGIN: usize = 64;

/// A long line's columns: every `MARK_EVERY`th character's byte and
/// display column, so that a character's column (and back) is found
/// without going over the whole line.
pub struct Columns {
    marks: Vec<(usize, usize)>,
}

/// The display column after `c` at `col`: tabs to the next stop.
pub fn step(col: usize, c: char) -> usize {
    if c == '\t' {
        col + TAB_STOP - col % TAB_STOP
    } else {
        col + 1
    }
}

impl Columns {
    pub fn new(line: &str) -> Self {
        let mut marks = Vec::with_capacity(line.len() / MARK_EVERY + 1);
        let mut dcol = 0;
        for (n, (b, c)) in line.char_indices().enumerate() {
            if n % MARK_EVERY == 0 {
                marks.push((b, dcol));
            }
            dcol = step(dcol, c);
        }
        if marks.is_empty() {
            marks.push((0, 0));
        }
        Columns { marks }
    }

    /// The characters from mark `k`: (index, byte, display column, char).
    fn chars_from<'a>(
        &self,
        line: &'a str,
        k: usize,
    ) -> impl Iterator<Item = (usize, usize, usize, char)> + 'a {
        let (byte, dcol) = self.marks[k];
        line[byte..]
            .char_indices()
            .scan(dcol, |d, (b, c)| {
                let at = *d;
                *d = step(*d, c);
                Some((b, at, c))
            })
            .enumerate()
            .map(move |(i, (b, d, c))| (k * MARK_EVERY + i, byte + b, d, c))
    }

    /// Character `col`'s display column (the end's past the last).
    fn dcol(&self, line: &str, col: usize) -> usize {
        let k = (col / MARK_EVERY).min(self.marks.len() - 1);
        let mut end = self.marks[k].1;
        for (i, _, d, c) in self.chars_from(line, k) {
            if i == col {
                return d;
            }
            end = step(d, c);
        }
        end
    }

    /// Character `col`'s byte.
    fn byte(&self, line: &str, col: usize) -> usize {
        let k = (col / MARK_EVERY).min(self.marks.len() - 1);
        self.chars_from(line, k)
            .find(|&(i, ..)| i == col)
            .map_or(line.len(), |(_, b, ..)| b)
    }

    /// The first character at display column `dcol` or after it (the
    /// line's length past its end).
    fn col(&self, line: &str, dcol: usize) -> usize {
        let k = self
            .marks
            .partition_point(|m| m.1 <= dcol)
            .saturating_sub(1);
        let mut count = k * MARK_EVERY;
        for (i, _, d, _) in self.chars_from(line, k) {
            if d >= dcol {
                return i;
            }
            count = i + 1;
        }
        count
    }
}

/// A line as it is drawn: its layout, and where each character (and the
/// end) is in it; inlay hints are in it too. A long line is laid out from
/// character `from` only, as far as it is seen, starting at `x0`; its
/// other characters are placed by their columns.
struct Laid<'a> {
    galley: Arc<Galley>,
    map: Vec<usize>,
    from: usize,
    x0: f32,
    long: Option<(&'a str, &'a Columns, f32)>,
}

impl Laid<'_> {
    /// The x of character `col`.
    fn x(&self, col: usize) -> f32 {
        if let Some((line, cols, char_w)) = self.long
            && (col < self.from || col >= self.from + self.map.len())
        {
            return cols.dcol(line, col) as f32 * char_w;
        }
        let i = self.map[col.saturating_sub(self.from).min(self.map.len() - 1)];
        self.x0 + self.galley.pos_from_cursor(CCursor::new(i)).min.x
    }

    /// The character at `x`.
    fn col_at(&self, x: f32) -> usize {
        if let Some((line, cols, char_w)) = self.long
            && (x < self.x0 || x > self.x0 + self.galley.size().x)
        {
            return cols.col(line, (x.max(0.0) / char_w).round() as usize);
        }
        let i = self
            .galley
            .cursor_from_pos(Vec2::new(x - self.x0, 0.0))
            .index
            .0;
        self.from
            + self
                .map
                .iter()
                .position(|&m| m >= i)
                .unwrap_or(self.map.len() - 1)
    }
}

/// What laying out a line needs besides it.
struct LayCx<'a> {
    highlights: &'a [Vec<(usize, usize, Syntax)>],
    hints: &'a [(Pos, String)],
    long: Option<&'a HashMap<usize, Columns>>,
    font: FontId,
    char_w: f32,
}

/// Line `line` as it is drawn where `view` (x, from the text's left) is
/// seen.
fn lay_line<'a>(
    ui: &Ui,
    doc: &'a Document,
    line: usize,
    view: egui::Rangef,
    cx: &LayCx<'a>,
    p: &Palette,
) -> Laid<'a> {
    let text = doc.line(line);
    let colours = cx.highlights.get(line).map_or(&[][..], |h| h.as_slice());
    if let Some(cols) = cx.long.and_then(|l| l.get(&line)) {
        // The characters in view, and a margin on each side.
        let left = (view.min.max(0.0) / cx.char_w) as usize;
        let right = (view.max.max(0.0) / cx.char_w) as usize;
        let first = cols.col(text, left.saturating_sub(MARGIN));
        let last = cols.col(text, right + MARGIN);
        let chars: Vec<char> = text[cols.byte(text, first)..]
            .chars()
            .take(last - first)
            .collect();
        let start = colours.partition_point(|h| h.1 <= first);
        let shifted: Vec<(usize, usize, Syntax)> = colours[start..]
            .iter()
            .take_while(|h| h.0 < last)
            .map(|h| (h.0.saturating_sub(first), h.1.min(last) - first, h.2))
            .collect();
        let dcol = cols.dcol(text, first);
        let (galley, map) = lay(ui, &chars, dcol, &shifted, &[], &cx.font, p);
        return Laid {
            galley,
            map,
            from: first,
            x0: dcol as f32 * cx.char_w,
            long: Some((text, cols, cx.char_w)),
        };
    }
    let mut here: Vec<(usize, &str)> = cx
        .hints
        .iter()
        .filter(|(pos, _)| pos.line == line)
        .map(|(pos, label)| (pos.col, label.as_str()))
        .collect();
    here.sort_by_key(|h| h.0);
    let chars: Vec<char> = text.chars().collect();
    let (galley, map) = lay(ui, &chars, 0, colours, &here, &cx.font, p);
    Laid {
        galley,
        map,
        from: 0,
        x0: 0.0,
        long: None,
    }
}

/// Lays out a line's characters in their colours (character columns),
/// with the hints at their columns (`st-inlay`); `tab_col` is the display
/// column of the first, for its tabs' stops. Returns the layout and each
/// character's place in it.
fn lay(
    ui: &Ui,
    chars: &[char],
    mut tab_col: usize,
    highlights: &[(usize, usize, Syntax)],
    hints: &[(usize, &str)],
    font: &FontId,
    p: &Palette,
) -> (Arc<Galley>, Vec<usize>) {
    let mut job = LayoutJob::default();
    let mut map = Vec::with_capacity(chars.len() + 1);
    let mut index = 0;
    let mut run = String::new();
    let mut run_style: Option<Syntax> = None;
    let flush = |job: &mut LayoutJob, run: &mut String, style_of: Option<Syntax>| {
        if !run.is_empty() {
            let format = match style_of {
                Some(s) => style(s, font, p),
                None => TextFormat::simple(font.clone(), p.ink),
            };
            job.append(run, 0.0, format);
            run.clear();
        }
    };
    let hint_format = TextFormat {
        font_id: Face::Plex.at(11.0),
        color: p.ink_muted,
        background: p.bg_300,
        valign: egui::Align::Center,
        ..Default::default()
    };
    let mut h = 0;
    // The highlights are in order: the one at a column is found by
    // walking on, not by searching them all for each character.
    let mut next = 0;
    for col in 0..=chars.len() {
        while let Some(&(at, label)) = hints.get(h)
            && at == col
        {
            flush(&mut job, &mut run, run_style);
            let text = format!("\u{2009}{label}\u{2009}");
            index += text.chars().count();
            job.append(&text, 0.0, hint_format.clone());
            h += 1;
        }
        map.push(index);
        let Some(&c) = chars.get(col) else {
            break;
        };
        while highlights.get(next).is_some_and(|x| x.1 <= col) {
            next += 1;
        }
        let st = highlights
            .get(next)
            .filter(|x| x.0 <= col && col < x.1)
            .map(|x| x.2);
        if st != run_style {
            flush(&mut job, &mut run, run_style);
            run_style = st;
        }
        if c == '\t' {
            let n = TAB_STOP - tab_col % TAB_STOP;
            run.extend(std::iter::repeat_n(' ', n));
            index += n;
            tab_col += n;
        } else {
            run.push(c);
            index += 1;
            tab_col += 1;
        }
    }
    flush(&mut job, &mut run, run_style);
    job.wrap.max_rows = 1;
    (ui.fonts_mut(|f| f.layout_job(job)), map)
}

/// The code: the gutter and the visible rows; the keyboard, the pointer
/// and the popups.
#[allow(clippy::too_many_arguments)]
fn code(
    ui: &mut Ui,
    rect: Rect,
    tab: &mut Tab,
    cx: &Cx<'_>,
    focus: bool,
    index: usize,
    found: &[(Pos, Pos)],
    actions: &mut Vec<Action>,
) {
    let p = palette(ui.ctx());
    let ctx = ui.ctx().clone();
    let id = Id::unique(("wowlua-studio-editor", &tab.doc.path));
    let font = Face::Plex.at(FONT_SIZE);
    let char_w = ui.fonts_mut(|f| f.glyph_width(&font, 'M'));
    let text_rect = Rect::from_min_max(Pos2::new(rect.left() + GUTTER, rect.top()), rect.max);
    let now = ctx.input(|i| i.time);

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
                    escape: true,
                },
            );
        });
        let changed = keyboard(ui, tab, cx.mac, (rect.height() / LINE_H) as usize, actions);
        if changed.save {
            actions.push(Action::Save(index));
        }
        if changed.moved {
            tab.reveal = true;
            tab.hover = None;
            ctx.data_mut(|d| d.insert_temp(id.with("typed"), now));
        }
    }

    // The caret in a fold opens it.
    let head_line = tab.doc.selection.head.line;
    tab.folded
        .retain(|&(a, b)| !(head_line > a && head_line <= b));

    let widest = tab.widest();
    tab.highlights(cx.system);
    tab.long_lines();
    let foldable: Vec<(usize, usize)> = tab.foldable(cx.system).to_vec();
    // Hints and lenses made for an older text stay until the new ones come,
    // moved on with the edits since: they do not blink while the check
    // runs. Those where the text was replaced go.
    let doc = &tab.doc;
    let hints: Vec<(Pos, String)> = tab
        .hints
        .as_ref()
        .map(|(v, h)| {
            h.iter()
                .filter_map(|(pos, label)| Some((doc.moved(*v, *pos)?, label.clone())))
                .collect()
        })
        .unwrap_or_default();
    let lenses: Vec<(usize, String, Pos)> = tab
        .lenses
        .as_ref()
        .map(|(v, l)| {
            l.iter()
                .filter_map(|(line, title, at)| {
                    let line = doc.moved(*v, Pos::new(*line, 0))?.line;
                    Some((line, title.clone(), doc.moved(*v, *at)?))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut lens_lines: Vec<usize> = lenses.iter().map(|l| l.0).collect();
    lens_lines.sort_unstable();
    lens_lines.dedup();
    let highlights: &[Vec<(usize, usize, Syntax)>] =
        tab.highlights.as_ref().map_or(&[], |(_, h)| h.as_slice());
    let folded = tab.folded.clone();
    let doc = &mut tab.doc;
    let lines = doc.line_count();
    let rows = Rows::new(lines, &folded, &lens_lines);
    let content = Vec2::new(
        GUTTER.max(widest as f32 * char_w + 160.0),
        rows.height + 24.0 + rect.height() * 0.5,
    );
    let mut child = ui.new_child(UiBuilder::new().max_rect(text_rect));
    child.set_clip_rect(text_rect);
    let mut reveal = std::mem::take(&mut tab.reveal);
    let mut caret_screen = None;
    let mut hover_at: Option<(Pos, Pos2)> = None;
    let mut clear_hover = false;
    let mut unfold: Option<usize> = None;
    // cmd held over a name with a definition: it is underlined, as a link.
    let cmd = ctx.input(|i| i.modifiers.command);
    let underline = tab
        .link
        .as_ref()
        .filter(|l| cmd && l.found == Some(true) && l.version == doc.version)
        .map(|l| (l.line, l.start, l.end));
    let lay_cx = LayCx {
        highlights,
        hints: &hints,
        long: tab.long.as_ref().map(|(_, l)| l),
        font: font.clone(),
        char_w,
    };
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
            let pos_at = |ui: &Ui, doc: &Document, point: Pos2| match rows.at(point.y - origin.y) {
                Row::Lens(line) => Pos::new(line, 0),
                Row::Line(line) => Pos::new(
                    line,
                    lay_line(ui, doc, line, viewport.x_range(), &lay_cx, p)
                        .col_at(point.x - origin.x),
                ),
            };

            // The pointer: place the caret, select by dragging, words and
            // lines by double and triple clicks; go to a definition with
            // cmd (ctrl) and a click.
            if let Some(pointer) = response.interact_pointer_pos() {
                let at = pos_at(ui, doc, pointer);
                let pressed = ui.input(|i| i.pointer.primary_pressed());
                let modifiers = ui.input(|i| i.modifiers);
                if pressed && response.is_pointer_button_down_on() {
                    ctx.memory_mut(|m| m.request_focus(id));
                    doc.selection = if modifiers.shift {
                        Selection {
                            anchor: doc.selection.anchor,
                            head: at,
                        }
                    } else {
                        Selection::caret(at)
                    };
                    doc.goal_col = None;
                    if modifiers.command {
                        actions.push(Action::Definition(at));
                    }
                } else if response.dragged() {
                    doc.selection.head = at;
                    reveal = true;
                }
                if response.triple_clicked() {
                    let end = if at.line + 1 < lines {
                        Pos::new(at.line + 1, 0)
                    } else {
                        Pos::new(at.line, doc.line_len(at.line))
                    };
                    doc.selection = Selection {
                        anchor: Pos::new(at.line, 0),
                        head: end,
                    };
                } else if response.double_clicked() {
                    let (start, end) = word_at(doc.line(at.line), at.col);
                    doc.selection = Selection {
                        anchor: Pos::new(at.line, start),
                        head: Pos::new(at.line, end),
                    };
                }
                ctx.data_mut(|d| d.insert_temp(id.with("typed"), now));
                clear_hover = true;
            } else if let Some(point) = response.hover_pos() {
                if matches!(rows.at(point.y - origin.y), Row::Line(_)) {
                    hover_at = Some((pos_at(ui, doc, point), point));
                }
            } else {
                clear_hover = true;
            }

            let (sel_start, sel_end) = doc.selection.range();
            let head = doc.selection.head;
            for &(row, top) in rows.between(viewport.min.y, viewport.max.y) {
                let y = origin.y + top;
                let line = row.line();
                let laid = lay_line(ui, doc, line, viewport.x_range(), &lay_cx, p);
                let text = doc.line(line);
                if let Row::Lens(_) = row {
                    // The lens: how many places refer to the function.
                    let indent = text.chars().take_while(|c| c.is_whitespace()).count();
                    let mut x = origin.x + laid.x(indent);
                    for (_, title, at) in lenses.iter().filter(|l| l.0 == line) {
                        let r = ui.painter().text(
                            Pos2::new(x, y + LENS_H / 2.0),
                            Align2::LEFT_CENTER,
                            title,
                            Face::Plex.at(11.0),
                            p.ink_muted,
                        );
                        ui.painter().hline(
                            r.x_range(),
                            r.bottom() + 1.0,
                            Stroke::new(1.0, p.ink_faint),
                        );
                        let resp = ui.interact(
                            r.expand(2.0),
                            id.with(("lens", line, title)),
                            Sense::click(),
                        );
                        if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                            actions.push(Action::References(*at));
                        }
                        x = r.right() + 12.0;
                    }
                    continue;
                }
                let row_rect = Rect::from_min_size(
                    Pos2::new(viewport.min.x + origin.x, y),
                    Vec2::new(viewport.width().max(full.width()), LINE_H),
                );
                let line_diags: Vec<&Diag> = cx
                    .diagnostics
                    .iter()
                    .filter(|d| d.start.line <= line && line <= d.end.line)
                    .collect();
                if line_diags
                    .iter()
                    .any(|d| d.is_error() && d.start.line == line)
                {
                    ui.painter()
                        .rect_filled(row_rect, CornerRadius::ZERO, p.error_soft);
                } else if line == head.line && doc.selection.is_empty() {
                    ui.painter()
                        .rect_filled(row_rect, CornerRadius::ZERO, p.bg_100);
                }
                // The find bar's matches on this line; the current one
                // outlined.
                let first = found.partition_point(|m| m.1.line < line);
                for m in found[first..].iter().take_while(|m| m.0.line <= line) {
                    let from = if m.0.line == line { m.0.col } else { 0 };
                    let to = if m.1.line == line {
                        m.1.col
                    } else {
                        doc.line_len(line)
                    };
                    let r = Rect::from_min_max(
                        Pos2::new(origin.x + laid.x(from), y + 1.0),
                        Pos2::new(
                            origin.x + laid.x(to).max(laid.x(from) + 2.0),
                            y + LINE_H - 1.0,
                        ),
                    );
                    ui.painter()
                        .rect_filled(r, CornerRadius::ZERO, p.amber_soft);
                    if (m.0, m.1) == (sel_start, sel_end) {
                        ui.painter().rect_stroke(
                            r,
                            CornerRadius::ZERO,
                            Stroke::new(1.0, p.amber_ink),
                            egui::StrokeKind::Inside,
                        );
                    }
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
                    let x0 = laid.x(from);
                    let mut x1 = laid.x(to);
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
                let text_y = y + (LINE_H - laid.galley.size().y) / 2.0;
                ui.painter()
                    .galley(Pos2::new(origin.x, text_y), laid.galley.clone(), p.ink);
                // The name cmd + click goes to the definition of.
                if let Some((l, s, e)) = underline
                    && l == line
                {
                    ui.painter().hline(
                        origin.x + laid.x(s)..=origin.x + laid.x(e),
                        y + LINE_H - 3.0,
                        Stroke::new(1.0, p.ink),
                    );
                }
                // A folded line ends in `⋯`: a click opens it.
                if let Some(&(_, last)) = folded.iter().find(|f| f.0 == line) {
                    let x = origin.x + laid.galley.size().x + 8.0;
                    let chip =
                        Rect::from_min_size(Pos2::new(x, y + 3.0), Vec2::new(28.0, LINE_H - 6.0));
                    ui.painter()
                        .rect_filled(chip, CornerRadius::same(2), p.bg_300);
                    ui.painter().text(
                        chip.center(),
                        Align2::CENTER_CENTER,
                        "⋯",
                        Face::Plex.at(12.0),
                        p.ink_muted,
                    );
                    let resp = ui.interact(chip, id.with(("fold-chip", line)), Sense::click());
                    if resp
                        .on_hover_text(format!("lines {}–{}", line + 2, last + 1))
                        .clicked()
                    {
                        unfold = Some(line);
                    }
                }
                // The problems, underlined in waves (`st-sq-err`, `st-sq-warn`).
                for d in line_diags {
                    let from = if d.start.line == line {
                        lsp::from_wire(doc, d.start).col
                    } else {
                        0
                    };
                    let to = if d.end.line == line {
                        lsp::from_wire(doc, d.end).col
                    } else {
                        doc.line_len(line)
                    };
                    let x0 = origin.x + laid.x(from);
                    let x1 = (origin.x + laid.x(to)).max(x0 + char_w);
                    let color = if d.is_error() { p.error } else { p.warning };
                    wave(ui, x0, x1, y + LINE_H - 3.0, color);
                }
                if line == head.line {
                    let x = origin.x + laid.x(head.col);
                    caret_screen = Some(Rect::from_min_size(
                        Pos2::new(x, y + 2.0),
                        Vec2::new(2.0, LINE_H - 4.0),
                    ));
                }
            }
            if reveal {
                let laid = lay_line(ui, doc, head.line, viewport.x_range(), &lay_cx, p);
                let x = origin.x + laid.x(head.col);
                let y = origin.y + rows.y_of(head.line);
                ui.scroll_to_rect(
                    Rect::from_min_size(
                        Pos2::new(x - 24.0, y - LINE_H),
                        Vec2::new(48.0, LINE_H * 3.0),
                    ),
                    None,
                );
            }
        });
    if let Some(line) = unfold {
        tab.folded.retain(|f| f.0 != line);
    }

    // The name under the pointer while cmd is held: asked once whether it
    // has a definition; a hand over it when it has.
    let pointed = hover_at.filter(|_| cmd).and_then(|(at, _)| {
        let text = tab.doc.line(at.line);
        let (start, end) = word_at(text, at.col);
        let name = text
            .chars()
            .skip(start)
            .take(end - start)
            .any(|c| c.is_alphanumeric() || c == '_');
        name.then_some((at.line, start, end))
    });
    match pointed {
        Some((line, start, end)) => {
            let version = tab.doc.version;
            let same = tab.link.as_ref().is_some_and(|l| {
                (l.version, l.line, l.start, l.end) == (version, line, start, end)
            });
            if !same {
                tab.link = Some(super::Link {
                    version,
                    line,
                    start,
                    end,
                    found: None,
                });
                actions.push(Action::Linkable(Pos::new(line, start)));
            }
            if underline == Some((line, start, end)) {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
            }
        }
        None => tab.link = None,
    }

    // The hover: asked for once the pointer rests on a place.
    let popup_hovered: bool = ctx
        .data(|d| d.get_temp(id.with("hover-pop")))
        .unwrap_or(false);
    // The pointer left the hover's place: it stays a moment, to be
    // reached; on it, it stays.
    let left = tab
        .hover
        .as_ref()
        .is_some_and(|h| hover_at.is_none_or(|(at, _)| h.at != at))
        && (hover_at.is_some() || clear_hover);
    if left && !popup_hovered {
        let since: f64 = ctx.data(|d| d.get_temp(id.with("leave"))).unwrap_or(now);
        ctx.data_mut(|d| d.insert_temp(id.with("leave"), since));
        if now - since >= HOVER_LEAVE {
            tab.hover = None;
            ctx.data_mut(|d| {
                d.remove::<f64>(id.with("leave"));
                d.remove::<Pos>(id.with("asked"));
            });
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                HOVER_LEAVE - (now - since),
            ));
        }
    } else {
        ctx.data_mut(|d| d.remove::<f64>(id.with("leave")));
    }
    if let Some((at, point)) = hover_at {
        let rest: Option<(Pos, f64)> = ctx.data(|d| d.get_temp(id.with("rest")));
        match rest {
            Some((pos, since)) if pos == at => {
                let asked: Option<Pos> = ctx.data(|d| d.get_temp(id.with("asked")));
                if now - since >= HOVER_AFTER && asked != Some(at) && !popup_hovered {
                    actions.push(Action::Hover(at));
                    // The popup stands where the pointer rested: it does
                    // not follow the pointer on its way to it.
                    ctx.data_mut(|d| {
                        d.insert_temp(id.with("asked"), at);
                        d.insert_temp(id.with("point"), point);
                    });
                } else if now - since < HOVER_AFTER {
                    ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                        HOVER_AFTER - (now - since),
                    ));
                }
            }
            _ => {
                ctx.data_mut(|d| d.insert_temp(id.with("rest"), (at, now)));
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(HOVER_AFTER));
            }
        }
    }

    // The caret: 2 px of amber, blinking while nothing is typed.
    if has_focus && let Some(caret) = caret_screen {
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

    // The gutter, beside the scrolled rows: line numbers and the folds.
    let offset = output.state.clamped_offset().y;
    let gutter = Rect::from_min_max(rect.min, Pos2::new(rect.left() + GUTTER, rect.bottom()));
    let painter = ui.painter().with_clip_rect(gutter);
    painter.rect_filled(gutter, CornerRadius::ZERO, p.bg_000);
    let gutter_hovered = ctx
        .input(|i| i.pointer.hover_pos())
        .is_some_and(|pt| gutter.contains(pt));
    let head_line = tab.doc.selection.head.line;
    let mut toggle = None;
    for &(row, top) in rows.between(offset, offset + rect.height()) {
        let Row::Line(line) = row else {
            continue;
        };
        let cy = rect.top() + top - offset + LINE_H / 2.0;
        let color = if line == head_line {
            p.ink
        } else {
            p.ink_muted
        };
        painter.text(
            Pos2::new(gutter.right() - 16.0, cy),
            Align2::RIGHT_CENTER,
            (line + 1).to_string(),
            Face::Plex.at(FONT_SIZE),
            color,
        );
        if let Some(&range) = foldable.iter().find(|f| f.0 == line) {
            let is_folded = folded.iter().any(|f| f.0 == line);
            if is_folded || gutter_hovered {
                let r =
                    Rect::from_center_size(Pos2::new(gutter.right() - 7.0, cy), Vec2::splat(12.0));
                let resp = ui.interact(r, id.with(("fold", line)), Sense::click());
                painter.text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    if is_folded { "+" } else { "−" },
                    Face::Plex.at(12.0),
                    if resp.hovered() { p.ink } else { p.ink_muted },
                );
                if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    toggle = Some(range);
                }
            }
        }
    }
    if let Some(range) = toggle {
        if tab.folded.iter().any(|f| f.0 == range.0) {
            tab.folded.retain(|f| f.0 != range.0);
        } else {
            tab.folded.push(range);
        }
    }

    // The popups, over everything.
    let caret = caret_screen.filter(|c| text_rect.expand(4.0).intersects(*c));
    hover_popup(&ctx, id, tab, cx.diagnostics, p);
    if let Some(caret) = caret {
        signature_popup(&ctx, tab, caret, p);
        completion_popup(&ctx, tab, caret, p);
        fixes_popup(&ctx, tab, caret, p, actions);
        rename_popup(&ctx, id, tab, caret, p, actions);
    }
}

/// A wavy line from `x0` to `x1` at `y`.
fn wave(ui: &Ui, x0: f32, x1: f32, y: f32, color: Color32) {
    let mut points = Vec::new();
    let mut x = x0;
    let mut up = true;
    while x <= x1 {
        points.push(Pos2::new(x, if up { y - 1.25 } else { y + 1.25 }));
        x += 2.0;
        up = !up;
    }
    if points.len() >= 2 {
        ui.painter()
            .add(Shape::line(points, Stroke::new(1.0, color)));
    }
}

/// A popup's frame: `bg-200`, `line-strong`, the pop shadow.
fn popup<R>(
    ctx: &egui::Context,
    id: Id,
    pos: Pos2,
    pivot: Align2,
    width: f32,
    add: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<R> {
    let p = palette(ctx);
    egui::Area::new(id)
        .order(Order::Foreground)
        .pivot(pivot)
        .fixed_pos(pos)
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(p.bg_200)
                .stroke(Stroke::new(1.0, p.line_strong))
                .shadow(ui.visuals().popup_shadow)
                .show(ui, |ui| {
                    ui.set_max_width(width);
                    add(ui)
                })
                .inner
        })
}

/// The hover: the problems at the place, then the server's text.
fn hover_popup(ctx: &egui::Context, id: Id, tab: &Tab, diagnostics: &[Diag], p: &Palette) {
    let Some(super::Hover { at, text, code }) = &tab.hover else {
        ctx.data_mut(|d| d.insert_temp(id.with("hover-pop"), false));
        return;
    };
    let wire = lsp::to_wire(&tab.doc, *at);
    let here: Vec<&Diag> = diagnostics
        .iter()
        .filter(|d| d.start <= wire && wire <= d.end)
        .collect();
    if text.trim().is_empty() && here.is_empty() {
        return;
    }
    let point: Pos2 = ctx
        .data(|d| d.get_temp(id.with("point")))
        .unwrap_or_default();
    let shown = popup(
        ctx,
        id.with("hover"),
        point + Vec2::new(0.0, -10.0),
        Align2::LEFT_BOTTOM,
        560.0,
        |ui| {
            // A long hover (a function's many overloads) scrolls in it.
            let max_h = (ctx.content_rect().height() - 48.0).min(HOVER_MAX_H);
            egui::ScrollArea::vertical()
                .id_salt(("wowlua-studio-hover-scroll", at.line, at.col))
                .max_height(max_h)
                .show(ui, |ui| hover_body(ui, &here, text, code, p));
        },
    );
    let over = shown.response.contains_pointer();
    ctx.data_mut(|d| d.insert_temp(id.with("hover-pop"), over));
}

/// The hover's problems, then the server's text.
fn hover_body(ui: &mut Ui, here: &[&Diag], text: &str, code: &[Highlight], p: &Palette) {
    ui.spacing_mut().item_spacing.y = 0.0;
    for d in here {
        forms::padded(ui, 12, 8, |ui| {
            let (word, color) = if d.is_error() {
                ("error", p.error)
            } else {
                ("warning", p.warning)
            };
            let head = match &d.code {
                Some(code) => format!("{word}[{code}]"),
                None => word.to_owned(),
            };
            let mut lines = d.message.lines();
            let first = lines.next().unwrap_or("");
            let width = ui.available_width();
            let g = rich(
                ui,
                &[
                    (&head, Face::PlexSemiBold.at(12.0), color),
                    (&format!(": {first}"), Face::Plex.at(12.0), p.ink),
                ],
                width,
            );
            let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
            ui.painter().galley(r.min, g, p.ink);
            for rest in lines {
                let (prefix, body) = match rest.strip_prefix("help: ") {
                    Some(h) => ("= help: ", h),
                    None => ("", rest),
                };
                let g = rich(
                    ui,
                    &[
                        (prefix, Face::PlexSemiBold.at(12.0), p.note),
                        (body, Face::Plex.at(12.0), p.ink_muted),
                    ],
                    width,
                );
                let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
                ui.painter().galley(r.min, g, p.ink);
            }
        });
        separator(ui, p);
    }
    if !text.trim().is_empty() {
        forms::padded(ui, 12, 8, |ui| markdown(ui, text, code, p));
    }
}

fn separator(ui: &mut Ui, p: &Palette) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .hline(r.x_range(), r.center().y, Stroke::new(1.0, p.line));
}

/// The server's markdown, simply: code blocks in the code font on
/// `bg-000`, coloured as the editor colours them (`colours`: byte ranges
/// in `text`), the rest as text without its marks.
fn markdown(ui: &mut Ui, text: &str, colours: &[Highlight], p: &Palette) {
    ui.spacing_mut().item_spacing.y = 4.0;
    let font = Face::Plex.at(12.5);
    // The code block being read: where it starts in `text`, and its end.
    let mut code: Option<(usize, usize)> = None;
    let flush_code = |ui: &mut Ui, (start, end): (usize, usize)| {
        let block = text[start..end].trim_end();
        if block.is_empty() {
            return;
        }
        let mut job = LayoutJob::default();
        let mut at = start;
        for h in colours {
            let (s, e) = (h.start.max(at), h.end.min(start + block.len()));
            if s >= e {
                continue;
            }
            job.append(&text[at..s], 0.0, TextFormat::simple(font.clone(), p.ink));
            job.append(&text[s..e], 0.0, style(h.syntax, &font, p));
            at = e;
        }
        job.append(
            &text[at..start + block.len()],
            0.0,
            TextFormat::simple(font.clone(), p.ink),
        );
        egui::Frame::NONE
            .fill(p.bg_000)
            .inner_margin(egui::Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(job);
            });
    };
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        at += line.len();
        if line.trim_start().starts_with("```") {
            code = match code {
                Some(block) => {
                    flush_code(ui, block);
                    None
                }
                None => Some((at, at)),
            };
            continue;
        }
        if let Some((_, end)) = &mut code {
            *end = at;
        } else if !line.trim().is_empty() && line.trim() != "---" {
            let plain = line.trim_end().replace("**", "").replace('`', "");
            linked_paragraph(ui, plain.trim_start_matches("# "), p);
        }
    }
    if let Some(block) = code {
        flush_code(ui, block);
    }
}

/// A web link of markdown (`[text](https://…)`): where it is in `line`
/// (bytes), its text and its address. Other addresses stay text.
fn links(line: &str) -> Vec<(std::ops::Range<usize>, &str, &str)> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(open) = line[at..].find('[').map(|i| at + i) {
        let Some(close) = line[open..].find("](").map(|i| open + i) else {
            break;
        };
        let Some(end) = line[close + 2..].find(')').map(|i| close + 2 + i) else {
            break;
        };
        let url = &line[close + 2..end];
        if url.starts_with("https://") || url.starts_with("http://") {
            out.push((open..end + 1, &line[open + 1..close], url));
        }
        at = end + 1;
    }
    out
}

/// A paragraph of the hover's text, its links in the note colour and
/// underlined: a click opens one in the browser.
fn linked_paragraph(ui: &mut Ui, line: &str, p: &Palette) {
    let found = links(line);
    if found.is_empty() {
        forms::paragraph(ui, line, 12.0, 18.0, p.ink);
        return;
    }
    let text = TextFormat {
        font_id: Face::Plex.at(12.0),
        color: p.ink,
        line_height: Some(18.0),
        ..Default::default()
    };
    let link = TextFormat {
        color: p.note,
        underline: Stroke::new(1.0, p.note),
        ..text.clone()
    };
    let mut job = LayoutJob::default();
    job.wrap.max_width = ui.available_width();
    // Each link's characters in the laid text, and its address.
    let mut targets = Vec::new();
    let (mut at, mut chars) = (0, 0);
    for (range, label, url) in found {
        let before = &line[at..range.start];
        job.append(before, 0.0, text.clone());
        chars += before.chars().count();
        job.append(label, 0.0, link.clone());
        let n = label.chars().count();
        targets.push((chars..chars + n, url));
        chars += n;
        at = range.end;
    }
    job.append(&line[at..], 0.0, text);
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let (rect, response) = ui.allocate_exact_size(galley.size(), Sense::click());
    if let Some(pos) = response.hover_pos() {
        let i = galley.cursor_from_pos(pos - rect.min).index.0;
        if let Some((_, url)) = targets.iter().find(|(r, _)| r.contains(&i)) {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            if response.clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(*url));
            }
        }
    }
    ui.painter().galley(rect.min, galley, p.ink);
}

/// The call's signature over the caret, the active parameter in amber.
fn signature_popup(ctx: &egui::Context, tab: &Tab, caret: Rect, p: &Palette) {
    let Some((label, active)) = &tab.signature else {
        return;
    };
    popup(
        ctx,
        Id::unique("wowlua-studio-signature"),
        Pos2::new(caret.left(), caret.top() - 4.0),
        Align2::LEFT_BOTTOM,
        640.0,
        |ui| {
            forms::padded(ui, 10, 6, |ui| {
                let mut job = LayoutJob::default();
                let chars: Vec<char> = label.chars().collect();
                let (a, b) = active.unwrap_or((chars.len(), chars.len()));
                let part = |from: usize, to: usize| {
                    chars[from.min(chars.len())..to.min(chars.len())]
                        .iter()
                        .collect::<String>()
                };
                job.append(
                    &part(0, a),
                    0.0,
                    TextFormat::simple(Face::Plex.at(12.5), p.ink_muted),
                );
                job.append(
                    &part(a, b),
                    0.0,
                    TextFormat::simple(Face::PlexSemiBold.at(12.5), p.amber_ink),
                );
                job.append(
                    &part(b, chars.len()),
                    0.0,
                    TextFormat::simple(Face::Plex.at(12.5), p.ink_muted),
                );
                job.wrap.max_width = 620.0;
                let g = ui.fonts_mut(|f| f.layout_job(job));
                let (r, _) = ui.allocate_exact_size(g.size(), Sense::hover());
                ui.painter().galley(r.min, g, p.ink);
            });
        },
    );
}

/// The suggestions under the caret, filtered by the word typed.
fn completion_popup(ctx: &egui::Context, tab: &mut Tab, caret: Rect, p: &Palette) {
    let prefix = word_before(&tab.doc).1;
    let Some(c) = &mut tab.completion else {
        return;
    };
    let shown = filtered(c, &prefix);
    if shown.is_empty() {
        return;
    }
    c.selected = c.selected.min(shown.len() - 1);
    let rows = 10usize;
    let first = c.selected.saturating_sub(rows - 1);
    let mut picked = None;
    popup(
        ctx,
        Id::unique("wowlua-studio-completion"),
        Pos2::new(caret.left() - 8.0, caret.bottom() + 4.0),
        Align2::LEFT_TOP,
        460.0,
        |ui| {
            ui.set_width(460.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            for (i, &item_i) in shown.iter().enumerate().skip(first).take(rows) {
                let item = &c.items[item_i];
                let (r, resp) = ui.allocate_exact_size(Vec2::new(460.0, 24.0), Sense::click());
                if i == c.selected {
                    ui.painter().rect_filled(r, CornerRadius::ZERO, p.bg_400);
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, CornerRadius::ZERO, p.bg_300);
                }
                let painter = ui.painter().with_clip_rect(r);
                painter.text(
                    Pos2::new(r.left() + 10.0, r.center().y),
                    Align2::LEFT_CENTER,
                    kind_word(item.kind),
                    Face::Plex.at(11.0),
                    p.ink_muted,
                );
                let label = painter.text(
                    Pos2::new(r.left() + 62.0, r.center().y),
                    Align2::LEFT_CENTER,
                    &item.label,
                    Face::Plex.at(12.5),
                    p.ink,
                );
                // The detail in the room the label leaves, cut with `…`.
                let room = r.right() - 10.0 - (label.right() + 16.0);
                if room > 24.0 && !item.detail.is_empty() {
                    let mut job = LayoutJob::simple_singleline(
                        item.detail.clone(),
                        Face::Plex.at(11.5),
                        p.ink_muted,
                    );
                    job.wrap = egui::text::TextWrapping {
                        max_width: room,
                        max_rows: 1,
                        break_anywhere: true,
                        overflow_character: Some('…'),
                    };
                    let g = painter.layout_job(job);
                    let at = Pos2::new(
                        r.right() - 10.0 - g.size().x,
                        r.center().y - g.size().y / 2.0,
                    );
                    painter.galley(at, g, p.ink_muted);
                }
                if resp.clicked() {
                    picked = Some(i);
                }
            }
        },
    );
    if let Some(i) = picked {
        c.selected = i;
        accept(tab);
    }
}

/// The quick fixes at the caret: one is chosen, enter applies it.
fn fixes_popup(
    ctx: &egui::Context,
    tab: &mut Tab,
    caret: Rect,
    p: &Palette,
    actions: &mut Vec<Action>,
) {
    let Some((fixes, selected)) = &mut tab.fixes else {
        return;
    };
    let mut picked = None;
    popup(
        ctx,
        Id::unique("wowlua-studio-fixes"),
        Pos2::new(caret.left() - 8.0, caret.bottom() + 4.0),
        Align2::LEFT_TOP,
        420.0,
        |ui| {
            ui.set_width(420.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            for (i, fix) in fixes.iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(Vec2::new(420.0, 26.0), Sense::click());
                if i == *selected {
                    ui.painter().rect_filled(r, CornerRadius::ZERO, p.bg_400);
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, CornerRadius::ZERO, p.bg_300);
                }
                ui.painter().with_clip_rect(r).text(
                    Pos2::new(r.left() + 10.0, r.center().y),
                    Align2::LEFT_CENTER,
                    &fix.title,
                    Face::Plex.at(12.5),
                    p.ink,
                );
                if resp.clicked() {
                    picked = Some(i);
                }
            }
        },
    );
    if let Some(i) = picked {
        *selected = i;
        if let Some((fixes, i)) = tab.fixes.take() {
            actions.push(Action::ApplyFix(fixes[i].clone()));
        }
    }
}

/// The new name for a rename, at the caret.
fn rename_popup(
    ctx: &egui::Context,
    id: Id,
    tab: &mut Tab,
    caret: Rect,
    p: &Palette,
    actions: &mut Vec<Action>,
) {
    let Some((at, name)) = &mut tab.rename else {
        return;
    };
    let at = *at;
    let edit_id = id.with("rename");
    let mut done = None;
    popup(
        ctx,
        Id::unique("wowlua-studio-rename"),
        Pos2::new(caret.left() - 8.0, caret.bottom() + 4.0),
        Align2::LEFT_TOP,
        320.0,
        |ui| {
            forms::padded(ui, 8, 8, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.label(
                    egui::RichText::new("rename to")
                        .font(Face::PlexMedium.at(11.5))
                        .color(p.ink_muted),
                );
                let resp = ui.add(
                    egui::TextEdit::singleline(name)
                        .id(edit_id)
                        .font(Face::Plex.at(13.0))
                        .desired_width(300.0),
                );
                if !resp.has_focus() && !resp.lost_focus() {
                    resp.request_focus();
                }
                if ui.input(|i| i.key_pressed(Key::Enter)) {
                    done = Some(true);
                } else if ui.input(|i| i.key_pressed(Key::Escape)) || resp.lost_focus() {
                    done = Some(false);
                }
            });
        },
    );
    match done {
        Some(true) => {
            let name = name.trim().to_owned();
            tab.rename = None;
            if !name.is_empty() {
                actions.push(Action::Rename(at, name));
            }
            ctx.memory_mut(|m| m.request_focus(id));
        }
        Some(false) => {
            tab.rename = None;
            ctx.memory_mut(|m| m.request_focus(id));
        }
        None => {}
    }
}

/// The word for an LSP completion kind.
fn kind_word(kind: u8) -> &'static str {
    match kind {
        2 | 3 => "fn",
        4 => "new",
        5 | 10 => "field",
        6 => "local",
        7 | 22 => "class",
        8 => "type",
        9 => "module",
        12 => "value",
        14 => "keyword",
        21 => "const",
        _ => "",
    }
}

/// The items that match `prefix`: those it starts first.
fn filtered(c: &Completion, prefix: &str) -> Vec<usize> {
    let lower = prefix.to_lowercase();
    let mut starts: Vec<usize> = Vec::new();
    let mut contains: Vec<usize> = Vec::new();
    for (i, item) in c.items.iter().enumerate() {
        let label = item.label.to_lowercase();
        if label.starts_with(&lower) {
            starts.push(i);
        } else if !lower.is_empty() && label.contains(&lower) {
            contains.push(i);
        }
    }
    starts.extend(contains);
    starts
}

/// The identifier the caret ends: where it starts, and its text.
fn word_before(doc: &Document) -> (usize, String) {
    let head = doc.selection.head;
    let chars: Vec<char> = doc.line(head.line).chars().take(head.col).collect();
    let mut start = chars.len();
    while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
        start -= 1;
    }
    (start, chars[start..].iter().collect())
}

/// Puts the chosen suggestion in.
fn accept(tab: &mut Tab) {
    let Some(c) = tab.completion.take() else {
        return;
    };
    let (word_start, prefix) = word_before(&tab.doc);
    let shown = filtered(&c, &prefix);
    let Some(&i) = shown.get(c.selected) else {
        return;
    };
    let item = &c.items[i];
    let head = tab.doc.selection.head;
    let start = match item.range {
        Some((start, _)) if start.line == head.line => lsp::from_wire(&tab.doc, start),
        _ => Pos::new(head.line, word_start),
    };
    tab.doc.edit(start, head, &item.insert, EditKind::Other);
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

/// The character at display column `dcol` (the nearest one's start).
fn char_col(line: &str, dcol: usize) -> usize {
    let (_, map) = display(line);
    map.iter().position(|&d| d >= dcol).unwrap_or(map.len() - 1)
}

/// The word around `col`: letters, digits and `_`, or a run of others.
pub(super) fn word_at(line: &str, col: usize) -> (usize, usize) {
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

/// The editor's keys and text input; the popups' keys first.
fn keyboard(ui: &Ui, tab: &mut Tab, mac: bool, page: usize, actions: &mut Vec<Action>) -> Changed {
    let mut changed = Changed::default();
    if tab.rename.is_some() {
        return changed;
    }
    let events = ui.input(|i| i.events.clone());
    for event in events {
        // The quick fixes take the keys that choose one.
        if let Some((fixes, selected)) = &mut tab.fixes
            && let Event::Key {
                key, pressed: true, ..
            } = &event
        {
            match key {
                Key::ArrowDown => {
                    *selected = (*selected + 1) % fixes.len().max(1);
                    continue;
                }
                Key::ArrowUp => {
                    *selected = (*selected + fixes.len().max(1) - 1) % fixes.len().max(1);
                    continue;
                }
                Key::Enter => {
                    if let Some((fixes, i)) = tab.fixes.take()
                        && let Some(fix) = fixes.get(i)
                    {
                        actions.push(Action::ApplyFix(fix.clone()));
                    }
                    continue;
                }
                Key::Escape => {
                    tab.fixes = None;
                    continue;
                }
                _ => tab.fixes = None,
            }
        }
        // The suggestions take the keys that choose one.
        if tab.completion.is_some()
            && let Event::Key {
                key, pressed: true, ..
            } = &event
        {
            let prefix = word_before(&tab.doc).1;
            let count = tab
                .completion
                .as_ref()
                .map_or(0, |c| filtered(c, &prefix).len());
            let c = tab.completion.as_mut().expect("open");
            match key {
                Key::ArrowDown => {
                    c.selected = (c.selected + 1) % count.max(1);
                    continue;
                }
                Key::ArrowUp => {
                    c.selected = (c.selected + count.max(1) - 1) % count.max(1);
                    continue;
                }
                Key::Enter | Key::Tab if count > 0 => {
                    accept(tab);
                    changed.moved = true;
                    continue;
                }
                Key::Escape => {
                    tab.completion = None;
                    continue;
                }
                _ => {}
            }
        }
        match event {
            Event::Text(text) => {
                tab.doc.insert(&text, EditKind::Typing);
                changed.moved = true;
                typed(tab, &text, actions);
            }
            Event::Ime(egui::ImeEvent::Commit(text)) if !text.is_empty() => {
                tab.doc.insert(&text, EditKind::Typing);
                changed.moved = true;
            }
            Event::Copy | Event::Cut => {
                let cut = event == Event::Cut;
                let doc = &mut tab.doc;
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
                tab.doc.insert(&text.replace("\r\n", "\n"), EditKind::Other);
                changed.moved = true;
            }
            Event::Key {
                key: Key::Escape,
                pressed: true,
                ..
            } => {
                tab.completion = None;
                tab.signature = None;
                tab.hover = None;
            }
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                let head = tab.doc.selection.head;
                match key {
                    Key::F12 if modifiers.shift => actions.push(Action::References(head)),
                    Key::F12 => actions.push(Action::Definition(head)),
                    Key::F2 => {
                        let (start, end) = word_at(tab.doc.line(head.line), head.col);
                        let word: String = tab
                            .doc
                            .line(head.line)
                            .chars()
                            .skip(start)
                            .take(end - start)
                            .collect();
                        tab.rename = Some((head, word));
                    }
                    Key::Space if modifiers.ctrl => actions.push(Action::Complete(head)),
                    Key::Period if modifiers.command => actions.push(Action::Fixes(head)),
                    _ => {
                        if key_press(&mut tab.doc, key, modifiers, mac, page) {
                            changed.moved = true;
                            if matches!(key, Key::Backspace | Key::Delete) {
                                if tab.completion.is_some() && word_before(&tab.doc).1.is_empty() {
                                    tab.completion = None;
                                }
                            } else {
                                tab.completion = None;
                                if matches!(key, Key::Enter) {
                                    tab.signature = None;
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    changed
}

/// Typing asks for suggestions and signatures.
fn typed(tab: &mut Tab, text: &str, actions: &mut Vec<Action>) {
    let Some(c) = text.chars().last() else {
        return;
    };
    let head = tab.doc.selection.head;
    match c {
        // A key or a string value of a JSON file with a schema.
        '"' if super::json::schema_of(&tab.doc.path).is_some() => {
            tab.completion = None;
            actions.push(Action::Complete(head));
        }
        '.' | ':' => {
            tab.completion = None;
            actions.push(Action::Complete(head));
        }
        '(' | ',' => actions.push(Action::Signature(head)),
        ')' => tab.signature = None,
        c if c.is_alphanumeric() || c == '_' => {
            // A word's first letter asks; so does each one after while the
            // server's list is cut (the game's names, for a short prefix).
            let first = tab.completion.is_none() && word_before(&tab.doc).1.chars().count() == 1;
            let cut = tab.completion.as_ref().is_some_and(|c| c.incomplete);
            if first || cut {
                actions.push(Action::Complete(head));
            }
        }
        _ => tab.completion = None,
    }
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
        Key::ArrowUp | Key::Home if doc_edge => Pos::default(),
        Key::ArrowDown | Key::End if doc_edge => doc.end(),
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
    let mut col = at.col.min(chars.len());
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
    use crate::lsp::Item;
    use std::path::PathBuf;

    fn doc(text: &str) -> Document {
        Document::new(PathBuf::from("a.wowlua"), text)
    }

    #[test]
    fn the_hover_s_web_links_are_found() {
        let line = "See [warcraft.wiki.gg](https://warcraft.wiki.gg/wiki/API_CreateFrame) and [local](./a.md), a [b] alone.";
        let found = links(line);
        assert_eq!(found.len(), 1);
        let (range, text, url) = &found[0];
        assert_eq!(*text, "warcraft.wiki.gg");
        assert_eq!(*url, "https://warcraft.wiki.gg/wiki/API_CreateFrame");
        assert_eq!(
            &line[range.clone()],
            "[warcraft.wiki.gg](https://warcraft.wiki.gg/wiki/API_CreateFrame)"
        );
        assert!(links("a [b] c").is_empty());
    }

    #[test]
    fn tabs_are_drawn_to_the_next_stop() {
        let (shown, map) = display("\ta\tb");
        assert_eq!(shown, "    a   b");
        assert_eq!(map, [0, 4, 5, 8, 9]);
        assert_eq!(char_col("\ta", 2), 1);
    }

    #[test]
    fn a_long_line_s_columns_are_found_from_its_marks() {
        // Tabs and a two-byte letter, across several marks.
        let line = "ä\tcd".repeat(1000);
        let cols = Columns::new(&line);
        let (_, map) = display(&line);
        for col in [0, 1, 2, 3, 1023, 1024, 1025, 2047, 2048, 3999, 4000] {
            assert_eq!(cols.dcol(&line, col), map[col], "column of {col}");
        }
        for dcol in [0, 1, 3, 4, 5, 4095, 4096, 4097, 9000, 9999] {
            assert_eq!(cols.col(&line, dcol), char_col(&line, dcol), "at {dcol}");
        }
        for col in [0, 1, 1025, 3999] {
            let byte = line.char_indices().nth(col).unwrap().0;
            assert_eq!(cols.byte(&line, col), byte, "byte of {col}");
        }
        assert_eq!(cols.byte(&line, 4000), line.len());
    }

    #[test]
    fn rows_leave_folds_out_and_put_lenses_in() {
        let rows = Rows::new(6, &[(1, 3)], &[4]);
        let kinds: Vec<Row> = rows.rows.iter().map(|r| r.0).collect();
        assert_eq!(
            kinds,
            [
                Row::Line(0),
                Row::Line(1),
                Row::Lens(4),
                Row::Line(4),
                Row::Line(5)
            ]
        );
        assert_eq!(rows.y_of(4), PAD_TOP + 2.0 * LINE_H + LENS_H);
        assert_eq!(rows.at(PAD_TOP + 2.0 * LINE_H + 1.0), Row::Lens(4));
        // A hidden line is drawn where its fold's first is.
        assert_eq!(rows.y_of(2), rows.y_of(1));
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

    #[test]
    fn a_suggestion_replaces_the_word_typed() {
        let mut tab = Tab::new(doc("self.fr"));
        tab.doc.selection = Selection::caret(Pos::new(0, 7));
        let item = |label: &str| Item {
            label: label.into(),
            detail: String::new(),
            kind: 5,
            insert: label.into(),
            range: None,
            sort: label.into(),
        };
        tab.completion = Some(Completion {
            at: Pos::new(0, 5),
            items: vec![item("name"), item("frame"), item("free")],
            selected: 1,
            incomplete: false,
        });
        // `fr` keeps frame and free; the second is free.
        assert_eq!(filtered(tab.completion.as_ref().unwrap(), "fr"), [1, 2]);
        accept(&mut tab);
        assert_eq!(tab.doc.text(), "self.free");
    }
}
