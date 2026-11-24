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

    pub(super) fn cover_arrived(&mut self, ready: Ready<Cover>) {
        let renderer = self.gfx.as_ref().map(|g| &*g.renderer);
        if self.covers.arrived(ready, renderer) {
            self.request_redraw();
        }
    }

    pub(super) fn thumb_arrived(&mut self, ready: Ready<Loaded>) {
        let renderer = self.gfx.as_ref().map(|g| &*g.renderer);
        if self.thumbs.arrived(ready, renderer) {
            self.request_redraw();
        }
    }

    /// Una pagina ai suoi pixel, per la lente. Pesano: se ne tengono poche,
    /// le piu' vicine alla pagina che si legge.
    pub(super) fn lens_page_arrived(&mut self, ready: Ready<Loaded>) {
        let renderer = self.gfx.as_ref().map(|g| &*g.renderer);
        if self.lens_pages.arrived(ready, renderer) {
            self.lens_pages.keep_near(self.reader.as_ref().map_or(0, |r| r.here()), 4);
            self.request_redraw();
        }
    }

    /// Una pagina migliorata dall'AI, se vale ancora per cio' che si guarda;
    /// poi la prossima. Senza una scheda video adatta lo si dice, e si spegne.
    pub(super) fn upscaled(&mut self, Ready { item: u, image }: Ready<Upscaled>) {
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
        match &u.page {
            Some(page) if current == Some(u.target) => {
                if let Some(Ok(image)) = on_gpu(self.renderer(), image, page) {
                    self.shown.insert(u.index, Shown { image, target: u.target, upscaled: true, stale: false });
                    self.request_redraw();
                }
            }
            None => {
                self.upscale_failed.insert((u.index, u.target));
            }
            Some(_) => {}
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
