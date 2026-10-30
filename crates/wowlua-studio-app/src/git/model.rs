//! What Studio knows of a Git repository (M11): its status, branches,
//! history, diffs and blame, as the native side reads them with the `git`
//! program (S027). Plain data: the page, the explorer, the editor and the
//! status bar draw from it.

use std::path::PathBuf;

/// How a file differs, in the index or the working tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Change {
    Modified,
    Added,
    Deleted,
    Renamed,
    /// New and not tracked (only in the working tree).
    Untracked,
    /// Both sides of a merge changed it: resolve, then stage.
    Conflicted,
}

impl Change {
    /// The explorer's and the lists' letter.
    pub fn letter(self) -> &'static str {
        match self {
            Change::Modified => "M",
            Change::Added => "A",
            Change::Deleted => "D",
            Change::Renamed => "R",
            Change::Untracked => "U",
            Change::Conflicted => "!",
        }
    }
}

/// A changed file: what is staged and what is not. `path` is relative to
/// the repository's root, with `/`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    pub path: String,
    /// What it was before a rename.
    pub from: Option<String>,
    pub staged: Option<Change>,
    pub unstaged: Option<Change>,
}

impl File {
    /// What the file shows as: a conflict first, then the working tree's
    /// change, then the index's.
    pub fn change(&self) -> Change {
        match (self.staged, self.unstaged) {
            (Some(Change::Conflicted), _) | (_, Some(Change::Conflicted)) => Change::Conflicted,
            (_, Some(c)) => c,
            (Some(c), None) => c,
            (None, None) => Change::Modified,
        }
    }
}

/// A stash Studio made when switching away from a branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stash {
    /// `stash@{0}`.
    pub name: String,
    /// The branch it was made on.
    pub branch: String,
    pub message: String,
}

/// The repository's state: the branch, how far it is from its upstream,
/// the changed files, a merge or rebase under way.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// The repository's root folder.
    pub root: PathBuf,
    /// The branch checked out; none when a commit is (detached).
    pub branch: Option<String>,
    /// The commit checked out, short; empty in a repository without one.
    pub head: String,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub files: Vec<File>,
    /// The branch or commit being merged in (`main`), during a merge.
    pub merging: Option<String>,
    pub rebasing: bool,
    pub stashes: Vec<Stash>,
}

impl Status {
    /// The status bar's words for the branch: `feat/x`, `feat/x · merging
    /// main`, `8b1e0d4 · no branch`.
    pub fn branch_label(&self) -> String {
        let name = match &self.branch {
            Some(b) => b.clone(),
            None if self.head.is_empty() => "no commits".to_owned(),
            None => format!("{} · no branch", self.head),
        };
        match (&self.merging, self.rebasing) {
            (Some(m), _) => format!("{name} · merging {m}"),
            (None, true) => format!("{name} · rebasing"),
            _ => name,
        }
    }

    /// The file at a path relative to the root.
    pub fn file(&self, path: &str) -> Option<&File> {
        self.files.iter().find(|f| f.path == path)
    }

    pub fn conflicts(&self) -> impl Iterator<Item = &File> {
        self.files
            .iter()
            .filter(|f| f.change() == Change::Conflicted)
    }
}

/// A branch, local or a remote's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Branch {
    /// `main`, or `origin/main` for a remote's.
    pub name: String,
    pub remote: bool,
    pub current: bool,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// When its last commit was made (Unix seconds) and by whom.
    pub time: i64,
    pub author: String,
}

/// A commit of the history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    pub hash: String,
    pub short: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    /// Unix seconds.
    pub time: i64,
    /// The branches and tags at it (`HEAD -> main`, `origin/main`, `tag:
    /// v1.0`), as git names them.
    pub refs: Vec<String>,
    pub subject: String,
    pub body: String,
}

/// A line of a hunk: unchanged, added or removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub kind: LineKind,
    pub text: String,
    /// The line's number before and after (none on the side it isn't on).
    pub old: Option<u32>,
    pub new: Option<u32>,
}

/// A hunk of a diff: where it is on each side, its lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    /// What git puts after the `@@` (the function it is in).
    pub section: String,
    pub lines: Vec<Line>,
    /// The old or new side ends without a newline after this hunk's
    /// last line (`\ No newline at end of file`).
    pub old_no_newline: bool,
    pub new_no_newline: bool,
}

impl Hunk {
    pub fn added(&self) -> usize {
        self.lines
            .iter()
            .filter(|l| l.kind == LineKind::Added)
            .count()
    }

    pub fn removed(&self) -> usize {
        self.lines
            .iter()
            .filter(|l| l.kind == LineKind::Removed)
            .count()
    }
}

/// Which diff of a file: the working tree against the index (what isn't
/// staged), the index against HEAD (what is), or a commit against its
/// first parent.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum DiffOf {
    Unstaged,
    Staged,
    Commit(String),
}

/// A file's diff.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diff {
    pub path: String,
    pub hunks: Vec<Hunk>,
    pub binary: bool,
    /// A new file (all lines added) or a deleted one.
    pub new_file: bool,
    pub deleted: bool,
}

/// A commit's files with their counts (`+14 −3`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitFile {
    pub path: String,
    pub added: u32,
    pub removed: u32,
}

/// Who last changed a line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlameLine {
    /// Empty for a line not committed yet.
    pub hash: String,
    pub author: String,
    pub time: i64,
    pub subject: String,
}

/// What a branch switch does with uncommitted changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Carry {
    /// They come along (git refuses when it would overwrite them).
    Carry,
    /// Stashed for the branch, put back when switching back to it.
    Stash,
}

/// How a hunk's patch is applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Apply {
    /// From the working tree's diff into the index.
    Stage,
    /// From the index's diff back out of it.
    Unstage,
    /// From the working tree's diff, undone in the working tree.
    Discard,
}

/// The side a conflict takes, for a whole file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Mine,
    Theirs,
}

/// Why a push, pull or fetch failed, as far as Studio can tell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteError {
    /// The remote asked for credentials Studio doesn't have.
    Credentials(String),
    /// The remote has commits this branch doesn't (a push).
    Rejected(String),
    Other(String),
}

impl std::fmt::Display for RemoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RemoteError::Credentials(s) | RemoteError::Rejected(s) | RemoteError::Other(s) => {
                f.write_str(s)
            }
        }
    }
}
