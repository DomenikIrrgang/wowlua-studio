//! Every command, with its menu and its keys: the title bar's menus (the
//! system menu bar on macOS), the command palette and the keyboard all
//! come from this one list.

use egui::{Key, KeyboardShortcut, Modifiers};
use wowlua_studio_ui::Theme;

use crate::{Platform, dock::Dock, pages::Page, panels::Panel};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    ShowPage(Page),
    SetTheme(Theme),
    OpenPalette,
    ResetLayout,
    /// Opens the focused panel's menu to move it with the keyboard.
    MovePanel,
    ShowPanel(Panel, Dock),
    /// Whether opening a page switches to its layout.
    FollowPresets(bool),
    /// Whether the status bar shows the frame stats (S041).
    FrameStats(bool),
    /// Opens a folder from the system's picker (S025: in this window or a
    /// new one, when this one has a workspace).
    OpenFolder,
    /// An empty window, a process of its own (S031).
    NewWindow,
    /// Closes the workspace: the window shows the start page.
    CloseFolder,
    /// Writes the active file.
    Save,
    /// Closes the active file's tab.
    CloseEditor,
    /// Opens the find bar (S051); with its replace row.
    Find,
    Replace,
    /// The find bar's next and previous match.
    FindNext,
    FindPrevious,
    /// Runs every test, or those that failed last (S052).
    RunAllTests,
    RunFailedTests,
    /// Runs the workspace's addon on the game page, or stops it (S055).
    RunAddon,
    StopSession,
    /// The start page's new-addon form.
    NewAddon,
    /// The start page's adopt form.
    AdoptAddon,
    OpenDocs,
    About,
    Quit,
}

/// The menus, in the title bar's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Menu {
    File,
    Edit,
    View,
    Go,
    Run,
    Test,
    Build,
    Help,
}

impl Menu {
    pub const ALL: [Menu; 8] = [
        Menu::File,
        Menu::Edit,
        Menu::View,
        Menu::Go,
        Menu::Run,
        Menu::Test,
        Menu::Build,
        Menu::Help,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Menu::File => "file",
            Menu::Edit => "edit",
            Menu::View => "view",
            Menu::Go => "go",
            Menu::Run => "run",
            Menu::Test => "test",
            Menu::Build => "build",
            Menu::Help => "help",
        }
    }
}

/// A command as the palette and the menus list it.
#[derive(Clone, Debug)]
pub struct Entry {
    pub command: Command,
    /// The palette's kind column: `go`, `view`, `layout`, `panel`, …
    pub kind: &'static str,
    pub title: String,
    pub detail: String,
    pub menu: Option<Menu>,
    pub key: Option<KeyboardShortcut>,
}

/// Every command Studio has now; later milestones add theirs. Without a
/// workspace the window is the start page: files, the view and help.
pub fn entries(
    platform: Platform,
    follow_presets: bool,
    frame_stats: bool,
    workspace: bool,
) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut add = |command, kind, title: &str, detail: &str, menu, key| {
        out.push(Entry {
            command,
            kind,
            title: title.to_owned(),
            detail: detail.to_owned(),
            menu,
            key,
        });
    };
    add(
        Command::OpenFolder,
        "file",
        "open folder…",
        "a package or workspace",
        Some(Menu::File),
        Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::O)),
    );
    if !workspace {
        add(
            Command::NewAddon,
            "file",
            "new addon…",
            "wowlua new, from a template",
            Some(Menu::File),
            None,
        );
        add(
            Command::AdoptAddon,
            "file",
            "adopt addon…",
            "TOC and .pkgmeta to wowlua.json",
            Some(Menu::File),
            None,
        );
    }
    add(
        Command::NewWindow,
        "file",
        "new window",
        "",
        Some(Menu::File),
        Some(KeyboardShortcut::new(
            Modifiers::COMMAND | Modifiers::SHIFT,
            Key::N,
        )),
    );
    if workspace {
        add(
            Command::Save,
            "file",
            "save",
            "the active file",
            Some(Menu::File),
            Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::S)),
        );
        add(
            Command::CloseEditor,
            "file",
            "close editor",
            "the active file's tab",
            Some(Menu::File),
            Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::W)),
        );
        add(
            Command::CloseFolder,
            "file",
            "close folder",
            "back to the start page",
            Some(Menu::File),
            None,
        );
    }
    if workspace {
        add(
            Command::RunAllTests,
            "test",
            "run all tests",
            "every test file, with coverage when it is on",
            Some(Menu::Test),
            None,
        );
        add(
            Command::RunFailedTests,
            "test",
            "run the failed tests",
            "the tests that failed last",
            Some(Menu::Test),
            None,
        );
        add(
            Command::RunAddon,
            "run",
            "run the addon",
            "the addon logged in, on the game page",
            Some(Menu::Run),
            None,
        );
        add(
            Command::StopSession,
            "run",
            "stop the session",
            "the game page's running addon",
            Some(Menu::Run),
            None,
        );
    }
    if workspace {
        // The find bar (S051): cmd f, cmd alt f (ctrl h), cmd g (F3).
        let mac = platform == Platform::MacOs;
        add(
            Command::Find,
            "edit",
            "find",
            "in the active file",
            Some(Menu::Edit),
            Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::F)),
        );
        add(
            Command::Replace,
            "edit",
            "replace",
            "in the active file",
            Some(Menu::Edit),
            Some(if mac {
                KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::ALT, Key::F)
            } else {
                KeyboardShortcut::new(Modifiers::COMMAND, Key::H)
            }),
        );
        add(
            Command::FindNext,
            "edit",
            "find next",
            "the next match",
            Some(Menu::Edit),
            Some(if mac {
                KeyboardShortcut::new(Modifiers::COMMAND, Key::G)
            } else {
                KeyboardShortcut::new(Modifiers::NONE, Key::F3)
            }),
        );
        add(
            Command::FindPrevious,
            "edit",
            "find previous",
            "the previous match",
            Some(Menu::Edit),
            Some(if mac {
                KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::G)
            } else {
                KeyboardShortcut::new(Modifiers::SHIFT, Key::F3)
            }),
        );
    }
    add(
        Command::OpenPalette,
        "view",
        "command palette",
        "files, symbols, commands",
        Some(Menu::View),
        Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::K)),
    );
    for page in Page::ALL.into_iter().filter(|_| workspace) {
        add(
            Command::ShowPage(page),
            "go",
            &format!("go: {}", page.title()),
            page.label(),
            Some(Menu::Go),
            None,
        );
    }
    add(
        Command::SetTheme(Theme::Dark),
        "view",
        "theme: dark",
        "",
        Some(Menu::View),
        None,
    );
    add(
        Command::SetTheme(Theme::Light),
        "view",
        "theme: light",
        "",
        Some(Menu::View),
        None,
    );
    add(
        Command::FrameStats(!frame_stats),
        "view",
        if frame_stats {
            "frame stats: hide"
        } else {
            "frame stats: show"
        },
        "redraws a second and the frame's time, in the status bar",
        Some(Menu::View),
        None,
    );
    if workspace {
        layout_entries(&mut add, follow_presets);
    }
    add(
        Command::OpenDocs,
        "help",
        "wowlua documentation",
        "wowlua.dev",
        Some(Menu::Help),
        None,
    );
    add(
        Command::About,
        "help",
        "about wowlua studio",
        "",
        Some(Menu::Help),
        None,
    );
    if platform != Platform::MacOs {
        // macOS has these in the application menu.
        if workspace {
            add(
                Command::ShowPage(Page::Settings),
                "go",
                "settings",
                "",
                Some(Menu::File),
                None,
            );
        }
        add(Command::Quit, "file", "quit", "", Some(Menu::File), None);
    }
    out
}

/// The layout's and the panels' commands: a window with a workspace.
fn layout_entries(
    add: &mut impl FnMut(Command, &'static str, &str, &str, Option<Menu>, Option<KeyboardShortcut>),
    follow_presets: bool,
) {
    add(
        Command::ResetLayout,
        "layout",
        "layout: reset this page's",
        "back to the page's preset",
        Some(Menu::View),
        None,
    );
    add(
        Command::FollowPresets(!follow_presets),
        "layout",
        if follow_presets {
            "layout: keep it when changing pages"
        } else {
            "layout: switch with the page"
        },
        "each page has a layout preset",
        Some(Menu::View),
        None,
    );
    add(
        Command::MovePanel,
        "layout",
        "panel: move the focused panel",
        "to a dock, a split or a window",
        Some(Menu::View),
        Some(KeyboardShortcut::new(
            Modifiers::COMMAND | Modifiers::SHIFT,
            Key::M,
        )),
    );
    for &(panel, dock) in SHOWABLE {
        add(
            Command::ShowPanel(panel, dock),
            "panel",
            &format!("panel: show {}", panel.title()),
            "",
            None,
            None,
        );
    }
}

/// Panels the palette can bring back, with the dock they go to.
const SHOWABLE: &[(Panel, Dock)] = &[
    (Panel::Explorer, Dock::Left),
    (Panel::Outline, Dock::Left),
    (Panel::Search, Dock::Left),
    (Panel::TestExplorer, Dock::Left),
    (Panel::CallStack, Dock::Left),
    (Panel::Breakpoints, Dock::Left),
    (Panel::Frames, Dock::Left),
    (Panel::Editor, Dock::Main),
    (Panel::Game, Dock::Main),
    (Panel::Profiler, Dock::Main),
    (Panel::Coverage, Dock::Main),
    (Panel::Problems, Dock::Bottom),
    (Panel::Output, Dock::Bottom),
    (Panel::Terminal, Dock::Bottom),
    (Panel::Console, Dock::Bottom),
    (Panel::Events, Dock::Bottom),
    (Panel::Variables, Dock::Right),
    (Panel::Watch, Dock::Right),
    (Panel::Inspector, Dock::Right),
    (Panel::LiveWorld, Dock::Right),
];

/// A shortcut as the design writes it: words in kbd keys, `ctrl` on
/// Windows and Linux, `cmd` on macOS.
pub fn key_words(key: &KeyboardShortcut, platform: Platform) -> Vec<String> {
    let m = key.modifiers;
    let mut words = Vec::new();
    let mac = platform == Platform::MacOs;
    if m.command || m.mac_cmd {
        words.push(if mac { "cmd" } else { "ctrl" }.to_owned());
    }
    if m.ctrl && !m.command {
        words.push("ctrl".to_owned());
    }
    if m.alt {
        words.push(if mac { "option" } else { "alt" }.to_owned());
    }
    if m.shift {
        words.push("shift".to_owned());
    }
    words.push(key.logical_key.name().to_lowercase());
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_words_per_platform() {
        let k = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::M);
        assert_eq!(key_words(&k, Platform::Windows), ["ctrl", "shift", "m"]);
        assert_eq!(key_words(&k, Platform::MacOs), ["cmd", "shift", "m"]);
    }

    #[test]
    fn no_two_commands_share_keys() {
        for workspace in [false, true] {
            let all = entries(Platform::Linux, true, false, workspace);
            let keys: Vec<_> = all.iter().filter_map(|e| e.key).collect();
            for (i, a) in keys.iter().enumerate() {
                assert!(!keys[i + 1..].contains(a), "{a:?} is bound twice");
            }
        }
    }

    #[test]
    fn the_start_page_has_files_the_view_and_help() {
        let menus: Vec<_> = entries(Platform::Linux, true, false, false)
            .iter()
            .filter_map(|e| e.menu)
            .collect();
        for menu in Menu::ALL {
            let shown = menus.contains(&menu);
            let expected = matches!(menu, Menu::File | Menu::View | Menu::Help);
            assert_eq!(shown, expected, "{menu:?}");
        }
    }
}
