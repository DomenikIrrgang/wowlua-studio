//! An open file's text: its lines, edits with undo and redo, the
//! selection, and what the disk had when it was last read or written.
//! Positions are a line and a column in characters.

use std::path::PathBuf;

/// A place in the text: a line and a column, both from 0, the column in
/// characters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

impl Pos {
    pub fn new(line: usize, col: usize) -> Pos {
        Pos { line, col }
    }
}

/// The selection: from `anchor` to `head`, where the caret is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Pos,
    pub head: Pos,
}

impl Selection {
    pub fn caret(at: Pos) -> Selection {
        Selection {
            anchor: at,
            head: at,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// The selection's start and end.
    pub fn range(&self) -> (Pos, Pos) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }
}

/// One replacement: `removed` at `at` became `inserted`.
#[derive(Clone, Debug, PartialEq)]
struct Edit {
    at: Pos,
    removed: String,
    inserted: String,
}

/// What a step of the undo history was, for joining typing into one step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKind {
    Typing,
    Deleting,
    Other,
}

/// A step of the undo history.
#[derive(Clone, Debug)]
struct Step {
    edits: Vec<Edit>,
    before: Selection,
    after: Selection,
    kind: EditKind,
}

pub struct Document {
    pub path: PathBuf,
    lines: Vec<String>,
    /// The file's lines end in `\r\n`; saving keeps them so.
    crlf: bool,
    /// Counts every change, for the language server and caches.
    pub version: u64,
    pub selection: Selection,
    /// The column moving up and down keeps to.
    pub goal_col: Option<usize>,
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// The undo depth at the last save; `None` when no undo or redo
    /// reaches it.
    saved: Option<usize>,
    /// The text on disk as last read or written: a change on disk is
    /// told apart from Studio's own save by it.
    pub disk: String,
}

impl Document {
    pub fn new(path: PathBuf, text: &str) -> Document {
        let crlf = text.contains("\r\n");
        Document {
            path,
            lines: split(text),
            crlf,
            version: 0,
            selection: Selection::default(),
            goal_col: None,
            undo: Vec::new(),
            redo: Vec::new(),
            saved: Some(0),
            disk: text.to_owned(),
        }
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line(&self, i: usize) -> &str {
        self.lines.get(i).map_or("", String::as_str)
    }

    /// The line's length in characters.
    pub fn line_len(&self, i: usize) -> usize {
        self.line(i).chars().count()
    }

    /// The text with `\n` line ends.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// The text as it is written to disk: the file's own line ends.
    pub fn disk_text(&self) -> String {
        self.lines.join(if self.crlf { "\r\n" } else { "\n" })
    }

    pub fn is_dirty(&self) -> bool {
        self.saved != Some(self.undo.len())
    }

    /// The text was written to disk.
    pub fn mark_saved(&mut self) {
        self.saved = Some(self.undo.len());
        self.disk = self.disk_text();
    }

    /// `pos`, moved into the text.
    pub fn clamp(&self, pos: Pos) -> Pos {
        let line = pos.line.min(self.lines.len() - 1);
        Pos::new(line, pos.col.min(self.line_len(line)))
    }

    /// The end of the text.
    pub fn end(&self) -> Pos {
        let line = self.lines.len() - 1;
        Pos::new(line, self.line_len(line))
    }

    /// The text from `start` to `end`.
    pub fn slice(&self, start: Pos, end: Pos) -> String {
        let (start, end) = (self.clamp(start), self.clamp(end));
        if start.line == end.line {
            let line = self.line(start.line);
            return line[byte(line, start.col)..byte(line, end.col)].to_owned();
        }
        let first = self.line(start.line);
        let mut out = first[byte(first, start.col)..].to_owned();
        for i in start.line + 1..end.line {
            out.push('\n');
            out.push_str(self.line(i));
        }
        out.push('\n');
        let last = self.line(end.line);
        out.push_str(&last[..byte(last, end.col)]);
        out
    }

    /// The selected text.
    pub fn selected(&self) -> String {
        let (start, end) = self.selection.range();
        self.slice(start, end)
    }

    /// Replaces `start..end` with `text`, kept in the undo history: typing
    /// and deleting at the caret join the step before them. The caret goes
    /// after the new text.
    pub fn edit(&mut self, start: Pos, end: Pos, text: &str, kind: EditKind) {
        let before = self.selection;
        let (start, end) = (self.clamp(start), self.clamp(end));
        let removed = self.slice(start, end);
        if removed.is_empty() && text.is_empty() {
            return;
        }
        let after_end = self.replace(start, end, text);
        self.selection = Selection::caret(after_end);
        self.goal_col = None;
        let edit = Edit {
            at: start,
            removed,
            inserted: text.to_owned(),
        };
        self.redo.clear();
        if self.saved.is_some_and(|s| s > self.undo.len()) {
            self.saved = None;
        }
        let joins = kind != EditKind::Other
            && self.saved != Some(self.undo.len())
            && self.undo.last().is_some_and(|step| {
                step.kind == kind && step.after == before && !text.contains('\n')
            });
        if joins {
            let step = self.undo.last_mut().expect("joined");
            step.edits.push(edit);
            step.after = self.selection;
        } else {
            self.undo.push(Step {
                edits: vec![edit],
                before,
                after: self.selection,
                kind,
            });
        }
    }

    /// Several replacements (a formatting, a rename) as one step: each
    /// range is where it was before any of them. The selection stays, as
    /// far as the text goes.
    pub fn edits(&mut self, mut list: Vec<(Pos, Pos, String)>) {
        if list.is_empty() {
            return;
        }
        let before = self.selection;
        // From the end, so the earlier ranges stay where they were.
        list.sort_by_key(|a| std::cmp::Reverse(a.0));
        let mut edits = Vec::new();
        for (start, end, text) in list {
            let (start, end) = (self.clamp(start), self.clamp(end));
            let removed = self.slice(start, end);
            if removed == text {
                continue;
            }
            self.replace(start, end, &text);
            edits.push(Edit {
                at: start,
                removed,
                inserted: text,
            });
        }
        if edits.is_empty() {
            return;
        }
        self.redo.clear();
        if self.saved.is_some_and(|s| s > self.undo.len()) {
            self.saved = None;
        }
        self.selection = Selection {
            anchor: self.clamp(before.anchor),
            head: self.clamp(before.head),
        };
        self.undo.push(Step {
            edits,
            before,
            after: self.selection,
            kind: EditKind::Other,
        });
    }

    /// Replaces the selection with `text`.
    pub fn insert(&mut self, text: &str, kind: EditKind) {
        let (start, end) = self.selection.range();
        self.edit(start, end, text, kind);
    }

    /// The text on disk replaces the editor's, as one step that can be
    /// undone; the caret stays where it was, as far as the text goes.
    pub fn reload(&mut self, disk: &str) {
        let selection = self.selection;
        let text = disk.replace("\r\n", "\n");
        if text != self.text() {
            let end = self.end();
            self.edit(Pos::default(), end, &text, EditKind::Other);
            let kept = Selection {
                anchor: self.clamp(selection.anchor),
                head: self.clamp(selection.head),
            };
            if let Some(step) = self.undo.last_mut() {
                step.after = kept;
                self.selection = step.after;
            }
        }
        self.crlf = disk.contains("\r\n");
        self.saved = Some(self.undo.len());
        self.disk = disk.to_owned();
    }

    pub fn undo(&mut self) -> bool {
        let Some(step) = self.undo.pop() else {
            return false;
        };
        for edit in step.edits.iter().rev() {
            let end = end_of(edit.at, &edit.inserted);
            self.replace(edit.at, end, &edit.removed);
        }
        self.selection = step.before;
        self.redo.push(step);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(step) = self.redo.pop() else {
            return false;
        };
        for edit in &step.edits {
            let end = end_of(edit.at, &edit.removed);
            self.replace(edit.at, end, &edit.inserted);
        }
        self.selection = step.after;
        self.undo.push(step);
        true
    }

    /// Replaces `start..end` with `text`; returns the end of the new text.
    fn replace(&mut self, start: Pos, end: Pos, text: &str) -> Pos {
        self.version += 1;
        let first = &self.lines[start.line];
        let head = first[..byte(first, start.col)].to_owned();
        let last = &self.lines[end.line];
        let tail = last[byte(last, end.col)..].to_owned();
        let mut new: Vec<String> = text.split('\n').map(str::to_owned).collect();
        let end_pos = Pos::new(
            start.line + new.len() - 1,
            if new.len() == 1 {
                start.col + new[0].chars().count()
            } else {
                new.last().map_or(0, |l| l.chars().count())
            },
        );
        new[0].insert_str(0, &head);
        new.last_mut().expect("one line at least").push_str(&tail);
        self.lines.splice(start.line..=end.line, new);
        end_pos
    }
}

/// The text's lines; `\r\n` and `\r` end lines too.
fn split(text: &str) -> Vec<String> {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .map(str::to_owned)
        .collect()
}

/// Where `text` ends when it is put at `at`.
fn end_of(at: Pos, text: &str) -> Pos {
    let lines = text.split('\n').count();
    let last = text.rsplit('\n').next().unwrap_or("").chars().count();
    if lines == 1 {
        Pos::new(at.line, at.col + last)
    } else {
        Pos::new(at.line + lines - 1, last)
    }
}

/// The byte offset of character `col` in `line` (its length past the end).
pub fn byte(line: &str, col: usize) -> usize {
    line.char_indices().nth(col).map_or(line.len(), |(b, _)| b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Document {
        Document::new(PathBuf::from("a.wowlua"), text)
    }

    #[test]
    fn edits_across_lines_and_back() {
        let mut d = doc("local a = 1\nlocal b = 2\n");
        d.edit(
            Pos::new(0, 6),
            Pos::new(1, 7),
            "x\nlocal y",
            EditKind::Other,
        );
        assert_eq!(d.text(), "local x\nlocal y = 2\n");
        assert_eq!(d.selection.head, Pos::new(1, 7));
        assert!(d.is_dirty());
        assert!(d.undo());
        assert_eq!(d.text(), "local a = 1\nlocal b = 2\n");
        assert!(!d.is_dirty());
        assert!(d.redo());
        assert_eq!(d.text(), "local x\nlocal y = 2\n");
    }

    #[test]
    fn typing_is_one_step() {
        let mut d = doc("");
        for c in ["a", "b", "c"] {
            d.insert(c, EditKind::Typing);
        }
        d.insert("\n", EditKind::Typing);
        assert_eq!(d.text(), "abc\n");
        d.undo();
        assert_eq!(d.text(), "abc");
        d.undo();
        assert_eq!(d.text(), "");
    }

    #[test]
    fn line_ends_and_characters_are_kept() {
        let mut d = doc("ä = 1\r\nb = 2\r\n");
        assert_eq!(d.line(0), "ä = 1");
        d.edit(Pos::new(0, 1), Pos::new(0, 1), "x", EditKind::Typing);
        assert_eq!(d.disk_text(), "äx = 1\r\nb = 2\r\n");
        assert_eq!(d.slice(Pos::new(0, 0), Pos::new(1, 1)), "äx = 1\nb");
    }

    #[test]
    fn a_reload_is_saved_and_can_be_undone() {
        let mut d = doc("a\n");
        d.insert("x", EditKind::Typing);
        d.reload("theirs\n");
        assert_eq!(d.text(), "theirs\n");
        assert!(!d.is_dirty());
        d.undo();
        assert_eq!(d.text(), "xa\n");
        assert!(d.is_dirty());
    }
}
