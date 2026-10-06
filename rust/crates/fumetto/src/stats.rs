//! Le misure che decidono se il prototipo regge: quanto passa tra un tasto e
//! la pagina consegnata allo schermo, e se il nastro perde fotogrammi.

use std::fmt::Write;
use std::time::Instant;

pub struct Stats {
    pub started: Instant,
    /// Tappe dell'avvio, in ms dall'inizio del processo.
    pub window_ms: Option<f32>,
    pub gpu_ms: Option<f32>,
    pub first_loaded_ms: Option<f32>,
    pub first_page_ms: Option<f32>,
    /// Da tasto a pagina consegnata allo schermo, quando la pagina era pronta.
    pub turns: Vec<f32>,
    /// Giri di pagina in cui la pagina non era ancora pronta.
    pub misses: u32,
    /// Intervalli tra fotogrammi consegnati mentre il nastro scorre.
    pub frames: Vec<f32>,
    pub refresh_ms: f32,
    pub prepared: Vec<(f32, f32, f32)>,
    /// Attesa per avere il fotogramma su cui disegnare (dentro c'e' il vblank).
    pub acquire: Vec<f32>,
    pub acquire_failures: Vec<String>,
    pub notes: Vec<String>,
}

impl Stats {
    pub fn new(started: Instant) -> Stats {
        Stats {
            started,
            window_ms: None,
            gpu_ms: None,
            first_loaded_ms: None,
            first_page_ms: None,
            turns: Vec::new(),
            misses: 0,
            frames: Vec::new(),
            refresh_ms: 1000.0 / 60.0,
            prepared: Vec::new(),
            acquire: Vec::new(),
            acquire_failures: Vec::new(),
            notes: Vec::new(),
        }
    }

    /// Un fatto da ricordare nel resoconto, con il suo momento.
    pub fn note(&mut self, what: &str) {
        self.notes.push(format!("{:6.0} ms  {what}", self.now_ms()));
    }

    /// Millisecondi dall'avvio del processo.
    pub fn now_ms(&self) -> f32 {
        self.started.elapsed().as_secs_f32() * 1000.0
    }

    pub fn report(&self, gpu: &str) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "\n== NicoReader, misure del prototipo ==");
        let _ = writeln!(
            s,
            "scheda video: {gpu}, schermo a {:.0} Hz ({:.2} ms per fotogramma)",
            1000.0 / self.refresh_ms,
            self.refresh_ms
        );
        let ms = |v: Option<f32>| v.map_or("-".to_string(), |v| format!("{v:.0} ms"));
        let _ = writeln!(
            s,
            "avvio: finestra {}, scheda video pronta {}, prima pagina preparata {}, a schermo {}",
            ms(self.window_ms),
            ms(self.gpu_ms),
            ms(self.first_loaded_ms),
            ms(self.first_page_ms)
        );
        if !self.turns.is_empty() {
            let _ = writeln!(
                s,
                "giro di pagina (tasto -> consegna allo schermo), {} giri: {}; pagina non pronta {} volte",
                self.turns.len(),
                summary(&self.turns),
                self.misses
            );
        }
        if !self.frames.is_empty() {
            let late = self.frames.iter().filter(|&&f| f > self.refresh_ms * 1.5).count();
            let _ = writeln!(
                s,
                "nastro, {} fotogrammi: intervallo {}; persi {late} ({:.2}%)",
                self.frames.len(),
                summary(&self.frames),
                late as f32 * 100.0 / self.frames.len() as f32
            );
        }
        if !self.acquire.is_empty() {
            let _ = writeln!(
                s,
                "attesa del fotogramma: {}; fallite {} {:?}",
                summary(&self.acquire),
                self.acquire_failures.len(),
                self.acquire_failures.iter().take(3).collect::<Vec<_>>()
            );
        }
        if !self.prepared.is_empty() {
            let col = |f: fn(&(f32, f32, f32)) -> f32| self.prepared.iter().map(f).collect::<Vec<_>>();
            let _ = writeln!(
                s,
                "pagine preparate: {} (su un thread) lettura {}, decodifica {}, rimpicciolimento {}",
                self.prepared.len(),
                summary(&col(|p| p.0)),
                summary(&col(|p| p.1)),
                summary(&col(|p| p.2))
            );
        }
        for n in &self.notes {
            let _ = writeln!(s, "  {n}");
        }
        s
    }
}

/// "mediana 1.2 ms, 95% sotto 3.4 ms, peggiore 5.6 ms"
fn summary(v: &[f32]) -> String {
    let mut v = v.to_vec();
    v.sort_by(f32::total_cmp);
    let at = |p: f32| v[((v.len() as f32 * p) as usize).min(v.len() - 1)];
    format!("mediana {:.2} ms, 95% sotto {:.2} ms, peggiore {:.2} ms", at(0.5), at(0.95), v[v.len() - 1])
}
