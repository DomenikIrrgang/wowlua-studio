//! Find and replace in the active file (the find board, S051): a bar over
//! the editor's top right, every match marked in the text and on the
//! scrollbar, the current one selected.

use egui::{
    Align2, Color32, CornerRadius, Event, Frame, Id, Key, Margin, Modifiers, Pos2, Rect, Sense,
    Stroke, StrokeKind, TextEdit, Ui, UiBuilder, Vec2,
    text::{CCursor, CCursorRange},
};
use wowlua_studio_ui::{Face, Palette};

use super::Tab;
use super::doc::{Document, Pos, Selection};
use crate::system::{FoundIn, MOST_FOUND, Search, System};

/// The window's find bar: open or not, with its replace row or not, the
/// query and its options, the replacement. The next file's bar opens with
/// them (rule 6).
#[derive(Default)]
pub struct Finder {
    pub open: bool,
    pub replacing: bool,
    pub search: Search,
    pub replacement: String,
    /// Where the caret was when the bar opened: typing finds from there.
    origin: Pos,
    /// A field to focus at the next frame, its text selected.
    focus: Option<Field>,
    /// Just opened: the first frame finds from the origin (rule 2).
    fresh: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Query,
    Replacement,
}

/// A file's matches for a search, for the text's version.
pub struct Matches {
    version: u64,
    search: Search,
    pub list: Vec<(Pos, Pos)>,
    pub error: Option<String>,
}

impl Finder {
    /// Opens the bar (`cmd f`; with `replacing`, `cmd alt f`): the query is
    /// the selection, or the word at the caret, and is selected (rule 1).
    pub fn show(&mut self, tab: Option<&Tab>, replacing: bool) {
        self.open = true;
        self.replacing = replacing;
        self.focus = Some(Field::Query);
        self.fresh = true;
        let Some(tab) = tab else {
            return;
        };
        let doc = &tab.doc;
        let (start, end) = doc.selection.range();
        self.origin = start;
        if start.line == end.line && start != end {
            self.search.query = doc.selected();
        } else if start == end {
            let line = doc.line(start.line);
            let (a, b) = super::view::word_at(line, start.col);
            let word: String = line.chars().skip(a).take(b - a).collect();
            if word.chars().any(|c| c.is_alphanumeric() || c == '_') {
                self.search.query = word;
                self.origin = Pos::new(start.line, a);
            }
        }
    }

    /// Opens the bar with `query`.
    pub fn show_with(&mut self, query: &str, replacing: bool) {
        self.open = true;
        self.replacing = replacing;
        self.search.query = query.to_owned();
        self.focus = Some(Field::Query);
        self.fresh = true;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.focus = None;
    }
}

/// `tab`'s matches for `search`, found again when the text or the search
/// changed (rule 5).
pub fn matches<'a>(tab: &'a mut Tab, search: &Search, system: &dyn System) -> &'a Matches {
    let version = tab.doc.version;
    let stale = tab
        .matches
        .as_ref()
        .is_none_or(|m| m.version != version || m.search != *search);
    if stale {
        let (list, error) = if search.query.is_empty() {
            (Vec::new(), None)
        } else {
            match system.find_in(&tab.doc.text(), search, None) {
                Ok(found) => (positions(&tab.doc, &found), None),
                Err(e) => (Vec::new(), Some(e)),
            }
        };
        tab.matches = Some(Matches {
            version,
            search: search.clone(),
            list,
            error,
        });
    }
    tab.matches.as_ref().expect("made")
}

/// Byte ranges of the text (lines joined by `\n`) as places, in order.
fn positions(doc: &Document, found: &[FoundIn]) -> Vec<(Pos, Pos)> {
    // Two walks on through the text: the starts, and the ends.
    struct Walk {
        line: usize,
        line_start: usize,
        byte: usize,
        col: usize,
    }
    impl Walk {
        fn to(&mut self, doc: &Document, b: usize) -> Pos {
            while self.line + 1 < doc.line_count()
                && b > self.line_start + doc.line(self.line).len()
            {
                self.line_start += doc.line(self.line).len() + 1;
                self.line += 1;
                self.byte = self.line_start;
                self.col = 0;
            }
            let text = doc.line(self.line);
            let to = (b - self.line_start).min(text.len());
            let from = self.byte - self.line_start;
            if to >= from {
                self.col += text[from..to].chars().count();
            } else {
                self.col = text[..to].chars().count();
            }
            self.byte = self.line_start + to;
            Pos::new(self.line, self.col)
        }
    }
    let walk = || Walk {
        line: 0,
        line_start: 0,
        byte: 0,
        col: 0,
    };
    let (mut starts, mut ends) = (walk(), walk());
    found
        .iter()
        .map(|f| (starts.to(doc, f.start), ends.to(doc, f.end)))
        .collect()
}

/// Which match the selection is, if one.
pub fn current(doc: &Document, list: &[(Pos, Pos)]) -> Option<usize> {
    let (start, end) = doc.selection.range();
    let i = list.partition_point(|m| m.0 < start);
    list.get(i).filter(|m| m.1 == end).map(|_| i)
}

fn select(tab: &mut Tab, m: (Pos, Pos)) {
    tab.doc.selection = Selection {
        anchor: m.0,
        head: m.1,
    };
    tab.doc.goal_col = None;
    tab.reveal = true;
}

/// The next match after the selection, or the one before it; round at
/// the file's ends (rule 3).
pub fn step(tab: &mut Tab, finder: &Finder, system: &dyn System, forward: bool) {
    let list = matches(tab, &finder.search, system).list.clone();
    if list.is_empty() {
        return;
    }
    let (start, _) = tab.doc.selection.range();
    let i = if forward {
        let i = list.partition_point(|m| m.0 <= start);
        if i == list.len() { 0 } else { i }
    } else {
        let i = list.partition_point(|m| m.0 < start);
        if i == 0 { list.len() - 1 } else { i - 1 }
    };
    select(tab, list[i]);
}

/// The first match at the bar's origin or after it (rule 2).
fn find_from_origin(tab: &mut Tab, finder: &Finder, system: &dyn System) {
    let list = &matches(tab, &finder.search, system).list;
    let i = list.partition_point(|m| m.0 < finder.origin);
    if let Some(&m) = list.get(i).or(list.first()) {
        select(tab, m);
    }
}

/// Replaces the current match and goes to the next.
fn replace_one(tab: &mut Tab, finder: &Finder, system: &dyn System) {
    let list = matches(tab, &finder.search, system).list.clone();
    let Some(i) = current(&tab.doc, &list) else {
        step(tab, finder, system, true);
        return;
    };
    let Ok(found) = system.find_in(&tab.doc.text(), &finder.search, Some(&finder.replacement))
    else {
        return;
    };
    let Some(with) = found.get(i).and_then(|f| f.replacement.clone()) else {
        return;
    };
    let (start, end) = list[i];
    tab.doc.edit(start, end, &with, super::EditKind::Other);
    let caret = tab.doc.selection.head;
    let list = matches(tab, &finder.search, system).list.clone();
    let next = list.partition_point(|m| m.0 < caret);
    if let Some(&m) = list.get(next).or(list.first()) {
        select(tab, m);
    }
}

/// Replaces every match, as one step to undo (rule 4).
fn replace_all(tab: &mut Tab, finder: &Finder, system: &dyn System) {
    let Ok(found) = system.find_in(&tab.doc.text(), &finder.search, Some(&finder.replacement))
    else {
        return;
    };
    let places = positions(&tab.doc, &found);
    let edits = places
        .into_iter()
        .zip(found)
        .map(|((start, end), f)| (start, end, f.replacement.unwrap_or_default()))
        .collect();
    tab.doc.edits(edits);
}

const W: f32 = 470.0;
const ROW: f32 = 26.0;

/// The bar over `rect` (the code's): its fields, options, count and
/// buttons. Returns whether it closed (the editor takes the focus back).
pub fn bar(
    ui: &mut Ui,
    rect: Rect,
    finder: &mut Finder,
    tab: &mut Tab,
    system: &dyn System,
    p: &Palette,
) -> bool {
    let id = Id::unique("wowlua-studio-find");
    let query_id = id.with("query");
    let replace_id = id.with("replace");
    let mut closed = false;
    let mut next = None;
    let mut replace = None;
    let mut changed = std::mem::take(&mut finder.fresh);

    // The keys of the fields, before the fields see them.
    let in_query = ui.memory(|m| m.has_focus(query_id));
    let in_replace = ui.memory(|m| m.has_focus(replace_id));
    if in_query || in_replace {
        ui.input_mut(|i| {
            let shift = Modifiers::SHIFT;
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                closed = true;
            }
            if i.consume_key(Modifiers::COMMAND, Key::Enter) && in_replace {
                replace = Some(true);
            }
            if i.consume_key(shift, Key::Enter) {
                next = Some(false);
            }
            if i.consume_key(Modifiers::NONE, Key::Enter) {
                if in_replace {
                    replace = Some(false);
                } else {
                    next = Some(true);
                }
            }
            if i.consume_key(Modifiers::NONE, Key::Tab) || i.consume_key(shift, Key::Tab) {
                finder.focus = Some(if in_query && finder.replacing {
                    Field::Replacement
                } else {
                    Field::Query
                });
            }
            // alt c, alt w, alt r: the options; their text is not typed.
            let mut toggled = false;
            for (key, on) in [
                (Key::C, &mut finder.search.case),
                (Key::W, &mut finder.search.word),
                (Key::R, &mut finder.search.regex),
            ] {
                if i.consume_key(Modifiers::ALT, key) {
                    *on = !*on;
                    toggled = true;
                }
            }
            if toggled {
                i.events.retain(|e| !matches!(e, Event::Text(_)));
                changed = true;
            }
        });
    }

    matches(tab, &finder.search, system);
    let found = tab.matches.as_ref().expect("found");
    let error = found.error.clone();
    let total = found.list.len();
    let now = current(&tab.doc, &found.list);
    let rows = if finder.replacing { 2.0 } else { 1.0 };
    let message = error.is_some() as u8 as f32 * 18.0;
    let width = W.min(rect.width() - 24.0).max(200.0);
    let size = Vec2::new(width, 8.0 + rows * ROW + (rows - 1.0) * 4.0 + message);
    let bar = Rect::from_min_size(
        Pos2::new(rect.right() - 18.0 - width, rect.top() + 6.0),
        size,
    );
    // The bar takes the pointer from the code under it.
    ui.interact(bar, id.with("ground"), Sense::click());
    let painter = ui.painter().clone();
    painter.rect_filled(
        bar.expand(2.0),
        CornerRadius::ZERO,
        Color32::from_black_alpha(40),
    );
    painter.rect_filled(bar, CornerRadius::ZERO, p.bg_200);
    painter.rect_stroke(
        bar,
        CornerRadius::ZERO,
        Stroke::new(1.0, p.line_strong),
        StrokeKind::Inside,
    );

    let mut x = bar.left() + 4.0;
    let top = bar.top() + 4.0;
    let row1 = |x: f32, w: f32| Rect::from_min_size(Pos2::new(x, top), Vec2::new(w, ROW));
    // The replace row's toggle.
    if glyph_button(
        ui,
        row1(x, 24.0),
        id.with("toggle"),
        if finder.replacing {
            Glyph::Open
        } else {
            Glyph::Right
        },
        true,
        p,
    )
    .on_hover_text(if finder.replacing {
        "hide replace"
    } else {
        "show replace"
    })
    .clicked()
    {
        finder.replacing = !finder.replacing;
    }
    x += 28.0;
    let right_w = 84.0 + 3.0 * 26.0;
    let field = row1(x, bar.right() - 4.0 - right_w - x - 4.0);
    let has_error = error.is_some() || (total == 0 && !finder.search.query.is_empty());
    let q = text_field(
        ui,
        field,
        query_id,
        &mut finder.search.query,
        "find",
        3,
        has_error,
        finder.focus == Some(Field::Query),
        p,
    );
    if q.changed() {
        changed = true;
    }
    // The options inside the field.
    let mut ox = field.right() - 3.0 * 24.0 - 2.0;
    for (label, on, tip) in [
        ("Aa", &mut finder.search.case, "case (alt c)"),
        ("ab", &mut finder.search.word, "whole word (alt w)"),
        (".*", &mut finder.search.regex, "regular expression (alt r)"),
    ] {
        let r = Rect::from_min_size(Pos2::new(ox, field.top() + 3.0), Vec2::new(22.0, ROW - 6.0));
        if option(ui, r, id.with(label), label, *on, p)
            .on_hover_text(tip)
            .clicked()
        {
            *on = !*on;
            changed = true;
        }
        ox += 24.0;
    }
    x = field.right() + 4.0;
    // The count.
    let (count, color) = if error.is_some() {
        (String::new(), p.error)
    } else if finder.search.query.is_empty() {
        (String::new(), p.ink_muted)
    } else if total == 0 {
        ("no matches".to_owned(), p.error)
    } else {
        let all = if total >= MOST_FOUND {
            format!("{MOST_FOUND}+")
        } else {
            total.to_string()
        };
        match now {
            Some(i) => (format!("{} of {all}", i + 1), p.ink_muted),
            None => (format!("{all} found"), p.ink_muted),
        }
    };
    painter.text(
        row1(x, 84.0).center(),
        Align2::CENTER_CENTER,
        count,
        Face::Plex.at(11.5),
        color,
    );
    x += 84.0;
    let can = total > 0;
    if glyph_button(ui, row1(x, 24.0), id.with("previous"), Glyph::Up, can, p)
        .on_hover_text("previous match")
        .clicked()
    {
        next = Some(false);
    }
    x += 26.0;
    if glyph_button(ui, row1(x, 24.0), id.with("next"), Glyph::Down, can, p)
        .on_hover_text("next match")
        .clicked()
    {
        next = Some(true);
    }
    x += 26.0;
    if glyph_button(ui, row1(x, 24.0), id.with("close"), Glyph::Close, true, p)
        .on_hover_text("close (esc)")
        .clicked()
    {
        closed = true;
    }
    let mut y = top + ROW;
    if let Some(e) = &error {
        painter.text(
            Pos2::new(field.left() + 4.0, y + 9.0),
            Align2::LEFT_CENTER,
            e,
            Face::Plex.at(11.5),
            p.error,
        );
        y += message;
    }
    if finder.replacing {
        y += 4.0;
        let row2 = |x: f32, w: f32| Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, ROW));
        let all_w = 64.0;
        let one_w = 70.0;
        let rfield = row2(
            bar.left() + 32.0,
            bar.right() - 4.0 - all_w - 4.0 - one_w - 4.0 - (bar.left() + 32.0),
        );
        text_field(
            ui,
            rfield,
            replace_id,
            &mut finder.replacement,
            "replace",
            0,
            false,
            finder.focus == Some(Field::Replacement),
            p,
        );
        let mut bx = rfield.right() + 4.0;
        if text_button(ui, row2(bx, one_w), id.with("one"), "replace", can, p)
            .on_hover_text("enter")
            .clicked()
        {
            replace = Some(false);
        }
        bx += one_w + 4.0;
        let label = format!(
            "all {}",
            if total >= MOST_FOUND {
                format!("{MOST_FOUND}+")
            } else {
                total.to_string()
            }
        );
        if text_button(ui, row2(bx, all_w), id.with("all"), &label, can, p)
            .on_hover_text("cmd enter")
            .clicked()
        {
            replace = Some(true);
        }
    }
    finder.focus = None;

    if changed {
        find_from_origin(tab, finder, system);
    }
    if let Some(forward) = next {
        step(tab, finder, system, forward);
    }
    match replace {
        Some(true) => replace_all(tab, finder, system),
        Some(false) => replace_one(tab, finder, system),
        None => {}
    }
    if closed {
        finder.close();
    }
    closed
}

/// A field of the bar: its box (amber when focused, red with no
/// matches), the text, room on the right for `options`.
#[allow(clippy::too_many_arguments)]
fn text_field(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    text: &mut String,
    hint: &str,
    options: usize,
    error: bool,
    focus: bool,
    p: &Palette,
) -> egui::Response {
    let font = Face::Plex.at(12.5);
    let row = ui.fonts_mut(|f| f.row_height(&font));
    let inner = Rect::from_min_max(
        Pos2::new(rect.left() + 8.0, rect.center().y - row / 2.0),
        Pos2::new(
            rect.right() - 4.0 - options as f32 * 24.0,
            rect.center().y + row / 2.0,
        ),
    );
    if focus {
        // Focused with its text selected, to type over.
        let mut state = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
        let all = text.chars().count();
        state
            .cursor
            .set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(all))));
        state.store(ui.ctx(), id);
        ui.memory_mut(|m| m.request_focus(id));
    }
    let ground = ui.painter().add(egui::Shape::Noop);
    let mut child = ui.new_child(UiBuilder::new().max_rect(inner));
    let response = TextEdit::singleline(text)
        .id(id)
        .frame(Frame::NONE)
        .margin(Margin::ZERO)
        .font(font.clone())
        .text_color(p.ink)
        .hint_text(egui::RichText::new(hint).font(font).color(p.ink_faint))
        .desired_width(inner.width())
        .show(&mut child)
        .response
        .response;
    let focused = response.has_focus();
    let border = if error {
        p.error
    } else if focused {
        p.amber_ink
    } else {
        p.line_strong
    };
    ui.painter().set(
        ground,
        egui::Shape::rect_filled(rect, CornerRadius::ZERO, p.bg_000),
    );
    ui.painter().rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, border),
        StrokeKind::Inside,
    );
    response
}

/// An option in a field: on in amber.
fn option(ui: &mut Ui, r: Rect, id: Id, label: &str, on: bool, p: &Palette) -> egui::Response {
    let resp = ui.interact(r, id, Sense::click());
    let (bg, fg) = if on {
        (p.amber_soft, p.ink)
    } else if resp.hovered() {
        (p.bg_300, p.ink)
    } else {
        (Color32::TRANSPARENT, p.ink_muted)
    };
    ui.painter().rect_filled(r, CornerRadius::ZERO, bg);
    if on {
        ui.painter().rect_stroke(
            r,
            CornerRadius::ZERO,
            Stroke::new(1.0, p.amber_ink),
            StrokeKind::Inside,
        );
    }
    let g = ui.painter().text(
        r.center(),
        Align2::CENTER_CENTER,
        label,
        Face::Plex.at(11.0),
        fg,
    );
    if label == "ab" {
        ui.painter()
            .hline(g.x_range(), g.bottom() - 1.0, Stroke::new(1.0, fg));
    }
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, label));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A small text button of the replace row.
fn text_button(
    ui: &mut Ui,
    r: Rect,
    id: Id,
    label: &str,
    enabled: bool,
    p: &Palette,
) -> egui::Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let resp = ui.interact(r, id, sense);
    let bg = if enabled && resp.hovered() {
        p.bg_300
    } else {
        p.bg_100
    };
    ui.painter().rect_filled(r, CornerRadius::ZERO, bg);
    ui.painter().rect_stroke(
        r,
        CornerRadius::ZERO,
        Stroke::new(1.0, p.line_strong),
        StrokeKind::Inside,
    );
    let fg = if enabled { p.ink } else { p.ink_faint };
    ui.painter().text(
        r.center(),
        Align2::CENTER_CENTER,
        label,
        Face::Plex.at(11.5),
        fg,
    );
    resp
}

/// The bar's glyphs: the replace row's chevron (closed, open), the
/// previous and next arrows, close.
#[derive(Clone, Copy)]
enum Glyph {
    Right,
    Open,
    Up,
    Down,
    Close,
}

/// A 24 px button with a stroked glyph (the design's icons).
fn glyph_button(
    ui: &mut Ui,
    r: Rect,
    id: Id,
    glyph: Glyph,
    enabled: bool,
    p: &Palette,
) -> egui::Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let resp = ui.interact(r, id, sense);
    let fg = if !enabled {
        p.ink_faint
    } else if resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::ZERO, p.bg_300);
        p.ink
    } else {
        p.ink_muted
    };
    let c = r.center();
    let s = Stroke::new(1.25, fg);
    let pt = |x: f32, y: f32| c + Vec2::new(x, y);
    let painter = ui.painter();
    match glyph {
        Glyph::Right => {
            painter.line(vec![pt(-2.0, -4.0), pt(2.0, 0.0), pt(-2.0, 4.0)], s);
        }
        Glyph::Open => {
            painter.line(vec![pt(-4.0, -2.0), pt(0.0, 2.0), pt(4.0, -2.0)], s);
        }
        Glyph::Down => {
            painter.line_segment([pt(0.0, -4.5), pt(0.0, 4.5)], s);
            painter.line(vec![pt(-4.0, 0.5), pt(0.0, 4.5), pt(4.0, 0.5)], s);
        }
        Glyph::Up => {
            painter.line_segment([pt(0.0, 4.5), pt(0.0, -4.5)], s);
            painter.line(vec![pt(-4.0, -0.5), pt(0.0, -4.5), pt(4.0, -0.5)], s);
        }
        Glyph::Close => {
            painter.line_segment([pt(-4.0, -4.0), pt(4.0, 4.0)], s);
            painter.line_segment([pt(4.0, -4.0), pt(-4.0, 4.0)], s);
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The matches on the scrollbar's track: a tick per line that has one.
pub fn ticks(ui: &Ui, rect: Rect, lines: usize, list: &[(Pos, Pos)], p: &Palette) {
    if list.is_empty() || lines == 0 {
        return;
    }
    let track = Rect::from_min_max(Pos2::new(rect.right() - 8.0, rect.top()), rect.max);
    let mut last = f32::MIN;
    for m in list {
        let y = track.top() + track.height() * m.0.line as f32 / lines as f32;
        if y - last < 2.0 {
            continue;
        }
        last = y;
        ui.painter().rect_filled(
            Rect::from_min_size(Pos2::new(track.left() + 1.0, y), Vec2::new(6.0, 2.0)),
            CornerRadius::ZERO,
            p.amber_ink,
        );
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::system::NoSystem;

    fn tab(text: &str) -> Tab {
        Tab::new(Document::new(PathBuf::from("a.wowlua"), text))
    }

    fn search(query: &str) -> Search {
        Search {
            query: query.into(),
            ..Search::default()
        }
    }

    #[test]
    fn opening_takes_the_word_at_the_caret_and_finds_from_it() {
        let mut t = tab("local frame = f\nframe:Show()\nprint(frame)\n");
        t.doc.selection = Selection::caret(Pos::new(1, 2));
        let mut finder = Finder::default();
        finder.show(Some(&t), false);
        assert_eq!(finder.search.query, "frame");
        find_from_origin(&mut t, &finder, &NoSystem);
        assert_eq!(t.doc.selection.range(), (Pos::new(1, 0), Pos::new(1, 5)));
        let list = matches(&mut t, &finder.search, &NoSystem).list.clone();
        assert_eq!(list.len(), 3);
        assert_eq!(current(&t.doc, &list), Some(1));
    }

    #[test]
    fn next_and_previous_go_round_at_the_ends() {
        let mut t = tab("a x\nb x\nc x");
        let finder = Finder {
            search: search("x"),
            ..Finder::default()
        };
        t.doc.selection = Selection::caret(Pos::new(2, 3));
        step(&mut t, &finder, &NoSystem, true);
        assert_eq!(t.doc.selection.range().0, Pos::new(0, 2));
        step(&mut t, &finder, &NoSystem, false);
        assert_eq!(t.doc.selection.range().0, Pos::new(2, 2));
    }

    #[test]
    fn replace_one_goes_on_and_replace_all_is_one_step() {
        let mut t = tab("ab ab\nab");
        let mut finder = Finder {
            search: search("ab"),
            replacement: "xyz".into(),
            ..Finder::default()
        };
        find_from_origin(&mut t, &finder, &NoSystem);
        replace_one(&mut t, &finder, &NoSystem);
        assert_eq!(t.doc.text(), "xyz ab\nab");
        assert_eq!(t.doc.selection.range(), (Pos::new(0, 4), Pos::new(0, 6)));
        finder.replacement = "c".into();
        replace_all(&mut t, &finder, &NoSystem);
        assert_eq!(t.doc.text(), "xyz c\nc");
        t.doc.undo();
        assert_eq!(t.doc.text(), "xyz ab\nab");
    }

    #[test]
    fn matches_are_placed_in_characters_on_their_lines() {
        let doc = Document::new(PathBuf::from("a"), "ä x\n\nx äx");
        let found: Vec<FoundIn> = crate::system::find_literal(&doc.text(), &search("x"))
            .into_iter()
            .map(|(start, end)| FoundIn {
                start,
                end,
                replacement: None,
            })
            .collect();
        assert_eq!(
            positions(&doc, &found),
            [
                (Pos::new(0, 2), Pos::new(0, 3)),
                (Pos::new(2, 0), Pos::new(2, 1)),
                (Pos::new(2, 3), Pos::new(2, 4)),
            ]
        );
    }

    #[test]
    fn literal_matches_follow_case_and_whole_words() {
        let text = "Frame frame frames";
        let found = |case, word| {
            crate::system::find_literal(
                text,
                &Search {
                    query: "frame".into(),
                    case,
                    word,
                    regex: false,
                },
            )
        };
        assert_eq!(found(false, false), [(0, 5), (6, 11), (12, 17)]);
        assert_eq!(found(true, false), [(6, 11), (12, 17)]);
        assert_eq!(found(false, true), [(0, 5), (6, 11)]);
    }
}
