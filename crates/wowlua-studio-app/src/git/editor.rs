//! Git in the editor (M11, the canvas's git-in-the-editor board): the
//! gutter's marks against the last commit and their hunk, blame after the
//! cursor's line, and a merge conflict's lenses.

use std::path::Path;
use std::time::{Duration, Instant};

use egui::{Align2, Id, Order, Pos2, Stroke};
use wowlua_studio_ui::widgets::{self, ButtonKind};
use wowlua_studio_ui::{Face, Icon, palette};

use super::model::{Apply, DiffOf, Hunk, LineKind};
use super::page::{age, diff_row};
use super::{Dialog, GitState, Selected};
use crate::editor::diff;
use crate::editor::doc::Document;
use crate::editor::view::{GitMark, Lens, LensAct, LensClick, Take, Tone};
use crate::system::{GitRequest, System};

/// How often blame is asked again while a file is edited.
const BLAME_EVERY: Duration = Duration::from_millis(1500);

/// The marks of `text` against `head` (the file at the last commit):
/// added, changed, and where lines were removed.
pub fn marks(head: &str, text: &str) -> Vec<(usize, GitMark)> {
    let Some(lines) = diff::lines(head, text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut line = 0usize;
    let mut i = 0;
    while i < lines.len() {
        match lines[i] {
            diff::Line::Same(_) => {
                line += 1;
                i += 1;
            }
            _ => {
                let removed = lines[i..]
                    .iter()
                    .take_while(|l| matches!(l, diff::Line::Removed(_)))
                    .count();
                let added = lines[i + removed..]
                    .iter()
                    .take_while(|l| matches!(l, diff::Line::Added(_)))
                    .count();
                if removed + added == 0 {
                    i += 1;
                    continue;
                }
                for k in 0..added {
                    let mark = if k < removed {
                        GitMark::Changed
                    } else {
                        GitMark::Added
                    };
                    out.push((line + k, mark));
                }
                if added == 0 {
                    out.push((line, GitMark::Removed));
                }
                line += added;
                i += removed + added;
            }
        }
    }
    out
}

/// A merge conflict in the editor's text: its marker lines (from 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Conflict {
    pub start: usize,
    pub middle: usize,
    pub end: usize,
}

/// The conflicts git left in a file (`<<<<<<<`, `=======`, `>>>>>>>`).
pub fn conflicts(doc: &Document) -> Vec<Conflict> {
    let mut out = Vec::new();
    let mut start = None;
    let mut middle = None;
    for i in 0..doc.line_count() {
        let l = doc.line(i);
        if l.starts_with("<<<<<<< ") || l == "<<<<<<<" {
            start = Some(i);
            middle = None;
        } else if l == "=======" && start.is_some() {
            middle = Some(i);
        } else if (l.starts_with(">>>>>>> ") || l == ">>>>>>>")
            && let (Some(s), Some(m)) = (start, middle)
        {
            out.push(Conflict {
                start: s,
                middle: m,
                end: i,
            });
            start = None;
            middle = None;
        }
    }
    out
}

/// A conflict's lenses: take mine, theirs, both; which of how many.
pub fn conflict_lenses(path: &Path, list: &[Conflict]) -> Vec<Lens> {
    let mut out = Vec::new();
    for (i, c) in list.iter().enumerate() {
        for (title, take) in [
            ("take mine", Take::Mine),
            ("take theirs", Take::Theirs),
            ("take both", Take::Both),
        ] {
            out.push(Lens {
                line: c.start,
                title: title.to_owned(),
                act: LensAct::Click(LensClick::Conflict {
                    file: path.to_path_buf(),
                    line: c.start,
                    take,
                }),
            });
        }
        out.push(Lens {
            line: c.start,
            title: format!("conflict {} of {}", i + 1, list.len()),
            act: LensAct::Note(Tone::Muted),
        });
    }
    out
}

/// A conflict resolved in the document: its lines replaced by the side
/// taken (mine first for both), undoable.
pub fn resolve(doc: &mut Document, line: usize, take: Take) {
    let Some(c) = conflicts(doc).into_iter().find(|c| c.start == line) else {
        return;
    };
    let mine: Vec<String> = (c.start + 1..c.middle)
        .map(|i| doc.line(i).to_owned())
        .collect();
    let theirs: Vec<String> = (c.middle + 1..c.end)
        .map(|i| doc.line(i).to_owned())
        .collect();
    let kept: Vec<String> = match take {
        Take::Mine => mine,
        Take::Theirs => theirs,
        Take::Both => mine.into_iter().chain(theirs).collect(),
    };
    let start = crate::editor::doc::Pos::new(c.start, 0);
    let (end, mut text) = if c.end + 1 < doc.line_count() {
        (crate::editor::doc::Pos::new(c.end + 1, 0), kept.join("\n"))
    } else {
        (
            crate::editor::doc::Pos::new(c.end, doc.line_len(c.end)),
            kept.join("\n"),
        )
    };
    if c.end + 1 < doc.line_count() && !kept.is_empty() {
        text.push('\n');
    }
    doc.edit(start, end, &text, crate::editor::doc::EditKind::Other);
}

/// What the editor shows of Git for its file: the marks (a conflict's
/// sides, else the changes since the last commit) and the cursor line's
/// blame when asked for.
pub fn of(
    state: &mut GitState,
    system: &dyn System,
    path: &Path,
    doc: &Document,
    blame: bool,
) -> (Vec<(usize, GitMark)>, Vec<Conflict>, Option<String>) {
    let list = conflicts(doc);
    let Some(rel) = state.relative(path) else {
        return (Vec::new(), list, None);
    };
    let tracked = state.status.as_ref().is_some_and(|s| {
        s.file(&rel)
            .is_none_or(|f| f.unstaged != Some(super::model::Change::Untracked))
    });
    if !tracked {
        return (Vec::new(), list, None);
    }
    let marks = if list.is_empty() {
        let version = doc.version;
        match state.marks.get(&rel) {
            Some((v, m)) if *v == version => m.clone(),
            _ => match state.head_text(system, &rel).map(str::to_owned) {
                Some(head) => {
                    let m = marks(&head, &doc.text());
                    state.marks.insert(rel.clone(), (version, m.clone()));
                    m
                }
                None => Vec::new(),
            },
        }
    } else {
        let mut m = Vec::new();
        for c in &list {
            m.extend((c.start..c.middle).map(|l| (l, GitMark::Mine)));
            m.extend((c.middle..=c.end).map(|l| (l, GitMark::Theirs)));
        }
        m
    };
    let line = doc.selection.head.line;
    let blame_text = if blame && list.is_empty() {
        let due = state
            .blame_at
            .get(&rel)
            .is_none_or(|at| at.elapsed() >= BLAME_EVERY);
        let lines = if due {
            state.blame_at.insert(rel.clone(), Instant::now());
            state.blame_of(system, &rel, doc.version, || doc.text())
        } else {
            state.blame.get(&rel).map(|(_, l)| l.as_slice())
        };
        lines.and_then(|l| l.get(line)).map(|b| {
            if b.hash.is_empty() {
                "not committed yet".to_owned()
            } else {
                format!("{} · {} · {}", b.author, age(b.time), b.subject)
            }
        })
    } else {
        None
    };
    (marks, list, blame_text)
}

/// The commit of a line's blame, to show on the git page.
pub fn blame_commit(state: &GitState, path: &Path, line: usize) -> Option<String> {
    let rel = state.relative(path)?;
    let b = state.blame.get(&rel)?.1.get(line)?;
    (!b.hash.is_empty()).then(|| b.hash.clone())
}

/// A gutter mark's hunk, over the editor: its lines, stage or unstage it,
/// discard it, compare it on the git page.
pub enum PopupAction {
    ShowGit,
}

/// The hunk of the unstaged (else staged) diff that holds `line` (from 0).
fn hunk_at(state: &GitState, rel: &str, line: usize) -> Option<(Hunk, DiffOf)> {
    let n = line as u32 + 1;
    for of in [DiffOf::Unstaged, DiffOf::Staged] {
        if let Some(d) = state.diff(rel, &of) {
            let hit = d.hunks.iter().find(|h| {
                let last = h.new_start + h.new_lines.max(1);
                n + 1 >= h.new_start && n <= last
            });
            if let Some(h) = hit {
                return Some((h.clone(), of));
            }
        }
    }
    None
}

/// The popup of a gutter mark clicked, while it is open.
pub fn popup(
    ctx: &egui::Context,
    state: &mut GitState,
    system: &dyn System,
) -> Option<PopupAction> {
    let (rel, line, at) = state.mark_popup.clone()?;
    for of in [DiffOf::Unstaged, DiffOf::Staged] {
        if state.diff(&rel, &of).is_none() {
            state.want_diff(system, &rel, of);
        }
    }
    let p = palette(ctx);
    let mut close = false;
    let mut action = None;
    let found = hunk_at(state, &rel, line);
    let area = egui::Area::new(Id::unique("wowlua-studio-git-hunk"))
        .order(Order::Foreground)
        .pivot(Align2::LEFT_TOP)
        .fixed_pos(Pos2::new(at.x + 6.0, at.y + 10.0))
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(p.bg_200)
                .stroke(Stroke::new(1.0, p.line_strong))
                .shadow(ui.visuals().popup_shadow)
                .show(ui, |ui| {
                    ui.set_width(620.0);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let Some((hunk, of)) = &found else {
                        wowlua_studio_ui::forms::padded(ui, 12, 10, |ui| {
                            ui.label(
                                egui::RichText::new("Reading the hunk…")
                                    .font(Face::Plex.at(12.0))
                                    .color(p.ink_muted),
                            );
                        });
                        return;
                    };
                    let staged = *of == DiffOf::Staged;
                    egui::Frame::new()
                        .fill(p.bg_100)
                        .inner_margin(egui::Margin::symmetric(12, 4))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "@@ {}–{} · {}",
                                        hunk.new_start,
                                        hunk.new_start + hunk.new_lines.saturating_sub(1),
                                        if staged { "staged" } else { "not staged" }
                                    ))
                                    .font(Face::Plex.at(11.5))
                                    .color(p.ink_muted),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if widgets::icon_button(ui, Icon::Close, "close", false)
                                            .clicked()
                                        {
                                            close = true;
                                        }
                                    },
                                );
                            });
                        });
                    for l in hunk.lines.iter().filter(|l| l.kind != LineKind::Context) {
                        let (sign, n, ground) = match l.kind {
                            LineKind::Added => ("+", l.new, p.success_soft),
                            _ => ("−", None, p.error_soft),
                        };
                        diff_row(ui, system, sign, n, &l.text, ground);
                    }
                    wowlua_studio_ui::forms::padded(ui, 10, 8, |ui| {
                        ui.horizontal(|ui| {
                            let request = |how| GitRequest::Hunk {
                                path: rel.clone(),
                                hunk: hunk.clone(),
                                how,
                            };
                            if staged {
                                if widgets::button(
                                    ui,
                                    ButtonKind::Secondary,
                                    "unstage hunk",
                                    true,
                                    Some(Icon::Minimize),
                                )
                                .clicked()
                                {
                                    state.change(
                                        system,
                                        "unstaging the hunk…",
                                        request(Apply::Unstage),
                                    );
                                    close = true;
                                }
                            } else {
                                if widgets::button(
                                    ui,
                                    ButtonKind::Secondary,
                                    "stage hunk",
                                    true,
                                    None,
                                )
                                .clicked()
                                {
                                    state.change(
                                        system,
                                        "staging the hunk…",
                                        request(Apply::Stage),
                                    );
                                    close = true;
                                }
                                if widgets::button(
                                    ui,
                                    ButtonKind::Tertiary,
                                    "discard hunk",
                                    true,
                                    None,
                                )
                                .clicked()
                                {
                                    state.dialog = Some(Dialog::Discard {
                                        what: format!("a hunk of {rel}"),
                                        request: request(Apply::Discard),
                                    });
                                    close = true;
                                }
                            }
                            if widgets::button(
                                ui,
                                ButtonKind::Inline,
                                "compare in the git page",
                                true,
                                None,
                            )
                            .clicked()
                            {
                                state.selected = Some(Selected::File(rel.clone(), of.clone()));
                                action = Some(PopupAction::ShowGit);
                                close = true;
                            }
                        });
                    });
                });
        });
    let clicked_out = ctx.input(|i| i.pointer.any_pressed())
        && ctx
            .input(|i| i.pointer.interact_pos())
            .is_some_and(|pos| !area.response.rect.contains(pos));
    if close || clicked_out || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.mark_popup = None;
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn marks_say_what_changed_since_the_commit() {
        let head = "a\nb\nc\nd\ne\n";
        let text = "a\nB\nc\nnew\nd\n";
        // b changed, a line added after c, e removed at the end
        assert_eq!(
            marks(head, text),
            [
                (1, GitMark::Changed),
                (3, GitMark::Added),
                (5, GitMark::Removed)
            ]
        );
        assert!(marks(head, head).is_empty());
    }

    #[test]
    fn a_conflict_is_found_and_resolved_to_a_side() {
        let text = "x\n<<<<<<< HEAD\nmine\n=======\ntheirs\n>>>>>>> main\ny\n";
        let mut doc = Document::new(PathBuf::from("a.wowlua"), text);
        let found = conflicts(&doc);
        assert_eq!(
            found,
            [Conflict {
                start: 1,
                middle: 3,
                end: 5
            }]
        );
        let lenses = conflict_lenses(Path::new("a.wowlua"), &found);
        assert_eq!(lenses.len(), 4);
        assert_eq!(lenses[3].title, "conflict 1 of 1");
        resolve(&mut doc, 1, Take::Both);
        assert_eq!(doc.text(), "x\nmine\ntheirs\ny\n");
        let mut doc = Document::new(PathBuf::from("a.wowlua"), text);
        resolve(&mut doc, 1, Take::Theirs);
        assert_eq!(doc.text(), "x\ntheirs\ny\n");
        assert!(conflicts(&doc).is_empty());
    }
}
