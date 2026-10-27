//! The tests page's runs (S052): the workspace's test files run in
//! Studio's process by wowlua's test runner, as `wowlua test` runs them
//! (`wowlua_test::runs`), on a thread of their own. Each file's results,
//! the coverage and the minimums come back as events; coverage is written
//! to the workspace's `dist/test` (`coverage.json`, `coverage.html`), where
//! the next start reads it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use wowlua_driver::Session;
use wowlua_driver::project::{LoadOptions, load_projects};
use wowlua_manifest::model::{FlavorMode, PackageKind};
use wowlua_studio_app::system::{
    Coverage, CoverageFile, CoverageTotals, TestCase, TestDone, TestEvent, TestFile, TestOutcome,
    TestRequest,
};
use wowlua_test::coverage::{Report, Totals};
use wowlua_test::runs::{self, Switches};

/// Runs the tests of the workspace at `root` on a thread, telling `tell`
/// what happens; `stop` set runs no more files.
pub fn start(
    root: PathBuf,
    request: TestRequest,
    stop: Arc<AtomicBool>,
    tell: impl Fn(TestEvent) + Send + Sync + 'static,
) {
    let spawned = std::thread::Builder::new()
        .name("tests".into())
        .stack_size(64 << 20)
        .spawn(move || {
            user_initiated(move || {
                let done = run(&root, &request, &stop, &tell);
                tell(TestEvent::Done(done));
            });
        });
    if let Err(e) = spawned {
        eprintln!("wowlua studio: cannot start the test run: {e}");
    }
}

/// Runs `work` as work the user started (S054): macOS's App Nap slows a
/// window that is not in front to a fraction of one core, a test run of
/// SuuUI's 185 files from 80 s to many minutes. The system may still
/// sleep.
#[cfg(target_os = "macos")]
fn user_initiated(work: impl FnOnce() + 'static) {
    use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
    let work = std::cell::Cell::new(Some(work));
    let block = block2::RcBlock::new(move || {
        if let Some(work) = work.take() {
            work();
        }
    });
    NSProcessInfo::processInfo().performActivityWithOptions_reason_usingBlock(
        NSActivityOptions::UserInitiatedAllowingIdleSystemSleep,
        &NSString::from_str("running tests"),
        &block,
    );
}

#[cfg(not(target_os = "macos"))]
fn user_initiated(work: impl FnOnce() + 'static) {
    work()
}

/// The first error of a session, as wowlua writes it.
fn first_error(session: &Session) -> Option<String> {
    session
        .diagnostics
        .iter()
        .find(|d| d.is_error())
        .map(|d| match d.code {
            Some(code) => format!("error[{code}]: {}", d.message),
            None => format!("error: {}", d.message),
        })
}

fn run(
    root: &Path,
    request: &TestRequest,
    stop: &AtomicBool,
    tell: &(dyn Fn(TestEvent) + Sync),
) -> TestDone {
    let started = std::time::Instant::now();
    let mut done = TestDone::default();
    let mut session = Session::new();
    let options = LoadOptions {
        cwd: root.to_path_buf(),
        lock_in_memory: true,
        offline: true,
        all_members: true,
        ..LoadOptions::default()
    };
    let Some(projects) = load_projects(&mut session, &options) else {
        done.error =
            Some(first_error(&session).unwrap_or_else(|| "the workspace does not load".into()));
        return done;
    };
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let switches = Switches {
        update_snapshots: request.update_snapshots,
        coverage: request.coverage,
        game_data: false,
    };
    let mut merged = Report::default();
    let mut settings = None;
    let mut out_dir: Option<PathBuf> = None;
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut title: Option<String> = None;
    'projects: for project in &projects {
        // A library's tests run against it built as an addon.
        if !matches!(project.kind(), PackageKind::Addon | PackageKind::Library) {
            continue;
        }
        let test_settings = &project.manifest.manifest.test;
        if settings.is_none() {
            settings = test_settings.coverage.clone();
        }
        out_dir.get_or_insert_with(|| project.manifest.out_dir().join("test"));
        roots.push(project.root().to_path_buf());
        title.get_or_insert_with(|| project.addon_name());
        let flavors = project.manifest.flavors.clone();
        // A shared-mode package is one build for all its flavors.
        let shared_mode = project.manifest.manifest.build.flavor_mode == FlavorMode::Shared;
        let mut shared = None;
        for flavor in &flavors {
            if stop.load(Ordering::Relaxed) {
                break 'projects;
            }
            tell(TestEvent::Status(format!(
                "building {} for {flavor}",
                project.addon_name()
            )));
            let owned;
            let built = if shared_mode {
                if shared.is_none() {
                    shared = Some(runs::build(
                        &mut session,
                        project,
                        flavors.clone(),
                        "shared",
                        "test",
                        wowlua_driver::parallel::Jobs::resolve(None),
                        &mut |_| {},
                    ));
                }
                shared.as_ref().and_then(Option::as_ref)
            } else {
                owned = runs::build(
                    &mut session,
                    project,
                    vec![flavor.clone()],
                    flavor,
                    "test",
                    wowlua_driver::parallel::Jobs::resolve(None),
                    &mut |_| {},
                );
                owned.as_ref()
            };
            if let Some(error) = first_error(&session) {
                done.error = Some(error);
                break 'projects;
            }
            let Some((build, left_out)) = built else {
                done.error = Some(format!("{} does not build", project.addon_name()));
                break 'projects;
            };
            let has_tests = build
                .tests
                .iter()
                .any(|t| t.role == wowlua_driver::TestRole::Test);
            let blizzard_ui = if test_settings.blizzard_ui && has_tests {
                let fetching = |what: &str| tell(TestEvent::Status(format!("fetching {what}")));
                match wowlua_test::ui_source(flavor, &fetching) {
                    Ok(ui) => Some(ui),
                    Err(e) => {
                        done.error = Some(format!("test.blizzard-ui: {e}"));
                        break 'projects;
                    }
                }
            } else {
                None
            };
            let camera = wowlua_test::camera::camera(project, build, flavor);
            let mut suite =
                runs::suite(project, build, flavor, blizzard_ui, switches, None, camera);
            if let Some(profile) = &request.profile {
                suite.profile = Some(profile.clone());
            }
            let planned = build
                .tests
                .iter()
                .filter(|t| t.role == wowlua_driver::TestRole::Test)
                .filter(|t| selected(&request.filters, &t.rel))
                .count();
            tell(TestEvent::Planned(planned + left_out.len()));
            tell(TestEvent::Status(format!(
                "testing {} ({flavor})",
                project.addon_name()
            )));
            let package = project.root().to_path_buf();
            let (broken, _) = runs::not_compiling(left_out, &request.filters, flavor);
            for result in &broken {
                tell(TestEvent::File(file_of(&package, result)));
            }
            let results = wowlua_test::run_with(&suite, &request.filters, threads, stop, &|r| {
                tell(TestEvent::File(file_of(&package, r)))
            });
            for result in &results {
                if let Some(c) = &result.coverage {
                    merged.merge(c);
                }
                done.snapshots.0 += result.snapshots.written;
                done.snapshots.1 += result.snapshots.updated;
                done.snapshots.2 += result.snapshots.removed;
            }
        }
    }
    done.stopped = stop.load(Ordering::Relaxed);
    if request.coverage && done.error.is_none() {
        let exclude = wowlua_driver::glob_set(
            &settings
                .as_ref()
                .map(|s: &wowlua_manifest::model::CoverageSettings| s.exclude.clone())
                .unwrap_or_default(),
        );
        merged.files.retain(|path, _| !exclude.is_match(path));
        let mut html = None;
        if let Some(dir) = &out_dir {
            let _ = std::fs::create_dir_all(dir);
            let _ = std::fs::write(
                dir.join("coverage.json"),
                wowlua_test::reports::coverage_json(&merged),
            );
            let page =
                wowlua_test::reports::html(&merged, &roots, title.as_deref().unwrap_or("wowlua"));
            if std::fs::write(dir.join("coverage.html"), page).is_ok() {
                html = Some(dir.join("coverage.html"));
            }
        }
        // (a run with filters checks part of the suite: no minimum)
        if request.filters.is_empty()
            && !done.stopped
            && let Some(s) = &settings
        {
            done.below = runs::below_minimums(&merged, s)
                .into_iter()
                .map(|b| (b.kind.to_string(), b.percent, b.minimum))
                .collect();
        }
        let mut coverage = coverage_of(&merged, &roots);
        coverage.html = html;
        done.coverage = Some(coverage);
    }
    done.seconds = started.elapsed().as_secs_f64();
    done
}

/// Whether `filters` may select test file `rel`: none, or one whose path
/// part (`path::name`'s) is in it, or one without a path (a test's name).
fn selected(filters: &[String], rel: &str) -> bool {
    filters.is_empty()
        || filters.iter().any(|f| match f.split_once("::") {
            Some((path, _)) => rel.ends_with(path) || path.ends_with(rel),
            None => true,
        })
}

fn file_of(package: &Path, r: &wowlua_test::FileResult) -> TestFile {
    TestFile {
        package: package.to_path_buf(),
        file: r.file.clone(),
        flavor: r.flavor.clone(),
        tests: r
            .tests
            .iter()
            .map(|t| TestCase {
                name: t.name.clone(),
                outcome: match &t.outcome {
                    wowlua_test::Outcome::Pass => TestOutcome::Pass,
                    wowlua_test::Outcome::Fail { message, stack } => TestOutcome::Fail {
                        message: message.clone(),
                        stack: stack.clone(),
                    },
                    wowlua_test::Outcome::Skip(why) => TestOutcome::Skip(why.clone()),
                },
                seconds: t.seconds,
                functions: t.functions.clone(),
            })
            .collect(),
        error: r.error.clone(),
        printed: r.printed.clone(),
        seconds: r.seconds,
    }
}

fn totals_of(t: &Totals) -> CoverageTotals {
    CoverageTotals {
        lines: t.lines,
        branches: t.branches,
        functions: t.functions,
        conditions: t.conditions,
    }
}

/// The package a source belongs to (the first of `roots` that has it).
fn absolute(path: &str, roots: &[PathBuf]) -> PathBuf {
    roots
        .iter()
        .map(|r| r.join(path))
        .find(|p| p.is_file())
        .unwrap_or_else(|| {
            roots
                .first()
                .map_or_else(|| PathBuf::from(path), |r| r.join(path))
        })
}

fn coverage_of(report: &Report, roots: &[PathBuf]) -> Coverage {
    Coverage {
        totals: totals_of(&report.totals()),
        files: report
            .files
            .iter()
            .map(|(path, f)| {
                let file = CoverageFile {
                    totals: totals_of(&Report::file_totals(f)),
                    lines: f.lines.clone(),
                    branches: f.branches.clone(),
                    functions: f
                        .functions
                        .iter()
                        .map(|((line, _), (name, calls))| (*line, (name.clone(), *calls)))
                        .collect(),
                };
                (absolute(path, roots), file)
            })
            .collect(),
        html: None,
    }
}

/// A package of the workspace: its name, root, source root and main module.
struct Package {
    name: String,
    root: PathBuf,
    src: PathBuf,
    main: PathBuf,
}

/// The test files a change to `changed` may change the results of: itself
/// when it is one, and those that import it, directly or through other
/// files (Language §5.3's specifiers: `./x`, `/x`, a member's `pkg/x`).
pub fn affected(root: &Path, changed: &Path) -> Vec<PathBuf> {
    let mut session = Session::new();
    let options = LoadOptions {
        cwd: root.to_path_buf(),
        lock_in_memory: true,
        offline: true,
        skip_dependencies: true,
        all_members: true,
        ..LoadOptions::default()
    };
    let Some(projects) = load_projects(&mut session, &options) else {
        return Vec::new();
    };
    let packages: Vec<Package> = projects
        .iter()
        .map(|p| {
            let src = p.root().join(&p.manifest.manifest.build.src);
            Package {
                name: p
                    .manifest
                    .manifest
                    .package
                    .as_ref()
                    .map(|k| k.name.clone())
                    .unwrap_or_default(),
                root: p.root().to_path_buf(),
                main: src.join("init.wowlua"),
                src,
            }
        })
        .collect();
    let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    // Who imports each file.
    let mut importers: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for package in &packages {
        let files = ignore::WalkBuilder::new(&package.root)
            .build()
            .flatten()
            .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
            .map(|e| e.into_path())
            .filter(|p| p.extension().is_some_and(|e| e == "wowlua"));
        for file in files {
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            let file = canonical(&file);
            for spec in imports(&text) {
                if let Some(target) = resolve(&spec, &file, package, &packages) {
                    importers
                        .entry(canonical(&target))
                        .or_default()
                        .push(file.clone());
                }
            }
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut todo = vec![canonical(changed)];
    while let Some(file) = todo.pop() {
        if !seen.insert(file.clone()) {
            continue;
        }
        if let Some(list) = importers.get(&file) {
            todo.extend(list.iter().cloned());
        }
    }
    seen.into_iter()
        .filter(|p| p.to_string_lossy().ends_with(".test.wowlua"))
        .collect()
}

/// The module specifiers a file imports (`… from "x"`, `import "x"`).
fn imports(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for marker in ["from \"", "import \""] {
        let mut rest = text;
        while let Some(i) = rest.find(marker) {
            rest = &rest[i + marker.len()..];
            if let Some(end) = rest.find('"') {
                out.push(rest[..end].to_owned());
                rest = &rest[end..];
            }
        }
    }
    out
}

/// The file a specifier names, as the compiler resolves it (§5.3): the
/// exact file, `.wowlua`, `.d.wowlua`, `.lua`, `/init.wowlua`.
fn resolve(spec: &str, file: &Path, own: &Package, packages: &[Package]) -> Option<PathBuf> {
    let base = if spec.starts_with("./") || spec.starts_with("../") {
        file.parent()?.join(spec)
    } else if let Some(rest) = spec.strip_prefix('/') {
        own.src.join(rest)
    } else if spec.contains(':') {
        return None;
    } else {
        let (name, rest) = spec.split_once('/').unwrap_or((spec, ""));
        let package = packages.iter().find(|p| p.name == name)?;
        if rest.is_empty() {
            return package.main.is_file().then(|| package.main.clone());
        }
        package.src.join(rest)
    };
    let with = |ext: &str| PathBuf::from(format!("{}{ext}", base.display()));
    [
        base.clone(),
        with(".wowlua"),
        with(".d.wowlua"),
        with(".lua"),
        base.join("init.wowlua"),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

/// The coverage a workspace's `dist/test/coverage.json` keeps: the last
/// run's with coverage, Studio's or `wowlua test --coverage`'s.
pub fn read_coverage(root: &Path) -> Option<Coverage> {
    let mut session = Session::new();
    let options = LoadOptions {
        cwd: root.to_path_buf(),
        lock_in_memory: true,
        offline: true,
        skip_dependencies: true,
        all_members: true,
        ..LoadOptions::default()
    };
    let projects = load_projects(&mut session, &options)?;
    let roots: Vec<PathBuf> = projects.iter().map(|p| p.root().to_path_buf()).collect();
    let dir = projects.first()?.manifest.out_dir().join("test");
    let text = std::fs::read_to_string(dir.join("coverage.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let count = |t: &serde_json::Value| -> (u64, u64) {
        (
            t["covered"].as_u64().unwrap_or(0),
            t["total"].as_u64().unwrap_or(0),
        )
    };
    let totals = |t: &serde_json::Value| CoverageTotals {
        lines: count(&t["lines"]),
        branches: count(&t["branches"]),
        functions: count(&t["functions"]),
        conditions: count(&t["conditions"]),
    };
    let mut files = BTreeMap::new();
    for (path, f) in json["files"].as_object()? {
        let lines = f["lines"]
            .as_object()
            .map(|m| {
                m.iter()
                    .filter_map(|(l, h)| Some((l.parse().ok()?, h.as_u64()?)))
                    .collect()
            })
            .unwrap_or_default();
        let branches = f["branches"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|b| {
                        let key = (b["line"].as_u64()? as u32, b["index"].as_u64()? as u32);
                        Some((key, [b["taken"].as_u64()?, b["notTaken"].as_u64()?]))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let functions = f["functions"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| {
                        let name = x["name"].as_str()?.to_owned();
                        Some((x["line"].as_u64()? as u32, (name, x["calls"].as_u64()?)))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let file = CoverageFile {
            totals: totals(&f["totals"]),
            lines,
            branches,
            functions,
        };
        files.insert(absolute(path, &roots), file);
    }
    let html = dir.join("coverage.html");
    Some(Coverage {
        totals: totals(&json["totals"]),
        files,
        html: html.is_file().then_some(html),
    })
}

#[cfg(test)]
mod unit {
    use super::{affected, read_coverage};
    use std::path::{Path, PathBuf};

    /// A package in a folder of its own: its files, by path and text.
    fn package(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("wowlua-studio-tests-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let manifest = r#"{ "package": { "name": "demo", "version": "1.0.0" }, "flavors": { "retail": {} }, "addon": { "title": "Demo" } }"#;
        for (path, text) in [("wowlua.json", manifest)]
            .into_iter()
            .chain(files.iter().copied())
        {
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        std::fs::canonicalize(dir).unwrap()
    }

    fn names(files: Vec<PathBuf>, root: &Path) -> Vec<String> {
        files
            .iter()
            .map(|f| {
                f.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }

    /// A saved file runs the tests importing it, directly or through other
    /// files, by any specifier; a saved test file runs itself.
    #[test]
    fn a_change_affects_the_tests_that_import_it() {
        let root = package(
            "affected",
            &[
                (
                    "src/a.wowlua",
                    "export function double(x: number): number\n    return x * 2\nend\n",
                ),
                (
                    "src/b.wowlua",
                    "import { double } from \"./a\"\nexport function quad(x: number): number\n    return double(double(x))\nend\n",
                ),
                ("src/c.wowlua", "export const C = 1\n"),
                ("tests/b.test.wowlua", "import { quad } from \"/b\"\n"),
                (
                    "tests/name.test.wowlua",
                    "import { double } from \"demo/a\"\n",
                ),
                ("tests/c.test.wowlua", "import { C } from \"/c\"\n"),
            ],
        );
        assert_eq!(
            names(affected(&root, &root.join("src/a.wowlua")), &root),
            ["tests/b.test.wowlua", "tests/name.test.wowlua"]
        );
        assert_eq!(
            names(affected(&root, &root.join("tests/c.test.wowlua")), &root),
            ["tests/c.test.wowlua"]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// `dist/test/coverage.json` as Studio shows it: by absolute path.
    #[test]
    fn the_kept_coverage_is_read() {
        let json = r#"{
            "totals": { "lines": { "covered": 3, "total": 4, "percent": 75 } },
            "files": { "src/a.wowlua": {
                "totals": { "lines": { "covered": 3, "total": 4, "percent": 75 } },
                "lines": { "1": 2, "2": 0 },
                "branches": [{ "line": 2, "index": 0, "taken": 1, "notTaken": 0 }],
                "functions": [{ "line": 1, "name": "double", "calls": 2 }]
            } }
        }"#;
        let root = package(
            "coverage",
            &[
                (
                    "src/a.wowlua",
                    "export function double(x: number): number\n    return x * 2\nend\n",
                ),
                ("dist/test/coverage.json", json),
            ],
        );
        let coverage = read_coverage(&root).expect("coverage");
        assert_eq!(coverage.totals.lines, (3, 4));
        let file = &coverage.files[&root.join("src/a.wowlua")];
        assert_eq!(file.lines.get(&1), Some(&2));
        assert_eq!(file.lines.get(&2), Some(&0));
        assert_eq!(file.branches.get(&(2, 0)), Some(&[1, 0]));
        assert_eq!(file.functions.get(&1), Some(&("double".to_owned(), 2)));
        assert_eq!(coverage.html, None);
        let _ = std::fs::remove_dir_all(root);
    }
}
