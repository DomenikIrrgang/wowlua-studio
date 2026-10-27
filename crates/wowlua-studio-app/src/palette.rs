//! The command palette (the design's command palette board): one field
//! whose amber prompt holds the mode: `/` files, `>` commands, `#`
//! symbols, `@` this file's symbols, `:` a line. Typing a mode's sign in
//! an empty field switches to it; backspace in an empty field goes back to
//! files. Files and symbols come with the workspace (M2) and the language
//! server (M3).

use egui::{
    Align2, CornerRadius, Id, Key, Order, Pos2, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui,
    UiBuilder, Vec2,
    text::{LayoutJob, TextFormat},
};
use wowlua_studio_ui::{Face, palette, widgets};

use crate::{
    Platform,
    commands::{Command, Entry, key_words},
};

/// What the palette searches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Files,
    Commands,
    Symbols,
    FileSymbols,
    Line,
}

impl Mode {
    const ALL: [Mode; 5] = [
        Mode::Files,
        Mode::Commands,
        Mode::Symbols,
        Mode::FileSymbols,
        Mode::Line,
    ];

    /// The prompt's sign.
    fn sign(self) -> char {
        match self {
            Mode::Files => '/',
            Mode::Commands => '>',
            Mode::Symbols => '#',
            Mode::FileSymbols => '@',
            Mode::Line => ':',
        }
    }

    fn label(self) -> &'static str {
        match self {
            Mode::Files => "files",
            Mode::Commands => "> commands",
            Mode::Symbols => "# symbols",
            Mode::FileSymbols => "@ in this file",
            Mode::Line => ": line",
        }
    }

    /// The mode a typed sign switches to (`/` is the prompt, not a sign).
    fn from_sign(c: char) -> Option<Mode> {
        Mode::ALL
            .into_iter()
            .find(|m| *m != Mode::Files && m.sign() == c)
    }
}

#[derive(Default)]
pub struct PaletteState {
    pub open: bool,
    pub mode: Mode,
    /// The typed text, without the mode's sign.
    pub query: String,
    pub selected: usize,
    focus: bool,
}

impl PaletteState {
    pub fn open(&mut self, mode: Mode) {
        self.open = true;
        self.mode = mode;
        self.query.clear();
        self.selected = 0;
        self.focus = true;
    }

    /// Applies an edit of the field: a sign typed into an empty field
    /// switches the mode; backspace in an empty field goes back to files.
    fn edited(&mut self, was_empty: bool, backspace: bool) {
        if was_empty && backspace && self.query.is_empty() {
            self.mode = Mode::Files;
        }
        if was_empty && self.mode == Mode::Files {
            let mut chars = self.query.chars();
            if let Some(mode) = chars.next().and_then(Mode::from_sign) {
                self.mode = mode;
                self.query = chars.as_str().to_owned();
            }
        }
    }
}

/// Draws the palette over the window; returns the command picked.
pub fn show(
    ui: &mut Ui,
    state: &mut PaletteState,
    entries: &[Entry],
    platform: Platform,
) -> Option<Command> {
    if !state.open {
        return None;
    }
    let ctx = ui.ctx().clone();
    let p = palette(&ctx);
    let screen = ctx.content_rect();
    let mut picked = None;

    let width = (screen.width() - 32.0).min(640.0);
    let dialog_min = Pos2::new(screen.center().x - width / 2.0, screen.top() + 72.0);
    let area = egui::Area::new(Id::unique("wowlua-studio-palette-dialog"))
        .order(Order::Foreground)
        .fixed_pos(dialog_min)
        .show(&ctx, |ui| {
            ui.set_width(width);
            // The scrim; a click on it closes the palette (below).
            ui.painter()
                .with_clip_rect(screen)
                .rect_filled(screen, CornerRadius::ZERO, p.overlay);
            let top = ui.max_rect().min;
            let painter = ui.painter().clone();
            // The dialog's ground goes under everything drawn after it.
            let ground = painter.add(egui::Shape::Noop);

            // The field: 52 px, the amber prompt, the query, esc.
            let (field, _) = ui.allocate_exact_size(Vec2::new(width, 52.0), Sense::hover());
            painter.text(
                Pos2::new(field.left() + 16.0, field.center().y),
                Align2::LEFT_CENTER,
                state.mode.sign(),
                Face::PlexSemiBold.at(15.0),
                p.amber_ink,
            );
            let input_rect = Rect::from_min_max(
                Pos2::new(field.left() + 36.0, field.top() + 14.0),
                Pos2::new(field.right() - 56.0, field.bottom() - 14.0),
            );
            let was_empty = state.query.is_empty();
            let backspace = ui.input(|i| i.key_pressed(Key::Backspace));
            let mut input = ui.new_child(UiBuilder::new().max_rect(input_rect));
            let edit = input.add(
                TextEdit::singleline(&mut state.query)
                    .frame(egui::Frame::NONE)
                    .font(Face::Plex.at(15.0))
                    .text_color(p.ink)
                    .hint_text("file name, > command, # symbol, @ in this file, : line")
                    .desired_width(input_rect.width()),
            );
            if state.focus {
                edit.request_focus();
                state.focus = false;
            }
            if edit.changed() || backspace {
                state.edited(was_empty, backspace);
                state.selected = 0;
            }
            let mut esc_key = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_max(
                Pos2::new(field.right() - 44.0, field.center().y - 8.0),
                Pos2::new(field.right() - 12.0, field.center().y + 8.0),
            )));
            widgets::kbd(&mut esc_key, "esc");
            painter.hline(
                field.x_range(),
                field.bottom() - 1.0,
                Stroke::new(2.0, p.amber),
            );

            // The modes.
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                for mode in Mode::ALL {
                    let on = mode == state.mode;
                    let color = if on { p.amber_ink } else { p.ink_muted };
                    let border = if on { p.amber_ink } else { p.line };
                    let g = widgets::galley(ui, mode.label(), Face::Plex.at(11.5), color, 0.0);
                    let (r, resp) =
                        ui.allocate_exact_size(g.size() + Vec2::new(16.0, 8.0), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(r, CornerRadius::ZERO, p.bg_300);
                    }
                    ui.painter().rect_stroke(
                        r,
                        CornerRadius::ZERO,
                        Stroke::new(1.0, border),
                        StrokeKind::Inside,
                    );
                    ui.painter().galley(r.min + Vec2::new(8.0, 4.0), g, color);
                    if resp.clicked() {
                        state.mode = mode;
                        state.selected = 0;
                        state.focus = true;
                    }
                }
            });
            ui.add_space(8.0);

            // The results.
            let needle = state.query.trim().to_lowercase();
            let matches = if state.mode == Mode::Commands {
                command_matches(entries, &needle)
            } else {
                Vec::new()
            };
            if state.selected >= matches.len() {
                state.selected = matches.len().saturating_sub(1);
            }
            match state.mode {
                Mode::Commands if matches.is_empty() => {
                    empty(
                        ui,
                        &format!("No commands match \"{}\".", state.query.trim()),
                    );
                }
                Mode::Commands => {
                    group_title(ui, "commands");
                    let rows = matches.len().min(12);
                    let first = state.selected.saturating_sub(rows - 1);
                    for (i, (entry, hits)) in matches.iter().enumerate().skip(first).take(rows) {
                        if result_row(ui, entry, hits, i == state.selected, platform, width) {
                            picked = Some(entry.command);
                        }
                    }
                }
                Mode::Line => empty(ui, "Go to a line: comes with the editor (M3)."),
                Mode::Files | Mode::Symbols | Mode::FileSymbols => empty(
                    ui,
                    "No folder open: files come with M2, symbols with M3. Type > for commands.",
                ),
            }

            // The footer: key hints and the count.
            ui.add_space(4.0);
            let (foot, _) = ui.allocate_exact_size(Vec2::new(width, 32.0), Sense::hover());
            painter.hline(foot.x_range(), foot.top() + 0.5, Stroke::new(1.0, p.line));
            painter.text(
                Pos2::new(foot.left() + 12.0, foot.center().y),
                Align2::LEFT_CENTER,
                "↑ ↓ move   enter open   esc close",
                Face::Plex.at(11.5),
                p.ink_muted,
            );
            if state.mode == Mode::Commands {
                let n = matches.len();
                painter.text(
                    Pos2::new(foot.right() - 12.0, foot.center().y),
                    Align2::RIGHT_CENTER,
                    format!("{n} result{}", if n == 1 { "" } else { "s" }),
                    Face::Plex.at(11.5),
                    p.ink_muted,
                );
            }
            let whole = Rect::from_min_max(top, Pos2::new(top.x + width, foot.bottom()));
            painter.set(
                ground,
                egui::Shape::rect_filled(whole, CornerRadius::ZERO, p.bg_200),
            );
            painter.rect_stroke(
                whole,
                CornerRadius::ZERO,
                Stroke::new(1.0, p.line_strong),
                StrokeKind::Outside,
            );

            // Keys.
            let (up, down, enter, esc) = ui.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                    i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                    i.consume_key(egui::Modifiers::NONE, Key::Enter),
                    i.consume_key(egui::Modifiers::NONE, Key::Escape),
                )
            });
            if up {
                state.selected = state.selected.saturating_sub(1);
            }
            if down && state.selected + 1 < matches.len() {
                state.selected += 1;
            }
            if enter && let Some((entry, _)) = matches.get(state.selected) {
                picked = Some(entry.command);
            }
            if esc {
                state.open = false;
            }
            whole
        });
    // A click outside the dialog closes it.
    let outside = ctx.input(|i| {
        i.pointer.any_pressed()
            && i.pointer
                .interact_pos()
                .is_some_and(|pos| !area.inner.contains(pos))
    });
    if outside || picked.is_some() {
        state.open = false;
    }
    picked
}

/// The commands matching `needle`, best first; all of them, in the
/// registry's order, for an empty needle.
fn command_matches<'a>(entries: &'a [Entry], needle: &str) -> Vec<(&'a Entry, Vec<usize>)> {
    let mut found: Vec<_> = entries
        .iter()
        .filter_map(|e| fuzzy(&e.title, needle).map(|hits| (e, hits)))
        .collect();
    if !needle.is_empty() {
        // Earlier and tighter matches first.
        found.sort_by_key(|(e, hits)| {
            let first = hits.first().copied().unwrap_or(0);
            let spread = hits.last().copied().unwrap_or(0) - first;
            (spread, first, e.title.len())
        });
    }
    found
}

fn group_title(ui: &mut Ui, title: &str) {
    let p = palette(ui.ctx());
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new(format!("# {title}"))
                .font(Face::Plex.at(11.5))
                .color(p.ink_muted),
        );
    });
}

fn empty(ui: &mut Ui, text: &str) {
    let p = palette(ui.ctx());
    ui.horizontal_wrapped(|ui| {
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new(text)
                .font(Face::Plex.at(12.5))
                .color(p.ink_muted),
        );
    });
    ui.add_space(12.0);
}

/// One result: kind, title with its matched letters, detail, keys.
fn result_row(
    ui: &mut Ui,
    entry: &Entry,
    hits: &[usize],
    selected: bool,
    platform: Platform,
    width: f32,
) -> bool {
    let p = palette(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 30.0), Sense::click());
    if selected || resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::ZERO, p.amber_soft);
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(2.0, rect.height())),
            CornerRadius::ZERO,
            p.amber,
        );
    }
    let y = rect.center().y;
    ui.painter().text(
        Pos2::new(rect.left() + 14.0, y),
        Align2::LEFT_CENTER,
        entry.kind,
        Face::Plex.at(11.0),
        p.ink_muted,
    );
    let mut job = LayoutJob::default();
    for (i, ch) in entry.title.chars().enumerate() {
        let hit = hits.contains(&i);
        job.append(
            ch.encode_utf8(&mut [0; 4]),
            0.0,
            TextFormat {
                font_id: if hit {
                    Face::PlexSemiBold.at(12.5)
                } else {
                    Face::Plex.at(12.5)
                },
                color: if hit { p.amber_ink } else { p.ink },
                ..Default::default()
            },
        );
    }
    let title = ui.fonts_mut(|f| f.layout_job(job));
    let title_x = rect.left() + 76.0;
    ui.painter().galley(
        Pos2::new(title_x, y - title.size().y / 2.0),
        title.clone(),
        p.ink,
    );
    ui.painter().text(
        Pos2::new(title_x + title.size().x + 12.0, y),
        Align2::LEFT_CENTER,
        &entry.detail,
        Face::Plex.at(11.5),
        p.ink_muted,
    );
    if let Some(key) = &entry.key {
        let mut x = rect.right() - 12.0;
        for word in key_words(key, platform).iter().rev() {
            let g = widgets::galley(ui, word, Face::Plex.at(10.5), p.ink, 0.0);
            let r = Rect::from_min_size(
                Pos2::new(x - g.size().x - 8.0, y - 8.0),
                Vec2::new(g.size().x + 8.0, 16.0),
            );
            ui.painter().rect_filled(r, CornerRadius::same(2), p.bg_200);
            ui.painter().rect_stroke(
                r,
                CornerRadius::same(2),
                Stroke::new(1.0, p.line_strong),
                StrokeKind::Inside,
            );
            ui.painter().galley(r.center() - g.size() / 2.0, g, p.ink);
            x = r.left() - 4.0;
        }
    }
    resp.clicked()
}

/// The positions of `needle`'s letters in `text`, in order, or `None`.
fn fuzzy(text: &str, needle: &str) -> Option<Vec<usize>> {
    let mut hits = Vec::with_capacity(needle.len());
    let mut wanted = needle.chars().filter(|c| !c.is_whitespace()).peekable();
    for (i, ch) in text.chars().enumerate() {
        let Some(&w) = wanted.peek() else { break };
        if ch.to_lowercase().eq(w.to_lowercase()) {
            hits.push(i);
            wanted.next();
        }
    }
    wanted.peek().is_none().then_some(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_in_order() {
        assert_eq!(fuzzy("go: debugger", "dbg"), Some(vec![4, 6, 8]));
        assert_eq!(fuzzy("theme: dark", "td"), Some(vec![0, 7]));
        assert_eq!(fuzzy("theme: dark", "z"), None);
        assert_eq!(fuzzy("anything", ""), Some(vec![]));
    }

    #[test]
    fn a_sign_in_an_empty_field_switches_the_mode() {
        let mut s = PaletteState {
            query: ">".into(),
            ..Default::default()
        };
        s.edited(true, false);
        assert_eq!((s.mode, s.query.as_str()), (Mode::Commands, ""));
        s.query = "deb".into();
        s.edited(false, false);
        assert_eq!((s.mode, s.query.as_str()), (Mode::Commands, "deb"));
        s.query.clear();
        s.edited(false, true);
        assert_eq!(
            s.mode,
            Mode::Commands,
            "deleting the last letter keeps the mode"
        );
        s.edited(true, true);
        assert_eq!(
            s.mode,
            Mode::Files,
            "backspace in an empty field goes back to files"
        );
    }

    #[test]
    fn a_sign_later_in_the_text_is_text() {
        let mut s = PaletteState {
            query: "a>b".into(),
            ..Default::default()
        };
        s.edited(false, false);
        assert_eq!((s.mode, s.query.as_str()), (Mode::Files, "a>b"));
    }
}
