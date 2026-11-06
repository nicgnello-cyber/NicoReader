//! Scatti dell'interfaccia vera, fuori schermo: le pagine di un volume vero,
//! preparate come le prepara l'app, e sopra la didascalia, "vai a pagina", il
//! menu, la galleria vuota. Per guardare il risultato senza aprire finestre
//! (e senza disturbare chi sta usando il computer).

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use fumetto::keys::Keymap;
use fumetto::reader::{Action, Item, Pages, Reader, Zoom};
use fumetto::thumbs::{Thumbs, ThumbsData};
use fumetto::ui::{self, BookInfo, Context, Key, Recent, Ui};
use fumetto_core::{Book, Fit, Loaded, Loader, Settings, Target, decode_page, to_screen};
use fumetto_render::{Adjust, GpuImage, OFFSCREEN_FORMAT, Overlay, Pass, Placement, Renderer, wgpu};

mod libreria;

pub use libreria::libreria;

/// Le impostazioni di serie, per gli scatti che non ne vogliono altre.
static DEFAULTS: std::sync::LazyLock<Settings> = std::sync::LazyLock::new(Settings::default);

struct Ready(HashMap<usize, (GpuImage, Target)>);

impl Pages for Ready {
    fn prepared_for(&self, i: usize) -> Option<Target> {
        self.0.get(&i).map(|p| p.1)
    }
}

pub fn run(
    volume: &Path, out: &Path, size: (u32, u32), scale: f32, power: wgpu::PowerPreference,
) -> Result<(), String> {
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let (gpu, renderer) = crate::gpu(power)?;
    let t = Instant::now();
    let overlay = Overlay::new(&gpu, OFFSCREEN_FORMAT);
    println!("caratteri e interfaccia pronti in {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
    let book = Arc::new(Book::open(volume).map_err(|e| e.to_string())?);
    let mut studio = Studio::new(book, &renderer, overlay, out, size, scale, neighbours(volume));
    reading_shots(&mut studio)?;
    miniature(&studio.book, &renderer, &mut studio.overlay, out, size, scale, studio.start)?;
    gallery(&mut studio)
}

/// Cio' che serve agli scatti di lettura: il volume e chi ne prepara le
/// pagine, la scheda video, la finestra (finta) e dove salvare.
struct Studio<'a> {
    book: Arc<Book>,
    loader: Loader,
    arrived: mpsc::Receiver<Loaded>,
    renderer: &'a Renderer,
    overlay: Overlay,
    out: &'a Path,
    size: (u32, u32),
    scale: f32,
    recent: Vec<Recent>,
    /// La pagina da cui si parte: un ottavo del volume.
    start: usize,
    /// Il momento dello scatto: didascalie gia' comparse.
    later: Instant,
}

/// Una pagina nella lente: l'immagine ai suoi pixel e dove sta, ingrandita
/// (x, y, larghezza, altezza).
type Magnified = (GpuImage, f32, f32, f32, f32);

/// Come si presenta uno scatto: con la barra in alto (le pagine stanno
/// sotto), con quali impostazioni (regolazioni dell'immagine, lente), e
/// dove sta la lente, come frazione della finestra.
struct Look<'s> {
    hud: bool,
    settings: &'s Settings,
    lens: Option<(f32, f32)>,
}

impl Look<'_> {
    fn bar() -> Look<'static> {
        Look { hud: true, settings: &DEFAULTS, lens: None }
    }

    fn bare() -> Look<'static> {
        Look { hud: false, ..Look::bar() }
    }
}

impl<'a> Studio<'a> {
    fn new(
        book: Arc<Book>, renderer: &'a Renderer, overlay: Overlay, out: &'a Path, size: (u32, u32), scale: f32,
        recent: Vec<Recent>,
    ) -> Studio<'a> {
        let (tx, arrived) = mpsc::channel();
        let loader = Loader::new(Loader::default_threads(), move |l| {
            let _ = tx.send(l);
        });
        loader.set_book(Some(book.clone()));
        let n = book.len();
        let start = (n / 8).min(n.saturating_sub(1));
        let later = Instant::now() + Duration::from_millis(700);
        Studio { book, loader, arrived, renderer, overlay, out, size, scale, recent, start, later }
    }

    fn view(&self) -> (f32, f32) {
        (self.size.0 as f32, self.size.1 as f32)
    }

    /// Uno scatto: `mode` prepara il lettore (decide quali pagine servono),
    /// `dress` l'interfaccia sopra.
    fn shot(
        &mut self, name: &str, look: Look, mode: &dyn Fn(&mut Reader), dress: &dyn Fn(&mut Ui, &Context),
    ) -> Result<(), String> {
        let (scale, size, view) = (self.scale, self.size, self.view());
        let top = if look.hud { ui::hud_height(scale) } else { 0.0 };
        let mut reader = Reader::new(self.book.len(), self.start);
        reader.set_view(size.0, size.1 - top as u32);
        mode(&mut reader);
        let ready = self.prepared(name, &mut reader)?;
        let mut items = reader.layout(&ready);
        for it in &mut items {
            it.y += top;
        }
        let placements: Vec<Placement> = items
            .iter()
            .filter_map(|it| ready.0.get(&it.page).map(|(image, _)| Placement::new(image, it.x, it.y, it.w, it.h)))
            .collect();
        let st = look.settings;
        let ctx = Context {
            view,
            scale,
            book: Some(book_info(&self.book.title, &reader, &items, view.0)),
            recent: &self.recent,
            hud: look.hud,
            fullscreen: false,
            shelf: None,
            thumbs: None,
            settings: st,
            keys: Keymap::defaults(),
            lens: look.lens.map(|(fx, fy)| (fx * view.0, fy * view.1, (st.lens_size * scale).round())),
        };
        let native = match ctx.lens {
            Some(circle) => self.lens_pages(&reader, &items, circle, st.lens_zoom)?,
            None => Vec::new(),
        };
        let lens_placements: Vec<Placement> =
            native.iter().map(|(image, x, y, w, h)| Placement::new(image, *x, *y, *w, *h)).collect();
        let adjust = Adjust::from_steps(st.brightness, st.contrast, st.gamma);
        let mut ui = Ui::new();
        dress(&mut ui, &ctx);
        let scene = ui.scene(&ctx, self.later, &mut self.overlay);
        let (renderer, overlay) = (self.renderer, &mut self.overlay);
        let rgba =
            renderer.render_to_rgba_with(size, &placements, Pass { adjust, ..Pass::PLAIN }, |encoder, target| {
                if let Some(circle) = ctx.lens {
                    renderer.draw(
                        encoder,
                        target,
                        OFFSCREEN_FORMAT,
                        size,
                        &lens_placements,
                        Pass { clear: false, adjust, clip: Some(circle) },
                    );
                }
                if let Err(e) = overlay.draw(encoder, target, size, &scene) {
                    eprintln!("{name}: {e}");
                }
            });
        crate::save(&self.out.join(format!("{name}.png")), &rgba, size.0, size.1)?;
        println!("{name}.png");
        Ok(())
    }

    /// Le pagine che il lettore vuole a schermo, preparate come le prepara
    /// l'app e caricate sulla scheda video.
    fn prepared(&self, name: &str, reader: &mut Reader) -> Result<Ready, String> {
        let mut ready = Ready(HashMap::new());
        let deadline = Instant::now() + Duration::from_secs(20);
        while !reader.ready(&ready) {
            let (target, order) = reader.prefetch(&ready);
            self.loader.set_target(target);
            self.loader.request(&order);
            let l = self
                .arrived
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| format!("{name}: le pagine non arrivano"))?;
            let page = l.page?;
            reader.known(l.index, l.native);
            ready.0.insert(l.index, (self.renderer.upload(&page)?, l.target));
        }
        Ok(ready)
    }

    /// La lente: le pagine sotto di lei, ai loro pixel, ingrandite di
    /// `zoom` attorno al centro.
    fn lens_pages(
        &self, reader: &Reader, items: &[Item], (cx, cy, r): (f32, f32, f32), zoom: f32,
    ) -> Result<Vec<Magnified>, String> {
        let under =
            items.iter().filter(|it| it.x < cx + r && it.x + it.w > cx - r && it.y < cy + r && it.y + it.h > cy - r);
        let mut native = Vec::new();
        for it in under {
            let target = Target { fit: Fit::Contain { width: 8192, height: 1 << 16 }, ..reader.target() };
            let page = to_screen(decode_page(&self.book, it.page, target)?, target);
            let m = zoom;
            native.push((self.renderer.upload(&page)?, cx + (it.x - cx) * m, cy + (it.y - cy) * m, it.w * m, it.h * m));
        }
        Ok(native)
    }
}

/// Il volume aperto, come lo vede l'interfaccia (con `view_w`, la
/// larghezza della finestra, i margini ai lati delle pagine).
fn book_info<'r>(title: &'r str, reader: &'r Reader, items: &[Item], view_w: f32) -> BookInfo<'r> {
    let left = items.iter().map(|i| i.x).fold(f32::INFINITY, f32::min).max(0.0);
    let right = (view_w - items.iter().map(|i| i.x + i.w).fold(0.0, f32::max)).max(0.0);
    BookInfo {
        title,
        folio: reader.folio(),
        pages: reader.pages(),
        here: reader.here(),
        modes: reader.modes(),
        margins: (left, right),
        zoom: reader.zoom_percent(),
        double: reader.mode == fumetto::reader::Mode::Double,
        strip: reader.mode == fumetto::reader::Mode::Strip,
        manga: reader.manga,
        cover_alone: reader.cover_alone(),
        bookmarked: reader.bookmarked(),
        bookmarks: reader.bookmarks(),
        trim: reader.trim,
        slideshow: false,
        next: None,
        prev: None,
    }
}

/// Gli scatti di lettura: la barra, la doppia pagina, il nastro, i segni,
/// "vai a pagina", il menu, le impostazioni, la lente.
fn reading_shots(studio: &mut Studio) -> Result<(), String> {
    let now = Instant::now();
    let double = |r: &mut Reader| {
        r.act(Action::ToggleDouble, now);
    };
    let unchanged = |_: &mut Reader| {};
    studio.shot("barra-doppia", Look::bar(), &double, &|ui, ctx| {
        // il mouse sopra "doppia pagina": si vede il suo nome
        let x = ctx.view.0 - 350.0 * ctx.scale;
        ui.motion(x, 20.0, ctx, now);
        ui.toast("Doppia pagina", now);
    })?;
    let to_width = |r: &mut Reader| {
        r.act(Action::ZoomTo(Zoom::Width), now);
    };
    studio.shot("barra-zoom", Look::bar(), &to_width, &|ui, ctx| {
        // un clic sulla percentuale: i livelli di zoom
        let s = ctx.scale;
        let x = ctx.view.0 - 10.0 * s - 36.0 * s * 3.0 - 2.0 * s - 17.0 * s - 32.0 * s;
        ui.click(x, 20.0, ctx);
        ui.key(Key::Down, ctx);
        ui.key(Key::Down, ctx);
    })?;
    studio.shot("lettura-doppia", Look::bare(), &double, &|ui, _| {
        ui.poke(now);
        ui.toast("Doppia pagina", now);
    })?;
    let strip = |r: &mut Reader| {
        r.act(Action::ToggleStrip, now);
    };
    studio.shot("lettura-nastro", Look::bar(), &strip, &|ui, _| ui.poke(now))?;
    // qualche segno: sulla pagina a schermo e piu' avanti
    let mark = |r: &mut Reader| {
        let (here, n) = (r.here(), r.pages());
        for p in [n / 3, here, n * 3 / 4] {
            r.act(Action::GoTo(p), now);
            r.act(Action::ToggleBookmark, now);
        }
        r.act(Action::GoTo(here), now);
    };
    studio.shot("segnalibro", Look::bare(), &mark, &|ui, _| {
        ui.poke(now);
        ui.toast("Pagina segnata", now);
    })?;
    studio.shot("vai-a-pagina", Look::bar(), &mark, &|ui, ctx| {
        let Some(b) = &ctx.book else { return };
        ui.ask_page(b.here, b.pages, b.bookmarks);
        for d in (b.pages * 7 / 10).max(1).to_string().bytes() {
            ui.key(Key::Digit(d - b'0'), ctx);
        }
    })?;
    studio.shot("menu", Look::bar(), &mark, &|ui, ctx| {
        ui.open_menu(ctx.view.0 * 0.56, ctx.view.1 * 0.16, ctx);
        for _ in 0..3 {
            ui.key(Key::Down, ctx);
        }
    })?;
    settings_shots(studio, now, &unchanged)?;
    // la lente su un terzo della pagina, a 2,5x
    let lens = Look { lens: Some((0.47, 0.42)), ..Look::bar() };
    studio.shot("lente", lens, &unchanged, &|_, _| {})
}

/// Le impostazioni: ogni sezione, una regolazione sotto il mouse, un tasto
/// da cambiare.
fn settings_shots(studio: &mut Studio, now: Instant, unchanged: &dyn Fn(&mut Reader)) -> Result<(), String> {
    // con la luminosita' e il contrasto alzati: le pagine a sinistra si
    // vedono gia' regolate
    let bright = Settings { brightness: 20, contrast: 25, ..Settings::default() };
    studio.shot("impostazioni", Look { settings: &bright, ..Look::bar() }, unchanged, &|ui, ctx| {
        ui.toggle_prefs();
        ui.motion(ctx.view.0 - 200.0 * ctx.scale, 200.0 * ctx.scale, ctx, now);
    })?;
    studio.shot("impostazioni-tasti", Look::bar(), unchanged, &|ui, ctx| {
        ui.toggle_prefs();
        for _ in 0..3 {
            ui.key(Key::PageDown, ctx);
        }
    })?;
    // le altre sezioni: interruttori, passi, la lente, un tasto da cambiare
    let prefs = |tab| Some(fumetto::prefs::Prefs::open(tab));
    studio.shot("impostazioni-lettura", Look::bar(), unchanged, &|ui, ctx| {
        ui.prefs = prefs(fumetto::prefs::Tab::Reading);
        ui.motion(ctx.view.0 - 60.0 * ctx.scale, 300.0 * ctx.scale, ctx, now);
        ui.key(Key::Down, ctx);
    })?;
    studio.shot("impostazioni-lente", Look::bar(), unchanged, &|ui, ctx| {
        ui.prefs = prefs(fumetto::prefs::Tab::Lens);
        ui.motion(ctx.view.0 - 200.0 * ctx.scale, 190.0 * ctx.scale, ctx, now);
    })?;
    studio.shot("impostazioni-nuovo-tasto", Look::bar(), unchanged, &|ui, ctx| {
        ui.prefs = prefs(fumetto::prefs::Tab::Keys);
        for _ in 0..4 {
            ui.key(Key::Down, ctx);
        }
        ui.key(Key::Enter, ctx);
    })
}

/// La galleria vuota: nessun volume, gli ultimi letti, uno scelto.
fn gallery(studio: &mut Studio) -> Result<(), String> {
    let (size, view) = (studio.size, studio.view());
    let ctx = Context {
        view,
        scale: studio.scale,
        book: None,
        recent: &studio.recent,
        hud: true,
        fullscreen: false,
        shelf: None,
        thumbs: None,
        settings: &DEFAULTS,
        keys: Keymap::defaults(),
        lens: None,
    };
    let mut ui = Ui::new();
    ui.key(Key::Down, &ctx);
    ui.key(Key::Down, &ctx);
    let scene = ui.scene(&ctx, studio.later, &mut studio.overlay);
    // le copertine degli ultimi letti, alla misura delle righe
    let cache = studio.out.join("copertine");
    let mut covers = Vec::new();
    for slot in ui::welcome_covers(&ctx) {
        match fumetto_core::covers::cover(&slot.path, &cache) {
            Ok(p) => {
                let page = fumetto_core::resize(&p, slot.w as u32, slot.h as u32, true);
                covers.push((studio.renderer.upload(&page)?, slot));
            }
            // come nell'app: senza copertina resta il suo filo
            Err(e) => println!("  copertina di {}: {e}", slot.path.display()),
        }
    }
    let placements: Vec<Placement> =
        covers.iter().map(|(image, s)| Placement::new(image, s.x, s.y, s.w, s.h).rounded(s.radius)).collect();
    let overlay = &mut studio.overlay;
    let rgba = studio.renderer.render_to_rgba(size, &placements, |encoder, target| {
        if let Err(e) = overlay.draw(encoder, target, size, &scene) {
            eprintln!("galleria: {e}");
        }
    });
    crate::save(&studio.out.join("galleria.png"), &rgba, size.0, size.1)?;
    println!("galleria.png");
    Ok(())
}

/// Le miniature di tutte le pagine, preparate come le prepara l'app.
fn miniature(
    book: &Arc<Book>, renderer: &fumetto_render::Renderer, overlay: &mut Overlay, out: &Path, size: (u32, u32),
    scale: f32, here: usize,
) -> Result<(), String> {
    let (w, h) = size;
    let view = (w as f32, h as f32);
    let marks = [here, here + 3, here + 11];
    // la forma tipica delle pagine: dalle prime, come la conoscerebbe l'app
    let dims: Vec<(u32, u32)> = book.sample_sizes(7).into_iter().map(|s| s.1).collect();
    let ratio = if dims.is_empty() {
        1.5
    } else {
        dims.iter().map(|d| d.1 as f32 / d.0 as f32).sum::<f32>() / dims.len() as f32
    };
    fn always(_: usize) -> bool {
        true
    }
    let d = ThumbsData { title: &book.title, pages: book.len(), here, bookmarks: &marks, ratio, ready: &always };
    let mut thumbs = Thumbs::default();
    thumbs.open(here);
    let (cw, ch) = Thumbs::cell_size(&d, view, scale);
    let slots: Vec<_> = thumbs.slots(&d, view, scale).into_iter().filter(|s| s.5).collect();
    let (tx, rx) = mpsc::channel();
    let loader = Loader::new(Loader::default_threads(), move |l| {
        let _ = tx.send(l);
    });
    loader.set_book(Some(book.clone()));
    loader.set_target(Target::plain(Fit::Contain { width: cw, height: ch }));
    let order: Vec<usize> = slots.iter().map(|s| s.0).collect();
    loader.request(&order);
    let mut images = HashMap::new();
    let t = Instant::now();
    while images.len() < order.len() {
        let l = rx.recv_timeout(Duration::from_secs(30)).map_err(|_| "miniature: non arrivano".to_string())?;
        images.insert(l.index, renderer.upload(&l.page?)?);
    }
    println!("{} miniature in {:.0} ms", images.len(), t.elapsed().as_secs_f64() * 1000.0);
    let placements: Vec<Placement> = slots
        .iter()
        .filter_map(|&(i, x, y, cw, ch, _)| {
            let image = images.get(&i)?;
            let (iw, ih) = (image.width as f32, image.height as f32);
            Some(Placement::new(image, (x + (cw - iw) / 2.0).round(), (y + ch - ih).round(), iw, ih))
        })
        .collect();
    let ctx = Context {
        view,
        scale,
        book: None,
        recent: &[],
        hud: true,
        fullscreen: false,
        shelf: None,
        thumbs: Some(d),
        settings: &DEFAULTS,
        keys: Keymap::defaults(),
        lens: None,
    };
    let mut ui = Ui::new();
    ui.thumbs.open(here);
    ui.key(Key::Right, &ctx);
    let scene = ui.scene(&ctx, Instant::now() + Duration::from_millis(700), overlay);
    let rgba = renderer.render_to_rgba(size, &placements, |encoder, target| {
        if let Err(e) = overlay.draw(encoder, target, size, &scene) {
            eprintln!("miniature: {e}");
        }
    });
    crate::save(&out.join("miniature.png"), &rgba, w, h)?;
    println!("miniature.png");
    Ok(())
}

/// Gli "ultimi letti" degli scatti: i volumi accanto a quello scelto.
fn neighbours(volume: &Path) -> Vec<Recent> {
    let siblings = |of: &Path| -> Vec<std::path::PathBuf> {
        let dir = of.parent().unwrap_or(Path::new("."));
        let mut v: Vec<_> = std::fs::read_dir(dir)
            .map(|d| d.flatten().map(|e| e.path()).filter(|p| p != of).collect())
            .unwrap_or_default();
        v.sort();
        v
    };
    // da soli in una cartella: i vicini della cartella
    let mut v = siblings(volume);
    if v.len() < 3
        && let Some(parent) = volume.parent()
    {
        v.extend(siblings(parent));
    }
    v.into_iter()
        .take(5)
        .enumerate()
        .map(|(i, path)| Recent {
            title: fumetto_core::title_of(&path),
            place: if i % 3 == 2 { "letto".into() } else { format!("{} / {}", 12 + i * 31, 180 + i * 7) },
            path,
        })
        .collect()
}
