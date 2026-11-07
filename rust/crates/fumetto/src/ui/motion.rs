//! Le transizioni: cio' che compare (un menu, il foglio delle impostazioni,
//! la libreria, le miniature) entra in una frazione di secondo invece di
//! scattare. Il giro di pagina resta istantaneo: chi legge non aspetta.
//!
//! Si guarda cosa si vedeva al fotogramma prima: cio' che c'e' adesso e
//! prima no, comincia a comparire. Il primo fotogramma non anima niente
//! (la finestra appena aperta, e gli scatti della sonda).

use super::*;

/// Cio' che puo' comparire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Part {
    Menu,
    Prefs,
    Shelf,
    Thumbs,
}

impl Part {
    const ALL: [Part; 4] = [Part::Menu, Part::Prefs, Part::Shelf, Part::Thumbs];

    /// Quanto ci mette a comparire, in secondi.
    fn duration(self) -> f32 {
        match self {
            Part::Menu => 0.12,
            Part::Prefs => 0.18,
            Part::Shelf | Part::Thumbs => 0.16,
        }
    }
}

/// Cosa si vedeva al fotogramma prima, e da quando compare cio' che si vede.
#[derive(Default)]
pub(super) struct Motion {
    /// `None` prima del primo fotogramma.
    seen: Option<[bool; 4]>,
    since: [Option<Instant>; 4],
}

impl Motion {
    /// Il fotogramma di adesso: cosa si vede.
    pub(super) fn frame(&mut self, visible: [bool; 4], now: Instant) {
        for (k, &on) in visible.iter().enumerate() {
            if !on {
                self.since[k] = None;
            } else if self.seen.is_some_and(|seen| !seen[k]) {
                self.since[k] = Some(now);
            }
        }
        self.seen = Some(visible);
    }

    /// Quanto e' comparso `part`, da 0 a 1, addolcito all'inizio e alla fine.
    pub(super) fn shown(&self, part: Part, now: Instant) -> f32 {
        let k = Part::ALL.iter().position(|&p| p == part).unwrap_or(0);
        self.since[k].map_or(1.0, |t| {
            let a = (now.saturating_duration_since(t).as_secs_f32() / part.duration()).min(1.0);
            a * a * (3.0 - 2.0 * a)
        })
    }

    /// Qualcosa sta ancora comparendo: serve il fotogramma dopo.
    pub(super) fn moving(&self, now: Instant) -> bool {
        Part::ALL.iter().any(|&p| self.shown(p, now) < 1.0)
    }
}

/// Uno strato che sta comparendo: trasparente quanto manca ad `a`, e
/// spostato di (`dx`, `dy`) che a fine corsa diventano zero.
pub(super) fn appear(l: &mut Layer, a: f32, dx: f32, dy: f32) {
    if a >= 1.0 {
        return;
    }
    let (dx, dy) = ((1.0 - a) * dx, (1.0 - a) * dy);
    for r in &mut l.rects {
        r.x += dx;
        r.y += dy;
        if r.line {
            // una linea va da (x, y) a (w, h): si spostano tutti e due i capi
            r.w += dx;
            r.h += dy;
        }
        r.top[3] *= a;
        r.bottom[3] *= a;
    }
    for t in &mut l.texts {
        t.x += dx;
        t.y += dy;
        t.color = fade(t.color, a);
    }
}

/// Un velo nero sopra cio' che sta comparendo da sotto (la libreria, le
/// miniature): si dirada fino a sparire.
pub(super) fn veil(view: (f32, f32), a: f32) -> Option<Layer> {
    (a < 1.0).then(|| Layer {
        rects: vec![Rect::new(0.0, 0.0, view.0, view.1, [0.0, 0.0, 0.0, 1.0 - a])],
        texts: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn compare_solo_cio_che_prima_non_c_era() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut m = Motion::default();
        // il primo fotogramma: niente si anima
        m.frame([true, false, true, false], t0);
        assert_eq!(m.shown(Part::Menu, t0), 1.0);
        assert!(!m.moving(t0));
        // si apre il foglio delle impostazioni
        m.frame([true, true, true, false], ms(10));
        assert_eq!(m.shown(Part::Prefs, ms(10)), 0.0);
        assert!(m.moving(ms(50)));
        let half = m.shown(Part::Prefs, ms(100));
        assert!(half > 0.2 && half < 0.8, "{half}");
        assert_eq!(m.shown(Part::Prefs, ms(400)), 1.0);
        assert!(!m.moving(ms(400)));
        // resta aperto: non ricomincia
        m.frame([true, true, true, false], ms(500));
        assert_eq!(m.shown(Part::Prefs, ms(500)), 1.0);
    }

    #[test]
    fn uno_strato_che_compare() {
        let mut l = Layer::default();
        l.rects.push(Rect::new(10.0, 10.0, 5.0, 5.0, [1.0, 1.0, 1.0, 1.0]));
        l.rects.push(Rect::line(0.0, 0.0, 10.0, 0.0, 1.0, [1.0, 1.0, 1.0, 1.0]));
        l.texts.push(Text::new("x", Face::Sans, 10.0, [255, 255, 255, 255]).at(0.0, 0.0));
        appear(&mut l, 0.5, 20.0, 0.0);
        assert_eq!((l.rects[0].x, l.rects[0].w), (20.0, 5.0), "il rettangolo si sposta, la sua larghezza no");
        assert_eq!((l.rects[1].x, l.rects[1].w), (10.0, 20.0), "la linea: tutti e due i capi");
        assert_eq!(l.rects[0].top[3], 0.5);
        assert_eq!(l.texts[0].color[3], 128);
        assert_eq!(l.texts[0].x, 10.0);
    }
}
