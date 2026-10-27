//! The system Studio runs on, natively: the disk, the system's folder
//! picker, Git through the `git` program (S027), wowlua's library, and
//! Studio itself for new windows (S031). Slow work runs on a thread of its
//! own and comes back through a channel.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui;
use notify::{RecursiveMode, Watcher};
use wowlua_driver::project::{LoadOptions, find_manifest, load_projects};
use wowlua_driver::scaffold::{InitRequest, NewRequest};
use wowlua_manifest::model::{CheckMode, PackageKind};
use wowlua_studio_app::system::{
    Adopt, AdoptPreview, ClientFolder, Entry, Event, FileChange, Highlight, Installation, Member,
    NewAddon, Opened, PackageNames, Syntax, System, UiState, Workspace,
};

/// Folders the explorer and the watcher leave out: build output and the
/// tools' own.
const SKIPPED: &[&str] = &["dist", "target", "node_modules"];
/// Files and folders no one edits: Git's own and the Finder's. Other dot
/// files show (`.github`, `.pkgmeta`).
const HIDDEN: &[&str] = &[".git", ".DS_Store"];

pub struct NativeSystem {
    send: Sender<Event>,
    events: Receiver<Event>,
    ctx: egui::Context,
    watcher: RefCell<Option<notify::RecommendedWatcher>>,
    /// The language server's thread, once started.
    lsp: RefCell<Option<Sender<crate::lsp::Input>>>,
}

impl NativeSystem {
    pub fn new(ctx: &egui::Context) -> Self {
        let (send, events) = channel();
        NativeSystem {
            send,
            events,
            ctx: ctx.clone(),
            watcher: RefCell::new(None),
            lsp: RefCell::new(None),
        }
    }

    /// Runs `work` on a thread of its own; its event wakes Studio up.
    fn spawn(&self, work: impl FnOnce() -> Event + Send + 'static) {
        let send = self.send.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let _ = send.send(work());
            ctx.request_repaint();
        });
    }

    fn post(&self, event: Event) {
        let _ = self.send.send(event);
        self.ctx.request_repaint();
    }
}

impl System for NativeSystem {
    fn now(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }

    fn home(&self) -> Option<PathBuf> {
        dirs::home_dir()
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn pick_folder(&self, title: &str) -> Option<PathBuf> {
        rfd::FileDialog::new().set_title(title).pick_folder()
    }

    fn list(&self, dir: &Path) -> Vec<Entry> {
        let Ok(read) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut entries: Vec<Entry> = read
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let is_dir = e.file_type().ok()?.is_dir();
                let hidden =
                    HIDDEN.contains(&name.as_str()) || (is_dir && SKIPPED.contains(&name.as_str()));
                (!hidden).then(|| Entry {
                    is_dir,
                    name,
                    path: e.path(),
                })
            })
            .collect();
        // Folders first, then by name.
        entries.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        entries
    }

    fn read_file(&self, path: &Path) -> Result<String, String> {
        let bytes =
            std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        String::from_utf8(bytes).map_err(|_| format!("{} is not UTF-8 text", path.display()))
    }

    fn write_file(&self, path: &Path, text: &str) -> Result<(), String> {
        // Written beside the file, then moved over it: a crash never leaves
        // half a file.
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let temp = path.with_file_name(format!(".{name}.wowlua-studio"));
        std::fs::write(&temp, text)
            .and_then(|()| std::fs::rename(&temp, path))
            .map_err(|e| {
                let _ = std::fs::remove_file(&temp);
                format!("cannot write {}: {e}", path.display())
            })
    }

    fn highlight(&self, text: &str) -> Vec<Highlight> {
        highlight(text)
    }

    fn open(&self, path: &Path) {
        let path = path.to_path_buf();
        self.spawn(move || Event::Opened(open_workspace(&path)));
    }

    fn watch(&self, root: Option<&Path>) {
        let Some(root) = root else {
            *self.watcher.borrow_mut() = None;
            return;
        };
        let send = self.send.clone();
        let ctx = self.ctx.clone();
        let watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
            let Ok(event) = result else { return };
            let paths: Vec<PathBuf> = event
                .paths
                .into_iter()
                // Git's own files stay out, but `HEAD`: a checkout (S049).
                .filter(|p| {
                    p.ends_with(".git/HEAD")
                        || !p
                            .components()
                            .any(|c| skipped(&c.as_os_str().to_string_lossy()))
                })
                .collect();
            if !paths.is_empty() {
                let _ = send.send(Event::FilesChanged(paths));
                ctx.request_repaint();
            }
        });
        let watcher = watcher.and_then(|mut w| w.watch(root, RecursiveMode::Recursive).map(|()| w));
        match watcher {
            Ok(w) => *self.watcher.borrow_mut() = Some(w),
            Err(e) => eprintln!("wowlua studio: cannot watch {}: {e}", root.display()),
        }
    }

    fn find_installation(&self) {
        self.spawn(|| Event::Installation(installation()));
    }

    fn set_installation(&self, path: &Path) {
        if let Err(e) = write_wow_path(path) {
            self.post(Event::Installation(Installation {
                problem: Some(format!(
                    "could not keep the installation in wowlua's config: {e}"
                )),
                ..installation()
            }));
            return;
        }
        self.find_installation();
    }

    fn check_ui(&self, flavor: &str) {
        let flavor = flavor.to_owned();
        self.spawn(move || {
            let state = if wowlua_test::cached_ui_source(&flavor).is_some() {
                UiState::Cached
            } else {
                UiState::Missing
            };
            Event::Ui { flavor, state }
        });
    }

    fn download_ui(&self, flavor: &str) {
        self.post(Event::Ui {
            flavor: flavor.to_owned(),
            state: UiState::Downloading,
        });
        let flavor = flavor.to_owned();
        self.spawn(move || {
            let state = match wowlua_test::ui_source(&flavor, &|_| {}) {
                Ok(_) => UiState::Cached,
                Err(e) => UiState::Failed(e),
            };
            Event::Ui { flavor, state }
        });
    }

    fn names(&self, name: &str) -> PackageNames {
        let package = wowlua_driver::scaffold::kebab_case(name);
        PackageNames {
            addon: wowlua_manifest::pascal_case(&package),
            package,
        }
    }

    fn in_git(&self, dir: &Path) -> bool {
        // The folder may not be made yet: the nearest one that is.
        let mut dir = dir;
        while !dir.is_dir() {
            match dir.parent() {
                Some(parent) => dir = parent,
                None => return false,
            }
        }
        wowlua_driver::vcs::inside_work_tree(dir)
    }

    fn new_addon(&self, request: NewAddon) {
        self.spawn(move || Event::Created(new_addon(&request)));
    }

    fn adopt_preview(&self, dir: &Path) -> AdoptPreview {
        let mut preview = AdoptPreview {
            has_manifest: dir.join(wowlua_manifest::MANIFEST_FILE).exists(),
            pkgmeta: dir.join(".pkgmeta").exists() || dir.join("pkgmeta.yaml").exists(),
            git: wowlua_driver::vcs::inside_work_tree(dir),
            ..AdoptPreview::default()
        };
        for entry in self.list(dir) {
            if !entry.is_dir && entry.name.to_lowercase().ends_with(".toc") {
                preview.tocs.push(entry.name);
            }
        }
        preview.sources = count_sources(dir);
        preview
    }

    fn adopt(&self, request: Adopt) {
        self.spawn(move || Event::Adopted(adopt(&request)));
    }

    fn open_window(&self, path: Option<&Path>) {
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let mut command = Command::new(exe);
        if let Some(path) = path {
            command.arg(path);
        }
        if let Err(e) = command.spawn() {
            eprintln!("wowlua studio: cannot open a new window: {e}");
        }
    }

    fn whats_new(&self) -> Vec<String> {
        wowlua_driver::release_notes(wowlua_driver::COMPILER_VERSION)
            .map(new_lines)
            .unwrap_or_default()
    }

    fn branch(&self, dir: &Path) -> Option<String> {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .ok()?;
        let branch = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        (out.status.success() && !branch.is_empty()).then_some(branch)
    }

    fn canonical(&self, path: &Path) -> PathBuf {
        std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }

    fn lsp_send(&self, message: serde_json::Value) {
        let mut lsp = self.lsp.borrow_mut();
        let sender =
            lsp.get_or_insert_with(|| crate::lsp::start(self.send.clone(), self.ctx.clone()));
        if sender
            .send(crate::lsp::Input::Message(message.clone()))
            .is_err()
        {
            // The thread ended (`exit`): a new one.
            let fresh = crate::lsp::start(self.send.clone(), self.ctx.clone());
            let _ = fresh.send(crate::lsp::Input::Message(message));
            *lsp = Some(fresh);
        }
    }

    fn lsp_files(&self, changes: Vec<(PathBuf, FileChange)>) {
        if let Some(sender) = self.lsp.borrow().as_ref() {
            let _ = sender.send(crate::lsp::Input::Files(changes));
        }
    }

    fn lsp_stop(&self) {
        // Dropping the channel ends the thread.
        self.lsp.borrow_mut().take();
    }

    fn poll(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }
}

fn skipped(name: &str) -> bool {
    HIDDEN.contains(&name) || SKIPPED.contains(&name)
}

/// Opens the workspace around `path` through wowlua's driver, as the
/// language server loads it: every member, dependencies neither resolved
/// nor fetched, the lockfile left as it is.
fn open_workspace(path: &Path) -> Opened {
    let Some(manifest) = find_manifest(path) else {
        return Opened::NoManifest(path.to_path_buf());
    };
    let mut session = wowlua_driver::Session::new();
    let options = LoadOptions {
        cwd: path.to_path_buf(),
        lock_in_memory: true,
        offline: true,
        skip_dependencies: true,
        all_members: true,
        ..LoadOptions::default()
    };
    if let Some(projects) = load_projects(&mut session, &options) {
        return Opened::Workspace(workspace_of(&projects));
    }
    let errors: Vec<_> = session.diagnostics.iter().cloned().collect();
    // The package's `wowlua` requirement excludes this wowlua: it opens
    // with checks off (the design's "can't be checked").
    if let Some(e4300) = errors.iter().find(|d| d.code == Some("E4300"))
        && let Some(workspace) = raw_workspace(&manifest, &e4300.message)
    {
        return Opened::Workspace(workspace);
    }
    let message = errors
        .first()
        .map(|d| match d.code {
            Some(code) => format!("error[{code}]: {}", d.message),
            None => format!("error: {}", d.message),
        })
        .unwrap_or_else(|| "the workspace could not be loaded".into());
    Opened::Failed {
        path: path.to_path_buf(),
        message,
    }
}

fn workspace_of(projects: &[wowlua_driver::project::Project]) -> Workspace {
    let root = projects
        .iter()
        .find_map(|p| p.workspace_root.clone())
        .or_else(|| projects.first().map(|p| p.manifest.root.clone()))
        .unwrap_or_default();
    let mut members: Vec<Member> = projects
        .iter()
        .map(|p| {
            let package = p.manifest.package();
            Member {
                name: package.map_or_else(String::new, |p| p.name.clone()),
                kind: package.map_or_else(|| "addon".into(), |p| kind_name(p.kind).into()),
                version: package.map_or_else(String::new, |p| p.version.clone()),
                rel: p
                    .manifest
                    .root
                    .strip_prefix(&root)
                    .map(|r| r.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default(),
                dir: p.manifest.root.clone(),
            }
        })
        .collect();
    // The root package first, then the members by folder.
    members.sort_by(|a, b| (!a.rel.is_empty(), &a.rel).cmp(&(!b.rel.is_empty(), &b.rel)));
    let first = projects
        .iter()
        .find(|p| p.manifest.root == root)
        .or(projects.first());
    Workspace {
        name: members.first().map(|m| m.name.clone()).unwrap_or_default(),
        flavors: first
            .map(|p| p.manifest.flavors.clone())
            .unwrap_or_default(),
        root,
        members,
        cannot_check: None,
    }
}

/// A workspace whose manifest the driver refused (E4300), read as plain
/// JSON: its package and the members its `workspace.members` names.
fn raw_workspace(manifest: &Path, why: &str) -> Option<Workspace> {
    let root = manifest.parent()?.to_path_buf();
    let read = |path: &Path| -> Option<serde_json::Value> {
        serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
    };
    let value = read(manifest)?;
    let member_of = |dir: &Path, value: &serde_json::Value| Member {
        name: value
            .pointer("/package/name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned(),
        kind: value
            .pointer("/package/kind")
            .and_then(|v| v.as_str())
            .unwrap_or("addon")
            .to_owned(),
        version: value
            .pointer("/package/version")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned(),
        rel: dir
            .strip_prefix(&root)
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default(),
        dir: dir.to_path_buf(),
    };
    let mut members = vec![member_of(&root, &value)];
    let patterns = value
        .pointer("/workspace/members")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let set = wowlua_driver::glob_set(
        &patterns
            .iter()
            .filter_map(|p| p.as_str().map(str::to_owned))
            .collect::<Vec<_>>(),
    );
    for entry in walkdir_dirs(&root) {
        let rel = entry
            .strip_prefix(&root)
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        if !rel.is_empty()
            && set.is_match(&rel)
            && let Some(v) = read(&entry.join(wowlua_manifest::MANIFEST_FILE))
        {
            members.push(member_of(&entry, &v));
        }
    }
    let flavors = value
        .get("flavors")
        .and_then(|v| v.as_object())
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    Some(Workspace {
        name: members[0].name.clone(),
        root,
        members,
        flavors,
        cannot_check: Some(why.to_owned()),
    })
}

/// The folders under `root`, two levels deep (`libs/*`, `Examples/*`).
fn walkdir_dirs(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() && !skipped(&name) {
                out.push(path.clone());
                if depth < 2 {
                    stack.push((path, depth + 1));
                }
            }
        }
    }
    out
}

fn kind_name(kind: PackageKind) -> &'static str {
    match kind {
        PackageKind::Addon => "addon",
        PackageKind::Library => "library",
        PackageKind::LuaLibrary => "lua-library",
        PackageKind::Types => "types",
    }
}

/// The installation as `wowlua install` finds it, with its client folders.
fn installation() -> Installation {
    match wowlua_driver::install::find_wow(None) {
        Ok(path) => {
            let mut clients = Vec::new();
            if let Ok(read) = std::fs::read_dir(&path) {
                let mut folders: Vec<String> = read
                    .flatten()
                    .filter(|e| e.path().is_dir())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.starts_with('_') && n.ends_with('_'))
                    .collect();
                folders.sort();
                for folder in folders {
                    let client = wowlua_flavors::table().clients.iter().find(|c| {
                        c.folders.iter().any(|f| {
                            f.trim_matches('_')
                                .eq_ignore_ascii_case(folder.trim_matches('_'))
                        })
                    });
                    if let Some(client) = client {
                        clients.push(ClientFolder {
                            folder,
                            client: client.name.clone(),
                            flavor: client.flavor.clone(),
                        });
                    }
                }
            }
            Installation {
                path: Some(path),
                clients,
                problem: None,
            }
        }
        Err(d) => Installation {
            path: None,
            clients: Vec::new(),
            problem: Some(match d.help {
                Some(help) => format!("{} ({help})", d.message),
                None => d.message,
            }),
        },
    }
}

/// Keeps the installation in `wow.path` of wowlua's config (S028), leaving
/// the rest of the file as it is.
fn write_wow_path(path: &Path) -> std::io::Result<()> {
    let home = wowlua_driver::install::wowlua_home()
        .ok_or_else(|| std::io::Error::other("no home folder for wowlua's config"))?;
    let file = home.join("config.json");
    let mut config: serde_json::Value = std::fs::read_to_string(&file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let object = config
        .as_object_mut()
        .ok_or_else(|| std::io::Error::other("~/.wowlua/config.json is not a JSON object"))?;
    let wow = object.entry("wow").or_insert_with(|| serde_json::json!({}));
    let wow = wow
        .as_object_mut()
        .ok_or_else(|| std::io::Error::other("`wow` in ~/.wowlua/config.json is not an object"))?;
    wow.insert(
        "path".into(),
        serde_json::Value::String(path.to_string_lossy().into_owned()),
    );
    std::fs::create_dir_all(&home)?;
    let mut text = serde_json::to_string_pretty(&config).map_err(std::io::Error::other)?;
    text.push('\n');
    std::fs::write(&file, text)
}

fn new_addon(request: &NewAddon) -> Result<PathBuf, String> {
    let kind = match request.kind.as_str() {
        "library" => PackageKind::Library,
        "lua-library" => PackageKind::LuaLibrary,
        "types" => PackageKind::Types,
        _ => PackageKind::Addon,
    };
    let check = match request.check.as_str() {
        "strict" => Some(CheckMode::Strict),
        "nocheck" => Some(CheckMode::Nocheck),
        _ => None,
    };
    let new = NewRequest {
        path: request.path.clone(),
        kind,
        name: request.name.clone(),
        flavors: request.flavors.clone(),
        ci_github: request.ci_github,
        check,
        git: request.git,
        world_profile: request.world_profile,
    };
    wowlua_driver::scaffold::new_package(&new)
        .map(|_| request.path.clone())
        .map_err(|d| d.message)
}

/// Adopts the addon: on a new branch when asked (S027), `wowlua init`'s
/// conversion, then one commit of it on that branch.
fn adopt(request: &Adopt) -> Result<(PathBuf, Vec<String>), String> {
    let dir = &request.dir;
    let git = |args: &[&str]| -> Result<(), String> {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .map_err(|e| format!("cannot run git: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    };
    let branch = request.branch && wowlua_driver::vcs::inside_work_tree(dir);
    if branch {
        git(&["switch", "-c", "wowlua/adopt"])?;
    }
    let report = wowlua_driver::scaffold::init_package(&InitRequest {
        dir: dir.clone(),
        from_toc: None,
        // `--from-pkgmeta` as the form shows it: the `.pkgmeta` must be
        // there.
        from_pkgmeta: request.pkgmeta,
        pkgmeta_file: None,
        no_pkgmeta: !request.pkgmeta,
        inherited: None,
        addon_name: None,
        neighbors: Vec::new(),
    })
    .map_err(|d| d.message)?;
    let mut lines = vec![format!(
        "{} for {}: {} moved into src/, {} updated",
        report.addon_name,
        report.flavors.join(", "),
        plural(report.moved, "file", "files"),
        plural(report.rewritten_paths, "in-game path", "in-game paths"),
    )];
    if report.dependencies > 0 {
        lines.push(format!(
            "{} from .pkgmeta became dependencies",
            plural(report.dependencies, "external", "externals")
        ));
    }
    for member in &report.members {
        lines.push(format!("{member} became a workspace member"));
    }
    for (name, _) in &report.external_addons {
        lines.push(format!("{name} became an external addon"));
    }
    for warning in &report.warnings {
        lines.push(format!("warning: {}", warning.message));
    }
    if !report.old_tocs.is_empty() {
        lines.push(format!(
            "TOC files are made from wowlua.json now; {} can be deleted",
            report.old_tocs.join(", ")
        ));
    }
    if branch {
        git(&["add", "-A"])?;
        git(&[
            "commit",
            "-q",
            "-m",
            &format!("Adopt {} with wowlua", report.addon_name),
        ])?;
        lines.push("committed on the new branch wowlua/adopt".into());
    }
    Ok((dir.clone(), lines))
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The Lua and XML files under `dir`.
fn count_sources(dir: &Path) -> usize {
    let mut count = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            let path = entry.path();
            if path.is_dir() {
                if !skipped(&name) {
                    stack.push(path);
                }
            } else if name.ends_with(".lua") || name.ends_with(".xml") {
                count += 1;
            }
        }
    }
    count
}

/// The colours of wowlua's lexer (S045): keywords, literals, comments and
/// operators; a name after `function` is a function, after `@` a
/// decorator. Types and contextual keywords come from the language
/// server's semantic tokens.
fn highlight(text: &str) -> Vec<Highlight> {
    use wowlua_syntax::token::{TokenKind as K, TriviaKind};
    let mut db = wowlua_span::SourceDb::new();
    let file = db.add("highlight", String::new());
    let lexed = wowlua_syntax::lexer::lex(text.as_bytes(), file);
    let mut out = Vec::new();
    for trivia in &lexed.trivia {
        if trivia.kind != TriviaKind::Newline {
            out.push(Highlight {
                start: trivia.start as usize,
                end: trivia.end as usize,
                syntax: Syntax::Comment,
            });
        }
    }
    let mut previous = K::Eof;
    for i in 0..lexed.tokens.len() {
        let token = lexed.tokens.token(i);
        let syntax = match token.kind {
            K::Nil | K::True | K::False | K::Number => Some(Syntax::Number),
            K::String | K::LongString | K::InterpStart | K::InterpText | K::InterpEnd => {
                Some(Syntax::String)
            }
            K::Name if previous == K::Function => Some(Syntax::Function),
            K::Name if previous == K::At => Some(Syntax::Decorator),
            K::At => Some(Syntax::Decorator),
            k if k.is_keyword() => Some(Syntax::Keyword),
            K::Name
            | K::Error
            | K::Eof
            | K::LParen
            | K::RParen
            | K::LBrace
            | K::RBrace
            | K::LBracket
            | K::RBracket
            | K::Comma
            | K::Semi
            | K::Dot
            | K::Colon => None,
            _ => Some(Syntax::Punct),
        };
        if let Some(syntax) = syntax {
            out.push(Highlight {
                start: token.start as usize,
                end: token.end as usize,
                syntax,
            });
        }
        previous = token.kind;
    }
    out.sort_by_key(|h| h.start);
    out
}

/// The `### New` lines of a changelog section, as plain text.
fn new_lines(section: &str) -> Vec<String> {
    let Some(start) = section.find("### New") else {
        return Vec::new();
    };
    let body = &section[start + "### New".len()..];
    let body = &body[..body.find("\n### ").unwrap_or(body.len())];
    body.lines()
        .filter_map(|l| l.strip_prefix("- "))
        .map(|l| l.replace("**", "").replace('`', ""))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::new_lines;

    #[test]
    fn whats_new_is_the_new_section_as_text() {
        let section = "### New\n\n- **Test runner**: `wowlua test` runs.\n- Two.\n\n### Fixed\n\n- Not this.\n";
        assert_eq!(
            new_lines(section),
            ["Test runner: wowlua test runs.", "Two."]
        );
    }
}
