//! Le copertine della libreria.
//!
//! La prima pagina di un volume, ritagliata in 2:3 (la forma delle celle: i
//! webtoon, strisce alte dieci volte la larghezza, si prendono dall'alto; le
//! tavole orizzontali dal centro) e rimpicciolita in luce lineare come ogni
//! pagina. Si tiene in cache su disco in WebP, 320x480: al secondo avvio la
//! libreria compare subito, e mille copertine pesano una ventina di MB.
//!
//! La chiave di cache e' il percorso con la misura e la data del file: se il
//! volume cambia, la copertina si rifa'.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;

use crate::book::{Book, Content};
use crate::decode::{Page, decode};
use crate::loader::Fit;
use crate::resize::resize;

/// La misura delle copertine in cache.
pub const COVER_W: u32 = 320;
pub const COVER_H: u32 = 480;

/// Il nome del file in cache per un volume: FNV-1a a 64 bit di percorso,
/// misura e data. Stabile fra versioni del programma (l'hash della libreria
/// standard non lo e').
pub fn cache_name(path: &Path) -> Option<String> {
    // un volume remoto non ha misura ne' data: basta l'indirizzo (con il
    // numero di pagine dentro, che cambia se il volume cambia)
    let (len, modified) = if crate::remote::is_remote(path) {
        (0, 0)
    } else {
        let meta = std::fs::metadata(path).ok()?;
        (meta.len(), meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs())
    };
    let abs = crate::book::absolute(path);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in abs.to_string_lossy().bytes().chain(len.to_le_bytes()).chain(modified.to_le_bytes()) {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    Some(format!("{h:016x}.webp"))
}

/// La copertina, dalla cache o fatta adesso (e messa in cache).
pub fn cover(path: &Path, cache: &Path) -> Result<Page, String> {
    let file = cache_name(path).map(|n| cache.join(n));
    if let Some(bytes) = file.as_ref().and_then(|f| std::fs::read(f).ok())
        && let Ok(page) = decode(&bytes)
    {
        return Ok(page);
    }
    let page = make(path)?;
    if let Some(file) = file {
        // la cache e' un di piu': se non si scrive, la copertina c'e' lo stesso
        let _ = std::fs::create_dir_all(cache).and_then(|_| {
            let tmp = file.with_extension("tmp");
            std::fs::write(&tmp, encode_webp(&page, 82.0)?)?;
            std::fs::rename(&tmp, &file)
        });
    }
    Ok(page)
}

/// La copertina di un volume, 320x480, senza cache.
pub fn make(path: &Path) -> Result<Page, String> {
    // la copertina gia' pronta sul server pesa meno della prima pagina
    if let Some(page) = crate::remote::cover(path).and_then(|bytes| decode(&bytes).ok()) {
        return Ok(resize(&crop_to_cover(&page), COVER_W, COVER_H, true));
    }
    let book = Book::open(path).map_err(|e| e.to_string())?;
    let page =
        match book.content(0, Fit::Contain { width: COVER_W * 3, height: COVER_H * 3 }).map_err(|e| e.to_string())? {
            Content::Encoded(bytes) => decode(&bytes).map_err(|e| e.to_string())?,
            Content::Pixels(p) | Content::Exact(p, _) => p,
        };
    let page = crop_to_cover(&page);
    Ok(resize(&page, COVER_W, COVER_H, true))
}

/// Il pezzo 2:3 della pagina: dall'alto se e' piu' alta, dal centro se e'
/// piu' larga.
fn crop_to_cover(p: &Page) -> Page {
    let (w, h) = (p.width, p.height);
    let (cw, ch) = if h * COVER_W >= w * COVER_H {
        (w, (w * COVER_H / COVER_W).max(1))
    } else {
        ((h * COVER_W / COVER_H).max(1), h)
    };
    let x0 = (w - cw) / 2;
    let mut rgba = Vec::with_capacity((cw * ch * 4) as usize);
    for row in 0..ch {
        let start = ((row * w + x0) * 4) as usize;
        rgba.extend_from_slice(&p.rgba[start..start + (cw * 4) as usize]);
    }
    Page { width: cw, height: ch, rgba, opaque: p.opaque }
}

fn encode_webp(p: &Page, quality: f32) -> std::io::Result<Vec<u8>> {
    let mut out: *mut u8 = std::ptr::null_mut();
    // SAFETY: rgba e' largo `width * 4` per `height` righe, come dichiarato;
    // `out` lo alloca libwebp e lo si libera con WebPFree dopo averlo copiato.
    unsafe {
        let n = libwebp_sys::WebPEncodeRGBA(
            p.rgba.as_ptr(),
            p.width as i32,
            p.height as i32,
            (p.width * 4) as i32,
            quality,
            &mut out,
        );
        if n == 0 || out.is_null() {
            return Err(std::io::Error::other("WebP: codifica non riuscita"));
        }
        let bytes = std::slice::from_raw_parts(out, n).to_vec();
        libwebp_sys::WebPFree(out.cast());
        Ok(bytes)
    }
}

/// Una copertina pronta, alla misura chiesta.
pub struct Cover {
    pub path: PathBuf,
    pub size: (u32, u32),
    pub page: Result<Page, String>,
}

/// Prepara le copertine in due thread a bassa priorita': chi disegna e chi
/// prepara le pagine passano prima.
pub struct CoverLoader {
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

struct State {
    cache: PathBuf,
    queue: VecDeque<(PathBuf, (u32, u32))>,
    running: HashSet<PathBuf>,
    stop: bool,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl CoverLoader {
    pub fn new(cache: PathBuf, deliver: impl Fn(Cover) + Send + Sync + 'static) -> CoverLoader {
        let shared = Arc::new(Shared {
            state: Mutex::new(State { cache, queue: VecDeque::new(), running: HashSet::new(), stop: false }),
            wake: Condvar::new(),
        });
        let deliver = Arc::new(deliver);
        let workers = (0..2)
            .map(|n| {
                let (shared, deliver) = (shared.clone(), deliver.clone());
                std::thread::Builder::new()
                    .name(format!("copertine-{n}"))
                    .spawn(move || {
                        let _ = thread_priority::set_current_thread_priority(thread_priority::ThreadPriority::Min);
                        work(&shared, &*deliver)
                    })
                    .expect("thread delle copertine")
            })
            .collect();
        CoverLoader { shared, workers }
    }

    /// Sostituisce la coda: le copertine che servono, dalla piu' urgente.
    pub fn request(&self, wanted: Vec<(PathBuf, (u32, u32))>) {
        let mut s = self.shared.lock();
        let queue = wanted.into_iter().filter(|(p, _)| !s.running.contains(p)).collect();
        s.queue = queue;
        drop(s);
        self.shared.wake.notify_all();
    }
}

impl Drop for CoverLoader {
    fn drop(&mut self) {
        self.shared.lock().stop = true;
        self.shared.wake.notify_all();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}

fn work(shared: &Shared, deliver: &dyn Fn(Cover)) {
    loop {
        let (path, size, cache) = {
            let mut s = shared.lock();
            loop {
                if s.stop {
                    return;
                }
                if let Some((path, size)) = s.queue.pop_front() {
                    s.running.insert(path.clone());
                    break (path, size, s.cache.clone());
                }
                s = shared.wake.wait(s).unwrap_or_else(|e| e.into_inner());
            }
        };
        let page = cover(&path, &cache)
            .map(|p| if (p.width, p.height) == size { p } else { resize(&p, size.0.max(1), size.1.max(1), true) });
        shared.lock().running.remove(&path);
        deliver(Cover { path, size, page });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copertina_ritagliata_e_in_cache() {
        let dir = std::env::temp_dir().join(format!("fumetto-copertine-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let volume = dir.join("Webtoon");
        std::fs::create_dir_all(&volume).unwrap();
        // una striscia alta: la copertina ne prende l'inizio
        let (w, h) = (200u32, 2000u32);
        let rgba: Vec<u8> = (0..h)
            .flat_map(|y| (0..w).flat_map(move |_| if y < 300 { [250, 250, 250, 255] } else { [10, 10, 10, 255] }))
            .collect();
        image::save_buffer(volume.join("001.png"), &rgba, w, h, image::ColorType::Rgba8).unwrap();
        let cache = dir.join("cache");
        let c = cover(&volume, &cache).unwrap();
        assert_eq!((c.width, c.height), (COVER_W, COVER_H));
        assert!(c.rgba[..4].iter().take(3).all(|&v| v > 200), "l'inizio della striscia, chiaro");
        let cached: Vec<_> = std::fs::read_dir(&cache).unwrap().flatten().collect();
        assert_eq!(cached.len(), 1, "una copertina in cache");
        let again = cover(&volume, &cache).unwrap();
        assert_eq!((again.width, again.height), (COVER_W, COVER_H), "riletta dalla cache");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
