//! The wowlua design system's colour tokens (its `tokens.json`), for the
//! dark and the light theme, and egui's visuals made from them.

use egui::{Color32, CornerRadius, Shadow, Stroke, Visuals, style::Selection};

/// The two themes; dark is the primary one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

impl Theme {
    pub fn palette(self) -> &'static Palette {
        match self {
            Theme::Dark => &DARK,
            Theme::Light => &LIGHT,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Theme::Dark => "dark",
            Theme::Light => "light",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "dark" => Some(Theme::Dark),
            "light" => Some(Theme::Light),
            _ => None,
        }
    }
}

/// One theme's colours, named as the tokens are (`bg-000` is `bg_000`).
#[derive(Debug)]
pub struct Palette {
    /// Sunk wells: code, terminals, inputs.
    pub bg_000: Color32,
    /// The window's ground.
    pub bg_100: Color32,
    /// Panels: the title bar, the rail, docks, menus, dialogs.
    pub bg_200: Color32,
    /// Raised and hovered.
    pub bg_300: Color32,
    /// Pressed and selected.
    pub bg_400: Color32,
    pub line: Color32,
    pub line_strong: Color32,
    pub ink: Color32,
    pub ink_muted: Color32,
    pub ink_faint: Color32,
    pub amber: Color32,
    pub on_amber: Color32,
    pub amber_ink: Color32,
    pub amber_soft: Color32,
    pub error: Color32,
    pub error_soft: Color32,
    pub on_error: Color32,
    pub warning: Color32,
    pub warning_soft: Color32,
    pub note: Color32,
    pub note_soft: Color32,
    pub success: Color32,
    pub success_soft: Color32,
    pub syn_keyword: Color32,
    pub syn_type: Color32,
    pub syn_string: Color32,
    pub syn_number: Color32,
    pub syn_decorator: Color32,
    pub syn_function: Color32,
    pub syn_comment: Color32,
    pub overlay: Color32,
    /// `shadow-pop`'s colour; its offset and blur are the same in both.
    pub shadow: Color32,
    /// `shadow-key`: the 1px edge under kbd keys.
    pub key_edge: Color32,
}

impl Palette {
    /// The focus ring's colour (`focus` is `amber-ink`).
    pub fn focus(&self) -> Color32 {
        self.amber_ink
    }

    /// `syn-punct` is `ink-muted`.
    pub fn syn_punct(&self) -> Color32 {
        self.ink_muted
    }
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

pub static DARK: Palette = Palette {
    bg_000: hex(0x0f0d0a),
    bg_100: hex(0x14120e),
    bg_200: hex(0x1a1813),
    bg_300: hex(0x24201a),
    bg_400: hex(0x2e2920),
    line: hex(0x353027),
    line_strong: hex(0x6e6554),
    ink: hex(0xece6d6),
    ink_muted: hex(0xa39b8a),
    ink_faint: hex(0x756d5f),
    amber: hex(0xffb547),
    on_amber: hex(0x1a1308),
    amber_ink: hex(0xffb547),
    amber_soft: hex(0x2e2416),
    error: hex(0xff7262),
    error_soft: hex(0x2c1613),
    on_error: hex(0x1a0b08),
    warning: hex(0xecd660),
    warning_soft: hex(0x29250f),
    note: hex(0x74c7ec),
    note_soft: hex(0x10232b),
    success: hex(0x6fdcb4),
    success_soft: hex(0x0f2620),
    syn_keyword: hex(0xffb547),
    syn_type: hex(0x74c7ec),
    syn_string: hex(0x9bd982),
    syn_number: hex(0xf49a6c),
    syn_decorator: hex(0xe59bd8),
    syn_function: hex(0xfff6e2),
    syn_comment: hex(0x8a8172),
    overlay: Color32::from_rgba_unmultiplied_const(8, 7, 5, 184),
    shadow: Color32::from_rgba_unmultiplied_const(0, 0, 0, 128),
    key_edge: hex(0x353027),
};

pub static LIGHT: Palette = Palette {
    bg_000: hex(0xefece4),
    bg_100: hex(0xf7f6f2),
    bg_200: hex(0xffffff),
    bg_300: hex(0xebe8df),
    bg_400: hex(0xe0dbcd),
    line: hex(0xdcd8cc),
    line_strong: hex(0x8a8170),
    ink: hex(0x1a1813),
    ink_muted: hex(0x5c5548),
    ink_faint: hex(0x8f8778),
    amber: hex(0xffb547),
    on_amber: hex(0x1a1308),
    amber_ink: hex(0x975200),
    amber_soft: hex(0xfbe9cc),
    error: hex(0xb82e1c),
    error_soft: hex(0xfbe4e0),
    on_error: hex(0xffffff),
    warning: hex(0x7a5f00),
    warning_soft: hex(0xf6efcc),
    note: hex(0x0b6789),
    note_soft: hex(0xdcedf4),
    success: hex(0x14714f),
    success_soft: hex(0xdcf1e8),
    syn_keyword: hex(0x975200),
    syn_type: hex(0x0b6789),
    syn_string: hex(0x3b7420),
    syn_number: hex(0xa8481a),
    syn_decorator: hex(0x923585),
    syn_function: hex(0x1a1813),
    syn_comment: hex(0x6f6858),
    overlay: Color32::from_rgba_unmultiplied_const(26, 24, 19, 92),
    shadow: Color32::from_rgba_unmultiplied_const(26, 24, 19, 41),
    key_edge: hex(0xc9c4b6),
};

/// egui's own widgets in the theme: square, flat, depth from the surfaces.
pub fn visuals(theme: Theme) -> Visuals {
    let p = theme.palette();
    let mut v = match theme {
        Theme::Dark => Visuals::dark(),
        Theme::Light => Visuals::light(),
    };
    let square = CornerRadius::ZERO;
    let pop = Shadow {
        offset: [0, 12],
        blur: 32,
        spread: 0,
        color: p.shadow,
    };
    v.override_text_color = Some(p.ink);
    v.weak_text_color = Some(p.ink_muted);
    v.hyperlink_color = p.amber_ink;
    v.faint_bg_color = p.bg_300;
    v.extreme_bg_color = p.bg_000;
    v.text_edit_bg_color = Some(p.bg_000);
    v.code_bg_color = p.bg_000;
    v.warn_fg_color = p.warning;
    v.error_fg_color = p.error;
    v.window_corner_radius = square;
    v.menu_corner_radius = square;
    v.window_shadow = pop;
    v.popup_shadow = pop;
    v.window_fill = p.bg_200;
    v.window_stroke = Stroke::new(1.0, p.line_strong);
    v.panel_fill = p.bg_100;
    v.selection = Selection {
        bg_fill: p.amber_soft,
        stroke: Stroke::new(1.0, p.amber_ink),
    };
    v.text_cursor.stroke = Stroke::new(2.0, p.amber);
    v.striped = false;
    v.indent_has_left_vline = false;

    let w = &mut v.widgets;
    for (state, bg, fg, border) in [
        (&mut w.noninteractive, p.bg_200, p.ink, p.line),
        (&mut w.inactive, p.bg_300, p.ink, p.line_strong),
        (&mut w.hovered, p.bg_300, p.ink, p.line_strong),
        (&mut w.active, p.bg_400, p.ink, p.line_strong),
        (&mut w.open, p.bg_400, p.ink, p.line_strong),
    ] {
        state.bg_fill = bg;
        state.weak_bg_fill = bg;
        state.bg_stroke = Stroke::new(1.0, border);
        state.fg_stroke = Stroke::new(1.0, fg);
        state.corner_radius = square;
        state.expansion = 0.0;
    }
    v
}
