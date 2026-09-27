//! Il nastro dei webtoon: dove sta ogni pagina mentre si scorre.
//!
//! La posizione e' "tanti pixel dentro la pagina N", non "tanti pixel
//! dall'inizio del volume": l'altezza vera di una pagina si scopre solo quando
//! e' decodificata, e con una posizione assoluta ogni scoperta sopra lo schermo
//! farebbe saltare la vista. Cosi' invece chi legge non si accorge di nulla.

/// Posizione nel nastro. Dopo ogni movimento `offset` sta dentro la pagina
/// `anchor`: 0 <= offset < altezza(anchor).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strip {
    pub anchor: usize,
    /// Pixel della pagina `anchor` gia' passati sopra il bordo della finestra.
    pub offset: f32,
}

impl Strip {
    pub fn at(page: usize) -> Strip {
        Strip { anchor: page, offset: 0.0 }
    }

    /// Scorre di `dy` pixel (positivo = avanti) senza uscire dal volume: in
    /// cima si ferma all'inizio della prima pagina, in fondo quando l'ultima
    /// pagina tocca il bordo basso della finestra.
    pub fn scroll(&mut self, dy: f32, h: impl Fn(usize) -> f32, len: usize, view_h: f32) {
        if len == 0 {
            return;
        }
        self.offset += dy;
        self.normalize(&h, len);
        // quanto nastro resta sotto il bordo alto
        let mut bottom = -self.offset;
        let mut i = self.anchor;
        while i < len && bottom < view_h {
            bottom += h(i);
            i += 1;
        }
        if i == len && bottom < view_h {
            self.offset -= view_h - bottom;
            self.normalize(&h, len);
        }
    }

    fn normalize(&mut self, h: &impl Fn(usize) -> f32, len: usize) {
        while self.offset < 0.0 && self.anchor > 0 {
            self.anchor -= 1;
            self.offset += h(self.anchor);
        }
        while self.anchor + 1 < len && self.offset >= h(self.anchor) {
            self.offset -= h(self.anchor);
            self.anchor += 1;
        }
        if self.anchor == 0 && self.offset < 0.0 {
            self.offset = 0.0;
        }
    }

    /// Le pagine a schermo, con la y del loro bordo alto.
    pub fn visible(&self, h: impl Fn(usize) -> f32, len: usize, view_h: f32) -> Vec<(usize, f32)> {
        let mut out = Vec::new();
        let mut y = -self.offset;
        let mut i = self.anchor;
        while i < len && y < view_h {
            out.push((i, y));
            y += h(i);
            i += 1;
        }
        out
    }

    /// L'altezza della pagina d'ancora e' cambiata (se ne e' scoperta la
    /// misura vera): si resta nello stesso punto *relativo* della pagina.
    pub fn rescale_anchor(&mut self, old_h: f32, new_h: f32) {
        if old_h > 0.0 {
            self.offset *= new_h / old_h;
        }
    }
}

/// La sensazione dello scorrimento: di quanti pixel avanzare in questo
/// fotogramma, sapendo che ne restano `remaining` da percorrere (con segno:
/// positivo = avanti) e che dal fotogramma precedente sono passati `dt` secondi.
///
/// Viene chiamata a ogni fotogramma finche' `remaining` non si esaurisce;
/// ogni rotella o freccia aggiunge strada a `remaining`. Deve dipendere dal
/// tempo e non dal numero di fotogrammi, cosi' a 60 e a 144 Hz si sente uguale.
pub fn glide_step(remaining: f32, dt: f32) -> f32 {
    // TODO(human): la curva dello scorrimento
    const SECONDS: f32 = 0.07;
    remaining * (1.0 - (-dt / SECONDS).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: f32 = 1000.0;

    /// A 60 e a 144 Hz, dopo lo stesso tempo, si e' percorsa la stessa strada.
    #[test]
    fn scorrimento_indipendente_dai_fotogrammi() {
        let run = |hz: f32| {
            let (mut left, dt) = (1000.0, 1.0 / hz);
            for _ in 0..(hz * 0.25) as usize {
                left -= glide_step(left, dt);
            }
            left
        };
        assert!((run(60.0) - run(144.0)).abs() < 25.0, "60 Hz {} contro 144 Hz {}", run(60.0), run(144.0));
        assert!(run(144.0).abs() < 100.0, "dopo un quarto di secondo manca ancora troppo");
    }

    fn pages(n: usize) -> (impl Fn(usize) -> f32, usize) {
        (|i| if i % 2 == 0 { 1500.0 } else { 700.0 }, n)
    }

    #[test]
    fn scorre_attraverso_le_pagine() {
        let (h, n) = pages(10);
        let mut s = Strip::at(0);
        s.scroll(1600.0, &h, n, H);
        assert_eq!(s, Strip { anchor: 1, offset: 100.0 });
        s.scroll(-200.0, &h, n, H);
        assert_eq!(s, Strip { anchor: 0, offset: 1400.0 });
    }

    #[test]
    fn si_ferma_in_cima() {
        let (h, n) = pages(10);
        let mut s = Strip::at(0);
        s.scroll(-500.0, &h, n, H);
        assert_eq!(s, Strip::at(0));
    }

    #[test]
    fn si_ferma_in_fondo_con_l_ultima_pagina_sul_bordo() {
        let (h, n) = pages(4); // 1500 + 700 + 1500 + 700 = 4400
        let mut s = Strip::at(0);
        s.scroll(99_999.0, &h, n, H);
        let v = s.visible(&h, n, H);
        let (last, y) = *v.last().unwrap();
        assert_eq!(last, 3);
        assert!((y + h(3) - H).abs() < 0.01, "ultima pagina non sul bordo: {v:?}");
    }

    #[test]
    fn volume_piu_corto_della_finestra() {
        let (h, n) = pages(1);
        let mut s = Strip::at(0);
        s.scroll(300.0, &h, n, 2000.0);
        assert_eq!(s, Strip::at(0));
    }

    #[test]
    fn pagine_visibili() {
        let (h, n) = pages(10);
        let mut s = Strip::at(0);
        s.scroll(1400.0, &h, n, H);
        assert_eq!(s.visible(&h, n, H), [(0, -1400.0), (1, 100.0), (2, 800.0)]);
    }
}
