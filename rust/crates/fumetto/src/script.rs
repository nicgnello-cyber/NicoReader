//! La prova automatica (`--prova`): un lettore finto che usa l'app come una
//! persona, con i tempi di una persona, mentre le misure girano.
//!
//! 1. aspetta la prima pagina
//! 2. 40 giri di pagina con calma (uno ogni 150 ms dopo che il precedente e' arrivato)
//! 3. 20 giri alla velocita' della ripetizione di un tasto tenuto premuto (30 al secondo)
//! 4. doppia pagina: 10 giri con calma, poi di nuovo pagina singola
//! 5. zoom: ingrandisce, si sposta, torna alla pagina intera
//! 6. passa al nastro e scorre per 6 secondi, con una spinta ogni 250 ms

use std::time::{Duration, Instant};

use crate::app::App;
use fumetto::reader::Action;

pub enum Step {
    Do(Action),
    Wait,
    Done,
}

enum Phase {
    FirstPage,
    Reading,
    Flipping,
    Double,
    Zooming,
    ToStrip,
    Scrolling { until: Instant },
    Settling,
}

pub struct Script {
    phase: Phase,
    next_at: Instant,
    count: u32,
    started: Instant,
}

impl Script {
    pub fn new() -> Script {
        let now = Instant::now();
        Script { phase: Phase::FirstPage, next_at: now, count: 0, started: now }
    }

    fn enter(&mut self, phase: Phase, what: &str) {
        eprintln!("[prova] {:5.1} s  {what}", self.started.elapsed().as_secs_f32());
        self.phase = phase;
        self.count = 0;
    }

    pub fn step(&mut self, app: &App) -> Step {
        let now = Instant::now();
        if now - self.started > Duration::from_secs(60) {
            eprintln!("[prova] tempo scaduto: qualcosa si e' incastrato ({})", app.describe_state());
            return Step::Done;
        }
        if now < self.next_at {
            return Step::Wait;
        }
        match self.phase {
            Phase::FirstPage => {
                if app.page_ready() && app.settled() {
                    eprintln!("[prova] si parte da: {}", app.describe_state());
                    self.enter(Phase::Reading, "prima pagina a schermo: 40 giri con calma");
                    self.next_at = now + Duration::from_millis(500);
                }
                Step::Wait
            }
            Phase::Reading => {
                if !app.settled() {
                    return Step::Wait;
                }
                self.count += 1;
                if self.count == 40 {
                    self.enter(Phase::Flipping, "20 giri a tasto premuto");
                }
                self.next_at = now + Duration::from_millis(150);
                Step::Do(Action::Next)
            }
            Phase::Flipping => {
                self.count += 1;
                if self.count == 20 {
                    self.enter(Phase::Double, "doppia pagina: 10 giri con calma");
                    self.next_at = now + Duration::from_millis(800);
                } else {
                    self.next_at = now + Duration::from_millis(33);
                }
                Step::Do(Action::Next)
            }
            Phase::Double => {
                if !app.settled() {
                    return Step::Wait;
                }
                self.count += 1;
                self.next_at = now + Duration::from_millis(150);
                match self.count {
                    1 => Step::Do(Action::ToggleDouble),
                    2..=11 => Step::Do(Action::Next),
                    _ => {
                        self.enter(Phase::Zooming, "zoom: ingrandisce, si sposta, torna intera");
                        Step::Do(Action::ToggleDouble)
                    }
                }
            }
            Phase::Zooming => {
                let (w, h) = (app.view_height() * 16.0 / 9.0, app.view_height());
                let steps = [
                    Action::Zoom { factor: 2.5, x: w * 0.4, y: h * 0.3 },
                    Action::Pan(-200.0, -300.0),
                    Action::Pan(-200.0, -300.0),
                    Action::Zoom { factor: 1.5, x: w / 2.0, y: h / 2.0 },
                    Action::Pan(400.0, 0.0),
                    Action::ZoomReset,
                ];
                let step = steps.get(self.count as usize).copied();
                self.count += 1;
                self.next_at = now + Duration::from_millis(250);
                match step {
                    Some(a) => Step::Do(a),
                    None => {
                        self.enter(Phase::ToStrip, "pausa, poi nastro");
                        self.next_at = now + Duration::from_millis(800);
                        Step::Wait
                    }
                }
            }
            Phase::ToStrip => {
                self.enter(Phase::Scrolling { until: now + Duration::from_secs(6) }, "nastro: 6 s di scorrimento");
                self.next_at = now + Duration::from_millis(500);
                Step::Do(Action::ToggleStrip)
            }
            Phase::Scrolling { until } => {
                if now > until {
                    self.enter(Phase::Settling, "fine scorrimento");
                    return Step::Wait;
                }
                self.next_at = now + Duration::from_millis(250);
                Step::Do(Action::Scroll(app.view_height() * 0.35))
            }
            Phase::Settling if app.settled() => {
                eprintln!("[prova] {:5.1} s  fermo: fine", self.started.elapsed().as_secs_f32());
                Step::Done
            }
            Phase::Settling => {
                eprintln!(
                    "[prova] {:5.1} s  in attesa: {}",
                    self.started.elapsed().as_secs_f32(),
                    app.describe_state()
                );
                self.next_at = now + Duration::from_secs(1);
                Step::Wait
            }
        }
    }
}
