//! What Studio needs from the system it runs on: folders and files, Git,
//! wowlua's library and new windows. The native host implements it; slow
//! work runs off the UI thread and comes back as [`Event`]s.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

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
    /// The flavors it targets (`flavors` in its `wowlua.json`).
    pub flavors: Vec<String>,
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
    /// A text search finished.
    Searched {
        search: Search,
        found: Found,
    },
    /// A test run's progress (S052): its test files planned, what it does
    /// (building, fetching Blizzard's UI), each file's results, its end.
    Tests {
        run: u64,
        event: TestEvent,
    },
    /// A game session's news (the game page, S055).
    Game {
        session: u64,
        event: GameEvent,
    },
    /// A profile read (S057).
    Profile {
        path: PathBuf,
        result: Result<Profile, String>,
    },
    /// The game's recordings read.
    GameRecordings(Result<Vec<GameRecordings>, String>),
    /// Functions named (a profile's): the source, the line a function is
    /// defined on, its name.
    FunctionNames(Vec<(String, u32, String)>),
    /// The snapshots found in the installation (`/wl snapshot`, wowlua
    /// T498), newest first, and warnings for files that can't be read.
    WorldSnapshots(Result<(Vec<WorldSnapshot>, Vec<String>), String>),
    /// Game art read from the installation (none: it has none).
    GameArt(Art, Option<Image>),
    /// The client's spells a search found: the query, the spells.
    Spells(String, Vec<SpellRow>),
    /// The repository's answer (M11): the workspace's root asked about, and
    /// what came back.
    Git(PathBuf, GitEvent),
    /// GitHub's answer (M12): the workspace's root asked about, and what
    /// came back.
    GithubApi(PathBuf, GithubEvent),
    /// GitHub's answer about the signed-in user's repositories (M12, →
    /// [`System::github_account`]).
    Account(AccountEvent),
    /// A clone under way (→ [`System::clone_repo`]): its share done, when
    /// git says, and git's words.
    Cloning {
        repo: String,
        progress: Option<f32>,
        said: String,
    },
    /// A clone done: the folder it is in, or why not.
    Cloned {
        repo: String,
        branch: String,
        result: Result<PathBuf, String>,
    },
    /// What an ID picker's search found (→ [`System::find`]): what, the
    /// flavor, the query, the rows.
    Found(Find, String, String, Vec<SpellRow>),
    /// A class's spells from the client: the class, its spells.
    ClassSpells(String, Vec<ClassSpell>),
    /// The ship page's news (S061).
    Ship(ShipEvent),
    /// The build profiles of the package in a folder, as wowlua resolves
    /// them, for its wowlua.json's form (S064).
    Profiles {
        dir: PathBuf,
        read: Result<Vec<BuildProfile>, String>,
    },
    /// The dependencies page's news (S062).
    Deps(DepsEvent),
    /// The settings page's facts of this computer (S062).
    Settings(SettingsInfo),
    Updates(UpdateCheck),
    Github(GithubSignIn),
}

/// What the game page asks a running session (S055): the debugger's UI
/// requests (wowlua decision T484).
#[derive(Clone, Debug, PartialEq)]
pub enum GameRequest {
    /// The frame tree (hidden frames too or not).
    FrameTree {
        hidden: bool,
    },
    /// One frame's or region's details by its id in the tree.
    Inspect(usize),
    /// What draws at a point of the screen (pixels from the top left).
    Pick {
        x: f64,
        y: f64,
    },
    Units,
    /// The player targets a unit by its index in the units' list.
    Target(Option<usize>),
    /// The events told since the last read.
    Events,
    /// The game's settings: the window's size and the UI scale (none:
    /// "Use UI Scale" off).
    SetScreen {
        width: f64,
        height: f64,
        ui_scale: Option<f64>,
    },
    Input(GameInput),
    /// A PNG of the screen, or of a frame (by its id), written to `to`.
    Screenshot {
        frame: Option<usize>,
        to: PathBuf,
    },
    // The debugger's (S056).
    /// A source's line breakpoints (its path from the package's root).
    SetBreakpoints {
        source: String,
        lines: Vec<LineBp>,
    },
    /// The function, event, script and timer breakpoints.
    SetFunctionBreakpoints(Vec<FunctionBp>),
    /// What stops the session: `uncaught`, `all`, `taint`, `blocked`.
    SetStopOn(Vec<String>),
    Continue,
    Next,
    StepIn,
    StepOut,
    Pause,
    StackTrace,
    Scopes(u32),
    Variables(u32),
    SetVariable {
        reference: u32,
        name: String,
        value: String,
    },
    /// An expression in a frame of the stop (none: the running client's).
    Evaluate {
        expression: String,
        frame: Option<u32>,
        watch: bool,
    },
    /// Time moves on (a manual clock): frames or seconds.
    Advance {
        frames: Option<u32>,
        seconds: Option<f64>,
    },
    SetClock {
        manual: bool,
    },
    /// Stepping into Blizzard's UI code from now on (wowlua T496).
    SetDebugBlizzardUi {
        on: bool,
    },
    /// Every breakpoint with its hit count.
    Breakpoints,
    /// Lua with `mock` the engine's controls (the live world).
    World(String),
    /// A documented event fired with these arguments.
    FireEvent {
        event: String,
        args: Vec<serde_json::Value>,
    },
    /// The running world as a world profile's JSON.
    SaveWorld,
    // The profiler's (S057).
    /// Starts profiling the session (a fresh profile).
    ProfileStart,
    /// Stops it: wowlua's report, its `speedscope` the whole profile.
    ProfileStop,
}

/// A line breakpoint (S056): its line from 1, and a condition, a hit
/// count (`5`, `>5`, `%5`) or a log message (`{expr}` evaluated).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LineBp {
    pub line: u32,
    /// Turned off: kept, not set in a session.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_condition: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_message: Option<String>,
}

/// A function breakpoint: a path (`Module:Update`, `C_Timer.After`),
/// `event:NAME`, `script:OnClick`, `script:Frame:OnUpdate` or `timer`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FunctionBp {
    pub name: String,
    /// Turned off: kept, not set in a session.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
}

/// What a session runs (S056; `.wowlua/launch.json`'s options).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Launch {
    /// A test file (its path from the package's root) and the tests whose
    /// names contain any of `filters`; none: the addon.
    pub test: Option<(String, Vec<String>)>,
    /// The workspace's addon to run (its folder); none: the root's, else
    /// the first addon.
    pub package: Option<PathBuf>,
    pub flavor: Option<String>,
    /// The world profile over `test.profile`.
    pub profile: Option<String>,
    /// The clock moves only when asked.
    pub manual: bool,
    pub stop_on_entry: bool,
    pub debug_blizzard_ui: bool,
    /// The settings' (S062): draw with the installation's art; answer from
    /// its data where a profile defines nothing; the screen of a package
    /// whose `test.screen` sets none.
    pub art: bool,
    pub game_data: bool,
    pub screen: Option<(f64, f64)>,
}

/// Input as the user gives it, in screen pixels from the top left.
#[derive(Clone, Debug, PartialEq)]
pub enum GameInput {
    Click {
        x: f64,
        y: f64,
        right: bool,
    },
    Hover {
        x: f64,
        y: f64,
    },
    Drag {
        x: f64,
        y: f64,
        dx: f64,
        dy: f64,
    },
    /// Text typed into the focused edit box.
    Type(String),
    /// A key or binding by WoW's name (`ESCAPE`, `SHIFT-1`).
    Key(String),
    /// A slash command (`/wl`).
    Slash(String),
}

/// What a game session tells.
#[derive(Clone, Debug, PartialEq)]
pub enum GameEvent {
    /// What it does before the addon runs: building, fetching Blizzard's
    /// UI, reading the installation's art.
    Status(String),
    /// The addon runs: its name and flavor, and the installation whose art
    /// it draws with (none: the stand-ins).
    Started {
        addon: String,
        flavor: String,
        installation: Option<String>,
    },
    /// A request's answer: its data (JSON) or the error.
    Reply {
        id: u64,
        result: Result<serde_json::Value, String>,
    },
    /// Printed text and errors (`stdout`, `stderr`, `console`).
    Output { category: String, text: String },
    /// The session ended (an error that ended it), what it said at the end.
    Ended { error: Option<String> },
    /// It stopped (S056): why (`breakpoint`, `step`, `pause`, `exception`,
    /// `entry`, `functionBreakpoint`), the text, the breakpoints hit, an
    /// exception's details.
    Stopped {
        reason: String,
        text: Option<String>,
        hit: Vec<u32>,
        detail: Option<serde_json::Value>,
    },
    /// Running again after a stop.
    Continued,
    /// A breakpoint's state changed (its chunk loaded).
    Breakpoint(serde_json::Value),
    /// The UI reloaded (`/reload`).
    Reloaded,
}

/// The game's screen as last drawn: its texture and size in pixels, and
/// how many scenes were drawn (a new one: changed).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GameScreen {
    pub texture: egui::TextureId,
    pub size: [u32; 2],
    pub drawn: u64,
    /// The game's time (`GetTime`, seconds) and the frames run, as last
    /// drawn.
    pub time: f64,
    pub frames: u64,
}

/// A test run (the tests page, S052): which tests (`path::name` filters,
/// all without one), with coverage or not, snapshots and screenshots
/// written anew or not, in another world profile than `test.profile`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TestRequest {
    pub filters: Vec<String>,
    pub coverage: bool,
    pub update_snapshots: bool,
    pub profile: Option<String>,
    /// Profile each test file: kept in the workspace's
    /// `.wowlua/profiles` (S057).
    pub profiler: bool,
}

/// What a test run tells as it goes.
#[derive(Clone, Debug, PartialEq)]
pub enum TestEvent {
    /// More test files to run (a flavor's, once built).
    Planned(usize),
    /// What it does now: building an addon, fetching Blizzard's UI.
    Status(String),
    /// A test file's results.
    File(TestFile),
    /// A test file's profile, kept at `path` (a profiling run).
    Profiled {
        file: String,
        path: PathBuf,
    },
    Done(TestDone),
}

/// A test file's run for a flavor: its package (the root), the file
/// relative to it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TestFile {
    pub package: PathBuf,
    pub file: String,
    pub flavor: String,
    pub tests: Vec<TestCase>,
    /// The file could not run (it does not compile, an error loading it).
    pub error: Option<String>,
    /// What its code printed.
    pub printed: String,
    pub seconds: f64,
}

impl TestFile {
    pub fn failed(&self) -> bool {
        self.error.is_some()
            || self
                .tests
                .iter()
                .any(|t| matches!(t.outcome, TestOutcome::Fail { .. }))
    }
}

/// A test's (a case's) result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TestCase {
    pub name: String,
    pub outcome: TestOutcome,
    pub seconds: f64,
    /// With coverage: the functions it called, by source (relative to the
    /// package) and the line each is defined on.
    pub functions: Vec<(String, u32)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TestOutcome {
    Pass,
    /// The message and where it happened.
    Fail {
        message: String,
        stack: String,
    },
    Skip(String),
}

/// How a test run ended.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TestDone {
    /// Nothing could run: the error (the package does not build).
    pub error: Option<String>,
    /// Stopped before every file ran.
    pub stopped: bool,
    /// With coverage: the run's.
    pub coverage: Option<Coverage>,
    /// The coverage kinds below `test.coverage`'s minimums: the kind, the
    /// run's percentage, the minimum (a whole suite's run only).
    pub below: Vec<(String, f64, f64)>,
    /// Snapshots and screenshots written, updated, removed.
    pub snapshots: (usize, usize, usize),
    pub seconds: f64,
}

/// A profile (S057), as wowlua keeps it in a speedscope file: its
/// functions and dispatches, the call tree with each node's calls and own
/// costs (instructions, bytes allocated, host nanoseconds), and the game
/// frames with the trees of the newest.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Profile {
    pub name: String,
    pub keys: Vec<ProfileKey>,
    /// The tree, the root first; a parent before its children.
    pub nodes: Vec<ProfileNode>,
    pub frames: Vec<ProfileFrame>,
}

/// A function (its source and the line it is defined on) or a dispatch
/// (its kind: `event`, `script`, `update`, `timer`, `frame`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProfileKey {
    pub name: String,
    pub file: Option<String>,
    pub line: u32,
    pub kind: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProfileNode {
    pub key: u32,
    pub parent: u32,
    pub calls: u64,
    /// Instructions, bytes allocated, host nanoseconds.
    pub cost: [u64; 3],
}

/// A game frame: its cost, and the nodes that ran in it with their calls
/// and costs there (empty when its tree was not kept).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProfileFrame {
    pub name: String,
    pub cost: [u64; 3],
    pub nodes: Vec<(u32, u64, [u64; 3])>,
}

/// A profile kept in the workspace (`.wowlua/profiles`): its file, its
/// name and when it was written (Unix seconds).
#[derive(Clone, Debug, PartialEq)]
pub struct SavedProfile {
    pub path: PathBuf,
    pub name: String,
    pub modified: u64,
}

/// The recordings the dev window exported in the game: a client folder's
/// account, its SavedVariables file, and each recording (wowlua's export
/// format, its locations in the source).
#[derive(Clone, Debug, PartialEq)]
pub struct GameRecordings {
    pub client: String,
    pub account: String,
    pub path: PathBuf,
    pub recordings: Vec<serde_json::Value>,
}

/// Game art from the installation (S060).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Art {
    /// A texture by file ID.
    File(u32),
    /// A class's icon, by file name (`WARRIOR`): the class sheet's part.
    Class(String),
    /// A spell's icon, by spell ID.
    Spell(u32),
    /// An item's icon, by item ID.
    Item(u32),
}

/// A spell of the client's tables: its ID, name, icon and description.
#[derive(Clone, Debug, PartialEq)]
pub struct SpellRow {
    pub id: u32,
    pub name: String,
    pub icon: Option<u32>,
    pub text: String,
}

/// A spell of a class's skill lines: its ID, name, rank, the level it is
/// learned at, its skill line and icon.
#[derive(Clone, Debug, PartialEq)]
pub struct ClassSpell {
    pub id: u32,
    pub name: String,
    pub rank: String,
    pub level: u32,
    pub line: String,
    pub icon: Option<u32>,
}

/// What Studio asks of the workspace's repository on GitHub (M12, →
/// [`Event::GithubApi`]).
#[derive(Clone, Debug, PartialEq)]
pub enum GithubRequest {
    /// The repository the workspace's remote names, its default branch,
    /// and who is signed in.
    Repo,
    /// The open pull requests.
    Pulls,
    /// A pull request with its comments, reviews and files.
    Pull(u64),
    /// The recent Actions runs.
    Runs,
    /// A run's jobs and their steps.
    Jobs(u64),
    /// A job's log and annotations.
    Log(u64),
    Comment {
        number: u64,
        body: String,
    },
    /// A reply in a review thread (to its first comment).
    Reply {
        number: u64,
        to: u64,
        body: String,
    },
    /// A review comment on a line of a file of the pull request's head.
    LineComment {
        number: u64,
        commit: String,
        path: String,
        line: u64,
        side: String,
        body: String,
    },
    /// A review, with the line comments kept for it.
    Review {
        number: u64,
        event: crate::github::model::ReviewEvent,
        body: String,
        comments: Vec<crate::github::model::Pending>,
    },
    /// A review thread resolved, or opened again.
    Resolve {
        thread: String,
        resolve: bool,
    },
    /// The repository's branches on GitHub (a new pull request's base and
    /// head).
    Branches,
    /// What `head` brings over `base` (a new pull request's commits and
    /// files).
    Compare {
        base: String,
        head: String,
    },
    Merge {
        number: u64,
        method: crate::github::model::MergeMethod,
    },
    Create {
        title: String,
        body: String,
        head: String,
        base: String,
        draft: bool,
    },
    /// A run again: all of it, or its failed jobs.
    Rerun {
        run: u64,
        failed_only: bool,
    },
    Cancel(u64),
    /// A fork of the repository as the signed-in user, with the branch
    /// checked out pushed to it (a read-only repository's commits kept).
    Fork,
}

/// What Studio asks GitHub about the signed-in user's repositories (M12,
/// → [`Event::Account`]).
#[derive(Clone, Debug, PartialEq)]
pub enum AccountRequest {
    /// Yours and the ones you can push to, recently pushed first.
    Repos,
    /// A repository by its `owner/name`.
    Lookup(String),
    /// A repository's branches.
    Branches(String),
}

/// What GitHub answered (→ [`System::github_account`]).
#[derive(Clone, Debug, PartialEq)]
pub enum AccountEvent {
    Repos(Result<Vec<crate::github::model::RepoInfo>, crate::github::model::GithubError>),
    Lookup {
        repo: String,
        result: Result<crate::github::model::RepoInfo, crate::github::model::GithubError>,
    },
    Branches {
        repo: String,
        result: Result<Vec<String>, crate::github::model::GithubError>,
    },
}

/// What deleting a clone of Studio's would lose (M12): what isn't on
/// GitHub.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Losses {
    /// Files changed and not committed, untracked ones too.
    pub changed: Vec<String>,
    /// Commits on no remote: the short hash and the subject.
    pub unpushed: Vec<(String, String)>,
    pub stashes: usize,
}

impl Losses {
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.unpushed.is_empty() && self.stashes == 0
    }
}

/// A clone left in Studio's data by a window that quit while it was open,
/// with something to lose.
#[derive(Clone, Debug, PartialEq)]
pub struct LeftClone {
    pub path: PathBuf,
    /// `owner/name`.
    pub repo: String,
    pub branch: Option<String>,
    pub losses: Losses,
}

/// What GitHub answered (→ [`System::github`]).
#[derive(Clone, Debug, PartialEq)]
pub enum GithubEvent {
    /// The repository, its default branch, the signed-in login, whether
    /// they can push to it.
    Repo(
        Result<
            (crate::github::model::Repo, String, String, bool),
            crate::github::model::GithubError,
        >,
    ),
    Pulls(Result<Vec<crate::github::model::PullSummary>, crate::github::model::GithubError>),
    Pull {
        number: u64,
        result: Result<Box<crate::github::model::PullDetail>, crate::github::model::GithubError>,
    },
    Runs(Result<Vec<crate::github::model::Run>, crate::github::model::GithubError>),
    Branches(Result<Vec<String>, crate::github::model::GithubError>),
    Compare {
        base: String,
        head: String,
        result: Result<crate::github::model::Compare, crate::github::model::GithubError>,
    },
    Jobs {
        run: u64,
        result: Result<Vec<crate::github::model::Job>, crate::github::model::GithubError>,
    },
    Log {
        job: u64,
        result: Result<
            (String, Vec<crate::github::model::Annotation>),
            crate::github::model::GithubError,
        >,
    },
    /// A change made (a comment, review, merge, pull request, re-run,
    /// cancel): what it was, and what came of it (a pull request's number,
    /// a link) or why not.
    Done {
        what: String,
        result: Result<String, crate::github::model::GithubError>,
    },
}

/// What Studio asks of the repository (M11, → [`Event::Git`]).
#[derive(Clone, Debug, PartialEq)]
pub enum GitRequest {
    Status,
    Branches,
    /// The last commits of a branch or commit (HEAD when none).
    Log {
        rev: Option<String>,
        count: usize,
    },
    Diff {
        path: String,
        of: crate::git::model::DiffOf,
    },
    CommitFiles(String),
    Stage(Vec<String>),
    Unstage(Vec<String>),
    /// Changes thrown away (asked first: they can't be brought back).
    Discard(Vec<String>),
    Hunk {
        path: String,
        hunk: crate::git::model::Hunk,
        how: crate::git::model::Apply,
    },
    /// A commit of what is staged: its `.wowlua` files formatted and
    /// staged again, then checked, first when asked (S069); pushed after
    /// when asked.
    Commit {
        message: String,
        format_and_check: bool,
        push: bool,
    },
    Push,
    Pull,
    /// A fetch: `quiet` when Studio fetches on its own (S070).
    Fetch {
        quiet: bool,
    },
    /// The changed files a switch to `target` would overwrite.
    Blocking(String),
    Switch {
        target: String,
        carry: crate::git::model::Carry,
    },
    SwitchCommit(String),
    NewBranch {
        name: String,
        from: Option<String>,
        switch: bool,
    },
    BranchName(String),
    Revert(String),
    TakeSide {
        path: String,
        side: crate::git::model::Side,
    },
    Abort,
    /// Who last changed each line of a file as the editor has it.
    Blame {
        path: String,
        contents: String,
    },
    HeadText(String),
}

/// What a commit stopped at (S069): the staged files' errors, or git's.
#[derive(Clone, Debug, PartialEq)]
pub enum CommitStop {
    /// wowlua check's errors in staged files: file, line (1-based), code
    /// and message.
    Errors {
        formatted: Vec<String>,
        errors: Vec<(String, u32, String, String)>,
    },
    Failed(String),
}

/// A commit made (S069).
#[derive(Clone, Debug, PartialEq)]
pub struct Committed {
    pub hash: String,
    pub message: String,
    /// The staged files wowlua fmt changed and staged again.
    pub formatted: Vec<String>,
    /// Files with unstaged changes too: checked, not formatted.
    pub not_formatted: Vec<String>,
    pub checked: bool,
    pub warnings: usize,
    /// What the repository's hooks printed.
    pub hooks: String,
    /// The push after it, when one was asked for.
    pub pushed: Option<Result<String, crate::git::model::RemoteError>>,
}

/// What the repository answered (→ [`System::git`]).
#[derive(Clone, Debug, PartialEq)]
pub enum GitEvent {
    Status(Result<crate::git::model::Status, String>),
    Branches(Result<Vec<crate::git::model::Branch>, String>),
    Log {
        rev: Option<String>,
        result: Result<Vec<crate::git::model::Commit>, String>,
    },
    Diff {
        path: String,
        of: crate::git::model::DiffOf,
        result: Result<crate::git::model::Diff, String>,
    },
    CommitFiles {
        hash: String,
        result: Result<Vec<crate::git::model::CommitFile>, String>,
    },
    /// A change of the repository done (staging, switching, a branch, a
    /// revert, a conflict taken): what it did, or why not.
    Done {
        what: String,
        result: Result<String, String>,
    },
    Committed(Result<Committed, CommitStop>),
    Remote {
        what: String,
        quiet: bool,
        result: Result<String, crate::git::model::RemoteError>,
    },
    Blocking {
        target: String,
        files: Result<Vec<String>, String>,
    },
    BranchName {
        name: String,
        bad: Option<String>,
    },
    Blame {
        path: String,
        result: Result<Vec<crate::git::model::BlameLine>, String>,
    },
    HeadText {
        path: String,
        text: Option<String>,
    },
}

/// What an ID picker finds in the flavor's client: a spell or an item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Find {
    Spell,
    Item,
}

impl Find {
    /// As the picker names it.
    pub fn word(self) -> &'static str {
        match self {
            Find::Spell => "spell",
            Find::Item => "item",
        }
    }
}

/// A CVar of a flavor's client: its name, default, and whether addons may
/// not set it (secure) or nobody may (read-only).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CVarInfo {
    pub name: String,
    pub default: String,
    pub secure: bool,
    pub read_only: bool,
}

/// A build profile: a built-in one (`dev`, `release`, `test`) or the
/// manifest's (S061).
#[derive(Clone, Debug, PartialEq)]
pub struct BuildProfile {
    pub name: String,
    pub builtin: bool,
    /// Its `inherits` chain, base first, itself last.
    pub chain: Vec<String>,
    /// Every setting as wowlua resolves it, by `wowlua.json`'s keys.
    pub effective: serde_json::Value,
    /// Why it does not resolve.
    pub problem: Option<String>,
}

/// The package's AddOn folder in a client folder of the installation.
#[derive(Clone, Debug, PartialEq)]
pub struct InstallFolder {
    /// The client folder (`_retail_`).
    pub client: String,
    pub path: PathBuf,
    pub state: InstallState,
}

#[derive(Clone, Debug, PartialEq)]
pub enum InstallState {
    Absent,
    /// A link to a build folder.
    Linked(PathBuf),
    /// Copied by wowlua: what changed since (none: no record, an older
    /// wowlua's install).
    Copied(Option<Vec<InstallChange>>),
    /// A folder wowlua did not install (E4331 unless replaced anyway).
    Foreign,
}

/// A file of an installed folder that is not what wowlua copied (wowlua
/// T502).
#[derive(Clone, Debug, PartialEq)]
pub struct InstallChange {
    /// Its path in the AddOn folder, with `/`.
    pub path: String,
    pub kind: ChangeKind,
    /// When it was last written (ms since the epoch), while it is there.
    pub modified: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ChangeKind {
    Changed { was: u64, now: u64 },
    Added { size: u64 },
    Removed,
}

/// A profile's build output (`dist/<profile>/<Addon>`).
#[derive(Clone, Debug, PartialEq)]
pub struct BuiltOutput {
    pub profile: String,
    pub dir: PathBuf,
    /// Its files with their sizes, by path with `/`.
    pub files: Vec<(String, u64)>,
    /// When it was written (ms since the epoch).
    pub modified: Option<u64>,
}

/// A publish target of the manifest.
#[derive(Clone, Debug, PartialEq)]
pub struct ShipTarget {
    pub name: String,
    /// `curseforge`, `wago`, `wowinterface`, `github` or `registry`: the
    /// token's kind (`wowlua login <kind>`).
    pub kind: String,
    pub project: String,
    /// The build profile it gets.
    pub profile: String,
    /// The environment variable of its token, when the manifest names one.
    pub token_env: Option<String>,
    pub enabled: bool,
}

/// What the ship page shows of the package (→ [`ShipEvent::Read`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShipFacts {
    /// A workspace without a package of its own: its addons (name and
    /// folder) to pick one from; the other facts are empty.
    pub members: Vec<(String, PathBuf)>,
    /// The AddOn folder's name.
    pub addon: String,
    pub version: String,
    pub manifest: PathBuf,
    /// The build output folder (`build.out`).
    pub out: PathBuf,
    pub profiles: Vec<BuildProfile>,
    pub outputs: Vec<BuiltOutput>,
    pub installs: Vec<InstallFolder>,
    /// Why the installation's folders are not known.
    pub install_problem: Option<String>,
    pub targets: Vec<ShipTarget>,
    /// `release`, `beta` or `alpha`, from the version.
    pub release: String,
    /// The repository's root, its branch and the remote it pushes to.
    pub repo: Option<PathBuf>,
    pub branch: Option<String>,
    pub remote: Option<String>,
    /// The working tree has no changes (a publish needs a clean tree).
    pub clean: bool,
    /// The tag this version gets (`v1.2.0`, or `1.2.0` where the
    /// repository's tags have no `v`), and whether it exists: on `HEAD`
    /// (`true`) or another commit.
    pub tag: String,
    pub tag_at_head: Option<bool>,
    /// The changelog file and this version's section in it.
    pub changelog: Option<(PathBuf, String)>,
    /// The CI workflow that runs `wowlua publish`, when there is one.
    pub ci: Option<String>,
}

/// A publish token's state (wowlua's credentials, decision T501).
#[derive(Clone, Debug, PartialEq)]
pub enum TokenState {
    Saved,
    Missing,
    /// From this environment variable.
    FromEnv(String),
    /// The credential store could not be read.
    Unreadable(String),
}

/// A step of a build (wowlua's `build::Step`, T503), or the install after.
#[derive(Clone, Debug, PartialEq)]
pub enum BuildStep {
    Read {
        files: usize,
    },
    Parsed {
        done: usize,
        of: usize,
    },
    Linted {
        done: usize,
        of: usize,
    },
    Checking {
        flavor: Option<String>,
        index: usize,
        count: usize,
    },
    Bundling,
    Writing,
    Installing,
}

/// A diagnostic of a build or publish, where it points.
#[derive(Clone, Debug, PartialEq)]
pub struct ShipDiagnostic {
    /// `error`, `warning`, `note` or `help`.
    pub severity: String,
    pub code: Option<String>,
    pub message: String,
    pub path: Option<PathBuf>,
    /// 1-based, where it points.
    pub line: Option<u32>,
    pub column: Option<u32>,
}

/// What to build.
#[derive(Clone, Debug, PartialEq)]
pub struct ShipBuild {
    pub profile: String,
    /// Install it afterwards.
    pub install: Option<ShipInstall>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShipInstall {
    /// Link the AddOn folder to the build folder instead of copying.
    pub link: bool,
    /// Replace a folder wowlua did not install.
    pub force: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BuildDone {
    pub ok: bool,
    pub profile: String,
    pub dir: Option<PathBuf>,
    pub ms: u64,
    pub diagnostics: Vec<ShipDiagnostic>,
    /// The AddOn folders written into the installation.
    pub installed: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BuildEvent {
    Step(BuildStep),
    Done(BuildDone),
}

/// A build of the dev loop (`wowlua dev`).
#[derive(Clone, Debug, PartialEq)]
pub struct DevBuild {
    /// When it started (ms since the epoch), and its local time
    /// (`10:58:12`).
    pub at: u64,
    pub clock: String,
    /// The changed files that started it (package-relative), none for the
    /// first build.
    pub changed: Vec<String>,
    pub ok: bool,
    pub ms: u64,
    pub diagnostics: Vec<ShipDiagnostic>,
    /// The AddOn folders it wrote into the installation.
    pub synced: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DevEvent {
    /// Watching the packages (their names).
    Watching(Vec<String>),
    Building(Vec<String>),
    Built(DevBuild),
    /// The loop ended (why, when it could not go on).
    Stopped(Option<String>),
}

/// What to publish (`wowlua publish`).
#[derive(Clone, Debug, PartialEq)]
pub struct ShipPublish {
    pub dry_run: bool,
    /// The targets, by name (none: every enabled one).
    pub targets: Vec<String>,
    /// Run the tests first.
    pub tests: bool,
    /// The version's tag (`wowlua publish --tag`): made locally once the
    /// tests pass, when it does not exist.
    pub tag: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PublishOutcome {
    pub target: String,
    pub file: String,
    pub ok: bool,
    pub skipped: bool,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PublishDone {
    pub ok: bool,
    pub dry_run: bool,
    pub outcomes: Vec<PublishOutcome>,
    pub notes: Vec<String>,
    pub diagnostics: Vec<ShipDiagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PublishEvent {
    /// A step, as `wowlua publish` says it (`Packaged`, `Reading`, …).
    Line {
        label: String,
        text: String,
    },
    Done(PublishDone),
}

/// Tag a version and push it (S061: after the user confirms).
#[derive(Clone, Debug, PartialEq)]
pub struct TagPush {
    pub tag: String,
    pub message: String,
    pub remote: String,
    pub branch: String,
}

impl TagPush {
    /// The commands, as the confirm shows them and Studio runs them.
    pub fn commands(&self) -> Vec<Vec<String>> {
        vec![
            vec![
                "git".into(),
                "tag".into(),
                "-a".into(),
                self.tag.clone(),
                "-m".into(),
                self.message.clone(),
            ],
            vec![
                "git".into(),
                "push".into(),
                self.remote.clone(),
                self.branch.clone(),
                self.tag.clone(),
            ],
        ]
    }
}

/// The ship page's news (S061).
#[derive(Clone, Debug, PartialEq)]
pub enum ShipEvent {
    Read(Box<Result<ShipFacts, String>>),
    /// Each target's token's state, by the target's name.
    Tokens(Vec<(String, TokenState)>),
    /// A token saved (where) or removed, or why not.
    TokenSaved {
        kind: String,
        result: Result<String, String>,
    },
    Build {
        job: u64,
        event: BuildEvent,
    },
    Dev(DevEvent),
    Publish {
        job: u64,
        event: PublishEvent,
    },
    /// Tagged and pushed (git's output), or why not.
    Tagged(Result<String, String>),
}

/// What the settings page shows of this computer (→ [`Event::Settings`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettingsInfo {
    /// The language server's check cache on disk, in bytes.
    pub check_cache: Option<u64>,
    /// The installation's art kept on disk, in bytes, and how many files.
    pub art_cache: Option<(u64, usize)>,
    /// Studio's log file.
    pub log: Option<PathBuf>,
    /// Signed in to GitHub as.
    pub github: Option<String>,
    /// A Claude API key is saved.
    pub claude_key: bool,
}

/// Studio's releases (`check for updates`).
#[derive(Clone, Debug, PartialEq)]
pub enum UpdateCheck {
    /// This is the newest.
    Newest,
    Newer {
        version: String,
        url: String,
    },
    Failed(String),
}

/// GitHub's device sign-in (S062): the code to enter, then the account.
#[derive(Clone, Debug, PartialEq)]
pub enum GithubSignIn {
    /// Enter `code` at `url`; the sign-in waits for it.
    Code {
        code: String,
        url: String,
    },
    SignedIn(String),
    Failed(String),
}

/// A dependency the package declares, as it resolved (S062).
#[derive(Clone, Debug, PartialEq)]
pub struct DepRow {
    pub name: String,
    /// In `dev-dependencies`.
    pub dev: bool,
    /// `registry`, `git · owner/repo`, `svn · host`, `path · libs/X`,
    /// `archive · host`.
    pub source: String,
    /// From a registry: a version requirement, versions to choose from.
    pub registry: bool,
    pub kind: String,
    /// What `wowlua.json` asks: `^1.2`, `tag Release-r1390`, `path`.
    pub requirement: String,
    /// The locked version (and tag, commit or revision); empty when it
    /// did not resolve.
    pub locked: String,
    pub yanked: bool,
    pub description: String,
}

/// What the dependencies page shows of a package (→ [`DepsEvent::Read`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DepsFacts {
    pub package: String,
    pub manifest: PathBuf,
    pub rows: Vec<DepRow>,
    /// The resolution's problems and warnings (W4322 for a yanked version),
    /// as compiler output.
    pub problems: Vec<String>,
}

/// A newer version, tag, commit or revision, and what brings it in
/// (`wowlua outdated`).
#[derive(Clone, Debug, PartialEq)]
pub struct DepNewer {
    pub name: String,
    pub latest: String,
    pub action: String,
}

/// A registry package a search found.
#[derive(Clone, Debug, PartialEq)]
pub struct DepFound {
    pub name: String,
    pub newest: String,
    pub description: String,
}

/// What the dependencies page changes, as `wowlua add`, `remove` and
/// `update` do.
#[derive(Clone, Debug, PartialEq)]
pub enum DepsCommand {
    /// Adds a registry package, or changes a dependency's requirement.
    Add {
        name: String,
        requirement: Option<String>,
        dev: bool,
    },
    Remove {
        name: String,
        dev: bool,
    },
    /// Updates these (none: every dependency).
    Update(Vec<String>),
}

impl DepsCommand {
    /// The command as the CLI writes it.
    pub fn line(&self) -> String {
        match self {
            DepsCommand::Add {
                name,
                requirement,
                dev,
            } => format!(
                "wowlua add {name}{}{}",
                requirement
                    .as_ref()
                    .map_or(String::new(), |r| format!("@{r}")),
                if *dev { " --dev" } else { "" }
            ),
            DepsCommand::Remove { name, dev } => {
                format!("wowlua remove {name}{}", if *dev { " --dev" } else { "" })
            }
            DepsCommand::Update(names) if names.is_empty() => "wowlua update".into(),
            DepsCommand::Update(names) => format!("wowlua update {}", names.join(" ")),
        }
    }
}

/// The dependencies page's news (S062).
#[derive(Clone, Debug, PartialEq)]
pub enum DepsEvent {
    Read(Box<Result<DepsFacts, String>>),
    /// `wowlua outdated`: newer ones, or why not (offline).
    Outdated(Result<Vec<DepNewer>, String>),
    Searched {
        query: String,
        found: Result<Vec<DepFound>, String>,
    },
    /// A registry package's versions, newest first, and whether each was
    /// yanked.
    Versions {
        name: String,
        versions: Vec<(String, bool)>,
    },
    /// A command ran: its lines (`Updating a 1.2.1 -> 1.2.3`) or its
    /// errors.
    Ran {
        job: u64,
        command: String,
        ok: bool,
        output: Vec<String>,
    },
}

/// A world profile of the package (`tests/profiles/<name>.json`).
#[derive(Clone, Debug, PartialEq)]
pub struct WorldProfile {
    pub name: String,
    pub path: PathBuf,
}

/// A problem of a world profile's text (E4400, E4401): its bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileProblem {
    pub code: String,
    pub message: String,
    pub start: usize,
    pub end: usize,
}

/// A snapshot taken in the game (`/wl snapshot`): where, when, and the
/// world profile it makes (its names scrubbed).
#[derive(Clone, Debug, PartialEq)]
pub struct WorldSnapshot {
    pub client: String,
    pub account: String,
    pub path: PathBuf,
    /// Unix seconds.
    pub at: Option<f64>,
    /// The profile's JSON.
    pub text: String,
    /// The parts the client refused.
    pub failed: Vec<String>,
}

/// Covered and all, of a kind.
pub type Count = (u64, u64);

/// A coverage's totals (wowlua's `coverage.json`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CoverageTotals {
    pub lines: Count,
    pub branches: Count,
    pub functions: Count,
    pub conditions: Count,
}

/// A source file's coverage: each line's hits, each branch's counts (the
/// line and its ordinal: taken, not taken), each function's calls (its
/// line: name, calls).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CoverageFile {
    pub totals: CoverageTotals,
    pub lines: BTreeMap<u32, u64>,
    pub branches: BTreeMap<(u32, u32), [u64; 2]>,
    pub functions: BTreeMap<u32, (String, u64)>,
}

/// A run's coverage, by source file (absolute paths).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Coverage {
    pub totals: CoverageTotals,
    pub files: BTreeMap<PathBuf, CoverageFile>,
    /// The HTML report, when one was written.
    pub html: Option<PathBuf>,
}

/// An image to show (a screenshot test's): its size and RGBA pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// How a file changed on disk, for the language server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileChange {
    Created,
    Changed,
    Deleted,
}

/// A text search across the workspace (the search board's text tab).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Search {
    pub query: String,
    pub case: bool,
    pub word: bool,
    pub regex: bool,
}

/// How many matches the find bar counts and marks.
pub const MOST_FOUND: usize = 10_000;

/// A match of the find bar: its bytes, and what replaces it.
#[derive(Clone, Debug, PartialEq)]
pub struct FoundIn {
    pub start: usize,
    pub end: usize,
    pub replacement: Option<String>,
}

/// The matches of a search that is no regular expression: its case or
/// not (ASCII letters), a whole word or not.
pub fn find_literal(text: &str, search: &Search) -> Vec<(usize, usize)> {
    let query = search.query.as_str();
    let mut out = Vec::new();
    if query.is_empty() {
        return out;
    }
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let mut at = 0;
    while at + query.len() <= text.len() {
        let here = &text[at..];
        let found = if search.case {
            here.find(query)
        } else {
            here.char_indices().map(|(i, _)| i).find(|&i| {
                here.get(i..i + query.len())
                    .is_some_and(|s| s.eq_ignore_ascii_case(query))
            })
        };
        let Some(i) = found else {
            break;
        };
        let (start, end) = (at + i, at + i + query.len());
        let whole = !word(text[..start].chars().next_back()) && !word(text[end..].chars().next());
        if !search.word || whole {
            out.push((start, end));
            at = end;
        } else {
            at = start + text[start..].chars().next().map_or(1, char::len_utf8);
        }
    }
    out
}

/// A match: its line (from 0), where it is in the line in UTF-16 units
/// (as the language server counts), and the piece of the line around it
/// that is shown, the match its characters `start..end`. A line can be
/// megabytes long (a data file on one line): only the piece is kept.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub line: usize,
    pub at: (usize, usize),
    pub text: String,
    pub start: usize,
    pub end: usize,
}

/// What a search found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Found {
    pub files: Vec<(PathBuf, Vec<Hit>)>,
    pub searched: usize,
    pub millis: u64,
    /// The query is no regular expression, or nothing could be read.
    pub error: Option<String>,
    /// More matches than are kept.
    pub cut: bool,
}

/// A colour of a terminal's cell: the terminal's own, one of the sixteen
/// ANSI colours, one of the 256, or a colour of its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TermColor {
    #[default]
    Default,
    Ansi(u8),
    Indexed(u8),
    Rgb([u8; 3]),
}

/// A cell of a terminal's screen.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TermCell {
    pub c: char,
    pub fg: TermColor,
    pub bg: TermColor,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    pub dim: bool,
    /// The second half of a wide character: nothing to draw.
    pub spacer: bool,
    /// In the selection.
    pub selected: bool,
}

/// What a selection in a terminal takes: characters (a drag), words (a
/// double click), lines (a triple click).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TermSelectKind {
    Chars,
    Words,
    Lines,
}

/// A step of selecting in a terminal, at a row and column of its screen;
/// `right` is the cell's right half.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TermSelect {
    Start {
        row: usize,
        col: usize,
        right: bool,
        kind: TermSelectKind,
    },
    Extend {
        row: usize,
        col: usize,
        right: bool,
    },
    Clear,
}

/// What a terminal shows now (S043).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TermScreen {
    pub rows: Vec<Vec<TermCell>>,
    /// Row and column; `None` when hidden or scrolled away.
    pub cursor: Option<(usize, usize)>,
    /// The arrows send application sequences (`ESC O A`).
    pub app_cursor: bool,
    pub bracketed_paste: bool,
    /// Lines scrolled back from the bottom.
    pub scrolled: usize,
    /// The shell ended.
    pub exited: bool,
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
    /// The system's file picker, files with these endings (a speedscope
    /// file, S057).
    fn pick_file(&self, _title: &str, _endings: &[&str]) -> Option<PathBuf> {
        None
    }
    /// The folder's files and folders, sorted; hidden ones and build output
    /// left out.
    fn list(&self, dir: &Path) -> Vec<Entry>;
    fn read_file(&self, path: &Path) -> Result<String, String>;
    fn write_file(&self, path: &Path, text: &str) -> Result<(), String>;
    /// The highlighting of a wowlua or Lua file's text: wowlua's lexer,
    /// and over its names the server's semantic tokens (S045).
    fn highlight(&self, text: &str) -> Vec<Highlight>;
    /// The lines a wowlua or Lua file's text folds: first and last, the
    /// last staying in sight.
    fn folding(&self, text: &str) -> Vec<(usize, usize)>;
    /// The JSON schema wowlua makes of `wowlua.json` (`world`: of a world
    /// profile), as text (S046).
    fn schema(&self, world: bool) -> Option<String>;
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
    /// A new window showing the start page's from-GitHub panel (M12).
    fn open_window_from_github(&self) {}
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
    /// Searches the files under `root` (→ [`Event::Searched`]): ignored
    /// and hidden files left out, as Git leaves them.
    fn search(&self, root: &Path, search: Search);
    /// The matches of `search` in `text` (the find bar), as byte ranges,
    /// the first [`MOST_FOUND`]; with `replace`, each one's replacement
    /// (a regular expression's `$1` … its groups). Without regular
    /// expressions here: the desktop's system has them.
    fn find_in(
        &self,
        text: &str,
        search: &Search,
        replace: Option<&str>,
    ) -> Result<Vec<FoundIn>, String> {
        if search.regex {
            return Err("regular expressions are found by the desktop app".to_owned());
        }
        Ok(find_literal(text, search)
            .into_iter()
            .take(MOST_FOUND)
            .map(|(start, end)| FoundIn {
                start,
                end,
                replacement: replace.map(str::to_owned),
            })
            .collect())
    }
    /// Starts the user's shell in a terminal of `cols` × `rows` in `cwd`
    /// (S043); returns its id. The browser has none.
    fn terminal_open(&self, cwd: &Path, cols: u16, rows: u16) -> Result<u64, String>;
    /// Keys and text typed into the terminal, as bytes.
    fn terminal_input(&self, id: u64, bytes: &[u8]);
    fn terminal_resize(&self, id: u64, cols: u16, rows: u16);
    /// Scrolls the terminal back (positive) or forward, in lines.
    fn terminal_scroll(&self, id: u64, lines: i32);
    fn terminal_screen(&self, id: u64) -> Option<TermScreen>;
    fn terminal_close(&self, id: u64);
    /// Selects in the terminal: starts, extends or clears its selection.
    fn terminal_select(&self, _id: u64, _select: TermSelect) {}
    /// The terminal's selected text.
    fn terminal_selection(&self, _id: u64) -> Option<String> {
        None
    }
    /// The system clipboard's text (`ctrl shift v` in the terminal).
    fn clipboard_text(&self) -> Option<String> {
        None
    }
    /// The test files (`*.test.wowlua`) under a package's root, as Git
    /// sees the folder.
    fn test_files(&self, _root: &Path) -> Vec<PathBuf> {
        Vec::new()
    }
    /// Runs the workspace's tests (→ [`Event::Tests`]): run `run`.
    fn test_run(&self, _root: &Path, _run: u64, _request: TestRequest) {}
    /// Stops the test run: no more files run.
    fn test_stop(&self) {}
    /// The test files a change to `changed` may change the results of:
    /// itself, and those that import it, directly or not (run on save).
    fn affected_tests(&self, _root: &Path, _changed: &Path) -> Vec<PathBuf> {
        Vec::new()
    }
    /// The last run's coverage the workspace's `dist/test/coverage.json`
    /// keeps (`wowlua test --coverage`'s too).
    fn coverage_report(&self, _root: &Path) -> Option<Coverage> {
        None
    }
    /// A PNG's pixels (a screenshot test's images).
    fn read_image(&self, _path: &Path) -> Option<Image> {
        None
    }
    /// Copies a file over another (accepting a screenshot).
    fn copy_file(&self, from: &Path, _to: &Path) -> Result<(), String> {
        Err(format!("{} cannot be copied here", from.display()))
    }
    /// Runs a session of the workspace's addon, or a test file (→
    /// [`Event::Game`]): session `session` as `launch` says, `setup` (its
    /// breakpoints, each with its request's id) asked before it runs; one at a time, a new one ends the
    /// last.
    fn game_start(
        &self,
        _root: &Path,
        _session: u64,
        _launch: Launch,
        _setup: Vec<(u64, GameRequest)>,
    ) {
    }
    /// Asks the running session (→ [`GameEvent::Reply`] with `id`).
    fn game_request(&self, _id: u64, _request: GameRequest) {}
    /// The profiles kept in the workspace, the newest first (S057).
    fn profiles(&self, _root: &Path) -> Vec<SavedProfile> {
        Vec::new()
    }
    /// Reads a speedscope file as a profile (wowlua's, or another tool's
    /// as the tree of its samples; → [`Event::Profile`]).
    fn profile_read(&self, _path: &Path) {}
    /// Keeps a recorded profile (a session's report's `speedscope`) in the
    /// workspace, named `name` and the time: its file and the profile.
    fn profile_keep(
        &self,
        _root: &Path,
        _name: &str,
        _speedscope: &serde_json::Value,
    ) -> Result<(PathBuf, Profile), String> {
        Err("profiles cannot be kept here".into())
    }
    /// What a code means and how to fix it (wowlua's explanation, in
    /// Markdown): a profiler hint's `P7301`.
    fn explain(&self, _code: &str) -> Option<String> {
        None
    }
    /// Names the functions defined in these sources (from the package's
    /// root) by their outlines: `ThreatModule:Update` (→
    /// [`Event::FunctionNames`]).
    fn function_names(&self, _root: &Path, _files: Vec<String>) {}
    /// Reads the recordings exported in the game for the workspace's
    /// addon, in the installation's SavedVariables (→
    /// [`Event::GameRecordings`]).
    fn game_recordings(&self, _root: &Path) {}
    /// Ends the running session.
    fn game_stop(&self) {}
    // The world profiles' (M8).
    /// The package's world profiles, by name.
    fn world_profiles(&self, _root: &Path) -> Vec<WorldProfile> {
        Vec::new()
    }
    /// A world profile's problems (E4400, E4401), from its text as edited.
    fn check_world_profile(&self, _text: &str) -> Vec<ProfileProblem> {
        Vec::new()
    }
    /// Where the package names a world profile: test files'
    /// `config({ profile })`, the manifest's `test.profile`, debug
    /// configurations.
    fn profile_uses(&self, _root: &Path, _name: &str) -> Vec<String> {
        Vec::new()
    }
    /// The snapshots of the package's addon in the installation
    /// (→ [`Event::WorldSnapshots`]).
    fn world_snapshots(&self, _root: &Path) {}
    /// Game art of the flavor's client (→ [`Event::GameArt`]).
    fn game_art(&self, _flavor: &str, _art: Art) {}
    /// The flavor's client's spells by name or ID (→ [`Event::Spells`]).
    fn search_spells(&self, _flavor: &str, _query: &str) {}
    /// The flavor's client's spells or items by name or ID, for an ID
    /// picker (→ [`Event::Found`]; an item's text: its quality and level).
    fn find(&self, _what: Find, _flavor: &str, _query: &str) {}
    /// A class's spells of the flavor's client (→ [`Event::ClassSpells`]).
    fn class_spells(&self, _flavor: &str, _class: &str) {}
    /// A SavedVariables file's variables as JSON, read without running it.
    fn read_saved_variables(
        &self,
        _path: &Path,
    ) -> Result<Vec<(String, serde_json::Value)>, String> {
        Err("no SavedVariables here".into())
    }
    /// The running session's screen, drawn on the UI's GPU device.
    fn game_screen(&self) -> Option<GameScreen> {
        None
    }
    /// What the ship page shows of the package (→ [`ShipEvent::Read`]).
    fn ship_read(&self, _root: &Path) {}
    /// The build profiles of the package in `dir` (→ [`Event::Profiles`]).
    fn profiles_read(&self, _dir: &Path) {}
    /// The flavor's binding actions with their category (`MOVEFORWARD`,
    /// `movement`), from Blizzard's UI code wowlua keeps; none while it
    /// isn't fetched.
    fn binding_actions(&self, _flavor: &str) -> Vec<(String, String)> {
        Vec::new()
    }
    /// A question for the Git repository `root` is in (M11, → [`Event::Git`]).
    fn git(&self, _root: &Path, _request: GitRequest) {}
    /// A question for the GitHub repository `root`'s remote names (M12, →
    /// [`Event::GithubApi`]).
    fn github(&self, _root: &Path, _request: GithubRequest) {}
    /// A question about the signed-in user's repositories (M12, →
    /// [`Event::Account`]).
    fn github_account(&self, _request: AccountRequest) {}
    /// Clones `owner/name` at `branch` into Studio's data (M12, →
    /// [`Event::Cloning`], [`Event::Cloned`]).
    fn clone_repo(&self, _repo: &str, _branch: &str) {}
    /// The folder Studio's clones are in: a workspace under it is a
    /// repository opened from GitHub, deleted when it closes.
    fn clones_dir(&self) -> Option<PathBuf> {
        None
    }
    /// Marks a clone as open in this window, so no other window takes it
    /// for one left behind.
    fn hold_clone(&self, _root: &Path) {}
    /// What deleting the clone would lose.
    fn clone_losses(&self, _root: &Path) -> Result<Losses, String> {
        Ok(Losses::default())
    }
    /// Deletes a clone (only under [`System::clones_dir`]).
    fn delete_clone(&self, _root: &Path) -> Result<(), String> {
        Ok(())
    }
    /// The clones no window has open: those with nothing to lose are
    /// deleted, the others listed.
    fn left_clones(&self) -> Vec<LeftClone> {
        Vec::new()
    }
    /// The CVars the flavor's client has, with their defaults (wowlua's
    /// recorded ones); none where wowlua has none.
    fn cvars(&self, _flavor: &str) -> Vec<CVarInfo> {
        Vec::new()
    }
    /// The targets' tokens' states (→ [`ShipEvent::Tokens`]): the OS
    /// credential store may ask the user first.
    fn ship_tokens(&self, _targets: Vec<ShipTarget>) {}
    /// Saves a token, or removes it (→ [`ShipEvent::TokenSaved`]).
    fn ship_token(&self, _kind: &str, _token: Option<String>) {}
    /// Builds a profile, and installs it (→ [`ShipEvent::Build`]).
    fn ship_build(&self, _root: &Path, _job: u64, _build: ShipBuild) {}
    /// Starts the dev loop, or stops it (→ [`ShipEvent::Dev`]).
    fn ship_dev(&self, _root: &Path, _dev: Option<ShipInstall>) {}
    /// Publishes, or a dry run (→ [`ShipEvent::Publish`]).
    fn ship_publish(&self, _root: &Path, _job: u64, _publish: ShipPublish) {}
    /// Tags the version and pushes it (→ [`ShipEvent::Tagged`]).
    fn ship_tag(&self, _root: &Path, _tag: TagPush) {}
    /// Shows a folder in the system's file manager.
    fn reveal(&self, _path: &Path) {}
    /// The settings page's facts of this computer (→ [`Event::Settings`]).
    fn settings_info(&self) {}
    /// Studio's log file.
    fn log_file(&self) -> Option<PathBuf> {
        None
    }
    /// Clears the language server's check cache on disk.
    fn clear_check_cache(&self) {}
    /// Clears the installation's art kept on disk.
    fn clear_art_cache(&self) {}
    /// Looks for a newer Studio (→ [`Event::Updates`]).
    fn check_updates(&self) {}
    /// Fetches Blizzard's UI source of a flavor again.
    fn refresh_ui(&self, _flavor: &str) {}
    /// Signs in to GitHub with the device flow (→ [`Event::Github`]), or
    /// out (`false`).
    fn github_sign_in(&self, _on: bool) {}
    /// Saves the Claude API key, or removes it.
    fn claude_key(&self, _key: Option<String>) {}
    /// The package's dependencies (→ [`DepsEvent::Read`]).
    fn deps_read(&self, _dir: &Path) {}
    /// Newer versions of them (→ [`DepsEvent::Outdated`]): the network.
    fn deps_outdated(&self, _dir: &Path) {}
    /// Searches the registry (→ [`DepsEvent::Searched`]).
    fn deps_search(&self, _dir: &Path, _query: &str) {}
    /// A registry package's versions (→ [`DepsEvent::Versions`]).
    fn deps_versions(&self, _dir: &Path, _name: &str) {}
    /// Adds, removes or updates (→ [`DepsEvent::Ran`]).
    fn deps_run(&self, _dir: &Path, _job: u64, _command: DepsCommand) {}
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
    fn folding(&self, _: &str) -> Vec<(usize, usize)> {
        Vec::new()
    }
    fn schema(&self, _: bool) -> Option<String> {
        None
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
    fn search(&self, _: &Path, _: Search) {}
    fn terminal_open(&self, _: &Path, _: u16, _: u16) -> Result<u64, String> {
        Err("this system has no terminal".into())
    }
    fn terminal_input(&self, _: u64, _: &[u8]) {}
    fn terminal_resize(&self, _: u64, _: u16, _: u16) {}
    fn terminal_scroll(&self, _: u64, _: i32) {}
    fn terminal_screen(&self, _: u64) -> Option<TermScreen> {
        None
    }
    fn terminal_close(&self, _: u64) {}
    fn poll(&self) -> Vec<Event> {
        Vec::new()
    }
}
