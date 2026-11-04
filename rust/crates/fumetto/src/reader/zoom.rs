//! Lo zoom della pagina singola (e, nel nastro, la sua larghezza).

use super::*;

impl Reader {
    /// Il rettangolo della pagina singola, con lo zoom: (x, y, larghezza, altezza).
    pub(super) fn single_rect(&self, page: usize) -> Option<(f32, f32, f32, f32)> {
        let (nw, nh) = self.native.get(page).copied().flatten()?;
        let (vw, vh) = (self.view.0 as f32, self.view.1 as f32);
        let (w0, h0) = Fit::Contain { width: self.view.0, height: self.view.1 }.size(nw, nh);
        let (w, h) = (w0 as f32 * self.zoom, h0 as f32 * self.zoom);
        // lungo un asse: centrata se ci sta, altrimenti mai oltre i bordi
        let axis = |view: f32, size: f32, c: f32| {
            if size <= view { ((view - size) / 2.0).round() } else { (view / 2.0 - c * size).clamp(view - size, 0.0) }
        };
        Some((axis(vw, w, self.center.0), axis(vh, h, self.center.1), w, h))
    }

    pub(super) fn zoom_at(&mut self, factor: f32, px: f32, py: f32) {
        if self.mode != Mode::Single {
            return;
        }
        let Some((x, y, w, h)) = self.single_rect(self.page) else { return };
        // il punto della pagina sotto il puntatore resta sotto il puntatore
        let (u, v) = ((px - x) / w, (py - y) / h);
        let zoom = (self.zoom * factor).clamp(1.0, MAX_ZOOM);
        let k = zoom / self.zoom;
        let (w2, h2) = (w * k, h * k);
        self.zoom = zoom;
        self.center = ((self.view.0 as f32 / 2.0 - px) / w2 + u, (self.view.1 as f32 / 2.0 - py) / h2 + v);
    }

    pub(super) fn zoom_to(&mut self, z: Zoom) {
        let native = self.native.get(self.here()).copied().flatten();
        match self.mode {
            Mode::Single => {
                let Some((nw, nh)) = native else { return };
                let (w0, _) = Fit::Contain { width: self.view.0, height: self.view.1 }.size(nw, nh);
                self.zoom = match z {
                    Zoom::Page => 1.0,
                    Zoom::Width => self.view.0 as f32 / w0 as f32,
                    Zoom::Actual => nw as f32 / w0 as f32,
                }
                .clamp(1.0, MAX_ZOOM);
                // si parte dall'alto: e' li' che si comincia a leggere
                self.center = (0.5, 0.0);
            }
            Mode::Strip => {
                let old = self.strip_width();
                let natural = self.natural_strip_frac().unwrap_or(STRIP_FRAC);
                self.strip_frac = Some(
                    match (z, native) {
                        (Zoom::Page, _) | (Zoom::Actual, None) => natural,
                        (Zoom::Width, _) => 1.0,
                        (Zoom::Actual, Some((nw, _))) => nw as f32 / self.view.0 as f32,
                    }
                    .clamp(0.2, 1.0),
                );
                self.rescale_strip(old);
            }
            Mode::Double => {}
        }
    }

    /// Quanto e' ingrandita la pagina rispetto ai suoi pixel, in percento
    /// (100: un pixel della pagina per un pixel dello schermo). `None` dove
    /// lo zoom non si regola (la doppia pagina).
    pub fn zoom_percent(&self) -> Option<u32> {
        let (nw, _) = self.native.get(self.here()).copied().flatten()?;
        let shown = match self.mode {
            Mode::Single => self.single_rect(self.page)?.2,
            Mode::Strip => self.strip_width() as f32,
            Mode::Double => return None,
        };
        Some((shown / nw as f32 * 100.0).round() as u32)
    }

    /// Riporta il centro dove la pagina lo lascia davvero, dopo i bordi.
    pub(super) fn clamp_center(&mut self) {
        if self.zoom <= 1.0 {
            self.zoom = 1.0;
            self.center = (0.5, 0.5);
            return;
        }
        if let Some((x, y, w, h)) = self.single_rect(self.page) {
            self.center = ((self.view.0 as f32 / 2.0 - x) / w, (self.view.1 as f32 / 2.0 - y) / h);
        }
    }
}
