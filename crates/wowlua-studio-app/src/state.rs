//! What Studio keeps between runs (decision S012): its settings, and a
//! layout per page per workspace, in the system's folder for application
//! data. The host reads and writes the files ([`crate::Store`]).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wowlua_studio_ui::Theme;

use crate::{dock::Layout, pages::Page};

/// Studio's own settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: String,
    /// Opening a page switches to its layout (the panels board's rules).
    pub follow_presets: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: Theme::Dark.name().to_owned(),
            follow_presets: true,
        }
    }
}

impl Settings {
    pub fn theme(&self) -> Theme {
        Theme::from_name(&self.theme).unwrap_or_default()
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
    }
}
