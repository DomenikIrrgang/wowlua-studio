//! Recently opened workspaces (the start page's recent list), kept in
//! Studio's folder for application data (S012) and shared by its windows
//! (S031).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::system::Workspace;

pub const RECENTS_FILE: &str = "recents.json";
/// How many the list keeps.
const KEEP: usize = 12;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Recent {
    pub path: PathBuf,
    pub name: String,
    /// The packages, the root first.
    pub members: Vec<String>,
    pub flavors: Vec<String>,
    /// Why it can't be checked (E4300), when it couldn't the last time.
    #[serde(default)]
    pub cannot_check: Option<String>,
    /// When it was last opened, in seconds since the Unix epoch.
    pub opened: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Recents {
    pub list: Vec<Recent>,
}

impl Recents {
    /// Puts the workspace at the top.
    pub fn opened(&mut self, workspace: &Workspace, now: u64) {
        self.forget(&workspace.root);
        self.list.insert(
            0,
            Recent {
                path: workspace.root.clone(),
                name: workspace.name.clone(),
                members: workspace.members.iter().map(|m| m.name.clone()).collect(),
                flavors: workspace.flavors.clone(),
                cannot_check: workspace.cannot_check.clone(),
                opened: now,
            },
        );
        self.list.truncate(KEEP);
    }

    pub fn forget(&mut self, path: &Path) {
        self.list.retain(|r| r.path != path);
    }

    /// Moves an entry whose folder moved.
    pub fn relocate(&mut self, from: &Path, to: PathBuf) {
        if let Some(r) = self.list.iter_mut().find(|r| r.path == from) {
            r.path = to;
        }
    }
}

/// When it was opened, as the start page says it.
pub fn when(now: u64, opened: u64) -> String {
    let days = now.saturating_sub(opened) / 86_400;
    match days {
        0 => "today".into(),
        1 => "yesterday".into(),
        2..=13 => format!("{days} days ago"),
        14..=59 => format!("{} weeks ago", days / 7),
        _ => format!("{} months ago", days / 30),
    }
}

/// `path` with the home folder written `~`.
pub fn tilde(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.to_string_lossy().replace('\\', "/")),
        None => path.to_string_lossy().into_owned(),
    }
}

/// The line under a recent's name: where, what, for which flavors.
pub fn detail(recent: &Recent, home: Option<&Path>) -> String {
    let mut parts = vec![tilde(&recent.path, home)];
    match recent.members.len() {
        0 | 1 => {}
        n if n <= 3 => parts.push(recent.members.join(", ")),
        n => parts.push(format!(
            "{}, {} and {} more",
            recent.members[0],
            recent.members[1],
            n - 2
        )),
    }
    if !recent.flavors.is_empty() {
        parts.push(recent.flavors.join(", "));
    }
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::Member;

    fn workspace(root: &str, members: &[&str]) -> Workspace {
        Workspace {
            root: PathBuf::from(root),
            name: members[0].into(),
            members: members
                .iter()
                .map(|m| Member {
                    name: (*m).into(),
                    kind: "addon".into(),
                    version: "0.1.0".into(),
                    dir: PathBuf::from(root),
                    rel: String::new(),
                })
                .collect(),
            flavors: vec!["camelot".into()],
            cannot_check: None,
        }
    }

    #[test]
    fn opening_moves_to_the_top_and_keeps_twelve() {
        let mut r = Recents::default();
        for i in 0..15 {
            r.opened(&workspace(&format!("/p/{i}"), &["a"]), i);
        }
        assert_eq!(r.list.len(), 12);
        assert_eq!(r.list[0].path, PathBuf::from("/p/14"));
        r.opened(&workspace("/p/10", &["a"]), 20);
        assert_eq!(r.list[0].path, PathBuf::from("/p/10"));
        assert_eq!(r.list.len(), 12, "no duplicate");
    }

    #[test]
    fn the_detail_line() {
        let mut r = Recents::default();
        r.opened(
            &workspace(
                "/home/me/Projects/suuui",
                &["suuui", "foreverlfg", "hello", "badge"],
            ),
            0,
        );
        assert_eq!(
            detail(&r.list[0], Some(Path::new("/home/me"))),
            "~/Projects/suuui · suuui, foreverlfg and 2 more · camelot"
        );
        assert_eq!(when(10 * 86_400, 0), "10 days ago");
        assert_eq!(when(86_400 + 5, 0), "yesterday");
    }
}
