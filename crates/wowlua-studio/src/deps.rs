//! The dependencies page's work (M10, S062), in Studio's process as
//! wowlua's commands do it: the package's dependencies and how they
//! resolved, `wowlua outdated`, the registry's search and a package's
//! versions, `wowlua add`, `remove` and `update` with what changed in the
//! lockfile.

use std::collections::BTreeMap;
use std::path::Path;

use wowlua_driver::Session;
use wowlua_driver::deps::AddRequest;
use wowlua_driver::project::{LoadOptions, Project, load_project};
use wowlua_manifest::model::{Dependency, DependencyDetail};
use wowlua_pkg::{GitRef, Source};
use wowlua_studio_app::system::{DepFound, DepNewer, DepRow, DepsCommand, DepsFacts};

/// A diagnostic as compiler output: `warning[W4322]: …`.
fn line(d: &wowlua_diagnostics::Diagnostic) -> String {
    match d.code {
        Some(code) => format!("{}[{code}]: {}", d.severity.as_str(), d.message),
        None => format!("{}: {}", d.severity.as_str(), d.message),
    }
}

/// The package, as its commands load it, without writing the lockfile.
fn load(session: &mut Session, dir: &Path, offline: bool) -> Option<Project> {
    let options = LoadOptions {
        lock_in_memory: true,
        offline,
        ..crate::ship::options(dir)
    };
    load_project(session, &options)
}

/// `owner/repo` of a GitHub URL, else the host and path.
fn short_url(url: &str) -> String {
    let rest = url
        .split_once("://")
        .map_or(url, |(_, r)| r)
        .trim_end_matches('/')
        .trim_end_matches(".git");
    match rest.strip_prefix("github.com/") {
        Some(repo) => repo.to_owned(),
        None => rest.to_owned(),
    }
}

/// What a declaration asks for: its requirement, tag, branch or revision.
fn requirement(dep: &Dependency) -> String {
    let detail: DependencyDetail = match dep {
        Dependency::Version(v) => return v.clone(),
        Dependency::Detailed(d) => (**d).clone(),
    };
    if let Some(v) = detail.version {
        return v;
    }
    if detail.git.is_some() {
        return match (detail.tag, detail.branch, detail.rev) {
            (Some(t), _, _) => format!("tag {t}"),
            (_, Some(b), _) => format!("branch {b}"),
            (_, _, Some(r)) => format!("rev {r}"),
            _ => "the default branch".into(),
        };
    }
    if detail.svn.is_some() {
        return detail
            .rev
            .map_or_else(|| "the newest revision".into(), |r| format!("r{r}"));
    }
    if detail.path.is_some() {
        return "path".into();
    }
    if detail.archive.is_some() {
        return "its checksum".into();
    }
    "*".into()
}

fn source(source: &Source) -> String {
    match source {
        Source::Registry { url, .. } => {
            if url.trim_end_matches('/')
                == wowlua_pkg::registry::DEFAULT_REGISTRY.trim_end_matches('/')
            {
                "registry".into()
            } else {
                format!("registry · {}", short_url(url))
            }
        }
        Source::Git { url, .. } => format!("git · {}", short_url(url)),
        Source::Svn { url, .. } => format!("svn · {}", short_url(url)),
        Source::Path { path, .. } => format!("path · {path}"),
        Source::Archive { url, .. } => format!("archive · {}", short_url(url)),
    }
}

/// The locked version, with its tag's commit or revision.
fn locked(p: &wowlua_pkg::ResolvedPackage) -> String {
    let short = |s: &str| s.chars().take(8).collect::<String>();
    match (&p.source, &p.pin) {
        (Source::Git { reference, .. }, Some(pin)) => match reference {
            GitRef::Tag(_) | GitRef::Branch(_) | GitRef::Head => short(pin),
            GitRef::Rev(r) => short(r),
        },
        (Source::Svn { .. }, Some(pin)) => format!("r{pin}"),
        _ => p.version.to_string(),
    }
}

pub fn read(dir: &Path) -> Result<DepsFacts, String> {
    let mut session = Session::new();
    let Some(project) = load(&mut session, dir, false) else {
        return Err(crate::tests::first_error(&session)
            .unwrap_or_else(|| "the package does not load".into()));
    };
    let manifest = &project.manifest.manifest;
    let yanked: Vec<String> = project
        .deps
        .warnings
        .iter()
        .filter(|w| w.code == Some("W4322"))
        .map(|w| w.message.clone())
        .collect();
    let mut rows = Vec::new();
    for (dev, deps) in [
        (false, &manifest.dependencies),
        (true, &manifest.dev_dependencies),
    ] {
        for (name, dep) in deps {
            let resolved = project.deps.packages.iter().find(|p| &p.name == name);
            rows.push(DepRow {
                name: name.clone(),
                dev,
                source: resolved.map_or_else(String::new, |p| source(&p.source)),
                registry: resolved.is_some_and(|p| matches!(p.source, Source::Registry { .. })),
                kind: resolved.map_or_else(String::new, |p| {
                    serde_json::to_value(p.kind)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .unwrap_or_default()
                }),
                requirement: requirement(dep),
                locked: resolved.map_or_else(String::new, locked),
                yanked: yanked
                    .iter()
                    .any(|m| m.contains(&format!("`{name}`")) || m.contains(&format!("{name} "))),
                description: resolved
                    .and_then(|p| p.manifest.as_ref())
                    .and_then(|m| m.package.as_ref())
                    .and_then(|p| p.description.clone())
                    .unwrap_or_default(),
            });
        }
    }
    let problems = session
        .take_diagnostics()
        .into_vec()
        .iter()
        .chain(project.deps.warnings.iter())
        .map(line)
        .collect();
    Ok(DepsFacts {
        package: project
            .manifest
            .package()
            .map(|p| p.name.clone())
            .unwrap_or_default(),
        manifest: project.manifest.path.clone(),
        rows,
        problems,
    })
}

/// `wowlua outdated`.
pub fn outdated(dir: &Path) -> Result<Vec<DepNewer>, String> {
    let mut session = Session::new();
    let Some(project) = load(&mut session, dir, false) else {
        return Err(crate::tests::first_error(&session)
            .unwrap_or_else(|| "the package does not load".into()));
    };
    let rows = wowlua_driver::deps::outdated(&mut session, &project, false, None);
    if let Some(error) = crate::tests::first_error(&session) {
        return Err(error);
    }
    Ok(rows
        .into_iter()
        .map(|o| DepNewer {
            name: o.name,
            latest: o.latest,
            action: o.action,
        })
        .collect())
}

/// `wowlua search`: the default registry's first page.
pub fn search(query: &str) -> Result<Vec<DepFound>, String> {
    let http = wowlua_driver::publish::http::Http::default();
    let registry = wowlua_driver::publish::registry::chosen_registry(None);
    let page = wowlua_driver::publish::registry::search(&http, &registry, query, 1, 20)
        .map_err(|d| d.message)?;
    Ok(page
        .packages
        .into_iter()
        .map(|f| DepFound {
            name: f.name,
            newest: f.vers,
            description: f.description,
        })
        .collect())
}

/// A registry package's versions, newest first, with whether each was
/// yanked.
pub fn versions(name: &str) -> Vec<(String, bool)> {
    let Some(cache) = wowlua_pkg::Cache::new() else {
        return Vec::new();
    };
    let fetcher = wowlua_pkg::Fetcher::new(cache, false, |_, _| {});
    let url = wowlua_pkg::registry::DEFAULT_REGISTRY;
    let Ok(Some(entries)) = fetcher.registry_index(url, name, true) else {
        return Vec::new();
    };
    let mut list: Vec<(semver::Version, bool)> = entries
        .iter()
        .filter_map(|e| Some((semver::Version::parse(&e.vers).ok()?, e.yanked)))
        .collect();
    list.sort_by(|a, b| b.0.cmp(&a.0));
    list.into_iter().map(|(v, y)| (v.to_string(), y)).collect()
}

/// The locked packages by name: version and pin.
fn lock_of(project: &Project) -> BTreeMap<String, String> {
    project
        .lock
        .packages
        .iter()
        .map(|p| {
            let pin = p.pin().map(|s| s.chars().take(8).collect::<String>());
            let shown = match pin {
                Some(pin) if !pin.is_empty() => format!("{} ({pin})", p.version),
                _ => p.version.clone(),
            };
            (p.name.clone(), shown)
        })
        .collect()
}

/// What changed in the lockfile, as the CLI says it.
fn changes(before: &BTreeMap<String, String>, after: &BTreeMap<String, String>) -> Vec<String> {
    let mut out = Vec::new();
    for (name, now) in after {
        match before.get(name) {
            None => out.push(format!("    Locking {name} {now}")),
            Some(was) if was != now => out.push(format!("   Updating {name} {was} -> {now}")),
            Some(_) => {}
        }
    }
    for name in before.keys().filter(|n| !after.contains_key(*n)) {
        out.push(format!("   Removing {name}"));
    }
    out
}

/// Runs a command: whether it worked, and what it changed or its errors.
pub fn run(dir: &Path, command: &DepsCommand) -> (bool, Vec<String>) {
    let before = {
        let mut session = Session::new();
        load(&mut session, dir, true)
            .map(|p| lock_of(&p))
            .unwrap_or_default()
    };
    let mut session = Session::new();
    let options = crate::ship::options(dir);
    let project = match command {
        DepsCommand::Add {
            name,
            requirement,
            dev,
        } => {
            let request = AddRequest {
                name: name.clone(),
                requirement: requirement.clone(),
                dev: *dev,
                ..AddRequest::default()
            };
            wowlua_driver::deps::add(&mut session, &options, &request)
        }
        DepsCommand::Remove { name, dev } => {
            wowlua_driver::deps::remove(&mut session, &options, name, *dev)
        }
        DepsCommand::Update(names) => {
            let options = LoadOptions {
                update: Some(if names.is_empty() {
                    wowlua_pkg::Update::All
                } else {
                    wowlua_pkg::Update::Only(names.clone())
                }),
                refresh: true,
                ..options
            };
            load_project(&mut session, &options)
        }
    };
    let errors: Vec<String> = session
        .take_diagnostics()
        .into_vec()
        .iter()
        .filter(|d| d.is_error())
        .map(line)
        .collect();
    match project {
        Some(p) if errors.is_empty() => {
            let mut lines = changes(&before, &lock_of(&p));
            if lines.is_empty() {
                lines.push("    nothing changed in wowlua.lock".into());
            }
            (true, lines)
        }
        _ => (false, errors),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_read_short() {
        assert_eq!(
            short_url("https://github.com/WoWUIDev/Ace3.git"),
            "WoWUIDev/Ace3"
        );
        assert_eq!(
            short_url("https://repos.wowace.com/wow/libstub/"),
            "repos.wowace.com/wow/libstub"
        );
    }

    #[test]
    fn what_a_declaration_asks() {
        let detail = |v: serde_json::Value| -> Dependency { serde_json::from_value(v).unwrap() };
        assert_eq!(requirement(&detail(serde_json::json!("^1.2"))), "^1.2");
        assert_eq!(
            requirement(&detail(
                serde_json::json!({ "git": "https://x/y", "tag": "Release-r1390" })
            )),
            "tag Release-r1390"
        );
        assert_eq!(
            requirement(&detail(serde_json::json!({ "path": "libs/X" }))),
            "path"
        );
        assert_eq!(
            requirement(&detail(serde_json::json!({ "svn": "https://x/y" }))),
            "the newest revision"
        );
    }

    #[test]
    fn lock_changes_read_as_the_cli_says_them() {
        let map = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
            pairs
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect()
        };
        let before = map(&[
            ("lib-tooltip", "1.2.1"),
            ("old", "0.1.0"),
            ("same", "1.0.0"),
        ]);
        let after = map(&[
            ("lib-tooltip", "1.2.3"),
            ("new", "2.0.0"),
            ("same", "1.0.0"),
        ]);
        assert_eq!(
            changes(&before, &after),
            [
                "   Updating lib-tooltip 1.2.1 -> 1.2.3",
                "    Locking new 2.0.0",
                "   Removing old"
            ]
        );
    }

    /// A package with a path dependency: read, then removed and added
    /// again, offline.
    #[test]
    fn a_path_dependency_read_removed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let write = |p: &str, t: &str| {
            let p = root.join(p);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, t).unwrap();
        };
        write(
            "wowlua.json",
            r#"{ "package": { "name": "deps-test", "version": "1.0.0" },
                "dependencies": { "helper": { "path": "libs/helper" } } }"#,
        );
        write("src/main.wowlua", "print(1)\n");
        write(
            "libs/helper/wowlua.json",
            r#"{ "package": { "name": "helper", "version": "0.3.0", "kind": "library",
                "description": "A helper." } }"#,
        );
        write("libs/helper/src/init.wowlua", "return {}\n");
        let facts = read(root).unwrap();
        assert_eq!(facts.package, "deps-test");
        let helper = &facts.rows[0];
        assert_eq!(
            (
                helper.name.as_str(),
                helper.source.as_str(),
                helper.requirement.as_str()
            ),
            ("helper", "path · libs/helper", "path")
        );
        assert_eq!(helper.locked, "0.3.0");
        assert_eq!(helper.kind, "library");
        assert_eq!(helper.description, "A helper.");
        let (ok, lines) = run(
            root,
            &DepsCommand::Remove {
                name: "helper".into(),
                dev: false,
            },
        );
        assert!(ok, "{lines:?}");
        assert!(read(root).unwrap().rows.is_empty());
        let (ok, lines) = run(
            root,
            &DepsCommand::Remove {
                name: "helper".into(),
                dev: false,
            },
        );
        assert!(!ok);
        assert!(lines[0].contains("is not in `dependencies`"), "{lines:?}");
    }
}
