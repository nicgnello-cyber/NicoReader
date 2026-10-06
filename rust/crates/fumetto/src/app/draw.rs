//! Le pagine che arrivano e la scena da disegnare, con la scheda video o
//! copiando dal processore.

use super::*;

impl App {
    pub(super) fn prefetch(&mut self) {
        // finche' la finestra non e' visibile non ha ancora la sua misura vera
        // (Windows la massimizza nel momento in cui la mostra): le pagine
        // preparate adesso andrebbero rifatte, o peggio mostrate ingrandite
        if !self.visible() {
            return;
        }
        let Some(reader) = &mut self.reader else { return };
        let (target, order) = reader.prefetch(&Prepared { shown: &self.shown, cpu: &self.cpu_pages });
        self.loader.set_target(target);
        self.loader.request(&order);
    }

    pub(super) fn request_redraw(&self) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    pub(super) fn arrived(&mut self, loaded: Loaded) {
        if loaded.generation != self.generation {
            return; // un ritardatario del volume precedente
        }
        if self.stats.first_loaded_ms.is_none() {
            self.stats.first_loaded_ms = Some(self.stats.now_ms());
        }
        let page = match loaded.page {
            Ok(p) => p,
            Err(e) => {
                eprintln!("pagina {}: {e}", loaded.index + 1);
                if let Some(reader) = &mut self.reader {
                    reader.failed(loaded.index);
                    if reader.broken_here() {
                        self.ui.toast(t("Pagina illeggibile", "Unreadable page"), Instant::now());
                    }
                }
                return self.changed();
            }
        };
        self.stats.prepared.push((loaded.read_ms, loaded.decode_ms, loaded.resize_ms));
        let Some(reader) = &mut self.reader else { return };
        // una pagina preparata per una misura vecchia non sostituisce una
        // giusta, e una normale non sostituisce la sua versione migliorata
        let current = reader.target();
        let have = Prepared { shown: &self.shown, cpu: &self.cpu_pages }.prepared_for(loaded.index);
        let upscaled = self.shown.get(&loaded.index).is_some_and(|s| s.upscaled);
        if have == Some(current) && (loaded.target != current || upscaled) {
            return;
        }
        // la misura va con la pagina che resta a schermo: dopo una rotazione,
        // finche' non arriva quella girata, valgono entrambe quelle di prima
        reader.known(loaded.index, loaded.native);
        self.keep(loaded.index, page, loaded.target);
        self.request_upscale();
        // arrivata una pagina a schermo, la lettura anticipata puo' allargarsi
        // alle successive
        self.prefetch();
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    /// Una pagina pronta va sulla scheda video; se non c'e' ancora, resta in
    /// memoria e la si disegna dal processore.
    fn keep(&mut self, index: usize, page: Page, target: Target) {
        match &self.gfx {
            Some(gfx) => match gfx.renderer.upload(&page) {
                Ok(image) => {
                    self.shown.insert(index, Shown { image, target, upscaled: false, stale: false });
                }
                Err(e) => eprintln!("pagina {}: {e}", index + 1),
            },
            None => {
                self.cpu_pages.insert(index, (page, target));
            }
        }
        self.evict();
    }

    /// Tiene la memoria sotto il budget, buttando le pagine piu' lontane da
    /// quella che si sta leggendo.
    fn evict(&mut self) {
        let here = self.reader.as_ref().map_or(0, |r| r.here());
        let far_from = |keys: &mut dyn Iterator<Item = &usize>| {
            keys.copied().max_by_key(|i| i.abs_diff(here)).filter(|i| i.abs_diff(here) > 4)
        };
        let mut total: usize = self.shown.values().map(|s| s.image.bytes()).sum();
        while total > BUDGET
            && let Some(far) = far_from(&mut self.shown.keys())
        {
            total -= self.shown.remove(&far).map_or(0, |s| s.image.bytes());
        }
        let mut total: usize = self.cpu_pages.values().map(|p| p.0.bytes()).sum();
        while total > BUDGET
            && let Some(far) = far_from(&mut self.cpu_pages.keys())
        {
            total -= self.cpu_pages.remove(&far).map_or(0, |p| p.0.bytes());
        }
    }

    pub(super) fn redraw(&mut self) {
        // il tempo fra l'inizio di questo fotogramma e l'inizio del precedente:
        // l'attesa del vblank sta in mezzo (dentro get_current_texture). Misurarlo
        // dalla consegna del precedente dava 0,05 ms invece di 16,6, e lo
        // scorrimento andava trecento volte piu' piano del voluto
        let now = Instant::now();
        // il centro della lente e' il puntatore: la freccia coprirebbe il
        // dettaglio. Dove la lente non si vede (un menu, la libreria) torna
        let hide = self.lens_circle().is_some();
        if hide != self.cursor_hidden
            && let Some(w) = &self.window
        {
            w.set_cursor_visible(!hide);
            self.cursor_hidden = hide;
        }
        if self.shelf_shown() {
            return self.draw_shelf(now);
        }
        if self.thumbs_shown() {
            return self.draw_thumbs(now);
        }
        let top = self.top() as f32;
        let dt = self.last_tick.map_or(self.stats.refresh_ms / 1000.0, |t| (now - t).as_secs_f32()).min(0.05);
        self.last_tick = Some(now);
        let Some(reader) = &mut self.reader else {
            // nessun volume: la galleria vuota
            let ctx = context(
                self.window.as_deref(),
                &self.book,
                None,
                &self.recent,
                &[],
                self.settings.hud,
                None,
                None,
                look!(self),
            );
            let scene = self.ui.scene(&ctx, now, measure(&mut self.gfx, &mut Estimate));
            self.present(&[], &[], &scene);
            return;
        };
        let gliding = reader.advance_glide(dt);
        let prepared = Prepared { shown: &self.shown, cpu: &self.cpu_pages };
        let mut items = reader.layout(&prepared);
        for it in &mut items {
            it.y += top;
        }
        let ready = reader.ready(&prepared);
        let lens = self.lens_items(&items);
        let ctx = context(
            self.window.as_deref(),
            &self.book,
            self.reader.as_ref(),
            &self.recent,
            &items,
            self.settings.hud,
            None,
            None,
            look!(self),
        );
        let scene = self.ui.scene(&ctx, now, measure(&mut self.gfx, &mut Estimate));
        let Some(presented) = self.present_with(&items, &[], lens, &scene) else { return };
        if let Some(reader) = &mut self.reader {
            reader.drawn(&Prepared { shown: &self.shown, cpu: &self.cpu_pages });
        }

        if ready && self.stats.first_page_ms.is_none() {
            self.stats.first_page_ms = Some((presented - self.stats.started).as_secs_f32() * 1000.0);
        }
        if let Some(turn) = &mut self.pending_turn {
            if ready {
                self.stats.turns.push((presented - turn.since).as_secs_f32() * 1000.0);
                self.stats.misses += turn.missed as u32;
                self.pending_turn = None;
            } else {
                turn.missed = true;
            }
        }
        if gliding {
            if let Some(t) = self.last_present {
                self.stats.frames.push((presented - t).as_secs_f32() * 1000.0);
            }
            // lungo il nastro le pagine a schermo cambiano: la lettura anticipata segue
            self.prefetch();
            self.update_title();
            self.remember();
            match (&self.gfx, &self.window) {
                // la scheda video aspetta il vblank da sola
                (Some(_), Some(w)) => w.request_redraw(),
                // dal processore nessuno aspetta: si riprova al prossimo vblank
                // invece di girare a vuoto consumando un core
                _ => self.retry_at = Some(presented + Duration::from_secs_f32(self.stats.refresh_ms / 1000.0)),
            }
        } else {
            self.last_tick = None; // il prossimo scorrimento riparte da un fotogramma pieno
        }
        self.last_present = gliding.then_some(presented);
    }

    /// La libreria: le copertine alla misura delle celle, e sopra il resto.
    fn draw_shelf(&mut self, now: Instant) {
        let (vw, vh) = self.view_size();
        let view = (vw as f32, vh as f32);
        let scale = self.scale();
        let covers = &self.covers;
        let covered = |p: &Path| covers.contains_key(p);
        let d = shelf_data(&self.lib, &self.settings.library, &covered, self.book.is_some());
        let slots = self.ui.shelf.cover_slots(&d, view, scale);
        let px = |v: f32| v.round().max(1.0) as u32;
        let wanted: Vec<(PathBuf, (u32, u32))> = slots
            .iter()
            .map(|(p, _, _, w, h, _)| (p.clone(), (px(*w), px(*h))))
            .filter(|(p, size)| !self.covers_failed.contains(p) && self.covers.get(p).is_none_or(|c| c.1 != *size))
            .collect();
        let ctx = context(
            self.window.as_deref(),
            &self.book,
            self.reader.as_ref(),
            &self.recent,
            &[],
            self.settings.hud,
            Some(d),
            None,
            look!(self),
        );
        let scene = self.ui.scene(&ctx, now, measure(&mut self.gfx, &mut Estimate));
        if wanted != self.cover_wanted {
            self.cover_loader.request(wanted.clone());
            self.cover_wanted = wanted;
        }
        let shown: Vec<(Extra, f32, f32, f32, f32)> =
            slots.iter().filter(|s| s.5).map(|(p, x, y, w, h, _)| (Extra::Cover(p.clone()), *x, *y, *w, *h)).collect();
        // le copertine lontane si lasciano andare: a 170x255 pesano 170 KB l'una
        if self.covers.len() > 400 {
            let keep: std::collections::HashSet<&PathBuf> = slots.iter().map(|s| &s.0).collect();
            self.covers.retain(|p, _| keep.contains(p));
        }
        self.present(&[], &shown, &scene);
    }

    /// Le miniature: ogni pagina nella sua cella, alla misura della cella.
    fn draw_thumbs(&mut self, now: Instant) {
        let (vw, vh) = self.view_size();
        let view = (vw as f32, vh as f32);
        let scale = self.scale();
        let images = &self.thumb_images;
        let ready = |i: usize| images.contains_key(&i);
        let Some(d) = thumbs_data(&self.book, self.reader.as_ref(), &ready) else { return };
        let (cw, ch) = Thumbs::cell_size(&d, view, scale);
        let base = self.reader.as_ref().map_or(Target::plain(Fit::Width(1)), |r| r.target());
        let target = Target { fit: Fit::Contain { width: cw, height: ch }, ..base };
        let slots = self.ui.thumbs.slots(&d, view, scale);
        let wanted: Vec<usize> =
            slots.iter().map(|s| s.0).filter(|i| images.get(i).is_none_or(|t| t.1 != target)).collect();
        if (Some(target), &wanted) != (self.thumb_wanted.0, &self.thumb_wanted.1) {
            self.thumb_loader.set_target(target);
            self.thumb_loader.request(&wanted);
            self.thumb_wanted = (Some(target), wanted);
        }
        // ogni miniatura sta nella sua cella, centrata, con la sua forma
        let shown: Vec<(Extra, f32, f32, f32, f32)> = slots
            .iter()
            .filter(|s| s.5)
            .filter_map(|&(i, x, y, w, h, _)| {
                let (img, _) = images.get(&i)?;
                let (iw, ih) = Fit::Contain { width: w as u32, height: h as u32 }.size(img.width, img.height);
                let (iw, ih) = (iw as f32, ih as f32);
                Some((Extra::Thumb(i), (x + (w - iw) / 2.0).round(), (y + h - ih).round(), iw, ih))
            })
            .collect();
        let ctx = context(
            self.window.as_deref(),
            &self.book,
            self.reader.as_ref(),
            &self.recent,
            &[],
            self.settings.hud,
            None,
            Some(d),
            look!(self),
        );
        let scene = self.ui.scene(&ctx, now, measure(&mut self.gfx, &mut Estimate));
        // le miniature lontane si lasciano andare
        if self.thumb_images.len() > THUMBS_KEPT {
            let keep: HashSet<usize> = slots.iter().map(|s| s.0).collect();
            self.thumb_images.retain(|i, _| keep.contains(i));
        }
        self.present(&[], &shown, &scene);
    }

    /// Disegna e consegna; `None` se il fotogramma e' saltato. La prima volta
    /// rende visibile la finestra, che solo allora ha la sua misura vera.
    fn present(&mut self, items: &[Item], covers: &[(Extra, f32, f32, f32, f32)], scene: &Scene) -> Option<Instant> {
        self.present_with(items, covers, None, scene)
    }

    /// Come `present`, con la lente: (cerchio, pagine ingrandite).
    fn present_with(
        &mut self, items: &[Item], covers: &[(Extra, f32, f32, f32, f32)], lens: Option<((f32, f32, f32), Vec<Item>)>,
        scene: &Scene,
    ) -> Option<Instant> {
        let presented = match self.gfx {
            // ponytail: l'interfaccia, le copertine e la lente le disegna solo
            // la scheda video; nei primi 600 ms, dal processore, si vedono solo
            // le pagine (con le regolazioni, che la' costano una tabella)
            Some(_) => self.present_gpu(items, covers, lens, scene),
            None => self.present_cpu(items),
        }?;
        if let Some(w) = &self.window
            && w.is_visible() == Some(false)
        {
            // la finestra appare gia' nera, mai bianca per un istante
            w.set_visible(true);
            self.fit_view();
            self.changed();
        }
        Some(presented)
    }

    fn present_gpu(
        &mut self, items: &[Item], covers: &[(Extra, f32, f32, f32, f32)], lens: Option<((f32, f32, f32), Vec<Item>)>,
        scene: &Scene,
    ) -> Option<Instant> {
        // le copertine della libreria restano com'erano: le regolazioni sono per le pagine
        let adjust = if covers.iter().any(|c| matches!(c.0, Extra::Cover(_))) { Adjust::NONE } else { self.adjust() };
        let target = self.reader.as_ref().map(|r| r.target());
        let size = self.view_size();
        let gfx = self.gfx.as_mut()?;
        let acquire = Instant::now();
        let frame = match gfx.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                gfx.surface.configure(&gfx.gpu.device, &gfx.config);
                gfx.window.request_redraw();
                return None;
            }
            // finestra coperta, ridotta a icona o schermo bloccato: se si stava
            // scorrendo, senza un nuovo tentativo lo scorrimento resterebbe
            // fermo a meta' per sempre
            wgpu::CurrentSurfaceTexture::Occluded => {
                self.retry_at = Some(Instant::now() + Duration::from_millis(100));
                return None;
            }
            // un fotogramma saltato non deve fermare uno scorrimento a meta'
            other => {
                self.stats.acquire_failures.push(format!("{other:?}"));
                gfx.window.request_redraw();
                return None;
            }
        };
        self.stats.acquire.push(acquire.elapsed().as_secs_f32() * 1000.0);
        let placements: Vec<Placement> = items
            .iter()
            .filter_map(|it| {
                self.shown.get(&it.page).map(|s| Placement { image: &s.image, x: it.x, y: it.y, w: it.w, h: it.h })
            })
            .chain(covers.iter().filter_map(|(what, x, y, w, h)| {
                let image = match what {
                    Extra::Cover(p) => &self.covers.get(p)?.0,
                    Extra::Thumb(i) => &self.thumb_images.get(i)?.0,
                };
                Some(Placement { image, x: *x, y: *y, w: *w, h: *h })
            }))
            .collect();
        // nella lente, le pagine ai loro pixel se sono gia' arrivate (e girate
        // e rifilate come quelle a schermo), se no quelle a schermo, ingrandite
        let same = |t: &Target| target.is_some_and(|c| (c.trim, c.rotation) == (t.trim, t.rotation));
        let lens_placements: Vec<Placement> = lens.as_ref().map_or(Vec::new(), |(_, under)| {
            under
                .iter()
                .filter_map(|it| {
                    let image = match self.lens_images.get(&it.page) {
                        Some((image, t)) if same(t) => image,
                        _ => &self.shown.get(&it.page)?.image,
                    };
                    Some(Placement { image, x: it.x, y: it.y, w: it.w, h: it.h })
                })
                .collect()
        });
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = gfx.gpu.device.create_command_encoder(&Default::default());
        gfx.renderer.draw(&mut encoder, &view, gfx.config.format, size, &placements, Pass { adjust, ..Pass::PLAIN });
        if let Some((circle, _)) = lens {
            gfx.renderer.draw(
                &mut encoder,
                &view,
                gfx.config.format,
                size,
                &lens_placements,
                Pass { clear: false, adjust, clip: Some(circle) },
            );
        }
        if let Err(e) = gfx.overlay.draw(&mut encoder, &view, size, scene) {
            eprintln!("interfaccia: {e}");
        }
        gfx.gpu.queue.submit([encoder.finish()]);
        gfx.window.pre_present_notify();
        gfx.gpu.queue.present(frame);
        Some(Instant::now())
    }

    /// Disegna copiando dal processore, finche' la scheda video non c'e'.
    fn present_cpu(&mut self, items: &[Item]) -> Option<Instant> {
        let size = self.view_size();
        let cpu = self.cpu.as_mut()?;
        let list: Vec<cpu_view::Item> = items
            .iter()
            .filter_map(|it| {
                self.cpu_pages.get(&it.page).map(|(page, _)| cpu_view::Item {
                    page,
                    x: it.x,
                    y: it.y,
                    w: it.w,
                    h: it.h,
                })
            })
            .collect();
        let adjust = Adjust::from_steps(self.settings.brightness, self.settings.contrast, self.settings.gamma);
        let table = (!adjust.is_none()).then(|| adjust.table());
        if let Err(e) = cpu.draw(size, &list, table.as_ref()) {
            eprintln!("finestra: {e}");
            return None;
        }
        Some(Instant::now())
    }

    /// La scheda video e' pronta: prende il posto della copia dal processore.
    pub(super) fn gpu_arrived(&mut self, start: GpuStart) {
        let t = Instant::now();
        match self.init_gpu(start) {
            Ok(()) => {
                self.stats.note(&format!("passaggio alla scheda video: {:.0} ms", t.elapsed().as_secs_f32() * 1000.0));
                self.changed();
            }
            Err(e) => eprintln!("scheda video: {e}; si continua copiando dal processore"),
        }
    }

    fn init_gpu(&mut self, start: GpuStart) -> Result<(), String> {
        let window = self.window.clone().ok_or("nessuna finestra")?;
        let GpuStart { instance, gpu, renderer, mut overlay, steps } = start;
        for (what, ms) in steps {
            self.stats.notes.push(format!("{ms:6.0} ms  scheda video: {what}"));
        }
        let surface = instance.create_surface(window.clone()).map_err(|e| e.to_string())?;
        if !gpu.adapter.is_surface_supported(&surface) {
            // ponytail: scheda scelta senza conoscere la finestra; se non la sa
            // disegnare (mai visto finora) si resta sulla copia dal processore
            return Err(format!("{} non sa disegnare in questa finestra", gpu.describe()));
        }
        let caps = surface.get_capabilities(&gpu.adapter);
        // formato non sRGB: i valori delle pagine sono gia' codificati e si copiano
        let format = if caps.formats.contains(&gpu_start::LIKELY_FORMAT) {
            gpu_start::LIKELY_FORMAT
        } else {
            caps.formats.iter().copied().find(|f| !f.is_srgb()).ok_or("nessun formato di schermo adatto")?
        };
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: Default::default(),
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            // un solo fotogramma in coda: il tasto si vede al vblank successivo
            desired_maximum_frame_latency: 1,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        // da qui non si torna indietro: la copia dal processore lascia la
        // finestra alla scheda video, e le pagine gia' pronte la seguono
        self.cpu = None;
        surface.configure(&gpu.device, &config);
        if format != gpu_start::LIKELY_FORMAT {
            overlay = Overlay::new(&gpu, format);
        }
        self.gfx = Some(Gfx { window, surface, config, gpu, renderer, overlay });
        for (index, (page, target)) in std::mem::take(&mut self.cpu_pages) {
            self.keep(index, page, target);
        }
        self.stats.gpu_ms = Some(self.stats.now_ms());
        Ok(())
    }

    pub fn gpu_name(&self) -> String {
        self.gfx.as_ref().map_or("-".into(), |g| g.gpu.describe())
    }
}

/// Cio' che l'interfaccia deve sapere, dai pezzi dell'app che servono (e
/// non da tutta l'app, cosi' l'interfaccia si puo' cambiare intanto).
#[allow(clippy::too_many_arguments)]
pub(super) fn context<'a>(
    window: Option<&Window>, book: &'a Option<Arc<Book>>, reader: Option<&'a Reader>, recent: &'a [Recent],
    items: &[Item], hud: bool, shelf: Option<ShelfData<'a>>, thumbs: Option<ThumbsData<'a>>, look: Look<'a>,
) -> Context<'a> {
    let slideshow = look.slideshow;
    let (vw, vh) = window.map_or((1.0, 1.0), |w| {
        let s = w.inner_size();
        (s.width.max(1) as f32, s.height.max(1) as f32)
    });
    let scale = window.map_or(1.0, |w| w.scale_factor() as f32);
    let book = book.as_ref().zip(reader).map(|(b, r)| {
        let left = items.iter().map(|i| i.x).fold(f32::INFINITY, f32::min);
        let right = items.iter().map(|i| i.x + i.w).fold(f32::NEG_INFINITY, f32::max);
        let margins = if items.is_empty() { (vw / 2.0, vw / 2.0) } else { (left.max(0.0), (vw - right).max(0.0)) };
        let mut modes = r.modes();
        if slideshow {
            modes.push(t("presentazione", "slideshow"));
        }
        BookInfo {
            title: b.title.as_str(),
            folio: r.folio(),
            pages: r.pages(),
            here: r.here(),
            modes,
            margins,
            zoom: r.zoom_percent(),
            double: r.mode == Mode::Double,
            strip: r.mode == Mode::Strip,
            manga: r.manga,
            cover_alone: r.cover_alone(),
            bookmarked: r.bookmarked(),
            bookmarks: r.bookmarks(),
            trim: r.trim,
            slideshow,
        }
    });
    let fullscreen = window.is_some_and(|w| w.fullscreen().is_some());
    Context {
        view: (vw, vh),
        scale,
        book,
        recent,
        hud,
        fullscreen,
        shelf,
        thumbs,
        settings: look.settings,
        keys: look.keys,
        lens: look.lens,
    }
}

/// Le miniature come le vede l'interfaccia.
pub(super) fn thumbs_data<'a>(
    book: &'a Option<Arc<Book>>, reader: Option<&'a Reader>, ready: &'a dyn Fn(usize) -> bool,
) -> Option<ThumbsData<'a>> {
    let (book, r) = (book.as_ref()?, reader?);
    Some(ThumbsData {
        title: &book.title,
        pages: r.pages(),
        here: r.here(),
        bookmarks: r.bookmarks(),
        ratio: r.typical_ratio(),
        ready,
    })
}

/// Per i tasti e i clic le miniature pronte non contano.
pub(super) fn always_ready(_: usize) -> bool {
    true
}

/// Chi misura i testi: i caratteri veri, o una stima finche' la scheda
/// video non c'e' (tanto, fino ad allora, l'interfaccia non si disegna).
fn measure<'a>(gfx: &'a mut Option<Gfx>, estimate: &'a mut Estimate) -> &'a mut dyn Measure {
    match gfx {
        Some(g) => &mut g.overlay,
        None => estimate,
    }
}
