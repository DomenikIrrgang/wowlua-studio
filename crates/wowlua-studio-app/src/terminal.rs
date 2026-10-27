//! The terminal panel (S043): the user's shell in the workspace's folder,
//! as the host's terminal emulator keeps its screen. Keys go to the shell
//! as a terminal sends them; the wheel scrolls back.

use std::path::Path;

use egui::{
    Align2, Color32, CornerRadius, Event, EventFilter, Id, Key, Modifiers, Pos2, Rect, Sense,
    Stroke, Ui, Vec2,
    text::{LayoutJob, TextFormat},
};
use wowlua_studio_ui::{Face, Palette, Theme, forms, palette, theme};

use crate::system::{System, TermCell, TermColor};

const LINE_H: f32 = 20.0;
const PAD_X: f32 = 14.0;
const PAD_Y: f32 = 10.0;

#[derive(Default)]
pub struct TerminalState {
    id: Option<u64>,
    size: (u16, u16),
    error: Option<String>,
    /// The wheel's fraction of a line not scrolled yet.
    wheel: f32,
}

impl TerminalState {
    /// Ends the shell (the workspace closed).
    pub fn close(&mut self, system: &dyn System) {
        if let Some(id) = self.id.take() {
            system.terminal_close(id);
        }
        *self = TerminalState::default();
    }
}

pub fn show(ui: &mut Ui, state: &mut TerminalState, system: &dyn System, cwd: &Path, mac: bool) {
    let p = palette(ui.ctx());
    let ctx = ui.ctx().clone();
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_000);
    if let Some(error) = &state.error {
        forms::padded(ui, 14, 12, |ui| {
            forms::paragraph(ui, error, 12.5, 19.0, p.ink_muted)
        });
        return;
    }
    let font = Face::Plex.at(12.5);
    let char_w = ui.fonts_mut(|f| f.glyph_width(&font, 'M'));
    let cols = (((rect.width() - 2.0 * PAD_X) / char_w).floor().max(10.0)) as u16;
    let rows = (((rect.height() - 2.0 * PAD_Y) / LINE_H).floor().max(2.0)) as u16;
    let id = match state.id {
        Some(id) => {
            if state.size != (cols, rows) {
                system.terminal_resize(id, cols, rows);
                state.size = (cols, rows);
            }
            id
        }
        None => match system.terminal_open(cwd, cols, rows) {
            Ok(id) => {
                state.id = Some(id);
                state.size = (cols, rows);
                id
            }
            Err(error) => {
                state.error = Some(error);
                return;
            }
        },
    };
    let Some(screen) = system.terminal_screen(id) else {
        return;
    };

    // Focus: a click; the keys a terminal needs stay in it.
    let focus_id = Id::unique("wowlua-studio-terminal");
    let response = ui.interact(rect, focus_id, Sense::click());
    if response.clicked() {
        response.request_focus();
    }
    let focused = response.has_focus();
    if focused {
        ctx.memory_mut(|m| {
            m.set_focus_lock_filter(
                focus_id,
                EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            )
        });
        let events = ctx.input(|i| i.events.clone());
        for event in events {
            let bytes: Option<Vec<u8>> = match event {
                Event::Text(t) => Some(t.into_bytes()),
                Event::Paste(t) => Some(if screen.bracketed_paste {
                    format!("\x1b[200~{t}\x1b[201~").into_bytes()
                } else {
                    t.into_bytes()
                }),
                // Ctrl C and X are the shell's (interrupt, cut) where they
                // are not the system's copy and cut.
                Event::Copy if !mac => Some(vec![0x03]),
                Event::Cut if !mac => Some(vec![0x18]),
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => key_bytes(key, modifiers, screen.app_cursor, mac),
                _ => None,
            };
            if let Some(bytes) = bytes {
                system.terminal_input(id, &bytes);
            }
        }
    }
    if response.hovered() {
        let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            state.wheel += scroll / LINE_H;
            let lines = state.wheel.trunc() as i32;
            if lines != 0 {
                state.wheel -= lines as f32;
                system.terminal_scroll(id, lines);
            }
        }
    }

    // The screen.
    let dark = theme(&ctx) == Theme::Dark;
    let painter = ui.painter().with_clip_rect(rect);
    let origin = Pos2::new(rect.left() + PAD_X, rect.top() + PAD_Y);
    for (r, row) in screen.rows.iter().enumerate() {
        let y = origin.y + r as f32 * LINE_H;
        // Backgrounds first, in runs.
        let mut c = 0;
        while c < row.len() {
            let (_, bg) = colors(&row[c], p, dark);
            let start = c;
            while c < row.len() && colors(&row[c], p, dark).1 == bg {
                c += 1;
            }
            if let Some(bg) = bg {
                painter.rect_filled(
                    Rect::from_min_size(
                        Pos2::new(origin.x + start as f32 * char_w, y),
                        Vec2::new((c - start) as f32 * char_w, LINE_H),
                    ),
                    CornerRadius::ZERO,
                    bg,
                );
            }
        }
        let mut job = LayoutJob::default();
        for cell in row.iter().filter(|cell| !cell.spacer) {
            let (fg, _) = colors(cell, p, dark);
            let face = match (cell.bold, cell.italic) {
                (true, _) => Face::PlexSemiBold,
                (false, true) => Face::PlexItalic,
                _ => Face::Plex,
            };
            let mut format = TextFormat::simple(face.at(12.5), fg);
            if cell.underline {
                format.underline = Stroke::new(1.0, fg);
            }
            let mut buf = [0u8; 4];
            job.append(cell.c.encode_utf8(&mut buf), 0.0, format);
        }
        job.wrap.max_rows = 1;
        let g = ui.fonts_mut(|f| f.layout_job(job));
        painter.galley(
            Pos2::new(origin.x, y + (LINE_H - g.size().y) / 2.0),
            g,
            p.ink,
        );
    }
    // The cursor: a block of amber, an outline when the panel is not
    // focused.
    if let Some((r, c)) = screen.cursor
        && !screen.exited
    {
        let cell = Rect::from_min_size(
            Pos2::new(
                origin.x + c as f32 * char_w,
                origin.y + r as f32 * LINE_H + 1.0,
            ),
            Vec2::new(char_w, LINE_H - 2.0),
        );
        if focused {
            painter.rect_filled(cell, CornerRadius::ZERO, p.amber);
            if let Some(ch) = screen
                .rows
                .get(r)
                .and_then(|row| row.get(c))
                .map(|cell| cell.c)
            {
                painter.text(
                    cell.center(),
                    Align2::CENTER_CENTER,
                    ch,
                    font.clone(),
                    p.on_amber,
                );
            }
        } else {
            painter.rect_stroke(
                cell,
                CornerRadius::ZERO,
                Stroke::new(1.0, p.amber_ink),
                egui::StrokeKind::Inside,
            );
        }
    }
    if screen.exited {
        let bar = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - 28.0), rect.max);
        painter.rect_filled(bar, CornerRadius::ZERO, p.bg_200);
        painter.text(
            Pos2::new(bar.left() + PAD_X, bar.center().y),
            Align2::LEFT_CENTER,
            "the shell ended · click to start another",
            Face::Plex.at(12.0),
            p.ink_muted,
        );
        if response.clicked() {
            system.terminal_close(id);
            *state = TerminalState::default();
            ctx.request_repaint();
        }
    }
}

/// A cell's text and background colours (none: the panel's).
fn colors(cell: &TermCell, p: &Palette, dark: bool) -> (Color32, Option<Color32>) {
    let fg = match cell.fg {
        TermColor::Default => p.ink,
        c => color(c, p, dark),
    };
    let bg = match cell.bg {
        TermColor::Default => None,
        c => Some(color(c, p, dark)),
    };
    let (fg, bg) = if cell.inverse {
        (bg.unwrap_or(p.bg_000), Some(fg))
    } else {
        (fg, bg)
    };
    let fg = if cell.dim { fg.gamma_multiply(0.6) } else { fg };
    (fg, bg)
}

/// The sixteen ANSI colours in the design's palette, then xterm's 256.
fn color(c: TermColor, p: &Palette, dark: bool) -> Color32 {
    match c {
        TermColor::Default => p.ink,
        TermColor::Rgb([r, g, b]) => Color32::from_rgb(r, g, b),
        TermColor::Ansi(i) => {
            let (black, white) = if dark {
                (p.bg_400, p.ink_muted)
            } else {
                (p.ink, p.bg_400)
            };
            match i {
                0 => black,
                1 | 9 => p.error,
                2 | 10 => p.success,
                3 => p.warning,
                11 => p.amber_ink,
                4 | 12 => p.note,
                5 | 13 => p.syn_decorator,
                6 | 14 => p.syn_type,
                7 => white,
                8 => p.ink_faint,
                _ => p.ink,
            }
        }
        TermColor::Indexed(i) if i < 16 => color(TermColor::Ansi(i), p, dark),
        TermColor::Indexed(i) if i < 232 => {
            let i = i - 16;
            let level = |n: u8| if n == 0 { 0 } else { 55 + n * 40 };
            Color32::from_rgb(level(i / 36), level((i / 6) % 6), level(i % 6))
        }
        TermColor::Indexed(i) => {
            let v = 8 + (i - 232) * 10;
            Color32::from_rgb(v, v, v)
        }
    }
}

/// The bytes a terminal sends for a key (xterm's sequences).
fn key_bytes(key: Key, m: Modifiers, app_cursor: bool, mac: bool) -> Option<Vec<u8>> {
    // Ctrl and a letter: its control character (on macOS, the control
    // key; cmd stays Studio's).
    if m.ctrl && !m.alt && (!mac || !m.mac_cmd) {
        let name = key.name();
        if name.len() == 1 {
            let c = name.as_bytes()[0].to_ascii_uppercase();
            if c.is_ascii_uppercase() {
                return Some(vec![c - b'A' + 1]);
            }
        }
        match key {
            Key::OpenBracket => return Some(vec![0x1b]),
            Key::Backslash => return Some(vec![0x1c]),
            Key::CloseBracket => return Some(vec![0x1d]),
            _ => {}
        }
    }
    let arrow = |c: u8| -> Vec<u8> {
        if app_cursor {
            vec![0x1b, b'O', c]
        } else {
            vec![0x1b, b'[', c]
        }
    };
    let seq = |s: &str| s.as_bytes().to_vec();
    Some(match key {
        Key::Enter => vec![b'\r'],
        Key::Backspace => vec![0x7f],
        Key::Tab if m.shift => seq("\x1b[Z"),
        Key::Tab => vec![b'\t'],
        Key::Escape => vec![0x1b],
        Key::ArrowUp => arrow(b'A'),
        Key::ArrowDown => arrow(b'B'),
        Key::ArrowRight => arrow(b'C'),
        Key::ArrowLeft => arrow(b'D'),
        Key::Home => arrow(b'H'),
        Key::End => arrow(b'F'),
        Key::PageUp => seq("\x1b[5~"),
        Key::PageDown => seq("\x1b[6~"),
        Key::Delete => seq("\x1b[3~"),
        Key::Insert => seq("\x1b[2~"),
        Key::F1 => seq("\x1bOP"),
        Key::F2 => seq("\x1bOQ"),
        Key::F3 => seq("\x1bOR"),
        Key::F4 => seq("\x1bOS"),
        Key::F5 => seq("\x1b[15~"),
        Key::F6 => seq("\x1b[17~"),
        Key::F7 => seq("\x1b[18~"),
        Key::F8 => seq("\x1b[19~"),
        Key::F9 => seq("\x1b[20~"),
        Key::F10 => seq("\x1b[21~"),
        Key::F11 => seq("\x1b[23~"),
        Key::F12 => seq("\x1b[24~"),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_become_what_a_terminal_sends() {
        assert_eq!(
            key_bytes(Key::C, Modifiers::CTRL, false, false),
            Some(vec![3])
        );
        assert_eq!(
            key_bytes(Key::ArrowUp, Modifiers::NONE, false, false),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            key_bytes(Key::ArrowUp, Modifiers::NONE, true, false),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            key_bytes(Key::Enter, Modifiers::NONE, false, false),
            Some(vec![b'\r'])
        );
        assert_eq!(
            key_bytes(Key::A, Modifiers::NONE, false, false),
            None,
            "letters come as text"
        );
    }

    #[test]
    fn the_256_colours() {
        let p = &wowlua_studio_ui::DARK;
        assert_eq!(
            color(TermColor::Indexed(16), p, true),
            Color32::from_rgb(0, 0, 0)
        );
        assert_eq!(
            color(TermColor::Indexed(231), p, true),
            Color32::from_rgb(255, 255, 255)
        );
        assert_eq!(
            color(TermColor::Indexed(232), p, true),
            Color32::from_rgb(8, 8, 8)
        );
    }
}
