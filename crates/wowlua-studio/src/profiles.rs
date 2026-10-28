//! The profiler's files (S057): recorded profiles kept in the workspace's
//! `.wowlua/profiles` as the speedscope files wowlua writes, read back with
//! `wowlua_debug::profile::Saved` (wowlua decision T495); and the
//! recordings the dev window exported in the game, read from the
//! installation's SavedVariables (`wowlua_driver::recordings`).

use std::path::{Path, PathBuf};

use wowlua_debug::profile::Saved;
use wowlua_driver::Session;
use wowlua_driver::project::{LoadOptions, load_projects};
use wowlua_driver::recordings;
use wowlua_manifest::model::PackageKind;
use wowlua_studio_app::system::{
    GameRecordings, Profile, ProfileFrame, ProfileKey, ProfileNode, SavedProfile,
};

/// Where a workspace keeps its profiles.
pub fn dir(root: &Path) -> PathBuf {
    root.join(".wowlua").join("profiles")
}

/// The ending of a profile's file.
const ENDING: &str = ".speedscope.json";

/// The profiles kept in the workspace, the newest first.
pub fn list(root: &Path) -> Vec<SavedProfile> {
    let mut out: Vec<SavedProfile> = std::fs::read_dir(dir(root))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let file = path.file_name()?.to_str()?;
            let name = file.strip_suffix(ENDING)?.to_string();
            let modified = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            Some(SavedProfile {
                path,
                name,
                modified,
            })
        })
        .collect();
    out.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.name.cmp(&b.name)));
    out
}

/// A speedscope file as a profile.
pub fn read(path: &Path) -> Result<Profile, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let json: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Saved::from_speedscope(&json)
        .map(|s| of(&s))
        .ok_or_else(|| format!("{} is not a speedscope file", path.display()))
}

/// Keeps a recorded profile in the workspace: `<name> <date> <time>`.
pub fn keep(
    root: &Path,
    name: &str,
    speedscope: &serde_json::Value,
) -> Result<(PathBuf, Profile), String> {
    let saved = Saved::from_speedscope(speedscope).ok_or("the session's profile is not one")?;
    let dir = dir(root);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let now = jiff::Zoned::now();
    let stem = format!("{} {}", safe(name), now.strftime("%Y-%m-%d %H.%M.%S"));
    let mut path = dir.join(format!("{stem}{ENDING}"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem} ({n}){ENDING}"));
        n += 1;
    }
    let bytes = serde_json::to_vec(speedscope).map_err(|e| e.to_string())?;
    std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((path, of(&saved)))
}

/// A name a file can have on every system.
fn safe(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    let s = s.trim().trim_end_matches('.');
    if s.is_empty() {
        "profile".into()
    } else {
        s.into()
    }
}

/// wowlua's profile as Studio's.
pub fn of(s: &Saved) -> Profile {
    let cost = |c: &wowlua_vm::profile::Cost| [c.instructions, c.bytes, c.nanos];
    Profile {
        name: s.name.clone(),
        keys: s
            .keys
            .iter()
            .map(|k| ProfileKey {
                name: k.name.clone(),
                file: k.file.clone(),
                line: k.line,
                kind: k.kind.clone(),
            })
            .collect(),
        nodes: s
            .nodes
            .iter()
            .map(|n| ProfileNode {
                key: n.key,
                parent: n.parent,
                calls: n.calls,
                cost: cost(&n.self_cost),
            })
            .collect(),
        frames: s
            .frames
            .iter()
            .map(|f| ProfileFrame {
                name: f.name.clone(),
                cost: cost(&f.cost),
                nodes: f
                    .nodes
                    .iter()
                    .map(|n| (n.node, n.calls, cost(&n.self_cost)))
                    .collect(),
            })
            .collect(),
    }
}

/// The recordings the workspace's addon exported in the game, each client
/// folder's and account's, their locations in the source.
pub fn game_recordings(root: &Path) -> Result<Vec<GameRecordings>, String> {
    let mut session = Session::new();
    let options = LoadOptions {
        cwd: root.to_path_buf(),
        lock_in_memory: true,
        offline: true,
        skip_dependencies: true,
        ..LoadOptions::default()
    };
    let projects = load_projects(&mut session, &options).ok_or_else(|| {
        crate::tests::first_error(&session).unwrap_or_else(|| "the workspace does not load".into())
    })?;
    let Some(project) = projects.iter().find(|p| p.kind() == PackageKind::Addon) else {
        return Ok(Vec::new());
    };
    let wow = wowlua_driver::install::find_wow(None).map_err(|d| d.message)?;
    let db = recordings::db_name(&project.addon_name());
    let (found, _) = recordings::find(&wow, &db, None, None);
    Ok(found
        .into_iter()
        .map(|f| {
            let mut exports = f.exports;
            for rec in &mut exports {
                recordings::map_locations(project, rec);
            }
            GameRecordings {
                client: f.client,
                account: f.account,
                path: f.path,
                recordings: exports,
            }
        })
        .collect())
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn names_work_on_every_system() {
        assert_eq!(safe("main · ThreatMeter/x:1"), "main · ThreatMeter-x-1");
        assert_eq!(safe("  ..."), "profile");
    }

    /// A kept profile is listed and reads back as it was.
    #[test]
    fn a_kept_profile_reads_back() {
        let root = std::env::temp_dir().join(format!("wl-profiles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let file = serde_json::json!({
            "shared": { "frames": [{ "name": "a" }, { "name": "b" }] },
            "profiles": [{ "type": "sampled", "samples": [[0], [0, 1]], "weights": [3, 4] }],
        });
        let (path, profile) = keep(&root, "main", &file).expect("kept");
        assert!(path.starts_with(dir(&root)));
        assert_eq!(profile.keys.len(), 2);
        let listed = list(&root);
        assert_eq!(listed.len(), 1);
        assert!(listed[0].name.starts_with("main "), "{:?}", listed[0]);
        assert_eq!(read(&path).expect("reads"), profile);
        let _ = std::fs::remove_dir_all(&root);
    }
}
