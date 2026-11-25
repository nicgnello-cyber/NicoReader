//! L'impaginazione dei testi dell'interfaccia, comune alla scheda video
//! (`Overlay`) e al processore (`SoftOverlay`): stessi caratteri, stesse
//! righe.
//!
//! Impaginare costa qualche microsecondo per testo, e con la libreria a
//! schermo i testi sono decine, a ogni fotogramma, quasi sempre gli stessi.
//! Si tengono le impaginazioni gia' fatte, per testo, corpo, carattere e
//! scatola (non per colore ne' posizione, che non le cambiano); quelle non
//! piu' usate da un po' si lasciano andare.
//!
//! I caratteri sono dentro l'eseguibile, cosi' l'aspetto e' lo stesso su ogni
//! sistema: Instrument Serif per numeri e titoli, Instrument Sans (variabile)
//! per le etichette. Licenza SIL OFL 1.1: i testi sono accanto ai file, in
//! `caratteri/`. Nessun carattere di sistema viene caricato: costerebbe
//! decine di millisecondi all'avvio e cambierebbe da un computer all'altro.

use std::collections::HashMap;
use std::sync::Arc;

use glyphon::cosmic_text::{Align as TextAlign, FeatureTag, FontFeatures};
use glyphon::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style, Weight, fontdb};

use crate::{Align, Face, Text};

const SERIF: &[u8] = include_bytes!("../caratteri/InstrumentSerif-Regular.ttf");
const SANS: &[u8] = include_bytes!("../caratteri/InstrumentSans.ttf");

/// Oltre quante impaginazioni si fa pulizia.
const KEPT: usize = 400;
/// Quanti fotogrammi resta un'impaginazione non piu' usata.
const IDLE_FRAMES: u64 = 120;

/// Cio' che decide un'impaginazione (non il colore, non la posizione).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    text: String,
    face: Face,
    size: u32,
    weight: u16,
    tracking: u32,
    tabular: bool,
    width: Option<u32>,
    align: Align,
    max_lines: Option<usize>,
}

impl Key {
    fn of(t: &Text, max_lines: Option<usize>) -> Key {
        Key {
            text: t.text.clone(),
            face: t.face,
            size: t.size.to_bits(),
            weight: t.weight,
            tracking: t.tracking.to_bits(),
            tabular: t.tabular,
            width: t.width.map(f32::to_bits),
            align: t.align,
            max_lines,
        }
    }
}

pub(crate) struct Typesetter {
    pub(crate) fonts: FontSystem,
    laid: HashMap<Key, (Buffer, u64)>,
    frame: u64,
}

impl Typesetter {
    pub(crate) fn new() -> Typesetter {
        let mut db = fontdb::Database::new();
        for bytes in [SERIF, SANS] {
            db.load_font_source(fontdb::Source::Binary(Arc::new(bytes)));
        }
        let fonts = FontSystem::new_with_locale_and_db("it-IT".into(), db);
        Typesetter { fonts, laid: HashMap::new(), frame: 0 }
    }

    /// Il testo impaginato, al massimo in `max_lines` righe (`None`: tutte).
    pub(crate) fn layout(&mut self, t: &Text, max_lines: Option<usize>) -> &Buffer {
        let frame = self.frame;
        let fonts = &mut self.fonts;
        let (buffer, used) =
            self.laid.entry(Key::of(t, max_lines)).or_insert_with(|| (shape(fonts, t, max_lines), frame));
        *used = frame;
        buffer
    }

    /// I testi di uno strato impaginati (due righe al massimo, come li mostra
    /// l'interfaccia), insieme ai caratteri che servono per disegnarli.
    pub(crate) fn layer<'a>(&'a mut self, texts: &[Text]) -> (&'a mut FontSystem, Vec<&'a Buffer>) {
        let keys: Vec<Key> = texts.iter().map(|t| Key::of(t, Some(2))).collect();
        for (key, t) in keys.iter().zip(texts) {
            let (frame, fonts) = (self.frame, &mut self.fonts);
            let entry = self.laid.entry(key.clone()).or_insert_with(|| (shape(fonts, t, Some(2)), frame));
            entry.1 = frame;
        }
        let laid = &self.laid;
        (&mut self.fonts, keys.iter().map(|k| &laid[k].0).collect())
    }

    /// Larghezza della riga, senza andare a capo.
    pub(crate) fn width(&mut self, t: &Text) -> f32 {
        let free = Text { width: None, ..t.clone() };
        self.layout(&free, Some(1)).layout_runs().map(|r| r.line_w).fold(0.0, f32::max)
    }

    /// Quante righe, andando a capo nella larghezza della scatola.
    pub(crate) fn lines(&mut self, t: &Text) -> usize {
        self.layout(t, None).layout_runs().count()
    }

    /// Finito un fotogramma: se sono troppe, si lasciano andare quelle non
    /// usate da un po'.
    pub(crate) fn end_frame(&mut self) {
        self.frame += 1;
        if self.laid.len() > KEPT {
            let now = self.frame;
            self.laid.retain(|_, (_, used)| now - *used < IDLE_FRAMES);
        }
    }

    /// Quante impaginazioni si tengono (per le prove).
    #[cfg(test)]
    pub(crate) fn kept(&self) -> usize {
        self.laid.len()
    }
}

/// Impagina un testo; al massimo `max_lines` righe (`None`: tutte).
fn shape(fonts: &mut FontSystem, t: &Text, max_lines: Option<usize>) -> Buffer {
    let line = (t.size * 1.2).ceil();
    let mut b = Buffer::new(fonts, Metrics::new(t.size, line));
    b.set_size(t.width, max_lines.map(|n| line * n as f32));
    let family = match t.face {
        Face::Serif => "Instrument Serif",
        Face::Sans => "Instrument Sans",
    };
    let mut attrs = Attrs::new().family(Family::Name(family)).style(Style::Normal).weight(Weight(t.weight));
    if t.tracking != 0.0 {
        attrs = attrs.letter_spacing(t.tracking);
    }
    if t.tabular {
        let mut f = FontFeatures::new();
        f.enable(FeatureTag::new(b"tnum"));
        attrs = attrs.font_features(f);
    }
    let align = match t.align {
        Align::Left => TextAlign::Left,
        Align::Center => TextAlign::Center,
        Align::Right => TextAlign::Right,
    };
    b.set_text(&t.text, &attrs, Shaping::Advanced, Some(align));
    b.shape_until_scroll(fonts, false);
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i_testi_uguali_si_impaginano_una_volta() {
        let mut ts = Typesetter::new();
        let t = Text::new("Nebbia sul Porto", Face::Serif, 18.0, [255; 4]);
        let w = ts.width(&t);
        // un altro colore e un altro posto: la stessa impaginazione
        let moved = Text { color: [10, 20, 30, 255], ..t.clone() }.at(300.0, 40.0);
        assert_eq!(ts.width(&moved), w);
        assert_eq!(ts.kept(), 1);
        // un altro corpo: un'altra
        assert!(ts.width(&Text { size: 24.0, ..t.clone() }) > w);
        assert_eq!(ts.kept(), 2);
        // quelle non piu' usate se ne vanno, quando sono troppe
        for n in 0..KEPT {
            ts.width(&Text::new(format!("titolo {n}"), Face::Sans, 12.0, [255; 4]));
        }
        for _ in 0..IDLE_FRAMES {
            ts.end_frame();
        }
        ts.width(&t);
        ts.end_frame();
        assert_eq!(ts.kept(), 1, "resta solo quella usata adesso");
    }
}
