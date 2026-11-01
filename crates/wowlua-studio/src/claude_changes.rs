//! What Claude changed in a conversation (M14, S098): each file as it was
//! before the conversation's first change, a checkpoint before each of the
//! user's messages (the files as they were before Claude's first change
//! after it), and what Claude last left in each file, so the user's own
//! edits since are told apart. The review diffs each file against how it
//! was; a hunk undone can be redone; going back to a checkpoint restores
//! only what Claude changed: a file the user edited since is kept.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use wowlua_studio_app::system::{ClaudeChanges, ClaudeCheckpoint, ClaudeFile, ClaudeHunk};

/// A file's text, none when it does not exist.
type Text = Option<String>;

/// The conversation's changes, shared by Claude's tools and Studio.
pub type Shared = Arc<Mutex<Changes>>;

#[derive(Default)]
pub struct Changes {
    root: PathBuf,
    /// Each file before the conversation's first change, by its path in
    /// the workspace.
    base: BTreeMap<String, Text>,
    /// Each checkpoint's label (the user's message) and the files as they
    /// were before Claude's first change after it.
    checkpoints: Vec<(String, BTreeMap<String, Text>)>,
    /// What Claude last left in each file.
    written: BTreeMap<String, Text>,
    /// Hunks undone, for redo.
    undone: Vec<(String, ClaudeHunk)>,
}

impl Changes {
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn new(root: &Path) -> Changes {
        Changes {
            root: root.to_path_buf(),
            ..Changes::default()
        }
    }

    fn read(&self, rel: &str) -> Text {
        std::fs::read_to_string(self.root.join(rel)).ok()
    }

    fn write(&self, rel: &str, text: &Text) -> Result<(), String> {
        let path = self.root.join(rel);
        match text {
            Some(t) => {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| format!("{rel}: {e}"))?;
                }
                std::fs::write(&path, t).map_err(|e| format!("{rel}: {e}"))
            }
            None => match std::fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(format!("{rel}: {e}")),
            },
        }
    }

    /// Before the user's message `label`.
    pub fn checkpoint(&mut self, label: &str) {
        self.checkpoints.push((label.to_owned(), BTreeMap::new()));
    }

    /// Claude is about to change `rel`: how it is now is kept.
    pub fn before(&mut self, rel: &str) {
        let now = self.read(rel);
        self.base
            .entry(rel.to_owned())
            .or_insert_with(|| now.clone());
        if let Some((_, files)) = self.checkpoints.last_mut() {
            files.entry(rel.to_owned()).or_insert(now);
        }
    }

    /// Claude changed `rel`.
    pub fn after(&mut self, rel: &str) {
        let now = self.read(rel);
        self.written.insert(rel.to_owned(), now);
    }

    /// The review: each file Claude changed, its hunks against how it was.
    pub fn view(&self) -> ClaudeChanges {
        let mut files = Vec::new();
        for (rel, was) in &self.base {
            let now = self.read(rel);
            if &now == was {
                continue;
            }
            let (a, b) = (was.as_deref().unwrap_or(""), now.as_deref().unwrap_or(""));
            let hunks = hunks(a, b);
            files.push(ClaudeFile {
                path: rel.clone(),
                added: hunks.iter().map(|h| h.new.len()).sum(),
                removed: hunks.iter().map(|h| h.old.len()).sum(),
                created: was.is_none(),
                deleted: now.is_none(),
                hunks,
            });
        }
        ClaudeChanges {
            files,
            checkpoints: self
                .checkpoints
                .iter()
                .map(|(label, files)| ClaudeCheckpoint {
                    label: label.clone(),
                    files: files.len(),
                })
                .collect(),
            undone: self.undone.clone(),
        }
    }

    /// Undoes `hunk` of `rel` (as the review showed it).
    pub fn undo(&mut self, rel: &str, hunk: &ClaudeHunk) -> Result<(), String> {
        let now = self.read(rel).unwrap_or_default();
        let mut lines = split(&now);
        let at = hunk.new_start.saturating_sub(1);
        if lines.get(at..at + hunk.new.len()) != Some(&hunk.new[..]) {
            return Err(format!("{rel} changed since: look again"));
        }
        lines.splice(at..at + hunk.new.len(), hunk.old.iter().cloned());
        let text = join(&lines, &now);
        // (a file Claude made, all of it undone: gone again)
        let text = match self.base.get(rel) {
            Some(None) if text.is_empty() => None,
            _ => Some(text),
        };
        self.write(rel, &text)?;
        self.written.insert(rel.to_owned(), text);
        self.undone.push((rel.to_owned(), hunk.clone()));
        Ok(())
    }

    /// Redoes the `i`th hunk undone, where its old lines are now.
    pub fn redo(&mut self, i: usize) -> Result<(), String> {
        let Some((rel, hunk)) = self.undone.get(i).cloned() else {
            return Err("nothing to redo".into());
        };
        let now = self.read(&rel).unwrap_or_default();
        let mut lines = split(&now);
        let want = hunk.old_start.saturating_sub(1);
        let n = hunk.old.len();
        // the old lines nearest where they were
        let mut found = None;
        for d in 0..=lines.len() {
            for at in [want.checked_sub(d), Some(want + d)].into_iter().flatten() {
                if found.is_none() && lines.get(at..at + n) == Some(&hunk.old[..]) {
                    found = Some(at);
                }
            }
            if found.is_some() {
                break;
            }
        }
        let at = found.ok_or_else(|| format!("{rel} changed since: the hunk does not fit"))?;
        lines.splice(at..at + n, hunk.new.iter().cloned());
        let text = Some(join(&lines, &now));
        self.write(&rel, &text)?;
        self.written.insert(rel.clone(), text);
        self.undone.remove(i);
        Ok(())
    }

    /// Back to checkpoint `k` (0: the conversation's start): each file
    /// Claude changed since is as it was then, unless the user changed it
    /// after Claude (then it is kept, and named). The checkpoints after
    /// it go.
    pub fn back_to(&mut self, k: usize) -> Result<Vec<String>, String> {
        let mut was: BTreeMap<String, Text> = BTreeMap::new();
        for (_, files) in self.checkpoints.iter().skip(k).rev() {
            for (rel, text) in files {
                was.insert(rel.clone(), text.clone());
            }
        }
        let mut kept = Vec::new();
        for (rel, text) in &was {
            let now = self.read(rel);
            if self.written.get(rel) != Some(&now) {
                kept.push(rel.clone());
                continue;
            }
            self.write(rel, text)?;
            self.written.insert(rel.clone(), text.clone());
        }
        self.checkpoints.truncate(k);
        self.undone.clear();
        Ok(kept)
    }
}

/// Lines without their ends.
fn split(text: &str) -> Vec<String> {
    text.lines().map(str::to_owned).collect()
}

/// Lines joined, ending as `like` ended (with a line end or not).
fn join(lines: &[String], like: &str) -> String {
    let mut t = lines.join("\n");
    if !lines.is_empty() && (like.ends_with('\n') || like.is_empty()) {
        t.push('\n');
    }
    t
}

/// The hunks from `a` to `b`: runs of lines removed and added, with where
/// they are (1-based) in each.
pub fn hunks(a: &str, b: &str) -> Vec<ClaudeHunk> {
    let (x, y) = (split(a), split(b));
    // the same start and end are left out
    let mut pre = 0;
    while pre < x.len() && pre < y.len() && x[pre] == y[pre] {
        pre += 1;
    }
    let mut post = 0;
    while post < x.len() - pre
        && post < y.len() - pre
        && x[x.len() - 1 - post] == y[y.len() - 1 - post]
    {
        post += 1;
    }
    let (xm, ym) = (&x[pre..x.len() - post], &y[pre..y.len() - post]);
    if xm.is_empty() && ym.is_empty() {
        return Vec::new();
    }
    // a middle too big to compare line by line is one hunk
    if xm.len().saturating_mul(ym.len()) > 4_000_000 {
        return vec![ClaudeHunk {
            old_start: pre + 1,
            old: xm.to_vec(),
            new_start: pre + 1,
            new: ym.to_vec(),
        }];
    }
    // the longest common subsequence's table, from the end
    let (n, m) = (xm.len(), ym.len());
    let mut l = vec![0u32; (n + 1) * (m + 1)];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            l[i * (m + 1) + j] = if xm[i] == ym[j] {
                l[(i + 1) * (m + 1) + j + 1] + 1
            } else {
                l[(i + 1) * (m + 1) + j].max(l[i * (m + 1) + j + 1])
            };
        }
    }
    let mut out: Vec<ClaudeHunk> = Vec::new();
    let (mut i, mut j) = (0, 0);
    let mut open: Option<ClaudeHunk> = None;
    while i < n || j < m {
        if i < n && j < m && xm[i] == ym[j] {
            if let Some(h) = open.take() {
                out.push(h);
            }
            i += 1;
            j += 1;
            continue;
        }
        let h = open.get_or_insert_with(|| ClaudeHunk {
            old_start: pre + i + 1,
            old: Vec::new(),
            new_start: pre + j + 1,
            new: Vec::new(),
        });
        if j < m && (i == n || l[i * (m + 1) + j + 1] >= l[(i + 1) * (m + 1) + j]) {
            h.new.push(ym[j].clone());
            j += 1;
        } else {
            h.old.push(xm[i].clone());
            i += 1;
        }
    }
    if let Some(h) = open {
        out.push(h);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hunks_are_the_runs_that_differ() {
        let h = hunks("a\nb\nc\nd\n", "a\nB\nc\nd\ne\n");
        assert_eq!(h.len(), 2);
        assert_eq!(
            (h[0].old_start, &h[0].old[..], h[0].new_start, &h[0].new[..]),
            (2, &["b".to_string()][..], 2, &["B".to_string()][..])
        );
        assert_eq!(
            (
                h[1].old_start,
                h[1].old.len(),
                h[1].new_start,
                &h[1].new[..]
            ),
            (5, 0, 5, &["e".to_string()][..])
        );
        assert!(hunks("same\n", "same\n").is_empty());
    }

    #[test]
    fn hunks_undo_and_redo_and_checkpoints_go_back() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("a.lua"), "one\ntwo\nthree\n").unwrap();
        let mut c = Changes::new(root);
        // message 1: Claude changes a.lua and makes b.lua
        c.checkpoint("first");
        c.before("a.lua");
        std::fs::write(root.join("a.lua"), "one\nTWO\nthree\nfour\n").unwrap();
        c.after("a.lua");
        c.before("b.lua");
        std::fs::write(root.join("b.lua"), "new\n").unwrap();
        c.after("b.lua");
        let v = c.view();
        assert_eq!(v.files.len(), 2);
        let a = v.files.iter().find(|f| f.path == "a.lua").unwrap();
        assert_eq!((a.added, a.removed, a.hunks.len()), (2, 1, 2));
        assert!(v.files.iter().any(|f| f.path == "b.lua" && f.created));
        // the first hunk undone, then redone
        c.undo("a.lua", &a.hunks[0]).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("a.lua")).unwrap(),
            "one\ntwo\nthree\nfour\n"
        );
        assert_eq!(c.view().undone.len(), 1);
        c.redo(0).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("a.lua")).unwrap(),
            "one\nTWO\nthree\nfour\n"
        );
        // message 2: Claude changes a.lua again; the user edits b.lua
        c.checkpoint("second");
        c.before("a.lua");
        std::fs::write(root.join("a.lua"), "zero\none\nTWO\nthree\nfour\n").unwrap();
        c.after("a.lua");
        std::fs::write(root.join("b.lua"), "new\nmine\n").unwrap();
        // back before the second: a.lua as after the first
        assert!(c.back_to(1).unwrap().is_empty());
        assert_eq!(
            std::fs::read_to_string(root.join("a.lua")).unwrap(),
            "one\nTWO\nthree\nfour\n"
        );
        // back to the start: a.lua as it was; b.lua is the user's now, kept
        assert_eq!(c.back_to(0).unwrap(), vec!["b.lua".to_string()]);
        assert_eq!(
            std::fs::read_to_string(root.join("a.lua")).unwrap(),
            "one\ntwo\nthree\n"
        );
        assert!(root.join("b.lua").exists());
        assert!(c.view().checkpoints.is_empty());
    }
}
