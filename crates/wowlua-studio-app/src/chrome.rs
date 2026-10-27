//! The window's chrome (the design's title bar, rail and status bar
//! boards): a 40 px title bar, a 64 px rail, a 26 px status bar.

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, Id, Pos2, Rect, ResizeDirection, Sense, Stroke,
    StrokeKind, Ui, Vec2, ViewportCommand,
};
use wowlua_studio_ui::{Face, Icon, icons, mark, palette, widgets};

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

/// The title bar's chip: the open workspace.
pub struct Chip<'a> {
    pub name: &'a str,
    /// The Git branch checked out.
    pub branch: Option<&'a str>,
    /// Its folder, for the tooltip.
    pub folder: String,
}

/// The title bar: the app, the menus (not on macOS: its menu bar has
/// them), the workspace, search, and the window's buttons. The start page
/// (no workspace) has neither the chip nor search.
pub fn title_bar(
    ui: &mut Ui,
    rect: Rect,
    platform: Platform,
    entries: &[Entry],
    chip: Option<Chip<'_>>,
    commands: &mut Vec<Command>,
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

    // The workspace and its branch.
    let has_chip = chip.is_some();
    if let Some(chip) = chip.filter(|_| !narrow) {
        let name = widgets::galley(ui, chip.name, Face::Plex.at(12.0), p.ink, 0.0);
        let branch = chip.branch.map(|b| {
            widgets::galley(
                ui,
                &format!(" · {b}"),
                Face::Plex.at(12.0),
                p.ink_muted,
                0.0,
            )
        });
        let text_w = name.size().x + branch.as_ref().map_or(0.0, |b| b.size().x);
        let r = Rect::from_min_size(
            Pos2::new(x, cy - 13.0),
            Vec2::new(text_w + 16.0 + 20.0, 26.0),
        );
        painter.rect_filled(r, CornerRadius::ZERO, p.bg_100);
        painter.rect_stroke(
            r,
            CornerRadius::ZERO,
            Stroke::new(1.0, p.line),
            StrokeKind::Inside,
        );
        icons::paint(
            &painter,
            Rect::from_min_size(Pos2::new(r.left() + 8.0, cy - 7.0), Vec2::splat(14.0)),
            Icon::Folder,
            p.ink_muted,
        );
        let name_x = r.left() + 28.0;
        let name_w = name.size().x;
        painter.galley(Pos2::new(name_x, cy - name.size().y / 2.0), name, p.ink);
        if let Some(b) = branch {
            painter.galley(
                Pos2::new(name_x + name_w, cy - b.size().y / 2.0),
                b,
                p.ink_muted,
            );
        }
        ui.interact(r, Id::unique("wowlua-studio-chip"), Sense::hover())
            .on_hover_text(format!("a folder: {}", chip.folder));
        x = r.right() + 8.0;
    }

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

    // Search: up to 420 px, centred in the window where it fits.
    if !narrow && has_chip {
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
}

/// The status bar: wowlua's version and the session. The theme is a
/// setting, not here (S021).
pub fn status_bar(ui: &mut Ui, rect: Rect, wowlua_version: &str, session: &Session<'_>) {
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
    painter.text(
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
    if rect.width() < NARROW {
        return;
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
