//! L'ingrandimento AI: le scansioni a bassa risoluzione, mostrate piu' grandi
//! dei loro pixel, rifatte con Real-ESRGAN invece che solo interpolate.
//!
//! Qui la coda delle pagine e il loro giro; la rete la fa girare un "motore"
//! che passa chi crea l'Upscaler (nel programma: fumetto_render::esrgan, sulla
//! scheda video). Il nucleo resta senza grafica.
//!
//! Il modello e' realesr-animevideov3, non x4plus-anime: misurato su una tavola
//! degradata contro l'originale, 25,1 dB in 1,4 s contro 17,1 dB in 4,5 s; il
//! modello "grande" ingrandisce 4x quando ne serve 2x e altera lo spessore dei
//! tratti.
//!
//! Il lettore mostra subito la pagina normale; quella migliorata arriva dopo,
//! dall'unico thread di questo modulo, e prende il suo posto senza avvisi.

use std::collections::VecDeque;
use std::process::Command;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;

use crate::book::Book;
use crate::decode::Page;
use crate::loader::{Decoded, Target, decode_page, to_screen};

/// Si migliora solo una pagina mostrata almeno 1,2 volte i suoi pixel: sotto,
/// l'interpolazione della scheda video non si distingue.
pub const MIN_GAIN: f32 = 1.2;

/// Quante pagine migliorate (ai loro pixel, prima della misura dello schermo)
/// si tengono: cambiando zoom si riusano invece di rifarle. Una tavola 2x pesa
/// 25-40 MB.
const KEEP_DONE: usize = 3;

/// Vale la pena di migliorare una pagina `native` mostrata come vuole `target`?
pub fn worth(native: (u32, u32), target: Target) -> bool {
    let (w, _) = target.fit.size(native.0, native.1);
    native.0 > 0 && w as f32 >= native.0 as f32 * MIN_GAIN
}

/// Su Windows un programma lanciato da qui (curl, per le versioni nuove)
/// non apre la finestra nera della console.
pub(crate) fn hide_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

/// Il motore: la pagina ingrandita 2 o 4 volte; `Ok(None)` se il terzo
/// argomento dice di smettere, `Err` se l'ingrandimento non si puo' fare
/// (nessuna scheda video adatta): lo si dice una volta, e basta.
pub type Engine = Box<dyn FnMut(&Page, u32, &dyn Fn() -> bool) -> Result<Option<Page>, String> + Send>;

/// Una pagina migliorata, alla misura dello schermo; `None` se non si e'
/// potuto (una pagina PDF disegnata, il motore che non va): non la si
/// richiede piu'. `error`: perche' il motore non va, la prima volta.
pub struct Upscaled {
    pub generation: u64,
    pub index: usize,
    pub target: Target,
    pub page: Option<Page>,
    pub error: Option<String>,
}

/// Una pagina da migliorare.
#[derive(Clone)]
pub struct Job {
    pub book: Arc<Book>,
    pub generation: u64,
    pub index: usize,
    pub target: Target,
}

pub struct Upscaler {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

struct State {
    queue: VecDeque<Job>,
    /// La pagina in lavorazione, e se non serve piu' (la coda nuova non la
    /// vuole): allora la si lascia a meta'.
    current: Option<Key>,
    drop_current: bool,
    stop: bool,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Upscaler {
    /// `engine` fa la rete (dal thread del modulo); `deliver` riceve le
    /// pagine migliorate, dallo stesso thread.
    pub fn new(engine: Engine, deliver: impl Fn(Upscaled) + Send + Sync + 'static) -> Upscaler {
        let shared = Arc::new(Shared {
            state: Mutex::new(State { queue: VecDeque::new(), current: None, drop_current: false, stop: false }),
            wake: Condvar::new(),
        });
        let worker = {
            let shared = shared.clone();
            std::thread::Builder::new()
                .name("ingrandimento".into())
                .spawn(move || {
                    let _ = thread_priority::set_current_thread_priority(thread_priority::ThreadPriority::Min);
                    work(&shared, engine, &deliver)
                })
                .expect("thread dell'ingrandimento")
        };
        Upscaler { shared, worker: Some(worker) }
    }

    /// Sostituisce la coda: le pagine a schermo, dalla piu' urgente. Quella in
    /// lavorazione, se la coda nuova non la vuole, si lascia a meta'.
    pub fn request(&self, jobs: Vec<Job>) {
        let mut s = self.shared.lock();
        if let Some(current) = s.current {
            s.drop_current = !jobs.iter().any(|j| key(j) == current);
        }
        s.queue = jobs.into();
        drop(s);
        self.shared.wake.notify_all();
    }
}

impl Drop for Upscaler {
    fn drop(&mut self) {
        self.shared.lock().stop = true;
        self.shared.wake.notify_all();
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

/// La chiave di una pagina migliorata: stesso volume, pagina e ritocchi.
type Key = (u64, usize, bool, u16);

/// Una pagina migliorata ai suoi pixel, con la misura della pagina originale.
type Big = ((u32, u32), Page);

fn key(job: &Job) -> Key {
    (job.generation, job.index, job.target.trim, job.target.rotation)
}

fn work(shared: &Shared, mut engine: Engine, deliver: &dyn Fn(Upscaled)) {
    let mut done: VecDeque<(Key, (u32, u32), Arc<Page>)> = VecDeque::new();
    // il motore che non va lo si dice una volta sola
    let mut broken = false;
    loop {
        let job = {
            let mut s = shared.lock();
            loop {
                if s.stop {
                    return;
                }
                if let Some(job) = s.queue.pop_front() {
                    s.current = Some(key(&job));
                    s.drop_current = false;
                    break job;
                }
                s = shared.wake.wait(s).unwrap_or_else(|e| e.into_inner());
            }
        };
        let k = key(&job);
        let mut error = None;
        let big = match done.iter().find(|d| d.0 == k) {
            Some((_, native, page)) => Some((*native, page.clone())),
            None => match upscale(shared, &mut engine, &job) {
                Ok(Some((native, page))) => {
                    let page = Arc::new(page);
                    done.push_back((k, native, page.clone()));
                    if done.len() > KEEP_DONE {
                        done.pop_front();
                    }
                    Some((native, page))
                }
                Ok(None) => None,
                Err(e) => {
                    if !std::mem::replace(&mut broken, true) {
                        error = Some(e);
                    }
                    None
                }
            },
        };
        let dropped = {
            let mut s = shared.lock();
            s.current = None;
            std::mem::take(&mut s.drop_current)
        };
        // lasciata a meta': non e' un fallimento, la si potra' richiedere
        if dropped && big.is_none() && error.is_none() {
            continue;
        }
        let page = big.map(|(native, big)| {
            let copy = Page { width: big.width, height: big.height, rgba: big.rgba.clone(), opaque: big.opaque };
            to_screen(Decoded { native, page: copy, exact: false }, job.target)
        });
        deliver(Upscaled { generation: job.generation, index: job.index, target: job.target, page, error });
    }
}

/// La pagina rifatta dalla rete, ai suoi pixel (2x, o 4x per le scansioni
/// minuscole); con la misura di riferimento della pagina originale.
fn upscale(shared: &Shared, engine: &mut Engine, job: &Job) -> Result<Option<Big>, String> {
    let Ok(d) = decode_page(&job.book, job.index, job.target) else { return Ok(None) };
    if d.exact {
        return Ok(None); // pagina PDF disegnata: ha gia' tutti i pixel che servono
    }
    let scale = if d.page.width.max(d.page.height) < 700 { 4 } else { 2 };
    let go_on = || {
        let s = shared.lock();
        !s.stop && !s.drop_current
    };
    Ok(engine(&d.page, scale, &go_on)?.map(|page| (d.native, page)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::Fit;

    #[test]
    fn si_migliora_solo_cio_che_si_ingrandisce() {
        let page = Target::plain(Fit::Contain { width: 1920, height: 1080 });
        assert!(worth((500, 700), page), "700 di altezza mostrati a 1080: 1,54x");
        assert!(!worth((1600, 2400), page), "si rimpicciolisce");
        assert!(!worth((800, 1000), page), "1,08x: non si vede la differenza");
        assert!(worth((720, 12_000), Target::plain(Fit::Width(1080))), "striscia da 720 larga 1080");
    }
}
