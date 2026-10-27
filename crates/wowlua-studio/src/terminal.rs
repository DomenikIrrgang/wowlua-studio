//! The terminal panel's shells (S043): the user's shell in a
//! pseudo-terminal (`portable-pty`, ConPTY on Windows), its output read by
//! `alacritty_terminal` on a thread of its own; Studio draws the screen it
//! keeps.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event as TermEvent, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{Color, NamedColor, Processor};
use eframe::egui;
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use wowlua_studio_app::system::{TermCell, TermColor, TermScreen, TermSelect, TermSelectKind};

/// Where the terminal's answers to programs go: back to the shell.
#[derive(Clone)]
struct Listener {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl EventListener for Listener {
    fn send_event(&self, event: TermEvent) {
        if let TermEvent::PtyWrite(text) = event
            && let Ok(mut w) = self.writer.lock()
        {
            let _ = w.write_all(text.as_bytes());
        }
    }
}

struct Session {
    term: Arc<Mutex<Term<Listener>>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    exited: Arc<Mutex<bool>>,
}

#[derive(Default)]
pub struct Terminals {
    sessions: RefCell<HashMap<u64, Session>>,
    next: RefCell<u64>,
}

impl Terminals {
    pub fn open(
        &self,
        cwd: &Path,
        cols: u16,
        rows: u16,
        ctx: &egui::Context,
    ) -> Result<u64, String> {
        let size = PtySize {
            rows: rows.max(2),
            cols: cols.max(10),
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = native_pty_system()
            .openpty(size)
            .map_err(|e| format!("cannot open a terminal: {e}"))?;
        let mut command = CommandBuilder::new_default_prog();
        command.cwd(cwd.as_os_str());
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|e| format!("cannot start the shell: {e}"))?;
        drop(pair.slave);
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("cannot read the terminal: {e}"))?;
        let writer: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(
            pair.master
                .take_writer()
                .map_err(|e| format!("cannot write to the terminal: {e}"))?,
        ));
        let listener = Listener {
            writer: writer.clone(),
        };
        let term = Arc::new(Mutex::new(Term::new(
            Config::default(),
            &TermSize::new(size.cols as usize, size.rows as usize),
            listener,
        )));
        let exited = Arc::new(Mutex::new(false));
        {
            let term = term.clone();
            let exited = exited.clone();
            let ctx = ctx.clone();
            std::thread::Builder::new()
                .name("terminal".into())
                .spawn(move || {
                    let mut parser: Processor = Processor::new();
                    let mut buf = [0u8; 16 * 1024];
                    loop {
                        match reader.read(&mut buf) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                if let Ok(mut t) = term.lock() {
                                    parser.advance(&mut *t, &buf[..n]);
                                }
                                ctx.request_repaint();
                            }
                        }
                    }
                    if let Ok(mut e) = exited.lock() {
                        *e = true;
                    }
                    ctx.request_repaint();
                })
                .map_err(|e| format!("cannot start the terminal's reader: {e}"))?;
        }
        let mut next = self.next.borrow_mut();
        *next += 1;
        self.sessions.borrow_mut().insert(
            *next,
            Session {
                term,
                writer,
                master: pair.master,
                child,
                exited,
            },
        );
        Ok(*next)
    }

    pub fn input(&self, id: u64, bytes: &[u8]) {
        if let Some(s) = self.sessions.borrow().get(&id) {
            // Typing shows the bottom again.
            if let Ok(mut t) = s.term.lock() {
                t.scroll_display(Scroll::Bottom);
            }
            if let Ok(mut w) = s.writer.lock() {
                let _ = w.write_all(bytes);
                let _ = w.flush();
            }
        }
    }

    pub fn resize(&self, id: u64, cols: u16, rows: u16) {
        if let Some(s) = self.sessions.borrow().get(&id) {
            let (cols, rows) = (cols.max(10), rows.max(2));
            let _ = s.master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
            if let Ok(mut t) = s.term.lock() {
                t.resize(TermSize::new(cols as usize, rows as usize));
            }
        }
    }

    pub fn scroll(&self, id: u64, lines: i32) {
        if let Some(s) = self.sessions.borrow().get(&id)
            && let Ok(mut t) = s.term.lock()
        {
            t.scroll_display(Scroll::Delta(lines));
        }
    }

    /// Starts, extends or clears the selection: a cell of the screen, as
    /// far back as it is scrolled.
    pub fn select(&self, id: u64, select: TermSelect) {
        let sessions = self.sessions.borrow();
        let Some(s) = sessions.get(&id) else {
            return;
        };
        let Ok(mut t) = s.term.lock() else {
            return;
        };
        let offset = t.grid().display_offset() as i32;
        let last = t.columns().saturating_sub(1);
        let point =
            |row: usize, col: usize| Point::new(Line(row as i32 - offset), Column(col.min(last)));
        let side = |right: bool| if right { Side::Right } else { Side::Left };
        match select {
            TermSelect::Start {
                row,
                col,
                right,
                kind,
            } => {
                let ty = match kind {
                    TermSelectKind::Chars => SelectionType::Simple,
                    TermSelectKind::Words => SelectionType::Semantic,
                    TermSelectKind::Lines => SelectionType::Lines,
                };
                t.selection = Some(Selection::new(ty, point(row, col), side(right)));
            }
            TermSelect::Extend { row, col, right } => {
                let at = point(row, col);
                if let Some(selection) = &mut t.selection {
                    selection.update(at, side(right));
                }
            }
            TermSelect::Clear => t.selection = None,
        }
    }

    /// The selected text.
    pub fn selection(&self, id: u64) -> Option<String> {
        let sessions = self.sessions.borrow();
        let t = sessions.get(&id)?.term.lock().ok()?;
        t.selection_to_string().filter(|s| !s.is_empty())
    }

    pub fn screen(&self, id: u64) -> Option<TermScreen> {
        let sessions = self.sessions.borrow();
        let s = sessions.get(&id)?;
        let exited = s.exited.lock().map(|e| *e).unwrap_or(true);
        let t = s.term.lock().ok()?;
        let rows = t.screen_lines();
        let cols = t.columns();
        let content = t.renderable_content();
        let offset = content.display_offset;
        let mut screen = TermScreen {
            rows: vec![
                vec![
                    TermCell {
                        c: ' ',
                        ..TermCell::default()
                    };
                    cols
                ];
                rows
            ],
            app_cursor: content.mode.contains(TermMode::APP_CURSOR),
            bracketed_paste: content.mode.contains(TermMode::BRACKETED_PASTE),
            scrolled: offset,
            exited,
            cursor: None,
        };
        let colors = content.colors;
        let selection = content.selection;
        let color = |c: Color| -> TermColor {
            match c {
                Color::Spec(rgb) => TermColor::Rgb([rgb.r, rgb.g, rgb.b]),
                Color::Indexed(i) => match colors[i as usize] {
                    Some(rgb) => TermColor::Rgb([rgb.r, rgb.g, rgb.b]),
                    None if i < 16 => TermColor::Ansi(i),
                    None => TermColor::Indexed(i),
                },
                Color::Named(n) => {
                    let i = n as usize;
                    match colors[i] {
                        Some(rgb)
                            if !matches!(n, NamedColor::Foreground | NamedColor::Background) =>
                        {
                            TermColor::Rgb([rgb.r, rgb.g, rgb.b])
                        }
                        _ if i < 16 => TermColor::Ansi(i as u8),
                        _ => match n {
                            NamedColor::DimBlack => TermColor::Ansi(0),
                            NamedColor::DimRed => TermColor::Ansi(1),
                            NamedColor::DimGreen => TermColor::Ansi(2),
                            NamedColor::DimYellow => TermColor::Ansi(3),
                            NamedColor::DimBlue => TermColor::Ansi(4),
                            NamedColor::DimMagenta => TermColor::Ansi(5),
                            NamedColor::DimCyan => TermColor::Ansi(6),
                            NamedColor::DimWhite => TermColor::Ansi(7),
                            NamedColor::BrightForeground => TermColor::Ansi(15),
                            _ => TermColor::Default,
                        },
                    }
                }
            }
        };
        for cell in content.display_iter {
            let row = cell.point.line.0 + offset as i32;
            let col = cell.point.column.0;
            if row < 0 || row as usize >= rows || col >= cols {
                continue;
            }
            let f = cell.flags;
            screen.rows[row as usize][col] = TermCell {
                c: if f.contains(Flags::HIDDEN) {
                    ' '
                } else {
                    cell.c
                },
                fg: color(cell.fg),
                bg: color(cell.bg),
                bold: f.contains(Flags::BOLD),
                italic: f.contains(Flags::ITALIC),
                underline: f.contains(Flags::UNDERLINE),
                inverse: f.contains(Flags::INVERSE),
                dim: f.contains(Flags::DIM),
                spacer: f.contains(Flags::WIDE_CHAR_SPACER),
                selected: selection.is_some_and(|s| s.contains(cell.point)),
            };
        }
        let cursor = content.cursor;
        if !matches!(
            cursor.shape,
            alacritty_terminal::vte::ansi::CursorShape::Hidden
        ) {
            let row = cursor.point.line.0 + offset as i32;
            if row >= 0 && (row as usize) < rows {
                screen.cursor = Some((row as usize, cursor.point.column.0));
            }
        }
        Some(screen)
    }

    pub fn close(&self, id: u64) {
        if let Some(mut s) = self.sessions.borrow_mut().remove(&id) {
            let _ = s.child.kill();
        }
    }
}

impl Drop for Terminals {
    fn drop(&mut self) {
        for (_, mut s) in self.sessions.borrow_mut().drain() {
            let _ = s.child.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The row and column where `text` shows on the screen, its last
    /// showing (the command's output, after the line typed).
    fn find(screen: &TermScreen, text: &str) -> Option<(usize, usize)> {
        screen.rows.iter().enumerate().rev().find_map(|(r, row)| {
            let line: String = row.iter().map(|c| c.c).collect();
            let b = line.find(text)?;
            Some((r, line[..b].chars().count()))
        })
    }

    #[test]
    fn a_word_and_a_line_are_selected_and_copied() {
        let terminals = Terminals::default();
        let dir = std::env::temp_dir();
        let id = terminals
            .open(&dir, 80, 24, &egui::Context::default())
            .expect("a shell");
        terminals.input(id, b"echo hello-select world\r");
        // The output, once the shell has written it.
        let mut at = None;
        for _ in 0..100 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            let screen = terminals.screen(id).expect("a screen");
            let (Some(typed), Some(out)) = (
                find(&screen, "echo hello"),
                find(&screen, "hello-select world"),
            ) else {
                continue;
            };
            if out.0 > typed.0 {
                at = Some(out);
                break;
            }
        }
        let (row, col) = at.expect("the echo's output");
        terminals.select(
            id,
            TermSelect::Start {
                row,
                col: col + 2,
                right: false,
                kind: TermSelectKind::Words,
            },
        );
        assert_eq!(terminals.selection(id).as_deref(), Some("hello-select"));
        let screen = terminals.screen(id).unwrap();
        assert!(screen.rows[row][col].selected && !screen.rows[row][col + 13].selected);
        terminals.select(
            id,
            TermSelect::Start {
                row,
                col: 0,
                right: false,
                kind: TermSelectKind::Lines,
            },
        );
        assert_eq!(
            terminals
                .selection(id)
                .map(|s| s.trim_end().to_owned())
                .as_deref(),
            Some("hello-select world")
        );
        // Characters: from `s` of select to the space.
        terminals.select(
            id,
            TermSelect::Start {
                row,
                col: col + 6,
                right: false,
                kind: TermSelectKind::Chars,
            },
        );
        terminals.select(
            id,
            TermSelect::Extend {
                row,
                col: col + 11,
                right: true,
            },
        );
        assert_eq!(terminals.selection(id).as_deref(), Some("select"));
        terminals.select(id, TermSelect::Clear);
        assert_eq!(terminals.selection(id), None);
        terminals.close(id);
    }
}
