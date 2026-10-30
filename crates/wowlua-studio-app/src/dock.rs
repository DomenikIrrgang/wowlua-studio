//! Docking (the design's panels board): four docks (left, main, bottom,
//! right), each a run of groups of tabbed panels. A panel moves by its tab
//! or grip to a drop zone, by its menu, or floats in a window of its own.
//! A layout is kept per page, from the page's preset (its artboard).

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, Id, LayerId, Order, Pos2, Rect, Sense, Shape,
    Stroke, StrokeKind, Ui, UiBuilder, Vec2, ViewportBuilder, ViewportClass, ViewportId,
};
use serde::{Deserialize, Serialize};
use wowlua_studio_ui::{Face, Icon, icons, palette, widgets};

use crate::{pages::Page, panels::Panel};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Dock {
    Left,
    Main,
    Bottom,
    Right,
}

impl Dock {
    pub const ALL: [Dock; 4] = [Dock::Left, Dock::Main, Dock::Bottom, Dock::Right];

    fn name(self) -> &'static str {
        match self {
            Dock::Left => "left",
            Dock::Main => "the main area",
            Dock::Bottom => "bottom",
            Dock::Right => "right",
        }
    }
}

/// Panels shown as tabs of one area; `share` is its part of the dock.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub tabs: Vec<Panel>,
    pub active: usize,
    pub share: f32,
}

impl Group {
    fn of(tabs: &[Panel]) -> Self {
        Group {
            tabs: tabs.to_vec(),
            active: 0,
            share: 1.0,
        }
    }

    fn active_panel(&self) -> Panel {
        self.tabs[self.active.min(self.tabs.len() - 1)]
    }
}

/// A panel in a window of its own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Floating {
    pub panel: Panel,
    pub size: [f32; 2],
}

/// Where every panel is, and the docks' sizes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub left: Vec<Group>,
    pub main: Vec<Group>,
    pub bottom: Vec<Group>,
    pub right: Vec<Group>,
    pub left_width: f32,
    pub right_width: f32,
    pub bottom_height: f32,
    #[serde(default)]
    pub floating: Vec<Floating>,
}

/// Where a panel goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    /// The dock's last group, or a new one.
    Dock(Dock),
    /// A new group at the end of the dock.
    Split(Dock),
    /// A window of its own.
    Float,
}

impl Layout {
    /// The page's preset: the panels its artboard draws, where it draws them.
    pub fn preset(page: Page) -> Self {
        use Panel::*;
        let g = Group::of;
        let (left, main, bottom, right) = match page {
            Page::Files => (
                vec![g(&[Explorer, Outline])],
                vec![g(&[Editor])],
                vec![g(&[Problems, Output, Terminal])],
                vec![],
            ),
            Page::Search => (vec![g(&[Search])], vec![g(&[Editor])], vec![], vec![]),
            Page::Git => (
                vec![g(&[Changes]), g(&[History])],
                vec![g(&[Diff])],
                vec![],
                vec![],
            ),
            Page::Prs => (
                vec![g(&[PullRequests])],
                vec![g(&[PullRequest])],
                vec![],
                vec![],
            ),
            Page::Debug => (
                vec![g(&[CallStack]), g(&[Breakpoints])],
                vec![g(&[Editor])],
                vec![g(&[Console, Output, LiveWorld, Game])],
                vec![g(&[Variables]), g(&[Watch])],
            ),
            Page::Tests => (
                vec![g(&[TestExplorer])],
                vec![g(&[TestResults, Coverage])],
                vec![],
                vec![],
            ),
            Page::Game => (
                vec![g(&[Frames, Units])],
                vec![g(&[Game])],
                vec![g(&[Events, Input, Screenshots])],
                vec![g(&[Inspector, LiveWorld])],
            ),
            Page::Perf => (vec![], vec![g(&[Profiler])], vec![], vec![g(&[Function])]),
            Page::World => (
                vec![g(&[Profiles]), g(&[Parts])],
                vec![g(&[ProfileEditor])],
                vec![],
                vec![g(&[LiveWorld])],
            ),
            Page::Ship => (vec![g(&[BuildProfiles])], vec![g(&[Ship])], vec![], vec![]),
            Page::Claude => (
                vec![g(&[Conversations])],
                vec![g(&[Chat])],
                vec![],
                vec![g(&[Tools])],
            ),
            Page::Settings => (
                vec![g(&[SettingsSections])],
                vec![g(&[Settings])],
                vec![],
                vec![],
            ),
        };
        // The overview board's anatomy: a 220 px side panel, a 170 px
        // bottom panel and a 250 px inspector; the tests board's explorer
        // is 330 px, for the tests' names.
        Layout {
            left,
            main,
            bottom,
            right,
            left_width: match page {
                Page::Tests => 330.0,
                Page::Git | Page::Prs => 320.0,
                _ => 220.0,
            },
            right_width: 250.0,
            bottom_height: 170.0,
            floating: Vec::new(),
        }
    }

    pub fn dock(&self, dock: Dock) -> &Vec<Group> {
        match dock {
            Dock::Left => &self.left,
            Dock::Main => &self.main,
            Dock::Bottom => &self.bottom,
            Dock::Right => &self.right,
        }
    }

    fn dock_mut(&mut self, dock: Dock) -> &mut Vec<Group> {
        match dock {
            Dock::Left => &mut self.left,
            Dock::Main => &mut self.main,
            Dock::Bottom => &mut self.bottom,
            Dock::Right => &mut self.right,
        }
    }

    /// Whether the panel is anywhere in the layout.
    pub fn contains(&self, panel: Panel) -> bool {
        self.floating.iter().any(|f| f.panel == panel)
            || Dock::ALL
                .iter()
                .any(|&d| self.dock(d).iter().any(|g| g.tabs.contains(&panel)))
    }

    /// The dock holding the panel, if it is docked.
    pub fn dock_of(&self, panel: Panel) -> Option<Dock> {
        Dock::ALL
            .into_iter()
            .find(|&d| self.dock(d).iter().any(|g| g.tabs.contains(&panel)))
    }

    /// Takes the panel out of the layout; empty groups go with it.
    pub fn remove(&mut self, panel: Panel) {
        self.floating.retain(|f| f.panel != panel);
        for dock in Dock::ALL {
            let groups = self.dock_mut(dock);
            for g in groups.iter_mut() {
                if let Some(i) = g.tabs.iter().position(|&p| p == panel) {
                    g.tabs.remove(i);
                    if g.active > i || g.active >= g.tabs.len() {
                        g.active = g.active.saturating_sub(1);
                    }
                }
            }
            groups.retain(|g| !g.tabs.is_empty());
        }
    }

    /// Moves the panel to `target`, as the shown tab there.
    pub fn place(&mut self, panel: Panel, target: Target) {
        self.remove(panel);
        match target {
            Target::Dock(dock) => match self.dock_mut(dock).last_mut() {
                Some(g) => {
                    g.tabs.push(panel);
                    g.active = g.tabs.len() - 1;
                }
                None => self.dock_mut(dock).push(Group::of(&[panel])),
            },
            Target::Split(dock) => self.dock_mut(dock).push(Group::of(&[panel])),
            Target::Float => self.floating.push(Floating {
                panel,
                size: [480.0, 360.0],
            }),
        }
    }

    /// Shows the panel's tab, docking it at `home` if the layout lacks it.
    pub fn reveal(&mut self, panel: Panel, home: Dock) {
        if !self.contains(panel) {
            self.place(panel, Target::Dock(home));
            return;
        }
        for dock in Dock::ALL {
            for g in self.dock_mut(dock) {
                if let Some(i) = g.tabs.iter().position(|&p| p == panel) {
                    g.active = i;
                }
            }
        }
    }
}

/// What the dock remembers between frames, never saved.
#[derive(Default)]
pub struct DockState {
    /// The panel being dragged.
    pub dragging: Option<Panel>,
    /// The panel shown alone over every dock.
    pub maximized: Option<Panel>,
    /// The panel last clicked: what `ctrl shift m` moves.
    pub focused: Option<Panel>,
    /// Open this panel's menu next frame.
    pub open_menu: Option<Panel>,
}

/// Draws one panel's content.
pub type Content<'a> = dyn FnMut(&mut Ui, Panel) + 'a;

const HEAD: f32 = 32.0;
const SPLIT_HIT: f32 = 6.0;

/// Draws the layout into `area`; returns whether the layout changed.
pub fn show(
    ui: &mut Ui,
    area: Rect,
    layout: &mut Layout,
    state: &mut DockState,
    content: &mut Content<'_>,
) -> bool {
    let before = layout.clone();
    let mut actions = Vec::new();

    if let Some(panel) = state.maximized {
        if layout.contains(panel) {
            let group = Group::of(&[panel]);
            draw_group(
                ui,
                area,
                Dock::Main,
                None,
                &group,
                state,
                content,
                &mut actions,
            );
        } else {
            state.maximized = None;
        }
    } else {
        draw_docks(ui, area, layout, state, content, &mut actions);
    }
    draw_floating(ui, layout, state, content, &mut actions);
    if state.dragging.is_some() {
        drop_zones(ui, area, state, &mut actions);
    }
    for action in actions {
        apply(layout, state, action);
    }
    *layout != before
}

enum Action {
    Activate(Dock, usize, usize),
    Place(Panel, Target),
    Close(Panel),
    ToggleMaximize(Panel),
    Resize(Dock, f32),
    Share(Dock, usize, f32),
    FloatSize(Panel, [f32; 2]),
}

fn apply(layout: &mut Layout, state: &mut DockState, action: Action) {
    match action {
        Action::Activate(dock, g, t) => {
            if let Some(group) = layout.dock_mut(dock).get_mut(g) {
                group.active = t;
                state.focused = group.tabs.get(t).copied();
            }
        }
        Action::Place(panel, target) => {
            layout.place(panel, target);
            state.focused = Some(panel);
        }
        Action::Close(panel) => {
            layout.remove(panel);
            if state.maximized == Some(panel) {
                state.maximized = None;
            }
        }
        Action::ToggleMaximize(panel) => {
            state.maximized = if state.maximized == Some(panel) {
                None
            } else {
                Some(panel)
            };
        }
        Action::Resize(dock, size) => match dock {
            Dock::Left => layout.left_width = size,
            Dock::Right => layout.right_width = size,
            Dock::Bottom => layout.bottom_height = size,
            Dock::Main => {}
        },
        Action::Share(dock, i, delta) => {
            let groups = layout.dock_mut(dock);
            if i + 1 < groups.len() {
                let total = groups[i].share + groups[i + 1].share;
                let a = (groups[i].share + delta).clamp(0.15 * total, 0.85 * total);
                groups[i].share = a;
                groups[i + 1].share = total - a;
            }
        }
        Action::FloatSize(panel, size) => {
            if let Some(f) = layout.floating.iter_mut().find(|f| f.panel == panel) {
                f.size = size;
            }
        }
    }
}

fn draw_docks(
    ui: &mut Ui,
    area: Rect,
    layout: &Layout,
    state: &mut DockState,
    content: &mut Content<'_>,
    actions: &mut Vec<Action>,
) {
    let p = palette(ui.ctx());
    let max_side = (area.width() * 0.4).max(180.0);
    let lw = if layout.left.is_empty() {
        0.0
    } else {
        layout.left_width.clamp(160.0, max_side)
    };
    let rw = if layout.right.is_empty() {
        0.0
    } else {
        layout.right_width.clamp(180.0, max_side)
    };
    let bh = if layout.bottom.is_empty() {
        0.0
    } else {
        layout
            .bottom_height
            .clamp(80.0, (area.height() * 0.6).max(80.0))
    };
    let left = Rect::from_min_max(area.min, Pos2::new(area.left() + lw, area.bottom()));
    let right = Rect::from_min_max(Pos2::new(area.right() - rw, area.top()), area.max);
    let middle = Rect::from_min_max(
        Pos2::new(left.right(), area.top()),
        Pos2::new(right.left(), area.bottom()),
    );
    let bottom = Rect::from_min_max(Pos2::new(middle.left(), middle.bottom() - bh), middle.max);
    let main = Rect::from_min_max(middle.min, Pos2::new(middle.right(), bottom.top()));

    ui.painter().rect_filled(main, CornerRadius::ZERO, p.bg_000);
    for (dock, rect) in [
        (Dock::Left, left),
        (Dock::Main, main),
        (Dock::Bottom, bottom),
        (Dock::Right, right),
    ] {
        let groups = layout.dock(dock);
        if groups.is_empty() || rect.width() < 1.0 || rect.height() < 1.0 {
            continue;
        }
        draw_dock(ui, dock, rect, groups, state, content, actions);
    }

    // The splitters: a 1 px line, a wider hit area.
    let line = Stroke::new(1.0, p.line);
    if lw > 0.0 {
        let x = left.right();
        ui.painter().vline(x - 0.5, area.y_range(), line);
        if let Some(d) = splitter(
            ui,
            "split-left",
            Rect::from_x_y_ranges(x - SPLIT_HIT / 2.0..=x + SPLIT_HIT / 2.0, area.y_range()),
            true,
        ) {
            actions.push(Action::Resize(Dock::Left, lw + d));
        }
    }
    if rw > 0.0 {
        let x = right.left();
        ui.painter().vline(x + 0.5, area.y_range(), line);
        if let Some(d) = splitter(
            ui,
            "split-right",
            Rect::from_x_y_ranges(x - SPLIT_HIT / 2.0..=x + SPLIT_HIT / 2.0, area.y_range()),
            true,
        ) {
            actions.push(Action::Resize(Dock::Right, rw - d));
        }
    }
    if bh > 0.0 {
        let y = bottom.top();
        ui.painter().hline(middle.x_range(), y + 0.5, line);
        if let Some(d) = splitter(
            ui,
            "split-bottom",
            Rect::from_x_y_ranges(middle.x_range(), y - SPLIT_HIT / 2.0..=y + SPLIT_HIT / 2.0),
            false,
        ) {
            actions.push(Action::Resize(Dock::Bottom, bh - d));
        }
    }
}

/// A draggable splitter; returns how far it moved this frame.
fn splitter(ui: &mut Ui, id: &str, rect: Rect, vertical: bool) -> Option<f32> {
    let resp = ui.interact(rect, Id::unique(("wowlua-studio-dock", id)), Sense::drag());
    let cursor = if vertical {
        CursorIcon::ResizeColumn
    } else {
        CursorIcon::ResizeRow
    };
    let resp = resp.on_hover_cursor(cursor);
    if resp.dragged() {
        let d = resp.drag_delta();
        let v = if vertical { d.x } else { d.y };
        (v != 0.0).then_some(v)
    } else {
        None
    }
}

fn draw_dock(
    ui: &mut Ui,
    dock: Dock,
    rect: Rect,
    groups: &[Group],
    state: &mut DockState,
    content: &mut Content<'_>,
    actions: &mut Vec<Action>,
) {
    let p = palette(ui.ctx());
    // Left and right stack their groups; main and bottom put them side by side.
    let stacked = matches!(dock, Dock::Left | Dock::Right);
    let total: f32 = groups.iter().map(|g| g.share.max(0.01)).sum();
    let length = if stacked { rect.height() } else { rect.width() };
    let mut at = if stacked { rect.top() } else { rect.left() };
    for (i, group) in groups.iter().enumerate() {
        let len = length * group.share.max(0.01) / total;
        let r = if stacked {
            Rect::from_x_y_ranges(rect.x_range(), at..=at + len)
        } else {
            Rect::from_x_y_ranges(at..=at + len, rect.y_range())
        };
        draw_group(ui, r, dock, Some(i), group, state, content, actions);
        at += len;
        if i + 1 < groups.len() {
            let line = Stroke::new(1.0, p.line);
            let hit = if stacked {
                ui.painter().hline(rect.x_range(), at - 0.5, line);
                Rect::from_x_y_ranges(rect.x_range(), at - SPLIT_HIT / 2.0..=at + SPLIT_HIT / 2.0)
            } else {
                ui.painter().vline(at - 0.5, rect.y_range(), line);
                Rect::from_x_y_ranges(at - SPLIT_HIT / 2.0..=at + SPLIT_HIT / 2.0, rect.y_range())
            };
            let id = format!("share-{dock:?}-{i}");
            if let Some(d) = splitter(ui, &id, hit, !stacked) {
                actions.push(Action::Share(dock, i, d / length * total));
            }
        }
    }
}

/// One group: its header (a pane head for one panel, tabs for several)
/// and the shown panel's content.
#[allow(clippy::too_many_arguments)]
fn draw_group(
    ui: &mut Ui,
    rect: Rect,
    dock: Dock,
    index: Option<usize>,
    group: &Group,
    state: &mut DockState,
    content: &mut Content<'_>,
    actions: &mut Vec<Action>,
) {
    let p = palette(ui.ctx());
    let head = Rect::from_min_size(rect.min, Vec2::new(rect.width(), HEAD));
    let body = Rect::from_min_max(Pos2::new(rect.left(), head.bottom()), rect.max);
    let painter = ui.painter_at(rect);
    painter.rect_filled(head, CornerRadius::ZERO, p.bg_200);
    let body_fill = if dock == Dock::Main {
        p.bg_000
    } else {
        p.bg_200
    };
    painter.rect_filled(body, CornerRadius::ZERO, body_fill);
    painter.hline(
        head.x_range(),
        head.bottom() - 0.5,
        Stroke::new(1.0, p.line),
    );

    let active = group.active_panel();
    let menu_w = 28.0;
    let tabs_rect = Rect::from_min_max(head.min, Pos2::new(head.right() - menu_w, head.bottom()));
    let mut x = tabs_rect.left();
    let (font, spacing) = widgets::label_font(12.0);
    if group.tabs.len() == 1 {
        // A pane head: grip, title, menu.
        let resp = ui.interact(
            tabs_rect,
            Id::unique(("wowlua-studio-head", active)),
            Sense::click_and_drag(),
        );
        let grip_color = if resp.hovered() {
            p.ink_muted
        } else {
            p.ink_faint
        };
        icons::paint(
            &painter,
            Rect::from_min_size(Pos2::new(x + 6.0, head.center().y - 7.0), Vec2::splat(14.0)),
            Icon::Grip,
            grip_color,
        );
        x += 24.0;
        let title = widgets::galley(ui, active.title(), font.clone(), p.ink, spacing);
        painter.galley(
            Pos2::new(x, head.center().y - title.size().y / 2.0),
            title,
            p.ink,
        );
        tab_interaction(ui, &resp, dock, index, active, 0, state, actions);
    } else {
        for (t, &panel) in group.tabs.iter().enumerate() {
            let on = panel == active;
            let color = if on { p.ink } else { p.ink_muted };
            let label = widgets::galley(ui, panel.title(), font.clone(), color, spacing);
            let w = label.size().x + 20.0;
            if x + w > tabs_rect.right() {
                break;
            }
            let tab = Rect::from_min_size(Pos2::new(x, head.top()), Vec2::new(w, HEAD - 1.0));
            let resp = ui.interact(
                tab,
                Id::unique(("wowlua-studio-tab", panel)),
                Sense::click_and_drag(),
            );
            if on {
                painter.rect_filled(tab, CornerRadius::ZERO, p.bg_100);
                painter.rect_filled(
                    Rect::from_min_size(tab.min, Vec2::new(w, 2.0)),
                    CornerRadius::ZERO,
                    p.amber,
                );
            } else if resp.hovered() {
                painter.rect_filled(tab, CornerRadius::ZERO, p.bg_300);
            }
            let fg = if resp.hovered() { p.ink } else { color };
            painter.galley(
                Pos2::new(x + 10.0, head.center().y - label.size().y / 2.0),
                label,
                fg,
            );
            painter.vline(tab.right() - 0.5, tab.y_range(), Stroke::new(1.0, p.line));
            tab_interaction(ui, &resp, dock, index, panel, t, state, actions);
            x += w;
        }
    }

    // The panel's menu.
    let menu_rect = Rect::from_center_size(
        Pos2::new(head.right() - menu_w / 2.0, head.center().y),
        Vec2::splat(24.0),
    );
    let menu = ui
        .scope_builder(UiBuilder::new().max_rect(menu_rect), |ui| {
            widgets::menu_button(ui, "panel menu")
        })
        .inner;
    if state.open_menu == Some(active) {
        egui::Popup::open_id(ui.ctx(), egui::Popup::default_response_id(&menu));
        state.open_menu = None;
    }
    egui::Popup::menu(&menu).show(|ui| {
        ui.set_min_width(200.0);
        for target in Dock::ALL {
            if target != dock && ui.button(format!("move to {}", target.name())).clicked() {
                actions.push(Action::Place(active, Target::Dock(target)));
            }
        }
        if group.tabs.len() > 1 && ui.button("split").clicked() {
            actions.push(Action::Place(active, Target::Split(dock)));
        }
        if ui.button("float in a window").clicked() {
            actions.push(Action::Place(active, Target::Float));
        }
        let max = if state.maximized == Some(active) {
            "restore"
        } else {
            "maximize"
        };
        if ui.button(max).clicked() {
            actions.push(Action::ToggleMaximize(active));
        }
        if ui.button("close").clicked() {
            actions.push(Action::Close(active));
        }
    });

    let inner = body;
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(inner)
            .id_salt(("wowlua-studio-body", active)),
        |ui| {
            ui.set_clip_rect(inner);
            content(ui, active);
        },
    );
}

#[allow(clippy::too_many_arguments)]
fn tab_interaction(
    ui: &Ui,
    resp: &egui::Response,
    dock: Dock,
    index: Option<usize>,
    panel: Panel,
    tab: usize,
    state: &mut DockState,
    actions: &mut Vec<Action>,
) {
    if resp.clicked() || resp.drag_started() {
        if let Some(g) = index {
            actions.push(Action::Activate(dock, g, tab));
        }
        state.focused = Some(panel);
    }
    if resp.double_clicked() {
        actions.push(Action::ToggleMaximize(panel));
    }
    if resp.drag_started() {
        state.dragging = Some(panel);
    }
    if resp.hovered() && state.dragging.is_none() {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    }
}

/// The drop zones while a panel is dragged (the panels board): left dock,
/// as a tab here, split the main area, bottom dock, right dock.
fn drop_zones(ui: &mut Ui, area: Rect, state: &mut DockState, actions: &mut Vec<Action>) {
    let Some(panel) = state.dragging else { return };
    let p = palette(ui.ctx());
    let ctx = ui.ctx().clone();
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.dragging = None;
        return;
    }
    let pointer = ctx.pointer_latest_pos();
    let m = 6.0;
    let side_w = (area.width() * 0.16).clamp(140.0, 240.0);
    let left = Rect::from_min_max(
        area.min + Vec2::splat(m),
        Pos2::new(area.left() + m + side_w, area.bottom() - m),
    );
    let right = Rect::from_min_max(
        Pos2::new(area.right() - m - side_w, area.top() + m),
        area.max - Vec2::splat(m),
    );
    let cx = (left.right() + 16.0)..=(right.left() - 16.0);
    let h = area.height() - 2.0 * m;
    let tab = Rect::from_x_y_ranges(cx.clone(), area.top() + m..=area.top() + m + h * 0.38);
    let split = Rect::from_x_y_ranges(
        cx.clone(),
        tab.bottom() + 8.0..=tab.bottom() + 8.0 + h * 0.26,
    );
    let bottom = Rect::from_x_y_ranges(cx, split.bottom() + 8.0..=area.bottom() - m);
    let zones = [
        (left, "left dock", Target::Dock(Dock::Left)),
        (tab, "as a tab here", Target::Dock(Dock::Main)),
        (split, "split the main area", Target::Split(Dock::Main)),
        (bottom, "bottom dock", Target::Dock(Dock::Bottom)),
        (right, "right dock", Target::Dock(Dock::Right)),
    ];
    let painter = ctx.layer_painter(LayerId::new(
        Order::Foreground,
        Id::unique("wowlua-studio-drop"),
    ));
    let mut hovered = None;
    for (rect, label, target) in zones {
        let over = pointer.is_some_and(|pos| rect.contains(pos));
        if over {
            hovered = Some(target);
        }
        let fill = if over {
            p.bg_300
        } else {
            p.amber_soft.gamma_multiply(0.85)
        };
        painter.rect_filled(rect, CornerRadius::ZERO, fill);
        dashed(&painter, rect.shrink(1.0), Stroke::new(2.0, p.amber_ink));
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            label,
            Face::PlexSemiBold.at(12.0),
            p.amber_ink,
        );
    }
    if let Some(pos) = pointer {
        let text = format!("{} · drop it in a dock", panel.title());
        let galley = painter.layout_no_wrap(text, Face::Plex.at(12.0), p.ink);
        let ghost = Rect::from_min_size(
            pos + Vec2::new(14.0, 14.0),
            galley.size() + Vec2::new(20.0, 12.0),
        );
        painter.rect_filled(ghost, CornerRadius::ZERO, p.bg_200);
        painter.rect_stroke(
            ghost,
            CornerRadius::ZERO,
            Stroke::new(1.0, p.amber_ink),
            StrokeKind::Inside,
        );
        painter.galley(ghost.min + Vec2::new(10.0, 6.0), galley, p.ink);
    }
    ctx.set_cursor_icon(CursorIcon::Grabbing);
    if ctx.input(|i| i.pointer.any_released()) {
        if let Some(target) = hovered {
            actions.push(Action::Place(panel, target));
        }
        state.dragging = None;
    }
}

fn dashed(painter: &egui::Painter, r: Rect, stroke: Stroke) {
    let corners = [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
        r.left_top(),
    ];
    for pair in corners.windows(2) {
        painter.extend(Shape::dashed_line(&[pair[0], pair[1]], stroke, 6.0, 4.0));
    }
}

/// Floating panels, each in a window of its own; closing the window docks
/// the panel back.
fn draw_floating(
    ui: &mut Ui,
    layout: &Layout,
    state: &mut DockState,
    content: &mut Content<'_>,
    actions: &mut Vec<Action>,
) {
    let ctx = ui.ctx().clone();
    // (the setting "floating panels as OS windows" off: windows of Studio's
    // own, S062)
    let os_windows = crate::state::prefs(&ctx).float_windows;
    for f in &layout.floating {
        let panel = f.panel;
        if !os_windows {
            let mut open = true;
            egui::Window::new(panel.title())
                .id(egui::Id::unique(("wowlua-studio-float-in", panel)))
                .default_size(f.size)
                .open(&mut open)
                .show(&ctx, |ui| {
                    let rect = ui.available_rect_before_wrap();
                    let group = Group::of(&[panel]);
                    draw_group(ui, rect, Dock::Main, None, &group, state, content, actions);
                    ui.allocate_rect(rect, egui::Sense::hover());
                });
            if !open {
                actions.push(Action::Place(panel, Target::Dock(Dock::Main)));
            }
            continue;
        }
        let builder = ViewportBuilder::default()
            .with_title(format!("{} · wowlua studio", panel.title()))
            .with_inner_size(f.size)
            .with_min_inner_size([240.0, 160.0]);
        let id = ViewportId::from_hash_of(("wowlua-studio-float", panel));
        ctx.show_viewport_immediate(id, builder, |ui, class| {
            let p = palette(ui.ctx());
            let rect = ui.max_rect();
            ui.painter().rect_filled(rect, CornerRadius::ZERO, p.bg_200);
            let group = Group::of(&[panel]);
            draw_group(ui, rect, Dock::Main, None, &group, state, content, actions);
            if class != ViewportClass::EmbeddedWindow {
                let size = ui
                    .ctx()
                    .input(|i| i.viewport().inner_rect.map(|r| r.size()));
                if let Some(size) = size
                    && (size.x - f.size[0]).abs() + (size.y - f.size[1]).abs() > 1.0
                {
                    actions.push(Action::FloatSize(panel, [size.x, size.y]));
                }
            }
            if ui.input(|i| i.viewport().close_requested()) {
                actions.push(Action::Place(panel, Target::Dock(Dock::Main)));
            }
        });
    }
}

/// What a panel shows until its milestone fills it.
pub fn placeholder(ui: &mut Ui, panel: Panel) {
    let p = palette(ui.ctx());
    let rect = ui.max_rect().shrink2(Vec2::new(16.0, 24.0));
    let text = format!(
        "{}: comes with {} of the roadmap.",
        panel.title(),
        panel.milestone()
    );
    let galley = ui.painter().layout(
        text,
        Face::Plex.at(12.5),
        p.ink_muted,
        rect.width().max(40.0),
    );
    ui.painter().galley(rect.min, galley, Color32::PLACEHOLDER);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_has_a_main_area() {
        for page in Page::ALL {
            let layout = Layout::preset(page);
            assert!(!layout.main.is_empty(), "{page:?} has no main area");
        }
    }

    #[test]
    fn a_panel_is_in_one_place_only() {
        for page in Page::ALL {
            let layout = Layout::preset(page);
            let mut seen = Vec::new();
            for dock in Dock::ALL {
                for g in layout.dock(dock) {
                    for &t in &g.tabs {
                        assert!(!seen.contains(&t), "{t:?} twice in {page:?}");
                        seen.push(t);
                    }
                }
            }
        }
    }

    #[test]
    fn moving_a_panel_keeps_the_rest() {
        let mut l = Layout::preset(Page::Debug);
        l.place(Panel::Watch, Target::Dock(Dock::Left));
        assert_eq!(l.dock_of(Panel::Watch), Some(Dock::Left));
        assert_eq!(l.right.len(), 1, "the empty group went");
        assert!(l.contains(Panel::Variables));
        l.place(Panel::Watch, Target::Split(Dock::Main));
        assert_eq!(l.main.len(), 2);
        l.place(Panel::Watch, Target::Float);
        assert_eq!(l.main.len(), 1);
        assert_eq!(l.floating.len(), 1);
        l.remove(Panel::Watch);
        assert!(!l.contains(Panel::Watch));
    }

    #[test]
    fn removing_the_shown_tab_shows_its_neighbour() {
        let mut l = Layout::preset(Page::Files);
        l.bottom[0].active = 2;
        l.remove(Panel::Terminal);
        assert_eq!(l.bottom[0].active, 1);
        assert_eq!(l.bottom[0].active_panel(), Panel::Output);
    }

    #[test]
    fn a_layout_saved_with_the_3d_world_tab_loads_without_it() {
        let mut old = Layout::preset(Page::Game);
        old.main[0].tabs.push(Panel::World3d);
        let text = serde_json::to_string(&old).unwrap();
        assert!(text.contains("world3d"));
        let mut l: Layout = serde_json::from_str(&text).unwrap();
        l.remove(Panel::World3d);
        assert!(!l.contains(Panel::World3d));
        assert!(l.contains(Panel::Game));
        assert!(!Layout::preset(Page::Game).contains(Panel::World3d));
    }

    #[test]
    fn layouts_round_trip_through_json() {
        let l = Layout::preset(Page::Game);
        let text = serde_json::to_string(&l).unwrap();
        assert_eq!(serde_json::from_str::<Layout>(&text).unwrap(), l);
    }
}
