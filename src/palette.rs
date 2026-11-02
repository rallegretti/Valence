//! Colour scheme: atoms are coloured by periodic-table column.

use valence_chem::elements::{self, TablePos};

/// sRGB triple in 0..1.
pub type Rgb = [f32; 3];

fn hsl(h: f32, s: f32, l: f32) -> Rgb {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = (h.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [r + m, g + m, b + m]
}

/// Colour for a column (1..=18); f-block rows get their own hues.
pub fn element_color(z: u8) -> Rgb {
    match elements::table_pos(z) {
        TablePos::Main { group, .. } => hsl((group as f32 - 1.0) * 285.0 / 17.0, 0.68, 0.58),
        TablePos::FBlock { row: 0, .. } => hsl(325.0, 0.40, 0.70),
        TablePos::FBlock { .. } => hsl(160.0, 0.28, 0.60),
    }
}

pub fn to_egui(c: Rgb) -> bevy_egui::egui::Color32 {
    bevy_egui::egui::Color32::from_rgb(
        (c[0] * 255.0) as u8,
        (c[1] * 255.0) as u8,
        (c[2] * 255.0) as u8,
    )
}

pub fn luminance(c: Rgb) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

pub const CATION: Rgb = [0.25, 0.55, 1.0];
pub const ANION: Rgb = [1.0, 0.3, 0.3];
pub const OPEN_SITE: Rgb = [1.0, 0.85, 0.2];
pub const GOOD: Rgb = [0.3, 0.9, 0.45];
pub const BAD: Rgb = [1.0, 0.35, 0.35];
