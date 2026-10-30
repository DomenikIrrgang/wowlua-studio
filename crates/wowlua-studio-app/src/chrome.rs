//! The window's chrome (the design's title bar, rail and status bar
//! boards): a 40 px title bar, a 64 px rail, a 26 px status bar.

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, Id, Pos2, Rect, ResizeDirection, Sense, Stroke,
    StrokeKind, Ui, Vec2, ViewportCommand,
};
use wowlua_studio_ui::{Face, Icon, forms, icons, mark, palette, widgets};

use crate::{
    Platform,
    commands::{Command, Entry, Menu, key_words},
    pages::Page,
};

pub const TITLE_H: f32 = 40.0;
pub const RAIL_W: f32 = 64.0;
pub const STATUS_H: f32 = 26.0;
/// Room for macOS's traffic lights at the left of the title bar.
const TRAFFIC_LIGHTS_W: f32 = 76.0;
/// Below this width the menus and the search button hide (`st-hide-md`).
const NARROW: f32 = 960.0;

/// The title bar's run controls (S071, option A of the canvas's run
/// controls board): what runs before a session, the session's state
/// during one, and the icons that follow it.
pub struct RunBar<'a> {
    /// The workspace's addons, the one picked.
    pub packages: &'a [String],
    pub package: usize,
    /// The addon's flavors (those that can't run yet: disabled).
    pub flavors: &'a [(String, bool)],
    pub flavor: usize,
    /// `world: none`, then the addon's world profiles.
    pub profiles: &'a [String],
    pub profile: usize,
    pub state: RunState,
}

/// Where a session is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunState {
    Idle,
    /// Running without the debugger.
    Running,
    Debugging,
    /// Paused, and where (`ThreatMeter:66`).
    Paused(Option<String>),
}

/// A picker changed: the new pick's place in its list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunPick {
    Package(usize),
    Flavor(usize),
    Profile(usize),
}

/// A picker of the run controls: its id, its options, the one picked.
type Picker<'a> = (&'a str, Vec<(&'a str, bool)>, usize);

/// An icon of the run controls: its command, its tooltip, whether it is
/// green (run, continue), red (stop) or greyed.
struct Tool {
    icon: Icon,
    label: &'static str,
    command: Command,
    tone: Tone,
    enabled: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tone {
    Plain,
    Go,
    Stop,
}

fn tools(state: &RunState) -> Vec<Tool> {
    let t = |icon, label, command, tone, enabled| Tool {
        icon,
        label,
        command,
        tone,
        enabled,
    };
    let session = |paused: bool| {
        vec![
            t(
                Icon::StepOver,
                "step over, f10",
                Command::StepOver,
                Tone::Plain,
                paused,
            ),
            t(
                Icon::StepInto,
                "step into, f11",
                Command::StepInto,
                Tone::Plain,
                paused,
            ),
            t(
                Icon::StepOut,
                "step out, shift f11",
                Command::StepOut,
                Tone::Plain,
                paused,
            ),
        ]
    };
    let tail = || {
        vec![
            t(
                Icon::Reload,
                "/reload, ctrl r",
                Command::ReloadSession,
                Tone::Plain,
                true,
            ),
            t(
                Icon::Restart,
                "restart",
                Command::RestartSession,
                Tone::Plain,
                true,
            ),
            t(
                Icon::Stop,
                "stop, shift f5",
                Command::StopSession,
                Tone::Stop,
                true,
            ),
        ]
    };
    match state {
        RunState::Idle => vec![
            t(Icon::Run, "run, ctrl f5", Command::RunAddon, Tone::Go, true),
            t(
                Icon::Debug,
                "debug, f5",
                Command::StartDebugging,
                Tone::Plain,
                true,
            ),
        ],
        RunState::Running => tail(),
        RunState::Debugging => {
            let mut v = vec![t(
                Icon::Pause,
                "pause, f6",
                Command::Pause,
                Tone::Plain,
                true,
            )];
            v.extend(session(false));
            v.extend(tail());
            v
        }
        RunState::Paused(_) => {
            let mut v = vec![t(
                Icon::Run,
                "continue, f5",
                Command::StartDebugging,
                Tone::Go,
                true,
            )];
            v.extend(session(true));
            v.extend(tail());
            v
        }
    }
}

/// The run controls from `right` leftwards: the icons, the divider, then
/// the pickers (before a run) or the session's state; where they start.
/// The pickers are left out first when room is short (`keep` for search).
fn run_controls(
    ui: &mut Ui,
    rect: Rect,
    left_limit: f32,
    right: f32,
    run: &RunBar<'_>,
    commands: &mut Vec<Command>,
    picked: &mut Option<RunPick>,
) -> f32 {
    let p = palette(ui.ctx());
    let cy = rect.center().y;
    let list = tools(&run.state);
    let mut x = right;
    for (i, tool) in list.iter().enumerate().rev() {
        let r = Rect::from_min_size(Pos2::new(x - 28.0, cy - 14.0), Vec2::splat(28.0));
        let resp = ui.interact(
            r,
            Id::unique(("wowlua-studio-run-tool", i, tool.label)),
            if tool.enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        resp.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Button, tool.enabled, tool.label)
        });
        if tool.enabled && resp.hovered() {
            ui.painter().rect_filled(r, CornerRadius::ZERO, p.bg_300);
        }
        let color = match (tool.enabled, tool.tone) {
            (false, _) => p.ink_faint,
            (true, Tone::Go) => p.success,
            (true, Tone::Stop) => p.error,
            (true, Tone::Plain) if resp.hovered() => p.ink,
            (true, Tone::Plain) => p.ink_muted,
        };
        icons::paint(
            ui.painter(),
            Rect::from_center_size(r.center(), Vec2::splat(16.0)),
            tool.icon,
            color,
        );
        if resp.on_hover_text(tool.label).clicked() && tool.enabled {
            commands.push(tool.command);
        }
        x = r.left() - 2.0;
    }
    // The divider before the icons.
    x -= 4.0;
    ui.painter()
        .vline(x, (cy - 10.0)..=(cy + 10.0), Stroke::new(1.0, p.line));
    x -= 8.0;
    match &run.state {
        RunState::Idle => {
            // The pickers, when the search keeps room beside them.
            let widths = [100.0, 100.0, 150.0];
            let total: f32 = widths.iter().sum::<f32>() + 12.0;
            if x - total < left_limit {
                return x;
            }
            let lists: [Picker<'_>; 3] = [
                (
                    "wowlua-studio-run-package",
                    run.packages.iter().map(|s| (s.as_str(), true)).collect(),
                    run.package,
                ),
                (
                    "wowlua-studio-run-flavor",
                    run.flavors
                        .iter()
                        .map(|(s, ok)| (s.as_str(), *ok))
                        .collect(),
                    run.flavor,
                ),
                (
                    "wowlua-studio-run-profile",
                    run.profiles.iter().map(|s| (s.as_str(), true)).collect(),
                    run.profile,
                ),
            ];
            let mut left = x - total;
            for (k, ((id, opts, at), w)) in lists.into_iter().zip(widths).enumerate() {
                let r = Rect::from_min_size(Pos2::new(left, cy - 14.0), Vec2::new(w, 28.0));
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r));
                let mut index = at;
                if forms::select_sized(&mut child, id, &opts, &mut index, w, 28.0) {
                    *picked = Some(match k {
                        0 => RunPick::Package(index),
                        1 => RunPick::Flavor(index),
                        _ => RunPick::Profile(index),
                    });
                }
                left = r.right() + 6.0;
            }
            x - total
        }
        state => {
            // The session's state, where the pickers were.
            let (word, dot) = match state {
                RunState::Running => ("running".to_owned(), p.success),
                RunState::Debugging => ("debugging".to_owned(), p.amber),
                RunState::Paused(Some(at)) => (format!("paused · {at}"), p.amber),
                _ => ("paused".to_owned(), p.amber),
            };
            let label = widgets::galley(ui, &word, Face::Plex.at(11.5), p.ink, 0.0);
            let w = label.size().x + 30.0;
            if x - w < left_limit {
                return x;
            }
            let r = Rect::from_min_size(Pos2::new(x - w, cy - 12.0), Vec2::new(w, 24.0));
            ui.painter().rect_stroke(
                r,
                CornerRadius::ZERO,
                Stroke::new(1.0, p.line),
                StrokeKind::Inside,
            );
            ui.painter()
                .circle_filled(Pos2::new(r.left() + 12.0, cy), 3.5, dot);
            ui.painter().galley(
                Pos2::new(r.left() + 22.0, cy - label.size().y / 2.0),
                label,
                p.ink,
            );
            r.left()
        }
    }
}

/// The title bar: the app, the menus (not on macOS: its menu bar has
/// them), search, the run controls and the window's buttons. The start
/// page (no workspace) has neither search nor run controls; the folder and
/// its branch are in the status bar (S071).
pub fn title_bar(
    ui: &mut Ui,
    rect: Rect,
    platform: Platform,
    entries: &[Entry],
    run: Option<RunBar<'_>>,
    commands: &mut Vec<Command>,
    picked: &mut Option<RunPick>,
) {
    let ctx = ui.ctx().clone();
    let p = palette(&ctx);
    let painter = ui.painter().clone();
    painter.rect_filled(rect, CornerRadius::ZERO, p.bg_200);
    painter.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, p.line),
    );
    let mac = platform == Platform::MacOs;
    let narrow = rect.width() < NARROW;

    // The bar moves the window; a double click maximizes it.
    let bg = ui.interact(
        rect,
        Id::unique("wowlua-studio-title"),
        Sense::click_and_drag(),
    );
    if bg.drag_started() {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if bg.double_clicked() {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    }

    let cy = rect.center().y;
    let mut x = rect.left() + if mac { TRAFFIC_LIGHTS_W } else { 10.0 };

    // The app: the mark at 20 px (S023) and the name.
    let mark_rect = Rect::from_min_size(Pos2::new(x + 6.0, cy - 10.0), Vec2::splat(20.0));
    mark::paint_mark(&ctx, &painter, mark_rect, p);
    let name = widgets::galley(ui, "wowlua studio", Face::PlexSemiBold.at(12.5), p.ink, 0.0);
    let name_x = mark_rect.right() + 8.0;
    painter.galley(
        Pos2::new(name_x, cy - name.size().y / 2.0),
        name.clone(),
        p.ink,
    );
    x = name_x + name.size().x + 14.0;

    // The menus; a menu with no commands yet is left out.
    if !mac && !narrow {
        for menu in Menu::ALL {
            let items: Vec<&Entry> = entries.iter().filter(|e| e.menu == Some(menu)).collect();
            if items.is_empty() {
                continue;
            }
            let label = widgets::galley(ui, menu.label(), Face::Plex.at(12.0), p.ink_muted, 0.0);
            let r = Rect::from_min_size(
                Pos2::new(x, cy - 14.0),
                Vec2::new(label.size().x + 16.0, 28.0),
            );
            let resp = ui.interact(
                r,
                Id::unique(("wowlua-studio-menu", menu.label())),
                Sense::click(),
            );
            let open = egui::Popup::is_id_open(&ctx, egui::Popup::default_response_id(&resp));
            let fg = if resp.hovered() || open {
                p.ink
            } else {
                p.ink_muted
            };
            if resp.hovered() || open {
                painter.rect_filled(r, CornerRadius::ZERO, p.bg_300);
            }
            painter.galley(r.center() - label.size() / 2.0, label, fg);
            egui::Popup::menu(&resp).show(|ui| {
                ui.set_min_width(240.0);
                for e in &items {
                    if menu_row(ui, e, platform) {
                        commands.push(e.command);
                    }
                }
            });
            x = r.right();
        }
        x += 6.0;
    }

    let has_workspace = run.is_some();

    // The window's buttons (macOS draws its own).
    let buttons_w = if mac { 0.0 } else { 3.0 * 44.0 };
    let right = rect.right() - buttons_w;
    if !mac {
        window_buttons(
            ui,
            Rect::from_min_max(
                Pos2::new(right, rect.top()),
                Pos2::new(rect.right(), rect.bottom() - 1.0),
            ),
        );
    }

    // The run controls at the right; search in what is left.
    let mut right = right;
    if let Some(run) = &run {
        right = run_controls(ui, rect, x + 220.0, right - 8.0, run, commands, picked);
    }
    // Search: up to 420 px, centred in the window where it fits.
    if !narrow && has_workspace {
        let room = (right - 12.0) - x;
        let w = room.min(420.0);
        if w >= 200.0 {
            let mut left = rect.center().x - w / 2.0;
            left = left.clamp(x, right - 12.0 - w);
            let r = Rect::from_min_size(Pos2::new(left, cy - 13.0), Vec2::new(w, 26.0));
            if search_button(ui, r, platform) {
                commands.push(Command::OpenPalette);
            }
        }
    }
}

/// A menu's row: the command and its keys.
fn menu_row(ui: &mut Ui, entry: &Entry, platform: Platform) -> bool {
    let p = palette(ui.ctx());
    let width = ui.available_width().max(240.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 26.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_300);
    }
    ui.painter().text(
        Pos2::new(rect.left() + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        &entry.title,
        Face::Plex.at(12.5),
        p.ink,
    );
    if let Some(key) = &entry.key {
        ui.painter().text(
            Pos2::new(rect.right() - 8.0, rect.center().y),
            Align2::RIGHT_CENTER,
            key_words(key, platform).join(" "),
            Face::Plex.at(11.5),
            p.ink_muted,
        );
    }
    let clicked = resp.clicked();
    if clicked {
        ui.close();
    }
    clicked
}

fn search_button(ui: &mut Ui, r: Rect, platform: Platform) -> bool {
    let p = palette(ui.ctx());
    let resp = ui.interact(r, Id::unique("wowlua-studio-search"), Sense::click());
    let painter = ui.painter();
    painter.rect_filled(
        r,
        CornerRadius::ZERO,
        if resp.hovered() { p.bg_300 } else { p.bg_000 },
    );
    painter.rect_stroke(
        r,
        CornerRadius::ZERO,
        Stroke::new(1.0, p.line_strong),
        StrokeKind::Inside,
    );
    let cy = r.center().y;
    painter.text(
        Pos2::new(r.left() + 8.0, cy),
        Align2::LEFT_CENTER,
        "/",
        Face::PlexSemiBold.at(12.0),
        p.amber_ink,
    );
    painter.text(
        Pos2::new(r.left() + 24.0, cy),
        Align2::LEFT_CENTER,
        "search files, symbols, commands",
        Face::Plex.at(12.0),
        p.ink_muted,
    );
    let mod_word = if platform == Platform::MacOs {
        "cmd"
    } else {
        "ctrl"
    };
    widgets::paint_keys_right(ui, r.right() - 6.0, cy, &[mod_word, "k"]);
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Button,
            true,
            "search files, symbols and commands",
        )
    });
    resp.on_hover_cursor(CursorIcon::PointingHand).clicked()
}

fn window_buttons(ui: &mut Ui, rect: Rect) {
    let ctx = ui.ctx().clone();
    let p = palette(&ctx);
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    let buttons = [
        (
            Icon::Minimize,
            "minimize",
            ViewportCommand::Minimized(true),
            false,
        ),
        (
            Icon::Maximize,
            if maximized { "restore" } else { "maximize" },
            ViewportCommand::Maximized(!maximized),
            false,
        ),
        (Icon::Close, "close", ViewportCommand::Close, true),
    ];
    for (i, (icon, label, command, close)) in buttons.into_iter().enumerate() {
        let r = Rect::from_min_size(
            Pos2::new(rect.left() + 44.0 * i as f32, rect.top()),
            Vec2::new(44.0, rect.height()),
        );
        let resp = ui.interact(r, Id::unique(("wowlua-studio-winbtn", i)), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
        let (bg, fg) = match (resp.hovered(), close) {
            (true, true) => (p.error, p.on_error),
            (true, false) => (p.bg_300, p.ink),
            _ => (Color32::TRANSPARENT, p.ink_muted),
        };
        ui.painter().rect_filled(r, CornerRadius::ZERO, bg);
        icons::paint(
            ui.painter(),
            Rect::from_center_size(r.center(), Vec2::splat(14.0)),
            icon,
            fg,
        );
        if resp.clicked() {
            ctx.send_viewport_cmd(command);
        }
    }
}

/// The rail: a page per item, settings at the bottom.
pub fn rail(ui: &mut Ui, rect: Rect, current: Page, commands: &mut Vec<Command>) {
    let p = palette(ui.ctx());
    let painter = ui.painter().clone();
    painter.rect_filled(rect, CornerRadius::ZERO, p.bg_200);
    painter.vline(rect.right() - 0.5, rect.y_range(), Stroke::new(1.0, p.line));
    let mut y = rect.top() + 2.0;
    for page in Page::RAIL {
        rail_item(
            ui,
            Rect::from_min_size(
                Pos2::new(rect.left(), y),
                Vec2::new(rect.width() - 1.0, 50.0),
            ),
            page,
            current,
            commands,
        );
        y += 50.0;
    }
    let settings = Rect::from_min_size(
        Pos2::new(rect.left(), rect.bottom() - 52.0),
        Vec2::new(rect.width() - 1.0, 50.0),
    );
    if settings.top() >= y {
        rail_item(ui, settings, Page::Settings, current, commands);
    }
}

fn rail_item(ui: &mut Ui, r: Rect, page: Page, current: Page, commands: &mut Vec<Command>) {
    let p = palette(ui.ctx());
    let resp = ui.interact(r, Id::unique(("wowlua-studio-rail", page)), Sense::click());
    let on = page == current;
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, on, page.label()));
    let fg = if on {
        p.amber_ink
    } else if resp.hovered() {
        p.ink
    } else {
        p.ink_muted
    };
    let painter = ui.painter();
    if on {
        painter.rect_filled(r, CornerRadius::ZERO, p.amber_soft);
        painter.rect_filled(
            Rect::from_min_size(r.min, Vec2::new(2.0, r.height())),
            CornerRadius::ZERO,
            p.amber,
        );
    } else if resp.hovered() {
        painter.rect_filled(r, CornerRadius::ZERO, p.bg_300);
    }
    let (font, spacing) = widgets::label_font(11.0);
    let label = widgets::galley(ui, page.label(), font, fg, spacing);
    let block = 16.0 + 3.0 + label.size().y;
    let top = r.center().y - block / 2.0;
    icons::paint(
        painter,
        Rect::from_min_size(Pos2::new(r.center().x - 8.0, top), Vec2::splat(16.0)),
        page.icon(),
        fg,
    );
    painter.galley(
        Pos2::new(r.center().x - label.size().x / 2.0, top + 19.0),
        label,
        fg,
    );
    if page.planned() {
        // A dashed ring marks planned work.
        let c = Pos2::new(r.right() - 10.0, r.top() + 9.0);
        painter.circle_stroke(c, 3.0, Stroke::new(1.0, p.ink_muted));
    }
    let tip = if page.planned() {
        format!("{} · planned", page.label())
    } else {
        page.label().to_owned()
    };
    let resp = resp
        .on_hover_text(tip)
        .on_hover_cursor(CursorIcon::PointingHand);
    if resp.clicked() {
        commands.push(Command::ShowPage(page));
    }
}

/// What the status bar says about the session.
pub struct Session<'a> {
    pub text: &'a str,
    /// Said as a warning (`checks off`).
    pub warning: bool,
    /// At the right: the flavor, the client.
    pub right: &'a [String],
    /// Room kept at the right end (the notifications' tray).
    pub inset: f32,
    /// Room kept after wowlua's version (the branch, M11).
    pub lead: f32,
    /// The workspace's folder: its name, its path for the tooltip (S071).
    pub folder: Option<(&'a str, String)>,
}

/// The status bar: wowlua's version and the session, and at its right
/// the flavor and the client. The theme is a setting, not here (S021).
/// Returns where the room after the version starts (the branch's) and
/// where the session's text ends: the background jobs and the problems
/// follow it.
pub fn status_bar(
    ui: &mut Ui,
    rect: Rect,
    wowlua_version: &str,
    session: &Session<'_>,
) -> (f32, f32) {
    let p = palette(ui.ctx());
    let painter = ui.painter().clone();
    painter.rect_filled(rect, CornerRadius::ZERO, p.bg_200);
    painter.hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0, p.line));
    let cy = rect.center().y;
    let font = Face::Plex.at(11.5);
    let mut x = rect.left() + 10.0;
    painter.circle_filled(Pos2::new(x + 3.0, cy), 3.0, p.success);
    x += 12.0;
    let r = painter.text(
        Pos2::new(x, cy),
        Align2::LEFT_CENTER,
        format!("wowlua {wowlua_version}"),
        font.clone(),
        p.success,
    );
    x = r.right() + 14.0;
    // The workspace's folder.
    if let Some((name, path)) = &session.folder {
        icons::paint(
            &painter,
            Rect::from_min_size(Pos2::new(x, cy - 7.0), Vec2::splat(14.0)),
            Icon::Folder,
            p.ink_muted,
        );
        let r = painter.text(
            Pos2::new(x + 20.0, cy),
            Align2::LEFT_CENTER,
            *name,
            font.clone(),
            p.ink,
        );
        let hover = Rect::from_min_max(
            Pos2::new(x, rect.top()),
            Pos2::new(r.right(), rect.bottom()),
        );
        ui.interact(
            hover,
            Id::unique("wowlua-studio-status-folder"),
            Sense::hover(),
        )
        .on_hover_text(format!("the workspace's folder: {path}"));
        x = r.right() + 14.0;
    }
    let lead = x - 6.0;
    if session.lead > 0.0 {
        x += session.lead + 8.0;
    }
    let r = painter.text(
        Pos2::new(x, cy),
        Align2::LEFT_CENTER,
        session.text,
        font.clone(),
        if session.warning {
            p.warning
        } else {
            p.ink_muted
        },
    );
    let end = r.right();
    if rect.width() < NARROW {
        return (lead, end);
    }
    let mut right = rect.right() - 10.0 - session.inset;
    for item in session.right.iter().rev() {
        let r = painter.text(
            Pos2::new(right, cy),
            Align2::RIGHT_CENTER,
            item,
            font.clone(),
            p.ink_muted,
        );
        right = r.left() - 14.0;
    }
    (lead, end)
}

/// The workspace's errors and warnings in the status bar, from `x`.
pub fn problems(ui: &Ui, x: f32, cy: f32, errors: usize, warnings: usize) {
    let p = palette(ui.ctx());
    let painter = ui.painter();
    let font = Face::Plex.at(11.5);
    let plural =
        |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let e = painter.text(
        Pos2::new(x, cy),
        Align2::LEFT_CENTER,
        plural(errors, "error", "errors"),
        font.clone(),
        if errors > 0 { p.error } else { p.ink_muted },
    );
    let dot = painter.text(
        Pos2::new(e.right(), cy),
        Align2::LEFT_CENTER,
        " · ",
        font.clone(),
        p.ink_muted,
    );
    painter.text(
        Pos2::new(dot.right(), cy),
        Align2::LEFT_CENTER,
        plural(warnings, "warning", "warnings"),
        font,
        if warnings > 0 { p.warning } else { p.ink_muted },
    );
}

/// Resizing a window without the system's frame: 5 px edges, 10 px
/// corners (Windows and Linux).
pub fn resize_edges(ui: &mut Ui, rect: Rect) {
    let ctx = ui.ctx().clone();
    if ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
        return;
    }
    let e = 5.0;
    let c = 10.0;
    let zones = [
        (
            Rect::from_min_size(rect.min, Vec2::splat(c)),
            ResizeDirection::NorthWest,
            CursorIcon::ResizeNorthWest,
        ),
        (
            Rect::from_min_size(Pos2::new(rect.right() - c, rect.top()), Vec2::splat(c)),
            ResizeDirection::NorthEast,
            CursorIcon::ResizeNorthEast,
        ),
        (
            Rect::from_min_size(Pos2::new(rect.left(), rect.bottom() - c), Vec2::splat(c)),
            ResizeDirection::SouthWest,
            CursorIcon::ResizeSouthWest,
        ),
        (
            Rect::from_min_size(rect.max - Vec2::splat(c), Vec2::splat(c)),
            ResizeDirection::SouthEast,
            CursorIcon::ResizeSouthEast,
        ),
        (
            Rect::from_min_max(rect.min, Pos2::new(rect.right(), rect.top() + e)),
            ResizeDirection::North,
            CursorIcon::ResizeNorth,
        ),
        (
            Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - e), rect.max),
            ResizeDirection::South,
            CursorIcon::ResizeSouth,
        ),
        (
            Rect::from_min_max(rect.min, Pos2::new(rect.left() + e, rect.bottom())),
            ResizeDirection::West,
            CursorIcon::ResizeWest,
        ),
        (
            Rect::from_min_max(Pos2::new(rect.right() - e, rect.top()), rect.max),
            ResizeDirection::East,
            CursorIcon::ResizeEast,
        ),
    ];
    for (i, (r, dir, cursor)) in zones.into_iter().enumerate() {
        let resp = ui.interact(r, Id::unique(("wowlua-studio-resize", i)), Sense::drag());
        if resp.hovered() || resp.dragged() {
            ctx.set_cursor_icon(cursor);
        }
        if resp.drag_started() {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
        }
    }
}
