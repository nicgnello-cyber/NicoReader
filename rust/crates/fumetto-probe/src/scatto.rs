//! Scatti dell'interfaccia vera, fuori schermo: le pagine di un volume vero,
//! preparate come le prepara l'app, e sopra la didascalia, "vai a pagina", il
//! menu, la galleria vuota. Per guardare il risultato senza aprire finestre
//! (e senza disturbare chi sta usando il computer).

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use fumetto::keys::Keymap;
use fumetto::reader::{Action, Pages, Reader, Zoom};
use fumetto::thumbs::{Thumbs, ThumbsData};
use fumetto::ui::{self, BookInfo, Context, Key, Recent, Ui};
use fumetto_core::{Book, Fit, Loader, Settings, Target, decode_page, to_screen};
use fumetto_render::{Adjust, GpuImage, OFFSCREEN_FORMAT, Overlay, Pass, Placement, wgpu};

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
    let mut overlay = Overlay::new(&gpu, OFFSCREEN_FORMAT);
    println!("caratteri e interfaccia pronti in {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
    let book = Arc::new(Book::open(volume).map_err(|e| e.to_string())?);
    let (tx, rx) = mpsc::channel();
    let loader = Loader::new(Loader::default_threads(), move |l| {
        let _ = tx.send(l);
    });
    loader.set_book(Some(book.clone()));
    let recent = neighbours(volume);
    let (w, h) = size;
    let view = (w as f32, h as f32);
    let later = Instant::now() + Duration::from_millis(700); // a didascalie gia' comparse
    let n = book.len();
    let start = (n / 8).min(n.saturating_sub(1));

    // `mode` prepara il lettore (decide quali pagine servono), `dress`
    // l'interfaccia sopra
    // `hud`: con la barra in alto (le pagine stanno sotto)
    // `st`: le impostazioni (regolazioni dell'immagine, lente); `lens`: dove
    // sta la lente, come frazione della finestra
    let mut shot = |name: &str,
                    hud: bool,
                    st: &Settings,
                    lens: Option<(f32, f32)>,
                    mode: &dyn Fn(&mut Reader),
                    dress: &dyn Fn(&mut Ui, &Context)|
     -> Result<(), String> {
        let top = if hud { ui::hud_height(scale) } else { 0.0 };
        let mut reader = Reader::new(n, start);
        reader.set_view(w, h - top as u32);
        mode(&mut reader);
        let mut ready = Ready(HashMap::new());
        let deadline = Instant::now() + Duration::from_secs(20);
        while !reader.ready(&ready) {
            let (target, order) = reader.prefetch(&ready);
            loader.set_target(target);
            loader.request(&order);
            let l = rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| format!("{name}: le pagine non arrivano"))?;
            let page = l.page?;
            reader.known(l.index, l.native);
            ready.0.insert(l.index, (renderer.upload(&page)?, l.target));
        }
        let mut items = reader.layout(&ready);
        for it in &mut items {
            it.y += top;
        }
        let placements: Vec<Placement> = items
            .iter()
            .filter_map(|it| {
                ready.0.get(&it.page).map(|(image, _)| Placement { image, x: it.x, y: it.y, w: it.w, h: it.h })
            })
            .collect();
        let left = items.iter().map(|i| i.x).fold(f32::INFINITY, f32::min).max(0.0);
        let right = (view.0 - items.iter().map(|i| i.x + i.w).fold(0.0, f32::max)).max(0.0);
        let ctx = Context {
            view,
            scale,
            book: Some(BookInfo {
                title: &book.title,
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
            }),
            recent: &recent,
            hud,
            fullscreen: false,
            shelf: None,
            thumbs: None,
            settings: st,
            keys: Keymap::defaults(),
            lens: lens.map(|(fx, fy)| (fx * view.0, fy * view.1, (st.lens_size * scale).round())),
        };
        // la lente: le pagine sotto di lei, ai loro pixel, ingrandite attorno al centro
        let mut native = Vec::new();
        if let Some((cx, cy, r)) = ctx.lens {
            let m = st.lens_zoom;
            for it in
                items.iter().filter(|it| it.x < cx + r && it.x + it.w > cx - r && it.y < cy + r && it.y + it.h > cy - r)
            {
                let target = Target { fit: Fit::Contain { width: 8192, height: 1 << 16 }, ..reader.target() };
                let page = to_screen(decode_page(&book, it.page, target)?, target);
                native.push((renderer.upload(&page)?, cx + (it.x - cx) * m, cy + (it.y - cy) * m, it.w * m, it.h * m));
            }
        }
        let lens_placements: Vec<Placement> =
            native.iter().map(|(image, x, y, w, h)| Placement { image, x: *x, y: *y, w: *w, h: *h }).collect();
        let adjust = Adjust::from_steps(st.brightness, st.contrast, st.gamma);
        let mut ui = Ui::new();
        dress(&mut ui, &ctx);
        let scene = ui.scene(&ctx, later, &mut overlay);
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
        crate::save(&out.join(format!("{name}.png")), &rgba, w, h)?;
        println!("{name}.png");
        Ok(())
    };

    let now = Instant::now();
    shot(
        "barra-doppia",
        true,
        &DEFAULTS,
        None,
        &|r| {
            r.act(Action::ToggleDouble, now);
        },
        &|ui, ctx| {
            // il mouse sopra "doppia pagina": si vede il suo nome
            let x = ctx.view.0 - 350.0 * ctx.scale;
            ui.motion(x, 20.0, ctx, now);
            ui.toast("Doppia pagina", now);
        },
    )?;
    shot(
        "barra-zoom",
        true,
        &DEFAULTS,
        None,
        &|r| {
            r.act(Action::ZoomTo(Zoom::Width), now);
        },
        &|ui, ctx| {
            // un clic sulla percentuale: i livelli di zoom
            let x = ctx.view.0
                - 10.0 * ctx.scale
                - 36.0 * ctx.scale * 3.0
                - 2.0 * ctx.scale
                - 17.0 * ctx.scale
                - 32.0 * ctx.scale;
            ui.click(x, 20.0, ctx);
            ui.key(Key::Down, ctx);
            ui.key(Key::Down, ctx);
        },
    )?;
    shot(
        "lettura-doppia",
        false,
        &DEFAULTS,
        None,
        &|r| {
            r.act(Action::ToggleDouble, now);
        },
        &|ui, _| {
            ui.poke(now);
            ui.toast("Doppia pagina", now);
        },
    )?;
    shot(
        "lettura-nastro",
        true,
        &DEFAULTS,
        None,
        &|r| {
            r.act(Action::ToggleStrip, now);
        },
        &|ui, _| ui.poke(now),
    )?;
    // qualche segno: sulla pagina a schermo e piu' avanti
    let mark = |r: &mut Reader| {
        let (here, n) = (r.here(), r.pages());
        for p in [n / 3, here, n * 3 / 4] {
            r.act(Action::GoTo(p), now);
            r.act(Action::ToggleBookmark, now);
        }
        r.act(Action::GoTo(here), now);
    };
    shot("segnalibro", false, &DEFAULTS, None, &mark, &|ui, _| {
        ui.poke(now);
        ui.toast("Pagina segnata", now);
    })?;
    shot("vai-a-pagina", true, &DEFAULTS, None, &mark, &|ui, ctx| {
        let Some(b) = &ctx.book else { return };
        ui.ask_page(b.here, b.pages, b.bookmarks);
        for d in (b.pages * 7 / 10).max(1).to_string().bytes() {
            ui.key(Key::Digit(d - b'0'), ctx);
        }
    })?;
    shot("menu", true, &DEFAULTS, None, &mark, &|ui, ctx| {
        ui.open_menu(ctx.view.0 * 0.56, ctx.view.1 * 0.16, ctx);
        for _ in 0..3 {
            ui.key(Key::Down, ctx);
        }
    })?;
    // le impostazioni aperte, con la luminosita' e il contrasto alzati: le
    // pagine a sinistra si vedono gia' regolate
    let bright = Settings { brightness: 20, contrast: 25, ..Settings::default() };
    shot("impostazioni", true, &bright, None, &|_| {}, &|ui, ctx| {
        ui.toggle_prefs();
        ui.motion(ctx.view.0 - 200.0 * ctx.scale, 200.0 * ctx.scale, ctx, now);
    })?;
    shot("impostazioni-tasti", true, &DEFAULTS, None, &|_| {}, &|ui, ctx| {
        ui.toggle_prefs();
        for _ in 0..3 {
            ui.key(Key::PageDown, ctx);
        }
    })?;
    // la lente su un terzo della pagina, a 2,5x
    shot("lente", true, &DEFAULTS, Some((0.47, 0.42)), &|_| {}, &|_, _| {})?;
    miniature(&book, &renderer, &mut overlay, out, size, scale, start)?;
    // la galleria vuota: nessun volume
    let ctx = Context {
        view,
        scale,
        book: None,
        recent: &recent,
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
    let scene = ui.scene(&ctx, later, &mut overlay);
    let rgba = renderer.render_to_rgba(size, &[], |encoder, target| {
        if let Err(e) = overlay.draw(encoder, target, size, &scene) {
            eprintln!("galleria: {e}");
        }
    });
    crate::save(&out.join("galleria.png"), &rgba, w, h)?;
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
            Some(Placement { image, x: (x + (cw - iw) / 2.0).round(), y: (y + ch - ih).round(), w: iw, h: ih })
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

/// La libreria vera di una cartella: copertine, serie, ricerca, menu.
pub fn libreria(
    root: &Path, out: &Path, size: (u32, u32), scale: f32, power: wgpu::PowerPreference,
) -> Result<(), String> {
    use fumetto::shelf::{Shelf, ShelfData};
    use fumetto_core::library::{self, Status};
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let (gpu, renderer) = crate::gpu(power)?;
    let mut overlay = Overlay::new(&gpu, OFFSCREEN_FORMAT);
    let t = Instant::now();
    let roots = vec![root.to_owned()];
    let entries = library::scan(&roots);
    println!("{} volumi trovati in {:.0} ms", entries.len(), t.elapsed().as_secs_f64() * 1000.0);
    // stati finti ma credibili: qualcuno in lettura, qualcuno letto
    let status: Vec<Status> = (0..entries.len())
        .map(|i| match i % 5 {
            1 => Status::Reading((20 + i * 7) % 170, 180),
            3 => Status::Done,
            _ => Status::New,
        })
        .collect();
    let read_at: Vec<u64> = (0..entries.len()).map(|i| i as u64 * 10).collect();
    let cache = out.join("cache");
    let (w, h) = size;
    let view = (w as f32, h as f32);
    let mut covers: HashMap<std::path::PathBuf, GpuImage> = HashMap::new();
    let t = Instant::now();

    let mut shot = |name: &str, dress: &dyn Fn(&mut Ui, &Context)| -> Result<(), String> {
        let d = ShelfData {
            entries: &entries,
            status: &status,
            read_at: &read_at,
            scanning: false,
            roots: &roots,
            covered: &|_| true,
            reading: false,
        };
        let mut ui = Ui::new();
        let ctx = Context {
            view,
            scale,
            book: None,
            recent: &[],
            hud: true,
            fullscreen: false,
            shelf: Some(d),
            thumbs: None,
            settings: &DEFAULTS,
            keys: Keymap::defaults(),
            lens: None,
        };
        dress(&mut ui, &ctx);
        let slots = ui.shelf.cover_slots(ctx.shelf.as_ref().unwrap(), view, scale);
        for (path, _, _, cw, ch, _) in slots.iter().filter(|s| s.5) {
            if !covers.contains_key(path) {
                let page = fumetto_core::covers::cover(path, &cache)
                    .map(|p| fumetto_core::resize(&p, cw.round() as u32, ch.round() as u32, true));
                match page {
                    Ok(p) => {
                        covers.insert(path.clone(), renderer.upload(&p)?);
                    }
                    Err(e) => println!("  copertina di {}: {e}", path.display()),
                }
            }
        }
        let covered = |p: &Path| covers.contains_key(p);
        let d = ShelfData {
            covered: &covered,
            ..ShelfData {
                entries: &entries,
                status: &status,
                read_at: &read_at,
                scanning: false,
                roots: &roots,
                covered: &|_| true,
                reading: false,
            }
        };
        let ctx = Context { shelf: Some(d), ..ctx };
        let scene = ui.scene(&ctx, Instant::now() + Duration::from_millis(700), &mut overlay);
        let placements: Vec<Placement> = slots
            .iter()
            .filter(|s| s.5)
            .filter_map(|(p, x, y, cw, ch, _)| {
                covers.get(p).map(|image| Placement { image, x: *x, y: *y, w: *cw, h: *ch })
            })
            .collect();
        let rgba = renderer.render_to_rgba(size, &placements, |encoder, target| {
            if let Err(e) = overlay.draw(encoder, target, size, &scene) {
                eprintln!("{name}: {e}");
            }
        });
        crate::save(&out.join(format!("{name}.png")), &rgba, w, h)?;
        println!("{name}.png");
        Ok(())
    };
    shot("libreria", &|_, _| {})?;
    println!("copertine fatte in {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
    let first_series = entries.iter().find_map(|e| e.series.clone());
    if let Some(series) = first_series {
        shot("libreria-serie", &|ui, _| ui.shelf.enter_series(series.clone()))?;
    }
    shot("libreria-cerca", &|ui, ctx| {
        for c in "god".chars() {
            ui.key(Key::Char(c), ctx);
        }
    })?;
    shot("libreria-menu", &|ui, ctx| {
        let g = ctx.view;
        ui.open_menu(g.0 * 0.3, ctx.scale * 200.0, ctx);
    })?;
    let _ = Shelf::default();
    Ok(())
}
