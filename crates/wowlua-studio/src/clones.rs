//! Repositories opened from GitHub (M12, S075): `git clone` into Studio's
//! data, opened as a folder, deleted when the workspace closes. Each clone
//! is `<data>/clones/<owner>-<name>-<n>/<name>`; the outer folder holds a
//! lock its window keeps while it is open, so a clone no window holds is
//! one left by a window that quit.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;

use wowlua_studio_app::github::model::Repo;
use wowlua_studio_app::system::{LeftClone, Losses};

use crate::git::{Credentials, TOKEN_ENV, command, ok, run_with};

/// The lock file in a clone's outer folder.
const LOCK: &str = "studio.lock";

/// The locks this process keeps: the outer folders of its clones.
static HELD: Mutex<Vec<(PathBuf, File)>> = Mutex::new(Vec::new());

/// Where Studio's clones are.
pub fn dir() -> Option<PathBuf> {
    crate::store::data_dir().map(|d| d.join("clones"))
}

/// Takes the lock of a clone's outer folder for this process; false when
/// another process has it.
fn hold(outer: &Path) -> bool {
    let Ok(mut held) = HELD.lock() else {
        return false;
    };
    if held.iter().any(|(p, _)| p == outer) {
        return true;
    }
    let Ok(file) = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(outer.join(LOCK))
    else {
        return false;
    };
    if file.try_lock().is_err() {
        return false;
    }
    held.push((outer.to_path_buf(), file));
    true
}

fn release(outer: &Path) {
    if let Ok(mut held) = HELD.lock() {
        held.retain(|(p, _)| p != outer);
    }
}

/// Whether another process holds the clone's lock (its window has it
/// open).
fn held_elsewhere(outer: &Path) -> bool {
    if HELD.lock().is_ok_and(|h| h.iter().any(|(p, _)| p == outer)) {
        return false;
    }
    let Ok(file) = File::options().write(true).open(outer.join(LOCK)) else {
        return false;
    };
    file.try_lock().is_err()
}

/// The outer folder of a clone at `root`, when `root` is one of `clones`.
fn outer_of(clones: &Path, root: &Path) -> Option<PathBuf> {
    let clones = std::fs::canonicalize(clones).ok()?;
    let root = std::fs::canonicalize(root).ok()?;
    let outer = root.parent()?;
    (outer.parent()? == clones).then(|| outer.to_path_buf())
}

/// Marks the clone at `root` as open in this window.
pub fn hold_clone(clones: &Path, root: &Path) {
    if let Some(outer) = outer_of(clones, root) {
        hold(&outer);
    }
}

/// A free outer folder for `owner/name` under `clones`.
fn new_outer(clones: &Path, repo: &str) -> PathBuf {
    let base = repo.replace('/', "-");
    (1..)
        .map(|n| clones.join(format!("{base}-{n}")))
        .find(|p| !p.exists())
        .unwrap_or_else(|| clones.join(base))
}

/// What a line of `git clone --progress` says: the share of the whole
/// clone done, and its words (`receiving objects 312 of 459`).
pub fn progress(line: &str) -> Option<(f32, String)> {
    let line = line.trim();
    let (phase, rest) = line
        .strip_prefix("remote:")
        .unwrap_or(line)
        .trim()
        .split_once(':')?;
    let phase = phase.trim();
    // (each phase's share of the whole)
    let (from, span) = match phase {
        "Counting objects" | "Enumerating objects" => (0.0, 0.02),
        "Compressing objects" => (0.02, 0.03),
        "Receiving objects" => (0.05, 0.75),
        "Resolving deltas" => (0.8, 0.1),
        "Updating files" => (0.9, 0.1),
        _ => return None,
    };
    let rest = rest.trim();
    let pct: f32 = rest.split('%').next()?.trim().parse().ok()?;
    let counts = rest
        .split_once('(')
        .and_then(|(_, c)| c.split(')').next())
        .and_then(|c| c.split_once('/'));
    let phase = phase.to_lowercase();
    let said = match counts {
        Some((done, all)) => format!("{phase} {done} of {all}"),
        None => format!("{phase} {pct:.0}%"),
    };
    Some((from + span * pct / 100.0, said))
}

/// Clones `url` (`owner/name` on GitHub) at `branch` into a new folder
/// under `clones`, telling its progress; the clone's folder, or why not.
pub fn clone_into(
    clones: &Path,
    url: &str,
    repo: &str,
    branch: &str,
    creds: &Credentials,
    tell: &dyn Fn(Option<f32>, String),
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(clones)
        .map_err(|e| format!("cannot make {}: {e}", clones.display()))?;
    let outer = new_outer(clones, repo);
    std::fs::create_dir_all(&outer).map_err(|e| format!("cannot make {}: {e}", outer.display()))?;
    hold(&outer);
    let name = repo.rsplit('/').next().unwrap_or(repo);
    let root = outer.join(name);
    let mut c = command(&outer);
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
    c.args(["clone", "--progress", "--branch", branch, url, name])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let failed = |why: String| {
        release(&outer);
        let _ = std::fs::remove_dir_all(&outer);
        Err(why)
    };
    let mut child = match c.spawn() {
        Ok(child) => child,
        Err(e) => return failed(format!("cannot run git: {e}")),
    };
    // git's progress: lines ended by `\r` as each phase goes, `\n` at its
    // end.
    let mut said = String::new();
    if let Some(mut err) = child.stderr.take() {
        let mut line = Vec::new();
        let mut byte = [0u8; 1];
        while err.read(&mut byte).is_ok_and(|n| n == 1) {
            if byte[0] == b'\r' || byte[0] == b'\n' {
                let text = String::from_utf8_lossy(&line).into_owned();
                if let Some((share, words)) = progress(&text) {
                    tell(Some(share), words);
                } else if !text.trim().is_empty() {
                    said = text;
                }
                line.clear();
            } else {
                line.push(byte[0]);
            }
        }
    }
    match child.wait() {
        Ok(status) if status.success() => Ok(root),
        Ok(_) => failed(format!("the clone failed: {}", said.trim())),
        Err(e) => failed(format!("the clone failed: {e}")),
    }
}

/// What deleting the clone at `root` would lose.
pub fn losses(root: &Path) -> Result<Losses, String> {
    let changed = ok(root, &["status", "--porcelain", "--untracked-files=all"])?
        .lines()
        .filter(|l| l.len() > 3)
        .map(|l| l[3..].trim_matches('"').to_owned())
        .collect();
    let unpushed = ok(
        root,
        &[
            "log",
            "--branches",
            "--not",
            "--remotes",
            "--format=%h%x09%s",
        ],
    )?
    .lines()
    .filter_map(|l| l.split_once('\t'))
    .map(|(h, s)| (h.to_owned(), s.to_owned()))
    .collect();
    let stashes = ok(root, &["stash", "list"])?.lines().count();
    Ok(Losses {
        changed,
        unpushed,
        stashes,
    })
}

/// Deletes the clone at `root`: only one of `clones`.
pub fn delete(clones: &Path, root: &Path) -> Result<(), String> {
    let outer = outer_of(clones, root)
        .ok_or_else(|| format!("{} is not one of Studio's clones", root.display()))?;
    release(&outer);
    std::fs::remove_dir_all(&outer).map_err(|e| format!("cannot delete {}: {e}", outer.display()))
}

/// The repository a clone is of, and its branch.
fn what(root: &Path) -> (Option<String>, Option<String>) {
    let repo = ok(root, &["remote", "get-url", "origin"])
        .ok()
        .and_then(|u| Repo::of_remote(u.trim()))
        .map(|r| r.full());
    let branch = ok(root, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()
        .map(|b| b.trim().to_owned())
        .filter(|b| b != "HEAD");
    (repo, branch)
}

/// The clones under `clones` no window holds: those with nothing to lose
/// (or no repository) deleted, the others listed. One git can't read is
/// kept as it is: nothing is deleted on a guess.
pub fn left(clones: &Path) -> Vec<LeftClone> {
    let Ok(entries) = std::fs::read_dir(clones) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let outer = entry.path();
        let ours = HELD
            .lock()
            .is_ok_and(|h| h.iter().any(|(p, _)| *p == outer));
        if !outer.is_dir() || ours || held_elsewhere(&outer) {
            continue;
        }
        let inner = std::fs::read_dir(&outer).ok().and_then(|d| {
            d.flatten()
                .map(|e| e.path())
                .find(|p| p.join(".git").exists())
        });
        let Some(root) = inner else {
            // (a clone that never finished)
            let _ = std::fs::remove_dir_all(&outer);
            continue;
        };
        match losses(&root) {
            Ok(l) if l.is_empty() => {
                let _ = std::fs::remove_dir_all(&outer);
            }
            Ok(l) => {
                let (repo, branch) = what(&root);
                found.push(LeftClone {
                    path: root,
                    repo: repo.unwrap_or_default(),
                    branch,
                    losses: l,
                });
            }
            Err(_) => {}
        }
    }
    found
}

/// A fork of the repository at `url` taken over by `root`'s branch: the
/// fork's remote `fork` added (or pointed at it) and the branch pushed to
/// it, tried again while GitHub makes the fork.
pub fn push_to_fork(root: &Path, url: &str, creds: &Credentials) -> Result<String, String> {
    if ok(root, &["remote", "get-url", "fork"]).is_ok() {
        ok(root, &["remote", "set-url", "fork", url])?;
    } else {
        ok(root, &["remote", "add", "fork", url])?;
    }
    let branch = ok(root, &["rev-parse", "--abbrev-ref", "HEAD"])?
        .trim()
        .to_owned();
    let mut last = String::new();
    for attempt in 0..6 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_secs(3));
        }
        let mut c = command(root);
        if let Some(token) = &creds.github_token {
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
        c.args(["push", "--set-upstream", "fork", &branch]);
        let out = run_with(c, None)?;
        if out.ok {
            return Ok(branch);
        }
        last = out.stderr;
    }
    Err(format!("the push to the fork failed: {}", last.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bare repository with one commit on `main`, to clone from.
    fn origin() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("a folder");
        let work = dir.path().join("work");
        std::fs::create_dir_all(&work).expect("made");
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.name", "Tester"],
            vec!["config", "user.email", "tester@example.com"],
            vec!["config", "commit.gpgsign", "false"],
        ] {
            ok(&work, &args).expect("git");
        }
        std::fs::write(work.join("a.wowlua"), "one\n").expect("written");
        ok(&work, &["add", "."]).expect("added");
        ok(&work, &["commit", "-q", "-m", "First"]).expect("committed");
        let bare = dir.path().join("o.git");
        ok(
            dir.path(),
            &[
                "clone",
                "-q",
                "--bare",
                "work",
                bare.to_str().expect("utf-8"),
            ],
        )
        .expect("bare");
        let url = format!("file://{}", bare.display());
        (dir, url)
    }

    fn configured(root: &Path) {
        for args in [
            vec!["config", "user.name", "Tester"],
            vec!["config", "user.email", "tester@example.com"],
            vec!["config", "commit.gpgsign", "false"],
        ] {
            ok(root, &args).expect("git");
        }
    }

    #[test]
    fn git_s_progress_reads_as_a_share_of_the_clone() {
        let (share, said) =
            progress("Receiving objects:  50% (2/4), 1.2 MiB | 3 MiB/s").expect("read");
        assert!((share - 0.425).abs() < 0.001, "{share}");
        assert_eq!(said, "receiving objects 2 of 4");
        assert!(progress("remote: Counting objects: 100% (8/8), done.").is_some());
        assert!(progress("Cloning into 'suuui'...").is_none());
    }

    #[test]
    fn a_clone_is_a_folder_named_for_the_repository_and_deleted_whole() {
        let (_o, url) = origin();
        let clones = tempfile::tempdir().expect("a folder");
        let told = std::cell::Cell::new(0);
        let root = clone_into(
            clones.path(),
            &url,
            "me/suuui",
            "main",
            &Credentials::default(),
            &|_, _| told.set(told.get() + 1),
        )
        .expect("cloned");
        assert_eq!(root.file_name().and_then(|n| n.to_str()), Some("suuui"));
        assert!(root.join("a.wowlua").exists());
        assert_eq!(losses(&root).expect("read"), Losses::default());
        // (nothing outside the clones is deleted)
        let elsewhere = tempfile::tempdir().expect("a folder");
        assert!(delete(clones.path(), elsewhere.path()).is_err());
        assert!(elsewhere.path().exists());
        delete(clones.path(), &root).expect("deleted");
        assert!(!root.parent().expect("outer").exists());
    }

    #[test]
    fn losses_are_changes_commits_on_no_remote_and_stashes() {
        let (_o, url) = origin();
        let clones = tempfile::tempdir().expect("a folder");
        let root = clone_into(
            clones.path(),
            &url,
            "me/r",
            "main",
            &Credentials::default(),
            &|_, _| {},
        )
        .expect("cloned");
        configured(&root);
        std::fs::write(root.join("a.wowlua"), "two\n").expect("written");
        std::fs::write(root.join("b.wowlua"), "new\n").expect("written");
        let l = losses(&root).expect("read");
        assert_eq!(
            l.changed,
            vec!["a.wowlua".to_owned(), "b.wowlua".to_owned()]
        );
        ok(&root, &["add", "."]).expect("added");
        ok(&root, &["commit", "-q", "-m", "Second"]).expect("committed");
        let l = losses(&root).expect("read");
        assert!(l.changed.is_empty());
        assert_eq!(l.unpushed.len(), 1);
        assert_eq!(l.unpushed[0].1, "Second");
    }

    #[test]
    fn a_clone_no_window_holds_is_left_and_kept_only_with_something_to_lose() {
        let (_o, url) = origin();
        let clones = tempfile::tempdir().expect("a folder");
        let clean = clone_into(
            clones.path(),
            &url,
            "me/a",
            "main",
            &Credentials::default(),
            &|_, _| {},
        )
        .expect("cloned");
        let dirty = clone_into(
            clones.path(),
            &url,
            "me/b",
            "main",
            &Credentials::default(),
            &|_, _| {},
        )
        .expect("cloned");
        std::fs::write(dirty.join("a.wowlua"), "changed\n").expect("written");
        // (held here: not left)
        assert!(left(clones.path()).is_empty());
        release(clean.parent().expect("outer"));
        release(dirty.parent().expect("outer"));
        // (a process another test starts holds a copy of the lock until it
        // runs its program: the clones are left a moment later)
        let mut found = left(clones.path());
        for _ in 0..40 {
            if found.len() == 1 && !clean.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
            found = left(clones.path());
        }
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, dirty);
        assert_eq!(found[0].branch.as_deref(), Some("main"));
        assert!(!clean.exists(), "nothing to lose: deleted");
    }
}
