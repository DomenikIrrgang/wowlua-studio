//! Git in Studio (M11): the git page, and what the explorer, the editor
//! and the status bar show of the repository. The state keeps the last
//! answers from the repository and asks again when files change.

pub mod bar;
pub mod editor;
pub mod model;
pub mod page;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use model::{BlameLine, Branch, Change, Commit, CommitFile, Diff, DiffOf, RemoteError, Status};

use crate::system::{CommitStop, Committed, GitEvent, GitRequest, System};

/// What the page shows in its main view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selected {
    /// A changed file's diff, unstaged or staged.
    File(String, DiffOf),
    /// A commit of the history.
    Commit(String),
}

/// A note over the main view: what the last commit, push or change did.
#[derive(Clone, Debug, PartialEq)]
pub enum Callout {
    Committed(Committed),
    Stopped(CommitStop),
    Remote(RemoteError),
    Done(String),
    Failed(String),
}

/// A dialog of the page.
#[derive(Clone, Debug, PartialEq)]
pub enum Dialog {
    /// Switching to a branch with changes: carry or stash them (S068).
    Switch {
        target: String,
        /// The files git would overwrite (none yet: asked).
        blocking: Option<Vec<String>>,
        carry: model::Carry,
    },
    NewBranch {
        from: Option<String>,
        name: String,
        switch: bool,
        bad: Option<String>,
    },
    /// Changes thrown away, asked first (S068).
    Discard { what: String, request: GitRequest },
}

/// The repository as Studio last read it, and what is asked.
pub struct GitState {
    /// The workspace's folder the questions go to.
    root: Option<PathBuf>,
    pub status: Option<Status>,
    /// The workspace's folder within the repository (`""` at its root).
    prefix: Option<String>,
    pub error: Option<String>,
    pub branches: Vec<Branch>,
    pub log: Vec<Commit>,
    pub selected: Option<Selected>,
    diffs: HashMap<(String, DiffOf), Diff>,
    commit_files: HashMap<String, Vec<CommitFile>>,
    pub commit_file: usize,
    pub message: String,
    pub format_and_check: bool,
    pub side_by_side: bool,
    pub callout: Option<Callout>,
    pub dialog: Option<Dialog>,
    /// Something running: its words (`committing…`).
    pub busy: Option<String>,
    /// A push to follow the pull under way (the changes panel's sync).
    pub then_push: bool,
    /// The status bar's branch list open, and what is typed to find one.
    pub bar_open: bool,
    pub bar_query: String,
    /// What the screenshot harness asked to show once the repository is
    /// read (`commit`, `new`, `switch:<branch>`, `discard`).
    pub harness: Option<String>,
    asked: HashSet<String>,
    stale: bool,
    /// When Studio last fetched (egui's clock, seconds).
    last_fetch: Option<f64>,
    /// Who last changed each line of a file (path relative to the root),
    /// for the editor's blame (S070).
    pub blame: HashMap<String, (u64, Vec<BlameLine>)>,
    /// A file's text at HEAD, for the editor's gutter (S070).
    pub head: HashMap<String, Option<String>>,
    /// The gutter's marks of a file, by the document's version.
    pub marks: HashMap<String, (u64, Vec<(usize, crate::editor::view::GitMark)>)>,
    /// When a file's blame was last asked for (asked again at most every
    /// 1.5 s while it is edited).
    pub blame_at: HashMap<String, f64>,
    /// A gutter mark's hunk shown: the file, its line, where.
    pub mark_popup: Option<(String, usize, egui::Pos2)>,
    head_asked: HashSet<String>,
}

impl Default for GitState {
    fn default() -> Self {
        GitState {
            root: None,
            status: None,
            prefix: None,
            error: None,
            branches: Vec::new(),
            log: Vec::new(),
            selected: None,
            diffs: HashMap::new(),
            commit_files: HashMap::new(),
            commit_file: 0,
            message: String::new(),
            format_and_check: true,
            side_by_side: true,
            callout: None,
            dialog: None,
            busy: None,
            then_push: false,
            bar_open: false,
            bar_query: String::new(),
            harness: None,
            asked: HashSet::new(),
            stale: true,
            last_fetch: None,
            blame: HashMap::new(),
            head: HashMap::new(),
            marks: HashMap::new(),
            blame_at: HashMap::new(),
            mark_popup: None,
            head_asked: HashSet::new(),
        }
    }
}

/// How many commits the history shows.
const HISTORY: usize = 200;

impl GitState {
    /// A workspace opened (or closed): everything read again.
    pub fn opened(&mut self, root: Option<&Path>) {
        *self = GitState {
            root: root.map(Path::to_path_buf),
            format_and_check: self.format_and_check,
            side_by_side: self.side_by_side,
            harness: self.harness.take(),
            ..GitState::default()
        };
    }

    /// Files changed on disk or in `.git`: the status is read again; HEAD's
    /// texts too when a commit or switch moved HEAD.
    pub fn files_changed(&mut self, paths: &[PathBuf]) {
        self.stale = true;
        if paths
            .iter()
            .any(|p| p.components().any(|c| c.as_os_str() == ".git"))
        {
            self.head.clear();
            self.head_asked.clear();
            self.marks.clear();
            self.asked.remove("log");
            self.asked.remove("branches");
        }
    }

    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    fn ask(&mut self, system: &dyn System, key: &str, request: GitRequest) {
        let Some(root) = &self.root else {
            return;
        };
        if self.asked.insert(key.to_owned()) {
            system.git(root, request);
        }
    }

    /// Something that changes the repository, with the page busy until it
    /// answers.
    pub fn change(&mut self, system: &dyn System, busy: &str, request: GitRequest) {
        if let Some(root) = &self.root {
            self.busy = Some(busy.to_owned());
            self.callout = None;
            system.git(root, request);
        }
    }

    /// Each frame: the status read when stale, Studio's own fetch every
    /// `fetch_minutes` (0: never; S070).
    pub fn tick(&mut self, system: &dyn System, fetch_minutes: u32, now: f64) {
        if self.root.is_none() {
            return;
        }
        if self.stale && !self.asked.contains("status") {
            self.stale = false;
            self.ask(system, "status", GitRequest::Status);
        }
        let due = match self.last_fetch {
            None => true,
            Some(at) => fetch_minutes > 0 && now - at >= f64::from(fetch_minutes) * 60.0,
        };
        if due && self.status.as_ref().is_some_and(|s| s.upstream.is_some()) {
            self.last_fetch = Some(now);
            if let Some(root) = &self.root {
                system.git(root, GitRequest::Fetch { quiet: true });
            }
        }
    }

    /// The page shown: the branches and history read, the selection's
    /// diff asked for.
    pub fn page_shown(&mut self, system: &dyn System) {
        if self.status.is_none() {
            return;
        }
        self.ask(system, "branches", GitRequest::Branches);
        self.ask(
            system,
            "log",
            GitRequest::Log {
                rev: None,
                count: HISTORY,
            },
        );
        match self.selected.clone() {
            Some(Selected::File(path, of)) => {
                if !self.diffs.contains_key(&(path.clone(), of.clone())) {
                    let key = format!("diff {path} {of:?}");
                    self.ask(system, &key, GitRequest::Diff { path, of });
                }
            }
            Some(Selected::Commit(hash)) => {
                if !self.commit_files.contains_key(&hash) {
                    self.ask(
                        system,
                        &format!("files {hash}"),
                        GitRequest::CommitFiles(hash.clone()),
                    );
                }
                if let Some(path) = self
                    .commit_files
                    .get(&hash)
                    .and_then(|f| f.get(self.commit_file))
                    .map(|f| f.path.clone())
                {
                    let of = DiffOf::Commit(hash);
                    if !self.diffs.contains_key(&(path.clone(), of.clone())) {
                        let key = format!("diff {path} {of:?}");
                        self.ask(system, &key, GitRequest::Diff { path, of });
                    }
                }
            }
            None => {}
        }
    }

    /// A file's diff asked for when not known yet.
    pub fn want_diff(&mut self, system: &dyn System, path: &str, of: DiffOf) {
        if !self.diffs.contains_key(&(path.to_owned(), of.clone())) {
            let key = format!("diff {path} {of:?}");
            self.ask(
                system,
                &key,
                GitRequest::Diff {
                    path: path.to_owned(),
                    of,
                },
            );
        }
    }

    pub fn diff(&self, path: &str, of: &DiffOf) -> Option<&Diff> {
        self.diffs.get(&(path.to_owned(), of.clone()))
    }

    pub fn commit_files(&self, hash: &str) -> Option<&[CommitFile]> {
        self.commit_files.get(hash).map(Vec::as_slice)
    }

    /// A file's text at HEAD for the gutter; asked once.
    pub fn head_text(&mut self, system: &dyn System, path: &str) -> Option<&str> {
        if !self.head.contains_key(path)
            && self.head_asked.insert(path.to_owned())
            && let Some(root) = &self.root
        {
            system.git(root, GitRequest::HeadText(path.to_owned()));
        }
        self.head.get(path).and_then(|t| t.as_deref())
    }

    /// Blame of a file as the editor has it (`version`: the document's),
    /// asked when the version moved on.
    pub fn blame_of(
        &mut self,
        system: &dyn System,
        path: &str,
        version: u64,
        contents: impl FnOnce() -> String,
    ) -> Option<&[BlameLine]> {
        let key = format!("blame {path} {version}");
        let current = self.blame.get(path).is_some_and(|(v, _)| *v == version);
        if !current && !self.asked.contains(&key) {
            self.asked
                .retain(|k| !k.starts_with(&format!("blame {path} ")));
            self.asked.insert(key);
            if let Some(root) = &self.root {
                system.git(
                    root,
                    GitRequest::Blame {
                        path: path.to_owned(),
                        contents: contents(),
                    },
                );
            }
            // (the version asked for, the lines as last known)
            if let Some(entry) = self.blame.get_mut(path) {
                entry.0 = version;
            } else {
                self.blame.insert(path.to_owned(), (version, Vec::new()));
            }
        }
        self.blame.get(path).map(|(_, l)| l.as_slice())
    }

    /// A file's path relative to the repository's root, with `/`: the
    /// workspace's folder within the repository, then the path within it.
    pub fn relative(&self, path: &Path) -> Option<String> {
        let ws = self.root.as_deref()?;
        let within = path.strip_prefix(ws).ok()?;
        let within = within.to_string_lossy().replace('\\', "/");
        let prefix = self.prefix.as_deref()?;
        Some(match (prefix.is_empty(), within.is_empty()) {
            (true, _) => within,
            (false, true) => prefix.to_owned(),
            (false, false) => format!("{prefix}/{within}"),
        })
    }

    /// What the explorer shows for a file: its change.
    pub fn change_of(&self, path: &Path) -> Option<Change> {
        let rel = self.relative(path)?;
        self.status.as_ref()?.file(&rel).map(model::File::change)
    }

    /// Whether a folder holds changed files (the explorer's dot).
    pub fn folder_changed(&self, dir: &Path) -> bool {
        let Some(rel) = self.relative(dir) else {
            return false;
        };
        let prefix = if rel.is_empty() {
            String::new()
        } else {
            format!("{rel}/")
        };
        self.status
            .as_ref()
            .is_some_and(|s| s.files.iter().any(|f| f.path.starts_with(&prefix)))
    }

    /// An answer from the repository.
    pub fn event(&mut self, root: &Path, event: GitEvent, system: &dyn System) {
        if self.root.as_deref() != Some(root) {
            return;
        }
        match event {
            GitEvent::Status(result) => {
                self.asked.remove("status");
                match result {
                    Ok(status) => {
                        if self.status.as_ref() != Some(&status) {
                            // (the diffs of the working tree and index are old)
                            self.diffs
                                .retain(|(_, of), _| matches!(of, DiffOf::Commit(_)));
                            self.asked.retain(|k| !k.starts_with("diff "));
                            let moved = self
                                .status
                                .as_ref()
                                .is_none_or(|s| s.head != status.head || s.branch != status.branch);
                            if moved {
                                self.asked.remove("log");
                                self.asked.remove("branches");
                                self.head.clear();
                                self.head_asked.clear();
                                self.marks.clear();
                            }
                            // (a selected file that no longer changed: none)
                            if let Some(Selected::File(path, _)) = &self.selected
                                && status.file(path).is_none()
                            {
                                self.selected = None;
                            }
                            if self.selected.is_none()
                                && let Some(f) = status.files.first()
                            {
                                let of = if f.unstaged.is_some() {
                                    DiffOf::Unstaged
                                } else {
                                    DiffOf::Staged
                                };
                                self.selected = Some(Selected::File(f.path.clone(), of));
                            }
                            self.prefix = self.root.as_deref().and_then(|ws| {
                                let ws = std::fs::canonicalize(ws).ok()?;
                                let repo = std::fs::canonicalize(&status.root).ok()?;
                                let rel = ws.strip_prefix(repo).ok()?;
                                Some(rel.to_string_lossy().replace('\\', "/"))
                            });
                            self.status = Some(status);
                        }
                        self.error = None;
                    }
                    Err(e) => {
                        self.status = None;
                        self.error = Some(e);
                    }
                }
            }
            GitEvent::Branches(result) => {
                if let Ok(b) = result {
                    self.branches = b;
                }
            }
            GitEvent::Log { result, .. } => {
                if let Ok(l) = result {
                    self.log = l;
                }
            }
            GitEvent::Diff { path, of, result } => {
                self.asked.remove(&format!("diff {path} {of:?}"));
                if let Ok(d) = result {
                    self.diffs.insert((path, of), d);
                }
            }
            GitEvent::CommitFiles { hash, result } => {
                if let Ok(f) = result {
                    self.commit_files.insert(hash, f);
                }
            }
            GitEvent::Done { what, result } => {
                self.busy = None;
                self.stale = true;
                match result {
                    Ok(said) if !said.is_empty() => self.callout = Some(Callout::Done(said)),
                    Ok(_) => {}
                    Err(e) => self.callout = Some(Callout::Failed(format!("{what}: {e}"))),
                }
                if matches!(what.as_str(), "switch" | "new branch" | "revert" | "abort") {
                    self.asked.remove("log");
                    self.asked.remove("branches");
                    self.selected = None;
                }
            }
            GitEvent::Committed(result) => {
                self.busy = None;
                self.stale = true;
                self.asked.remove("log");
                self.asked.remove("branches");
                match result {
                    Ok(c) => {
                        self.message.clear();
                        self.callout = Some(Callout::Committed(c));
                    }
                    Err(stop) => self.callout = Some(Callout::Stopped(stop)),
                }
            }
            GitEvent::Remote {
                what,
                quiet,
                result,
            } => {
                // (the sync: the push after a pull that went through)
                if what == "pull" && std::mem::take(&mut self.then_push) && result.is_ok() {
                    self.stale = true;
                    if let Some(root) = &self.root {
                        self.busy = Some("pushing…".into());
                        system.git(root, GitRequest::Push);
                    }
                    return;
                }
                if !quiet {
                    self.busy = None;
                    self.callout = Some(match result {
                        Ok(said) => {
                            Callout::Done(if said.is_empty() { "done".into() } else { said })
                        }
                        Err(e) => Callout::Remote(e),
                    });
                }
                self.stale = true;
                self.asked.remove("log");
                self.asked.remove("branches");
            }
            GitEvent::Blocking { target, files } => {
                if let Some(Dialog::Switch {
                    target: t,
                    blocking,
                    carry,
                }) = &mut self.dialog
                    && *t == target
                {
                    let files = files.unwrap_or_default();
                    if !files.is_empty() {
                        *carry = model::Carry::Stash;
                    }
                    *blocking = Some(files);
                }
            }
            GitEvent::BranchName { name, bad } => {
                if let Some(Dialog::NewBranch {
                    name: n, bad: b, ..
                }) = &mut self.dialog
                    && n.trim() == name.trim()
                {
                    *b = bad;
                }
            }
            GitEvent::Blame { path, result } => {
                if let Ok(lines) = result
                    && let Some(entry) = self.blame.get_mut(&path)
                {
                    entry.1 = lines;
                }
            }
            GitEvent::HeadText { path, text } => {
                self.marks.remove(&path);
                self.head.insert(path, text);
            }
        }
        self.harness_step(system);
    }

    /// The screenshot harness's step, once what it needs is read.
    fn harness_step(&mut self, system: &dyn System) {
        let Some(what) = self.harness.clone() else {
            return;
        };
        if self.status.is_none() {
            return;
        }
        match what.as_str() {
            "branches" => {
                self.bar_open = true;
                self.ask(system, "branches", GitRequest::Branches);
            }
            "commit" => {
                let Some(c) = self.log.get(1).or(self.log.first()) else {
                    return;
                };
                self.selected = Some(Selected::Commit(c.hash.clone()));
            }
            "new" => {
                self.dialog = Some(Dialog::NewBranch {
                    from: None,
                    name: String::new(),
                    switch: true,
                    bad: None,
                });
            }
            "discard" => {
                let Some(f) = self.status.as_ref().and_then(|s| s.files.first()) else {
                    return;
                };
                self.dialog = Some(Dialog::Discard {
                    what: format!("the changes to {}", f.path),
                    request: GitRequest::Discard(vec![f.path.clone()]),
                });
            }
            other => {
                if let Some(target) = other.strip_prefix("switch:") {
                    page::start_switch(self, system, target.to_owned());
                }
            }
        }
        self.harness = None;
    }

    /// The commits ahead and behind, for the status bar.
    pub fn branch_label(&self) -> Option<(String, u32, u32)> {
        let s = self.status.as_ref()?;
        Some((s.branch_label(), s.ahead, s.behind))
    }
}
