//! Il tocco (schermi touch e penne) e i gesti del trackpad.
//!
//! Un dito fa da mouse: toccare e' un clic (ai lati gira pagina, come il
//! mouse), trascinare sposta la pagina ingrandita, scorre il nastro, la
//! libreria e le miniature. Strisciare di lato gira pagina; due tocchi al
//! centro ingrandiscono la pagina (o la riportano intera); il dito tenuto
//! giu' apre il menu. Due dita ingrandiscono (nel nastro, lo allargano) e
//! spostano. Il pizzico e il doppio tocco del trackpad del Mac arrivano come
//! gesti a parte.

use super::*;
use winit::event::{Touch, TouchPhase};

/// Quanto tenere giu' il dito perche' sia il menu e non un tocco.
const LONG_PRESS: Duration = Duration::from_millis(550);
/// Due tocchi entro questo tempo sono un doppio tocco.
const DOUBLE_TAP: Duration = Duration::from_millis(350);

#[derive(Default)]
pub(super) struct Touches {
    /// Le dita giu': quale, e dove.
    fingers: Vec<(u64, (f32, f32))>,
    /// Il gesto di un dito: dove e quando e' cominciato.
    start: Option<(f32, f32, Instant)>,
    /// Ci sono state due dita: il gesto non e' piu' un tocco ne' uno strisciare.
    pinched: bool,
    /// Distanza e centro delle due dita all'ultimo movimento.
    pinch: Option<(f32, (f32, f32))>,
    /// Le ultime altezze del dito, per la spinta del nastro.
    trail: Vec<(f32, Instant)>,
    /// L'ultimo tocco (per il doppio tocco).
    last_tap: Option<(Instant, f32, f32)>,
}

impl Touches {
    fn two(&self) -> Option<(f32, (f32, f32))> {
        let [(_, a), (_, b), ..] = self.fingers[..] else { return None };
        Some((((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt(), ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)))
    }
}

impl App {
    pub(super) fn touch(&mut self, t: Touch, event_loop: &ActiveEventLoop) {
        let (x, y) = (t.location.x as f32, t.location.y as f32);
        let now = Instant::now();
        match t.phase {
            TouchPhase::Started => {
                self.touches.fingers.retain(|f| f.0 != t.id);
                self.touches.fingers.push((t.id, (x, y)));
                match self.touches.fingers.len() {
                    1 => {
                        let last_tap = self.touches.last_tap;
                        self.touches = Touches {
                            fingers: std::mem::take(&mut self.touches.fingers),
                            start: Some((x, y, now)),
                            trail: vec![(y, now)],
                            last_tap,
                            ..Touches::default()
                        };
                        self.pointer_moved(x, y, event_loop);
                        self.left_down(event_loop);
                    }
                    2 => {
                        // due dita: niente clic alla fine
                        self.touches.pinched = true;
                        self.press = None;
                        self.touches.pinch = self.touches.two();
                    }
                    _ => {}
                }
            }
            TouchPhase::Moved => {
                let Some(f) = self.touches.fingers.iter_mut().find(|f| f.0 == t.id) else { return };
                let old_y = f.1.1;
                f.1 = (x, y);
                if self.touches.pinched {
                    return self.pinch_step(event_loop);
                }
                self.touches.trail.push((y, now));
                if self.touches.trail.len() > 6 {
                    self.touches.trail.remove(0);
                }
                self.finger_moved(x, y, y - old_y, event_loop);
            }
            TouchPhase::Ended => {
                self.touches.fingers.retain(|f| f.0 != t.id);
                if self.touches.pinched {
                    if self.touches.fingers.is_empty() {
                        self.touches.pinched = false;
                        self.touches.pinch = None;
                        self.press = None;
                    }
                    return;
                }
                self.finger_up(x, y, now, event_loop);
            }
            TouchPhase::Cancelled => {
                self.touches.fingers.retain(|f| f.0 != t.id);
                self.press = None;
                self.touches.start = None;
            }
        }
    }

    /// Un dito solo si muove: come il mouse trascinato; in piu', in su e in
    /// giu' scorrono libreria, miniature e nastro.
    fn finger_moved(&mut self, x: f32, y: f32, dy: f32, event_loop: &ActiveEventLoop) {
        let s = self.scale();
        // un dito trema: un clic resta un clic entro qualche pixel in piu' del mouse
        let far = self.touches.start.is_some_and(|(sx, sy, _)| (x - sx).abs() + (y - sy).abs() > 14.0 * s);
        if !far && self.press.as_ref().is_some_and(|p| !p.dragged) {
            self.cursor = (x, y);
            return;
        }
        self.pointer_moved(x, y, event_loop);
        if let Some(p) = &mut self.press {
            p.dragged = true;
        }
        let zoomed = self.reader.as_ref().is_some_and(|r| r.zoomed());
        // le regolazioni si trascinano; la pagina ingrandita l'ha gia' spostata il puntatore
        if self.ui.prefs.is_some() || (zoomed && !self.shelf_shown() && !self.thumbs_shown()) {
            return;
        }
        let ctx = input_context!(self);
        if self.ui.wheel(-dy, &ctx) {
            return self.request_redraw();
        }
        if !self.ui.modal() && self.reader.as_ref().is_some_and(|r| r.mode == Mode::Strip) {
            self.act(Action::Scroll(-dy), event_loop);
        }
    }

    /// Il dito si alza: uno strisciare gira pagina, un dito tenuto apre il
    /// menu, nel nastro la spinta continua lo scorrimento; se no e' un tocco.
    fn finger_up(&mut self, x: f32, y: f32, now: Instant, event_loop: &ActiveEventLoop) {
        let Some((sx, sy, since)) = self.touches.start.take() else { return };
        let (dx, dy) = (x - sx, y - sy);
        let s = self.scale();
        let reading = !self.ui.modal() && !self.shelf_shown() && !self.thumbs_shown() && self.ui.prefs.is_none();
        let reader = self.reader.as_ref();
        let paged = reader.is_some_and(|r| r.mode != Mode::Strip && !r.zoomed());
        if reading
            && paged
            && let Some(turn) = swipe(dx, dy, s, reader.is_some_and(|r| r.manga))
        {
            self.press = None;
            return self.act(turn, event_loop);
        }
        let dragged = self.press.as_ref().is_some_and(|p| p.dragged);
        if !dragged && now.duration_since(since) >= LONG_PRESS {
            self.press = None;
            self.cursor = (x, y);
            return self.context_menu();
        }
        if dragged {
            // la spinta: il nastro continua per un po' alla velocita' del dito
            let strip = reader.is_some_and(|r| r.mode == Mode::Strip);
            if reading
                && strip
                && let (Some(&(y0, t0)), Some(&(y1, t1))) = (self.touches.trail.first(), self.touches.trail.last())
                && t1 > t0
                && now.duration_since(t1) < Duration::from_millis(80)
            {
                let speed = (y1 - y0) / (t1 - t0).as_secs_f32();
                self.act(Action::Scroll(-speed * 0.25), event_loop);
            }
            return self.left_up(event_loop);
        }
        // due tocchi vicini, lontano dai lati che girano pagina: ingrandisce
        let double = self.touches.last_tap.take().is_some_and(|(t, px, py)| {
            now.duration_since(t) <= DOUBLE_TAP && (x - px).abs() + (y - py).abs() < 40.0 * s
        });
        if reading && double && self.click(x).is_none() {
            self.press = None;
            return self.double_tap(x, y, event_loop);
        }
        self.touches.last_tap = Some((now, x, y));
        self.left_up(event_loop);
    }

    /// Due dita: la distanza ingrandisce, il centro sposta.
    fn pinch_step(&mut self, event_loop: &ActiveEventLoop) {
        let (Some((d0, c0)), Some((d1, c1))) = (self.touches.pinch, self.touches.two()) else { return };
        self.touches.pinch = Some((d1, c1));
        if d0 < 1.0 || self.ui.modal() || self.shelf_shown() || self.thumbs_shown() {
            return;
        }
        self.cursor = c1;
        self.zoom_by(d1 / d0, c1, event_loop);
        if self.reader.as_ref().is_some_and(|r| r.zoomed()) {
            self.act(Action::Pan(c1.0 - c0.0, c1.1 - c0.1), event_loop);
        }
    }

    /// Il pizzico del trackpad (Mac): `delta` e' quanto cambia l'ingrandimento.
    pub(super) fn pinch_gesture(&mut self, delta: f32, event_loop: &ActiveEventLoop) {
        if self.ui.modal() || self.shelf_shown() || self.thumbs_shown() {
            return;
        }
        self.zoom_by((1.0 + delta).clamp(0.5, 2.0), self.cursor, event_loop);
    }

    /// Ingrandisce di `factor` tenendo fermo il punto `at` della finestra; nel
    /// nastro lo allarga.
    fn zoom_by(&mut self, factor: f32, at: (f32, f32), event_loop: &ActiveEventLoop) {
        let Some(reader) = &self.reader else { return };
        let action = match reader.mode {
            Mode::Strip => Action::StripWider(factor),
            _ => Action::Zoom { factor, x: at.0, y: at.1 - self.top() as f32 },
        };
        self.act(action, event_loop);
    }

    /// Doppio tocco: la pagina intera si ingrandisce sul punto toccato; quella
    /// ingrandita torna intera.
    pub(super) fn double_tap(&mut self, x: f32, y: f32, event_loop: &ActiveEventLoop) {
        if self.ui.modal() || self.shelf_shown() || self.thumbs_shown() {
            return;
        }
        let Some(reader) = &self.reader else { return };
        let action = match (reader.mode, reader.zoomed()) {
            (Mode::Single, true) => Action::ZoomReset,
            (Mode::Single, false) => Action::Zoom { factor: 2.5, x, y: y - self.top() as f32 },
            _ => return,
        };
        self.act(action, event_loop);
    }
}

/// Uno strisciare di lato gira pagina: il dito verso sinistra va avanti (da
/// destra a sinistra, il contrario). Deve essere lungo e piu' di lato che
/// in verticale.
fn swipe(dx: f32, dy: f32, scale: f32, manga: bool) -> Option<Action> {
    if dx.abs() <= 50.0 * scale || dx.abs() <= 1.5 * dy.abs() {
        return None;
    }
    Some(if (dx < 0.0) != manga { Action::Next } else { Action::Prev })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strisciare_gira_pagina() {
        assert_eq!(swipe(-120.0, 10.0, 1.0, false), Some(Action::Next));
        assert_eq!(swipe(120.0, 10.0, 1.0, false), Some(Action::Prev));
        assert_eq!(swipe(120.0, 10.0, 1.0, true), Some(Action::Next), "manga: verso destra e' avanti");
        assert_eq!(swipe(-40.0, 0.0, 1.0, false), None, "troppo corto");
        assert_eq!(swipe(-80.0, 0.0, 2.0, false), None, "corto su uno schermo al 200%");
        assert_eq!(swipe(-120.0, 100.0, 1.0, false), None, "piu' in verticale che di lato");
    }
}
