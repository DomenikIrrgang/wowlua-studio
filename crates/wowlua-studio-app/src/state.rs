//! What Studio keeps between runs (decision S012): its settings, and a
//! layout per page per workspace, in the system's folder for application
//! data. The host reads and writes the files ([`crate::Store`]).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wowlua_studio_ui::Theme;

use crate::{dock::Layout, pages::Page};

/// Studio's own settings, on this computer (the settings page, S062).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Settings {
    /// `dark`, `light` or `system` (the OS's).
    pub theme: String,
    /// One scale for Studio's whole interface, in percent.
    pub interface_size: u32,
    /// `compact` or `comfortable`: the rows of trees, tables and lists.
    pub density: String,
    /// No blinking caret, no animation.
    pub reduce_motion: bool,
    pub font_size: f32,
    pub format_on_save: bool,
    pub inlay_hints: bool,
    pub code_lenses: bool,
    pub coverage_gutter: bool,
    /// `off`, `line width` (the package's fmt line width) or `window`.
    pub word_wrap: String,
    pub suggest_while_typing: bool,
    /// `tab`, `enter` or `tab and enter`.
    pub accept_with: String,
    pub add_imports: bool,
    pub completion_docs: bool,
    pub signature_help: bool,
    /// `while typing`, `when you pause` or `on save`.
    pub check_when: String,
    /// The language server's threads for per-file work; 0: every core.
    pub check_threads: u32,
    /// The build profile the language server's diagnostics follow.
    pub check_profile: String,
    /// `manual` or `realtime`, for new launch configurations.
    pub default_clock: String,
    /// The debugger stops on: `uncaught`, `error`, `taint`, `blocked`.
    pub stop_on: Vec<String>,
    pub step_into_blizzard: bool,
    pub show_dispatch: bool,
    pub inline_values: bool,
    pub reveal_secrets: bool,
    /// A session's screen: `1920 × 1080`, `2560 × 1440` or `3840 × 2160`.
    pub screen_size: String,
    pub installation_art: bool,
    pub installation_data: bool,
    /// `opened files first` or `everything at once` (M12 reads it).
    pub github_load: String,
    pub github_watch: bool,
    pub github_notify: bool,
    /// What Claude may do without asking (M14 reads it).
    pub claude_approvals: String,
    pub claude_tools: bool,
    pub claude_shell: bool,
    /// Opening a page switches to its layout (the panels board's rules).
    #[serde(alias = "follow_presets")]
    pub follow_presets: bool,
    /// Each workspace keeps its own panels; off: one layout per page.
    pub layouts_per_workspace: bool,
    /// A panel dragged out becomes a window of the OS.
    pub float_windows: bool,
    /// The status bar shows the redraws a second and the last frame's
    /// time (S041).
    #[serde(alias = "frame_stats")]
    pub frame_stats: bool,
    /// Keys recorded for commands, by the command's name (S062): they
    /// replace the command's own.
    pub keys: BTreeMap<String, Vec<String>>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: Theme::Dark.name().to_owned(),
            interface_size: 100,
            density: "compact".into(),
            reduce_motion: false,
            font_size: 13.0,
            format_on_save: true,
            inlay_hints: true,
            code_lenses: true,
            coverage_gutter: true,
            word_wrap: "off".into(),
            suggest_while_typing: true,
            accept_with: "tab and enter".into(),
            add_imports: true,
            completion_docs: true,
            signature_help: true,
            check_when: "when you pause".into(),
            check_threads: 0,
            check_profile: "dev".into(),
            default_clock: "manual".into(),
            stop_on: vec!["uncaught".into(), "blocked".into()],
            step_into_blizzard: false,
            show_dispatch: true,
            inline_values: true,
            reveal_secrets: true,
            screen_size: "1920 × 1080".into(),
            installation_art: true,
            installation_data: false,
            github_load: "opened files first".into(),
            github_watch: true,
            github_notify: true,
            claude_approvals: "ask every time".into(),
            claude_tools: true,
            claude_shell: false,
            follow_presets: true,
            layouts_per_workspace: true,
            float_windows: true,
            frame_stats: false,
            keys: BTreeMap::new(),
        }
    }
}

impl Settings {
    /// The theme to draw now: the OS's for `system`.
    pub fn theme(&self) -> Theme {
        Theme::from_name(&self.theme).unwrap_or_default()
    }

    /// Follows the OS's dark or light.
    pub fn follows_system(&self) -> bool {
        self.theme == "system"
    }

    /// The interface's scale, 0.5 to 2.
    pub fn zoom(&self) -> f32 {
        (self.interface_size as f32 / 100.0).clamp(0.5, 2.0)
    }

    pub fn comfortable(&self) -> bool {
        self.density == "comfortable"
    }

    pub fn stops_on(&self, kind: &str) -> bool {
        self.stop_on.iter().any(|s| s == kind)
    }
}

/// The layouts of every workspace, keyed by the workspace's path (`""`
/// while none is open).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layouts {
    pub workspaces: BTreeMap<String, BTreeMap<Page, Layout>>,
}

pub const SETTINGS_FILE: &str = "settings.json";
pub const LAYOUTS_FILE: &str = "layouts.json";

/// Parses a saved file; a broken or missing file is the default.
pub fn parse<T: for<'de> Deserialize<'de> + Default>(text: Option<String>) -> T {
    text.and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn to_text<T: Serialize>(value: &T) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_and_broken_files_give_the_defaults() {
        assert_eq!(parse::<Settings>(None), Settings::default());
        assert_eq!(parse::<Settings>(Some("{".into())), Settings::default());
        let s: Settings = parse(Some(r#"{"theme":"light"}"#.into()));
        assert_eq!(s.theme(), Theme::Light);
        assert!(s.follow_presets);
        // The settings before M10 (snake case) are read too.
        let s: Settings = parse(Some(
            r#"{"theme":"dark","follow_presets":false,"frame_stats":true}"#.into(),
        ));
        assert!(!s.follow_presets && s.frame_stats);
        let s: Settings = parse(Some(r#"{"interface-size":125,"stop-on":["taint"]}"#.into()));
        assert_eq!(s.zoom(), 1.25);
        assert!(s.stops_on("taint") && !s.stops_on("uncaught"));
    }
}
