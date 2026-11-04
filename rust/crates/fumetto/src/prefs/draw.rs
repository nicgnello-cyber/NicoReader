//! Il disegno del foglio: il fondo, le righe, e in alto il titolo con le
//! sezioni.

use super::*;

impl Prefs {
    pub fn layers(
        &self, settings: &Settings, keys: &Keymap, view: (f32, f32), s: f32, m: &mut dyn Measure,
    ) -> Vec<Layer> {
        let g = Geo::new(view, s);
        let rows = self.rows(settings, keys);
        let tops = self.tops(&rows, &g);
        let mut l = Layer::default();
        for (i, (row, &top)) in rows.iter().zip(&tops).enumerate() {
            let h = row.height() * s;
            if top + h >= HEADER * s && top <= view.1 {
                self.draw_row(&mut l, settings, &g, row, (i, top), m);
            }
        }
        vec![background(&g), l, self.header(&g)]
    }

    /// Una riga, con la cima in `top`; scelta o sotto il mouse, illuminata.
    fn draw_row(
        &self, l: &mut Layer, settings: &Settings, g: &Geo, row: &Row, (i, top): (usize, f32), m: &mut dyn Measure,
    ) {
        let (s, x0, w, pad) = (g.s, g.x, g.w, g.pad);
        let h = row.height() * s;
        let hovered = self.hover == Some(Hit::Row(i));
        let chosen = self.selected == Some(i) || hovered;
        if chosen && row.selectable() {
            l.rects.push(
                Rect::new(x0 + 14.0 * s, top + 2.0 * s, w - 28.0 * s, h - 4.0 * s, [1.0, 1.0, 1.0, 0.055])
                    .radius(8.0 * s),
            );
        }
        match row {
            Row::Head(text) => l.texts.push(caps(text, 10.5 * s, MUTED).on_baseline(x0 + pad, top + 32.0 * s)),
            Row::Note(text) => {
                let note = Text::new(*text, Face::Sans, 12.0 * s, MUTED).boxed(w - 2.0 * pad, Align::Left);
                l.texts.push(note.on_baseline(x0 + pad, top + 20.0 * s));
            }
            Row::Slider { label, which } => {
                let lit = self.drag == Some(*which) || hovered;
                draw_slider(l, g, label, *which, which.value(settings), top, lit);
            }
            Row::Toggle { label, on, .. } => draw_toggle(l, g, label, *on, top, h),
            Row::Stepper { label, value, .. } => {
                let lit = (self.hover == Some(Hit::Less(i)), self.hover == Some(Hit::More(i)));
                draw_stepper(l, g, label, value, (top, h), lit, m);
            }
            Row::Button { label, .. } => {
                let color = if chosen { INK } else { crate::ui::ACCENT_TEXT };
                l.texts.push(caps(label, 11.0 * s, color).on_baseline(x0 + pad, top + 28.0 * s));
            }
            Row::Key { bind, keys } => draw_keys(l, g, *bind, keys, self.capture == Some(*bind), top, m),
        }
    }

    /// In alto, sopra le righe che scorrono: il titolo, la crocetta e le sezioni.
    fn header(&self, g: &Geo) -> Layer {
        let (s, x0, w, pad) = (g.s, g.x, g.w, g.pad);
        let mut head = Layer::default();
        head.rects.push(Rect::new(x0 + s, 0.0, w - s, HEADER * s, alpha(rgb(0x11, 0x10, 0x0E), 1.0)));
        head.rects.push(Rect::new(x0 + pad, (HEADER * s - 10.0 * s).round(), w - 2.0 * pad, s.round(), HAIR));
        head.texts
            .push(Text::new(t("Impostazioni", "Settings"), Face::Serif, 30.0 * s, INK).on_baseline(x0 + pad, 54.0 * s));
        let close = if self.hover == Some(Hit::Close) { INK_F } else { MUTED_F };
        draw_icon(&mut head, Icon::Close, (g.right() - 12.0 * s).round(), (44.0 * s).round(), s, close);
        for (i, tab) in Tab::ALL.iter().enumerate() {
            let (tx, tw) = g.tab_x(i);
            let on = *tab == self.tab;
            let color = if on || self.hover == Some(Hit::Tab(*tab)) { INK } else { MUTED };
            head.texts.push(caps(tab.label(), 10.5 * s, color).boxed(tw, Align::Center).on_baseline(tx, 96.0 * s));
            if on {
                let d = 4.0 * s;
                head.rects.push(Rect::new(tx + tw / 2.0 - d / 2.0, 104.0 * s, d, d, ACCENT).radius(d));
            }
        }
        head
    }
}

/// Il fondo del foglio: l'ombra verso le pagine, un filo, il fondo.
fn background(g: &Geo) -> Layer {
    let (s, x0, w, vh) = (g.s, g.x, g.w, g.view.1);
    let mut back = Layer::default();
    back.rects.push(Rect::new(x0 - 30.0 * s, 0.0, 60.0 * s, vh, [0.0, 0.0, 0.0, 0.6]).blur(24.0 * s));
    back.rects.push(Rect::new(x0, 0.0, w, vh, alpha(rgb(0x11, 0x10, 0x0E), 0.985)));
    back.rects.push(Rect::new(x0, 0.0, s.round(), vh, alpha(HAIR, 0.9)));
    back
}

/// Il nome di una riga, a sinistra.
fn label(g: &Geo, text: &str, top: f32, base: f32) -> Text {
    Text::new(text, Face::Sans, 14.0 * g.s, INK).weight(450).on_baseline(g.x + g.pad, top + base * g.s)
}

/// Il valore di una riga, a destra.
fn value(g: &Geo, text: String, top: f32, base: f32, color: [u8; 4]) -> Text {
    let s = g.s;
    Text::new(text, Face::Sans, 13.0 * s, color)
        .tabular()
        .boxed(140.0 * s, Align::Right)
        .on_baseline(g.right() - 140.0 * s, top + base * s)
}

/// Una regolazione: nome e valore (acceso se cambiato), la pista, il tratto
/// dal punto di partenza al valore, la manopola (`lit`: sotto il mouse o
/// trascinata).
fn draw_slider(l: &mut Layer, g: &Geo, name: &str, which: Slider, v: f32, top: f32, lit: bool) {
    let s = g.s;
    l.texts.push(label(g, name, top, 26.0));
    let changed = v != which.range().3 && matches!(which, Slider::Brightness | Slider::Contrast | Slider::Gamma);
    l.texts.push(value(g, which.shown(v), top, 26.0, if changed { INK } else { MUTED }));
    let (tx, tw, ty) = g.track(top);
    let (lo, hi, _, start) = which.range();
    let at = |v: f32| (tx + (v - lo) / (hi - lo) * tw).round();
    l.rects.push(Rect::new(tx, ty, tw, s.round().max(1.0), TICK));
    // il tratto fra il punto di partenza (lo zero) e il valore
    let (a, b) = (at(start).min(at(v)), at(start).max(at(v)));
    l.rects.push(Rect::new(a, ty - s.round(), (b - a).max(1.0), (3.0 * s).round(), ACCENT));
    if start > lo {
        // lo zero delle regolazioni che vanno nei due sensi: una tacca
        l.rects.push(Rect::new(at(start), ty - 5.0 * s, s.round(), 11.0 * s, TICK));
    }
    let d = (14.0 * s).round();
    let knob = if lit { INK_F } else { rgb(0xD6, 0xD0, 0xC5) };
    l.rects.push(Rect::new(at(v) - d / 2.0, ty - d / 2.0, d, d, knob).radius(d / 2.0));
}

/// Un interruttore: acceso, rosso con il pallino a destra.
fn draw_toggle(l: &mut Layer, g: &Geo, name: &str, on: bool, top: f32, h: f32) {
    let s = g.s;
    l.texts.push(label(g, name, top, 28.0));
    let (sw, sh) = ((34.0 * s).round(), (18.0 * s).round());
    let (sx, sy) = (g.right() - sw, (top + (h - sh) / 2.0).round());
    l.rects.push(Rect::new(sx, sy, sw, sh, if on { ACCENT } else { rgb(0x3A, 0x37, 0x32) }).radius(sh / 2.0));
    let d = sh - 4.0 * s;
    let kx = if on { sx + sw - 2.0 * s - d } else { sx + 2.0 * s };
    l.rects.push(Rect::new(kx, sy + 2.0 * s, d, d, INK_F).radius(d / 2.0));
}

/// Una riga a passi: il nome, il meno, il valore, il piu' (`lit`: quale dei
/// due e' sotto il mouse).
fn draw_stepper(
    l: &mut Layer, g: &Geo, name: &str, v: &str, (top, h): (f32, f32), lit: (bool, bool), m: &mut dyn Measure,
) {
    let s = g.s;
    let right = g.right();
    let room = g.w - 2.0 * g.pad - 130.0 * s;
    l.texts.push(
        fit(m, Text::new(name, Face::Sans, 14.0 * s, INK).weight(450), room).on_baseline(g.x + g.pad, top + 28.0 * s),
    );
    for (on, icon, cx) in [(lit.0, Icon::Minus, right - 93.0 * s), (lit.1, Icon::Plus, right - 14.0 * s)] {
        draw_icon(l, icon, cx.round(), (top + h / 2.0).round(), s * 0.8, if on { INK_F } else { MUTED_F });
    }
    l.texts.push(
        Text::new(v, Face::Sans, 13.0 * s, INK)
            .tabular()
            .boxed(52.0 * s, Align::Center)
            .on_baseline(right - 80.0 * s, top + 28.0 * s),
    );
}

/// Un comando e i suoi tasti, come piccole targhe da destra; o, mentre si
/// aspetta il tasto nuovo (`capturing`), l'invito a premerlo.
fn draw_keys(l: &mut Layer, g: &Geo, bind: Bind, keys: &[String], capturing: bool, top: f32, m: &mut dyn Measure) {
    let s = g.s;
    let name = Text::new(bind.label(), Face::Sans, 13.5 * s, INK).weight(450);
    l.texts.push(fit(m, name, g.w - 2.0 * g.pad - 150.0 * s).on_baseline(g.x + g.pad, top + 24.0 * s));
    if capturing {
        let text = t("premi un tasto\u{2026}", "press a key\u{2026}").to_owned();
        l.texts.push(value(g, text, top, 24.0, crate::ui::ACCENT_TEXT));
        return;
    }
    let mut kx = g.right();
    for k in keys.iter().take(2).rev() {
        let text = Text::new(k.as_str(), Face::Sans, 12.0 * s, INK).weight(500);
        let kw = (m.width(&text) + 16.0 * s).round();
        kx -= kw;
        let ky = (top + 8.0 * s).round();
        l.rects.push(Rect::new(kx, ky, kw, (22.0 * s).round(), [1.0, 1.0, 1.0, 0.07]).radius(5.0 * s));
        l.texts.push(text.boxed(kw, Align::Center).on_baseline(kx, ky + 15.5 * s));
        kx -= 6.0 * s;
    }
    if keys.is_empty() {
        l.texts.push(value(g, "\u{2014}".to_owned(), top, 24.0, MUTED));
    }
}
