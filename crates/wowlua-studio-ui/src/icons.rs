//! Studio's icons (the design's icons board): a 16 × 16 box, a 1.5 px
//! stroke in the text colour, square caps and miter joins, straight lines
//! only. Each is the design's SVG path, parsed once into polylines.

use std::sync::OnceLock;

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2, epaint::PathShape};

macro_rules! icons {
    ($($variant:ident = $name:literal, $d:literal;)*) => {
        /// An icon of the design.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Icon { $($variant),* }

        impl Icon {
            pub const ALL: &[Icon] = &[$(Icon::$variant),*];

            /// The design's name for the icon.
            pub fn name(self) -> &'static str {
                match self { $(Icon::$variant => $name),* }
            }

            fn path(self) -> &'static str {
                match self { $(Icon::$variant => $d),* }
            }
        }
    };
}

icons! {
    // Navigation
    Files = "files", "M3.75 1.75h5.5l3 3v9.5h-8.5z M9.25 1.75v3h3 M6 8.25h4 M6 11.25h4";
    Git = "git", "M5 2v8.5 M3.5 11.25h3v3h-3z M9.5 1.75h3v3h-3z M11 4.75v3.5H5";
    PullRequest = "pull-request", "M4.5 4.75v6.5 M3 1.75h3v3H3z M3 11.25h3v3H3z M11.5 11.25v-6.5H8 M9.75 2.75l-2 2 2 2 M10 11.25h3v3h-3z";
    Debug = "debug", "M6.25 2.75h3.5v3h-3.5z M5.25 5.75h5.5v8.5h-5.5z M8 5.75v8.5 M2.25 7.75h3 M10.75 7.75h3 M2.25 11.75h3 M10.75 11.75h3";
    Ui = "ui", "M1.75 2.75h12.5v10.5H1.75z M1.75 5.75h12.5 M4.25 8.25h4v2.5h-4z";
    Perf = "perf", "M1.75 13.25h12.5 M1.75 10.25h9 M3.75 7.25h5.5 M4.75 4.25h2.5";
    World = "world", "M1.75 3.5l4-1.75 4.5 1.75 4-1.75v10.75l-4 1.75-4.5-1.75-4 1.75z M5.75 1.75v10.75 M10.25 3.5v10.75";
    Build = "build", "M1.75 9.25h5.5v5h-5.5z M8.75 9.25h5.5v5h-5.5z M5.25 2.75h5.5v5h-5.5z";
    Config = "config", "M1.75 4h3.5 M8.75 4h5.5 M5.25 2.5h3.5v3h-3.5z M1.75 8h7 M12.25 8h2 M8.75 6.5h3.5v3h-3.5z M1.75 12h1.5 M6.75 12h7.5 M3.25 10.5h3.5v3h-3.5z";
    Search = "search", "M2.75 2.75h7v7h-7z M9.75 9.75l3.5 3.5";
    Repo = "repo", "M3.25 1.75h9.5v12.5h-9.5z M3.25 11.25h9.5 M5.75 1.75v9.5";
    Terminal = "terminal", "M1.75 2.75h12.5v10.5H1.75z M4.25 6l2 2-2 2 M7.75 10.25h4";
    // Debugger
    Run = "run", "M4.25 2.75v10.5l8.25-5.25z";
    Pause = "pause", "M5 3v10 M11 3v10";
    StepOver = "step-over", "M2.75 9.25v-4.5h9.5v4.5 M10.25 7.25l2 2 2-2 M7.25 12.25h1.5v1.5h-1.5z";
    StepInto = "step-into", "M8 1.75v7.5 M5 6.25l3 3 3-3 M7.25 12.25h1.5v1.5h-1.5z";
    StepOut = "step-out", "M8 9.25v-7.5 M5 4.75l3-3 3 3 M7.25 12.25h1.5v1.5h-1.5z";
    Restart = "restart", "M3.25 8V3.25h9.5v9.5H6.25 M8.25 10.75l-2 2 2 2";
    Stop = "stop", "M3.25 3.25h9.5v9.5h-9.5z";
    Watch = "watch", "M1.75 8l3-4.25h6.5l3 4.25-3 4.25h-6.5z M6.75 6.75h2.5v2.5h-2.5z";
    Event = "event", "M9.25 1.75L4 9h4l-1.25 5.25L12 7H8z";
    Timer = "timer", "M3.75 1.75h8.5 M3.75 14.25h8.5 M4.75 1.75v2.5L8 8l3.25-3.75v-2.5 M4.75 14.25v-2.5L8 8l3.25 3.75v2.5";
    Record = "record", "M3.75 3.75h8.5v8.5h-8.5z M6.25 6.25h3.5v3.5h-3.5z";
    Lint = "lint", "M1.75 13.25L8 2l6.25 11.25z M8 6.5v3.25 M8 11.5v0.25";
    // Inspection and world
    Inspect = "inspect", "M1.75 5V1.75H5 M1.75 11v3.25H5 M11 1.75h3.25V5 M6.25 6.25l7.5 3-3.25 1.25-1.25 3.25z";
    Layers = "layers", "M8 1.75l6.25 3.25L8 8.25 1.75 5z M1.75 8.25L8 11.5l6.25-3.25 M1.75 11.25L8 14.5l6.25-3.25";
    Snapshot = "snapshot", "M1.75 4.75h3l1.5-2h3.5l1.5 2h3v8.5H1.75z M6.25 7.25h3.5v3.5h-3.5z";
    Character = "character", "M6.25 1.75h3.5v3.5h-3.5z M3.25 14.25v-4.5l2-2h5.5l2 2v4.5";
    Group = "group", "M2.25 2.75h3v3h-3z M10.75 2.75h3v3h-3z M6.5 6.75h3v3h-3z M1.25 14.25v-2.5l1.5-1.5h2.5 M14.75 14.25v-2.5l-1.5-1.5h-2.5 M5.25 14.25v-1.75l1-1h3.5l1 1v1.75";
    Bag = "bag", "M2.75 5.25h10.5v9h-10.5z M5.25 5.25v-3.5h5.5v3.5 M2.75 9h10.5";
    Folder = "folder", "M1.75 3.25h4.5l1.5 1.5h6.5v8.5H1.75z";
    Test = "test", "M1.75 2.75h12.5v10.5H1.75z M4.75 8l2 2 4.5-4.5";
    // Source control and shipping
    Commit = "commit", "M1.5 8h4 M10.5 8h4 M5.5 5.5h5v5h-5z";
    Diff = "diff", "M4.5 1.75v6 M1.5 4.75h6 M8.5 11.25h6";
    Sync = "sync", "M2.25 6.25V3.25h9.5 M9.75 1.25l2 2-2 2 M13.75 9.75v3h-9.5 M6.25 14.75l-2-2 2-2";
    Workflow = "workflow", "M1.75 1.75h4.5v4.5h-4.5z M9.75 9.75h4.5v4.5h-4.5z M6.25 4h3.75v5.75";
    Package = "package", "M2.25 4.75L8 1.75l5.75 3v6.5L8 14.25l-5.75-3z M2.25 4.75L8 7.75l5.75-3 M8 7.75v6.5";
    Publish = "publish", "M8 10.25v-8.5 M4.75 5L8 1.75 11.25 5 M2.25 9.25v4.5h11.5v-4.5";
    Token = "token", "M3.75 7.25h8.5v6.5h-8.5z M5.25 7.25v-3.5h5.5v3.5 M8 9.75v1.5";
    Format = "format", "M1.75 3.25h12.5 M4.75 6.5h9.5 M4.75 9.75h9.5 M1.75 13h12.5";
    // New in the design's version
    Clock = "clock", "M2.75 2.75h10.5v10.5H2.75z M8 5v3.25h2.5";
    Secret = "secret", "M1.75 8l3-4.25h6.5l3 4.25-3 4.25h-6.5z M2.75 13.25L13.25 2.75";
    Taint = "taint", "M8 1.75l4.25 6v3.5L9.75 14.25h-3.5L3.75 11.25v-3.5z";
    Coverage = "coverage", "M1.75 4.75h12.5v6.5H1.75z M5.75 4.75v6.5 M9.75 4.75v6.5";
    Compare = "compare", "M1.75 2.75h5.5v10.5h-5.5z M8.75 2.75h5.5v10.5h-5.5z M3.25 9.25l1.5-1.5 1 1 M10.25 9.25l1.5-1.5 1 1";
    Install = "install", "M8 1.75v8.5 M4.75 7L8 10.25 11.25 7 M2.25 9.25v4.5h11.5v-4.5";
    Reload = "reload", "M12.75 8V3.25h-9.5v9.5h6.5 M7.75 10.75l2 2-2 2";
    Nameplate = "nameplate", "M3.25 2.25h9.5v3h-9.5z M6.75 7.75h2.5v2.5h-2.5z M5.25 14.25v-2l1.5-1.5h2.5l1.5 1.5v2";
    World3d = "world-3d", "M1.75 13.25h12.5 M3.75 9.25h8.5 M5.25 6.25h5.5 M1.75 13.25L5.25 6.25 M14.25 13.25L10.75 6.25 M8 6.25v7";
    Logpoint = "logpoint", "M8 2.5l5.5 5.5L8 13.5 2.5 8z";
    Condition = "condition", "M3.25 3.25h9.5v9.5h-9.5z M5.75 6.75h4.5 M5.75 9.25h4.5";
    Agent = "agent", "M1.75 2.75h12.5v8h-7l-3.5 3v-3h-2z M4.75 6.75h6.5";
    Approve = "approve", "M8 1.75l5.25 2v4.5L8 14.25 2.75 8.25v-4.5z M5.5 8l1.75 1.75L10.75 6.25";
    Input = "input", "M3.25 2.25v9l2.5-2.5 2 4 1.5-.75-2-4h3.5z";
    ProfileDoc = "profile-doc", "M3.75 1.75h5.5l3 3v9.5h-8.5z M9.25 1.75v3h3 M5.75 11.5v-2l1-1h2.5l1 1v2 M7 6.25h2v1.5H7z";
    Pin = "pin", "M5.75 1.75h4.5 M6.75 1.75v4.5l-2 2.5h6.5l-2-2.5v-4.5 M8 8.75v5.5";
    Notification = "notification", "M3.75 11.25V6.75l1.5-3.5h5.5l1.5 3.5v4.5 M2.25 11.25h11.5 M6.5 13.75h3";
    // File types (the explorer); the manifest is `config`, the lockfile
    // `token`, a world profile `profile-doc`.
    WowluaFile = "wowlua", "M3.75 1.75h5.5l3 3v9.5h-8.5z M9.25 1.75v3h3 M9.75 7l-1.5 1.5v2l1.5 1.5h-3l-1.5-1.5v-2l1.5-1.5z";
    TestFile = "test-file", "M3.75 1.75h5.5l3 3v9.5h-8.5z M9.25 1.75v3h3 M5.5 10l1.75 1.75 3.25-3.25";
    TypesFile = "types", "M3.75 1.75h5.5l3 3v9.5h-8.5z M9.25 1.75v3h3 M5.75 7.75h4.5 M8 7.75v4.5";
    LuaFile = "lua", "M3.75 1.75h5.5l3 3v9.5h-8.5z M9.25 1.75v3h3 M5.75 7.25v4.5h3.5";
    XmlFile = "xml", "M3.75 1.75h5.5l3 3v9.5h-8.5z M9.25 1.75v3h3 M6.25 7.75l-1.5 1.75 1.5 1.75 M9.75 7.75l1.5 1.75-1.5 1.75";
    JsonFile = "json", "M3.75 1.75h5.5l3 3v9.5h-8.5z M9.25 1.75v3h3 M6.5 7.25h-1v4.5h1 M9.5 7.25h1v4.5h-1";
    SnapFile = "snap", "M3.75 1.75h5.5l3 3v9.5h-8.5z M9.25 1.75v3h3 M5.75 8h4.5v3.5h-4.5z";
    MarkdownFile = "markdown", "M3.75 1.75h5.5l3 3v9.5h-8.5z M9.25 1.75v3h3 M5.75 8h4.5 M5.75 10.5h3";
    Image = "image", "M1.75 2.75h12.5v10.5H1.75z M3.75 11.25l3-3.5 2 2 1.5-1.5 2 3 M10.25 5.25h1.5v1.5h-1.5z";
    Audio = "audio", "M3.75 1.75h5.5l3 3v9.5h-8.5z M9.25 1.75v3h3 M5.25 10.75h1.5v1.5h-1.5z M6.75 11.25v-4l3-1v3.5";
    FontFile = "font", "M3.75 1.75h5.5l3 3v9.5h-8.5z M9.25 1.75v3h3 M5.75 12.25l2.25-5.5 2.25 5.5 M6.5 10.5h3";
    FolderOpen = "folder-open", "M1.75 13.25V3.25h4.5l1.5 1.5h5v2 M1.75 13.25l2-6.5h10.5l-2 6.5z";
    // The window's chrome (the title bar and panels boards)
    Grip = "grip", "M5.5 3.5h1 M9.5 3.5h1 M5.5 8h1 M9.5 8h1 M5.5 12.5h1 M9.5 12.5h1";
    Add = "add", "M8 2.5v11 M2.5 8h11";
    Minimize = "minimize", "M3 8.5h10";
    Maximize = "maximize", "M3.25 3.25h9.5v9.5h-9.5z";
    Close = "close", "M3.5 3.5l9 9 M12.5 3.5l-9 9";
}

/// A run of straight lines in the 16 × 16 box.
#[derive(Debug, PartialEq)]
pub(crate) struct Subpath {
    pub(crate) points: Vec<Pos2>,
    pub(crate) closed: bool,
}

fn parsed(icon: Icon) -> &'static [Subpath] {
    static ALL: OnceLock<Vec<Vec<Subpath>>> = OnceLock::new();
    let all = ALL.get_or_init(|| Icon::ALL.iter().map(|i| parse(i.path())).collect());
    &all[icon as usize]
}

/// Parses the design's path data: `M L H V Z` and their relative forms.
pub(crate) fn parse(d: &str) -> Vec<Subpath> {
    let mut out: Vec<Subpath> = Vec::new();
    let mut cur = Pos2::ZERO;
    let mut start = Pos2::ZERO;
    let mut cmd = 'M';
    let mut tokens = Tokens {
        s: d.as_bytes(),
        i: 0,
    };
    while let Some(tok) = tokens.next() {
        let n = match tok {
            Tok::Cmd(c) => {
                cmd = c;
                if c == 'z' || c == 'Z' {
                    if let Some(last) = out.last_mut() {
                        last.closed = true;
                    }
                    cur = start;
                }
                continue;
            }
            Tok::Num(n) => n,
        };
        let c = cmd;
        let rel = c.is_ascii_lowercase();
        let base = if rel { cur.to_vec2() } else { Vec2::ZERO };
        let next = match c.to_ascii_uppercase() {
            'M' | 'L' => {
                let y = tokens.num();
                Pos2::new(n, y) + base
            }
            'H' => Pos2::new(n + base.x, cur.y),
            'V' => Pos2::new(cur.x, n + base.y),
            other => panic!("icon path command `{other}` is not supported"),
        };
        if c.eq_ignore_ascii_case(&'M') {
            out.push(Subpath {
                points: vec![next],
                closed: false,
            });
            start = next;
            // Coordinates after a move are lines.
            cmd = if rel { 'l' } else { 'L' };
        } else if let Some(last) = out.last_mut() {
            last.points.push(next);
        }
        cur = next;
    }
    out
}

enum Tok {
    Cmd(char),
    Num(f32),
}

struct Tokens<'a> {
    s: &'a [u8],
    i: usize,
}

impl Tokens<'_> {
    fn next(&mut self) -> Option<Tok> {
        while self.i < self.s.len() && (self.s[self.i] == b' ' || self.s[self.i] == b',') {
            self.i += 1;
        }
        let b = *self.s.get(self.i)?;
        if b.is_ascii_alphabetic() {
            self.i += 1;
            return Some(Tok::Cmd(b as char));
        }
        let from = self.i;
        self.i += 1;
        let mut dot = b == b'.';
        while let Some(&c) = self.s.get(self.i) {
            if c.is_ascii_digit() || (c == b'.' && !dot) {
                dot |= c == b'.';
                self.i += 1;
            } else {
                break;
            }
        }
        let text = std::str::from_utf8(&self.s[from..self.i]).unwrap_or("0");
        Some(Tok::Num(text.parse().unwrap_or(0.0)))
    }

    fn num(&mut self) -> f32 {
        match self.next() {
            Some(Tok::Num(n)) => n,
            _ => 0.0,
        }
    }
}

/// Paints `icon` filling `rect` (16 px, or 14 px for the small size) in
/// `color`.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let scale = rect.width() / 16.0;
    let width = 1.5 * scale;
    let stroke = Stroke::new(width, color);
    let at = |p: Pos2| rect.min + p.to_vec2() * scale;
    for sub in parsed(icon) {
        let mut points: Vec<Pos2> = sub.points.iter().map(|&p| at(p)).collect();
        if sub.closed {
            painter.add(Shape::Path(PathShape::closed_line(points, stroke)));
            continue;
        }
        // Square caps: each open end reaches half a stroke further.
        let n = points.len();
        if n >= 2 {
            let ext = width / 2.0;
            let first = (points[0] - points[1]).normalized() * ext;
            let last = (points[n - 1] - points[n - 2]).normalized() * ext;
            points[0] += first;
            points[n - 1] += last;
        }
        painter.add(Shape::Path(PathShape::line(points, stroke)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_parses_inside_its_box() {
        for &icon in Icon::ALL {
            let subs = parsed(icon);
            assert!(!subs.is_empty(), "{} has no lines", icon.name());
            for p in subs.iter().flat_map(|s| &s.points) {
                assert!(
                    (0.0..=16.0).contains(&p.x) && (0.0..=16.0).contains(&p.y),
                    "{} leaves its box at {p:?}",
                    icon.name()
                );
            }
        }
    }

    #[test]
    fn relative_and_absolute_commands() {
        assert_eq!(
            parse("M3.25 3.25h9.5v9.5h-9.5z"),
            vec![Subpath {
                points: vec![
                    Pos2::new(3.25, 3.25),
                    Pos2::new(12.75, 3.25),
                    Pos2::new(12.75, 12.75),
                    Pos2::new(3.25, 12.75),
                ],
                closed: true,
            }]
        );
        assert_eq!(
            parse("M3.25 2.25v9l2.5-2.5 2 4 1.5-.75")[0].points,
            vec![
                Pos2::new(3.25, 2.25),
                Pos2::new(3.25, 11.25),
                Pos2::new(5.75, 8.75),
                Pos2::new(7.75, 12.75),
                Pos2::new(9.25, 12.0),
            ]
        );
    }
}
