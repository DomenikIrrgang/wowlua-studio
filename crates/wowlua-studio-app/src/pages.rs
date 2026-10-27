//! The rail's pages (the design's rail and overview boards), in its order.

use serde::{Deserialize, Serialize};
use wowlua_studio_ui::Icon;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Page {
    Files,
    Search,
    Git,
    Prs,
    Debug,
    Tests,
    Game,
    Perf,
    World,
    Ship,
    Claude,
    Settings,
}

impl Page {
    /// The pages above the rail's gap; settings sits at its bottom.
    pub const RAIL: [Page; 11] = [
        Page::Files,
        Page::Search,
        Page::Git,
        Page::Prs,
        Page::Debug,
        Page::Tests,
        Page::Game,
        Page::Perf,
        Page::World,
        Page::Ship,
        Page::Claude,
    ];

    pub const ALL: [Page; 12] = [
        Page::Files,
        Page::Search,
        Page::Git,
        Page::Prs,
        Page::Debug,
        Page::Tests,
        Page::Game,
        Page::Perf,
        Page::World,
        Page::Ship,
        Page::Claude,
        Page::Settings,
    ];

    /// The rail's word for the page.
    pub fn label(self) -> &'static str {
        match self {
            Page::Files => "files",
            Page::Search => "search",
            Page::Git => "git",
            Page::Prs => "prs",
            Page::Debug => "debug",
            Page::Tests => "tests",
            Page::Game => "game",
            Page::Perf => "perf",
            Page::World => "world",
            Page::Ship => "ship",
            Page::Claude => "claude",
            Page::Settings => "settings",
        }
    }

    /// What the page is, for the command palette.
    pub fn title(self) -> &'static str {
        match self {
            Page::Files => "editor",
            Page::Search => "search",
            Page::Git => "git",
            Page::Prs => "pull requests",
            Page::Debug => "debugger",
            Page::Tests => "tests and coverage",
            Page::Game => "game",
            Page::Perf => "profiler",
            Page::World => "world profiles",
            Page::Ship => "build and ship",
            Page::Claude => "claude",
            Page::Settings => "settings",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Page::Files => Icon::Files,
            Page::Search => Icon::Search,
            Page::Git => Icon::Git,
            Page::Prs => Icon::PullRequest,
            Page::Debug => Icon::Debug,
            Page::Tests => Icon::Test,
            Page::Game => Icon::Ui,
            Page::Perf => Icon::Perf,
            Page::World => Icon::World,
            Page::Ship => Icon::Publish,
            Page::Claude => Icon::Agent,
            Page::Settings => Icon::Config,
        }
    }

    /// Pages the design marks as planned: they need new work in wowlua or
    /// come last (the roadmap's M14).
    pub fn planned(self) -> bool {
        self == Page::Claude
    }
}
