//! wowlua studio's design system in egui (the wowlua design system and the
//! studio design's components): the theme, the fonts, the icons and the
//! controls Studio is drawn with. It depends on egui alone, so it builds
//! for the browser too (decision S002).

pub mod controls;
mod fonts;
pub mod forms;
pub mod icons;
pub mod mark;
mod raster;
mod theme;
pub mod verbs;
pub mod widgets;
pub mod wordmark;

pub use fonts::Face;
pub use icons::Icon;
pub use theme::{DARK, LIGHT, Palette, Theme, visuals};

use egui::{Context, FontFamily, FontId, Id, TextStyle};

/// The rows of trees, tables and lists: 1, or larger when comfortable
/// (the setting "density", S062), as `f32` bits.
static DENSITY: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3f80_0000);

/// Sets the rows' scale: 1 compact, 1.25 comfortable.
pub fn set_density(comfortable: bool) {
    let scale: f32 = if comfortable { 1.25 } else { 1.0 };
    DENSITY.store(scale.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// A row's height at the density: `base` compact.
pub fn row(base: f32) -> f32 {
    (base * f32::from_bits(DENSITY.load(std::sync::atomic::Ordering::Relaxed))).round()
}

fn theme_id() -> Id {
    Id::unique("wowlua-studio-theme")
}

/// Sets Studio's fonts, text styles and spacing, in `theme`.
pub fn install(ctx: &Context, theme: Theme) {
    ctx.set_fonts(fonts::definitions());
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::new(11.5, FontFamily::Monospace)),
            (TextStyle::Body, FontId::new(13.0, FontFamily::Monospace)),
            (
                TextStyle::Monospace,
                FontId::new(13.0, FontFamily::Monospace),
            ),
            (TextStyle::Button, Face::PlexMedium.at(12.0)),
            (TextStyle::Heading, Face::MartianBold.at(20.0)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(8.0, 4.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        style.spacing.interact_size.y = 24.0;
        style.spacing.menu_margin = egui::Margin::same(4);
        style.interaction.tooltip_delay = 0.4;
        // No animations but the caret (the design's motion rules).
        style.animation_time = 0.0;
    });
    set_theme(ctx, theme);
}

/// Switches the theme.
pub fn set_theme(ctx: &Context, theme: Theme) {
    ctx.data_mut(|d| d.insert_temp(theme_id(), theme));
    ctx.set_visuals(visuals(theme));
}

/// The current theme.
pub fn theme(ctx: &Context) -> Theme {
    ctx.data(|d| d.get_temp(theme_id())).unwrap_or_default()
}

/// The current theme's colours.
pub fn palette(ctx: &Context) -> &'static Palette {
    theme(ctx).palette()
}
