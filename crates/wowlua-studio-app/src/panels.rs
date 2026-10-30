//! Every dock panel (the design's panels board and page artboards), with
//! the milestone that fills it.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Panel {
    // Left by default
    Explorer,
    Outline,
    Search,
    Changes,
    History,
    PullRequests,
    TestExplorer,
    CallStack,
    Breakpoints,
    Frames,
    Units,
    Profiles,
    Parts,
    BuildProfiles,
    SettingsSections,
    Conversations,
    // Main
    Editor,
    Diff,
    PullRequest,
    Game,
    /// A tab of layouts saved before the 3D world became the game
    /// screen's toggle (M13): read, then taken out of them.
    World3d,
    Profiler,
    TestResults,
    Coverage,
    ProfileEditor,
    Ship,
    Settings,
    Chat,
    // Bottom
    Problems,
    Output,
    Terminal,
    Console,
    Events,
    Input,
    Screenshots,
    // Right
    Variables,
    Watch,
    Inspector,
    LiveWorld,
    Function,
    Tools,
}

impl Panel {
    /// The panel's name in its header or tab.
    pub fn title(self) -> &'static str {
        match self {
            Panel::Explorer => "explorer",
            Panel::Outline => "outline",
            Panel::Search => "search",
            Panel::Changes => "changes",
            Panel::History => "history",
            Panel::PullRequests => "pull requests",
            Panel::TestExplorer => "tests",
            Panel::CallStack => "call stack",
            Panel::Breakpoints => "breakpoints",
            Panel::Frames => "frames",
            Panel::Units => "units",
            Panel::Profiles => "profiles",
            Panel::Parts => "parts",
            Panel::BuildProfiles => "build profiles",
            Panel::SettingsSections => "settings",
            Panel::Conversations => "claude",
            Panel::Editor => "editor",
            Panel::Diff => "diff",
            Panel::PullRequest => "pull request",
            Panel::Game => "game screen",
            Panel::World3d => "3d world",
            Panel::Profiler => "profiler",
            Panel::TestResults => "results",
            Panel::Coverage => "coverage",
            Panel::ProfileEditor => "profile",
            Panel::Ship => "ship",
            Panel::Settings => "settings",
            Panel::Chat => "conversation",
            Panel::Problems => "problems",
            Panel::Output => "output",
            Panel::Terminal => "terminal",
            Panel::Console => "console",
            Panel::Events => "events",
            Panel::Input => "input",
            Panel::Screenshots => "screenshots",
            Panel::Variables => "variables",
            Panel::Watch => "watch",
            Panel::Inspector => "inspector",
            Panel::LiveWorld => "live world",
            Panel::Function => "function",
            Panel::Tools => "tools and permissions",
        }
    }

    /// The roadmap milestone that gives the panel its content.
    pub fn milestone(self) -> &'static str {
        match self {
            Panel::Explorer => "M2",
            Panel::Outline
            | Panel::Search
            | Panel::Editor
            | Panel::Problems
            | Panel::Output
            | Panel::Terminal => "M3",
            Panel::TestExplorer | Panel::TestResults | Panel::Coverage => "M4",
            Panel::Frames
            | Panel::Units
            | Panel::Game
            | Panel::Events
            | Panel::Input
            | Panel::Screenshots
            | Panel::Inspector => "M5",
            Panel::CallStack
            | Panel::Breakpoints
            | Panel::Console
            | Panel::Variables
            | Panel::Watch => "M6",
            Panel::Profiler | Panel::Function => "M7",
            Panel::Profiles | Panel::Parts | Panel::ProfileEditor | Panel::LiveWorld => "M8",
            Panel::BuildProfiles | Panel::Ship => "M9",
            Panel::SettingsSections | Panel::Settings => "M10",
            Panel::Changes | Panel::History | Panel::Diff => "M11",
            Panel::PullRequests | Panel::PullRequest => "M12",
            Panel::World3d => "M13",
            Panel::Conversations | Panel::Chat | Panel::Tools => "M14",
        }
    }
}
