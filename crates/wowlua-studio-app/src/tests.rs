//! The tests page (the tests board, S052): the test explorer (the
//! workspace's test files and their tests as the language server reads
//! them, with the last run's results) and the results: the run's progress
//! and summary, a failure with its source and stack, a snapshot's
//! difference, a screenshot's images, the output. A run is a job (S044);
//! the results stay until the next run, across restarts.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use serde::{Deserialize, Serialize};
use wowlua_studio_ui::forms::{self, CalloutKind};
use wowlua_studio_ui::widgets::{self, ButtonKind};
use wowlua_studio_ui::{Face, Palette, palette};

use crate::editor::view::{Lens, LensAct, LensClick, Tone};
use crate::lsp::Discovered;
use crate::system::{System, TestDone, TestEvent, TestFile, TestOutcome, TestRequest, Workspace};

/// A row, at the settings' density (S062).
fn row_h() -> f32 {
    wowlua_studio_ui::row(24.0)
}

/// The results panel's tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Failure,
    Snapshot,
    Screenshot,
    Output,
}

/// The last run, as the summary shows it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LastRun {
    pub seconds: f64,
    pub stopped: bool,
    pub error: Option<String>,
    /// The coverage kinds below their minimums: kind, percentage, minimum.
    pub below: Vec<(String, f64, f64)>,
    pub coverage: bool,
    /// Every test ran (no filter).
    pub whole: bool,
    /// Snapshots and screenshots written, updated, removed.
    pub snapshots: (usize, usize, usize),
}

/// What a workspace keeps of its tests between starts.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Kept {
    pub results: Vec<TestFile>,
    pub last: Option<LastRun>,
}

/// The run going on.
#[derive(Clone, Debug, PartialEq)]
pub struct Running {
    pub run: u64,
    pub request: TestRequest,
    pub planned: usize,
    pub done: usize,
    pub status: String,
    pub current: String,
    pub passed: usize,
    pub failed: usize,
    /// Its job in the status bar.
    pub job: u64,
}

#[derive(Default)]
pub struct TestsState {
    /// The workspace's test files, once listed.
    pub files: Option<Vec<PathBuf>>,
    /// Each test file's tests, as the language server read them.
    pub discovered: BTreeMap<PathBuf, Vec<Discovered>>,
    /// Each test file's last results, by its path.
    pub results: BTreeMap<PathBuf, TestFile>,
    pub last: Option<LastRun>,
    pub running: Option<Running>,
    /// The last run's coverage (`dist/test/coverage.json`).
    pub coverage: Option<crate::system::Coverage>,
    pub coverage_loaded: bool,
    /// The toolbar's: runs with coverage, runs the affected tests on save.
    pub coverage_on: bool,
    pub run_on_save: bool,
    /// The world profile a run starts in, over `test.profile`.
    pub profile: Option<String>,
    /// The coverage panel's.
    pub cov: crate::coverage::CoverageView,
    /// The explorer shows only these tests (those calling a function, from
    /// its lens): what they are, which.
    pub pinned: Option<(String, Vec<(PathBuf, String)>)>,
    filter: String,
    only_failed: bool,
    /// The explorer's folded nodes.
    closed: BTreeSet<String>,
    /// The test file and test shown.
    pub selected: Option<(PathBuf, Option<String>)>,
    pub tab: Tab,
    /// Test files' lines, for a failure's source.
    sources: HashMap<PathBuf, Vec<String>>,
    /// A screenshot test's images.
    images: HashMap<PathBuf, Option<egui::TextureHandle>>,
    next: u64,
    /// Counts the changes the explorer's rows show (results, tests read,
    /// folds): its rows are made again only then.
    generation: u64,
    rows: Option<(RowsKey, Vec<Row>)>,
}

/// What the explorer's rows were made for.
type RowsKey = (u64, String, bool, usize);

/// What the tests page asks Studio to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Run(TestRequest),
    Stop,
    /// Open a file at a line (from 0).
    Open(PathBuf, usize),
    /// Debug a test (S056).
    Debug(PathBuf, Option<String>),
    /// Profile a test (S057).
    Profile(PathBuf, Option<String>),
    ShowCoverage,
}

/// A test's state in the explorer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Pass,
    Fail,
    Skip,
    NotRun,
    Running,
}

impl TestsState {
    /// The workspace's state as it was kept.
    pub fn restore(&mut self, kept: Kept) {
        self.generation += 1;
        self.results = kept
            .results
            .into_iter()
            .map(|f| (f.package.join(&f.file), f))
            .collect();
        self.last = kept.last;
    }

    /// What to keep.
    pub fn kept(&self) -> Kept {
        Kept {
            results: self.results.values().cloned().collect(),
            last: self.last.clone(),
        }
    }

    /// A new run's number.
    pub fn next_run(&mut self) -> u64 {
        self.next += 1;
        self.next
    }

    /// A test run's news; the run's end when it ended.
    pub fn event(&mut self, run: u64, event: TestEvent) -> Option<TestDone> {
        self.generation += 1;
        let running = self.running.as_mut().filter(|r| r.run == run)?;
        match event {
            TestEvent::Planned(n) => running.planned += n,
            TestEvent::Status(s) => running.status = s,
            TestEvent::File(file) => {
                running.done += 1;
                running.current = file.file.clone();
                for t in &file.tests {
                    match t.outcome {
                        TestOutcome::Pass => running.passed += 1,
                        TestOutcome::Fail { .. } => running.failed += 1,
                        TestOutcome::Skip(_) => {}
                    }
                }
                if file.error.is_some() {
                    running.failed += 1;
                }
                let key = file.package.join(&file.file);
                let merged = match self.results.remove(&key) {
                    // A filtered run's tests over the last run's of the file.
                    Some(mut old)
                        if !running.request.filters.is_empty() && file.error.is_none() =>
                    {
                        for t in file.tests {
                            match old.tests.iter_mut().find(|o| o.name == t.name) {
                                Some(o) => *o = t,
                                None => old.tests.push(t),
                            }
                        }
                        old.error = None;
                        old.printed = file.printed;
                        old.seconds = file.seconds;
                        old
                    }
                    _ => file,
                };
                self.results.insert(key, merged);
            }
            TestEvent::Profiled { .. } => {}
            TestEvent::Done(done) => {
                let request = self.running.take()?.request;
                self.last = Some(LastRun {
                    seconds: done.seconds,
                    stopped: done.stopped,
                    error: done.error.clone(),
                    below: done.below.clone(),
                    coverage: request.coverage,
                    whole: request.filters.is_empty(),
                    snapshots: done.snapshots,
                });
                if let Some(c) = &done.coverage {
                    self.coverage = Some(c.clone());
                    self.cov.minimums = None;
                }
                // Snapshots and screenshots written anew: their images too.
                self.images.clear();
                return Some(done);
            }
        }
        None
    }

    /// A run begins: a whole one starts from no results.
    pub fn begin(&mut self, run: u64, request: TestRequest, job: u64) {
        self.generation += 1;
        if request.filters.is_empty() {
            self.results.clear();
        }
        self.running = Some(Running {
            run,
            request,
            planned: 0,
            done: 0,
            status: "loading the workspace".into(),
            current: String::new(),
            passed: 0,
            failed: 0,
            job,
        });
    }

    /// The tests of files that failed, as filters (`path::name`).
    pub fn failed_filters(&self) -> Vec<String> {
        let mut out = Vec::new();
        for f in self.results.values() {
            if f.error.is_some() {
                out.push(f.file.clone());
                continue;
            }
            for t in &f.tests {
                if matches!(t.outcome, TestOutcome::Fail { .. }) {
                    out.push(format!("{}::{}", f.file, declared(&t.name)));
                }
            }
        }
        out.dedup();
        out
    }

    /// The counts of the last results: passed, failed, skipped, files.
    pub fn counts(&self) -> (usize, usize, usize, usize) {
        let (mut p, mut f, mut s) = (0, 0, 0);
        for file in self.results.values() {
            if file.error.is_some() {
                f += 1;
            }
            for t in &file.tests {
                match t.outcome {
                    TestOutcome::Pass => p += 1,
                    TestOutcome::Fail { .. } => f += 1,
                    TestOutcome::Skip(_) => s += 1,
                }
            }
        }
        (p, f, s, self.results.len())
    }

    /// Pins the explorer to the tests that call the function defined at
    /// `line` (from 1) of `file`, as the last run with coverage saw them.
    pub fn pin_callers(&mut self, workspace: &Workspace, file: &Path, line: u32) {
        let rel = rel_of(workspace, file);
        let mut tests: Vec<(PathBuf, String)> = Vec::new();
        for (path, f) in &self.results {
            for t in &f.tests {
                if t.functions.iter().any(|(src, l)| *src == rel && *l == line) {
                    tests.push((path.clone(), declared(&t.name).to_owned()));
                }
            }
        }
        tests.dedup();
        let name = self
            .coverage
            .as_ref()
            .and_then(|c| c.files.get(file))
            .and_then(|c| c.functions.get(&line))
            .map_or_else(|| format!("{rel}:{line}"), |(n, _)| n.clone());
        self.pinned = Some((format!("the tests that call {name}"), tests));
        self.generation += 1;
    }

    /// A test file's tests read anew (after a save).
    pub fn discovered(&mut self, path: PathBuf, tests: Vec<Discovered>) {
        self.generation += 1;
        self.sources.remove(&path);
        self.discovered.insert(path, tests);
    }

    /// The results of the test (or a case of it) named `name` in `file`.
    pub(crate) fn results_of(&self, file: &Path, name: &str) -> Vec<&crate::system::TestCase> {
        self.results
            .get(file)
            .map(|f| {
                f.tests
                    .iter()
                    .filter(|t| t.name == name || declared(&t.name) == name)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn status_of(&self, file: &Path, name: Option<&str>) -> Status {
        if let Some(r) = &self.running
            && !self.results.contains_key(file)
            && selects(&r.request.filters, file, name)
        {
            return Status::Running;
        }
        let Some(f) = self.results.get(file) else {
            return Status::NotRun;
        };
        let tests: Vec<&crate::system::TestCase> = match name {
            Some(n) => self.results_of(file, n),
            None => f.tests.iter().collect(),
        };
        if f.error.is_some()
            || tests
                .iter()
                .any(|t| matches!(t.outcome, TestOutcome::Fail { .. }))
        {
            Status::Fail
        } else if tests.is_empty() {
            Status::NotRun
        } else if tests
            .iter()
            .all(|t| matches!(t.outcome, TestOutcome::Skip(_)))
        {
            Status::Skip
        } else {
            Status::Pass
        }
    }
}

/// `path` from its package's root, with `/`.
pub(crate) fn rel_of(workspace: &Workspace, path: &Path) -> String {
    let package = package_of(workspace, path);
    path.strip_prefix(&package)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// The tests' lenses over a file (S053): a test file's run, each test's
/// result and run or failure; over a source function, after a run with
/// coverage, how many tests call it.
pub fn lenses(state: &TestsState, workspace: &Workspace, path: &Path) -> Vec<Lens> {
    let mut out = Vec::new();
    let file = path.to_path_buf();
    let click = |title: &str, line: usize, c: LensClick| Lens {
        line,
        title: title.to_owned(),
        act: LensAct::Click(c),
    };
    let note = |title: String, line: usize, tone: Tone| Lens {
        line,
        title,
        act: LensAct::Note(tone),
    };
    if let Some(tests) = state.discovered.get(path) {
        out.push(click(
            "run all tests",
            0,
            LensClick::Run {
                file: file.clone(),
                test: None,
            },
        ));
        out.push(click(
            "debug",
            0,
            LensClick::Debug {
                file: file.clone(),
                test: None,
            },
        ));
        out.push(click(
            "profile",
            0,
            LensClick::Profile {
                file: file.clone(),
                test: None,
            },
        ));
        let n = tests.len();
        out.push(note(
            format!("{n} test{}", if n == 1 { "" } else { "s" }),
            0,
            Tone::Muted,
        ));
        for t in tests {
            let results = state.results_of(path, &t.name);
            // Over the test's first decorator.
            let line = t.start;
            let run = || {
                click(
                    "run test",
                    line,
                    LensClick::Run {
                        file: file.clone(),
                        test: Some(t.name.clone()),
                    },
                )
            };
            let debug = || {
                click(
                    "debug",
                    line,
                    LensClick::Debug {
                        file: file.clone(),
                        test: Some(t.name.clone()),
                    },
                )
            };
            let profile = || {
                click(
                    "profile",
                    line,
                    LensClick::Profile {
                        file: file.clone(),
                        test: Some(t.name.clone()),
                    },
                )
            };
            if results.is_empty() {
                if t.skip {
                    out.push(note("skipped".into(), line, Tone::Muted));
                }
                out.push(run());
                out.push(debug());
                out.push(profile());
            } else if let Some(failed) = results
                .iter()
                .find(|r| matches!(r.outcome, TestOutcome::Fail { .. }))
            {
                let at = match &failed.outcome {
                    TestOutcome::Fail { message, stack } => places(&format!("{message}\n{stack}"))
                        .into_iter()
                        .find(|(_, p, _)| path.to_string_lossy().ends_with(p.as_str()))
                        .map(|(_, _, n)| format!(" at line {n}"))
                        .unwrap_or_default(),
                    _ => String::new(),
                };
                out.push(note(format!("failed{at}"), line, Tone::Error));
                out.push(click(
                    "show failure",
                    line,
                    LensClick::Failure {
                        file: file.clone(),
                        test: failed.name.clone(),
                    },
                ));
                out.push(run());
                out.push(debug());
                out.push(profile());
            } else if results
                .iter()
                .all(|r| matches!(r.outcome, TestOutcome::Skip(_)))
            {
                out.push(note("skipped".into(), line, Tone::Muted));
            } else {
                let passed = if t.cases {
                    format!("{} cases passed", results.len())
                } else {
                    "passed".into()
                };
                out.push(note(passed, line, Tone::Ok));
                out.push(run());
                out.push(debug());
                out.push(profile());
            }
        }
    }
    // A source function: the tests that call it.
    let rel = rel_of(workspace, path);
    let mut by_line: BTreeMap<u32, BTreeSet<(PathBuf, String)>> = BTreeMap::new();
    for (test_file, f) in &state.results {
        for t in &f.tests {
            for (src, line) in &t.functions {
                if *src == rel {
                    by_line
                        .entry(*line)
                        .or_default()
                        .insert((test_file.clone(), declared(&t.name).to_owned()));
                }
            }
        }
    }
    for (line, tests) in by_line {
        let n = tests.len();
        out.push(click(
            &format!("{n} test{}", if n == 1 { "" } else { "s" }),
            line.saturating_sub(1) as usize,
            LensClick::Callers {
                file: file.clone(),
                line,
            },
        ));
    }
    out
}

/// A test's name without its case (`doubles [1, 2]` is `doubles`'s).
pub fn declared(name: &str) -> &str {
    match name.rfind(" [") {
        Some(i) if name.ends_with(']') => &name[..i],
        _ => name,
    }
}

/// Whether a run's filters take the test `name` of `file` (or the file).
fn selects(filters: &[String], file: &Path, name: Option<&str>) -> bool {
    let path = file.to_string_lossy().replace('\\', "/");
    filters.is_empty()
        || filters.iter().any(|f| match f.split_once("::") {
            Some((p, n)) => path.ends_with(p) && name.is_none_or(|name| name.contains(n)),
            None => path.contains(f.as_str()) || name.is_some_and(|n| n.contains(f.as_str())),
        })
}

/// A request for one file, or one test of it.
pub fn request_for(
    package: &Path,
    file: &Path,
    test: Option<&str>,
    state: &TestsState,
) -> TestRequest {
    let rel = file
        .strip_prefix(package)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/");
    TestRequest {
        filters: vec![match test {
            Some(t) => format!("{rel}::{}", declared(t)),
            None => rel,
        }],
        coverage: state.coverage_on,
        update_snapshots: false,
        profile: state.profile.clone(),
        profiler: false,
    }
}

/// The package a file is in: the deepest member folder over it.
pub fn package_of(workspace: &Workspace, file: &Path) -> PathBuf {
    workspace
        .members
        .iter()
        .filter(|m| file.starts_with(&m.dir))
        .max_by_key(|m| m.dir.components().count())
        .map_or_else(|| workspace.root.clone(), |m| m.dir.clone())
}

// ---------------------------------------------------------------- explorer

/// A row of the explorer.
enum Row {
    Package {
        name: String,
        tests: usize,
        key: String,
    },
    Folder {
        name: String,
        files: usize,
        key: String,
        depth: usize,
    },
    File {
        path: PathBuf,
        name: String,
        depth: usize,
        key: String,
    },
    Test {
        path: PathBuf,
        name: String,
        line: usize,
        depth: usize,
        cases: usize,
        key: String,
        skip: bool,
    },
    Case {
        path: PathBuf,
        name: String,
        depth: usize,
    },
}

/// The test explorer (the board's `tests` pane): returns what to do.
pub fn explorer(ui: &mut Ui, state: &mut TestsState, workspace: &Workspace) -> Option<Action> {
    let p = palette(ui.ctx());
    let mut action = None;
    // The filter and all / failed.
    forms::padded(ui, 10, 8, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let w = ui.available_width() - 104.0;
            forms::text_input(ui, &mut state.filter, w.max(60.0), "filter: path::name");
            if let Some(i) = widgets::seg(
                ui,
                "tests-show",
                &["all", "failed"],
                usize::from(state.only_failed),
                false,
            ) {
                state.only_failed = i == 1;
            }
        });
    });
    if let Some((what, _)) = &state.pinned {
        let mut clear = false;
        forms::padded(ui, 10, 2, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(what)
                        .font(Face::Plex.at(11.5))
                        .color(p.amber_ink),
                );
                clear = forms::text_button(ui, "show all").clicked();
            });
        });
        if clear {
            state.pinned = None;
            state.generation += 1;
        }
    }
    let Some(files) = &state.files else {
        empty(ui, "Looking for the test files…", p);
        return None;
    };
    if files.is_empty() {
        empty(
            ui,
            "No test files. A test file ends in .test.wowlua; `wowlua new` makes one in tests/.",
            p,
        );
        return None;
    }
    // The rows, made again when what they show changed.
    let key: RowsKey = (
        state.generation,
        state.filter.clone(),
        state.only_failed,
        files.len(),
    );
    let rows = match state.rows.take() {
        Some((k, rows)) if k == key => rows,
        _ => rows(state, workspace, files),
    };
    if rows.is_empty() {
        empty(
            ui,
            "No test matches. The filter is path::name: \"ThreatMeter::secret\" shows the tests of files matching ThreatMeter whose names contain secret.",
            p,
        );
        return None;
    }
    egui::ScrollArea::vertical().auto_shrink(false).show_rows(
        ui,
        row_h(),
        rows.len(),
        |ui, range| {
            for row in &rows[range] {
                let (r, resp) = ui
                    .allocate_exact_size(Vec2::new(ui.available_width(), row_h()), Sense::click());
                let selected = match row {
                    Row::File { path, .. } => state.selected == Some((path.clone(), None)),
                    Row::Test { path, name, .. } => {
                        state.selected == Some((path.clone(), Some(name.clone())))
                    }
                    _ => false,
                };
                if selected {
                    ui.painter().rect_filled(r, CornerRadius::ZERO, p.bg_400);
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, CornerRadius::ZERO, p.bg_300);
                }
                let painter = ui.painter().with_clip_rect(r);
                let cy = r.center().y;
                let (depth, twist, status, name, meta, bold) = match row {
                    Row::Package { name, tests, key } => (
                        0,
                        Some(!state.closed.contains(key)),
                        None,
                        name.clone(),
                        format!("{tests} tests"),
                        true,
                    ),
                    Row::Folder {
                        name,
                        files,
                        key,
                        depth,
                    } => (
                        *depth,
                        Some(!state.closed.contains(key)),
                        None,
                        name.clone(),
                        format!("{files} file{}", if *files == 1 { "" } else { "s" }),
                        false,
                    ),
                    Row::File {
                        path,
                        name,
                        depth,
                        key,
                    } => {
                        let meta = state.results.get(path).map_or(String::new(), |f| {
                            format!("{} · {:.1} s", f.tests.len(), f.seconds)
                        });
                        (
                            *depth,
                            Some(!state.closed.contains(key)),
                            Some(state.status_of(path, None)),
                            name.clone(),
                            meta,
                            false,
                        )
                    }
                    Row::Test {
                        path,
                        name,
                        depth,
                        cases,
                        key,
                        skip,
                        ..
                    } => {
                        let st = if *skip && !state.results.contains_key(path) {
                            Status::Skip
                        } else {
                            state.status_of(path, Some(name))
                        };
                        (
                            *depth,
                            (*cases > 0).then(|| state.closed.contains(key)),
                            Some(st),
                            name.clone(),
                            if *cases > 0 {
                                format!("@cases {cases}")
                            } else {
                                String::new()
                            },
                            false,
                        )
                    }
                    Row::Case { path, name, depth } => {
                        let st = state
                            .results
                            .get(path)
                            .and_then(|f| f.tests.iter().find(|t| &t.name == name))
                            .map_or(Status::NotRun, |t| outcome_status(&t.outcome));
                        let case = name.rfind(" [").map_or(name.as_str(), |i| &name[i + 1..]);
                        (
                            *depth,
                            None,
                            Some(st),
                            case.to_owned(),
                            String::new(),
                            false,
                        )
                    }
                };
                let x = r.left() + 10.0 + depth as f32 * 14.0;
                if let Some(open) = twist {
                    painter.text(
                        Pos2::new(x + 4.0, cy),
                        Align2::CENTER_CENTER,
                        if open { "−" } else { "+" },
                        Face::Plex.at(12.0),
                        p.ink_muted,
                    );
                }
                let mut tx = x + 14.0;
                if let Some(st) = status {
                    paint_mark(&painter, Pos2::new(tx + 6.0, cy), st, p);
                    tx += 18.0;
                }
                let meta_w = if meta.is_empty() {
                    0.0
                } else {
                    let g = painter.layout_no_wrap(meta.clone(), Face::Plex.at(11.0), p.ink_muted);
                    let w = g.size().x;
                    painter.galley(
                        Pos2::new(r.right() - 10.0 - w, cy - g.size().y / 2.0),
                        g,
                        p.ink_muted,
                    );
                    w + 16.0
                };
                let face = if bold { Face::PlexSemiBold } else { Face::Plex };
                painter
                    .with_clip_rect(Rect::from_min_max(
                        r.min,
                        Pos2::new(r.right() - meta_w, r.bottom()),
                    ))
                    .text(
                        Pos2::new(tx, cy),
                        Align2::LEFT_CENTER,
                        &name,
                        face.at(12.5),
                        p.ink,
                    );
                let resp = resp.on_hover_text(match row {
                    Row::Case { .. } => "a case of @cases".to_owned(),
                    _ => name.clone(),
                });
                if resp.clicked() {
                    match row {
                        Row::Package { key, .. } | Row::Folder { key, .. } => toggle(state, key),
                        Row::File { path, key, .. } => {
                            if state.selected == Some((path.clone(), None)) {
                                toggle(state, key);
                            }
                            state.selected = Some((path.clone(), None));
                            state.tab = tab_for(state, path, None);
                        }
                        Row::Test {
                            path,
                            name,
                            key,
                            cases,
                            ..
                        } => {
                            if *cases > 0
                                && state.selected == Some((path.clone(), Some(name.clone())))
                            {
                                toggle(state, key);
                            }
                            state.selected = Some((path.clone(), Some(name.clone())));
                            state.tab = tab_for(state, path, Some(name));
                        }
                        Row::Case { path, name, .. } => {
                            state.selected = Some((path.clone(), Some(name.clone())));
                            state.tab = tab_for(state, path, Some(name));
                        }
                    }
                }
                if resp.double_clicked() {
                    match row {
                        Row::File { path, .. } => action = Some(Action::Open(path.clone(), 0)),
                        Row::Test { path, line, .. } => {
                            action = Some(Action::Open(path.clone(), *line))
                        }
                        _ => {}
                    }
                }
            }
        },
    );
    state.rows = Some((key, rows));
    action
}

fn toggle(state: &mut TestsState, key: &str) {
    if !state.closed.remove(key) {
        state.closed.insert(key.to_owned());
    }
    state.generation += 1;
}

fn outcome_status(o: &TestOutcome) -> Status {
    match o {
        TestOutcome::Pass => Status::Pass,
        TestOutcome::Fail { .. } => Status::Fail,
        TestOutcome::Skip(_) => Status::Skip,
    }
}

/// A test's mark: ✓ and ◌ drawn (the fonts have neither), ×, ○, ·.
fn paint_mark(painter: &egui::Painter, at: Pos2, st: Status, p: &Palette) {
    let text = |s: &str, color: Color32| {
        painter.text(
            at,
            Align2::CENTER_CENTER,
            s,
            Face::PlexSemiBold.at(12.0),
            color,
        );
    };
    match st {
        Status::Pass => widgets::check_mark(painter, at, p.success),
        Status::Running => {
            painter.circle_stroke(at, 3.5, Stroke::new(1.25, p.amber_ink));
        }
        Status::Fail => text("×", p.error),
        Status::Skip => text("○", p.ink_muted),
        Status::NotRun => text("·", p.ink_faint),
    }
}

/// The tab that shows what a test's failure is.
fn tab_for(state: &TestsState, file: &Path, name: Option<&str>) -> Tab {
    let message = failure_of(state, file, name).map(|(_, m, _)| m);
    match message {
        Some(m) if m.contains("the screenshot \"") => Tab::Screenshot,
        Some(m) if snapshot_diff(&m).is_some() => Tab::Snapshot,
        _ => Tab::Failure,
    }
}

fn rows(state: &TestsState, workspace: &Workspace, files: &[PathBuf]) -> Vec<Row> {
    let filter = state.filter.trim().to_lowercase();
    let (fpath, fname) = match filter.split_once("::") {
        Some((a, b)) => (a.to_owned(), b.to_owned()),
        None => (String::new(), filter.clone()),
    };
    let packages: Vec<(String, PathBuf)> = if workspace.members.is_empty() {
        vec![(workspace.name.clone(), workspace.root.clone())]
    } else {
        workspace
            .members
            .iter()
            .filter(|m| matches!(m.kind.as_str(), "addon" | "library"))
            .map(|m| (m.name.clone(), m.dir.clone()))
            .collect()
    };
    let mut out = Vec::new();
    for (pname, proot) in &packages {
        let mine: Vec<&PathBuf> = files
            .iter()
            .filter(|f| package_of(workspace, f) == *proot)
            .collect();
        if mine.is_empty() {
            continue;
        }
        let pkey = format!("p:{}", proot.display());
        let tests: usize = mine
            .iter()
            .map(|f| state.discovered.get(*f).map_or(0, Vec::len))
            .sum();
        let at = out.len();
        out.push(Row::Package {
            name: pname.clone(),
            tests,
            key: pkey.clone(),
        });
        if state.closed.contains(&pkey) && filter.is_empty() {
            continue;
        }
        // By folder, relative to the package.
        let mut folders: BTreeMap<String, Vec<&PathBuf>> = BTreeMap::new();
        for f in &mine {
            let rel = f.strip_prefix(proot).unwrap_or(f);
            let dir = rel
                .parent()
                .map(|d| d.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            folders.entry(dir).or_default().push(f);
        }
        let mut any = false;
        for (dir, list) in folders {
            let dkey = format!("d:{}/{dir}", proot.display());
            let folder_at = out.len();
            out.push(Row::Folder {
                name: if dir.is_empty() {
                    ".".into()
                } else {
                    dir.clone()
                },
                files: list.len(),
                key: dkey.clone(),
                depth: 1,
            });
            let mut kept = false;
            if !(state.closed.contains(&dkey) && filter.is_empty()) {
                for f in list {
                    let name = f
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let rel = f
                        .strip_prefix(proot)
                        .unwrap_or(f)
                        .to_string_lossy()
                        .to_lowercase();
                    if !fpath.is_empty() && !rel.contains(&fpath) {
                        continue;
                    }
                    let fkey = format!("f:{}", f.display());
                    let discovered = state.discovered.get(f).cloned().unwrap_or_default();
                    let pinned = state.pinned.as_ref().map(|(_, list)| list);
                    if pinned.is_some_and(|list| !list.iter().any(|(p, _)| p == f)) {
                        continue;
                    }
                    let tests: Vec<&Discovered> = discovered
                        .iter()
                        .filter(|t| {
                            pinned
                                .is_none_or(|list| list.iter().any(|(p, n)| p == f && *n == t.name))
                        })
                        .filter(|t| {
                            fname.is_empty()
                                || t.name.to_lowercase().contains(&fname)
                                || (fpath.is_empty() && rel.contains(&fname))
                        })
                        .filter(|t| {
                            !state.only_failed || state.status_of(f, Some(&t.name)) == Status::Fail
                        })
                        .collect();
                    let file_failed = state.status_of(f, None) == Status::Fail;
                    if (!fname.is_empty() || state.only_failed)
                        && tests.is_empty()
                        && !(state.only_failed && file_failed && fname.is_empty())
                    {
                        continue;
                    }
                    kept = true;
                    out.push(Row::File {
                        path: (*f).clone(),
                        name,
                        depth: 2,
                        key: fkey.clone(),
                    });
                    if state.closed.contains(&fkey) && filter.is_empty() {
                        continue;
                    }
                    for t in tests {
                        let cases: Vec<String> = state
                            .results
                            .get(f.as_path())
                            .map(|r| {
                                r.tests
                                    .iter()
                                    .filter(|c| c.name != t.name && declared(&c.name) == t.name)
                                    .map(|c| c.name.clone())
                                    .collect()
                            })
                            .unwrap_or_default();
                        let tkey = format!("t:{}::{}", f.display(), t.name);
                        out.push(Row::Test {
                            path: (*f).clone(),
                            name: t.name.clone(),
                            line: t.line,
                            depth: 3,
                            cases: cases.len(),
                            key: tkey.clone(),
                            skip: t.skip,
                        });
                        // Cases folded at first: a test's key in
                        // `closed` is an opened one.
                        if !cases.is_empty() && state.closed.contains(&tkey) {
                            for c in cases {
                                out.push(Row::Case {
                                    path: (*f).clone(),
                                    name: c,
                                    depth: 4,
                                });
                            }
                        }
                    }
                }
            }
            if !kept && (!filter.is_empty() || state.only_failed) {
                out.truncate(folder_at);
            } else {
                any = true;
            }
        }
        if !any && (!filter.is_empty() || state.only_failed) {
            out.truncate(at);
        }
    }
    out
}

fn empty(ui: &mut Ui, text: &str, p: &Palette) {
    forms::padded(ui, 12, 12, |ui| {
        forms::paragraph(ui, text, 12.5, 19.0, p.ink_muted);
    });
}

// ----------------------------------------------------------------- results

/// The failure shown for a selection: the test's name, its message and
/// stack (the first failure when nothing is selected).
fn failure_of(
    state: &TestsState,
    file: &Path,
    name: Option<&str>,
) -> Option<(String, String, String)> {
    let f = state.results.get(file)?;
    if let Some(e) = &f.error {
        return Some((f.file.clone(), e.clone(), String::new()));
    }
    f.tests
        .iter()
        .filter(|t| name.is_none_or(|n| t.name == n || declared(&t.name) == n))
        .find_map(|t| match &t.outcome {
            TestOutcome::Fail { message, stack } => {
                Some((t.name.clone(), message.clone(), stack.clone()))
            }
            _ => None,
        })
}

/// The selection, else the first failure: file and test.
fn shown(state: &TestsState) -> Option<(PathBuf, Option<String>)> {
    if let Some(s) = &state.selected
        && state.results.contains_key(&s.0)
    {
        return Some(s.clone());
    }
    state.results.iter().find_map(|(path, f)| {
        if f.error.is_some() {
            return Some((path.clone(), None));
        }
        f.tests
            .iter()
            .find(|t| matches!(t.outcome, TestOutcome::Fail { .. }))
            .map(|t| (path.clone(), Some(t.name.clone())))
    })
}

/// The first failure whose message `is` one of a kind (a snapshot's, a
/// screenshot's): its file, test and message.
fn first_of(state: &TestsState, is: impl Fn(&str) -> bool) -> Option<(PathBuf, String, String)> {
    state.results.iter().find_map(|(path, f)| {
        f.tests.iter().find_map(|t| match &t.outcome {
            TestOutcome::Fail { message, .. } if is(message) => {
                Some((path.clone(), t.name.clone(), message.clone()))
            }
            _ => None,
        })
    })
}

fn is_snapshot(message: &str) -> bool {
    snapshot_diff(message).is_some()
}

fn is_screenshot(message: &str) -> bool {
    message.contains("the screenshot \"")
}

/// A snapshot failure's lines (`  `, `+ `, `- `), when the message is one.
fn snapshot_diff(message: &str) -> Option<Vec<&str>> {
    let lines: Vec<&str> = message
        .lines()
        .filter(|l| l.starts_with("+ ") || l.starts_with("- ") || l.starts_with("  "))
        .collect();
    let changed = lines
        .iter()
        .any(|l| l.starts_with("+ ") || l.starts_with("- "));
    (message.contains("snapshot") && changed).then_some(lines)
}

/// A screenshot failure's images: stored, this run's, the difference.
fn screenshot_paths(message: &str) -> Option<(PathBuf, PathBuf, PathBuf)> {
    let find = |label: &str| {
        message
            .lines()
            .find_map(|l| l.trim_start().strip_prefix(label))
            .map(|p| PathBuf::from(p.trim()))
    };
    Some((find("stored:")?, find("now:")?, find("diff:")?))
}

/// Source places in a stack (`src/x.wowlua:12`), with the function each is
/// in when the line names one.
fn places(stack: &str) -> Vec<(String, String, usize)> {
    let mut out = Vec::new();
    for line in stack.lines() {
        for ext in [".wowlua:", ".lua:"] {
            let Some(at) = line.find(ext) else {
                continue;
            };
            let digits: String = line[at + ext.len()..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            let Ok(n) = digits.parse::<usize>() else {
                continue;
            };
            let start = line[..at]
                .rfind(|c: char| !(c.is_alphanumeric() || "_./@-".contains(c)))
                .map_or(0, |i| i + 1);
            let path = format!("{}{}", &line[start..at], &ext[..ext.len() - 1]);
            if path.starts_with("wowlua/") {
                break;
            }
            let function = line
                .split_once("function '")
                .or_else(|| line.split_once("function <"))
                .and_then(|(_, rest)| rest.split(['\'', '>']).next())
                .unwrap_or("")
                .to_owned();
            out.push((function, path, n));
            break;
        }
    }
    out
}

/// The results panel (the board's main pane): returns what to do.
#[allow(clippy::too_many_arguments)]
pub fn results(
    ui: &mut Ui,
    state: &mut TestsState,
    workspace: &Workspace,
    system: &dyn System,
    profiles: &[String],
) -> Option<Action> {
    let p = palette(ui.ctx());
    let mut action = None;
    let running = state.running.is_some();

    // The toolbar.
    forms::padded(ui, 12, 8, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if button(ui, ButtonKind::Primary, "run all", !running, None).clicked() {
                action = Some(Action::Run(TestRequest {
                    filters: Vec::new(),
                    coverage: state.coverage_on,
                    update_snapshots: false,
                    profile: state.profile.clone(),
                    profiler: false,
                }));
            }
            let failed = state.failed_filters();
            if button(
                ui,
                ButtonKind::Secondary,
                "run failed",
                !running && !failed.is_empty(),
                None,
            )
            .clicked()
            {
                action = Some(Action::Run(TestRequest {
                    filters: failed,
                    coverage: state.coverage_on,
                    update_snapshots: false,
                    profile: state.profile.clone(),
                    profiler: false,
                }));
            }
            // The selected test (or file), profiled (S057).
            let selected = state.selected.clone();
            if button(
                ui,
                ButtonKind::Secondary,
                "profile the selected test",
                !running && selected.is_some(),
                None,
            )
            .clicked()
                && let Some((file, test)) = selected
            {
                action = Some(Action::Profile(file, test));
            }
            forms::checkbox(ui, &mut state.coverage_on, "coverage", None);
            forms::checkbox(ui, &mut state.run_on_save, "run on save", None);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut options: Vec<(&str, bool)> = vec![("test.profile (wowlua.json)", true)];
                options.extend(profiles.iter().map(|p| (p.as_str(), true)));
                let mut chosen = state
                    .profile
                    .as_ref()
                    .and_then(|name| profiles.iter().position(|p| p == name).map(|i| i + 1))
                    .unwrap_or(0);
                if forms::select(ui, "tests-profile", &options, &mut chosen, 230.0) {
                    state.profile = (chosen > 0).then(|| profiles[chosen - 1].clone());
                }
                let flavor = workspace.flavors.first().cloned().unwrap_or_default();
                ui.label(
                    egui::RichText::new(flavor)
                        .font(Face::Plex.at(12.0))
                        .color(p.ink_muted),
                );
            });
        });
    });
    rule(ui, p);

    // The run going on.
    if let Some(r) = &state.running {
        let rect = ui
            .allocate_exact_size(Vec2::new(ui.available_width(), 46.0), Sense::hover())
            .0;
        ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_200);
        let what = if r.planned == 0 {
            r.status.clone()
        } else {
            format!(
                "running {} files · {} done · {}",
                r.planned, r.done, r.current
            )
        };
        let painter = ui.painter().with_clip_rect(rect);
        painter.text(
            Pos2::new(rect.left() + 14.0, rect.top() + 14.0),
            Align2::LEFT_CENTER,
            what,
            Face::Plex.at(12.0),
            p.ink_muted,
        );
        let meter = Rect::from_min_size(
            Pos2::new(rect.left() + 14.0, rect.top() + 30.0),
            Vec2::new(rect.width() - 110.0, 4.0),
        );
        painter.rect_filled(meter, CornerRadius::ZERO, p.bg_400);
        if r.planned > 0 {
            let done = (r.done as f32 / r.planned as f32).clamp(0.0, 1.0);
            let fill =
                Rect::from_min_size(meter.min, Vec2::new(meter.width() * done, meter.height()));
            painter.rect_filled(
                fill,
                CornerRadius::ZERO,
                if r.failed > 0 { p.error } else { p.success },
            );
        }
        painter.text(
            Pos2::new(meter.right(), rect.top() + 14.0),
            Align2::RIGHT_CENTER,
            format!("{} passed · {} failed", r.passed, r.failed),
            Face::Plex.at(12.0),
            p.ink_muted,
        );
        let stop = Rect::from_min_size(
            Pos2::new(rect.right() - 82.0, rect.top() + 10.0),
            Vec2::new(68.0, 26.0),
        );
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(stop));
        if button(&mut child, ButtonKind::Secondary, "stop", true, None).clicked() {
            action = Some(Action::Stop);
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(250));
    }

    // Nothing could run.
    if let Some(error) = state.last.as_ref().and_then(|l| l.error.clone())
        && !running
    {
        forms::padded(ui, 12, 8, |ui| {
            forms::callout(ui, CalloutKind::Error, |ui| {
                forms::paragraph(ui, &format!("no test ran: {error}"), 12.0, 18.0, p.ink);
            });
        });
    }

    // The summary.
    let (passed, failed, skipped, files) = state.counts();
    let last = state.last.clone();
    if last.is_some() || !state.results.is_empty() {
        forms::padded(ui, 16, 8, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 18.0;
                let text = |ui: &mut Ui, s: String, c: Color32| {
                    ui.label(egui::RichText::new(s).font(Face::Plex.at(12.0)).color(c));
                };
                let (r, _) = ui.allocate_exact_size(Vec2::new(12.0, 16.0), Sense::hover());
                widgets::check_mark(ui.painter(), r.center(), p.success);
                ui.add_space(-12.0);
                text(ui, format!("{passed} passed"), p.success);
                text(
                    ui,
                    format!("× {failed} failed"),
                    if failed > 0 { p.error } else { p.ink_muted },
                );
                text(ui, format!("○ {skipped} skipped"), p.ink_muted);
                let flavor = workspace.flavors.first().cloned().unwrap_or_default();
                let seconds = last
                    .as_ref()
                    .map_or(String::new(), |l| format!(" · {:.1} s", l.seconds));
                text(
                    ui,
                    format!("{files} files · {flavor}{seconds}"),
                    p.ink_muted,
                );
                if last.as_ref().is_some_and(|l| l.stopped) {
                    text(ui, "stopped".into(), p.warning);
                }
                let (snaps, shots) = differing(state);
                if snaps + shots > 0 {
                    text(
                        ui,
                        format!("snapshots: {snaps} differ · screenshots: {shots} differ"),
                        p.ink_muted,
                    );
                }
                if let Some(l) = &last
                    && l.coverage
                {
                    let note = match l.below.first() {
                        Some((kind, pct, min)) => {
                            format!("coverage: {kind} {pct:.1}% is below {min}%")
                        }
                        None if l.whole => "coverage: every minimum met".into(),
                        None => "coverage: a part of the suite, no minimum".into(),
                    };
                    if forms::text_button(ui, &note).clicked() {
                        action = Some(Action::ShowCoverage);
                    }
                }
            });
        });
        rule(ui, p);
    }

    // The tabs.
    let target = shown(state);
    let failure = target
        .as_ref()
        .and_then(|(f, n)| failure_of(state, f, n.as_deref()));
    let printed = target
        .as_ref()
        .and_then(|(f, _)| state.results.get(f))
        .map(|f| f.printed.clone())
        .unwrap_or_default();
    // A snapshot's or screenshot's difference: the one shown, else the
    // first of its kind.
    let of_kind = |is: fn(&str) -> bool| match (&target, &failure) {
        (Some((file, _)), Some((name, m, _))) if is(m) => {
            Some((file.clone(), name.clone(), m.clone()))
        }
        _ => first_of(state, is),
    };
    let snap = of_kind(is_snapshot);
    let shot = of_kind(is_screenshot);
    let (snaps, shots) = differing(state);
    let count = |n: usize| if n == 0 { String::new() } else { n.to_string() };
    {
        let tabs = [
            (Tab::Failure, "failure", count(failed)),
            (Tab::Snapshot, "snapshot diff", count(snaps)),
            (Tab::Screenshot, "screenshot", count(shots)),
            (Tab::Output, "output", String::new()),
        ];
        let (bar, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), Sense::hover());
        ui.painter()
            .hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, p.line));
        let mut x = bar.left() + 8.0;
        for (tab, label, count) in tabs {
            let text = if count.is_empty() {
                label.to_owned()
            } else {
                format!("{label}  {count}")
            };
            let g = widgets::galley(ui, &text, Face::PlexMedium.at(12.0), p.ink, 0.0);
            let r = Rect::from_min_size(
                Pos2::new(x, bar.top()),
                Vec2::new(g.size().x + 24.0, bar.height()),
            );
            let resp = ui.interact(
                r,
                egui::Id::unique(("wowlua-studio-tests-tab", label)),
                Sense::click(),
            );
            let on = state.tab == tab;
            let color = if on || resp.hovered() {
                p.ink
            } else {
                p.ink_muted
            };
            ui.painter().galley(r.center() - g.size() / 2.0, g, color);
            if on {
                ui.painter().rect_filled(
                    Rect::from_min_max(Pos2::new(r.left(), r.bottom() - 2.0), r.max),
                    CornerRadius::ZERO,
                    p.amber,
                );
            }
            if resp.clicked() {
                state.tab = tab;
            }
            x = r.right();
        }
    }

    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        forms::padded(ui, 18, 16, |ui| {
            ui.spacing_mut().item_spacing.y = 12.0;
            match state.tab {
                Tab::Failure => match (&target, &failure) {
                    (Some((file, _)), Some((name, message, stack))) => {
                        let file = file.clone();
                        if let Some(a) = failure_view(ui, state, workspace, system, &file, name, message, stack, p) {
                            action = Some(a);
                        }
                    }
                    _ if state.results.is_empty() && !running => {
                        forms::paragraph(
                            ui,
                            "No results yet: run all, or a test from the explorer. The results stay until the next run.",
                            12.5,
                            19.0,
                            p.ink_muted,
                        );
                    }
                    _ if failed == 0 && !running => {
                        forms::callout(ui, CalloutKind::Ok, |ui| {
                            let cov = match &last {
                                Some(l) if l.coverage && l.below.is_empty() && l.whole => " Coverage meets every minimum.",
                                _ => "",
                            };
                            forms::paragraph(
                                ui,
                                &format!("Every test passed: {passed} in {files} files.{cov}"),
                                12.5,
                                19.0,
                                p.ink,
                            );
                        });
                    }
                    _ => {
                        forms::paragraph(ui, "This test passed.", 12.5, 19.0, p.ink_muted);
                    }
                },
                Tab::Snapshot => match &snap {
                    Some((file, name, m)) => {
                        if let Some(a) = snapshot_view(ui, state, workspace, file, Some(name), m, p) {
                            action = Some(a);
                        }
                    }
                    _ => {
                        forms::paragraph(ui, "No snapshot differs.", 12.5, 19.0, p.ink_muted);
                    }
                },
                Tab::Screenshot => match shot.as_ref().and_then(|(_, n, m)| Some((n, m, screenshot_paths(m)?))) {
                    Some((name, message, paths)) => {
                        screenshot_view(ui, state, system, name, message, &paths, p);
                    }
                    _ => {
                        forms::paragraph(ui, "No screenshot differs.", 12.5, 19.0, p.ink_muted);
                    }
                },
                Tab::Output => {
                    if printed.trim().is_empty() {
                        forms::paragraph(ui, "The test file printed nothing.", 12.5, 19.0, p.ink_muted);
                    } else {
                        mono_block(ui, &printed, p);
                    }
                }
            }
        });
    });
    action
}

/// A small button, disabled when not `enabled`.
fn button(
    ui: &mut Ui,
    kind: ButtonKind,
    label: &str,
    enabled: bool,
    _: Option<()>,
) -> egui::Response {
    ui.add_enabled_ui(enabled, |ui| widgets::button(ui, kind, label, true, None))
        .inner
}

fn rule(ui: &mut Ui, p: &Palette) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .hline(r.x_range(), r.center().y, Stroke::new(1.0, p.line));
}

/// How many snapshots and screenshots differ in the results.
fn differing(state: &TestsState) -> (usize, usize) {
    let (mut snaps, mut shots) = (0, 0);
    for f in state.results.values() {
        for t in &f.tests {
            if let TestOutcome::Fail { message, .. } = &t.outcome {
                if message.contains("the screenshot \"") {
                    shots += 1;
                } else if snapshot_diff(message).is_some() {
                    snaps += 1;
                }
            }
        }
    }
    (snaps, shots)
}

fn mono_block(ui: &mut Ui, text: &str, p: &Palette) {
    egui::Frame::NONE
        .fill(p.bg_000)
        .stroke(Stroke::new(1.0, p.line))
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                egui::RichText::new(text.trim_end())
                    .font(Face::Plex.at(12.5))
                    .color(p.ink),
            );
        });
}

/// A failure: the test's name and place, the message, the test's source
/// with the failing line, the stack, and what to do.
#[allow(clippy::too_many_arguments)]
fn failure_view(
    ui: &mut Ui,
    state: &mut TestsState,
    workspace: &Workspace,
    system: &dyn System,
    file: &Path,
    name: &str,
    message: &str,
    stack: &str,
    p: &Palette,
) -> Option<Action> {
    let mut action = None;
    let package = package_of(workspace, file);
    let rel = file
        .strip_prefix(&package)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/");
    let test_line = state
        .discovered
        .get(file)
        .and_then(|d| d.iter().find(|t| t.name == declared(name)))
        .map(|t| t.line);
    let flavor = state
        .results
        .get(file)
        .map(|f| f.flavor.clone())
        .unwrap_or_default();
    let seconds = state
        .results_of(file, name)
        .first()
        .map_or(String::new(), |t| {
            format!(" · {:.0} ms", t.seconds * 1000.0)
        });
    let world = state
        .profile
        .clone()
        .unwrap_or_else(|| "test.profile".into());
    ui.label(
        egui::RichText::new(format!("× {name}"))
            .font(Face::PlexSemiBold.at(13.0))
            .color(p.ink),
    );
    let place = match test_line {
        Some(l) => format!("{rel}:{} · {flavor} · world {world}{seconds}", l + 1),
        None => format!("{rel} · {flavor} · world {world}{seconds}"),
    };
    forms::paragraph(ui, &place, 11.5, 16.0, p.ink_muted);
    // The message, without a place put first; where it happened.
    let mut places = places(&format!("{message}\n{stack}"));
    let at = places.first().cloned();
    forms::callout(ui, CalloutKind::Error, |ui| {
        let text = match &at {
            Some((_, path, n)) => {
                let prefix = format!("{path}:{n}: ");
                format!(
                    "{}\nat {path}:{n}",
                    message.strip_prefix(&prefix).unwrap_or(message)
                )
            }
            None => message.to_owned(),
        };
        forms::paragraph(ui, &text, 12.5, 19.0, p.ink);
    });
    // The test's source around the failing line (its file's).
    let fail_line = at
        .as_ref()
        .filter(|(_, path, _)| rel.ends_with(path.as_str()) || path.ends_with(rel.as_str()))
        .map(|(_, _, n)| n - 1)
        .or(test_line);
    if let Some(line) = fail_line {
        let lines = state
            .sources
            .entry(file.to_path_buf())
            .or_insert_with(|| {
                system
                    .read_file(file)
                    .map(|t| t.lines().map(str::to_owned).collect())
                    .unwrap_or_default()
            })
            .clone();
        let from = test_line.unwrap_or(line.saturating_sub(2)).min(line);
        let to = (line + 2).min(lines.len().saturating_sub(1));
        egui::Frame::NONE
            .fill(p.bg_000)
            .stroke(Stroke::new(1.0, p.line))
            .inner_margin(egui::Margin::symmetric(0, 4))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 0.0;
                for (i, text) in lines.iter().enumerate().take(to + 1).skip(from).take(12) {
                    let (r, _) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 21.0), Sense::hover());
                    if i == line {
                        ui.painter()
                            .rect_filled(r, CornerRadius::ZERO, p.error_soft);
                    }
                    let painter = ui.painter().with_clip_rect(r);
                    painter.text(
                        Pos2::new(r.left() + 52.0, r.center().y),
                        Align2::RIGHT_CENTER,
                        (i + 1).to_string(),
                        Face::Plex.at(12.0),
                        p.ink_muted,
                    );
                    painter.text(
                        Pos2::new(r.left() + 68.0, r.center().y),
                        Align2::LEFT_CENTER,
                        text.replace('\t', "    "),
                        Face::Plex.at(12.5),
                        p.ink,
                    );
                }
            });
    }
    // The stack: a click opens the place.
    places.retain(|(_, path, _)| !path.is_empty());
    if !places.is_empty() {
        ui.label(
            egui::RichText::new("stack")
                .font(Face::Plex.at(11.5))
                .color(p.ink_muted),
        );
        ui.spacing_mut().item_spacing.y = 0.0;
        for (function, path, n) in &places {
            let (r, resp) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h()), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(r, CornerRadius::ZERO, p.bg_300);
            }
            let painter = ui.painter().with_clip_rect(r);
            // (the runner calls each test `fn`: the test's name)
            let in_test = rel.ends_with(path.as_str()) || path.ends_with(rel.as_str());
            let main = if function == "fn" && in_test {
                declared(name).to_owned()
            } else if function.is_empty() {
                path.clone()
            } else {
                function.clone()
            };
            painter.text(
                Pos2::new(r.left() + 8.0, r.center().y),
                Align2::LEFT_CENTER,
                main,
                Face::Plex.at(12.5),
                p.ink,
            );
            painter.text(
                Pos2::new(r.right() - 8.0, r.center().y),
                Align2::RIGHT_CENTER,
                format!("{path}:{n}"),
                Face::Plex.at(11.5),
                p.ink_muted,
            );
            if resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                let target = workspace
                    .members
                    .iter()
                    .map(|m| m.dir.join(path))
                    .chain(std::iter::once(package.join(path)))
                    .find(|p| system.read_file(p).is_ok())
                    .unwrap_or_else(|| package.join(path));
                action = Some(Action::Open(target, n.saturating_sub(1)));
            }
        }
        ui.spacing_mut().item_spacing.y = 12.0;
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let idle = state.running.is_none();
        if button(ui, ButtonKind::Primary, "debug this test", true, None).clicked() {
            action = Some(Action::Debug(file.to_path_buf(), Some(name.to_owned())));
        }
        if button(ui, ButtonKind::Secondary, "run it again", idle, None).clicked() {
            action = Some(Action::Run(request_for(&package, file, Some(name), state)));
        }
        if button(ui, ButtonKind::Tertiary, "profile it", idle, None).clicked() {
            action = Some(Action::Profile(file.to_path_buf(), Some(name.to_owned())));
        }
        if button(ui, ButtonKind::Tertiary, "open the test", true, None).clicked() {
            action = Some(Action::Open(file.to_path_buf(), test_line.unwrap_or(0)));
        }
    });
    action
}

/// A snapshot's difference: the stored lines (`-`) and this run's (`+`);
/// accept runs the test again writing its snapshots anew.
fn snapshot_view(
    ui: &mut Ui,
    state: &TestsState,
    workspace: &Workspace,
    file: &Path,
    name: Option<&str>,
    message: &str,
    p: &Palette,
) -> Option<Action> {
    let mut action = None;
    let lines = snapshot_diff(message).unwrap_or_default();
    let head = message.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    ui.label(
        egui::RichText::new(format!("× {}", name.unwrap_or("")))
            .font(Face::PlexSemiBold.at(13.0))
            .color(p.ink),
    );
    forms::paragraph(ui, head, 11.5, 16.0, p.ink_muted);
    egui::Frame::NONE
        .fill(p.bg_000)
        .stroke(Stroke::new(1.0, p.line))
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            for l in &lines {
                let color = if l.starts_with("+ ") {
                    p.success
                } else if l.starts_with("- ") {
                    p.error
                } else {
                    p.ink
                };
                // A text's lines as the value quotes them: `"…\`.
                let (sign, text) = l.split_at(2.min(l.len()));
                let text = text
                    .trim_start_matches('"')
                    .trim_end_matches('\\')
                    .trim_end_matches('"');
                ui.label(
                    egui::RichText::new(format!("{sign}{text}"))
                        .font(Face::Plex.at(12.5))
                        .color(color),
                );
            }
        });
    forms::paragraph(
        ui,
        "- the stored snapshot · + this run. A snapshot is a value's text; the file keeps each one after a # key line.",
        11.5,
        16.0,
        p.ink_muted,
    );
    let package = package_of(workspace, file);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let idle = state.running.is_none();
        if button(ui, ButtonKind::Primary, "accept this run's", idle, None).clicked() {
            let mut r = request_for(&package, file, name, state);
            r.update_snapshots = true;
            action = Some(Action::Run(r));
        }
        if button(
            ui,
            ButtonKind::Tertiary,
            "accept every changed snapshot (wowlua test -u)",
            idle,
            None,
        )
        .clicked()
        {
            action = Some(Action::Run(TestRequest {
                filters: Vec::new(),
                coverage: state.coverage_on,
                update_snapshots: true,
                profile: state.profile.clone(),
                profiler: false,
            }));
        }
    });
    action
}

/// A screenshot's three images: stored, this run's, the difference;
/// accept copies this run's over the stored one.
fn screenshot_view(
    ui: &mut Ui,
    state: &mut TestsState,
    system: &dyn System,
    name: &str,
    message: &str,
    (stored, now, diff): &(PathBuf, PathBuf, PathBuf),
    p: &Palette,
) {
    ui.label(
        egui::RichText::new(format!("× {name}"))
            .font(Face::PlexSemiBold.at(13.0))
            .color(p.ink),
    );
    forms::paragraph(
        ui,
        message.lines().next().unwrap_or(""),
        11.5,
        16.0,
        p.ink_muted,
    );
    let width = ((ui.available_width() - 24.0) / 3.0).max(80.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        for (label, path) in [
            ("stored", stored),
            ("this run", now),
            ("diff · red: pixels that differ", diff),
        ] {
            ui.vertical(|ui| {
                ui.set_width(width);
                let texture = state
                    .images
                    .entry(path.clone())
                    .or_insert_with(|| {
                        system.read_image(path).map(|img| {
                            let image = egui::ColorImage::from_rgba_unmultiplied(
                                [img.width as usize, img.height as usize],
                                &img.rgba,
                            );
                            ui.ctx().load_texture(
                                path.to_string_lossy(),
                                image,
                                egui::TextureOptions::NEAREST,
                            )
                        })
                    })
                    .clone();
                match texture {
                    Some(t) => {
                        let size = t.size_vec2();
                        // The column's width, small images up to four times, their pixels sharp.
                        let scale = (width / size.x).min(4.0);
                        ui.image((t.id(), size * scale));
                    }
                    None => {
                        forms::paragraph(ui, "not found", 11.5, 16.0, p.ink_muted);
                    }
                }
                forms::paragraph(ui, label, 11.5, 16.0, p.ink_muted);
            });
        }
    });
    forms::paragraph(
        ui,
        "Drawn with your installation's art and fonts, by the same renderer as the game page.",
        11.5,
        16.0,
        p.ink_muted,
    );
    if button(
        ui,
        ButtonKind::Primary,
        "accept this run's image",
        state.running.is_none(),
        None,
    )
    .clicked()
        && system.copy_file(now, stored).is_ok()
    {
        state.images.remove(stored);
    }
}

#[cfg(test)]
mod unit {
    use super::*;
    use crate::system::{CoverageFile, TestCase};

    fn workspace() -> Workspace {
        Workspace {
            root: "/p/demo".into(),
            name: "demo".into(),
            members: Vec::new(),
            flavors: vec!["retail".into()],
            cannot_check: None,
        }
    }

    fn case(name: &str, outcome: TestOutcome, functions: &[(&str, u32)]) -> TestCase {
        TestCase {
            name: name.into(),
            outcome,
            seconds: 0.0,
            functions: functions.iter().map(|(s, l)| ((*s).into(), *l)).collect(),
        }
    }

    fn failed(message: &str) -> TestOutcome {
        TestOutcome::Fail {
            message: message.into(),
            stack: "tests/a.test.wowlua:7: in function 'fn'".into(),
        }
    }

    fn file(tests: Vec<TestCase>) -> TestFile {
        TestFile {
            package: "/p/demo".into(),
            file: "tests/a.test.wowlua".into(),
            flavor: "retail".into(),
            tests,
            error: None,
            printed: String::new(),
            seconds: 0.1,
        }
    }

    fn discovered(name: &str, line: usize) -> Discovered {
        Discovered {
            name: name.into(),
            line,
            start: line - 1,
            skip: false,
            cases: false,
        }
    }

    const A: &str = "/p/demo/tests/a.test.wowlua";

    #[test]
    fn a_case_is_its_tests() {
        assert_eq!(declared("doubles [1, 2]"), "doubles");
        assert_eq!(declared("doubles"), "doubles");
        assert_eq!(declared("[a] b"), "[a] b");
    }

    #[test]
    fn filters_select_files_and_tests() {
        let a = Path::new(A);
        assert!(selects(&[], a, None));
        assert!(selects(
            &["tests/a.test.wowlua::adds".into()],
            a,
            Some("adds")
        ));
        assert!(!selects(
            &["tests/a.test.wowlua::adds".into()],
            a,
            Some("draws")
        ));
        assert!(selects(&["a.test".into()], a, Some("draws")));
        assert!(!selects(&["b.test".into()], a, None));
    }

    /// A whole run, then one of its failures again: the second's results
    /// go over the first's, and a run already over is not heard.
    #[test]
    fn a_filtered_run_goes_over_the_last() {
        let mut state = TestsState::default();
        let run = state.next_run();
        state.begin(run, TestRequest::default(), 1);
        state.event(run, TestEvent::Planned(1));
        state.event(
            run,
            TestEvent::File(file(vec![
                case("adds", TestOutcome::Pass, &[]),
                case("draws [1]", failed("boom"), &[]),
            ])),
        );
        assert_eq!(
            state.running.as_ref().map(|r| (r.done, r.failed)),
            Some((1, 1))
        );
        assert!(
            state
                .event(run, TestEvent::Done(TestDone::default()))
                .is_some()
        );
        assert_eq!(state.counts(), (1, 1, 0, 1));
        assert_eq!(state.failed_filters(), ["tests/a.test.wowlua::draws"]);

        let again = state.next_run();
        let request = TestRequest {
            filters: state.failed_filters(),
            ..TestRequest::default()
        };
        state.begin(again, request, 2);
        assert_eq!(state.counts(), (1, 1, 0, 1), "kept while it runs");
        state.event(
            again,
            TestEvent::File(file(vec![case("draws [1]", TestOutcome::Pass, &[])])),
        );
        assert!(state.event(run, TestEvent::Status("old".into())).is_none());
        state.event(again, TestEvent::Done(TestDone::default()));
        assert_eq!(state.counts(), (2, 0, 0, 1));
        assert_eq!(state.last.as_ref().map(|l| l.whole), Some(false));
    }

    #[test]
    fn failures_are_read_from_their_messages() {
        let snapshot =
            "the snapshot \"rows\" differs (- stored, + now):\n  \"a\"\n- \"b\"\n+ \"c\"";
        assert_eq!(
            snapshot_diff(snapshot),
            Some(vec!["  \"a\"", "- \"b\"", "+ \"c\""])
        );
        assert_eq!(snapshot_diff("expected 1, got 2\n- nothing"), None);
        let screenshot = "the screenshot \"box\" differs: 3% of its pixels\n  stored: /s/box.png\n  now:    /s/box.actual.png\n  diff:   /s/box.diff.png";
        assert!(is_screenshot(screenshot));
        assert_eq!(
            screenshot_paths(screenshot),
            Some((
                "/s/box.png".into(),
                "/s/box.actual.png".into(),
                "/s/box.diff.png".into()
            ))
        );
        let stack = "src/tracker.wowlua:12: in function 'double'\ntests/a.test.wowlua:7: in function 'fn'\nwowlua/test.lua:40: in function 'run'\nsrc/never.lua:1:";
        assert_eq!(
            places(stack),
            [
                ("double".into(), "src/tracker.wowlua".into(), 12),
                ("fn".into(), "tests/a.test.wowlua".into(), 7),
                (String::new(), "src/never.lua".into(), 1),
            ]
        );
    }

    #[test]
    fn lenses_tell_each_tests_result_and_who_calls_a_function() {
        let mut state = TestsState::default();
        state.discovered(
            A.into(),
            vec![
                discovered("adds", 3),
                discovered("draws", 6),
                discovered("new", 9),
            ],
        );
        state.restore(Kept {
            results: vec![file(vec![
                case("adds", TestOutcome::Pass, &[("src/math.wowlua", 1)]),
                case(
                    "draws",
                    failed("boom"),
                    &[("src/math.wowlua", 1), ("src/ui.wowlua", 4)],
                ),
            ])],
            last: None,
        });
        let titles = |path: &str| -> Vec<(usize, String)> {
            lenses(&state, &workspace(), Path::new(path))
                .into_iter()
                .map(|l| (l.line, l.title))
                .collect()
        };
        let line = |n: usize, t: &str| (n, t.to_owned());
        assert_eq!(
            titles(A),
            [
                line(0, "run all tests"),
                line(0, "debug"),
                line(0, "profile"),
                line(0, "3 tests"),
                line(2, "passed"),
                line(2, "run test"),
                line(2, "debug"),
                line(2, "profile"),
                line(5, "failed at line 7"),
                line(5, "show failure"),
                line(5, "run test"),
                line(5, "debug"),
                line(5, "profile"),
                line(8, "run test"),
                line(8, "debug"),
                line(8, "profile"),
            ]
        );
        assert_eq!(titles("/p/demo/src/math.wowlua"), [line(0, "2 tests")]);
        assert_eq!(titles("/p/demo/src/ui.wowlua"), [line(3, "1 test")]);
    }

    /// A function's lens pins the explorer to the tests calling it.
    #[test]
    fn pinned_callers_are_all_the_explorer_shows() {
        let b = "/p/demo/tests/b.test.wowlua";
        let mut state = TestsState::default();
        let files: Vec<PathBuf> = vec![A.into(), b.into()];
        state.discovered(
            A.into(),
            vec![discovered("adds", 3), discovered("draws", 6)],
        );
        state.discovered(b.into(), vec![discovered("other", 2)]);
        state.restore(Kept {
            results: vec![file(vec![
                case("adds", TestOutcome::Pass, &[]),
                case("draws [1]", TestOutcome::Pass, &[("src/ui.wowlua", 4)]),
                case("draws [2]", TestOutcome::Pass, &[("src/ui.wowlua", 4)]),
            ])],
            last: None,
        });
        let mut coverage = crate::system::Coverage::default();
        let mut ui = CoverageFile::default();
        ui.functions.insert(4, ("draw".into(), 2));
        coverage.files.insert("/p/demo/src/ui.wowlua".into(), ui);
        state.coverage = Some(coverage);
        state.pin_callers(&workspace(), Path::new("/p/demo/src/ui.wowlua"), 4);
        let (what, list) = state.pinned.clone().unwrap_or_default();
        assert_eq!(what, "the tests that call draw");
        assert_eq!(list, [(PathBuf::from(A), "draws".to_owned())]);
        let names: Vec<String> = rows(&state, &workspace(), &files)
            .into_iter()
            .filter_map(|r| match r {
                Row::File { name, .. } | Row::Test { name, .. } => Some(name),
                _ => None,
            })
            .collect();
        assert_eq!(names, ["a.test.wowlua", "draws"]);
    }
}
