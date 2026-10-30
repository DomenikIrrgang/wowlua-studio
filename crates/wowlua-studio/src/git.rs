//! Git for Studio (M11) through the `git` program (S027): status,
//! branches, history, diffs, staging by file and hunk, commits, switching
//! with the changes carried or stashed, pulling, pushing and fetching
//! (with the GitHub sign-in first, the user's own git setup otherwise),
//! merge conflicts and blame. Each function runs git in the repository and
//! answers with the model the app draws.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use wowlua_studio_app::git::model::{
    Apply, BlameLine, Branch, Carry, Change, Commit, CommitFile, Diff, DiffOf, File, Hunk, Line,
    LineKind, RemoteError, Side, Stash, Status,
};
use wowlua_studio_app::system::{CommitStop, Committed, GitEvent, GitRequest};

/// The stash message Studio leaves when switching away from a branch.
const STASH_TAG: &str = "wowlua-studio:";

/// The environment variable a push's credential helper reads the GitHub
/// token from (never on the command line).
pub(crate) const TOKEN_ENV: &str = "WOWLUA_STUDIO_GIT_TOKEN";

/// What git printed.
pub(crate) struct Out {
    pub(crate) ok: bool,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

/// git in `root`, never asking on a terminal.
pub(crate) fn command(root: &Path) -> Command {
    let mut c = Command::new("git");
    c.arg("-C")
        .arg(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C");
    c
}

pub(crate) fn run_with(mut c: Command, input: Option<&str>) -> Result<Out, String> {
    c.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let mut child = c.spawn().map_err(|e| format!("cannot run git: {e}"))?;
    if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| format!("cannot write to git: {e}"))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("git failed: {e}"))?;
    Ok(Out {
        ok: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
    })
}

fn run(root: &Path, args: &[&str]) -> Result<Out, String> {
    let mut c = command(root);
    c.args(args);
    run_with(c, None)
}

/// git's output when it succeeds, its error otherwise.
pub(crate) fn ok(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = run(root, args)?;
    if out.ok {
        Ok(out.stdout)
    } else {
        Err(failure(&out, args))
    }
}

fn failure(out: &Out, args: &[&str]) -> String {
    let said = if out.stderr.is_empty() {
        out.stdout.trim()
    } else {
        &out.stderr
    };
    format!("git {} failed: {said}", args.first().copied().unwrap_or(""))
}

// ------------------------------------------------------------- status --

/// The repository's status (`git status --porcelain=v2`), with a merge or
/// rebase under way and Studio's stashes.
pub fn status(root: &Path) -> Result<Status, String> {
    let text = ok(
        root,
        &[
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=all",
        ],
    )?;
    let mut status = parse_status(&text);
    status.root = root.to_path_buf();
    let git_path = |name: &str| -> Option<PathBuf> {
        let p = ok(root, &["rev-parse", "--git-path", name]).ok()?;
        let p = PathBuf::from(p.trim());
        Some(if p.is_absolute() { p } else { root.join(p) })
    };
    if git_path("MERGE_HEAD").is_some_and(|p| p.exists()) {
        let message = git_path("MERGE_MSG")
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_default();
        status.merging = Some(merged_name(&message));
    }
    status.rebasing = ["rebase-merge", "rebase-apply"]
        .iter()
        .any(|d| git_path(d).is_some_and(|p| p.exists()));
    status.stashes = stashes(root).unwrap_or_default();
    Ok(status)
}

/// What a merge message says is merged: `main` of "Merge branch 'main'
/// into x", the commit otherwise.
fn merged_name(message: &str) -> String {
    let first = message.lines().next().unwrap_or("");
    first
        .split('\'')
        .nth(1)
        .map(str::to_owned)
        .unwrap_or_else(|| first.trim_start_matches("Merge ").trim().to_owned())
}

/// `git status --porcelain=v2 --branch -z`'s records.
fn parse_status(text: &str) -> Status {
    let mut status = Status::default();
    let mut fields = text.split('\0').peekable();
    while let Some(record) = fields.next() {
        if let Some(header) = record.strip_prefix("# ") {
            let (key, value) = header.split_once(' ').unwrap_or((header, ""));
            match key {
                "branch.oid" if value != "(initial)" => {
                    status.head = value.chars().take(7).collect();
                }
                "branch.head" if value != "(detached)" => status.branch = Some(value.to_owned()),
                "branch.upstream" => status.upstream = Some(value.to_owned()),
                "branch.ab" => {
                    for part in value.split(' ') {
                        if let Some(n) = part.strip_prefix('+') {
                            status.ahead = n.parse().unwrap_or(0);
                        } else if let Some(n) = part.strip_prefix('-') {
                            status.behind = n.parse().unwrap_or(0);
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        let mut parts = record.splitn(2, ' ');
        let kind = parts.next().unwrap_or("");
        let rest = parts.next().unwrap_or("");
        match kind {
            "1" | "2" => {
                // XY sub mH mI mW hH hI [score] path
                let n = if kind == "1" { 7 } else { 8 };
                let cols: Vec<&str> = rest.splitn(n + 1, ' ').collect();
                let xy = cols.first().copied().unwrap_or("..");
                let path = cols.get(n).copied().unwrap_or("").to_owned();
                let from = (kind == "2").then(|| fields.next().unwrap_or("").to_owned());
                let mut chars = xy.chars();
                let x = chars.next().unwrap_or('.');
                let y = chars.next().unwrap_or('.');
                status.files.push(File {
                    path,
                    from,
                    staged: change_of(x),
                    unstaged: change_of(y),
                });
            }
            "u" => {
                // XY sub m1 m2 m3 mW h1 h2 h3 path
                let cols: Vec<&str> = rest.splitn(10, ' ').collect();
                status.files.push(File {
                    path: cols.get(9).copied().unwrap_or("").to_owned(),
                    from: None,
                    staged: Some(Change::Conflicted),
                    unstaged: Some(Change::Conflicted),
                });
            }
            "?" => status.files.push(File {
                path: rest.to_owned(),
                from: None,
                staged: None,
                unstaged: Some(Change::Untracked),
            }),
            _ => {}
        }
    }
    status
}

fn change_of(c: char) -> Option<Change> {
    match c {
        'M' | 'T' => Some(Change::Modified),
        'A' => Some(Change::Added),
        'D' => Some(Change::Deleted),
        'R' | 'C' => Some(Change::Renamed),
        'U' => Some(Change::Conflicted),
        _ => None,
    }
}

/// Studio's stashes, each with the branch it was made on.
fn stashes(root: &Path) -> Result<Vec<Stash>, String> {
    let text = ok(root, &["stash", "list", "--format=%gd%x1f%s"])?;
    Ok(text
        .lines()
        .filter_map(|l| {
            let (name, message) = l.split_once('\x1f')?;
            let branch = message
                .split(STASH_TAG)
                .nth(1)?
                .split_whitespace()
                .next()?
                .to_owned();
            Some(Stash {
                name: name.to_owned(),
                branch,
                message: message.to_owned(),
            })
        })
        .collect())
}

// ----------------------------------------------------------- branches --

/// The local branches, then the remotes' (without their `HEAD`).
pub fn branches(root: &Path) -> Result<Vec<Branch>, String> {
    let format = "%(refname)%1f%(refname:short)%1f%(HEAD)%1f%(upstream:short)%1f%(upstream:track,nobracket)%1f%(committerdate:unix)%1f%(authorname)";
    let text = ok(
        root,
        &[
            "for-each-ref",
            &format!("--format={format}"),
            "--sort=-committerdate",
            "refs/heads",
            "refs/remotes",
        ],
    )?;
    let mut out: Vec<Branch> = text.lines().filter_map(parse_branch).collect();
    out.sort_by_key(|b| b.remote);
    Ok(out)
}

fn parse_branch(line: &str) -> Option<Branch> {
    let f: Vec<&str> = line.split('\x1f').collect();
    let (full, name) = (*f.first()?, *f.get(1)?);
    if full.ends_with("/HEAD") {
        return None;
    }
    let track = f.get(4).copied().unwrap_or("");
    let count = |word: &str| -> u32 {
        track
            .split(", ")
            .find_map(|p| p.strip_prefix(word))
            .and_then(|n| n.trim().parse().ok())
            .unwrap_or(0)
    };
    Some(Branch {
        name: name.to_owned(),
        remote: full.starts_with("refs/remotes/"),
        current: f.get(2) == Some(&"*"),
        upstream: f.get(3).filter(|u| !u.is_empty()).map(|u| (*u).to_owned()),
        ahead: count("ahead "),
        behind: count("behind "),
        time: f.get(5).and_then(|t| t.parse().ok()).unwrap_or(0),
        author: f.get(6).copied().unwrap_or("").to_owned(),
    })
}

// ------------------------------------------------------------ history --

/// The last `count` commits of `rev` (HEAD when none).
pub fn log(root: &Path, rev: Option<&str>, count: usize) -> Result<Vec<Commit>, String> {
    let n = format!("-n{count}");
    let mut args = vec![
        "log",
        n.as_str(),
        "--format=%H%x1f%h%x1f%P%x1f%an%x1f%ae%x1f%at%x1f%D%x1f%s%x1f%b%x1e",
    ];
    if let Some(r) = rev {
        args.push(r);
    }
    let out = run(root, &args)?;
    if !out.ok {
        // (a repository without commits has no history yet)
        if out.stderr.contains("does not have any commits") {
            return Ok(Vec::new());
        }
        return Err(failure(&out, &args));
    }
    Ok(parse_log(&out.stdout))
}

fn parse_log(text: &str) -> Vec<Commit> {
    text.split('\x1e')
        .filter_map(|record| {
            let record = record.trim_start_matches('\n');
            let f: Vec<&str> = record.split('\x1f').collect();
            if f.len() < 9 {
                return None;
            }
            Some(Commit {
                hash: f[0].to_owned(),
                short: f[1].to_owned(),
                parents: f[2].split_whitespace().map(str::to_owned).collect(),
                author: f[3].to_owned(),
                email: f[4].to_owned(),
                time: f[5].parse().unwrap_or(0),
                refs: f[6]
                    .split(", ")
                    .filter(|r| !r.is_empty())
                    .map(str::to_owned)
                    .collect(),
                subject: f[7].to_owned(),
                body: f[8].trim().to_owned(),
            })
        })
        .collect()
}

/// A commit's files with what each added and removed, against its first
/// parent.
pub fn commit_files(root: &Path, hash: &str) -> Result<Vec<CommitFile>, String> {
    let text = ok(
        root,
        &[
            "show",
            "--format=",
            "--numstat",
            "--first-parent",
            "-M",
            "-z",
            hash,
        ],
    )?;
    let mut out = Vec::new();
    let mut fields = text.split('\0');
    while let Some(record) = fields.next() {
        let record = record.trim_start_matches('\n');
        if record.is_empty() {
            continue;
        }
        let cols: Vec<&str> = record.splitn(3, '\t').collect();
        if cols.len() < 3 {
            continue;
        }
        // (a rename: an empty path, then the old and new names)
        let path = if cols[2].is_empty() {
            let _from = fields.next();
            fields.next().unwrap_or("").to_owned()
        } else {
            cols[2].to_owned()
        };
        out.push(CommitFile {
            path,
            added: cols[0].parse().unwrap_or(0),
            removed: cols[1].parse().unwrap_or(0),
        });
    }
    Ok(out)
}

// -------------------------------------------------------------- diffs --

/// A file's diff: unstaged, staged or a commit's. An untracked file shows
/// whole, as added.
pub fn diff(root: &Path, path: &str, of: &DiffOf) -> Result<Diff, String> {
    let text = match of {
        DiffOf::Unstaged => {
            let tracked = run(root, &["ls-files", "--error-unmatch", "--", path])?.ok;
            if tracked {
                ok(
                    root,
                    &["diff", "--no-color", "--no-ext-diff", "-U3", "--", path],
                )?
            } else {
                // (`--no-index` exits 1 when the files differ)
                run(
                    root,
                    &[
                        "diff",
                        "--no-color",
                        "--no-ext-diff",
                        "--no-index",
                        "-U3",
                        "--",
                        null_device(),
                        path,
                    ],
                )?
                .stdout
            }
        }
        DiffOf::Staged => ok(
            root,
            &[
                "diff",
                "--cached",
                "--no-color",
                "--no-ext-diff",
                "-U3",
                "--",
                path,
            ],
        )?,
        DiffOf::Commit(hash) => ok(
            root,
            &[
                "show",
                "--format=",
                "--no-color",
                "--no-ext-diff",
                "--first-parent",
                "-U3",
                hash,
                "--",
                path,
            ],
        )?,
    };
    let mut d = parse_diff(&text);
    d.path = path.to_owned();
    Ok(d)
}

fn null_device() -> &'static str {
    if cfg!(windows) { "NUL" } else { "/dev/null" }
}

/// One file's unified diff.
pub(crate) fn parse_diff(text: &str) -> Diff {
    let mut d = Diff::default();
    let (mut old, mut new) = (0u32, 0u32);
    for line in text.lines() {
        if line.starts_with("Binary files ") {
            d.binary = true;
        } else if line.starts_with("new file mode") || line.starts_with("--- /dev/null") {
            d.new_file = true;
        } else if line.starts_with("deleted file mode") || line.starts_with("+++ /dev/null") {
            d.deleted = true;
        } else if let Some(header) = line.strip_prefix("@@ ") {
            let Some((ranges, section)) = header.split_once(" @@") else {
                continue;
            };
            let mut it = ranges.split(' ');
            let (os, ol) = range(it.next().unwrap_or("-0"));
            let (ns, nl) = range(it.next().unwrap_or("+0"));
            old = os;
            new = ns;
            d.hunks.push(Hunk {
                old_start: os,
                old_lines: ol,
                new_start: ns,
                new_lines: nl,
                section: section.trim().to_owned(),
                lines: Vec::new(),
                old_no_newline: false,
                new_no_newline: false,
            });
        } else if let Some(h) = d.hunks.last_mut() {
            if line.starts_with("\\ ") {
                // (no newline after the line before: on its side)
                match h.lines.last().map(|l| l.kind) {
                    Some(LineKind::Removed) => h.old_no_newline = true,
                    Some(LineKind::Added) => h.new_no_newline = true,
                    _ => {
                        h.old_no_newline = true;
                        h.new_no_newline = true;
                    }
                }
                continue;
            }
            let (kind, text) = match line.chars().next() {
                Some('+') => (LineKind::Added, &line[1..]),
                Some('-') => (LineKind::Removed, &line[1..]),
                Some(' ') => (LineKind::Context, &line[1..]),
                None => (LineKind::Context, ""),
                _ => continue,
            };
            let (o, n) = match kind {
                LineKind::Context => {
                    old += 1;
                    new += 1;
                    (Some(old - 1), Some(new - 1))
                }
                LineKind::Removed => {
                    old += 1;
                    (Some(old - 1), None)
                }
                LineKind::Added => {
                    new += 1;
                    (None, Some(new - 1))
                }
            };
            h.lines.push(Line {
                kind,
                text: text.to_owned(),
                old: o,
                new: n,
            });
        }
    }
    d
}

/// `-12,3` → (12, 3); `+5` → (5, 1).
fn range(s: &str) -> (u32, u32) {
    let s = s.trim_start_matches(['-', '+']);
    match s.split_once(',') {
        Some((a, b)) => (a.parse().unwrap_or(0), b.parse().unwrap_or(0)),
        None => (s.parse().unwrap_or(0), 1),
    }
}

/// A patch of one hunk of `path`, as `git apply` takes it.
pub(crate) fn hunk_patch(path: &str, hunk: &Hunk) -> String {
    let mut p = format!(
        "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -{},{} +{},{} @@\n",
        hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
    );
    let last_old = hunk.lines.iter().rposition(|l| l.kind != LineKind::Added);
    let last_new = hunk.lines.iter().rposition(|l| l.kind != LineKind::Removed);
    for (i, l) in hunk.lines.iter().enumerate() {
        let mark = match l.kind {
            LineKind::Context => ' ',
            LineKind::Added => '+',
            LineKind::Removed => '-',
        };
        p.push(mark);
        p.push_str(&l.text);
        p.push('\n');
        let no_newline =
            (hunk.old_no_newline && Some(i) == last_old && l.kind == LineKind::Removed)
                || (hunk.new_no_newline && Some(i) == last_new && l.kind == LineKind::Added)
                || (hunk.old_no_newline
                    && hunk.new_no_newline
                    && l.kind == LineKind::Context
                    && Some(i) == last_old
                    && Some(i) == last_new);
        if no_newline {
            p.push_str("\\ No newline at end of file\n");
        }
    }
    p
}

/// A hunk staged, unstaged or discarded.
pub fn apply_hunk(root: &Path, path: &str, hunk: &Hunk, how: Apply) -> Result<(), String> {
    let patch = hunk_patch(path, hunk);
    let args: &[&str] = match how {
        Apply::Stage => &["apply", "--cached", "--whitespace=nowarn", "-"],
        Apply::Unstage => &["apply", "--cached", "-R", "--whitespace=nowarn", "-"],
        Apply::Discard => &["apply", "-R", "--whitespace=nowarn", "-"],
    };
    let mut c = command(root);
    c.args(args);
    let out = run_with(c, Some(&patch))?;
    if out.ok {
        Ok(())
    } else {
        Err(failure(&out, args))
    }
}

/// Files staged whole (`git add`, removals too).
pub fn stage(root: &Path, paths: &[String]) -> Result<(), String> {
    let mut args = vec!["add", "--all", "--"];
    args.extend(paths.iter().map(String::as_str));
    ok(root, &args).map(drop)
}

/// Files unstaged whole (back to HEAD's in the index).
pub fn unstage(root: &Path, paths: &[String]) -> Result<(), String> {
    let has_head = run(root, &["rev-parse", "--verify", "-q", "HEAD"])?.ok;
    let mut args = if has_head {
        vec!["restore", "--staged", "--"]
    } else {
        vec!["rm", "--cached", "-q", "--"]
    };
    args.extend(paths.iter().map(String::as_str));
    ok(root, &args).map(drop)
}

/// A file's unstaged changes discarded: a tracked file back to the
/// index's, an untracked one deleted.
pub fn discard(root: &Path, paths: &[String]) -> Result<(), String> {
    for path in paths {
        let tracked = run(root, &["ls-files", "--error-unmatch", "--", path])?.ok;
        if tracked {
            ok(root, &["restore", "--worktree", "--", path])?;
        } else {
            std::fs::remove_file(root.join(path))
                .map_err(|e| format!("cannot delete {path}: {e}"))?;
        }
    }
    Ok(())
}

/// A file's text at HEAD (none when HEAD doesn't have it).
pub fn head_text(root: &Path, path: &str) -> Option<String> {
    let out = run(root, &["show", &format!("HEAD:{path}")]).ok()?;
    out.ok.then_some(out.stdout)
}

// ------------------------------------------------------------ commits --

/// A commit of what is staged, the repository's hooks run; the new
/// commit's short hash and what the hooks said.
pub fn commit(root: &Path, message: &str) -> Result<(String, String), String> {
    let mut c = command(root);
    c.args(["commit", "-F", "-"]);
    let out = run_with(c, Some(message))?;
    if !out.ok {
        return Err(failure(&out, &["commit"]));
    }
    let hash = ok(root, &["rev-parse", "--short", "HEAD"])?
        .trim()
        .to_owned();
    Ok((hash, out.stderr))
}

/// A commit that undoes `hash` (`git revert`).
pub fn revert(root: &Path, hash: &str) -> Result<(), String> {
    ok(root, &["revert", "--no-edit", hash]).map(drop)
}

// ---------------------------------------------------- switching branches --

/// The changed files git would have to overwrite to switch to `target`:
/// none means the changes can be carried.
pub fn blocking(root: &Path, target: &str, status: &Status) -> Result<Vec<String>, String> {
    let differ = ok(root, &["diff", "--name-only", "HEAD", target, "--"])?;
    let differ: Vec<&str> = differ.lines().collect();
    Ok(status
        .files
        .iter()
        .filter(|f| differ.contains(&f.path.as_str()))
        .map(|f| f.path.clone())
        .collect())
}

/// Switches to a branch (a remote's: a local one tracking it), the
/// changes carried or stashed; a stash Studio made on the target branch is
/// put back.
pub fn switch(root: &Path, target: &str, carry: Carry) -> Result<String, String> {
    let current = status(root)?;
    let mut said = String::new();
    if carry == Carry::Stash && !current.files.is_empty() {
        let from = current
            .branch
            .clone()
            .unwrap_or_else(|| current.head.clone());
        ok(
            root,
            &[
                "stash",
                "push",
                "--include-untracked",
                "-m",
                &format!("{STASH_TAG}{from} switched to {target}"),
            ],
        )?;
        said.push_str(&format!("your changes on {from} are stashed for it. "));
    }
    // (a remote's branch: a local branch tracking it, unless one exists)
    let remote_branch = branches(root)?
        .into_iter()
        .find(|b| b.remote && b.name == target);
    match remote_branch {
        Some(b) => {
            let local = b.name.split_once('/').map_or(b.name.as_str(), |(_, n)| n);
            let exists = run(
                root,
                &[
                    "rev-parse",
                    "--verify",
                    "-q",
                    &format!("refs/heads/{local}"),
                ],
            )?
            .ok;
            if exists {
                ok(root, &["switch", local])?;
            } else {
                ok(root, &["switch", "--track", &b.name])?;
            }
        }
        None => {
            ok(root, &["switch", target])?;
        }
    }
    let now = status(root)?;
    let name = now.branch.clone().unwrap_or_default();
    if let Some(stash) = now.stashes.iter().find(|s| s.branch == name) {
        match ok(root, &["stash", "pop", &stash.name]) {
            Ok(_) => said.push_str(&format!("{name}'s stashed changes are back.")),
            Err(e) => said.push_str(&format!(
                "{name}'s stashed changes didn't go back cleanly ({e}); they stay in {}.",
                stash.name
            )),
        }
    }
    Ok(said.trim().to_owned())
}

/// A commit checked out with no branch (detached), as git does.
pub fn switch_to_commit(root: &Path, hash: &str) -> Result<(), String> {
    ok(root, &["switch", "--detach", hash]).map(drop)
}

/// A new branch at `from` (HEAD when none), switched to when asked.
pub fn new_branch(root: &Path, name: &str, from: Option<&str>, switch: bool) -> Result<(), String> {
    let at = from.unwrap_or("HEAD");
    if switch {
        ok(root, &["switch", "-c", name, at]).map(drop)
    } else {
        ok(root, &["branch", name, at]).map(drop)
    }
}

/// Why git wouldn't take `name` as a branch name; none when it would.
pub fn bad_branch_name(root: &Path, name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return Some("A branch needs a name.".into());
    }
    let valid = run(root, &["check-ref-format", "--branch", name]).ok()?.ok;
    if !valid {
        return Some(
            "Git doesn't take that as a branch name (no spaces, '..', '~', '^', ':', '?', '*' or '[').".into(),
        );
    }
    let exists = run(
        root,
        &["rev-parse", "--verify", "-q", &format!("refs/heads/{name}")],
    )
    .ok()?
    .ok;
    exists.then(|| "A branch has that name already.".into())
}

// ------------------------------------------------------------ remotes --

/// Who a remote operation signs in as: the GitHub sign-in's token, used
/// for github.com remotes over HTTPS; the user's own git setup otherwise.
#[derive(Clone, Default)]
pub struct Credentials {
    pub github_token: Option<String>,
}

/// git for a remote operation: with the GitHub token as the credential
/// helper (from the environment, never the command line) when the remote
/// is github.com over HTTPS and the user is signed in.
fn remote_command(root: &Path, remote: &str, creds: &Credentials) -> Command {
    let mut c = command(root);
    let url = run(root, &["remote", "get-url", remote])
        .map(|o| o.stdout.trim().to_owned())
        .unwrap_or_default();
    if let Some(token) = &creds.github_token
        && url.starts_with("https://github.com/")
    {
        c.env(TOKEN_ENV, token);
        c.args([
            "-c",
            "credential.helper=",
            "-c",
            &format!(
                "credential.helper=!f() {{ test \"$1\" = get || exit 0; echo username=x-access-token; echo \"password=${TOKEN_ENV}\"; }}; f"
            ),
        ]);
    }
    c
}

/// The remote of the branch checked out (its upstream's), else `origin`.
fn remote_of(root: &Path) -> String {
    ok(
        root,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    )
    .ok()
    .and_then(|u| u.trim().split_once('/').map(|(r, _)| r.to_owned()))
    .unwrap_or_else(|| "origin".to_owned())
}

fn remote_error(out: &Out, what: &str) -> RemoteError {
    let said = if out.stderr.is_empty() {
        out.stdout.trim().to_owned()
    } else {
        out.stderr.clone()
    };
    let lower = said.to_lowercase();
    if lower.contains("could not read username")
        || lower.contains("authentication failed")
        || lower.contains("permission denied")
        || lower.contains("terminal prompts disabled")
        || lower.contains("invalid username or password")
    {
        RemoteError::Credentials(format!("{what} needs credentials: {said}"))
    } else if lower.contains("[rejected]")
        || lower.contains("non-fast-forward")
        || lower.contains("fetch first")
    {
        RemoteError::Rejected(format!("{what} rejected: {said}"))
    } else {
        RemoteError::Other(format!("{what} failed: {said}"))
    }
}

/// Pushes the branch checked out; one without an upstream gets one on the
/// remote.
pub fn push(root: &Path, creds: &Credentials) -> Result<String, RemoteError> {
    let st = status(root).map_err(RemoteError::Other)?;
    let branch = st
        .branch
        .ok_or_else(|| RemoteError::Other("no branch is checked out: make one to push".into()))?;
    let remote = remote_of(root);
    let mut c = remote_command(root, &remote, creds);
    if st.upstream.is_some() {
        c.arg("push");
    } else {
        c.args(["push", "--set-upstream", &remote, &branch]);
    }
    let out = run_with(c, None).map_err(RemoteError::Other)?;
    if out.ok {
        Ok(format!("pushed {branch} to {remote}"))
    } else {
        Err(remote_error(&out, "push"))
    }
}

/// Pulls as the user's git config says (`git pull`: merge unless
/// pull.rebase says otherwise).
pub fn pull(root: &Path, creds: &Credentials) -> Result<String, RemoteError> {
    let remote = remote_of(root);
    let mut c = remote_command(root, &remote, creds);
    c.arg("pull");
    let out = run_with(c, None).map_err(RemoteError::Other)?;
    if out.ok {
        Ok(out.stdout.lines().last().unwrap_or("").trim().to_owned())
    } else if out.stdout.contains("CONFLICT") || out.stderr.contains("CONFLICT") {
        // (the merge stopped at conflicts: the page shows them)
        Ok("the pull stopped at merge conflicts".to_owned())
    } else {
        Err(remote_error(&out, "pull"))
    }
}

/// Fetches every remote quietly (the status bar's ↑ and ↓).
pub fn fetch(root: &Path, creds: &Credentials) -> Result<(), RemoteError> {
    let remote = remote_of(root);
    let mut c = remote_command(root, &remote, creds);
    c.args(["fetch", "--all", "--prune", "--quiet"]);
    let out = run_with(c, None).map_err(RemoteError::Other)?;
    if out.ok {
        Ok(())
    } else {
        Err(remote_error(&out, "fetch"))
    }
}

// ---------------------------------------------------------- conflicts --

/// A conflicted file taken whole from one side, and staged as resolved.
pub fn take_side(root: &Path, path: &str, side: Side) -> Result<(), String> {
    let flag = match side {
        Side::Mine => "--ours",
        Side::Theirs => "--theirs",
    };
    ok(root, &["checkout", flag, "--", path])?;
    ok(root, &["add", "--", path]).map(drop)
}

/// The merge (or rebase) under way given up.
pub fn abort(root: &Path) -> Result<(), String> {
    let st = status(root)?;
    if st.rebasing {
        ok(root, &["rebase", "--abort"]).map(drop)
    } else {
        ok(root, &["merge", "--abort"]).map(drop)
    }
}

// -------------------------------------------------------------- blame --

/// Who last changed each line of `path` as it is in the editor
/// (`contents`, unsaved edits too: their lines are "not committed yet").
pub fn blame(root: &Path, path: &str, contents: &str) -> Result<Vec<BlameLine>, String> {
    let mut c = command(root);
    c.args(["blame", "--porcelain", "--contents", "-", "--", path]);
    let out = run_with(c, Some(contents))?;
    if !out.ok {
        return Err(failure(&out, &["blame"]));
    }
    Ok(parse_blame(&out.stdout))
}

fn parse_blame(text: &str) -> Vec<BlameLine> {
    use std::collections::HashMap;
    let mut known: HashMap<String, BlameLine> = HashMap::new();
    let mut lines: Vec<(u32, String)> = Vec::new();
    let mut current = String::new();
    for line in text.lines() {
        if line.starts_with('\t') {
            continue;
        }
        let mut words = line.split(' ');
        let first = words.next().unwrap_or("");
        if first.len() == 40 && first.chars().all(|c| c.is_ascii_hexdigit()) {
            current = first.to_owned();
            let _orig = words.next();
            if let Some(n) = words.next().and_then(|n| n.parse().ok()) {
                lines.push((n, current.clone()));
            }
            known.entry(current.clone()).or_insert_with(|| BlameLine {
                hash: if current.chars().all(|c| c == '0') {
                    String::new()
                } else {
                    current.clone()
                },
                author: String::new(),
                time: 0,
                subject: String::new(),
            });
            continue;
        }
        let Some(entry) = known.get_mut(&current) else {
            continue;
        };
        let rest = line.split_once(' ').map_or("", |(_, r)| r);
        match first {
            "author" => entry.author = rest.to_owned(),
            "author-time" => entry.time = rest.parse().unwrap_or(0),
            "summary" => entry.subject = rest.to_owned(),
            _ => {}
        }
    }
    lines.sort_by_key(|(n, _)| *n);
    lines
        .into_iter()
        .map(|(_, h)| known.get(&h).cloned().expect("a commit seen"))
        .collect()
}

// ------------------------------------------ committing with fmt and check --

/// A `.wowlua` file's text as wowlua fmt writes it, with its package's
/// settings (as the language server formats); none when it can't be
/// formatted (a syntax error: the check reports it).
fn formatted(path: &Path, text: &str) -> Option<String> {
    let mut db = wowlua_span::SourceDb::new();
    let config = path
        .parent()
        .and_then(wowlua_driver::project::find_manifest)
        .and_then(|m| wowlua_driver::project::load_manifest(&mut db, &m).manifest)
        .map(|m| wowlua_driver::format::config(&m.manifest.fmt))
        .unwrap_or_default();
    let file = db.add(path.to_path_buf(), text.to_owned());
    let out = wowlua_fmt::format(text.as_bytes(), file, &config).ok()?;
    String::from_utf8(out).ok()
}

/// A commit of what is staged (S069): with `format_and_check`, the staged
/// `.wowlua` files formatted and staged again (those with unstaged
/// changes too only checked, so nothing unstaged slips in), then checked
/// with their packages; an error in a staged file stops the commit. The
/// repository's hooks run as git runs them. Pushed after when asked.
pub fn commit_checked(
    root: &Path,
    message: &str,
    format_and_check: bool,
    push_after: bool,
    creds: &Credentials,
) -> Result<Committed, CommitStop> {
    let st = status(root).map_err(CommitStop::Failed)?;
    let staged: Vec<&File> = st
        .files
        .iter()
        .filter(|f| {
            f.staged.is_some()
                && f.staged != Some(Change::Deleted)
                && f.staged != Some(Change::Conflicted)
        })
        .collect();
    if staged.is_empty() && st.merging.is_none() {
        return Err(CommitStop::Failed(
            "Nothing is staged: stage a file or a hunk first.".into(),
        ));
    }
    let mut formatted_files = Vec::new();
    let mut not_formatted = Vec::new();
    let mut warnings = 0;
    if format_and_check {
        for f in staged.iter().filter(|f| f.path.ends_with(".wowlua")) {
            if f.unstaged.is_some() {
                not_formatted.push(f.path.clone());
                continue;
            }
            let file = root.join(&f.path);
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            if let Some(new) = formatted(&file, &text)
                && new != text
            {
                std::fs::write(&file, &new)
                    .map_err(|e| CommitStop::Failed(format!("cannot write {}: {e}", f.path)))?;
                stage(root, std::slice::from_ref(&f.path)).map_err(CommitStop::Failed)?;
                formatted_files.push(f.path.clone());
            }
        }
        // The staged sources, checked with their packages.
        let mut by_package: std::collections::BTreeMap<
            PathBuf,
            std::collections::HashSet<PathBuf>,
        > = Default::default();
        for f in &staged {
            if !(f.path.ends_with(".wowlua") || f.path.ends_with(".lua")) {
                continue;
            }
            let file = root.join(&f.path);
            let Ok(canonical) = std::fs::canonicalize(&file) else {
                continue;
            };
            if let Some(manifest) = file
                .parent()
                .and_then(wowlua_driver::project::find_manifest)
                .and_then(|m| std::fs::canonicalize(m).ok())
            {
                by_package.entry(manifest).or_default().insert(canonical);
            }
        }
        let mut errors = Vec::new();
        let root_canonical = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        for (manifest, files) in &by_package {
            let checked = wowlua_lsp::check::check(
                manifest,
                false,
                &std::collections::HashSet::new(),
                Some(files),
                None,
            );
            for c in checked {
                for (path, diagnostics) in &c.diagnostics {
                    if !files.contains(path) {
                        continue;
                    }
                    let rel = path
                        .strip_prefix(&root_canonical)
                        .map(|r| r.to_string_lossy().replace('\\', "/"))
                        .unwrap_or_else(|_| path.to_string_lossy().into_owned());
                    for d in diagnostics {
                        match d["severity"].as_u64() {
                            Some(1) => errors.push((
                                rel.clone(),
                                d["range"]["start"]["line"].as_u64().unwrap_or(0) as u32 + 1,
                                d["code"].as_str().unwrap_or("").to_owned(),
                                d["message"].as_str().unwrap_or("").to_owned(),
                            )),
                            Some(2) => warnings += 1,
                            _ => {}
                        }
                    }
                }
            }
        }
        if !errors.is_empty() {
            return Err(CommitStop::Errors {
                formatted: formatted_files,
                errors,
            });
        }
    }
    let (hash, hooks) = commit(root, message).map_err(CommitStop::Failed)?;
    let pushed = push_after.then(|| push(root, creds));
    Ok(Committed {
        hash,
        message: message.lines().next().unwrap_or("").to_owned(),
        formatted: formatted_files,
        not_formatted,
        checked: format_and_check,
        warnings,
        hooks,
        pushed,
    })
}

// ----------------------------------------------------------- requests --

/// One change of the repository at a time (index.lock): reads go
/// alongside.
static CHANGING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Studio's question answered (on a worker thread).
pub fn answer(root: &Path, request: GitRequest, creds: &Credentials) -> GitEvent {
    let done = |what: &str, r: Result<String, String>| GitEvent::Done {
        what: what.to_owned(),
        result: r,
    };
    let unit = |r: Result<(), String>| r.map(|()| String::new());
    let changes = !matches!(
        request,
        GitRequest::Status
            | GitRequest::Branches
            | GitRequest::Log { .. }
            | GitRequest::Diff { .. }
            | GitRequest::CommitFiles(_)
            | GitRequest::Blocking(_)
            | GitRequest::BranchName(_)
            | GitRequest::Blame { .. }
            | GitRequest::HeadText(_)
    );
    let _held = changes.then(|| {
        CHANGING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    });
    match request {
        GitRequest::Status => GitEvent::Status(status(root)),
        GitRequest::Branches => GitEvent::Branches(branches(root)),
        GitRequest::Log { rev, count } => GitEvent::Log {
            result: log(root, rev.as_deref(), count),
            rev,
        },
        GitRequest::Diff { path, of } => GitEvent::Diff {
            result: diff(root, &path, &of),
            path,
            of,
        },
        GitRequest::CommitFiles(hash) => GitEvent::CommitFiles {
            result: commit_files(root, &hash),
            hash,
        },
        GitRequest::Stage(paths) => done("stage", unit(stage(root, &paths))),
        GitRequest::Unstage(paths) => done("unstage", unit(unstage(root, &paths))),
        GitRequest::Discard(paths) => done("discard", unit(discard(root, &paths))),
        GitRequest::Hunk { path, hunk, how } => {
            let what = match how {
                Apply::Stage => "stage hunk",
                Apply::Unstage => "unstage hunk",
                Apply::Discard => "discard hunk",
            };
            done(what, unit(apply_hunk(root, &path, &hunk, how)))
        }
        GitRequest::Commit {
            message,
            format_and_check,
            push,
        } => GitEvent::Committed(commit_checked(
            root,
            &message,
            format_and_check,
            push,
            creds,
        )),
        GitRequest::Push => GitEvent::Remote {
            what: "push".into(),
            quiet: false,
            result: push(root, creds),
        },
        GitRequest::Pull => GitEvent::Remote {
            what: "pull".into(),
            quiet: false,
            result: pull(root, creds),
        },
        GitRequest::Fetch { quiet } => GitEvent::Remote {
            what: "fetch".into(),
            quiet,
            result: fetch(root, creds).map(|()| String::new()),
        },
        GitRequest::Blocking(target) => GitEvent::Blocking {
            files: status(root).and_then(|st| blocking(root, &target, &st)),
            target,
        },
        GitRequest::Switch { target, carry } => done("switch", switch(root, &target, carry)),
        GitRequest::SwitchCommit(hash) => done("switch", unit(switch_to_commit(root, &hash))),
        GitRequest::NewBranch { name, from, switch } => done(
            "new branch",
            unit(new_branch(root, &name, from.as_deref(), switch)).map(|_| name),
        ),
        GitRequest::BranchName(name) => GitEvent::BranchName {
            bad: bad_branch_name(root, &name),
            name,
        },
        GitRequest::Revert(hash) => done("revert", unit(revert(root, &hash))),
        GitRequest::TakeSide { path, side } => done("take", unit(take_side(root, &path, side))),
        GitRequest::Abort => done("abort", unit(abort(root))),
        GitRequest::Blame { path, contents } => GitEvent::Blame {
            result: blame(root, &path, &contents),
            path,
        },
        GitRequest::HeadText(path) => GitEvent::HeadText {
            text: head_text(root, &path),
            path,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A repository in a temporary folder with one commit.
    fn repo() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("a folder");
        let root = dir.path().to_path_buf();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.name", "Tester"],
            vec!["config", "user.email", "tester@example.com"],
            vec!["config", "commit.gpgsign", "false"],
        ] {
            ok(&root, &args).expect("git");
        }
        std::fs::write(
            root.join("a.wowlua"),
            "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n",
        )
        .expect("written");
        stage(&root, &["a.wowlua".into()]).expect("staged");
        commit(&root, "First").expect("committed");
        (dir, root)
    }

    #[test]
    fn status_reads_branch_changes_and_untracked_files() {
        let (_dir, root) = repo();
        std::fs::write(
            root.join("a.wowlua"),
            "one\nTWO\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n",
        )
        .expect("written");
        std::fs::write(root.join("new.wowlua"), "x\n").expect("written");
        let st = status(&root).expect("status");
        assert_eq!(st.branch.as_deref(), Some("main"));
        assert_eq!(st.head.len(), 7);
        let a = st.file("a.wowlua").expect("a");
        assert_eq!((a.staged, a.unstaged), (None, Some(Change::Modified)));
        assert_eq!(
            st.file("new.wowlua").expect("new").change(),
            Change::Untracked
        );
        assert_eq!(st.branch_label(), "main");
    }

    #[test]
    fn a_hunk_is_staged_alone_and_unstaged_again() {
        let (_dir, root) = repo();
        // two hunks: line 1 and line 10 (farther apart than the context)
        std::fs::write(
            root.join("a.wowlua"),
            "ONE\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nTEN\n",
        )
        .expect("written");
        let d = diff(&root, "a.wowlua", &DiffOf::Unstaged).expect("diff");
        assert_eq!(d.hunks.len(), 2);
        apply_hunk(&root, "a.wowlua", &d.hunks[1], Apply::Stage).expect("staged");
        let staged = diff(&root, "a.wowlua", &DiffOf::Staged).expect("diff");
        assert_eq!(staged.hunks.len(), 1);
        assert!(staged.hunks[0].lines.iter().any(|l| l.text == "TEN"));
        let st = status(&root).expect("status");
        let a = st.file("a.wowlua").expect("a");
        assert_eq!(
            (a.staged, a.unstaged),
            (Some(Change::Modified), Some(Change::Modified))
        );
        apply_hunk(&root, "a.wowlua", &staged.hunks[0], Apply::Unstage).expect("unstaged");
        assert!(
            diff(&root, "a.wowlua", &DiffOf::Staged)
                .expect("diff")
                .hunks
                .is_empty()
        );
        // discarded: the working tree loses the second change only
        let d = diff(&root, "a.wowlua", &DiffOf::Unstaged).expect("diff");
        apply_hunk(&root, "a.wowlua", &d.hunks[1], Apply::Discard).expect("discarded");
        let text = std::fs::read_to_string(root.join("a.wowlua")).expect("read");
        assert!(text.contains("ONE") && text.contains("ten"));
    }

    #[test]
    fn a_file_without_a_last_newline_stages_by_hunk() {
        let (_dir, root) = repo();
        std::fs::write(
            root.join("a.wowlua"),
            "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nTEN",
        )
        .expect("written");
        let d = diff(&root, "a.wowlua", &DiffOf::Unstaged).expect("diff");
        assert!(d.hunks[0].new_no_newline);
        apply_hunk(&root, "a.wowlua", &d.hunks[0], Apply::Stage).expect("staged");
        assert!(
            diff(&root, "a.wowlua", &DiffOf::Unstaged)
                .expect("diff")
                .hunks
                .is_empty()
        );
    }

    #[test]
    fn an_untracked_file_diffs_as_added_and_commits() {
        let (_dir, root) = repo();
        std::fs::write(root.join("b.wowlua"), "x\ny\n").expect("written");
        let d = diff(&root, "b.wowlua", &DiffOf::Unstaged).expect("diff");
        assert!(d.new_file);
        assert_eq!(d.hunks[0].added(), 2);
        stage(&root, &["b.wowlua".into()]).expect("staged");
        let (hash, _) = commit(&root, "Second\n\nThe body.").expect("committed");
        let log = log(&root, None, 10).expect("log");
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].short, hash);
        assert_eq!(log[0].subject, "Second");
        assert_eq!(log[0].body, "The body.");
        assert!(log[0].refs.iter().any(|r| r.contains("main")));
        let files = commit_files(&root, &hash).expect("files");
        assert_eq!(
            files,
            [CommitFile {
                path: "b.wowlua".into(),
                added: 2,
                removed: 0
            }]
        );
        let shown = diff(&root, "b.wowlua", &DiffOf::Commit(hash)).expect("diff");
        assert_eq!(shown.hunks[0].added(), 2);
    }

    #[test]
    fn switching_stashes_and_puts_the_changes_back() {
        let (_dir, root) = repo();
        new_branch(&root, "feat/x", None, false).expect("branch");
        std::fs::write(root.join("a.wowlua"), "changed\n").expect("written");
        switch(&root, "feat/x", Carry::Stash).expect("switched");
        let st = status(&root).expect("status");
        assert_eq!(st.branch.as_deref(), Some("feat/x"));
        assert!(st.files.is_empty());
        assert_eq!(st.stashes.len(), 1);
        assert_eq!(st.stashes[0].branch, "main");
        let said = switch(&root, "main", Carry::Carry).expect("back");
        assert!(said.contains("back"), "{said}");
        let st = status(&root).expect("status");
        assert_eq!(st.files.len(), 1);
        assert!(st.stashes.is_empty());
        let bs = branches(&root).expect("branches");
        assert_eq!(bs.iter().filter(|b| !b.remote).count(), 2);
        assert!(bs.iter().any(|b| b.current && b.name == "main"));
    }

    #[test]
    fn a_carry_is_blocked_by_a_file_the_target_changes() {
        let (_dir, root) = repo();
        new_branch(&root, "other", None, true).expect("branch");
        std::fs::write(root.join("a.wowlua"), "theirs\n").expect("written");
        stage(&root, &["a.wowlua".into()]).expect("staged");
        commit(&root, "Other").expect("committed");
        switch(&root, "main", Carry::Carry).expect("switched");
        std::fs::write(root.join("a.wowlua"), "mine\n").expect("written");
        let st = status(&root).expect("status");
        assert_eq!(
            blocking(&root, "other", &st).expect("blocking"),
            ["a.wowlua"]
        );
    }

    #[test]
    fn branch_names_are_checked_as_git_does() {
        let (_dir, root) = repo();
        assert!(bad_branch_name(&root, "feat/ok").is_none());
        assert!(bad_branch_name(&root, "two words").is_some());
        assert!(bad_branch_name(&root, "a..b").is_some());
        assert_eq!(
            bad_branch_name(&root, "main").as_deref(),
            Some("A branch has that name already.")
        );
    }

    #[test]
    fn a_merge_conflict_shows_and_is_taken_from_a_side() {
        let (_dir, root) = repo();
        new_branch(&root, "other", None, true).expect("branch");
        std::fs::write(root.join("a.wowlua"), "theirs\n").expect("written");
        stage(&root, &["a.wowlua".into()]).expect("staged");
        commit(&root, "Other").expect("committed");
        switch(&root, "main", Carry::Carry).expect("switched");
        std::fs::write(root.join("a.wowlua"), "mine\n").expect("written");
        stage(&root, &["a.wowlua".into()]).expect("staged");
        commit(&root, "Mine").expect("committed");
        let merged = run(&root, &["merge", "other"]).expect("merge");
        assert!(!merged.ok);
        let st = status(&root).expect("status");
        assert_eq!(st.merging.as_deref(), Some("other"));
        assert_eq!(st.conflicts().count(), 1);
        assert_eq!(st.branch_label(), "main · merging other");
        take_side(&root, "a.wowlua", Side::Theirs).expect("taken");
        assert_eq!(
            std::fs::read_to_string(root.join("a.wowlua")).expect("read"),
            "theirs\n"
        );
        assert_eq!(status(&root).expect("status").conflicts().count(), 0);
        abort(&root).expect("aborted");
        assert!(status(&root).expect("status").merging.is_none());
    }

    #[test]
    fn blame_marks_unsaved_lines_as_not_committed() {
        let (_dir, root) = repo();
        let text = "one\nNEW\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n";
        let lines = blame(&root, "a.wowlua", text).expect("blame");
        assert_eq!(lines.len(), 10);
        assert_eq!(lines[0].author, "Tester");
        assert_eq!(lines[0].subject, "First");
        assert!(lines[1].hash.is_empty());
        assert_eq!(
            head_text(&root, "a.wowlua").expect("text").lines().count(),
            10
        );
    }

    #[test]
    fn diffs_and_status_parse_from_git_s_text() {
        let d = parse_diff(
            "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@ local function f()\n a\n-b\n+B\n\\ No newline at end of file\n",
        );
        assert_eq!(d.hunks[0].section, "local function f()");
        assert_eq!(d.hunks[0].lines[2].new, Some(2));
        assert!(d.hunks[0].new_no_newline && !d.hunks[0].old_no_newline);
        let st = parse_status(
            "# branch.oid 1234567890abcdef\0# branch.head feat/x\0# branch.upstream origin/feat/x\0# branch.ab +2 -1\0u UU N... 100644 100644 100644 100644 a b c src/x.wowlua\0",
        );
        assert_eq!((st.ahead, st.behind), (2, 1));
        assert_eq!(st.upstream.as_deref(), Some("origin/feat/x"));
        assert_eq!(st.files[0].path, "src/x.wowlua");
        assert_eq!(st.files[0].change(), Change::Conflicted);
        assert_eq!(merged_name("Merge branch 'main' into feat/x\n"), "main");
    }
}
