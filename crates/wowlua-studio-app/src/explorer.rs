//! The explorer (the design's editor board): the workspace's packages, the
//! root first, and their files. Folders are read when opened and again
//! when their files change on disk; the visible rows are flattened once
//! per change, so a frame draws only the rows on screen.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use egui::{Align2, Color32, CornerRadius, CursorIcon, Pos2, Rect, Sense, Ui, Vec2};
use wowlua_studio_ui::{Face, Icon, Palette, icons, palette, widgets};

use crate::system::{Entry, System, Workspace};

/// A row, at the settings' density (S062).
fn row_h() -> f32 {
    wowlua_studio_ui::row(24.0)
}
const SECTION_H: f32 = 30.0;

#[derive(Default)]
pub struct Explorer {
    /// Open folders and packages.
    expanded: HashSet<PathBuf>,
    /// Folders read from disk.
    listings: HashMap<PathBuf, Vec<Entry>>,
    selected: Option<PathBuf>,
    /// The rows to draw; `None` after a change.
    rows: Option<Vec<Row>>,
}

/// A row of the tree.
#[derive(Clone, Debug, PartialEq)]
enum Row {
    /// A package: its index in the workspace's members, and its folder.
    Package {
        member: usize,
        dir: PathBuf,
        open: bool,
    },
    Folder {
        path: PathBuf,
        name: String,
        depth: usize,
        open: bool,
    },
    File {
        path: PathBuf,
        name: String,
        depth: usize,
    },
}

impl Explorer {
    /// A new workspace: the root package open.
    pub fn new(workspace: &Workspace) -> Self {
        let mut explorer = Explorer::default();
        explorer.expanded.insert(workspace.root.clone());
        explorer
    }

    /// Files changed under these paths: their folders are read again.
    pub fn changed(&mut self, paths: &[PathBuf]) {
        for path in paths {
            self.listings.remove(path);
            if let Some(parent) = path.parent() {
                self.listings.remove(parent);
            }
        }
        self.rows = None;
    }

    /// Draws the tree; returns the file clicked, to open.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        workspace: &Workspace,
        system: &dyn System,
        git: &crate::git::GitState,
        note: Option<&str>,
    ) -> Option<PathBuf> {
        let p = palette(ui.ctx());
        let rows = match self.rows.take() {
            Some(rows) => rows,
            None => self.flatten(workspace, system),
        };

        // The section: the workspace's name and what it holds.
        let top = ui.max_rect().top();
        let (sec, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), SECTION_H), Sense::hover());
        let font = Face::Plex.at(11.5);
        let base = Pos2::new(sec.left(), top + 10.0 + 8.0);
        ui.painter().text(
            base + Vec2::new(12.0, 0.0),
            Align2::LEFT_CENTER,
            &workspace.name,
            font.clone(),
            p.ink_muted,
        );
        let what = match workspace.members.len() {
            1 => workspace.members[0].kind.clone(),
            n => format!("workspace · {n} packages"),
        };
        ui.painter().text(
            Pos2::new(sec.right() - 12.0, base.y),
            Align2::RIGHT_CENTER,
            what,
            font,
            p.ink_muted,
        );

        // (a clone's: deleted when the workspace closes, M12)
        if let Some(note) = note {
            wowlua_studio_ui::forms::padded(ui, 12, 4, |ui| {
                wowlua_studio_ui::forms::paragraph(ui, note, 11.5, 16.0, p.ink_muted);
            });
        }
        let mut clicked = None;
        egui::ScrollArea::vertical().auto_shrink(false).show_rows(
            ui,
            row_h(),
            rows.len(),
            |ui, range| {
                for i in range {
                    if self.row(ui, p, workspace, &rows[i], git) {
                        clicked = Some(i);
                    }
                }
            },
        );
        self.rows = Some(rows);
        clicked.and_then(|i| self.click(i))
    }

    /// Draws a row; returns whether it was clicked.
    fn row(
        &self,
        ui: &mut Ui,
        p: &Palette,
        workspace: &Workspace,
        row: &Row,
        git: &crate::git::GitState,
    ) -> bool {
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h()), Sense::click());
        let selected = match row {
            Row::File { path, .. } => self.selected.as_deref() == Some(path.as_path()),
            _ => false,
        };
        if selected {
            ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_400);
        } else if response.hovered() {
            ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_300);
        }
        let cy = rect.center().y;
        let (depth, open, icon, color, name, meta, strong) = match row {
            Row::Package { member, open, .. } => {
                let m = &workspace.members[*member];
                let meta = if m.rel.is_empty() {
                    [m.kind.as_str(), m.version.as_str()]
                        .into_iter()
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(" · ")
                } else {
                    let parent = m.rel.rsplit_once('/').map_or("", |(dir, _)| dir);
                    let mut parts = Vec::new();
                    if m.kind != "addon" {
                        parts.push(m.kind.clone());
                    }
                    if !parent.is_empty() {
                        parts.push(format!("{parent}/"));
                    }
                    parts.join(" · ")
                };
                (
                    0,
                    Some(*open),
                    Icon::Package,
                    p.amber_ink,
                    m.name.as_str(),
                    meta,
                    true,
                )
            }
            Row::Folder {
                name, depth, open, ..
            } => {
                let icon = if *open {
                    Icon::FolderOpen
                } else {
                    Icon::Folder
                };
                (
                    *depth,
                    Some(*open),
                    icon,
                    p.ink_muted,
                    name.as_str(),
                    String::new(),
                    false,
                )
            }
            Row::File { path, name, depth } => {
                let (icon, color) = file_icon(path, name, p);
                (
                    *depth,
                    None,
                    icon,
                    color,
                    name.as_str(),
                    String::new(),
                    false,
                )
            }
        };
        let x = rect.left() + 12.0 + depth as f32 * 16.0;
        let painter = ui.painter();
        if let Some(open) = open {
            painter.text(
                Pos2::new(x + 6.0, cy),
                Align2::CENTER_CENTER,
                if open { "−" } else { "+" },
                Face::Plex.at(12.5),
                p.ink_muted,
            );
        }
        icons::paint(
            painter,
            Rect::from_min_size(Pos2::new(x + 20.0, cy - 7.0), Vec2::splat(14.0)),
            icon,
            color,
        );
        let face = if strong {
            Face::PlexSemiBold
        } else {
            Face::Plex
        };
        // Git (M11): a changed file's name in its change's colour and its
        // letter at the right; a folder with changes, a dot.
        let (ink, letter, dot) = match row {
            Row::File { path, .. } => match git.change_of(path) {
                Some(c) => (
                    crate::git::page::change_color(c, p),
                    Some((c.letter(), crate::git::page::change_color(c, p))),
                    false,
                ),
                None => (p.ink, None, false),
            },
            // (a package's row has its kind and version at the right: its
            // folders show the dot)
            Row::Folder { path, .. } => (p.ink, None, git.folder_changed(path)),
            Row::Package { .. } => (p.ink, None, false),
        };
        if let Some((letter, color)) = letter {
            ui.painter().text(
                Pos2::new(rect.right() - 16.0, cy),
                Align2::CENTER_CENTER,
                letter,
                Face::PlexSemiBold.at(11.0),
                color,
            );
        }
        if dot {
            ui.painter().rect_filled(
                Rect::from_center_size(Pos2::new(rect.right() - 16.0, cy), Vec2::splat(6.0)),
                CornerRadius::ZERO,
                p.warning,
            );
        }
        let text = widgets::galley(ui, name, face.at(12.5), ink, 0.0);
        let text_x = x + 42.0;
        ui.painter().with_clip_rect(rect).galley(
            Pos2::new(text_x, cy - text.size().y / 2.0),
            text.clone(),
            ink,
        );
        if !meta.is_empty() {
            let at = text_x + text.size().x + 8.0;
            ui.painter().with_clip_rect(rect).text(
                Pos2::new(at, cy),
                Align2::LEFT_CENTER,
                meta,
                Face::Plex.at(11.5),
                p.ink_muted,
            );
        }
        response.on_hover_cursor(CursorIcon::PointingHand).clicked()
    }

    fn click(&mut self, i: usize) -> Option<PathBuf> {
        let rows = self.rows.as_ref()?;
        match &rows[i] {
            Row::Package { dir: path, .. } | Row::Folder { path, .. } => {
                let path = path.clone();
                if !self.expanded.remove(&path) {
                    self.expanded.insert(path);
                }
                self.rows = None;
                None
            }
            Row::File { path, .. } => {
                self.selected = Some(path.clone());
                self.selected.clone()
            }
        }
    }

    /// The visible rows: each package, and inside the open ones their
    /// files, members' own folders left out (they are packages).
    fn flatten(&mut self, workspace: &Workspace, system: &dyn System) -> Vec<Row> {
        let members: HashSet<&Path> = workspace
            .members
            .iter()
            .filter(|m| !m.rel.is_empty())
            .map(|m| m.dir.as_path())
            .collect();
        let mut rows = Vec::new();
        for (i, member) in workspace.members.iter().enumerate() {
            let open = self.expanded.contains(&member.dir);
            rows.push(Row::Package {
                member: i,
                dir: member.dir.clone(),
                open,
            });
            if open {
                self.folder(&member.dir, 1, &members, system, &mut rows);
            }
        }
        rows
    }

    fn folder(
        &mut self,
        dir: &Path,
        depth: usize,
        members: &HashSet<&Path>,
        system: &dyn System,
        rows: &mut Vec<Row>,
    ) {
        let entries = self
            .listings
            .entry(dir.to_path_buf())
            .or_insert_with(|| system.list(dir))
            .clone();
        for entry in entries {
            if entry.is_dir {
                if members.contains(entry.path.as_path()) {
                    continue;
                }
                let open = self.expanded.contains(&entry.path);
                rows.push(Row::Folder {
                    path: entry.path.clone(),
                    name: entry.name,
                    depth,
                    open,
                });
                if open {
                    self.folder(&entry.path, depth + 1, members, system, rows);
                }
            } else {
                rows.push(Row::File {
                    path: entry.path,
                    name: entry.name,
                    depth,
                });
            }
        }
    }
}

/// A file's icon and its colour (the icons board's file types).
pub(crate) fn file_icon(path: &Path, name: &str, p: &Palette) -> (Icon, Color32) {
    let lower = name.to_lowercase();
    let ext = lower.rsplit_once('.').map_or("", |(_, e)| e);
    let in_profiles = path
        .parent()
        .is_some_and(|d| d.ends_with("tests/profiles") || d.ends_with("tests\\profiles"));
    match ext {
        _ if lower == "wowlua.json" => (Icon::Config, p.amber_ink),
        _ if lower == "wowlua.lock" => (Icon::Token, p.ink_muted),
        _ if lower.ends_with(".test.wowlua") => (Icon::TestFile, p.success),
        _ if lower.ends_with(".d.wowlua") => (Icon::TypesFile, p.amber_ink),
        "wowlua" => (Icon::WowluaFile, p.amber_ink),
        "json" if in_profiles => (Icon::ProfileDoc, p.note),
        "json" => (Icon::JsonFile, p.note),
        "lua" => (Icon::LuaFile, p.ink_muted),
        "xml" => (Icon::XmlFile, p.ink_muted),
        "snap" => (Icon::SnapFile, p.ink_muted),
        "md" => (Icon::MarkdownFile, p.ink_muted),
        "tga" | "blp" | "png" | "jpg" | "jpeg" => (Icon::Image, p.ink_muted),
        "ogg" | "mp3" | "wav" => (Icon::Audio, p.ink_muted),
        "ttf" | "otf" => (Icon::FontFile, p.ink_muted),
        _ => (Icon::Files, p.ink_muted),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::{Member, NoSystem};

    #[test]
    fn file_types_have_the_boards_icons() {
        let p = &wowlua_studio_ui::DARK;
        let icon = |path: &str| file_icon(Path::new(path), path.rsplit('/').next().unwrap(), p).0;
        assert_eq!(icon("a/wowlua.json"), Icon::Config);
        assert_eq!(icon("a/src/Main.wowlua"), Icon::WowluaFile);
        assert_eq!(icon("a/tests/Main.test.wowlua"), Icon::TestFile);
        assert_eq!(icon("a/tests/profiles/raid.json"), Icon::ProfileDoc);
        assert_eq!(icon("a/locales/enUS.json"), Icon::JsonFile);
    }

    #[test]
    fn packages_are_rows_and_the_root_is_open() {
        let member = |name: &str, rel: &str| Member {
            name: name.into(),
            kind: "addon".into(),
            version: "1.0.0".into(),
            dir: Path::new("/w").join(rel),
            rel: rel.into(),
            flavors: Vec::new(),
        };
        let workspace = Workspace {
            root: PathBuf::from("/w"),
            name: "w".into(),
            members: vec![member("w", ""), member("lib", "libs/lib")],
            flavors: Vec::new(),
            cannot_check: None,
        };
        let mut explorer = Explorer::new(&workspace);
        let rows = explorer.flatten(&workspace, &NoSystem);
        assert_eq!(
            rows,
            [
                Row::Package {
                    member: 0,
                    dir: PathBuf::from("/w"),
                    open: true
                },
                Row::Package {
                    member: 1,
                    dir: PathBuf::from("/w/libs/lib"),
                    open: false
                }
            ]
        );
    }
}
