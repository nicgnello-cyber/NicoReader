//! Gli strumenti sopra la lettura: miniature, presentazione, ingrandimento
//! AI, lente, salvare e copiare la pagina.

use super::*;

impl App {
    /// Le miniature sono a schermo: aperte, su un volume, fuori dalla libreria.
    pub(super) fn thumbs_shown(&self) -> bool {
        self.thumbs_open && self.reader.is_some() && !self.shelf_shown()
    }

    /// Chiede all'ingranditore le pagine a schermo che ne guadagnano: quelle
    /// mostrate piu' grandi dei loro pixel, non ancora migliorate.
    pub(super) fn request_upscale(&mut self) {
        let (Some(book), Some(reader)) = (&self.book, &self.reader) else { return };
        if !self.settings.upscale || !self.visible() {
            return;
        }
        let target = reader.target();
        let wanted: Vec<(usize, Target)> = reader
            .on_screen()
            .into_iter()
            .filter(|&i| self.shown.get(&i).is_some_and(|s| !s.upscaled && s.target == target))
            .filter(|&i| reader.native(i).is_some_and(|n| upscale::worth(n, target)))
            .map(|i| (i, target))
            .filter(|w| !self.upscale_failed.contains(w))
            .collect();
        if wanted != self.upscale_wanted {
            let jobs = wanted.iter().map(|&(index, target)| Job {
                book: book.clone(),
                generation: self.generation,
                index,
                target,
            });
            self.upscaler.request(jobs.collect());
            self.upscale_wanted = wanted;
        }
    }

    /// Accende o spegne l'ingranditore.
    pub(super) fn toggle_upscale(&mut self) {
        let now = Instant::now();
        if self.settings.upscale {
            self.settings.upscale = false;
            self.save_settings();
            // le pagine migliorate tornano normali: restano a schermo finche'
            // non arrivano quelle nuove, niente lampi neri
            for s in self.shown.values_mut().filter(|s| s.upscaled) {
                s.stale = true;
            }
            self.upscale_wanted.clear();
            self.upscaler.request(Vec::new());
            self.ui.toast(t("Scansioni come sono", "Scans as they are"), now);
            return self.changed();
        }
        self.settings.upscale = true;
        self.save_settings();
        self.upscale_failed.clear();
        self.ui.toast(t("Scansioni migliorate con l'AI", "Scans enhanced with AI"), now);
        self.changed();
    }

    /// Apre o chiude le miniature di tutte le pagine.
    pub(super) fn toggle_thumbs(&mut self) {
        let Some(reader) = &self.reader else { return };
        self.thumbs_open = !self.thumbs_open;
        if self.thumbs_open {
            self.library_open = false;
            self.ui.reset();
            self.ui.thumbs.open(reader.here());
            if self.thumb_images.is_empty() {
                self.thumb_generation = self.thumb_loader.set_book(self.book.clone());
                self.thumb_wanted = (None, Vec::new());
            }
        }
        self.changed();
    }

    /// Accende o spegne la presentazione: le pagine girano da sole.
    pub(super) fn toggle_slideshow(&mut self) {
        let now = Instant::now();
        if self.slideshow.take().is_some() {
            self.ui.toast(t("Presentazione ferma", "Slideshow stopped"), now);
        } else if self.reader.is_some() {
            let secs = self.settings.slideshow.max(1);
            self.slideshow = Some(now + Duration::from_secs(secs as u64));
            self.slide_last = None;
            let text = if fumetto_core::lingua::italian() {
                format!("Presentazione: una pagina ogni {secs} s")
            } else {
                format!("Slideshow: a page every {secs} s")
            };
            self.ui.toast(text, now);
        }
        self.update_title();
        self.request_redraw();
    }

    /// La presentazione: e' ora di girare pagina?
    pub(super) fn slideshow_tick(&mut self, now: Instant, event_loop: &ActiveEventLoop) -> Option<Instant> {
        let at = self.slideshow?;
        if now < at {
            return Some(at);
        }
        let Some(reader) = &self.reader else {
            self.slideshow = None;
            return None;
        };
        // niente passo avanti se la pagina a schermo non e' ancora arrivata
        if !reader.ready(&self.prepared()) {
            return Some(now + Duration::from_millis(100));
        }
        let here = {
            let s = reader.snapshot();
            (s.page, s.strip_offset)
        };
        if self.slide_last == Some(here) {
            // non ci si muove piu': la fine del volume
            self.slideshow = None;
            self.ui.toast(t("Fine del volume", "End of the volume"), now);
            self.request_redraw();
            return None;
        }
        self.slide_last = Some(here);
        self.act(Action::Next, event_loop);
        let next = now + Duration::from_secs(self.settings.slideshow.max(1) as u64);
        self.slideshow = Some(next);
        Some(next)
    }

    /// Chiede dove salvare la pagina a schermo.
    pub(super) fn ask_save(&mut self) {
        let (Some(book), Some(reader), Some(window)) = (&self.book, &self.reader, &self.window) else { return };
        if self.dialog {
            return;
        }
        let index = reader.here();
        let ext = Path::new(&book.names[index])
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .filter(|_| !book.path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")))
            .unwrap_or_else(|| "png".into());
        let name = format!("{} - {} {:03}.{ext}", book.title, t("pagina", "page"), index + 1);
        let near = std::path::absolute(&book.path).ok();
        dialog::save(window, name, near.as_deref().and_then(Path::parent), self.proxy.clone());
        self.to_save = Some((book.clone(), index));
        self.dialog = true;
    }

    /// Copia negli appunti la pagina a schermo, come la si vede (rifilata,
    /// girata) ma con tutti i suoi pixel.
    pub(super) fn copy_page(&mut self) {
        let (Some(book), Some(reader)) = (&self.book, &self.reader) else { return };
        let target = Target { fit: Fit::Contain { width: 1 << 15, height: 1 << 15 }, ..reader.target() };
        let job = (book.clone(), reader.here(), target);
        let sent = self.copier.as_ref().is_some_and(|c| c.send(job.clone()).is_ok());
        if !sent {
            let (tx, rx) = mpsc::channel();
            let _ = tx.send(job);
            let proxy = self.proxy.clone();
            let spawned = std::thread::Builder::new().name("appunti".into()).spawn(move || copier(rx, proxy));
            self.copier = spawned.is_ok().then_some(tx);
        }
    }

    /// La lente, se si vede adesso: centro (il mouse) e raggio, in pixel.
    pub(super) fn lens_circle(&self) -> Option<(f32, f32, f32)> {
        let on = self.lens && self.reader.is_some() && !self.shelf_shown() && !self.thumbs_shown() && !self.ui.modal();
        on.then(|| (self.cursor.0, self.cursor.1, (self.settings.lens_size * self.scale()).round()))
    }

    pub(super) fn toggle_lens(&mut self) {
        if self.reader.is_none() {
            return;
        }
        self.lens = !self.lens;
        if self.lens && self.lens_images.is_empty() {
            self.lens_generation = self.lens_loader.set_book(self.book.clone());
            self.lens_wanted = (None, Vec::new());
        }
        let note = if self.lens {
            t("Lente: la rotella cambia l'ingrandimento", "Magnifier: the wheel changes the magnification")
        } else {
            t("Lente spenta", "Magnifier off")
        };
        self.ui.toast(note, Instant::now());
        self.request_redraw();
    }

    /// Le pagine sotto la lente e dove disegnarle ingrandite; intanto si
    /// chiedono ai loro pixel veri quelle che mancano.
    pub(super) fn lens_items(&mut self, items: &[Item]) -> Option<((f32, f32, f32), Vec<Item>)> {
        let (cx, cy, r) = self.lens_circle()?;
        let m = self.settings.lens_zoom;
        let under: Vec<Item> = items
            .iter()
            .filter(|it| it.x < cx + r && it.x + it.w > cx - r && it.y < cy + r && it.y + it.h > cy - r)
            .map(|it| Item {
                page: it.page,
                x: cx + (it.x - cx) * m,
                y: cy + (it.y - cy) * m,
                w: it.w * m,
                h: it.h * m,
            })
            .collect();
        let reader = self.reader.as_ref()?;
        // ai pixel veri: la pagina intera, senza rimpicciolirla (fino ai limiti
        // di una texture), ma rifilata e girata come quella a schermo
        let target = Target { fit: Fit::Contain { width: 8192, height: 1 << 16 }, ..reader.target() };
        let wanted: Vec<usize> =
            under.iter().map(|it| it.page).filter(|p| self.lens_images.get(p).is_none_or(|l| l.1 != target)).collect();
        if (Some(target), &wanted) != (self.lens_wanted.0, &self.lens_wanted.1) {
            self.lens_loader.set_target(target);
            self.lens_loader.request(&wanted);
            self.lens_wanted = (Some(target), wanted);
        }
        Some(((cx, cy, r), under))
    }
}

/// Salva la pagina `index` in `path`. Se l'estensione scelta e' quella della
/// pagina, i suoi byte cosi' come sono nell'archivio (nessuna perdita); se no
/// (o se viene da un PDF) i suoi pixel, nel formato che dice l'estensione.
pub(super) fn save_page(book: &Book, index: usize, path: &Path) -> Result<(), String> {
    let fail = |e: String| format!("{}\n\n{e}", t("Impossibile salvare la pagina.", "Can't save the page."));
    let want = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let same = |name: &str| {
        let have = Path::new(name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let jpeg = |e: &str| matches!(e, "jpg" | "jpeg" | "jpe" | "jfif");
        have == want || (jpeg(&have) && jpeg(&want))
    };
    // una pagina PDF disegnata: alla sua misura di riferimento (300 DPI)
    let big = Fit::Contain { width: 1 << 15, height: 1 << 15 };
    let page = match book.content(index, big).map_err(|e| fail(e.to_string()))? {
        Content::Encoded(bytes) if same(&book.names[index]) => {
            return std::fs::write(path, bytes).map_err(|e| fail(e.to_string()));
        }
        Content::Encoded(bytes) => fumetto_core::decode(&bytes).map_err(|e| fail(e.to_string()))?,
        Content::Pixels(p) | Content::Exact(p, _) => p,
    };
    let rgb: Vec<u8> = page.rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
    let saved = if page.opaque || matches!(want.as_str(), "jpg" | "jpeg") {
        image::save_buffer(path, &rgb, page.width, page.height, image::ColorType::Rgb8)
    } else {
        image::save_buffer(path, &page.rgba, page.width, page.height, image::ColorType::Rgba8)
    };
    saved.map_err(|e| fail(e.to_string()))
}

/// Il thread degli appunti: prepara le pagine da copiare e le tiene negli
/// appunti del sistema.
fn copier(jobs: mpsc::Receiver<(Arc<Book>, usize, Target)>, proxy: EventLoopProxy<UserEvent>) {
    let mut clipboard = None;
    for (book, index, target) in jobs {
        let done = fumetto_core::decode_page(&book, index, target).and_then(|d| {
            if clipboard.is_none() {
                clipboard = Some(arboard::Clipboard::new().map_err(|e| e.to_string())?);
            }
            let image = arboard::ImageData {
                width: d.page.width as usize,
                height: d.page.height as usize,
                bytes: d.page.rgba.into(),
            };
            clipboard.as_mut().expect("appena creata").set_image(image).map_err(|e| e.to_string())
        });
        let done = done
            .map(|_| t("Pagina copiata", "Page copied"))
            .map_err(|e| format!("{}\n\n{e}", t("Impossibile copiare la pagina.", "Can't copy the page.")));
        let _ = proxy.send_event(UserEvent::Done(done));
    }
}
