//! The coverage panel (the coverage board, S052): the last run with
//! coverage's totals against `wowlua.json`'s minimums, the files (where to
//! test next, or every file), and a file's source with each line's hits
//! and each branch's ways. The editor's gutter can show the same.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use egui::{Align2, CornerRadius, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use wowlua_studio_ui::forms::{self, CalloutKind};
use wowlua_studio_ui::widgets::{self, ButtonKind};
use wowlua_studio_ui::{Face, Palette, palette};

use crate::system::{Count, Coverage, CoverageFile, System, TestRequest, Workspace};
use crate::tests::{Action, TestsState};

const ROW_H: f32 = 24.0;

/// The files' list: where to test next (what is not run, most first), or
/// every file, sorted by a column.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Files {
    #[default]
    Next,
    All,
}

/// A column of the files' list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Column {
    #[default]
    Path,
    Lines,
    Branches,
    Functions,
    Conditions,
    NotRun,
}

/// The coverage panel's state.
#[derive(Default)]
pub struct CoverageView {
    /// The editor's gutter shows the coverage.
    pub in_gutter: bool,
    pub files: Files,
    /// Only the files not fully covered.
    pub partial: bool,
    pub sort: Column,
    pub descending: bool,
    /// The file whose source is shown.
    pub file: Option<PathBuf>,
    /// The line not run gone to last (from 1), and whether to scroll to it.
    at: Option<u32>,
    jump: bool,
    sources: HashMap<PathBuf, Vec<String>>,
    /// `test.coverage`'s minimums, read once a run.
    pub minimums: Option<HashMap<&'static str, f64>>,
}

fn percent((covered, total): Count) -> f64 {
    if total == 0 {
        100.0
    } else {
        covered as f64 * 100.0 / total as f64
    }
}

fn pct(c: Count) -> String {
    let p = percent(c);
    if p >= 100.0 {
        "100%".into()
    } else {
        format!("{p:.1}%")
    }
}

/// A file's lines not run.
fn not_run(f: &CoverageFile) -> Vec<u32> {
    f.lines
        .iter()
        .filter(|(_, h)| **h == 0)
        .map(|(l, _)| *l)
        .collect()
}

/// The coverage panel; returns what to do.
pub fn show(
    ui: &mut Ui,
    state: &mut TestsState,
    workspace: &Workspace,
    system: &dyn System,
) -> Option<Action> {
    let p = palette(ui.ctx());
    let mut action = None;
    // (taken while drawn, put back after: not copied each frame)
    let Some(coverage) = state.coverage.take() else {
        forms::padded(ui, 16, 16, |ui| {
            ui.spacing_mut().item_spacing.y = 10.0;
            ui.label(
                egui::RichText::new("No coverage yet")
                    .font(Face::PlexSemiBold.at(13.0))
                    .color(p.ink),
            );
            forms::paragraph(
                ui,
                "Run the tests with coverage on: Studio keeps the last run's counts per file and shows them here and in the editor's gutter.",
                12.5,
                19.0,
                p.ink_muted,
            );
            if widgets::button(ui, ButtonKind::Primary, "run all with coverage", true, None)
                .clicked()
            {
                action = Some(Action::Run(TestRequest {
                    filters: Vec::new(),
                    coverage: true,
                    update_snapshots: false,
                    profile: state.profile.clone(),
                    profiler: false,
                }));
            }
        });
        return action;
    };
    let last = state.last.clone().filter(|l| l.coverage);
    let whole = last.as_ref().is_none_or(|l| l.whole);

    // The bar: which run, the gutter, the report.
    forms::padded(ui, 12, 6, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            ui.label(
                egui::RichText::new("coverage")
                    .font(Face::PlexSemiBold.at(12.0))
                    .color(p.ink),
            );
            let what = match &last {
                Some(l) if l.whole => "the last run with coverage: the whole suite",
                Some(_) => "the last run with coverage: a part of the suite",
                None => "the last run with coverage (dist/test/coverage.json)",
            };
            ui.label(
                egui::RichText::new(what)
                    .font(Face::Plex.at(11.5))
                    .color(p.ink_muted),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(html) = &coverage.html
                    && forms::text_button(ui, "open the html report").clicked()
                {
                    ui.ctx()
                        .open_url(egui::OpenUrl::new_tab(format!("file://{}", html.display())));
                }
                forms::checkbox(
                    ui,
                    &mut state.cov.in_gutter,
                    "show in the editor's gutter",
                    None,
                );
            });
        });
    });
    rule(ui, p);
    if !whole {
        forms::padded(ui, 12, 8, |ui| {
            forms::callout(ui, CalloutKind::Note, |ui| {
                forms::paragraph(
                    ui,
                    "A run with a filter checks part of the suite while its totals still count every source file, so no minimum applies to it.",
                    12.0,
                    18.0,
                    p.ink,
                );
            });
        });
    }

    // The totals against the minimums.
    let minimums = &last.as_ref().map(|l| l.below.clone()).unwrap_or_default();
    let mins = state
        .cov
        .minimums
        .get_or_insert_with(|| minimums_of(workspace, system))
        .clone();
    let kinds = [
        ("lines", coverage.totals.lines),
        ("branches", coverage.totals.branches),
        ("functions", coverage.totals.functions),
        ("conditions", coverage.totals.conditions),
    ];
    let w = ((ui.available_width() - 24.0 - 3.0 * 10.0) / 4.0).max(120.0);
    forms::padded(ui, 12, 10, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            for (kind, count) in kinds {
                let min = mins.get(kind).copied();
                let below = minimums.iter().any(|(k, _, _)| k == kind);
                total_card(ui, w, kind, count, min, whole, below, p);
            }
        });
    });
    rule(ui, p);

    // The files, and the source of the one chosen.
    let avail = ui.available_height();
    let list_h = (avail * 0.42).clamp(140.0, 360.0);
    let mut list = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(
        ui.cursor().min,
        Vec2::new(ui.available_width(), list_h),
    )));
    if let Some(path) = files_list(&mut list, state, &coverage, workspace, p) {
        state.cov.file = Some(path);
    }
    ui.allocate_exact_size(Vec2::new(ui.available_width(), list_h), Sense::hover());
    rule(ui, p);
    let chosen = state.cov.file.clone().or_else(|| {
        // The file with the most not run.
        coverage
            .files
            .iter()
            .max_by_key(|(_, f)| not_run(f).len())
            .map(|(p, _)| p.clone())
    });
    if let Some(path) = chosen
        && let Some(file) = coverage.files.get(&path)
        && let Some(a) = source(ui, state, workspace, system, &path, file, p)
    {
        action = Some(a);
    }
    state.coverage = Some(coverage);
    action
}

/// The minimums `test.coverage` sets, by kind (`statements` are lines).
fn minimums_of(workspace: &Workspace, system: &dyn System) -> HashMap<&'static str, f64> {
    let mut out = HashMap::new();
    let Ok(text) = system.read_file(&workspace.root.join("wowlua.json")) else {
        return out;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return out;
    };
    let c = &json["test"]["coverage"];
    for kind in ["lines", "branches", "functions", "conditions"] {
        if let Some(v) = c[kind].as_f64() {
            out.insert(kind, v);
        }
    }
    if let Some(v) = c["statements"].as_f64() {
        out.entry("lines").or_insert(v);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn total_card(
    ui: &mut Ui,
    w: f32,
    kind: &str,
    count: Count,
    min: Option<f64>,
    whole: bool,
    below: bool,
    p: &Palette,
) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(w, 86.0), Sense::hover());
    let painter = ui.painter().with_clip_rect(r);
    painter.rect_filled(r, CornerRadius::ZERO, p.bg_200);
    painter.rect_stroke(
        r,
        CornerRadius::ZERO,
        Stroke::new(1.0, p.line),
        egui::StrokeKind::Inside,
    );
    let x = r.left() + 14.0;
    painter.text(
        Pos2::new(x, r.top() + 16.0),
        Align2::LEFT_CENTER,
        kind,
        Face::Plex.at(11.5),
        p.ink_muted,
    );
    let met = min.is_none_or(|m| percent(count) >= m);
    let color = if !whole || min.is_none() {
        p.ink
    } else if met && !below {
        p.success
    } else {
        p.error
    };
    painter.text(
        Pos2::new(x, r.top() + 40.0),
        Align2::LEFT_CENTER,
        pct(count),
        Face::PlexSemiBold.at(22.0),
        color,
    );
    let verdict = match min {
        None => String::new(),
        Some(_) if !whole => " · not checked".into(),
        Some(_) if met => " · met".into(),
        Some(_) => " · below: the run fails".into(),
    };
    let detail = match min {
        Some(m) => format!("{} of {} · minimum {m}%{verdict}", count.0, count.1),
        None => format!("{} of {}", count.0, count.1),
    };
    painter.text(
        Pos2::new(x, r.top() + 62.0),
        Align2::LEFT_CENTER,
        detail,
        Face::Plex.at(11.0),
        p.ink_muted,
    );
    // The meter, the minimum marked.
    let meter = Rect::from_min_size(
        Pos2::new(x, r.bottom() - 12.0),
        Vec2::new(r.width() - 28.0, 3.0),
    );
    painter.rect_filled(meter, CornerRadius::ZERO, p.bg_400);
    let fill = Rect::from_min_size(
        meter.min,
        Vec2::new(meter.width() * (percent(count) / 100.0) as f32, 3.0),
    );
    painter.rect_filled(
        fill,
        CornerRadius::ZERO,
        if whole && min.is_some() && !met {
            p.error
        } else {
            p.success
        },
    );
    if let Some(m) = min {
        let mx = meter.left() + meter.width() * (m / 100.0) as f32;
        painter.vline(
            mx,
            (meter.top() - 3.0)..=(meter.bottom() + 3.0),
            Stroke::new(2.0, p.ink),
        );
    }
}

fn rule(ui: &mut Ui, p: &Palette) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .hline(r.x_range(), r.center().y, Stroke::new(1.0, p.line));
}

/// The files' list: returns the one clicked.
fn files_list(
    ui: &mut Ui,
    state: &mut TestsState,
    coverage: &Coverage,
    workspace: &Workspace,
    p: &Palette,
) -> Option<PathBuf> {
    let mut picked = None;
    forms::padded(ui, 12, 6, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.label(
                egui::RichText::new("files")
                    .font(Face::PlexSemiBold.at(12.0))
                    .color(p.ink),
            );
            let i = usize::from(state.cov.files == Files::All);
            if let Some(i) = widgets::seg(
                ui,
                "coverage-files",
                &["where to test next", "every file"],
                i,
                false,
            ) {
                state.cov.files = if i == 0 { Files::Next } else { Files::All };
            }
            if state.cov.files == Files::All {
                forms::checkbox(ui, &mut state.cov.partial, "not fully covered", None);
            }
        });
    });
    let rel = |path: &Path| -> String {
        let package = crate::tests::package_of(workspace, path);
        path.strip_prefix(&package)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    };
    let mut rows: Vec<(&PathBuf, &CoverageFile, usize)> = coverage
        .files
        .iter()
        .map(|(p, f)| (p, f, not_run(f).len()))
        .collect();
    match state.cov.files {
        Files::Next => {
            rows.retain(|(_, f, n)| *n > 0 || f.functions.values().any(|(_, c)| *c == 0));
            rows.sort_by_key(|r| std::cmp::Reverse(r.2));
        }
        Files::All => {
            if state.cov.partial {
                rows.retain(|(_, f, n)| *n > 0 || f.totals.branches.0 < f.totals.branches.1);
            }
            let key = |r: &(&PathBuf, &CoverageFile, usize)| -> f64 {
                match state.cov.sort {
                    Column::Path => 0.0,
                    Column::Lines => percent(r.1.totals.lines),
                    Column::Branches => percent(r.1.totals.branches),
                    Column::Functions => percent(r.1.totals.functions),
                    Column::Conditions => percent(r.1.totals.conditions),
                    Column::NotRun => r.2 as f64,
                }
            };
            rows.sort_by(|a, b| {
                let o = if state.cov.sort == Column::Path {
                    a.0.cmp(b.0)
                } else {
                    key(a).total_cmp(&key(b))
                };
                if state.cov.descending { o.reverse() } else { o }
            });
        }
    }
    // The head: sortable columns.
    let w = ui.available_width();
    let cols = [
        (Column::Path, "file"),
        (Column::Lines, "lines"),
        (Column::Branches, "branches"),
        (Column::Functions, "functions"),
        (Column::Conditions, "conditions"),
        (Column::NotRun, "not run"),
    ];
    let col_x = |i: usize| -> f32 {
        if i == 0 {
            12.0
        } else {
            w - 12.0 - (6 - i) as f32 * 90.0
        }
    };
    let (head, _) = ui.allocate_exact_size(Vec2::new(w, 22.0), Sense::hover());
    for (i, (col, label)) in cols.iter().enumerate() {
        let arrow = if state.cov.files == Files::All && state.cov.sort == *col {
            if state.cov.descending { " ↓" } else { " ↑" }
        } else {
            ""
        };
        let at = Pos2::new(
            head.left() + col_x(i) + if i == 0 { 0.0 } else { 78.0 },
            head.center().y,
        );
        let align = if i == 0 {
            Align2::LEFT_CENTER
        } else {
            Align2::RIGHT_CENTER
        };
        let r = ui.painter().text(
            at,
            align,
            format!("{label}{arrow}"),
            Face::Plex.at(11.0),
            p.ink_muted,
        );
        let resp = ui.interact(
            r.expand(3.0),
            egui::Id::unique(("wowlua-studio-coverage-col", i)),
            Sense::click(),
        );
        if resp
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            state.cov.descending =
                state.cov.files == Files::All && state.cov.sort == *col && !state.cov.descending;
            state.cov.files = Files::All;
            state.cov.sort = *col;
        }
    }
    if rows.is_empty() {
        forms::padded(ui, 12, 8, |ui| {
            forms::paragraph(ui, "Every line and branch ran.", 12.5, 19.0, p.ink_muted);
        });
        return None;
    }
    egui::ScrollArea::vertical()
        .id_salt("wowlua-studio-coverage-files")
        .auto_shrink(false)
        .show_rows(ui, ROW_H, rows.len(), |ui, range| {
            for (path, f, missed) in &rows[range] {
                let (r, resp) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
                if state.cov.file.as_deref() == Some(path.as_path()) {
                    ui.painter().rect_filled(r, CornerRadius::ZERO, p.bg_400);
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, CornerRadius::ZERO, p.bg_300);
                }
                let painter = ui.painter().with_clip_rect(r);
                let cy = r.center().y;
                painter.text(
                    Pos2::new(r.left() + 12.0, cy),
                    Align2::LEFT_CENTER,
                    rel(path),
                    Face::Plex.at(12.5),
                    p.ink,
                );
                let never = f.functions.values().filter(|(_, c)| *c == 0).count();
                let cells = [
                    (pct(f.totals.lines), p.ink),
                    (pct(f.totals.branches), p.ink),
                    if state.cov.files == Files::Next {
                        (
                            if never > 0 {
                                format!("{never} never called")
                            } else {
                                "—".into()
                            },
                            p.ink_muted,
                        )
                    } else {
                        (pct(f.totals.functions), p.ink)
                    },
                    (pct(f.totals.conditions), p.ink),
                    (
                        if *missed > 0 {
                            missed.to_string()
                        } else {
                            String::new()
                        },
                        p.error,
                    ),
                ];
                for (i, (text, color)) in cells.into_iter().enumerate() {
                    painter.text(
                        Pos2::new(r.left() + col_x(i + 1) + 78.0, cy),
                        Align2::RIGHT_CENTER,
                        text,
                        Face::Plex.at(12.0),
                        color,
                    );
                }
                if resp.clicked() {
                    picked = Some((*path).clone());
                }
            }
        });
    picked
}

/// A file's source with each line's hits and each branch's ways; its
/// functions never called first.
#[allow(clippy::too_many_arguments)]
fn source(
    ui: &mut Ui,
    state: &mut TestsState,
    workspace: &Workspace,
    system: &dyn System,
    path: &Path,
    file: &CoverageFile,
    p: &Palette,
) -> Option<Action> {
    let mut action = None;
    let package = crate::tests::package_of(workspace, path);
    let rel = path
        .strip_prefix(&package)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    let gaps = not_run(file);
    forms::padded(ui, 12, 6, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.label(
                egui::RichText::new(&rel)
                    .font(Face::PlexSemiBold.at(12.5))
                    .color(p.ink),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if forms::text_button(ui, "open in the editor").clicked() {
                    action = Some(Action::Open(
                        path.to_path_buf(),
                        gaps.first().map_or(0, |l| *l as usize - 1),
                    ));
                }
                if forms::text_button(ui, "↓ next not run").clicked() {
                    let after = state.cov.at.unwrap_or(0);
                    state.cov.at = gaps.iter().find(|l| **l > after).or(gaps.first()).copied();
                    state.cov.jump = true;
                }
                if forms::text_button(ui, "↑ previous not run").clicked() {
                    let before = state.cov.at.unwrap_or(u32::MAX);
                    state.cov.at = gaps
                        .iter()
                        .rev()
                        .find(|l| **l < before)
                        .or(gaps.last())
                        .copied();
                    state.cov.jump = true;
                }
            });
        });
        let t = &file.totals;
        ui.label(
            egui::RichText::new(format!(
                "lines {} · branches {} · functions {} · conditions {} · not run: {}",
                pct(t.lines),
                pct(t.branches),
                pct(t.functions),
                pct(t.conditions),
                gaps.len()
            ))
            .font(Face::Plex.at(11.5))
            .color(p.ink_muted),
        );
    });
    // Functions never called, first.
    let never: Vec<(&u32, &(String, u64))> = file
        .functions
        .iter()
        .filter(|(_, (_, c))| *c == 0)
        .collect();
    if !never.is_empty() {
        forms::padded(ui, 12, 2, |ui| {
            let names: Vec<String> = never
                .iter()
                .map(|(l, (n, _))| format!("{n} (line {l})"))
                .collect();
            forms::paragraph(
                ui,
                &format!("never called: {}", names.join(", ")),
                11.5,
                17.0,
                p.error,
            );
        });
    }
    let lines = state
        .cov
        .sources
        .entry(path.to_path_buf())
        .or_insert_with(|| {
            system
                .read_file(path)
                .map(|t| t.lines().map(str::to_owned).collect())
                .unwrap_or_default()
        })
        .clone();
    let mut area = egui::ScrollArea::vertical()
        .id_salt(("wowlua-studio-coverage-source", path))
        .auto_shrink(false);
    if std::mem::take(&mut state.cov.jump)
        && let Some(line) = state.cov.at
    {
        area = area.vertical_scroll_offset(((line as f32 - 1.0) * 21.0 - 84.0).max(0.0));
    }
    area.show_rows(ui, 21.0, lines.len(), |ui, range| {
        for i in range {
            let n = i as u32 + 1;
            let (r, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 21.0), Sense::hover());
            let hits = file.lines.get(&n).copied();
            if hits == Some(0) {
                ui.painter()
                    .rect_filled(r, CornerRadius::ZERO, p.error_soft);
            }
            let painter = ui.painter().with_clip_rect(r);
            let cy = r.center().y;
            if let Some(h) = hits {
                painter.text(
                    Pos2::new(r.left() + 46.0, cy),
                    Align2::RIGHT_CENTER,
                    h.to_string(),
                    if h == 0 {
                        Face::PlexSemiBold.at(11.5)
                    } else {
                        Face::Plex.at(11.5)
                    },
                    if h == 0 { p.error } else { p.ink_muted },
                );
            }
            painter.text(
                Pos2::new(r.left() + 88.0, cy),
                Align2::RIGHT_CENTER,
                n.to_string(),
                Face::Plex.at(11.5),
                p.ink_faint,
            );
            let text = lines[i].replace('\t', "    ");
            let g = painter.layout_no_wrap(
                text,
                Face::Plex.at(12.5),
                if hits.is_none() { p.ink_muted } else { p.ink },
            );
            let gx = r.left() + 100.0;
            let gw = g.size().x;
            painter.galley(Pos2::new(gx, cy - g.size().y / 2.0), g, p.ink);
            // The line's branches: both ways, or one way only.
            let mut x = gx + gw + 10.0;
            for ((_, _), [taken, not]) in file.branches.range((n, 0)..(n + 1, 0)) {
                if *taken == 0 && *not == 0 {
                    continue;
                }
                let (label, color) = if *taken > 0 && *not > 0 {
                    ("both ways", p.ink_muted)
                } else {
                    ("one way only", p.warning)
                };
                let cr = painter.text(
                    Pos2::new(x + 5.0, cy),
                    Align2::LEFT_CENTER,
                    label,
                    Face::Plex.at(10.5),
                    color,
                );
                painter.rect_stroke(
                    cr.expand2(Vec2::new(5.0, 1.0)),
                    CornerRadius::ZERO,
                    Stroke::new(
                        1.0,
                        if *taken > 0 && *not > 0 {
                            p.line_strong
                        } else {
                            p.warning
                        },
                    ),
                    egui::StrokeKind::Inside,
                );
                x = cr.right() + 14.0;
            }
        }
    });
    action
}
