//! Il nastro: quanto e' largo, quanto e' alta ogni pagina, dove si e' e
//! lo scorrimento morbido.

use super::*;

impl Reader {
    pub(super) fn strip_width(&self) -> u32 {
        ((self.view.0 as f32 * self.strip_frac.unwrap_or(STRIP_FRAC)).round() as u32).max(64)
    }

    /// La larghezza naturale del nastro per queste strisce e questa finestra:
    /// al massimo una volta e mezza la loro misura vera, e al massimo il 60%
    /// della finestra. `None` finche' non si conoscono l'una e l'altra.
    pub(super) fn natural_strip_frac(&self) -> Option<f32> {
        if self.view.0 <= 1 {
            return None;
        }
        let mut widths: Vec<u32> =
            (0..self.len).filter(|&i| !self.broken[i]).filter_map(|i| self.native[i].map(|n| n.0)).collect();
        if widths.is_empty() {
            return None;
        }
        widths.sort_unstable();
        let typical = widths[widths.len() / 2] as f32;
        Some((STRIP_GROW * typical / self.view.0 as f32).min(STRIP_FRAC).clamp(0.2, 1.0))
    }

    /// Nel nastro, la larghezza non ancora scelta diventa quella naturale
    /// appena la si puo' calcolare, e da li' resta: una colonna che cambiasse
    /// man mano che arrivano le pagine le farebbe rifare tutte.
    pub(super) fn settle_strip_width(&mut self) {
        if self.mode == Mode::Strip
            && self.strip_frac.is_none()
            && let Some(f) = self.natural_strip_frac()
        {
            let old = self.strip_width();
            self.strip_frac = Some(f);
            self.rescale_strip(old);
        }
    }

    /// Il nastro piu' largo (`k` > 1) o piu' stretto, fra un quinto e tutta
    /// la finestra.
    pub(super) fn widen_strip(&mut self, k: f32) {
        let old = self.strip_width();
        self.strip_frac = Some((self.strip_frac.unwrap_or(STRIP_FRAC) * k).clamp(0.2, 1.0));
        self.rescale_strip(old);
    }

    /// La larghezza del nastro e' cambiata: gli scostamenti in pixel si
    /// riscalano, e si resta nello stesso punto della pagina.
    pub(super) fn rescale_strip(&mut self, old_width: u32) {
        let new = self.strip_width();
        if old_width > 0 && new != old_width {
            self.strip.offset *= new as f32 / old_width as f32;
            self.glide *= new as f32 / old_width as f32;
        }
    }

    /// La posizione nel nastro com'e' adesso, anche se ancora una frazione.
    pub(super) fn strip_now(&self) -> Strip {
        match self.anchor_frac {
            Some(f) => Strip { anchor: self.strip.anchor, offset: f * self.strip_height(self.strip.anchor) },
            None => self.strip,
        }
    }

    /// Da frazione a pixel, con l'altezza migliore che si conosce.
    pub(super) fn settle_anchor(&mut self) {
        self.strip = self.strip_now();
        self.anchor_frac = None;
    }

    /// Altezza a schermo di una pagina nel nastro; se non la si conosce ancora,
    /// quella media delle pagine gia' viste (i webtoon sono quasi tutti uguali).
    pub(super) fn strip_height(&self, i: usize) -> f32 {
        let w = self.strip_width() as f32;
        match self.native.get(i).copied().flatten() {
            Some((nw, nh)) => nh as f32 * w / nw as f32,
            None if self.ratios.1 == 0 => w * 1.5,
            None => w * (self.ratios.0 / self.ratios.1 as f64) as f32,
        }
    }

    pub(super) fn strip_visible(&self) -> Vec<(usize, f32)> {
        self.strip_now().visible(|i| self.strip_height(i), self.len, self.view.1 as f32)
    }

    /// Scorrimento morbido del nastro; `true` se c'e' ancora movimento.
    pub fn advance_glide(&mut self, dt: f32) -> bool {
        if self.mode != Mode::Strip || !self.gliding() {
            self.glide = 0.0;
            return false;
        }
        let step = glide_step(self.glide, dt);
        self.glide -= step;
        self.settle_anchor(); // si scorre: da qui in poi conta la posizione in pixel
        let before = self.strip;
        let mut strip = self.strip;
        strip.scroll(step, |i| self.strip_height(i), self.len, self.view.1 as f32);
        self.strip = strip;
        if self.strip == before && step.abs() >= 1.0 {
            self.glide = 0.0; // contro il bordo del volume
        }
        true
    }
}
