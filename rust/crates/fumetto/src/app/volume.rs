//! Aprire e chiudere i volumi, chiedere cosa aprire, e ricordare dove si
//! e' arrivati.

use super::*;

impl App {
    /// macOS: apre il primo dei volumi arrivati dal Finder (la finestra e'
    /// una), al posto di quello da riprendere e della richiesta di cosa
    /// aprire. Arrivati prima della finestra, li prende `resumed`.
    #[cfg(target_os = "macos")]
    pub(super) fn open_from_finder(&mut self) {
        if self.window.is_none() {
            return;
        }
        let Some(path) = crate::finder::take().into_iter().next() else { return };
        self.resume_later = None;
        self.ask_open = false;
        self.open(&path);
    }

    /// Chiude le miniature e dimentica tutto cio' che riguarda il volume:
    /// miniature, pagine migliorate, presentazione.
    fn forget_volume(&mut self) {
        self.thumbs_open = false;
        self.thumb_images.clear();
        self.thumb_generation = self.thumb_loader.set_book(None);
        self.thumb_wanted = (None, Vec::new());
        self.upscale_wanted.clear();
        self.upscale_failed.clear();
        self.upscaler.request(Vec::new());
        self.slideshow = None;
        self.lens_images.clear();
        self.lens_generation = self.lens_loader.set_book(None);
        self.lens_wanted = (None, Vec::new());
    }

    /// Gli ultimi letti, dal piu' recente: quelli che ci sono ancora, e non
    /// quello aperto.
    pub(super) fn refresh_recent(&mut self) {
        let open = self.book.as_ref().map(|b| std::path::absolute(&b.path).unwrap_or_else(|_| b.path.clone()));
        self.recent = self
            .progress
            .recent()
            .into_iter()
            .filter(|(path, _)| Some(path) != open.as_ref() && path.exists())
            .take(6)
            .map(|(path, saved)| Recent { title: fumetto_core::title_of(&path), place: place(saved), path })
            .collect();
    }

    /// Apre un volume; se non si puo', lo dice e resta quello di prima.
    pub fn open(&mut self, path: &Path) {
        match Book::open(path) {
            Ok(book) => self.set_book(book),
            Err(e) => {
                // una cartella senza immagini ma con dei volumi (una serie in
                // CBZ, capitoli in sottocartelle): si apre il primo
                if matches!(e, fumetto_core::Error::NoImages)
                    && let Some(first) = first_volume(path)
                {
                    return self.open(&first);
                }
                self.notify(dialog::cant_open(path, &e));
            }
        }
    }

    fn set_book(&mut self, book: Book) {
        self.remember();
        let book = Arc::new(book);
        let mut reader = Reader::new(book.len(), book.start_at);
        // aperto su un'immagine precisa: si parte da quella, non dal segnalibro
        if book.start_at == 0
            && let Some(saved) = self.progress.get(&book.path)
        {
            reader.restore(saved);
        }
        if let Some(r) = &self.reader {
            // stessi modi di lettura di chi c'era prima, se il volume e' nuovo
            reader.linear = r.linear;
        }
        reader.trim = self.settings.trim;
        // un volume mai letto: se le sue pagine sono strisce, e' un webtoon e
        // si legge a nastro. Chi ha gia' scelto come leggerlo decide lui
        let chosen = book.start_at > 0 || self.progress.get(&book.path).is_some_and(|s| s.pages > 0);
        // la ComicInfo.xml dice se e' un manga da destra a sinistra
        if !chosen && let Some(rtl) = book.info.as_ref().and_then(|i| i.right_to_left) {
            reader.manga = rtl;
        }
        let mut webtoon = false;
        if self.settings.webtoon && !chosen {
            let sizes = book.sample_sizes(7);
            let dims: Vec<(u32, u32)> = sizes.iter().map(|s| s.1).collect();
            if webtoon_width(&dims).is_some() {
                reader.adopt_webtoon(&sizes);
                webtoon = true;
            }
        }
        self.forget_volume();
        reader.set_view(self.page_view().0, self.page_view().1);
        self.reader = Some(reader);
        self.shown.clear();
        self.cpu_pages.clear();
        self.generation = self.loader.set_book(Some(book.clone()));
        self.book = Some(book);
        self.library_open = false;
        self.fit_view();
        self.changed();
        self.refresh_recent();
        // appena aperto, la didascalia dice cosa e dove
        self.ui.reset();
        self.ui.poke(Instant::now());
        if webtoon {
            self.ui.toast(t("Webtoon: lettura a nastro", "Webtoon: strip reading"), Instant::now());
        }
    }

    /// Chiude il volume: resta la galleria vuota, e il file si libera (lo si
    /// puo' spostare o cancellare).
    pub(super) fn close_book(&mut self) {
        if self.book.is_none() {
            return;
        }
        self.remember();
        self.forget_volume();
        self.book = None;
        self.reader = None;
        self.shown.clear();
        self.cpu_pages.clear();
        self.pending_turn = None;
        self.generation = self.loader.set_book(None);
        self.library_open = false;
        self.ui.reset();
        self.refresh_recent();
        self.refresh_shelf();
        self.changed();
    }

    /// La finestra di sistema per scegliere un volume, nella cartella di
    /// quello aperto: il prossimo da leggere di solito sta li'.
    pub(super) fn ask(&mut self, folder: bool) {
        let Some(window) = &self.window else { return };
        if self.dialog {
            return;
        }
        let near = self.book.as_ref().and_then(|b| std::path::absolute(&b.path).ok());
        dialog::open(window, folder, near.as_deref().and_then(Path::parent), false, self.proxy.clone());
        self.dialog = true;
    }

    /// La finestra di sistema per scegliere una cartella per la libreria.
    pub(super) fn ask_library_folder(&mut self) {
        let Some(window) = &self.window else { return };
        if self.dialog {
            return;
        }
        dialog::open(window, true, None, true, self.proxy.clone());
        self.dialog = true;
    }

    /// Un avviso per chi legge: in una finestra di sistema, e sulla console.
    pub fn notify(&mut self, text: String) {
        eprintln!("{text}");
        self.notices.push_back(text);
    }

    /// Mostra cio' che aspetta, prima gli avvisi e poi la richiesta di cosa
    /// aprire: solo a finestra visibile, e un dialogo alla volta.
    pub(super) fn show_pending(&mut self) {
        if self.dialog || !self.visible() {
            return;
        }
        let Some(window) = &self.window else { return };
        if let Some(text) = self.notices.pop_front() {
            dialog::error(window, text, self.proxy.clone());
            self.dialog = true;
        } else if std::mem::take(&mut self.ask_open) {
            self.ask(false);
        }
    }

    /// Annota dove si e' arrivati nel volume aperto (il disco lo vede dopo).
    pub(super) fn remember(&mut self) {
        if let (Some(book), Some(reader)) = (&self.book, &self.reader) {
            keep_position(&mut self.progress, &mut self.marked, &book.path, reader.snapshot());
        }
    }

    /// Salva i progressi su disco. Va chiamata anche all'uscita.
    pub fn save_progress(&mut self) {
        if std::mem::take(&mut self.settings_dirty) {
            self.save_settings();
        }
        self.remember();
        if let Err(e) = self.progress.save() {
            eprintln!("progressi non salvati: {e}");
            if !std::mem::replace(&mut self.save_failed, true) {
                let head = t("Impossibile salvare il punto di lettura.", "Can't save the reading position.");
                self.notify(format!("{head}\n\n{e}"));
            }
        }
        self.last_save = Instant::now();
    }
}

/// Il primo volume dentro una cartella, in ordine naturale di percorso
/// (v01 prima di v02, Vol 1/Ch 1 prima di Vol 1/Ch 2); `None` se non ce ne
/// sono, o se non e' una cartella.
fn first_volume(dir: &Path) -> Option<PathBuf> {
    if !dir.is_dir() {
        return None;
    }
    library::scan(&[dir.to_owned()])
        .into_iter()
        .map(|e| e.path)
        .min_by(|a, b| fumetto_core::natural_cmp(&a.to_string_lossy(), &b.to_string_lossy()))
}

/// Scrive nei progressi dove sta il lettore nel volume aperto. Ma se il
/// volume e' stato appena segnato dal menu (letto, o di nuovo da leggere) e
/// il lettore e' ancora fermo dove era, no: rimetterebbe fra quelli in
/// lettura, alla pagina di prima, il volume appena tolto. Appena ci si muove,
/// si torna a scriverlo: lo si sta leggendo davvero.
fn keep_position(progress: &mut Progress, marked: &mut Option<(PathBuf, Saved)>, path: &Path, now: Saved) {
    if let Some((was, at)) = marked {
        if was.as_path() == path && *at == now {
            return;
        }
        *marked = None;
    }
    progress.set(path, now);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(page: usize) -> Saved {
        Saved { page, pages: 20, ..Saved::default() }
    }

    /// Il volume aperto, segnato da leggere dal menu della libreria, non deve
    /// tornare fra quelli in lettura al salvataggio successivo.
    #[test]
    fn segnato_da_leggere_resta_da_leggere() {
        let path = Path::new("aperto.cbz");
        let mut progress = Progress::in_memory();
        let mut marked = None;
        keep_position(&mut progress, &mut marked, path, at(7));
        assert!(progress.get(path).is_some());

        progress.mark(path, false, 20);
        marked = Some((path.to_owned(), at(7)));
        for _ in 0..3 {
            keep_position(&mut progress, &mut marked, path, at(7));
        }
        assert!(progress.get(path).is_none(), "salvando, il lettore fermo lo rimetteva in lettura");

        keep_position(&mut progress, &mut marked, path, at(8));
        assert_eq!(progress.get(path).map(|s| s.page), Some(8), "ripreso a leggere: di nuovo in lettura");
        assert!(marked.is_none());
    }

    /// Lo stesso per "segna come letto": resta letto, non torna alla pagina di prima.
    #[test]
    fn segnato_letto_resta_letto() {
        let path = Path::new("aperto.cbz");
        let mut progress = Progress::in_memory();
        let mut marked = None;
        keep_position(&mut progress, &mut marked, path, at(3));
        progress.mark(path, true, 20);
        marked = Some((path.to_owned(), at(3)));
        keep_position(&mut progress, &mut marked, path, at(3));
        assert_eq!(progress.get(path).map(|s| s.page), Some(19));
    }

    /// Una cartella di soli archivi: si apre il primo, in ordine naturale.
    #[test]
    fn cartella_di_archivi_apre_il_primo() {
        let dir = std::env::temp_dir().join(format!("fumetto-primo-volume-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Serie")).unwrap();
        for name in ["Serie v10.cbz", "Serie v2.cbz", "Serie v1.cbz"] {
            std::fs::write(dir.join("Serie").join(name), b"x").unwrap();
        }
        assert_eq!(first_volume(&dir), Some(dir.join("Serie").join("Serie v1.cbz")));
        assert_eq!(first_volume(&dir.join("Serie").join("Serie v1.cbz")), None, "un file non e' una cartella");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Un altro volume aperto dopo non eredita la regola.
    #[test]
    fn altro_volume_si_scrive() {
        let mut progress = Progress::in_memory();
        let mut marked = Some((PathBuf::from("vecchio.cbz"), at(3)));
        keep_position(&mut progress, &mut marked, Path::new("nuovo.cbz"), at(3));
        assert!(progress.get(Path::new("nuovo.cbz")).is_some());
        assert!(marked.is_none());
    }
}
