//! Piu' volumi scelti insieme, con Ctrl+clic (Cmd+clic su macOS): una
//! copertina si sceglie o si lascia, una pila sceglie tutti i suoi volumi.
//! In basso una barra dice quanti sono, con "Tag…" per dar loro (o
//! togliere) dei tag in un colpo; Esc o la crocetta lasciano la scelta.

use super::*;

/// La barra in basso: larghezza e altezza, in punti.
const BAR: (f32, f32) = (420.0, 44.0);
/// Le parti che si cliccano, da destra: la crocetta e "Tag…".
const CLOSE_W: f32 = 44.0;
const TAGS_W: f32 = 96.0;

impl Shelf {
    /// I volumi scelti.
    pub fn picked(&self) -> &[PathBuf] {
        &self.picked
    }

    /// Lascia la scelta.
    pub fn clear_picks(&mut self) {
        self.picked.clear();
    }

    /// Ctrl+clic: sceglie o lascia la cella sotto il punto. `false` se li'
    /// non c'e' una cella che si possa scegliere (un gruppo, una cartella).
    pub fn pick_at(&mut self, d: &ShelfData, view: (f32, f32), s: f32, x: f32, y: f32) -> bool {
        let Some(tile) = self.tile_at(d, view, s, x, y) else { return false };
        let paths = tile_paths(d, &tile);
        if paths.is_empty() {
            return false;
        }
        if self.is_picked(d, &tile) {
            self.picked.retain(|p| !paths.contains(p));
        } else {
            for p in paths {
                if !self.picked.contains(&p) {
                    self.picked.push(p);
                }
            }
        }
        true
    }

    /// La cella e' scelta: il volume, o tutti i volumi della pila.
    pub(super) fn is_picked(&self, d: &ShelfData, tile: &Tile) -> bool {
        let paths = tile_paths(d, tile);
        !self.picked.is_empty() && !paths.is_empty() && paths.iter().all(|p| self.picked.contains(p))
    }

    /// La barra in basso, se c'e' una scelta.
    pub(super) fn pick_bar(&self, g: &Geo, m: &mut dyn Measure) -> Option<Layer> {
        if self.picked.is_empty() {
            return None;
        }
        let s = g.s;
        let [x, y, w, h] = pick_bar_rect(g.view, s);
        let mut l = Layer::default();
        l.rects.push(Rect::new(x, y + 6.0 * s, w, h, [0.0, 0.0, 0.0, 0.6]).radius(h / 2.0).blur(14.0 * s));
        l.rects.push(Rect::new(x, y, w, h, alpha(PLATE, 0.99)).radius(h / 2.0));
        l.rects.push(Rect::new(x, y, w, h, ACCENT).radius(h / 2.0).stroke(s.round().max(1.0)));
        let n = self.picked.len();
        let count = if italian() {
            format!("{n} {} ", if n == 1 { "volume scelto" } else { "volumi scelti" })
        } else {
            format!("{n} volume{} picked", if n == 1 { "" } else { "s" })
        };
        let room = w - (TAGS_W + CLOSE_W + 22.0) * s;
        let count = fit(m, Text::new(count, Face::Sans, 14.0 * s, INK).tabular(), room);
        l.texts.push(count.on_baseline(x + 22.0 * s, y + h / 2.0 + 5.0 * s));
        let tags_x = x + w - (TAGS_W + CLOSE_W) * s;
        let on = self.hover == Some(Hit::PickTags);
        let pill = Rect::new(
            tags_x,
            y + 7.0 * s,
            TAGS_W * s - 8.0 * s,
            h - 14.0 * s,
            [1.0, 1.0, 1.0, if on { 0.16 } else { 0.1 }],
        );
        l.rects.push(pill.radius((h - 14.0 * s) / 2.0));
        let label = Text::new(t("Tag\u{2026}", "Tags\u{2026}"), Face::Sans, 13.5 * s, INK)
            .boxed(TAGS_W * s - 8.0 * s, Align::Center);
        l.texts.push(label.on_baseline(tags_x, y + h / 2.0 + 5.0 * s));
        let close = if self.hover == Some(Hit::PickClear) { INK_F } else { MUTED_F };
        let cx = x + w - CLOSE_W * s / 2.0 - 4.0 * s;
        draw_icon(&mut l, Icon::Close, (cx - 3.0 * s).round(), (y + h / 2.0).round(), s * 0.7, close);
        Some(l)
    }

    /// Cosa c'e' nella barra della scelta sotto il punto.
    pub(super) fn pick_hit(&self, view: (f32, f32), s: f32, x: f32, y: f32) -> Option<Hit> {
        if self.picked.is_empty() {
            return None;
        }
        let [bx, by, w, h] = pick_bar_rect(view, s);
        if x < bx || x >= bx + w || y < by || y >= by + h {
            return None;
        }
        Some(if x >= bx + w - CLOSE_W * s {
            Hit::PickClear
        } else if x >= bx + w - (TAGS_W + CLOSE_W) * s {
            Hit::PickTags
        } else {
            Hit::Bar
        })
    }
}

/// Dove sta la barra della scelta: in basso al centro.
fn pick_bar_rect(view: (f32, f32), s: f32) -> [f32; 4] {
    let w = (BAR.0 * s).min(view.0 - 32.0 * s).round();
    let h = (BAR.1 * s).round();
    [((view.0 - w) / 2.0).round(), (view.1 - h - 24.0 * s).round(), w, h]
}

/// I volumi di una cella: il volume, o quelli della pila (della libreria).
fn tile_paths(d: &ShelfData, tile: &Tile) -> Vec<PathBuf> {
    match tile {
        Tile::Volume(i) => vec![d.entries[*i].path.clone()],
        Tile::Series { name, .. } => d.entries[..d.library_len()]
            .iter()
            .filter(|e| e.series.as_deref() == Some(name.as_str()))
            .map(|e| e.path.clone())
            .collect(),
        Tile::Group { .. } | Tile::Folder { .. } => Vec::new(),
    }
}
