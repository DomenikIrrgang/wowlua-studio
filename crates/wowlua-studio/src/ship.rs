//! The ship page's work (M9, S061), in Studio's process as wowlua's
//! commands do it: what the page shows of the package (its profiles,
//! outputs, installs and publish state); a build with wowlua's progress
//! (T503) and its install; the dev loop over the disk's changes (`wowlua
//! dev`); the publish tokens in wowlua's credential store (T501); a
//! publish or its dry run; a tag pushed after the user confirms.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use notify::{EventKind, RecursiveMode, Watcher};
use serde_json::Value as Json;
use wowlua_driver::Session;
use wowlua_driver::install::{self, InstallRequest};
use wowlua_driver::project::{LoadOptions, Project, load_project, load_projects};
use wowlua_manifest::BuiltinProfile;
use wowlua_manifest::model::Manifest;
use wowlua_studio_app::system::{
    BuildDone, BuildEvent, BuildProfile, BuildStep, BuiltOutput, ChangeKind, DevBuild, DevEvent,
    InstallChange, InstallFolder, InstallState, PublishDone, PublishEvent, PublishOutcome,
    ShipBuild, ShipDiagnostic, ShipFacts, ShipInstall, ShipPublish, ShipTarget, TagPush, TestEvent,
    TestRequest, TokenState,
};

/// Where a job tells what happens, from its thread.
pub type Tell<E> = Arc<dyn Fn(E) + Send + Sync>;

fn ms(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn modified(path: &Path) -> Option<u64> {
    std::fs::symlink_metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .map(ms)
}

/// The manifest in `dir`: whether it names a package, whether it is a
/// workspace's root.
fn manifest_at(dir: &Path) -> (Option<String>, bool) {
    let value: Json = std::fs::read_to_string(dir.join("wowlua.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let name = value
        .pointer("/package/name")
        .and_then(Json::as_str)
        .map(str::to_owned);
    (name, value.get("workspace").is_some())
}

/// As the CLI loads the package of `dir`: at a workspace's root its own
/// package (`-p <it>`, S061), not its members; the lockfile kept up to
/// date, dependencies fetched when missing.
fn options(dir: &Path) -> LoadOptions {
    let (name, workspace) = manifest_at(dir);
    LoadOptions {
        cwd: dir.to_path_buf(),
        package: name.filter(|_| workspace),
        ..LoadOptions::default()
    }
}

/// A workspace without a package of its own: its addons, by name and
/// folder.
fn members(dir: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let mut session = Session::new();
    let options = LoadOptions {
        cwd: dir.to_path_buf(),
        lock_in_memory: true,
        offline: true,
        all_members: true,
        ..LoadOptions::default()
    };
    let projects = load_projects(&mut session, &options).ok_or_else(|| {
        crate::tests::first_error(&session).unwrap_or_else(|| "the workspace does not load".into())
    })?;
    Ok(projects
        .iter()
        .filter(|p| p.kind() == wowlua_manifest::model::PackageKind::Addon)
        .filter_map(|p| Some((p.manifest.package()?.name.clone(), p.root().to_path_buf())))
        .collect())
}

/// Runs `work` on a thread of its own with a large stack, kept from App
/// Nap like a test run (S054).
fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    let spawned = std::thread::Builder::new()
        .name(name.into())
        .stack_size(64 << 20)
        .spawn(move || crate::tests::user_initiated(work));
    if let Err(e) = spawned {
        eprintln!("wowlua studio: cannot start {name}: {e}");
    }
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// The session's diagnostics where they point, lints at their levels.
fn diagnostics(session: &mut Session, root: &Path) -> (bool, Vec<ShipDiagnostic>) {
    let taken = session.take_diagnostics();
    let fail = taken.should_fail(false);
    let list = taken
        .into_vec()
        .iter()
        .map(|d| {
            let at = d.labels.first().map(|l| {
                let file = session.db.get(l.span.file);
                (
                    file.path().to_path_buf(),
                    file.line_col(l.span.start as usize),
                )
            });
            let path = at
                .as_ref()
                .map(|(p, _)| p.clone())
                .or_else(|| d.path.clone())
                .map(|p| if p.is_relative() { root.join(p) } else { p });
            ShipDiagnostic {
                severity: d.severity.as_str().to_owned(),
                code: d.code.map(str::to_owned),
                message: d.message.clone(),
                path,
                line: at.as_ref().map(|(_, lc)| lc.line),
                column: at.as_ref().map(|(_, lc)| lc.column),
            }
        })
        .collect();
    (!fail, list)
}

// ------------------------------------------------------------------ facts

fn profiles(manifest: &Manifest) -> Vec<BuildProfile> {
    let mut names: Vec<String> = manifest.profiles.keys().cloned().collect();
    for builtin in ["dev", "release", "test"] {
        if !names.iter().any(|n| n == builtin) {
            names.push(builtin.into());
        }
    }
    names
        .into_iter()
        .map(|name| {
            let builtin = BuiltinProfile::from_name(&name).is_some();
            match wowlua_manifest::profile::resolve_value(manifest, &name) {
                Ok((chain, effective)) => BuildProfile {
                    name,
                    builtin,
                    chain,
                    effective,
                    problem: None,
                },
                Err(problem) => BuildProfile {
                    name: name.clone(),
                    builtin,
                    chain: vec![name],
                    effective: Json::Null,
                    problem: Some(problem.to_string()),
                },
            }
        })
        .collect()
}

/// A folder's files with their sizes, by path with `/`.
fn files_of(dir: &Path) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                stack.push(path);
            } else if let Ok(rel) = path.strip_prefix(dir) {
                let rel = rel.to_string_lossy().replace('\\', "/");
                if rel != install::INSTALL_MARKER {
                    out.push((rel, meta.len()));
                }
            }
        }
    }
    out.sort();
    out
}

fn installs(
    session: &mut Session,
    project: &Project,
    addon: &str,
) -> (Vec<InstallFolder>, Option<String>) {
    let folders = install::folders(session, project, addon, &InstallRequest::default());
    let problem = crate::tests::first_error(session);
    let list = folders
        .into_iter()
        .map(|(client, path)| {
            let state = if let Ok(target) = std::fs::read_link(&path) {
                InstallState::Linked(target)
            } else if !path.exists() {
                InstallState::Absent
            } else if !install::is_wowlua_install(&path) {
                InstallState::Foreign
            } else {
                InstallState::Copied(install::changes(&path).map(|changes| {
                    changes
                        .into_iter()
                        .map(|c| {
                            let at = modified(&path.join(c.path()));
                            let (path, kind) = match c {
                                install::Change::Changed { path, was, now } => {
                                    (path, ChangeKind::Changed { was, now })
                                }
                                install::Change::Added { path, size } => {
                                    (path, ChangeKind::Added { size })
                                }
                                install::Change::Removed { path } => (path, ChangeKind::Removed),
                            };
                            InstallChange {
                                path,
                                kind,
                                modified: at,
                            }
                        })
                        .collect()
                }))
            };
            InstallFolder {
                client,
                path,
                state,
            }
        })
        .collect();
    (list, problem)
}

fn targets(manifest: &Manifest) -> Vec<ShipTarget> {
    let Some(publish) = &manifest.publish else {
        return Vec::new();
    };
    publish
        .targets
        .iter()
        .map(|(name, t)| {
            let kind = serde_json::to_value(t.target_type)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default();
            let project = match serde_json::to_value(&t.project_id) {
                Ok(Json::String(s)) => s,
                Ok(Json::Number(n)) => n.to_string(),
                _ => String::new(),
            };
            ShipTarget {
                name: name.clone(),
                kind,
                project,
                profile: t.profile.clone().unwrap_or_else(|| "release".into()),
                token_env: t.token_env.clone(),
                enabled: t.enabled,
            }
        })
        .collect()
}

/// The tag a version gets: with a `v`, unless the repository's tags have
/// none.
fn tag_name(repo: &Path, version: &str) -> String {
    let tags = git(repo, &["tag", "--list"]).unwrap_or_default();
    let bare = tags
        .lines()
        .any(|t| t.starts_with(|c: char| c.is_ascii_digit()));
    let with_v = tags
        .lines()
        .any(|t| t.starts_with('v') && t[1..].starts_with(|c: char| c.is_ascii_digit()));
    if bare && !with_v {
        version.to_owned()
    } else {
        format!("v{version}")
    }
}

/// The CI workflow that runs `wowlua publish`.
fn ci_workflow(repo: &Path) -> Option<String> {
    let dir = repo.join(".github").join("workflows");
    let mut names: Vec<PathBuf> = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    names.sort();
    names.into_iter().find_map(|p| {
        let text = std::fs::read_to_string(&p).ok()?;
        let name = p.file_name()?.to_string_lossy().into_owned();
        text.contains("wowlua publish").then_some(name)
    })
}

/// The changelog file and this version's section, as a publish reads it.
fn changelog(project: &Project, repo: &Path, version: &str) -> Option<(PathBuf, String)> {
    let root = project.root();
    let configured = project
        .manifest
        .manifest
        .publish
        .as_ref()
        .and_then(|p| p.changelog.clone());
    let path = match configured {
        Some(Some(git)) if git == "git" => {
            let text =
                wowlua_driver::publish::changelog::from_git(repo, &project.addon_name(), version);
            return Some((repo.join(".git"), text));
        }
        Some(Some(file)) => root.join(file),
        Some(None) => return None,
        None => root.join("CHANGELOG.md"),
    };
    let text = std::fs::read_to_string(&path).ok()?;
    let section = wowlua_driver::publish::changelog::section(&text, version);
    Some((path, section))
}

pub fn read(root: &Path) -> Result<ShipFacts, String> {
    if manifest_at(root).0.is_none() {
        return Ok(ShipFacts {
            members: members(root)?,
            ..ShipFacts::default()
        });
    }
    let mut session = Session::new();
    let options = LoadOptions {
        lock_in_memory: true,
        offline: true,
        ..options(root)
    };
    let Some(project) = load_project(&mut session, &options) else {
        return Err(crate::tests::first_error(&session)
            .unwrap_or_else(|| "the package does not load".into()));
    };
    let manifest = &project.manifest.manifest;
    let addon = project.addon_name();
    let version = project
        .manifest
        .package()
        .map(|p| p.version.clone())
        .unwrap_or_default();
    let out = project.manifest.out_dir();
    let profiles = profiles(manifest);
    let outputs = profiles
        .iter()
        .filter_map(|p| {
            let dir = out.join(&p.name).join(&addon);
            dir.is_dir().then(|| BuiltOutput {
                profile: p.name.clone(),
                files: files_of(&dir),
                modified: modified(&dir),
                dir,
            })
        })
        .collect();
    let (installs, install_problem) = installs(&mut session, &project, &addon);
    let repo_dir = project
        .workspace_root
        .clone()
        .unwrap_or_else(|| project.root().to_path_buf());
    let repo = git(&repo_dir, &["rev-parse", "--show-toplevel"]).map(PathBuf::from);
    let (branch, remote, clean, tag, tag_at_head, ci) = match &repo {
        Some(repo) => {
            let branch = crate::native::repository_branch(repo);
            let remote = branch
                .as_ref()
                .and_then(|b| git(repo, &["config", &format!("branch.{b}.remote")]))
                .or_else(|| {
                    let remotes = git(repo, &["remote"])?;
                    remotes
                        .lines()
                        .find(|r| *r == "origin")
                        .or(remotes.lines().next())
                        .map(str::to_owned)
                });
            let clean = !wowlua_driver::package::dirty(
                &repo_dir,
                &wowlua_driver::package::out_root(&project),
            );
            let tag = tag_name(repo, &version);
            let at = git(
                repo,
                &[
                    "rev-parse",
                    "--verify",
                    "--quiet",
                    &format!("{tag}^{{commit}}"),
                ],
            );
            let head = git(repo, &["rev-parse", "HEAD"]);
            let tag_at_head = at.map(|c| Some(c) == head);
            (branch, remote, clean, tag, tag_at_head, ci_workflow(repo))
        }
        None => (None, None, false, format!("v{version}"), None, None),
    };
    let packager = manifest.compat.packager_keywords;
    let release = wowlua_driver::package::release_type(
        manifest.publish.as_ref().and_then(|p| p.release_type),
        Some(&tag),
        !clean,
        packager,
    )
    .as_str()
    .to_owned();
    let changelog = changelog(&project, repo.as_deref().unwrap_or(&repo_dir), &version);
    Ok(ShipFacts {
        members: Vec::new(),
        addon,
        version,
        manifest: project.manifest.path.clone(),
        out,
        profiles,
        outputs,
        installs,
        install_problem,
        targets: targets(manifest),
        release,
        repo,
        branch,
        remote,
        clean,
        tag,
        tag_at_head,
        changelog,
        ci,
    })
}

// ----------------------------------------------------------------- tokens

/// The environment variables a target type's token comes from, in order
/// (wowlua's publish, outside packager mode).
fn token_vars(kind: &str) -> &'static [&'static str] {
    match kind {
        "curseforge" => &["CF_API_TOKEN", "CF_API_KEY"],
        "wago" => &["WAGO_API_TOKEN"],
        "wowinterface" => &["WOWI_API_TOKEN"],
        "github" => &[
            "GITHUB_TOKEN",
            "GH_API_TOKEN",
            "GITHUB_API_TOKEN",
            "GITHUB_OAUTH",
        ],
        "registry" => &["WOWLUA_REGISTRY_TOKEN"],
        _ => &[],
    }
}

/// Each target's token's state, by the target's name.
pub fn tokens(targets: &[ShipTarget]) -> Vec<(String, TokenState)> {
    let set = |v: &str| std::env::var(v).is_ok_and(|v| !v.is_empty());
    let mut read: HashMap<String, TokenState> = HashMap::new();
    targets
        .iter()
        .map(|t| {
            if let Some(var) = &t.token_env {
                let state = if set(var) {
                    TokenState::FromEnv(var.clone())
                } else {
                    TokenState::Missing
                };
                return (t.name.clone(), state);
            }
            if let Some(var) = token_vars(&t.kind).iter().find(|v| set(v)) {
                return (t.name.clone(), TokenState::FromEnv((*var).into()));
            }
            let state = read
                .entry(t.kind.clone())
                .or_insert_with(|| match wowlua_driver::credentials::get(&t.kind) {
                    Ok(_) => TokenState::Saved,
                    Err(wowlua_driver::credentials::Unread::Missing) => TokenState::Missing,
                    Err(wowlua_driver::credentials::Unread::NoTerminal) => {
                        TokenState::Unreadable("the OS credential store was not read".into())
                    }
                })
                .clone();
            (t.name.clone(), state)
        })
        .collect()
}

/// Saves a token after the site's check, as `wowlua login` does, or
/// removes it: where it went, or why not.
pub fn token(kind: &str, token: Option<&str>) -> Result<String, String> {
    use wowlua_driver::credentials::{self, Saved, Verified};
    let Some(token) = token.map(str::trim).filter(|t| !t.is_empty()) else {
        return match credentials::remove(kind)? {
            true => Ok("removed".into()),
            false => Ok("none was saved".into()),
        };
    };
    let checked = match credentials::verify(kind, token) {
        Verified::Accepted => " · the site accepted it",
        Verified::Unchecked(_) => "",
        Verified::Rejected(why) => return Err(format!("the site rejected it: {why}")),
    };
    let place = match credentials::set(kind, token)? {
        Saved::CredentialStore => "in the OS credential store",
        Saved::Config | Saved::ConfigNoTerminal => "in ~/.wowlua/config.json",
    };
    Ok(format!("saved {place}{checked}"))
}

// ------------------------------------------------------------------ build

/// wowlua's progress as the page's.
fn progress(tell: Tell<BuildEvent>) -> wowlua_driver::build::Progress {
    use wowlua_driver::build::Step;
    wowlua_driver::build::Progress(Arc::new(move |step| {
        let step = match step {
            Step::Read { files } => BuildStep::Read { files },
            Step::Parsed { done, of } => BuildStep::Parsed { done, of },
            Step::Linted { done, of } => BuildStep::Linted { done, of },
            Step::Checking {
                flavor,
                index,
                count,
            } => BuildStep::Checking {
                flavor,
                index,
                count,
            },
            Step::Bundling => BuildStep::Bundling,
            Step::Writing => BuildStep::Writing,
        };
        tell(BuildEvent::Step(step))
    }))
}

fn install_request(install: ShipInstall) -> InstallRequest {
    InstallRequest {
        link: install.link,
        force: install.force,
        ..InstallRequest::default()
    }
}

/// Builds a profile on a thread, then installs it when asked.
pub fn build(root: PathBuf, build: ShipBuild, tell: Tell<BuildEvent>) {
    spawn("build", move || {
        let started = Instant::now();
        let mut session = Session::new();
        let mut done = BuildDone {
            ok: false,
            profile: build.profile.clone(),
            dir: None,
            ms: 0,
            diagnostics: Vec::new(),
            installed: Vec::new(),
        };
        if let Some(project) = load_project(&mut session, &options(&root)) {
            let mut request = wowlua_driver::BuildRequest::new(build.profile.clone());
            request.progress = Some(progress(tell.clone()));
            let built = wowlua_driver::build(&mut session, &project, &request);
            project.apply_lints(&mut session);
            let failed = session.diagnostics.iter().any(|d| d.is_error());
            if let (Some(built), false) = (&built, failed) {
                done.dir = Some(built.dir.clone());
                if let Some(how) = build.install {
                    tell(BuildEvent::Step(BuildStep::Installing));
                    done.installed =
                        install::install(&mut session, &project, built, &install_request(how))
                            .into_iter()
                            .map(|i| i.path)
                            .collect();
                }
            }
            done.ok = built.is_some();
        }
        let (ok, diagnostics) = diagnostics(&mut session, &root);
        done.ok &= ok;
        done.diagnostics = diagnostics;
        done.ms = started.elapsed().as_millis() as u64;
        tell(BuildEvent::Done(done));
    });
}

// --------------------------------------------------------------- dev loop

/// The running dev loop: set to stop it.
#[derive(Default)]
pub struct Dev {
    stop: Option<Arc<AtomicBool>>,
}

impl Dev {
    pub fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
    }

    /// Builds the dev profile and installs it, then again after every
    /// batch of changes on disk (spec CLI §12.5, as `wowlua dev`).
    pub fn start(&mut self, root: PathBuf, how: ShipInstall, tell: Tell<DevEvent>) {
        self.stop();
        let stop = Arc::new(AtomicBool::new(false));
        self.stop = Some(stop.clone());
        spawn("dev", move || {
            let why = dev_loop(&root, how, &stop, &tell);
            tell(DevEvent::Stopped(why));
        });
    }
}

impl Drop for Dev {
    fn drop(&mut self) {
        self.stop();
    }
}

/// One build of the dev loop, installed (the first only, when linked).
fn dev_build(
    root: &Path,
    how: ShipInstall,
    cache: &mut wowlua_driver::BuildCache,
    first: bool,
    changed: Vec<String>,
) -> (DevBuild, Option<PathBuf>) {
    let at = ms(SystemTime::now());
    let started = Instant::now();
    let mut session = Session::new();
    let mut synced = Vec::new();
    let mut out_dir = None;
    let mut built_ok = false;
    if let Some(project) = load_project(&mut session, &options(root)) {
        out_dir = Some(project.manifest.out_dir());
        let request = wowlua_driver::BuildRequest::new("dev");
        let built = wowlua_driver::build_with_cache(&mut session, &project, &request, cache);
        project.apply_lints(&mut session);
        let failed = session.diagnostics.iter().any(|d| d.is_error());
        if let (Some(built), false) = (&built, failed) {
            built_ok = true;
            if first || !how.link {
                synced = install::install(&mut session, &project, built, &install_request(how))
                    .into_iter()
                    .map(|i| i.path)
                    .collect();
            }
        }
    }
    let (ok, diagnostics) = diagnostics(&mut session, root);
    let clock = jiff::Zoned::now().strftime("%H:%M:%S").to_string();
    let build = DevBuild {
        at,
        clock,
        changed,
        ok: ok && built_ok,
        ms: started.elapsed().as_millis() as u64,
        diagnostics,
        synced,
    };
    (build, out_dir)
}

fn dev_loop(
    root: &Path,
    how: ShipInstall,
    stop: &AtomicBool,
    tell: &Tell<DevEvent>,
) -> Option<String> {
    let mut cache = wowlua_driver::BuildCache::default();
    tell(DevEvent::Building(Vec::new()));
    let (first, out_dir) = dev_build(root, how, &mut cache, true, Vec::new());
    tell(DevEvent::Built(first));
    let out_dir = out_dir.unwrap_or_else(|| root.join("dist"));
    let (tx, rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = match notify::recommended_watcher(tx) {
        Ok(w) => w,
        Err(e) => return Some(format!("cannot watch files: {e}")),
    };
    if let Err(e) = watcher.watch(root, RecursiveMode::Recursive) {
        return Some(format!("cannot watch `{}`: {e}", root.display()));
    }
    let name = root
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    tell(DevEvent::Watching(vec![name]));
    let mut seen: HashMap<PathBuf, Option<(Option<SystemTime>, u64)>> = HashMap::new();
    let add = |result: notify::Result<notify::Event>, into: &mut Vec<PathBuf>| {
        if let Ok(event) = result
            && !matches!(event.kind, EventKind::Access(_))
        {
            into.extend(event.paths);
        }
    };
    while !stop.load(Ordering::Relaxed) {
        let first = match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(r) => r,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => return Some("the watcher stopped".into()),
        };
        let mut paths = Vec::new();
        add(first, &mut paths);
        while let Ok(result) = rx.recv_timeout(Duration::from_millis(300)) {
            add(result, &mut paths);
        }
        paths.sort();
        paths.dedup();
        let mut changed = Vec::new();
        for path in paths {
            let s = path.to_string_lossy();
            // (a folder that holds the output folder changes with it)
            if path.starts_with(&out_dir)
                || out_dir.starts_with(&path)
                || path.components().any(|c| c.as_os_str() == ".git")
                || s.contains(".wowlua-tmp")
                || s.contains(".wowlua-old")
            {
                continue;
            }
            let now = std::fs::metadata(&path)
                .ok()
                .map(|m| (m.modified().ok(), m.len()));
            let unknown = now.is_some_and(|(m, _)| m.is_none());
            if seen.insert(path.clone(), now) != Some(now) || unknown {
                let rel = path.strip_prefix(root).unwrap_or(&path);
                changed.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
        if changed.is_empty() || stop.load(Ordering::Relaxed) {
            continue;
        }
        tell(DevEvent::Building(changed.clone()));
        let (built, _) = dev_build(root, how, &mut cache, false, changed);
        tell(DevEvent::Built(built));
    }
    None
}

// ---------------------------------------------------------------- publish

fn library_manifest(root: &Path) -> bool {
    let Some(path) = wowlua_manifest::find_manifest(root, None) else {
        return false;
    };
    let value: Json = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    matches!(
        value.pointer("/package/kind").and_then(Json::as_str),
        Some("library" | "lua-library" | "types")
    )
}

/// Publishes on a thread as `wowlua publish --tag <tag>` does, or its dry
/// run; the tests run first unless left out.
pub fn publish(root: PathBuf, ask: ShipPublish, tell: Tell<PublishEvent>) {
    spawn("publish", move || {
        let mut session = Session::new();
        // A publish never changes the lockfile (§11.1.1).
        let library = library_manifest(&root);
        let options = LoadOptions {
            lock_in_memory: library,
            locked: !library,
            bundled: true,
            ..options(&root)
        };
        let mut done = PublishDone {
            ok: false,
            dry_run: ask.dry_run,
            outcomes: Vec::new(),
            notes: Vec::new(),
            diagnostics: Vec::new(),
        };
        if let Some(projects) = load_projects(&mut session, &options) {
            let line = tell.clone();
            let status = wowlua_driver::Status(std::rc::Rc::new(move |label: &str, text: &str| {
                line(PublishEvent::Line {
                    label: label.into(),
                    text: text.into(),
                })
            }));
            let tests = ask.tests.then(|| {
                let root = root.clone();
                let tell = tell.clone();
                wowlua_driver::publish::Tests(std::rc::Rc::new(move || tests_pass(&root, &tell)))
            });
            let request = wowlua_driver::publish::PublishRequest {
                package: wowlua_driver::package::PackageRequest {
                    targets: ask.targets.clone(),
                    ..Default::default()
                },
                dry_run: ask.dry_run,
                tag: Some(ask.tag.clone()),
                deny_warnings: false,
                status: Some(status),
                tests,
            };
            let report = wowlua_driver::publish::publish(&mut session, &projects, &request);
            for project in &projects {
                project.apply_lints(&mut session);
            }
            if let Some(report) = report {
                done.ok = report.ok();
                done.notes = report.notes.clone();
                done.outcomes = report
                    .outcomes
                    .iter()
                    .map(|o| PublishOutcome {
                        target: o.target.clone(),
                        file: o.file.clone(),
                        ok: o.ok,
                        skipped: o.skipped,
                        message: o.message.clone(),
                    })
                    .collect();
            }
        }
        let (ok, diagnostics) = diagnostics(&mut session, &root);
        done.ok &= ok;
        done.diagnostics = diagnostics;
        tell(PublishEvent::Done(done));
    });
}

/// The workspace's tests, as `wowlua test` runs them: whether they pass.
fn tests_pass(root: &Path, tell: &Tell<PublishEvent>) -> bool {
    let line = |text: String| {
        tell(PublishEvent::Line {
            label: "Testing".into(),
            text,
        })
    };
    line("the workspace's tests".into());
    let failed = std::sync::atomic::AtomicUsize::new(0);
    let files = std::sync::atomic::AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let done = crate::tests::run(root, &TestRequest::default(), &stop, &|event| {
        if let TestEvent::File(file) = event {
            files.fetch_add(1, Ordering::Relaxed);
            if file.failed() {
                failed.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
    let (failed, files) = (failed.into_inner(), files.into_inner());
    if let Some(error) = done.error {
        line(format!("cannot run: {error}"));
        return false;
    }
    line(if failed == 0 {
        format!("{files} files passed")
    } else {
        format!("{failed} of {files} files failed")
    });
    failed == 0
}

// ------------------------------------------------------------ tag, push

/// Runs the confirmed commands in the repository: git's output, or why
/// not.
pub fn tag_and_push(root: &Path, tag: &TagPush) -> Result<String, String> {
    let mut said = String::new();
    for command in tag.commands() {
        let out = Command::new(&command[0])
            .args(&command[1..])
            .current_dir(root)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| format!("cannot run git: {e}"))?;
        said.push_str(&String::from_utf8_lossy(&out.stdout));
        said.push_str(&String::from_utf8_lossy(&out.stderr));
        if !out.status.success() {
            return Err(format!("`{}` failed: {}", command.join(" "), said.trim()));
        }
    }
    Ok(said.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_follow_the_repository() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        let run = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(repo)
                .output()
                .unwrap()
        };
        run(&["init", "-q"]);
        assert_eq!(tag_name(repo, "1.2.0"), "v1.2.0", "no tags: with a v");
        run(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "one",
        ]);
        run(&["tag", "1.0.0"]);
        assert_eq!(tag_name(repo, "1.2.0"), "1.2.0", "bare tags: bare");
        run(&["tag", "v1.1.0"]);
        assert_eq!(tag_name(repo, "1.2.0"), "v1.2.0");
    }

    /// A package with a publish target (its token in a variable no one
    /// sets: nothing reaches the site), a changelog, committed.
    fn package() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let manifest = serde_json::json!({
            "package": { "name": "shiptest", "version": "1.0.0" },
            "profiles": { "readable": { "inherits": "release", "minify": "none" } },
            "publish": { "targets": { "curseforge": {
                "type": "curseforge", "project-id": 1234, "profile": "readable",
                "token-env": "WOWLUA_STUDIO_SHIP_TEST_NO_TOKEN"
            } } }
        });
        std::fs::write(root.join("wowlua.json"), manifest.to_string()).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.wowlua"), "local a = 1\nprint(a)\n").unwrap();
        std::fs::write(
            root.join("CHANGELOG.md"),
            "# Changelog\n\n## 1.0.0 (2026-10-28)\n\n### New\n- The first.\n",
        )
        .unwrap();
        std::fs::write(root.join(".gitignore"), "dist/\n").unwrap();
        // (its lockfile, as the CLI writes it: a publish needs it)
        assert!(load_project(&mut Session::new(), &options(root)).is_some());
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@t"])
                .args(args)
                .current_dir(root)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {out:?}");
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "one"]);
        dir
    }

    /// Waits for a job's last event.
    fn wait<E: Send + 'static>(start: impl FnOnce(Tell<E>), done: impl Fn(&E) -> bool) -> Vec<E> {
        let (send, receive) = std::sync::mpsc::channel();
        let send = std::sync::Mutex::new(send);
        start(Arc::new(move |e| {
            let _ = send.lock().unwrap().send(e);
        }));
        let mut events = Vec::new();
        while let Ok(e) = receive.recv_timeout(Duration::from_secs(120)) {
            let last = done(&e);
            events.push(e);
            if last {
                break;
            }
        }
        events
    }

    #[test]
    fn the_facts_of_a_package() {
        let dir = package();
        let facts = read(dir.path()).unwrap();
        assert!(facts.members.is_empty());
        assert_eq!(facts.version, "1.0.0");
        assert!(!facts.addon.is_empty());
        let readable = facts
            .profiles
            .iter()
            .find(|p| p.name == "readable")
            .unwrap();
        assert_eq!(readable.chain, ["release", "readable"]);
        assert_eq!(readable.effective["minify"], "none");
        assert_eq!(readable.effective["bundle"], true, "release's");
        for builtin in ["dev", "release", "test"] {
            assert!(
                facts
                    .profiles
                    .iter()
                    .any(|p| p.name == builtin && p.builtin)
            );
        }
        let target = &facts.targets[0];
        assert_eq!(
            (
                target.kind.as_str(),
                target.project.as_str(),
                target.profile.as_str()
            ),
            ("curseforge", "1234", "readable")
        );
        assert_eq!(facts.tag, "v1.0.0");
        assert_eq!(facts.tag_at_head, None);
        assert!(facts.clean, "committed");
        assert_eq!(facts.branch.as_deref(), Some("main"));
        let (_, section) = facts.changelog.unwrap();
        assert!(section.contains("The first."), "{section}");
        assert!(facts.outputs.is_empty(), "not built");
        let tokens = tokens(&facts.targets);
        assert_eq!(tokens, [("curseforge".to_owned(), TokenState::Missing)]);
    }

    #[test]
    fn a_build_tells_its_progress() {
        let dir = package();
        let root = dir.path().to_path_buf();
        let events = wait(
            |tell| {
                build(
                    root.clone(),
                    ShipBuild {
                        profile: "dev".into(),
                        install: None,
                    },
                    tell,
                )
            },
            |e| matches!(e, BuildEvent::Done(_)),
        );
        let Some(BuildEvent::Done(done)) = events.last() else {
            panic!("{events:?}");
        };
        assert!(done.ok, "{:?}", done.diagnostics);
        assert!(done.dir.as_ref().is_some_and(|d| d.is_dir()));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, BuildEvent::Step(BuildStep::Read { files: 1 })))
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, BuildEvent::Step(BuildStep::Writing)))
        );
        // The output is in the facts now.
        let facts = read(&root).unwrap();
        assert!(
            facts
                .outputs
                .iter()
                .any(|o| o.profile == "dev" && !o.files.is_empty())
        );
    }

    #[test]
    fn a_dry_run_uploads_nothing() {
        let dir = package();
        let root = dir.path().to_path_buf();
        let events = wait(
            |tell| {
                publish(
                    root.clone(),
                    ShipPublish {
                        dry_run: true,
                        targets: Vec::new(),
                        tests: false,
                        tag: "v1.0.0".into(),
                    },
                    tell,
                )
            },
            |e| matches!(e, PublishEvent::Done(_)),
        );
        let Some(PublishEvent::Done(done)) = events.last() else {
            panic!("{events:?}");
        };
        assert!(done.dry_run);
        assert_eq!(done.outcomes.len(), 1, "{done:?}");
        assert_eq!(done.outcomes[0].target, "curseforge");
        assert!(
            events
                .iter()
                .any(|e| matches!(e, PublishEvent::Line { label, .. } if label == "Packaged")),
            "{events:?}"
        );
        // A dry run makes no tag.
        assert_eq!(git(&root, &["tag", "--list"]).as_deref(), Some(""));
    }

    #[test]
    fn tag_and_push_says_what_failed() {
        let dir = tempfile::tempdir().unwrap();
        let tag = TagPush {
            tag: "v1.0.0".into(),
            message: "x 1.0.0".into(),
            remote: "origin".into(),
            branch: "main".into(),
        };
        // (not a repository)
        let err = tag_and_push(dir.path(), &tag).unwrap_err();
        assert!(
            err.starts_with("`git tag -a v1.0.0 -m x 1.0.0` failed"),
            "{err}"
        );
    }
}
