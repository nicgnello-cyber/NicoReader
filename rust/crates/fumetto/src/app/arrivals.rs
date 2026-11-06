//! Cio' che arriva dai thread di lavoro e dalle finestre di sistema:
//! copertine, miniature, pagine per la lente e migliorate, risposte.

use super::*;

impl App {
    /// La risposta della finestra per aprire (`None`: annullata): un volume
    /// da leggere, o una cartella da aggiungere alla libreria.
    pub(super) fn chosen(&mut self, path: Option<PathBuf>, to_library: bool) {
        self.dialog = false;
        let Some(path) = path else { return };
        if !to_library {
            return self.open(&path);
        }
        if !self.settings.library.contains(&path) {
            self.settings.library.push(path);
            self.save_settings();
        }
        self.rescan();
        self.changed();
    }

    /// Si', spostalo nel cestino.
    pub(super) fn trash_confirmed(&mut self, path: &Path) {
        self.dialog = false;
        match system::trash(path) {
            Ok(()) => {
                self.ui.toast(t("Spostato nel cestino", "Moved to the trash"), Instant::now());
                self.rescan();
            }
            Err(e) => self
                .notify(format!("{}\n\n{e}", t("Impossibile spostarlo nel cestino.", "Can't move it to the trash."))),
        }
    }

    pub(super) fn cover_arrived(&mut self, c: Cover) {
        match (&self.gfx, c.page) {
            (Some(gfx), Ok(page)) => {
                if let Ok(image) = gfx.renderer.upload(&page) {
                    // rifatta per una cella di un'altra misura: era gia' a
                    // schermo, non ricompare da capo
                    let since = self.covers.get(&c.path).map_or_else(Instant::now, |old| old.since);
                    self.covers.insert(c.path, ShelfCover { image, size: c.size, since });
                    self.request_redraw();
                }
            }
            // senza scheda video non si tengono: si richiedono quando c'e'
            (None, Ok(_)) => self.cover_wanted.clear(),
            (_, Err(e)) => {
                eprintln!("copertina di {}: {e}", c.path.display());
                self.covers_failed.insert(c.path);
                self.request_redraw();
            }
        }
    }

    pub(super) fn thumb_arrived(&mut self, loaded: Loaded) {
        if loaded.generation != self.thumb_generation {
            return;
        }
        match (&self.gfx, loaded.page) {
            (Some(gfx), Ok(page)) => {
                if let Ok(image) = gfx.renderer.upload(&page) {
                    self.thumb_images.insert(loaded.index, (image, loaded.target));
                    self.request_redraw();
                }
            }
            // senza scheda video non si tengono: si richiedono quando c'e'
            (None, Ok(_)) => self.thumb_wanted = (None, Vec::new()),
            (_, Err(e)) => eprintln!("miniatura {}: {e}", loaded.index + 1),
        }
    }

    /// Una pagina ai suoi pixel, per la lente. Pesano: se ne tengono poche,
    /// le piu' vicine alla pagina che si legge.
    pub(super) fn lens_page_arrived(&mut self, loaded: Loaded) {
        if loaded.generation != self.lens_generation {
            return;
        }
        let image = match (&self.gfx, loaded.page) {
            (Some(gfx), Ok(page)) => gfx.renderer.upload(&page),
            (None, Ok(_)) => {
                self.lens_wanted = (None, Vec::new());
                return;
            }
            (_, Err(e)) => Err(e.to_string()),
        };
        match image {
            Ok(image) => {
                self.lens_images.insert(loaded.index, (image, loaded.target));
                let here = self.reader.as_ref().map_or(0, |r| r.here());
                while self.lens_images.len() > 4 {
                    let far = *self.lens_images.keys().max_by_key(|i| i.abs_diff(here)).expect("non vuota");
                    self.lens_images.remove(&far);
                }
                self.request_redraw();
            }
            Err(e) => eprintln!("lente, pagina {}: {e}", loaded.index + 1),
        }
    }

    /// Una pagina migliorata dall'AI, se vale ancora per cio' che si guarda;
    /// poi la prossima. Senza una scheda video adatta lo si dice, e si spegne.
    pub(super) fn upscaled(&mut self, u: Upscaled) {
        if let Some(e) = u.error {
            if self.settings.upscale {
                self.toggle_upscale();
            }
            let head = t("Il miglioramento con l'AI non si può usare qui.", "AI enhancement can't be used here.");
            return self.notify(format!("{head}\n\n{e}"));
        }
        let current = self.reader.as_ref().map(|r| r.target());
        if u.generation != self.generation || !self.settings.upscale {
            return;
        }
        match (u.page, &self.gfx) {
            (Some(page), Some(gfx)) if current == Some(u.target) => {
                if let Ok(image) = gfx.renderer.upload(&page) {
                    self.shown.insert(u.index, Shown { image, target: u.target, upscaled: true, stale: false });
                    self.request_redraw();
                }
            }
            (None, _) => {
                self.upscale_failed.insert((u.index, u.target));
            }
            _ => {}
        }
        self.request_upscale();
    }

    /// Dove salvare la pagina (`None`: annullato): si salva in sottofondo.
    pub(super) fn save_to(&mut self, path: Option<PathBuf>) {
        self.dialog = false;
        let (Some(path), Some((book, index))) = (path, self.to_save.take()) else { return };
        let proxy = self.proxy.clone();
        std::thread::Builder::new()
            .name("salva-pagina".into())
            .spawn(move || {
                let done = save_page(&book, index, &path).map(|_| t("Pagina salvata", "Page saved"));
                let _ = proxy.send_event(UserEvent::Done(done));
            })
            .expect("thread del salvataggio");
    }

    /// Un altro NicoReader ha passato un volume (o niente) a questo: la
    /// finestra davanti, anche se era ridotta a icona.
    pub(super) fn forwarded(&mut self, path: Option<PathBuf>) {
        if let Some(w) = &self.window {
            w.set_minimized(false);
            w.focus_window();
        }
        if let Some(path) = path {
            self.open(&path);
        }
        self.request_redraw();
    }

    /// Com'e' andato un salvataggio o una copia.
    pub(super) fn done(&mut self, result: Result<&'static str, String>) {
        match result {
            Ok(text) => {
                self.ui.toast(text, Instant::now());
                self.request_redraw();
            }
            Err(e) => self.notify(e),
        }
    }
}
