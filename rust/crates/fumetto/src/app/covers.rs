//! Le copertine della libreria e della schermata iniziale, alla misura delle
//! celle: chi le prepara, quelle pronte (sulla scheda video o, prima che
//! arrivi, in RAM), quelle che non si riescono a fare, l'ultima richiesta.

use fumetto_core::covers::CoverLoader;

use super::*;

/// Quanto ci mette una copertina a comparire, in secondi.
const FADE: f32 = 0.22;

/// Quante se ne tengono: a 170x255 pesano 170 KB l'una.
const KEPT: usize = 400;

pub(super) struct Covers {
    loader: CoverLoader,
    images: HashMap<PathBuf, ShelfCover>,
    /// Quelle che non si riesce a fare: non si richiedono di nuovo.
    failed: HashSet<PathBuf>,
    /// L'ultima richiesta al preparatore, per non ripeterla a ogni fotogramma.
    wanted: Vec<(PathBuf, (u32, u32))>,
}

/// Una copertina pronta.
struct ShelfCover {
    image: CoverImage,
    /// La misura della cella per cui e' fatta.
    size: (u32, u32),
    /// Da quando c'e': compare sfumando.
    since: Instant,
}

/// Dove sta una copertina: sulla scheda video, o (prima che arrivi) in RAM,
/// per disegnarla dal processore.
pub(super) enum CoverImage {
    Gpu(GpuImage),
    Cpu(Page),
}

/// Le copertine da disegnare, e se qualcuna sta ancora comparendo.
pub(super) struct ShownCovers {
    pub(super) extras: Vec<(Extra, f32, f32, f32, f32)>,
    pub(super) fading: bool,
}

impl ShelfCover {
    /// Quanto si vede, mentre compare.
    fn shown(&self, now: Instant) -> f32 {
        let a = (now.saturating_duration_since(self.since).as_secs_f32() / FADE).min(1.0);
        a * a * (3.0 - 2.0 * a)
    }
}

impl Covers {
    pub(super) fn new(loader: CoverLoader) -> Covers {
        Covers { loader, images: HashMap::new(), failed: HashSet::new(), wanted: Vec::new() }
    }

    pub(super) fn image(&self, path: &Path) -> Option<&CoverImage> {
        self.images.get(path).map(|c| &c.image)
    }

    /// Quanto si vede la copertina di un volume: 0 se non c'e' ancora.
    pub(super) fn shown(&self, path: &Path, now: Instant) -> f32 {
        self.images.get(path).map_or(0.0, |c| c.shown(now))
    }

    /// Chiede quelle che mancano (o sono di un'altra misura) e dice come
    /// disegnare quelle a schermo; `opacity`: quanto si vede una copertina
    /// mentre compare. Se sono troppe, si lasciano andare quelle lontane.
    pub(super) fn show(&mut self, slots: &[CoverSlot], now: Instant, opacity: impl Fn(f32) -> f32) -> ShownCovers {
        let px = |v: f32| v.round().max(1.0) as u32;
        let wanted: Vec<(PathBuf, (u32, u32))> = slots
            .iter()
            .map(|s| (s.path.clone(), (px(s.w), px(s.h))))
            .filter(|(p, size)| !self.failed.contains(p) && self.images.get(p).is_none_or(|c| c.size != *size))
            .collect();
        if wanted != self.wanted {
            self.loader.request(wanted.clone());
            self.wanted = wanted;
        }
        if self.images.len() > KEPT {
            let keep: HashSet<&PathBuf> = slots.iter().map(|s| &s.path).collect();
            self.images.retain(|p, _| keep.contains(p));
        }
        let mut fading = false;
        let extras = slots
            .iter()
            .filter(|s| s.on_screen)
            .filter_map(|s| {
                let a = self.images.get(&s.path)?.shown(now);
                fading |= a < 1.0;
                let cover = Extra::Cover { path: s.path.clone(), radius: s.radius, opacity: opacity(a) };
                Some((cover, s.x, s.y, s.w, s.h))
            })
            .collect();
        ShownCovers { extras, fading }
    }

    /// Una copertina arrivata: `true` se c'e' da ridisegnare.
    pub(super) fn arrived(&mut self, Ready { item: c, image }: Ready<Cover>, renderer: Option<&Renderer>) -> bool {
        let page = match c.page {
            Ok(page) => page,
            Err(e) => {
                eprintln!("copertina di {}: {e}", c.path.display());
                self.failed.insert(c.path);
                return true;
            }
        };
        let image = match on_gpu(renderer, image, &page) {
            Some(Ok(image)) => CoverImage::Gpu(image),
            Some(Err(e)) => {
                eprintln!("copertina di {}: {e}", c.path.display());
                return false;
            }
            // senza scheda video, dal processore finche' non arriva
            None => CoverImage::Cpu(page),
        };
        // rifatta per una cella di un'altra misura: era gia' a schermo, non
        // ricompare da capo
        let since = self.images.get(&c.path).map_or_else(Instant::now, |old| old.since);
        self.images.insert(c.path, ShelfCover { image, size: c.size, since });
        true
    }

    /// La scheda video e' arrivata: le copertine in RAM salgono; una che non
    /// ci riesce si richiede.
    pub(super) fn upload_all(&mut self, renderer: &Renderer) {
        self.images.retain(|_, cover| match &cover.image {
            CoverImage::Cpu(page) => match renderer.upload(page) {
                Ok(image) => {
                    cover.image = CoverImage::Gpu(image);
                    true
                }
                Err(_) => false,
            },
            CoverImage::Gpu(_) => true,
        });
        self.wanted.clear();
    }
}
