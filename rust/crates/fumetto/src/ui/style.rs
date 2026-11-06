//! Lo stile dell'interfaccia: i colori, il maiuscolo spaziato delle
//! etichette, i testi tagliati perche' stiano nel loro spazio.

use fumetto_render::{Face, Measure, Text};

pub(crate) const INK: [u8; 4] = [0xED, 0xE8, 0xDF, 0xFF];
pub(crate) const MUTED: [u8; 4] = [0x9D, 0x97, 0x8B, 0xFF];
pub(crate) const ACCENT_TEXT: [u8; 4] = [0xE0, 0x6A, 0x50, 0xFF];
pub(crate) const HAIR: [f32; 4] = rgb(0x3B, 0x38, 0x33);
pub(crate) const TICK: [f32; 4] = rgb(0x5F, 0x5A, 0x52);
pub(crate) const ACCENT: [f32; 4] = rgb(0xC9, 0x55, 0x3C);
pub(crate) const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
/// Dove un'immagine non c'e' ancora (una copertina, una miniatura).
pub(crate) const PLATE: [f32; 4] = rgb(0x17, 0x16, 0x14);

pub(crate) const INK_F: [f32; 4] = rgb(0xED, 0xE8, 0xDF);

pub(crate) const MUTED_F: [f32; 4] = rgb(0x9D, 0x97, 0x8B);

pub(crate) const fn rgb(r: u8, g: u8, b: u8) -> [f32; 4] {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
}

pub(crate) fn fade(c: [u8; 4], a: f32) -> [u8; 4] {
    [c[0], c[1], c[2], (c[3] as f32 * a).round() as u8]
}

pub(crate) fn alpha(c: [f32; 4], a: f32) -> [f32; 4] {
    [c[0], c[1], c[2], c[3] * a]
}

/// Maiuscolo spaziato, per le etichette.
pub(crate) fn caps(text: &str, size: f32, color: [u8; 4]) -> Text {
    Text::new(text.to_uppercase(), Face::Sans, size, color).weight(500).tracking(0.16)
}

/// Il testo com'e', o tagliato con i puntini perche' stia in `room` pixel.
pub(crate) fn fit(m: &mut dyn Measure, t: Text, room: f32) -> Text {
    if m.width(&t) <= room {
        return t;
    }
    let chars: Vec<char> = t.text.chars().collect();
    let cut = |n: usize| {
        let kept: String = chars[..n].iter().collect();
        Text { text: format!("{}\u{2026}", kept.trim_end()), ..t.clone() }
    };
    // quante lettere tenere: la piu' lunga che ci sta, per bisezione
    let (mut fits, mut too_long) = (0, chars.len());
    while too_long - fits > 1 {
        let mid = (fits + too_long) / 2;
        if m.width(&cut(mid)) <= room { fits = mid } else { too_long = mid }
    }
    cut(fits)
}
