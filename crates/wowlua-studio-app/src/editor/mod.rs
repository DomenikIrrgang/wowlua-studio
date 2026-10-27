//! The editor (the design's editor board): open files in tabs, each a
//! [`Document`] drawn by [`view`], highlighted by wowlua's lexer (S045).
//! Files changed outside Studio reload, or show the changed-on-disk banner
//! when they have unsaved edits (S049).

pub mod diff;
pub mod doc;
pub mod view;

use std::path::{Path, PathBuf};

use crate::system::{Highlight, Syntax, System};
pub use doc::{Document, EditKind, Pos, Selection};

/// A file's highlighting: per line, character columns and colours.
type LineHighlights = Vec<Vec<(usize, usize, Syntax)>>;

/// Lenses over functions: the line, the title, where its references are
/// asked for.
pub type Lenses = Vec<(usize, String, Pos)>;

/// An open file.
pub struct Tab {
    pub doc: Document,
    /// The highlighting, per line: character columns and colour, for the
    /// version it was made for.
    highlights: Option<(u64, LineHighlights)>,
    /// The longest line's width in characters, for the version.
    widest: Option<(u64, usize)>,
    /// The file changed on disk while it had unsaved edits (S049).
    pub clash: Option<Clash>,
    /// The caret moved: scroll it into view.
    pub reveal: bool,
    /// The language server's hover where the pointer rests.
    pub hover: Option<(Pos, String)>,
    /// Completion suggestions at the caret.
    pub completion: Option<Completion>,
    /// The call's signature, its active parameter in characters.
    pub signature: Option<(String, Option<(usize, usize)>)>,
    /// The new name being typed for a rename, and where.
    pub rename: Option<(Pos, String)>,
    /// The types of locals declared without one (inlay hints), for the
    /// version they were asked for.
    pub hints: Option<(u64, Vec<(Pos, String)>)>,
    /// Lenses over functions: the line, the title, where its references
    /// are asked for; for the version.
    pub lenses: Option<(u64, Lenses)>,
    /// The lines that fold (first, last), for the version.
    foldable: Option<(u64, Vec<(usize, usize)>)>,
    /// The folded ranges (first, last): the first line stays.
    pub folded: Vec<(usize, usize)>,
    /// The quick fixes at the caret: their titles and edits; which is
    /// chosen.
    pub fixes: Option<(Vec<crate::lsp::Fix>, usize)>,
}

/// Completion suggestions: where they were asked for, and which is chosen.
pub struct Completion {
    pub at: Pos,
    pub items: Vec<crate::lsp::Item>,
    pub selected: usize,
}

/// The disk's text against the editor's.
pub struct Clash {
    pub theirs: String,
    /// A Git checkout changed the file (`.git/HEAD` changed with it).
    pub checkout: bool,
    /// The diff is shown.
    pub comparing: bool,
    /// Found when saving, not when the disk changed.
    pub on_save: bool,
}

/// What happened to an open file on disk.
#[derive(Clone, Debug, PartialEq)]
pub enum DiskChange {
    /// Read again: no unsaved edits.
    Reloaded(PathBuf),
    /// Unsaved edits: the banner shows.
    Clashed(PathBuf),
    /// Deleted outside Studio: the tab closed (S048), with the text it had.
    Deleted { path: PathBuf, text: String },
}

#[derive(Default)]
pub struct Editors {
    pub tabs: Vec<Tab>,
    pub active: usize,
    /// Focus the active editor at the next frame.
    pub focus: bool,
}

impl Editors {
    pub fn active(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    pub fn active_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active)
    }

    pub fn find(&self, path: &Path) -> Option<usize> {
        self.tabs.iter().position(|t| t.doc.path == path)
    }

    /// Opens `path`, or shows it when it is open.
    pub fn open(&mut self, path: &Path, system: &dyn System) -> Result<(), String> {
        if let Some(i) = self.find(path) {
            self.active = i;
            self.focus = true;
            return Ok(());
        }
        let text = system.read_file(path)?;
        self.tabs
            .push(Tab::new(Document::new(path.to_path_buf(), &text)));
        self.active = self.tabs.len() - 1;
        self.focus = true;
        Ok(())
    }

    pub fn close(&mut self, i: usize) {
        if i < self.tabs.len() {
            self.tabs.remove(i);
            if self.active >= i && self.active > 0 {
                self.active -= 1;
            }
        }
    }

    pub fn any_dirty(&self) -> bool {
        self.tabs.iter().any(|t| t.doc.is_dirty())
    }

    /// Writes tab `i`, unless the disk changed since Studio read it: then
    /// the banner asks first (S049).
    pub fn save(&mut self, i: usize, system: &dyn System) -> Result<bool, String> {
        let Some(tab) = self.tabs.get_mut(i) else {
            return Ok(false);
        };
        if let Ok(disk) = system.read_file(&tab.doc.path)
            && disk != tab.doc.disk
            && tab.clash.is_none()
        {
            tab.clash = Some(Clash {
                theirs: disk,
                checkout: false,
                comparing: false,
                on_save: true,
            });
            return Ok(false);
        }
        system.write_file(&tab.doc.path, &tab.doc.disk_text())?;
        tab.doc.mark_saved();
        tab.clash = None;
        Ok(true)
    }

    /// Files changed on disk: open ones reload, clash or close. `checkout`
    /// is whether Git's `HEAD` changed with them.
    pub fn disk_changed(
        &mut self,
        paths: &[PathBuf],
        checkout: bool,
        system: &dyn System,
    ) -> Vec<DiskChange> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.tabs.len() {
            let tab = &mut self.tabs[i];
            if !paths.contains(&tab.doc.path) {
                i += 1;
                continue;
            }
            match system.read_file(&tab.doc.path) {
                Ok(disk) if disk == tab.doc.disk => {}
                Ok(disk) if !tab.doc.is_dirty() => {
                    tab.doc.reload(&disk);
                    out.push(DiskChange::Reloaded(tab.doc.path.clone()));
                }
                Ok(disk) => {
                    tab.clash = Some(Clash {
                        theirs: disk,
                        checkout,
                        comparing: false,
                        on_save: false,
                    });
                    out.push(DiskChange::Clashed(tab.doc.path.clone()));
                }
                Err(_) if !system.exists(&tab.doc.path) => {
                    let text = tab.doc.disk_text();
                    let path = tab.doc.path.clone();
                    self.close(i);
                    out.push(DiskChange::Deleted { path, text });
                    continue;
                }
                Err(_) => {}
            }
            i += 1;
        }
        out
    }
}

impl Tab {
    pub fn new(doc: Document) -> Tab {
        Tab {
            doc,
            highlights: None,
            widest: None,
            clash: None,
            reveal: true,
            hover: None,
            completion: None,
            signature: None,
            rename: None,
            hints: None,
            lenses: None,
            foldable: None,
            folded: Vec::new(),
            fixes: None,
        }
    }

    /// Keep mine: the disk's change is passed over; the next save writes
    /// over it.
    pub fn keep_mine(&mut self) {
        if let Some(clash) = self.clash.take() {
            self.doc.disk = clash.theirs;
        }
    }

    /// Take theirs: the disk's text replaces the editor's (undoable).
    pub fn take_theirs(&mut self) {
        if let Some(clash) = self.clash.take() {
            self.doc.reload(&clash.theirs);
        }
    }

    /// The highlighting of each line, made again when the text changed.
    pub fn highlights(&mut self, system: &dyn System) -> &[Vec<(usize, usize, Syntax)>] {
        let version = self.doc.version;
        if self.highlights.as_ref().is_none_or(|(v, _)| *v != version) {
            let lines = if highlighted(&self.doc.path) {
                per_line(&self.doc, &system.highlight(&self.doc.text()))
            } else {
                Vec::new()
            };
            self.highlights = Some((version, lines));
        }
        &self.highlights.as_ref().expect("made").1
    }

    /// The lines that fold, made again when the text changed; folds
    /// whose first line no longer folds open.
    pub fn foldable(&mut self, system: &dyn System) -> &[(usize, usize)] {
        let version = self.doc.version;
        if self.foldable.as_ref().is_none_or(|(v, _)| *v != version) {
            let ranges = if highlighted(&self.doc.path) {
                system.folding(&self.doc.text())
            } else {
                Vec::new()
            };
            self.folded = self
                .folded
                .iter()
                .filter_map(|&(first, _)| ranges.iter().find(|r| r.0 == first).copied())
                .collect();
            self.foldable = Some((version, ranges));
        }
        &self.foldable.as_ref().expect("made").1
    }

    /// The longest line's width in display columns.
    pub fn widest(&mut self) -> usize {
        let version = self.doc.version;
        if self.widest.is_none_or(|(v, _)| v != version) {
            let w = (0..self.doc.line_count())
                .map(|i| {
                    view::display(self.doc.line(i))
                        .1
                        .last()
                        .copied()
                        .unwrap_or(0)
                })
                .max()
                .unwrap_or(0);
            self.widest = Some((version, w));
        }
        self.widest.expect("made").1
    }
}

/// Files the lexer highlights: wowlua and Lua. Large data files stay
/// plain, so typing in them stays fast.
fn highlighted(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    matches!(ext, "wowlua" | "lua")
}

/// Byte ranges over the whole text, as display columns per line.
fn per_line(doc: &Document, highlights: &[Highlight]) -> Vec<Vec<(usize, usize, Syntax)>> {
    let mut starts = Vec::with_capacity(doc.line_count());
    let mut at = 0;
    for i in 0..doc.line_count() {
        starts.push(at);
        at += doc.line(i).len() + 1;
    }
    let mut out: Vec<Vec<(usize, usize, Syntax)>> = vec![Vec::new(); doc.line_count()];
    // Each line's byte offsets as display columns, made once per line.
    let mut columns: Vec<Option<Vec<usize>>> = vec![None; doc.line_count()];
    for h in highlights {
        let first = starts.partition_point(|&s| s <= h.start).saturating_sub(1);
        let mut line = first;
        while line < doc.line_count() && starts[line] < h.end {
            let text = doc.line(line);
            let from = h.start.saturating_sub(starts[line]).min(text.len());
            let to = (h.end - starts[line]).min(text.len());
            if to > from {
                let cols = columns[line].get_or_insert_with(|| char_columns(text));
                out[line].push((cols[from], cols[to], h.syntax));
            }
            line += 1;
        }
    }
    out
}

/// Each byte offset of `line` (and its end) as a character column.
fn char_columns(line: &str) -> Vec<usize> {
    let mut out = vec![0; line.len() + 1];
    let mut count = 0;
    for (col, (b, c)) in line.char_indices().enumerate() {
        for slot in &mut out[b..b + c.len_utf8()] {
            *slot = col;
        }
        count = col + 1;
    }
    out[line.len()] = count;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::NoSystem;

    #[test]
    fn highlights_split_into_lines_and_columns() {
        let doc = Document::new(PathBuf::from("a.wowlua"), "--[[a\n\tb]] x");
        let lines = per_line(
            &doc,
            &[Highlight {
                start: 0,
                end: 10,
                syntax: Syntax::Comment,
            }],
        );
        assert_eq!(lines[0], [(0, 5, Syntax::Comment)]);
        // Characters: the tab is one.
        assert_eq!(lines[1], [(0, 4, Syntax::Comment)]);
    }

    #[test]
    fn a_tab_opens_once() {
        let mut editors = Editors::default();
        assert!(editors.open(Path::new("/x.wowlua"), &NoSystem).is_err());
        editors
            .tabs
            .push(Tab::new(Document::new(PathBuf::from("/a"), "")));
        editors
            .tabs
            .push(Tab::new(Document::new(PathBuf::from("/b"), "")));
        editors.open(Path::new("/a"), &NoSystem).unwrap();
        assert_eq!(editors.active, 0);
        assert_eq!(editors.tabs.len(), 2);
    }
}
