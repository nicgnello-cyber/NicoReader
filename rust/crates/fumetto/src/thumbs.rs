//! Le miniature: tutte le pagine del volume in una griglia sul nero, come
//! provini a contatto. Si vede dove si e' (la pagina a schermo ha il suo filo
//! rosso), dove si e' lasciato un segno (un nastrino), e un clic porta li'.
//!
//! Come la libreria (`shelf`): qui si decide cosa sta dove e cosa fanno tasti
//! e clic; le miniature le disegna la scheda video nei riquadri che dice
//! [`Thumbs::slots`], e il resto e' una [`Layer`] sopra di loro. Le celle
//! prendono la forma delle pagine del volume: niente vuoti attorno a tavole
//! orizzontali o a strisce.

use std::time::Instant;

use fumetto_core::lingua::{italian, t};
use fumetto_render::{Align, Face, Layer, Measure, Rect, Text};

use crate::reader::Action;
use crate::strip::glide_step;
use crate::ui::{
    self, ACCENT, BLACK, Command, HAIR, Handled, INK, INK_F, Icon, Key, MUTED, MUTED_F, alpha, caps, draw_icon, fit,
    rgb,
};

/// Cio' che le miniature devono sapere, dall'app.
pub struct ThumbsData<'a> {
    pub title: &'a str,
    pub pages: usize,
    /// La pagina che si sta leggendo.
    pub here: usize,
    /// Le pagine segnate, in ordine.
    pub bookmarks: &'a [usize],
    /// Altezza / larghezza tipica delle pagine: la forma delle celle.
    pub ratio: f32,
    /// La miniatura di questa pagina e' gia' sulla scheda video.
    pub ready: &'a dyn Fn(usize) -> bool,
}

/// Cio' che sta sotto il mouse.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Hit {
    Page(usize),
    Close,
    Bar,
}

/// Le misure della griglia per questa finestra.
struct Geo {
    s: f32,
    view: (f32, f32),
    top: f32,
    pad: f32,
    gap: f32,
    cols: usize,
    cell_w: f32,
    cell_h: f32,
    row_h: f32,
}

impl Geo {
    fn new(d: &ThumbsData, view: (f32, f32), s: f32) -> Geo {
        let pad = (44.0 * s).round();
        let gap = (24.0 * s).round();
        let avail = view.0 - 2.0 * pad;
        // celle piu' strette per le pagine larghe, cosi' l'area resta simile
        let ratio = d.ratio.clamp(0.4, 2.5);
        let want = 132.0 * s * (1.5 / ratio).sqrt().clamp(0.7, 1.9);
        let cols = (((avail + gap) / (want + gap)).floor() as usize).max(2);
        let cell_w = ((avail - (cols - 1) as f32 * gap) / cols as f32).floor();
        let cell_h = (cell_w * ratio).round().min((view.1 - ui::hud_height(s)) * 0.8);
        Geo { s, view, top: ui::hud_height(s), pad, gap, cols, cell_w, cell_h, row_h: cell_h + (44.0 * s).round() }
    }

    /// La cella della pagina `i`, senza scorrimento: (x, y).
    fn cell(&self, i: usize) -> (f32, f32) {
        let (row, col) = (i / self.cols, i % self.cols);
        (self.pad + col as f32 * (self.cell_w + self.gap), self.top + (28.0 * self.s).round() + row as f32 * self.row_h)
    }

    fn height(&self, pages: usize) -> f32 {
        pages.div_ceil(self.cols) as f32 * self.row_h + (56.0 * self.s).round()
    }

    fn max_scroll(&self, pages: usize) -> f32 {
        (self.height(pages) - (self.view.1 - self.top)).max(0.0)
    }
}

#[derive(Default)]
pub struct Thumbs {
    scroll: f32,
    /// Pixel di scorrimento ancora da percorrere (come nel nastro).
    glide: f32,
    last: Option<Instant>,
    /// La pagina scelta con la tastiera.
    selected: Option<usize>,
    hover: Option<Hit>,
    /// Appena aperte: la pagina da portare al centro, quando si conosce la griglia.
    reveal: Option<usize>,
}

impl Thumbs {
    /// Si aprono sulla pagina che si sta leggendo.
    pub fn open(&mut self, here: usize) {
        *self = Thumbs { reveal: Some(here), selected: Some(here), ..Thumbs::default() };
    }

    /// Porta a schermo cio' che e' in sospeso (l'apertura) e tiene lo
    /// scorrimento nei limiti (la finestra puo' essere cambiata).
    fn settle(&mut self, d: &ThumbsData, g: &Geo) {
        if let Some(p) = self.reveal.take() {
            let (_, y) = g.cell(p.min(d.pages.saturating_sub(1)));
            self.scroll = y + g.cell_h / 2.0 - g.top - (g.view.1 - g.top) / 2.0;
        }
        self.scroll = self.scroll.clamp(0.0, g.max_scroll(d.pages));
    }

    /// Dove disegnare le miniature, e quali preparare: quelle a schermo,
    /// poi una fila sopra e due sotto. (pagina, x, y, larghezza, altezza della
    /// cella, a schermo).
    pub fn slots(&mut self, d: &ThumbsData, view: (f32, f32), s: f32) -> Vec<(usize, f32, f32, f32, f32, bool)> {
        let g = Geo::new(d, view, s);
        self.settle(d, &g);
        let first_row = ((self.scroll - g.row_h - 28.0 * s) / g.row_h).floor().max(0.0) as usize;
        let rows = ((view.1 - g.top) / g.row_h).ceil() as usize + 4;
        let mut v: Vec<_> = (first_row * g.cols..((first_row + rows) * g.cols).min(d.pages))
            .map(|i| {
                let (x, y) = g.cell(i);
                let y = y - self.scroll;
                let lift = if self.hover == Some(Hit::Page(i)) { (4.0 * s).round() } else { 0.0 };
                (i, x, y - lift, g.cell_w, g.cell_h, y + g.cell_h > g.top && y < view.1)
            })
            .collect();
        v.sort_by_key(|slot| !slot.5);
        v
    }

    /// La misura delle celle, per preparare le miniature.
    pub fn cell_size(d: &ThumbsData, view: (f32, f32), s: f32) -> (u32, u32) {
        let g = Geo::new(d, view, s);
        (g.cell_w.max(1.0) as u32, g.cell_h.max(1.0) as u32)
    }

    fn hit(&self, d: &ThumbsData, view: (f32, f32), s: f32, x: f32, y: f32) -> Option<Hit> {
        let g = Geo::new(d, view, s);
        if y < g.top {
            let cx = view.0 - 14.0 * s - 36.0 * s;
            return Some(if x >= cx && x < cx + 36.0 * s { Hit::Close } else { Hit::Bar });
        }
        let row = ((y + self.scroll - g.top - 28.0 * s) / g.row_h).floor();
        let col = ((x - g.pad) / (g.cell_w + g.gap)).floor();
        if row < 0.0 || col < 0.0 || col as usize >= g.cols {
            return None;
        }
        let i = row as usize * g.cols + col as usize;
        let (cx, cy) = g.cell(i);
        let inside = x < cx + g.cell_w && y + self.scroll < cy + g.row_h - 8.0 * s;
        (i < d.pages && inside).then_some(Hit::Page(i))
    }

    pub fn motion(&mut self, d: &ThumbsData, view: (f32, f32), s: f32, x: f32, y: f32) -> bool {
        let hover = self.hit(d, view, s, x, y).filter(|h| *h != Hit::Bar);
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    pub fn click(&mut self, d: &ThumbsData, view: (f32, f32), s: f32, x: f32, y: f32) -> Handled {
        match self.hit(d, view, s, x, y) {
            Some(Hit::Page(i)) => Handled::Yes(Some(Command::Page(i))),
            Some(Hit::Close) => Handled::Yes(Some(Command::Act(Action::ToggleThumbs))),
            Some(Hit::Bar) | None => Handled::Yes(None),
        }
    }

    pub fn wheel(&mut self, pixels: f32) {
        self.glide += pixels;
    }

    pub fn key(&mut self, key: Key, d: &ThumbsData, view: (f32, f32), s: f32) -> Handled {
        let g = Geo::new(d, view, s);
        let last = d.pages.saturating_sub(1);
        let at = self.selected.unwrap_or(d.here);
        let visible_rows = (((view.1 - g.top) / g.row_h).floor() as usize).max(1);
        let step = |by: isize| (at as isize + by).clamp(0, last as isize) as usize;
        self.selected = Some(match key {
            Key::Right => step(1),
            Key::Left => step(-1),
            Key::Down => step(g.cols as isize),
            Key::Up => step(-(g.cols as isize)),
            Key::PageDown => step((g.cols * visible_rows) as isize),
            Key::PageUp => step(-((g.cols * visible_rows) as isize)),
            Key::Home => 0,
            Key::End => last,
            Key::Enter => return Handled::Yes(Some(Command::Page(at))),
            Key::Escape | Key::Char('t' | 'T') => return Handled::Yes(Some(Command::Act(Action::ToggleThumbs))),
            _ => return Handled::Yes(None),
        });
        self.reveal_selected(d, &g);
        Handled::Yes(None)
    }

    /// La miniatura scelta con la tastiera si porta tutta a schermo.
    fn reveal_selected(&mut self, d: &ThumbsData, g: &Geo) {
        let Some(i) = self.selected else { return };
        let (_, y) = g.cell(i);
        let y = y - self.scroll;
        let (lo, hi) = (g.top + 20.0 * g.s, g.view.1 - 20.0 * g.s);
        let delta = if y < lo {
            y - lo
        } else if y + g.row_h > hi {
            y + g.row_h - hi
        } else {
            0.0
        };
        self.scroll = (self.scroll + delta).clamp(0.0, g.max_scroll(d.pages));
        self.glide = 0.0;
    }

    /// Fa avanzare lo scorrimento morbido; `true` se c'e' ancora movimento.
    pub fn advance(&mut self, d: &ThumbsData, view: (f32, f32), s: f32, now: Instant) -> bool {
        let g = Geo::new(d, view, s);
        self.settle(d, &g);
        let dt = self.last.map_or(1.0 / 60.0, |t| (now - t).as_secs_f32()).min(0.05);
        if self.glide.abs() < 0.5 {
            (self.glide, self.last) = (0.0, None);
            return false;
        }
        self.last = Some(now);
        let step = glide_step(self.glide, dt);
        self.glide -= step;
        let before = self.scroll;
        self.scroll = (self.scroll + step).clamp(0.0, g.max_scroll(d.pages));
        if self.scroll == before {
            self.glide = 0.0; // contro il bordo
        }
        true
    }

    pub fn gliding(&self) -> bool {
        self.glide.abs() >= 0.5
    }

    /// Le miniature sono gia' disegnate: qui tutto il resto, sopra di loro.
    pub fn layers(&self, d: &ThumbsData, view: (f32, f32), s: f32, m: &mut dyn Measure) -> Vec<Layer> {
        let g = Geo::new(d, view, s);
        let mut l = Layer::default();
        let first_row = (self.scroll / g.row_h).floor().max(0.0) as usize;
        let rows = ((view.1 - g.top) / g.row_h).ceil() as usize + 2;
        for i in first_row * g.cols..((first_row + rows) * g.cols).min(d.pages) {
            let (x, y) = g.cell(i);
            let y = y - self.scroll;
            if y + g.row_h < g.top || y > view.1 {
                continue;
            }
            let hovered = self.hover == Some(Hit::Page(i));
            let y = y - if hovered { (4.0 * s).round() } else { 0.0 };
            let (w, h) = (g.cell_w, g.cell_h);
            if !(d.ready)(i) {
                l.rects.push(Rect::new(x, y, w, h, rgb(0x17, 0x16, 0x14)));
            }
            if self.selected == Some(i) {
                let o = 5.0 * s;
                l.rects.push(
                    Rect::new(x - o, y - o, w + 2.0 * o, h + 2.0 * o, ACCENT).radius(4.0 * s).stroke((2.0 * s).round()),
                );
            }
            if d.bookmarks.binary_search(&i).is_ok() {
                // un nastrino che spunta dal bordo alto, come fra le pagine di un libro
                let (bw, bh) = ((7.0 * s).round(), (18.0 * s).round());
                l.rects
                    .push(Rect::new((x + w - 16.0 * s).round(), (y - 4.0 * s).round(), bw, bh, ACCENT).radius(1.0 * s));
            }
            let here = i == d.here;
            if here {
                // la pagina che si legge: un filo sotto, come nella libreria
                l.rects.push(Rect::new(x, (y + h + 7.0 * s).round(), w, (2.0 * s).round(), ACCENT));
            }
            let color = if here || hovered { INK } else { MUTED };
            l.texts.push(
                Text::new((i + 1).to_string(), Face::Sans, 12.0 * s, color)
                    .tabular()
                    .boxed(w, Align::Center)
                    .on_baseline(x, y + h + 28.0 * s),
            );
        }
        vec![l, self.bar(d, &g, m)]
    }

    /// La barra in alto: il titolo, quante pagine, il tasto per tornare.
    fn bar(&self, d: &ThumbsData, g: &Geo, m: &mut dyn Measure) -> Layer {
        let s = g.s;
        let (vw, h) = (g.view.0, g.top);
        let cy = (h / 2.0).round();
        let mut l = Layer::default();
        l.rects.push(Rect::new(0.0, 0.0, vw, h, BLACK));
        l.rects.push(Rect::new(0.0, h - s.round(), vw, s.round(), alpha(HAIR, 0.8)));
        let count = caps(
            &if italian() { format!("{} pagine", d.pages) } else { format!("{} pages", d.pages) },
            11.0 * s,
            MUTED,
        )
        .tabular();
        let count_w = m.width(&count);
        let x0 = (24.0 * s).round();
        let title = fit(m, Text::new(d.title, Face::Serif, 22.0 * s, INK), vw - x0 - count_w - 160.0 * s);
        let title_w = m.width(&title);
        l.texts.push(title.on_baseline(x0, cy + 7.5 * s));
        l.texts.push(count.on_baseline(x0 + title_w + 18.0 * s, cy + 5.0 * s));

        let x = vw - 14.0 * s - 36.0 * s;
        let hovered = self.hover == Some(Hit::Close);
        let by = ((h - 36.0 * s) / 2.0).round();
        if hovered {
            l.rects.push(Rect::new(x, by, 36.0 * s, 36.0 * s, [1.0, 1.0, 1.0, 0.075]).radius(8.0 * s));
        }
        draw_icon(&mut l, Icon::Close, (x + 18.0 * s).round(), cy, s, if hovered { INK_F } else { MUTED_F });
        if hovered {
            ui::tooltip(
                &mut l,
                m,
                t("Torna alla lettura   \u{00b7}   Esc", "Back to reading   \u{00b7}   Esc"),
                x + 18.0 * s,
                h,
                vw,
                s,
            );
        }
        l
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(pages: usize, here: usize, bookmarks: &[usize]) -> ThumbsData<'_> {
        ThumbsData { title: "Nebbia sul Porto v03", pages, here, bookmarks, ratio: 1.5, ready: &|_| true }
    }

    const VIEW: (f32, f32) = (1920.0, 1080.0);

    #[test]
    fn si_aprono_sulla_pagina_che_si_legge() {
        let d = data(300, 200, &[]);
        let mut th = Thumbs::default();
        th.open(200);
        let slots = th.slots(&d, VIEW, 1.0);
        let here = slots.iter().find(|s| s.0 == 200).expect("la pagina 201 fra quelle da preparare");
        assert!(here.5, "e a schermo");
        assert!(slots.iter().take_while(|s| s.5).count() >= 10, "prima quelle a schermo");
    }

    #[test]
    fn un_clic_porta_alla_pagina() {
        let d = data(40, 0, &[]);
        let mut th = Thumbs::default();
        th.open(0);
        let (i, x, y, w, h, _) = th.slots(&d, VIEW, 1.0).into_iter().find(|s| s.0 == 3).unwrap();
        assert_eq!(i, 3);
        assert_eq!(th.click(&d, VIEW, 1.0, x + w / 2.0, y + h / 2.0), Handled::Yes(Some(Command::Page(3))));
        assert_eq!(th.click(&d, VIEW, 1.0, 1900.0, 20.0), Handled::Yes(Some(Command::Act(Action::ToggleThumbs))));
    }

    #[test]
    fn con_le_frecce_e_invio() {
        let d = data(100, 10, &[]);
        let mut th = Thumbs::default();
        th.open(10);
        th.key(Key::Right, &d, VIEW, 1.0);
        th.key(Key::Down, &d, VIEW, 1.0);
        let cols = Geo::new(&d, VIEW, 1.0).cols;
        assert_eq!(th.key(Key::Enter, &d, VIEW, 1.0), Handled::Yes(Some(Command::Page(11 + cols))));
        th.key(Key::End, &d, VIEW, 1.0);
        assert_eq!(th.key(Key::Enter, &d, VIEW, 1.0), Handled::Yes(Some(Command::Page(99))));
        assert_eq!(th.key(Key::Escape, &d, VIEW, 1.0), Handled::Yes(Some(Command::Act(Action::ToggleThumbs))));
    }

    #[test]
    fn le_celle_prendono_la_forma_delle_pagine() {
        let tall = Geo::new(&data(10, 0, &[]), VIEW, 1.0);
        let wide = Geo::new(&ThumbsData { ratio: 0.7, ..data(10, 0, &[]) }, VIEW, 1.0);
        assert!(tall.cell_h > tall.cell_w && wide.cell_h < wide.cell_w);
        assert!(wide.cols < tall.cols, "le tavole larghe hanno celle piu' larghe");
    }
}
