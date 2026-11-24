//! Le pagine "di lato" alla lettura: le miniature e quelle ai loro pixel
//! veri per la lente. Ognuna ha il suo preparatore, le sue immagini sulla
//! scheda video e l'ultima richiesta (per non ripeterla a ogni fotogramma).

use super::*;

pub(super) struct SidePages {
    /// Per i messaggi: "miniatura", "lente, pagina".
    what: &'static str,
    loader: Loader,
    /// Del volume aperto: le pagine di un altro si scartano.
    generation: u64,
    pub(super) images: HashMap<usize, (GpuImage, Target)>,
    wanted: (Option<Target>, Vec<usize>),
}

impl SidePages {
    pub(super) fn new(what: &'static str, loader: Loader) -> SidePages {
        SidePages { what, loader, generation: 0, images: HashMap::new(), wanted: (None, Vec::new()) }
    }

    /// Si riparte da capo, per questo volume (o per nessuno).
    pub(super) fn set_book(&mut self, book: Option<Arc<Book>>) {
        self.images.clear();
        self.generation = self.loader.set_book(book);
        self.wanted = (None, Vec::new());
    }

    /// Si comincia con il volume, se non si e' gia' cominciato.
    pub(super) fn start(&mut self, book: &Option<Arc<Book>>) {
        if self.images.is_empty() {
            self.set_book(book.clone());
        }
    }

    /// Chiede le pagine `wanted` alla misura `target`: solo se la richiesta
    /// e' cambiata.
    pub(super) fn request(&mut self, target: Target, wanted: Vec<usize>) {
        if (Some(target), &wanted) != (self.wanted.0, &self.wanted.1) {
            self.loader.set_target(target);
            self.loader.request(&wanted);
            self.wanted = (Some(target), wanted);
        }
    }

    /// Le pagine fra `pages` che non ci sono, o non a questa misura.
    pub(super) fn missing(&self, pages: impl Iterator<Item = usize>, target: Target) -> Vec<usize> {
        pages.filter(|p| self.images.get(p).is_none_or(|i| i.1 != target)).collect()
    }

    /// Una pagina arrivata: `true` se c'e' da ridisegnare. Senza scheda
    /// video non si tiene: la si richiede quando c'e'.
    pub(super) fn arrived(
        &mut self, Ready { item: loaded, image }: Ready<Loaded>, renderer: Option<&Renderer>,
    ) -> bool {
        if loaded.generation != self.generation {
            return false;
        }
        let page = match &loaded.page {
            Ok(page) => page,
            Err(e) => {
                eprintln!("{} {}: {e}", self.what, loaded.index + 1);
                return false;
            }
        };
        match on_gpu(renderer, image, page) {
            Some(Ok(image)) => {
                self.images.insert(loaded.index, (image, loaded.target));
                true
            }
            Some(Err(e)) => {
                eprintln!("{} {}: {e}", self.what, loaded.index + 1);
                false
            }
            None => {
                self.wanted = (None, Vec::new());
                false
            }
        }
    }

    /// Ne tiene al massimo `max`, le piu' vicine a `here`.
    pub(super) fn keep_near(&mut self, here: usize, max: usize) {
        while self.images.len() > max {
            let far = *self.images.keys().max_by_key(|i| i.abs_diff(here)).expect("non vuota");
            self.images.remove(&far);
        }
    }
}
