//! Lettura anticipata: le pagine si decodificano e si portano alla misura dello
//! schermo su piu' thread, nell'ordine in cui serviranno. L'ordine cambia a ogni
//! giro di pagina e le richieste non piu' utili spariscono dalla coda senza
//! essere eseguite.

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Instant;

use crate::adjust;
use crate::book::{Book, Content};
use crate::decode::{Page, decode};
use crate::resize::resize;

/// Come una pagina deve stare sullo schermo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Fit {
    /// Tutta dentro il riquadro: la lettura a pagina singola.
    Contain { width: u32, height: u32 },
    /// Larghezza fissa, altezza di conseguenza: il nastro dei webtoon.
    Width(u32),
    /// Doppia pagina: una pagina verticale sta in meta' riquadro, una
    /// orizzontale (una tavola doppia gia' unita) in tutto il riquadro.
    Spread { width: u32, height: u32 },
}

impl Fit {
    /// La misura a schermo di una pagina `w` x `h`, al pixel intero.
    pub fn size(self, w: u32, h: u32) -> (u32, u32) {
        let contain = |width: u32, height: u32| (width as f64 / w as f64).min(height as f64 / h as f64);
        let k = match self {
            Fit::Contain { width, height } => contain(width, height),
            Fit::Width(width) => width as f64 / w as f64,
            Fit::Spread { width, height } if w > h => contain(width, height),
            Fit::Spread { width, height } => contain(width / 2, height),
        };
        (((w as f64 * k).round() as u32).max(1), ((h as f64 * k).round() as u32).max(1))
    }
}

/// A che misura preparare le pagine, con quale filtro e quali ritocchi.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Target {
    pub fit: Fit,
    /// Luce lineare (il modo giusto) o valori sRGB (solo per confronto).
    pub linear: bool,
    /// Senza i margini uniformi della scansione.
    pub trim: bool,
    /// Girata di tanti gradi in senso orario (0, 90, 180, 270).
    pub rotation: u16,
}

impl Target {
    /// Pagina intera, luce lineare, nessun ritocco.
    pub fn plain(fit: Fit) -> Target {
        Target { fit, linear: true, trim: false, rotation: 0 }
    }
}

/// Una pagina decodificata e ritoccata, ai suoi pixel: manca solo la misura
/// dello schermo.
pub struct Decoded {
    /// La misura di riferimento (rifilata e girata): per impaginare.
    pub native: (u32, u32),
    pub page: Page,
    /// Gia' alla misura voluta (una pagina PDF disegnata apposta).
    pub exact: bool,
}

/// Legge, decodifica, rifila e gira la pagina `index` come vuole `target`.
pub fn decode_page(book: &Book, index: usize, target: Target) -> Result<Decoded, String> {
    decode_content(read_page(book, index, target)?, target)
}

/// La pagina come la da' il volume (per i PDF, gia' pronta per `target`).
fn read_page(book: &Book, index: usize, target: Target) -> Result<Content, String> {
    book.content_turned(index, target.fit, target.rotation % 180 == 90).map_err(|e| e.to_string())
}

fn decode_content(content: Content, target: Target) -> Result<Decoded, String> {
    let quarter = target.rotation % 180 == 90;
    let (native, page, exact) = match content {
        Content::Encoded(bytes) => {
            let p = decode(&bytes).map_err(|e| e.to_string())?;
            ((p.width, p.height), p, false)
        }
        Content::Pixels(p) => ((p.width, p.height), p, false),
        // pagina PDF vettoriale: disegnata apposta, e la sua misura di
        // riferimento non e' quella dei pixel appena disegnati
        Content::Exact(p, native) => (native, p, true),
    };
    // il rifilo non tocca le pagine disegnate: non vengono da uno scanner
    let page = if target.trim && !exact { adjust::trim(page) } else { page };
    let native = if exact { native } else { (page.width, page.height) };
    let page = adjust::rotate(page, target.rotation);
    let native = if quarter { (native.1, native.0) } else { native };
    Ok(Decoded { native, page, exact })
}

/// La pagina alla misura dello schermo: solo rimpicciolire, ingrandire lo fa
/// la scheda video, senza perdere nulla.
pub fn to_screen(d: Decoded, target: Target) -> Page {
    match target.fit.size(d.page.width, d.page.height) {
        (w, h) if !d.exact && w < d.page.width => resize(&d.page, w, h, target.linear),
        _ => d.page,
    }
}

/// Una pagina pronta (o fallita), consegnata dal thread che l'ha preparata.
pub struct Loaded {
    /// A quale libro si riferisce: dopo un cambio di volume i ritardatari si scartano.
    pub generation: u64,
    pub index: usize,
    /// Per quale misura e' stata preparata.
    pub target: Target,
    /// Misura originale della pagina: serve all'impaginazione del nastro.
    pub native: (u32, u32),
    /// Rimpicciolita alla misura del bersaglio, oppure originale se va
    /// ingrandita (lo fa la scheda video, a ogni fotogramma).
    pub page: Result<Page, String>,
    pub read_ms: f32,
    pub decode_ms: f32,
    pub resize_ms: f32,
}

pub struct Loader {
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

#[derive(Default)]
struct State {
    book: Option<Arc<Book>>,
    generation: u64,
    target: Option<Target>,
    queue: VecDeque<usize>,
    running: HashSet<(u64, usize, Option<Target>)>,
    stop: bool,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        // un thread morto a meta' non deve bloccare gli altri: lo stato resta coerente
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Loader {
    /// `deliver` viene chiamata dai thread di lavoro: deve solo passare il
    /// risultato a chi disegna (per esempio svegliando il ciclo degli eventi).
    pub fn new(threads: usize, deliver: impl Fn(Loaded) + Send + Sync + 'static) -> Loader {
        let shared = Arc::new(Shared { state: Mutex::default(), wake: Condvar::new() });
        let deliver = Arc::new(deliver);
        let workers = (0..threads.max(1))
            .map(|n| {
                let (shared, deliver) = (shared.clone(), deliver.clone());
                std::thread::Builder::new()
                    .name(format!("pagine-{n}"))
                    .spawn(move || {
                        // chi disegna deve passare sempre per primo: con tutti i
                        // core occupati a decodificare, a pari priorita' il
                        // nastro scendeva a 22 fotogrammi al secondo
                        let _ = thread_priority::set_current_thread_priority(thread_priority::ThreadPriority::Min);
                        work(&shared, &*deliver)
                    })
                    .expect("thread di decodifica")
            })
            .collect();
        Loader { shared, workers }
    }

    /// Quanti thread usare: tutti i core meno uno, che resta a chi disegna.
    pub fn default_threads() -> usize {
        std::thread::available_parallelism().map_or(4, |n| n.get().saturating_sub(1)).clamp(2, 8)
    }

    /// Cambia volume, o lo chiude (`None`: il file si libera appena finisce la
    /// pagina in lavorazione). Restituisce la generazione con cui arriveranno
    /// le sue pagine.
    pub fn set_book(&self, book: Option<Arc<Book>>) -> u64 {
        let mut s = self.shared.lock();
        s.generation += 1;
        s.book = book;
        s.queue.clear();
        s.generation
    }

    /// Cambia la misura delle pagine (finestra ridimensionata, altro modo di
    /// lettura). Le pagine gia' in lavorazione arrivano comunque, con la loro
    /// misura: meglio una pagina da riscalare che uno schermo nero.
    pub fn set_target(&self, target: Target) {
        self.shared.lock().target = Some(target);
    }

    /// Sostituisce la coda: `order` e' la lista completa di cio' che serve, dal
    /// piu' urgente. Le pagine gia' in lavorazione non si ripetono.
    pub fn request(&self, order: &[usize]) {
        let mut s = self.shared.lock();
        let key = (s.generation, s.target);
        let queue: VecDeque<usize> =
            order.iter().copied().filter(|&i| !s.running.contains(&(key.0, i, key.1))).collect();
        s.queue = queue;
        drop(s);
        self.shared.wake.notify_all();
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        self.shared.lock().stop = true;
        self.shared.wake.notify_all();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}

fn work(shared: &Shared, deliver: &dyn Fn(Loaded)) {
    loop {
        let (book, generation, target, index) = {
            let mut s = shared.lock();
            loop {
                if s.stop {
                    return;
                }
                if let (Some(book), Some(target)) = (s.book.clone(), s.target)
                    && let Some(index) = s.queue.pop_front()
                {
                    let generation = s.generation;
                    s.running.insert((generation, index, Some(target)));
                    break (book, generation, target, index);
                }
                s = shared.wake.wait(s).unwrap_or_else(|e| e.into_inner());
            }
        };
        let t = Instant::now();
        let content = read_page(&book, index, target);
        let read_ms = ms(t);
        let t = Instant::now();
        let decoded = content.and_then(|c| decode_content(c, target));
        let decode_ms = ms(t);
        let native = decoded.as_ref().map_or((0, 0), |d| d.native);
        let t = Instant::now();
        let page = decoded.map(|d| to_screen(d, target));
        let resize_ms = ms(t);
        shared.lock().running.remove(&(generation, index, Some(target)));
        deliver(Loaded { generation, index, target, native, page, read_ms, decode_ms, resize_ms });
    }
}

fn ms(t: Instant) -> f32 {
    t.elapsed().as_secs_f32() * 1000.0
}

/// In che ordine preparare le pagine.
///
/// Prima quelle a schermo (`first..=last`), poi `ahead` pagine nella direzione
/// di lettura, poi `behind` all'indietro per chi torna sui propri passi.
pub fn prefetch_order(first: usize, last: usize, len: usize, forward: bool, ahead: usize, behind: usize) -> Vec<usize> {
    if len == 0 {
        return Vec::new();
    }
    let last = last.min(len - 1);
    let mut order: Vec<usize> = (first..=last).collect();
    let after = (last + 1..len).take(if forward { ahead } else { behind });
    let before = (0..first).rev().take(if forward { behind } else { ahead });
    if forward {
        order.extend(after);
        order.extend(before);
    } else {
        order.extend(before);
        order.extend(after);
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn misure_a_schermo() {
        let contain = Fit::Contain { width: 1920, height: 1080 };
        assert_eq!(contain.size(1600, 2400), (720, 1080)); // pagina verticale: conta l'altezza
        assert_eq!(contain.size(4000, 1000), (1920, 480)); // doppia pagina larga: la larghezza
        assert_eq!(Fit::Width(1152).size(800, 20_000), (1152, 28_800));
        assert_eq!(Fit::Width(10).size(5000, 1), (10, 1)); // mai zero pixel
        let spread = Fit::Spread { width: 1920, height: 1080 };
        assert_eq!(spread.size(1600, 2400), (720, 1080)); // verticale: meta' larghezza
        assert_eq!(spread.size(3200, 2400), (1440, 1080)); // tavola doppia: tutto
        assert_eq!(spread.size(1000, 1000), (960, 960)); // quadrata: meta', limita la larghezza
    }

    #[test]
    fn avanti() {
        assert_eq!(prefetch_order(5, 5, 100, true, 3, 1), [5, 6, 7, 8, 4]);
    }

    #[test]
    fn indietro() {
        assert_eq!(prefetch_order(5, 5, 100, false, 3, 1), [5, 4, 3, 2, 6]);
    }

    #[test]
    fn ai_bordi() {
        assert_eq!(prefetch_order(0, 1, 3, true, 5, 5), [0, 1, 2]);
        assert_eq!(prefetch_order(2, 9, 3, true, 5, 5), [2, 1, 0]);
        assert!(prefetch_order(0, 0, 0, true, 5, 5).is_empty());
    }
}
