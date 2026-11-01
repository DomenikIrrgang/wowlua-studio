//! The brand's two typefaces, built into the binary (SIL OFL 1.1; the
//! licence texts are beside the files in `assets/fonts`). egui has no
//! weights, so every weight is a family of its own.

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily, FontId};

/// A face of the design system's type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    /// IBM Plex Mono 400: prose, code, tables, inputs.
    Plex,
    /// IBM Plex Mono 500: labels, tabs, buttons.
    PlexMedium,
    /// IBM Plex Mono 600: emphasis.
    PlexSemiBold,
    /// IBM Plex Mono italic: comments.
    PlexItalic,
    /// Martian Mono 500: large numbers.
    MartianMedium,
    /// Martian Mono 700: headings from 20 px up.
    MartianBold,
    /// Martian Mono 800: the largest display text.
    MartianExtraBold,
}

impl Face {
    const ALL: [Face; 7] = [
        Face::Plex,
        Face::PlexMedium,
        Face::PlexSemiBold,
        Face::PlexItalic,
        Face::MartianMedium,
        Face::MartianBold,
        Face::MartianExtraBold,
    ];

    fn name(self) -> &'static str {
        match self {
            Face::Plex => "plex",
            Face::PlexMedium => "plex-500",
            Face::PlexSemiBold => "plex-600",
            Face::PlexItalic => "plex-italic",
            Face::MartianMedium => "martian-500",
            Face::MartianBold => "martian-700",
            Face::MartianExtraBold => "martian-800",
        }
    }

    fn data(self) -> &'static [u8] {
        match self {
            Face::Plex => include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf"),
            Face::PlexMedium => include_bytes!("../assets/fonts/IBMPlexMono-Medium.ttf"),
            Face::PlexSemiBold => include_bytes!("../assets/fonts/IBMPlexMono-SemiBold.ttf"),
            Face::PlexItalic => include_bytes!("../assets/fonts/IBMPlexMono-Italic.ttf"),
            Face::MartianMedium => include_bytes!("../assets/fonts/MartianMono-StdMd.ttf"),
            Face::MartianBold => include_bytes!("../assets/fonts/MartianMono-StdBd.ttf"),
            Face::MartianExtraBold => include_bytes!("../assets/fonts/MartianMono-StdxBd.ttf"),
        }
    }

    pub fn family(self) -> FontFamily {
        match self {
            Face::Plex => FontFamily::Monospace,
            _ => FontFamily::Name(self.name().into()),
        }
    }

    pub fn at(self, size: f32) -> FontId {
        FontId::new(size, self.family())
    }
}

/// The font definitions: each face, then egui's own fonts as a fallback for
/// glyphs a face lacks.
pub fn definitions() -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    let fallbacks = defs
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    for face in Face::ALL {
        defs.font_data.insert(
            face.name().to_owned(),
            Arc::new(FontData::from_static(face.data())),
        );
        let mut chain = vec![face.name().to_owned()];
        chain.extend(fallbacks.iter().cloned());
        defs.families.insert(face.family(), chain);
    }
    // Studio's text is monospace throughout: proportional text is Plex too.
    let plex = defs.families[&FontFamily::Monospace].clone();
    defs.families.insert(FontFamily::Proportional, plex);
    defs
}
