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
    Adopt, AdoptPreview, ClientFolder, DepsEvent, Entry, Event, FileChange, Found, FoundIn,
    Highlight, Hit, Installation, MOST_FOUND, Member, NewAddon, Opened, PackageNames, Search,
    ShipEvent, Syntax, System, UiState, Workspace,
};

/// Git's own files the git page follows: `HEAD`, the index, the refs
/// and a merge's state (M11).
fn git_state_file(p: &Path) -> bool {
    let s = p.to_string_lossy().replace('\\', "/");
    let Some((_, inside)) = s.split_once("/.git/") else {
        return false;
    };
    matches!(
        inside,
        "HEAD" | "index" | "MERGE_HEAD" | "ORIG_HEAD" | "FETCH_HEAD" | "packed-refs"
    ) || inside.starts_with("refs/")
}

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
    terminals: crate::terminal::Terminals,
    /// The test run's stop button.
    test_stop: RefCell<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// The game page's session, its scenes, and its screen (S055).
    game: RefCell<Option<crate::game::Running>>,
    game_frames: RefCell<std::sync::Arc<std::sync::Mutex<crate::game::Frames>>>,
    game_screen: RefCell<Option<crate::game::Screen>>,
    /// The installation's art and spells (the world profiles', S060).
    gamedata: std::sync::Arc<crate::gamedata::GameData>,
    /// The ship page's dev loop (S061).
    dev: RefCell<crate::ship::Dev>,
    /// Set to stop a GitHub sign-in under way (S062).
    github_stop: RefCell<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

impl NativeSystem {
    pub fn new(ctx: &egui::Context) -> Self {
        // Someone is in front of Studio to answer the OS credential store's
        // dialogs: the ship page reads and saves `wowlua login`'s tokens
        // (wowlua T501, S061).
        wowlua_driver::credentials::allow_dialogs();
        let (send, events) = channel();
        NativeSystem {
            send,
            events,
            ctx: ctx.clone(),
            watcher: RefCell::new(None),
            lsp: RefCell::new(None),
            terminals: crate::terminal::Terminals::default(),
            test_stop: RefCell::default(),
            game: RefCell::new(None),
            game_frames: RefCell::default(),
            game_screen: RefCell::new(None),
            gamedata: std::sync::Arc::default(),
            dev: RefCell::default(),
            github_stop: RefCell::default(),
        }
    }

    /// eframe's GPU device and egui's renderer: the game's screen draws
    /// there.
    pub fn set_gpu(&mut self, gpu: eframe::egui_wgpu::RenderState) {
        self.game_screen = RefCell::new(Some(crate::game::Screen::new(gpu)));
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

    /// Where a job of the ship page tells its news: each wakes Studio up.
    fn ship_tell<E: Send + 'static>(
        &self,
        wrap: impl Fn(E) -> ShipEvent + Send + Sync + 'static,
    ) -> crate::ship::Tell<E> {
        let send = std::sync::Mutex::new(self.send.clone());
        let ctx = self.ctx.clone();
        std::sync::Arc::new(move |e| {
            if let Ok(send) = send.lock() {
                let _ = send.send(Event::Ship(wrap(e)));
            }
            ctx.request_repaint();
        })
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

    fn pick_file(&self, title: &str, endings: &[&str]) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title(title)
            .add_filter("speedscope", endings)
            .pick_file()
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
        let mut out = highlight(text);
        // The names, as the server's semantic tokens tell them (S042).
        let names = semantic(text);
        if !names.is_empty() {
            out.retain(|h| !names.iter().any(|n| n.start < h.end && h.start < n.end));
            out.extend(names);
            out.sort_by_key(|h| h.start);
        }
        out
    }

    fn schema(&self, world: bool) -> Option<String> {
        Some(if world {
            wowlua_manifest::world::schema_json()
        } else {
            wowlua_manifest::schema_json()
        })
    }

    fn folding(&self, text: &str) -> Vec<(usize, usize)> {
        wowlua_lsp::editor::folding_ranges(text)
            .iter()
            .filter_map(|r| {
                Some((
                    r["startLine"].as_u64()? as usize,
                    r["endLine"].as_u64()? as usize,
                ))
            })
            .collect()
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
                // Git's own files stay out, but `HEAD` (a checkout, S049)
                // and what the git page follows: the index, the refs, a
                // merge (M11).
                .filter(|p| {
                    git_state_file(p)
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
            Err(e) => crate::log::line(&format!("cannot watch {}: {e}", root.display())),
        }
    }

    fn find_installation(&self) {
        self.spawn(|| Event::Installation(installation()));
    }

    fn set_installation(&self, path: &Path) {
        if let Err(e) = wowlua_driver::install::set_wow_path(path) {
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
            crate::log::line(&format!("cannot open a new window: {e}"));
        }
    }

    fn open_window_from_github(&self) {
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        if let Err(e) = Command::new(exe).arg(crate::FROM_GITHUB).spawn() {
            crate::log::line(&format!("cannot open a new window: {e}"));
        }
    }

    fn whats_new(&self) -> Vec<String> {
        wowlua_driver::release_notes(wowlua_driver::COMPILER_VERSION)
            .map(new_lines)
            .unwrap_or_default()
    }

    fn branch(&self, dir: &Path) -> Option<String> {
        repository_branch(dir)
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

    fn terminal_open(&self, cwd: &Path, cols: u16, rows: u16) -> Result<u64, String> {
        self.terminals.open(cwd, cols, rows, &self.ctx)
    }

    fn terminal_input(&self, id: u64, bytes: &[u8]) {
        self.terminals.input(id, bytes);
    }

    fn terminal_resize(&self, id: u64, cols: u16, rows: u16) {
        self.terminals.resize(id, cols, rows);
    }

    fn terminal_scroll(&self, id: u64, lines: i32) {
        self.terminals.scroll(id, lines);
    }

    fn terminal_screen(&self, id: u64) -> Option<wowlua_studio_app::system::TermScreen> {
        self.terminals.screen(id)
    }

    fn terminal_close(&self, id: u64) {
        self.terminals.close(id);
    }

    fn terminal_select(&self, id: u64, select: wowlua_studio_app::system::TermSelect) {
        self.terminals.select(id, select);
    }

    fn terminal_selection(&self, id: u64) -> Option<String> {
        self.terminals.selection(id)
    }

    fn clipboard_text(&self) -> Option<String> {
        arboard::Clipboard::new().ok()?.get_text().ok()
    }

    fn find_in(
        &self,
        text: &str,
        search: &Search,
        replace: Option<&str>,
    ) -> Result<Vec<FoundIn>, String> {
        find_in(text, search, replace)
    }

    fn test_files(&self, root: &Path) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = ignore::WalkBuilder::new(root)
            .build()
            .flatten()
            .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
            .map(|e| e.into_path())
            .filter(|p| p.to_string_lossy().ends_with(".test.wowlua"))
            .filter(|p| {
                !p.components()
                    .any(|c| skipped(&c.as_os_str().to_string_lossy()))
            })
            .collect();
        files.sort();
        files
    }

    fn test_run(&self, root: &Path, run: u64, request: wowlua_studio_app::system::TestRequest) {
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        *self.test_stop.borrow_mut() = stop.clone();
        let send = std::sync::Mutex::new(self.send.clone());
        let ctx = self.ctx.clone();
        crate::tests::start(root.to_path_buf(), request, stop, move |event| {
            if let Ok(send) = send.lock() {
                let _ = send.send(Event::Tests { run, event });
            }
            ctx.request_repaint();
        });
    }

    fn game_start(
        &self,
        root: &Path,
        session: u64,
        launch: wowlua_studio_app::system::Launch,
        setup: Vec<(u64, wowlua_studio_app::system::GameRequest)>,
    ) {
        if let Some(old) = self.game.borrow_mut().take() {
            old.stop();
        }
        let frames = std::sync::Arc::new(std::sync::Mutex::new(crate::game::Frames::default()));
        *self.game_frames.borrow_mut() = frames.clone();
        let send = std::sync::Arc::new(std::sync::Mutex::new(self.send.clone()));
        let ctx = self.ctx.clone();
        let wake_ctx = self.ctx.clone();
        let running = crate::game::start(
            root.to_path_buf(),
            launch,
            setup,
            frames,
            move |event| {
                if let Ok(send) = send.lock() {
                    let _ = send.send(Event::Game { session, event });
                }
                ctx.request_repaint();
            },
            move || wake_ctx.request_repaint(),
        );
        *self.game.borrow_mut() = Some(running);
    }

    fn game_request(&self, id: u64, request: wowlua_studio_app::system::GameRequest) {
        if let Some(running) = self.game.borrow().as_ref() {
            running.ask(id, request);
        }
    }

    fn game_stop(&self) {
        if let Some(running) = self.game.borrow_mut().take() {
            running.stop();
        }
    }

    fn profiles(&self, root: &Path) -> Vec<wowlua_studio_app::system::SavedProfile> {
        crate::profiles::list(root)
    }

    fn profile_read(&self, path: &Path) {
        let path = path.to_path_buf();
        self.spawn(move || Event::Profile {
            result: crate::profiles::read(&path),
            path,
        });
    }

    fn profile_keep(
        &self,
        root: &Path,
        name: &str,
        speedscope: &serde_json::Value,
    ) -> Result<(PathBuf, wowlua_studio_app::system::Profile), String> {
        crate::profiles::keep(root, name, speedscope)
    }

    fn explain(&self, code: &str) -> Option<String> {
        wowlua_diagnostics::explanation(code).map(|e| e.text.clone())
    }

    fn function_names(&self, root: &Path, files: Vec<String>) {
        let root = root.to_path_buf();
        self.spawn(move || Event::FunctionNames(crate::profiles::function_names(&root, &files)));
    }

    fn game_recordings(&self, root: &Path) {
        let root = root.to_path_buf();
        self.spawn(move || Event::GameRecordings(crate::profiles::game_recordings(&root)));
    }

    fn game_art(&self, flavor: &str, art: wowlua_studio_app::system::Art) {
        let data = self.gamedata.clone();
        let flavor = flavor.to_string();
        self.spawn(move || {
            let image = data.art(&flavor, &art);
            Event::GameArt(art, image)
        });
    }

    fn class_spells(&self, flavor: &str, class: &str) {
        let data = self.gamedata.clone();
        let (flavor, class) = (flavor.to_string(), class.to_string());
        self.spawn(move || {
            let found = data.class_spells(&flavor, &class);
            Event::ClassSpells(class, found)
        });
    }

    fn search_spells(&self, flavor: &str, query: &str) {
        let data = self.gamedata.clone();
        let (flavor, query) = (flavor.to_string(), query.to_string());
        self.spawn(move || {
            let found = data.search_spells(&flavor, &query);
            Event::Spells(query, found)
        });
    }

    fn find(&self, what: wowlua_studio_app::system::Find, flavor: &str, query: &str) {
        let data = self.gamedata.clone();
        let (flavor, query) = (flavor.to_string(), query.to_string());
        self.spawn(move || {
            let rows = match what {
                wowlua_studio_app::system::Find::Spell => data.search_spells(&flavor, &query),
                wowlua_studio_app::system::Find::Item => data.search_items(&flavor, &query),
            };
            Event::Found(what, flavor, query, rows)
        });
    }

    fn github(&self, root: &Path, request: wowlua_studio_app::system::GithubRequest) {
        let root = root.to_path_buf();
        self.spawn(move || {
            let event = crate::github::answer(&root, request, crate::settings::github_token());
            Event::GithubApi(root, event)
        });
    }

    fn git(&self, root: &Path, request: wowlua_studio_app::system::GitRequest) {
        let root = root.to_path_buf();
        self.spawn(move || {
            let creds = crate::git::Credentials {
                github_token: crate::settings::github_token(),
            };
            let event = crate::git::answer(&root, request, &creds);
            Event::Git(root, event)
        });
    }

    fn github_account(&self, request: wowlua_studio_app::system::AccountRequest) {
        self.spawn(move || {
            Event::Account(crate::github::account(
                request,
                crate::settings::github_token(),
            ))
        });
    }

    fn clone_repo(&self, repo: &str, branch: &str) {
        let (repo, branch) = (repo.to_owned(), branch.to_owned());
        let send = self.send.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let creds = crate::git::Credentials {
                github_token: crate::settings::github_token(),
            };
            let tell = |progress: Option<f32>, said: String| {
                let _ = send.send(Event::Cloning {
                    repo: repo.clone(),
                    progress,
                    said,
                });
                ctx.request_repaint();
            };
            let result = match crate::clones::dir() {
                Some(dir) => crate::clones::clone_into(
                    &dir,
                    &format!("https://github.com/{repo}.git"),
                    &repo,
                    &branch,
                    &creds,
                    &tell,
                ),
                None => Err("Studio has no folder for its data, so nothing to clone into".into()),
            };
            let _ = send.send(Event::Cloned {
                repo,
                branch,
                result,
            });
            ctx.request_repaint();
        });
    }

    fn clones_dir(&self) -> Option<PathBuf> {
        crate::clones::dir()
    }

    fn hold_clone(&self, root: &Path) {
        if let Some(dir) = crate::clones::dir() {
            crate::clones::hold_clone(&dir, root);
        }
    }

    fn clone_losses(&self, root: &Path) -> Result<wowlua_studio_app::system::Losses, String> {
        crate::clones::losses(root)
    }

    fn delete_clone(&self, root: &Path) -> Result<(), String> {
        let dir = crate::clones::dir().ok_or("Studio has no folder for its data")?;
        crate::clones::delete(&dir, root)
    }

    fn left_clones(&self) -> Vec<wowlua_studio_app::system::LeftClone> {
        crate::clones::dir()
            .map(|d| crate::clones::left(&d))
            .unwrap_or_default()
    }

    fn binding_actions(&self, flavor: &str) -> Vec<(String, String)> {
        crate::bindings::actions(flavor)
    }

    fn cvars(&self, flavor: &str) -> Vec<wowlua_studio_app::system::CVarInfo> {
        wowlua_engine::cvars(flavor)
            .into_iter()
            .map(|c| wowlua_studio_app::system::CVarInfo {
                name: c.name,
                default: c.default,
                secure: c.secure,
                read_only: c.read_only,
            })
            .collect()
    }

    fn world_profiles(&self, root: &Path) -> Vec<wowlua_studio_app::system::WorldProfile> {
        crate::world::profiles(root)
    }

    fn default_profile(&self, flavor: &str) -> Option<String> {
        wowlua_test::profile::default_for(flavor).map(str::to_owned)
    }

    fn check_world_profile(&self, text: &str) -> Vec<wowlua_studio_app::system::ProfileProblem> {
        crate::world::check(text)
    }

    fn profile_uses(&self, root: &Path, name: &str) -> Vec<String> {
        crate::world::uses(root, name, &self.test_files(root))
    }

    fn world_snapshots(&self, root: &Path) {
        let root = root.to_path_buf();
        self.spawn(move || Event::WorldSnapshots(crate::world::snapshots(&root)));
    }

    fn read_saved_variables(
        &self,
        path: &Path,
    ) -> Result<Vec<(String, serde_json::Value)>, String> {
        crate::world::saved_variables(path)
    }

    fn reveal(&self, path: &Path) {
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else if cfg!(windows) {
            "explorer"
        } else {
            "xdg-open"
        };
        if let Err(e) = Command::new(opener).arg(path).spawn() {
            crate::log::line(&format!("cannot show {}: {e}", path.display()));
        }
    }

    fn settings_info(&self) {
        self.spawn(|| Event::Settings(crate::settings::info()));
    }

    fn log_file(&self) -> Option<PathBuf> {
        crate::log::path()
    }

    fn clear_check_cache(&self) {
        crate::settings::clear_check_cache();
    }

    fn clear_art_cache(&self) {
        crate::settings::clear_art_cache();
    }

    fn check_updates(&self) {
        self.spawn(|| Event::Updates(crate::settings::check_updates()));
    }

    fn refresh_ui(&self, flavor: &str) {
        if let Err(e) = wowlua_test::forget_ui_source(flavor) {
            crate::log::line(&e);
        }
        self.download_ui(flavor);
    }

    fn github_sign_in(&self, on: bool) {
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let old = std::mem::replace(&mut *self.github_stop.borrow_mut(), stop.clone());
        old.store(true, std::sync::atomic::Ordering::Relaxed);
        if !on {
            crate::settings::github_sign_out();
            return;
        }
        let send = std::sync::Mutex::new(self.send.clone());
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            crate::settings::github_sign_in(stop, |event| {
                if let Ok(send) = send.lock() {
                    let _ = send.send(Event::Github(event));
                }
                ctx.request_repaint();
            });
        });
    }

    fn claude_key(&self, key: Option<String>) {
        crate::settings::claude_key(key.as_deref());
    }

    fn deps_read(&self, dir: &Path) {
        let dir = dir.to_path_buf();
        self.spawn(move || Event::Deps(DepsEvent::Read(Box::new(crate::deps::read(&dir)))));
    }

    fn deps_outdated(&self, dir: &Path) {
        let dir = dir.to_path_buf();
        self.spawn(move || Event::Deps(DepsEvent::Outdated(crate::deps::outdated(&dir))));
    }

    fn deps_search(&self, _dir: &Path, query: &str) {
        let query = query.to_owned();
        self.spawn(move || {
            let found = crate::deps::search(&query);
            Event::Deps(DepsEvent::Searched { query, found })
        });
    }

    fn deps_versions(&self, _dir: &Path, name: &str) {
        let name = name.to_owned();
        self.spawn(move || {
            let versions = crate::deps::versions(&name);
            Event::Deps(DepsEvent::Versions { name, versions })
        });
    }

    fn deps_run(&self, dir: &Path, job: u64, command: wowlua_studio_app::system::DepsCommand) {
        let dir = dir.to_path_buf();
        self.spawn(move || {
            let (ok, output) = crate::deps::run(&dir, &command);
            Event::Deps(DepsEvent::Ran {
                job,
                command: command.line(),
                ok,
                output,
            })
        });
    }

    fn ship_read(&self, root: &Path) {
        let root = root.to_path_buf();
        self.spawn(move || Event::Ship(ShipEvent::Read(Box::new(crate::ship::read(&root)))));
    }

    fn profiles_read(&self, dir: &Path) {
        let dir = dir.to_path_buf();
        self.spawn(move || {
            let read = crate::ship::read(&dir).map(|facts| facts.profiles);
            Event::Profiles { dir, read }
        });
    }

    fn ship_tokens(&self, targets: Vec<wowlua_studio_app::system::ShipTarget>) {
        self.spawn(move || Event::Ship(ShipEvent::Tokens(crate::ship::tokens(&targets))));
    }

    fn ship_token(&self, kind: &str, token: Option<String>) {
        let kind = kind.to_owned();
        self.spawn(move || {
            let result = crate::ship::token(&kind, token.as_deref());
            Event::Ship(ShipEvent::TokenSaved { kind, result })
        });
    }

    fn ship_build(&self, root: &Path, job: u64, build: wowlua_studio_app::system::ShipBuild) {
        let tell = self.ship_tell(move |event| ShipEvent::Build { job, event });
        crate::ship::build(root.to_path_buf(), build, tell);
    }

    fn ship_dev(&self, root: &Path, dev: Option<wowlua_studio_app::system::ShipInstall>) {
        match dev {
            Some(how) => {
                let tell = self.ship_tell(ShipEvent::Dev);
                self.dev.borrow_mut().start(root.to_path_buf(), how, tell);
            }
            None => self.dev.borrow_mut().stop(),
        }
    }

    fn ship_publish(&self, root: &Path, job: u64, publish: wowlua_studio_app::system::ShipPublish) {
        let tell = self.ship_tell(move |event| ShipEvent::Publish { job, event });
        crate::ship::publish(root.to_path_buf(), publish, tell);
    }

    fn ship_tag(&self, root: &Path, tag: wowlua_studio_app::system::TagPush) {
        let root = root.to_path_buf();
        self.spawn(move || Event::Ship(ShipEvent::Tagged(crate::ship::tag_and_push(&root, &tag))));
    }

    fn game_screen(&self) -> Option<wowlua_studio_app::system::GameScreen> {
        let frames = self.game_frames.borrow().clone();
        self.game_screen.borrow_mut().as_mut()?.show(&frames)
    }

    fn test_stop(&self) {
        self.test_stop
            .borrow()
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    fn affected_tests(&self, root: &Path, changed: &Path) -> Vec<PathBuf> {
        crate::tests::affected(root, changed)
    }

    fn coverage_report(&self, root: &Path) -> Option<wowlua_studio_app::system::Coverage> {
        crate::tests::read_coverage(root)
    }

    fn read_image(&self, path: &Path) -> Option<wowlua_studio_app::system::Image> {
        read_png(path)
    }

    fn copy_file(&self, from: &Path, to: &Path) -> Result<(), String> {
        std::fs::copy(from, to)
            .map(|_| ())
            .map_err(|e| format!("cannot copy {} to {}: {e}", from.display(), to.display()))
    }

    fn search(&self, root: &Path, search: Search) {
        let root = root.to_path_buf();
        self.spawn(move || {
            let found = search_files(&root, &search);
            Event::Searched { search, found }
        });
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
                flavors: p.manifest.flavors.clone(),
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
        flavors: value
            .get("flavors")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|f| f.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
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

/// The branch checked out in `dir`'s repository.
pub(crate) fn repository_branch(dir: &Path) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    let branch = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (out.status.success() && !branch.is_empty()).then_some(branch)
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

/// The server's semantic tokens as byte ranges with the design's colours:
/// keywords, types (classes, enums, namespaces too), functions and
/// methods, decorators. Parameters and fields keep the text's colour.
fn semantic(text: &str) -> Vec<Highlight> {
    let data = wowlua_lsp::editor::semantic_tokens(text);
    let types = wowlua_lsp::editor::TOKEN_TYPES;
    let mut starts = vec![0];
    starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
    let (mut line, mut col) = (0usize, 0usize);
    let mut out = Vec::new();
    for q in data.chunks(5) {
        if q[0] > 0 {
            col = 0;
        }
        line += q[0] as usize;
        col += q[1] as usize;
        let syntax = match types.get(q[3] as usize).copied() {
            Some("keyword") => Syntax::Keyword,
            Some("type" | "class" | "namespace") => Syntax::Type,
            Some("function" | "method") => Syntax::Function,
            Some("decorator") => Syntax::Decorator,
            _ => continue,
        };
        let Some(&line_start) = starts.get(line) else {
            break;
        };
        // UTF-16 columns to bytes.
        let row = &text[line_start..];
        let byte = |units: usize| {
            let mut seen = 0;
            for (b, c) in row.char_indices() {
                if seen >= units {
                    return b;
                }
                seen += c.len_utf16();
            }
            row.len()
        };
        let start = line_start + byte(col);
        let end = line_start + byte(col + q[2] as usize);
        out.push(Highlight { start, end, syntax });
    }
    out
}

/// A PNG's pixels as RGBA.
fn read_png(path: &Path) -> Option<wowlua_studio_app::system::Image> {
    let file = std::fs::File::open(path).ok()?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let pixels = &buf[..info.buffer_size()];
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => pixels.to_vec(),
        png::ColorType::Rgb => pixels
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => pixels
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|&[g, a]| [g, g, g, a])
            .collect(),
        png::ColorType::Grayscale => pixels.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some(wowlua_studio_app::system::Image {
        width: info.width,
        height: info.height,
        rgba,
    })
}

/// How many matches a search keeps.
const MOST_HITS: usize = 5000;

/// The piece of `row` around the match `start..end` (bytes) that the
/// search panel shows: a little before it, the line's rest up to a
/// panel's width after; and the match in it, in characters.
fn excerpt(row: &str, start: usize, end: usize) -> (String, usize, usize) {
    const BEFORE: usize = 40;
    const AFTER: usize = 200;
    let from = row[..start]
        .char_indices()
        .rev()
        .nth(BEFORE - 1)
        .map_or(0, |(i, _)| i);
    // A long match is cut too.
    let end = row[start..]
        .char_indices()
        .nth(AFTER)
        .map_or(end, |(i, _)| end.min(start + i));
    let to = row[end..]
        .char_indices()
        .nth(AFTER)
        .map_or(row.len(), |(i, _)| end + i);
    let text = row[from..to].to_owned();
    let first = row[from..start].chars().count();
    let last = first + row[start..end].chars().count();
    (text, first, last)
}

/// The text files under `root` that match, as Git sees the folder (its
/// ignore files followed, hidden files left out).
/// The search's regular expression: its text escaped when it is none, in
/// word boundaries for a whole word.
fn regex_of(search: &Search) -> Result<regex::Regex, String> {
    let pattern = if search.regex {
        search.query.clone()
    } else {
        regex::escape(&search.query)
    };
    let pattern = if search.word {
        format!(r"\b(?:{pattern})\b")
    } else {
        pattern
    };
    regex::RegexBuilder::new(&pattern)
        .case_insensitive(!search.case)
        .build()
        .map_err(|e| format!("not a regular expression: {e}"))
}

/// The find bar's matches in `text`, with their replacements.
fn find_in(text: &str, search: &Search, replace: Option<&str>) -> Result<Vec<FoundIn>, String> {
    let re = regex_of(search)?;
    let mut out = Vec::new();
    for caps in re.captures_iter(text).take(MOST_FOUND) {
        let m = caps.get(0).expect("the whole match");
        if m.start() == m.end() {
            continue;
        }
        let replacement = replace.map(|r| {
            if search.regex {
                let mut expanded = String::new();
                caps.expand(r, &mut expanded);
                expanded
            } else {
                r.to_owned()
            }
        });
        out.push(FoundIn {
            start: m.start(),
            end: m.end(),
            replacement,
        });
    }
    Ok(out)
}

fn search_files(root: &Path, search: &Search) -> Found {
    let started = std::time::Instant::now();
    let re = match regex_of(search) {
        Ok(re) => re,
        Err(error) => {
            return Found {
                error: Some(error),
                ..Found::default()
            };
        }
    };
    let mut found = Found::default();
    let mut hits_total = 0;
    for entry in ignore::WalkBuilder::new(root).build().flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let Ok(bytes) = std::fs::read(entry.path()) else {
            continue;
        };
        // Binary files (a NUL early on) are passed over.
        if bytes.iter().take(8000).any(|&b| b == 0) {
            continue;
        }
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        found.searched += 1;
        let mut hits = Vec::new();
        for (line, row) in text.lines().enumerate() {
            // UTF-16 units up to `byte`, counted on from the last match:
            // once over the line however long it is.
            let (mut byte, mut units) = (0, 0);
            let mut units_to = |to: usize| {
                units += row[byte..to].encode_utf16().count();
                byte = to;
                units
            };
            for m in re.find_iter(row) {
                if m.start() == m.end() {
                    continue;
                }
                let at = (units_to(m.start()), units_to(m.end()));
                let (text, start, end) = excerpt(row, m.start(), m.end());
                hits.push(Hit {
                    line,
                    at,
                    text,
                    start,
                    end,
                });
                hits_total += 1;
                if hits_total >= MOST_HITS {
                    break;
                }
            }
            if hits_total >= MOST_HITS {
                found.cut = true;
                break;
            }
        }
        if !hits.is_empty() {
            found.files.push((entry.path().to_path_buf(), hits));
        }
        if found.cut {
            break;
        }
    }
    found.files.sort_by(|a, b| a.0.cmp(&b.0));
    found.millis = started.elapsed().as_millis() as u64;
    found
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
    use super::{new_lines, search_files};
    use wowlua_studio_app::system::Search;

    #[test]
    fn whats_new_is_the_new_section_as_text() {
        let section = "### New\n\n- **Test runner**: `wowlua test` runs.\n- Two.\n\n### Fixed\n\n- Not this.\n";
        assert_eq!(
            new_lines(section),
            ["Test runner: wowlua test runs.", "Two."]
        );
    }

    #[test]
    fn a_hover_s_type_is_coloured_as_the_editor_colours_it() {
        use wowlua_studio_app::editor::{Hover, Pos};
        let system = super::NativeSystem::new(&eframe::egui::Context::default());
        let text = "```wowlua\nActionBar: ClassOf<ActionBar>\n```\n\nThe bar.";
        let hover = Hover::new(Pos::new(0, 0), text.to_owned(), &system);
        let types: Vec<&str> = hover
            .code
            .iter()
            .filter(|h| h.syntax == wowlua_studio_app::system::Syntax::Type)
            .map(|h| &text[h.start..h.end])
            .collect();
        assert_eq!(types, ["ClassOf", "ActionBar"]);
        let text = "```wowlua\nstring?\n```";
        let hover = Hover::new(Pos::new(0, 0), text.to_owned(), &system);
        let coloured: Vec<&str> = hover.code.iter().map(|h| &text[h.start..h.end]).collect();
        assert!(coloured.contains(&"string"), "{coloured:?}");
    }

    #[test]
    fn wowlua_s_manifest_schema_completes_the_package_s_keys() {
        use wowlua_studio_app::editor::json;
        let schema: serde_json::Value =
            serde_json::from_str(&wowlua_manifest::schema_json()).unwrap();
        let text = "{\n  \"";
        let place = json::place(text, text.len());
        let keys: Vec<String> = json::suggestions(&schema, &place)
            .into_iter()
            .map(|s| s.label)
            .collect();
        assert!(keys.contains(&"package".to_owned()), "{keys:?}");
        assert!(keys.contains(&"flavors".to_owned()), "{keys:?}");
        let text = "{ \"package\": { \"";
        let place = json::place(text, text.len());
        let keys: Vec<String> = json::suggestions(&schema, &place)
            .into_iter()
            .map(|s| s.label)
            .collect();
        assert!(keys.contains(&"name".to_owned()), "{keys:?}");
    }

    #[test]
    fn a_search_finds_words_and_leaves_ignored_files_out() {
        let dir = std::env::temp_dir().join(format!("wowlua-studio-search-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/a.wowlua"),
            "local stateOf = 1
print(stateOfX, stateOf)
",
        )
        .unwrap();
        std::fs::write(dir.join(".ignore"), "dist/\n").unwrap();
        std::fs::create_dir_all(dir.join("dist")).unwrap();
        std::fs::write(
            dir.join("dist/b.lua"),
            "stateOf
",
        )
        .unwrap();
        let search = Search {
            query: "stateof".into(),
            word: true,
            ..Search::default()
        };
        let found = search_files(&dir, &search);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(found.files.len(), 1, "{found:?}");
        let hits = &found.files[0].1;
        assert_eq!(hits.len(), 2);
        assert_eq!((hits[1].line, hits[1].start, hits[1].end), (1, 16, 23));
        assert_eq!(hits[1].at, (16, 23));
    }

    #[test]
    fn the_find_bar_s_regular_expressions_replace_with_their_groups() {
        let search = Search {
            query: r"(\w+):Show\(\)".to_owned(),
            regex: true,
            case: true,
            ..Search::default()
        };
        let found = super::find_in("frame:Show()\nbar:Show()", &search, Some("$1:Hide()")).unwrap();
        let replaced: Vec<_> = found
            .iter()
            .map(|f| f.replacement.clone().unwrap())
            .collect();
        assert_eq!(replaced, ["frame:Hide()", "bar:Hide()"]);
        assert_eq!((found[1].start, found[1].end), (13, 23));
        let broken = Search {
            query: "(".to_owned(),
            regex: true,
            ..Search::default()
        };
        assert!(super::find_in("x", &broken, None).is_err());
    }

    #[test]
    fn a_match_on_a_long_line_keeps_a_piece_of_it() {
        let dir = std::env::temp_dir().join(format!("wowlua-studio-long-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // A data file on one line: 2 MB, matches all along it.
        let piece = "{\"name\":\"ä test\"},";
        std::fs::write(dir.join("data.json"), piece.repeat(100_000)).unwrap();
        let search = Search {
            query: "test".to_owned(),
            ..Search::default()
        };
        let found = search_files(&dir, &search);
        std::fs::remove_dir_all(&dir).unwrap();
        let hits = &found.files[0].1;
        assert_eq!(hits.len(), super::MOST_HITS);
        assert!(found.cut);
        let units = piece.encode_utf16().count();
        let h = &hits[1000];
        assert_eq!(h.at, (1000 * units + 11, 1000 * units + 15));
        assert!(h.text.chars().count() <= 40 + 4 + 200, "{}", h.text.len());
        let shown: String = h.text.chars().skip(h.start).take(h.end - h.start).collect();
        assert_eq!(shown, "test");
    }
}
