//! What Studio needs from the system it runs on: folders and files, Git,
//! wowlua's library and new windows. The native host implements it; slow
//! work runs off the UI thread and comes back as [`Event`]s.

use std::path::{Path, PathBuf};

/// A package of the opened workspace.
#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    /// The package's name in `wowlua.json`.
    pub name: String,
    /// `addon`, `library`, `lua-library` or `types`.
    pub kind: String,
    pub version: String,
    /// Its folder.
    pub dir: PathBuf,
    /// The folder relative to the workspace's root, empty for the root.
    pub rel: String,
}

/// An opened workspace: a package, or a workspace root with its members.
#[derive(Clone, Debug, PartialEq)]
pub struct Workspace {
    /// The workspace's root folder (where its `wowlua.json` is).
    pub root: PathBuf,
    /// The root package's name.
    pub name: String,
    pub members: Vec<Member>,
    /// The flavors the root package targets.
    pub flavors: Vec<String>,
    /// Why checks are off: the package's `wowlua` requirement excludes this
    /// wowlua (E4300).
    pub cannot_check: Option<String>,
}

/// What opening a folder found.
#[derive(Clone, Debug, PartialEq)]
pub enum Opened {
    Workspace(Workspace),
    /// A folder without `wowlua.json`: it can be adopted (S032).
    NoManifest(PathBuf),
    Failed {
        path: PathBuf,
        message: String,
    },
}

/// The WoW installation, as wowlua finds it (S028).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Installation {
    pub path: Option<PathBuf>,
    /// Its client folders (`_retail_`, `_classic_beta_`) with the client's
    /// name and flavor from wowlua's flavor table.
    pub clients: Vec<ClientFolder>,
    /// Why none was found, or why the configured one is wrong.
    pub problem: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClientFolder {
    pub folder: String,
    pub client: String,
    pub flavor: String,
}

/// Blizzard's UI code of a flavor in wowlua's cache (S029).
#[derive(Clone, Debug, PartialEq)]
pub enum UiState {
    Cached,
    Missing,
    Downloading,
    Failed(String),
}

/// A new package (`wowlua new`, S026).
#[derive(Clone, Debug, PartialEq)]
pub struct NewAddon {
    pub path: PathBuf,
    pub name: Option<String>,
    /// `addon`, `library`, `lua-library` or `types`.
    pub kind: String,
    pub flavors: Vec<String>,
    /// `strict`, `basic` or `nocheck`.
    pub check: String,
    pub git: bool,
    pub ci_github: bool,
    pub world_profile: bool,
}

/// What `wowlua new` makes of a name: the package's name and the AddOn's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PackageNames {
    /// `pull-timer`.
    pub package: String,
    /// `PullTimer`: the AddOn's folder in the game.
    pub addon: String,
}

/// Adopting an addon (`wowlua init`, S027).
#[derive(Clone, Debug, PartialEq)]
pub struct Adopt {
    pub dir: PathBuf,
    /// Convert a `.pkgmeta` too, or adopt from the TOC files only
    /// (`--no-pkgmeta`, S033).
    pub pkgmeta: bool,
    /// On a new branch `wowlua/adopt`, committed there.
    pub branch: bool,
}

/// What adopting a folder would take.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AdoptPreview {
    pub tocs: Vec<String>,
    /// Lua and XML files.
    pub sources: usize,
    pub pkgmeta: bool,
    /// The folder is in a Git repository.
    pub git: bool,
    pub has_manifest: bool,
}

/// Work that finished off the UI thread.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Opened(Opened),
    Installation(Installation),
    Ui {
        flavor: String,
        state: UiState,
    },
    /// A new package was made (and is opened next), or not.
    Created(Result<PathBuf, String>),
    /// An addon was adopted, with what it did (`wowlua init`'s report), or
    /// not.
    Adopted(Result<(PathBuf, Vec<String>), String>),
    /// Files changed under the watched workspace.
    FilesChanged(Vec<PathBuf>),
    /// A message of the language server (JSON-RPC, as an editor reads it).
    Lsp(serde_json::Value),
}

/// How a file changed on disk, for the language server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileChange {
    Created,
    Changed,
    Deleted,
}

/// A colour of the highlighting: the design system's syntax tokens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Syntax {
    Keyword,
    Type,
    String,
    Number,
    Decorator,
    Function,
    Comment,
    Punct,
}

/// A highlighted stretch of a file's text, in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Highlight {
    pub start: usize,
    pub end: usize,
    pub syntax: Syntax,
}

/// A file or folder in the explorer.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Entry {
    /// Folders first, then by name.
    pub is_dir: bool,
    pub name: String,
    pub path: PathBuf,
}

/// The system Studio runs on.
pub trait System {
    /// Seconds since the Unix epoch.
    fn now(&self) -> u64;
    /// The user's home folder.
    fn home(&self) -> Option<PathBuf>;
    fn exists(&self, path: &Path) -> bool;
    /// The system's folder picker; `None` when cancelled.
    fn pick_folder(&self, title: &str) -> Option<PathBuf>;
    /// The folder's files and folders, sorted; hidden ones and build output
    /// left out.
    fn list(&self, dir: &Path) -> Vec<Entry>;
    fn read_file(&self, path: &Path) -> Result<String, String>;
    fn write_file(&self, path: &Path, text: &str) -> Result<(), String>;
    /// The highlighting of a wowlua or Lua file's text, from wowlua's
    /// lexer (S045).
    fn highlight(&self, text: &str) -> Vec<Highlight>;
    /// Opens the workspace around `path` (→ [`Event::Opened`]).
    fn open(&self, path: &Path);
    /// Watches the workspace's files (→ [`Event::FilesChanged`]); `None`
    /// stops watching.
    fn watch(&self, root: Option<&Path>);
    /// Finds the installation (→ [`Event::Installation`]).
    fn find_installation(&self);
    /// Keeps `path` as the installation, then finds it again.
    fn set_installation(&self, path: &Path);
    /// Whether the flavor's UI code is in the cache (→ [`Event::Ui`]).
    fn check_ui(&self, flavor: &str);
    fn download_ui(&self, flavor: &str);
    /// The names `wowlua new` gives a package from `name`.
    fn names(&self, name: &str) -> PackageNames;
    /// Whether `dir` (or the folder it would be made in) is in a Git
    /// repository.
    fn in_git(&self, dir: &Path) -> bool;
    fn new_addon(&self, request: NewAddon);
    fn adopt_preview(&self, dir: &Path) -> AdoptPreview;
    fn adopt(&self, request: Adopt);
    /// Opens `path` in a new window (S031), or an empty one.
    fn open_window(&self, path: Option<&Path>);
    /// The `### New` lines of wowlua's changelog for the version built in.
    fn whats_new(&self) -> Vec<String>;
    /// The Git branch checked out in `dir`.
    fn branch(&self, dir: &Path) -> Option<String>;
    /// `path` as the language server names it: absolute, links resolved.
    fn canonical(&self, path: &Path) -> PathBuf;
    /// A message for wowlua's language server, as an editor sends it; the
    /// first starts the server. Its messages come back as [`Event::Lsp`].
    fn lsp_send(&self, message: serde_json::Value);
    /// Files changed on disk, for the server's checks.
    fn lsp_files(&self, changes: Vec<(PathBuf, FileChange)>);
    /// Stops the server (the workspace closed).
    fn lsp_stop(&self);
    /// Work finished since the last call.
    fn poll(&self) -> Vec<Event>;
}

/// A system with nothing in it: tests and the frame budget.
#[derive(Default)]
pub struct NoSystem;

impl System for NoSystem {
    fn now(&self) -> u64 {
        0
    }
    fn home(&self) -> Option<PathBuf> {
        None
    }
    fn exists(&self, _: &Path) -> bool {
        false
    }
    fn pick_folder(&self, _: &str) -> Option<PathBuf> {
        None
    }
    fn list(&self, _: &Path) -> Vec<Entry> {
        Vec::new()
    }
    fn read_file(&self, path: &Path) -> Result<String, String> {
        Err(format!("{} cannot be read here", path.display()))
    }
    fn write_file(&self, path: &Path, _: &str) -> Result<(), String> {
        Err(format!("{} cannot be written here", path.display()))
    }
    fn highlight(&self, _: &str) -> Vec<Highlight> {
        Vec::new()
    }
    fn open(&self, _: &Path) {}
    fn watch(&self, _: Option<&Path>) {}
    fn find_installation(&self) {}
    fn set_installation(&self, _: &Path) {}
    fn check_ui(&self, _: &str) {}
    fn download_ui(&self, _: &str) {}
    fn names(&self, name: &str) -> PackageNames {
        PackageNames {
            package: name.to_owned(),
            addon: name.to_owned(),
        }
    }
    fn in_git(&self, _: &Path) -> bool {
        false
    }
    fn new_addon(&self, _: NewAddon) {}
    fn adopt_preview(&self, _: &Path) -> AdoptPreview {
        AdoptPreview::default()
    }
    fn adopt(&self, _: Adopt) {}
    fn open_window(&self, _: Option<&Path>) {}
    fn whats_new(&self) -> Vec<String> {
        Vec::new()
    }
    fn branch(&self, _: &Path) -> Option<String> {
        None
    }
    fn canonical(&self, path: &Path) -> PathBuf {
        path.to_path_buf()
    }
    fn lsp_send(&self, _: serde_json::Value) {}
    fn lsp_files(&self, _: Vec<(PathBuf, FileChange)>) {}
    fn lsp_stop(&self) {}
    fn poll(&self) -> Vec<Event> {
        Vec::new()
    }
}
