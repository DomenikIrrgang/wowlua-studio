//! The world profiles' (M8): the package's profiles, their checks as they
//! are edited, where the package names one, the snapshots taken in the game
//! (wowlua's `/wl snapshot`, T498) and SavedVariables read as data.

use std::path::{Path, PathBuf};
use wowlua_driver::Session;
use wowlua_driver::project::{LoadOptions, load_projects};
use wowlua_driver::{recordings, snapshots};
use wowlua_manifest::model::PackageKind;
use wowlua_studio_app::system::{ProfileProblem, WorldProfile, WorldSnapshot};

/// The package's world profiles (`tests/profiles/<name>.json`), by name.
pub fn profiles(root: &Path) -> Vec<WorldProfile> {
    wowlua_driver::profiles::list(root)
        .into_iter()
        .map(|(name, path)| WorldProfile { name, path })
        .collect()
}

/// A profile's problems (E4400, E4401) from its text, at their first
/// label's bytes.
pub fn check(text: &str) -> Vec<ProfileProblem> {
    let mut db = wowlua_span::SourceDb::new();
    let (_, found) = wowlua_manifest::world::check(
        &mut db,
        Path::new("tests/profiles/profile.json"),
        text.to_string(),
    );
    found
        .into_vec()
        .into_iter()
        .map(|d| {
            let (start, end) = d
                .labels
                .first()
                .map(|l| (l.span.start as usize, l.span.end as usize))
                .unwrap_or((0, 0));
            ProfileProblem {
                code: d.code.unwrap_or("E4401").to_string(),
                message: d.message,
                start,
                end,
            }
        })
        .collect()
}

/// Where the package names the profile `name`: its test files'
/// `config({ profile = "…" })` and the manifest's `test.profile`.
pub fn uses(root: &Path, name: &str, tests: &[PathBuf]) -> Vec<String> {
    let mut out = Vec::new();
    for file in tests {
        let Ok(source) = std::fs::read(file) else {
            continue;
        };
        if let Some((named, _)) = wowlua_driver::profiles::config_text(&source, "profile")
            && named == name
        {
            let rel = file.strip_prefix(root).unwrap_or(file);
            out.push(format!(
                "{} · config({{ profile = \"{name}\" }})",
                rel.to_string_lossy().replace('\\', "/")
            ));
        }
    }
    let manifest = std::fs::read_to_string(root.join("wowlua.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
    if manifest
        .as_ref()
        .and_then(|m| m["test"]["profile"].as_str())
        .is_some_and(|p| p == name)
    {
        out.push("wowlua.json · test.profile".into());
    }
    out
}

/// The snapshots of the package's addon in the installation, newest
/// first, each as the profile it makes; warnings for unreadable files.
pub fn snapshots(root: &Path) -> Result<(Vec<WorldSnapshot>, Vec<String>), String> {
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
        return Ok((Vec::new(), Vec::new()));
    };
    let wow = wowlua_driver::install::find_wow(None).map_err(|d| d.message)?;
    let db = recordings::db_name(&project.addon_name());
    let (found, warnings) = snapshots::find(&wow, &db, None, None);
    let mut out = Vec::new();
    for f in found {
        let text = snapshots::profile_text(&f.profile)?.0;
        out.push(WorldSnapshot {
            client: f.client,
            account: f.account,
            path: f.path,
            at: f.at,
            text,
            failed: f.failed,
        });
    }
    Ok((out, warnings))
}

/// A SavedVariables file's variables, as the client wrote them.
pub fn saved_variables(path: &Path) -> Result<Vec<(String, serde_json::Value)>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    recordings::parse_saved(&bytes)
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn a_profile_s_problems_are_located() {
        let text = "{\n  \"untis\": []\n}";
        let found = check(text);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].code, "E4401");
        assert!(found[0].message.contains("untis"), "{found:?}");
        assert_eq!(&text[found[0].start..found[0].end], "\"untis\"");
        assert_eq!(check("{ \"money\": 5, }")[0].code, "E4400");
        assert!(check("{ \"money\": 5 }").is_empty());
    }

    #[test]
    fn uses_are_the_test_files_and_the_manifest() {
        let root = std::env::temp_dir().join(format!("wl-uses-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("tests")).unwrap();
        std::fs::write(
            root.join("wowlua.json"),
            r#"{ "package": { "name": "a", "version": "1.0.0" }, "test": { "profile": "raid" } }"#,
        )
        .unwrap();
        let a = root.join("tests/a.test.wowlua");
        let b = root.join("tests/b.test.wowlua");
        std::fs::write(&a, "config({ profile = \"raid\" })\n").unwrap();
        std::fs::write(&b, "config({ profile = \"solo\" })\n").unwrap();
        let found = uses(&root, "raid", &[a, b]);
        assert_eq!(
            found,
            [
                "tests/a.test.wowlua · config({ profile = \"raid\" })",
                "wowlua.json · test.profile"
            ]
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
