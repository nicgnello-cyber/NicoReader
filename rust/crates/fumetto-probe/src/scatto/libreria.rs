//! Gli scatti della libreria: le viste, i filtri, gli ordini, i tasti della
//! barra, i menu, i moduli. Servono anche a confrontare prima e dopo un
//! cambiamento che non deve cambiare niente a schermo.

use std::path::PathBuf;

use fumetto::shelf::{CoverSlot, Filter, ShelfData};
use fumetto_core::LibrarySort;
use fumetto_core::library::{self, Entry, Status};
use fumetto_render::Renderer;

use super::*;

pub fn libreria(
    root: &Path, out: &Path, size: (u32, u32), scale: f32, power: wgpu::PowerPreference,
) -> Result<(), String> {
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let (gpu, renderer) = crate::gpu(power)?;
    let overlay = Overlay::new(&gpu, OFFSCREEN_FORMAT);
    let t = Instant::now();
    let lib = Library::scan(root);
    println!("{} volumi trovati in {:.0} ms", lib.entries.len(), t.elapsed().as_secs_f64() * 1000.0);
    let mut s = Shelves { lib, renderer: &renderer, overlay, covers: HashMap::new(), out, size, scale };
    let t = Instant::now();
    s.shot("libreria", &|_, _| {})?;
    println!("copertine fatte in {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
    view_shots(&mut s, root)?;
    bar_shots(&mut s)?;
    form_shots(&mut s, root)?;
    let first_series = s.lib.entries.iter().find_map(|e| e.series.clone());
    s.shot("libreria-serie-in-lettura", &|ui, _| {
        if let Some(series) = first_series.clone() {
            ui.shelf.enter_series(series);
            ui.shelf.filter = Filter::Reading;
        }
    })
}

/// La libreria trovata in una cartella, con stati finti ma credibili:
/// qualcuno in lettura, qualcuno letto.
struct Library {
    entries: Vec<Entry>,
    status: Vec<Status>,
    read_at: Vec<u64>,
    roots: Vec<PathBuf>,
}

impl Library {
    fn scan(root: &Path) -> Library {
        let roots = vec![root.to_owned()];
        let entries = library::scan(&roots);
        let status = (0..entries.len())
            .map(|i| match i % 5 {
                1 => Status::Reading((20 + i * 7) % 170, 180),
                3 => Status::Done,
                _ => Status::New,
            })
            .collect();
        let read_at = (0..entries.len()).map(|i| i as u64 * 10).collect();
        Library { entries, status, read_at, roots }
    }

    /// Come la vede la libreria; `cover_shown`: le copertine gia' pronte.
    fn data<'a>(&'a self, cover_shown: &'a dyn Fn(&Path) -> f32) -> ShelfData<'a> {
        ShelfData {
            entries: &self.entries,
            status: &self.status,
            read_at: &self.read_at,
            offline: &[],
            edited: &[],
            found_from: usize::MAX,
            searching: false,
            scanning: false,
            roots: &self.roots,
            cover_shown,
            reading: false,
        }
    }
}

/// Cio' che serve agli scatti della libreria: i volumi, la scheda video, le
/// copertine gia' caricate, la finestra (finta) e dove salvare.
struct Shelves<'a> {
    lib: Library,
    renderer: &'a Renderer,
    overlay: Overlay,
    covers: HashMap<PathBuf, GpuImage>,
    out: &'a Path,
    size: (u32, u32),
    scale: f32,
}

impl Shelves<'_> {
    fn view(&self) -> (f32, f32) {
        (self.size.0 as f32, self.size.1 as f32)
    }

    fn context<'c>(&self, shelf: ShelfData<'c>) -> Context<'c> {
        Context {
            view: self.view(),
            scale: self.scale,
            book: None,
            recent: &[],
            hud: true,
            fullscreen: false,
            shelf: Some(shelf),
            thumbs: None,
            settings: &DEFAULTS,
            keys: Keymap::defaults(),
            lens: None,
        }
    }

    /// Uno scatto: `dress` porta l'interfaccia dove serve; le copertine a
    /// schermo si preparano (una volta sola) prima di disegnare.
    fn shot(&mut self, name: &str, dress: &dyn Fn(&mut Ui, &Context)) -> Result<(), String> {
        let (view, scale, size) = (self.view(), self.scale, self.size);
        let mut ui = Ui::new();
        let slots = {
            let shelf = self.lib.data(&|_| 1.0);
            let ctx = self.context(shelf);
            dress(&mut ui, &ctx);
            ui.shelf.cover_slots(ctx.shelf.as_ref().expect("la libreria"), view, scale)
        };
        self.upload_covers(&slots)?;
        let covers = &self.covers;
        let cover_shown = |p: &Path| if covers.contains_key(p) { 1.0 } else { 0.0 };
        let ctx = self.context(self.lib.data(&cover_shown));
        // cio' che si anima (la cella sotto il mouse che sale) parte qui, e
        // nello scatto, 700 ms dopo, e' arrivato
        let now = Instant::now();
        ui.scene(&ctx, now, &mut self.overlay);
        let scene = ui.scene(&ctx, now + Duration::from_millis(700), &mut self.overlay);
        let slots = ui.shelf.cover_slots(ctx.shelf.as_ref().expect("la libreria"), view, scale);
        let placements: Vec<Placement> = slots
            .iter()
            .filter(|s| s.on_screen)
            .filter_map(|s| {
                covers.get(&s.path).map(|image| Placement::new(image, s.x, s.y, s.w, s.h).rounded(s.radius))
            })
            .collect();
        let overlay = &mut self.overlay;
        let rgba = self.renderer.render_to_rgba(size, &placements, |encoder, target| {
            if let Err(e) = overlay.draw(encoder, target, size, &scene) {
                eprintln!("{name}: {e}");
            }
        });
        crate::save(&self.out.join(format!("{name}.png")), &rgba, size.0, size.1)?;
        println!("{name}.png");
        Ok(())
    }

    /// Le copertine a schermo che non sono ancora sulla scheda video.
    fn upload_covers(&mut self, slots: &[CoverSlot]) -> Result<(), String> {
        let cache = self.out.join("cache");
        for slot in slots.iter().filter(|s| s.on_screen) {
            if self.covers.contains_key(&slot.path) {
                continue;
            }
            let page = fumetto_core::covers::cover(&slot.path, &cache)
                .map(|p| fumetto_core::resize(&p, slot.w.round() as u32, slot.h.round() as u32, true));
            match page {
                Ok(p) => {
                    self.covers.insert(slot.path.clone(), self.renderer.upload(&p)?);
                }
                Err(e) => println!("  copertina di {}: {e}", slot.path.display()),
            }
        }
        Ok(())
    }
}

fn type_in(ui: &mut Ui, ctx: &Context, text: &str) {
    for c in text.chars() {
        ui.key(Key::Char(c), ctx);
    }
}

/// Le viste: una serie, la ricerca, il menu, i filtri, nessun risultato, le
/// cartelle e dentro una, gli ordini, la scelta con le frecce.
fn view_shots(s: &mut Shelves, root: &Path) -> Result<(), String> {
    if let Some(series) = s.lib.entries.iter().find_map(|e| e.series.clone()) {
        s.shot("libreria-serie", &|ui, _| ui.shelf.enter_series(series.clone()))?;
    }
    s.shot("libreria-cerca", &|ui, ctx| type_in(ui, ctx, "god"))?;
    s.shot("libreria-menu", &|ui, ctx| ui.open_menu(ctx.view.0 * 0.3, ctx.scale * 200.0, ctx))?;
    s.shot("libreria-in-lettura", &|ui, _| ui.shelf.filter = Filter::Reading)?;
    s.shot("libreria-letti", &|ui, _| ui.shelf.filter = Filter::Done)?;
    s.shot("libreria-nessuno", &|ui, ctx| type_in(ui, ctx, "zzqx"))?;
    s.shot("libreria-cartelle", &|ui, _| ui.shelf.folders = true)?;
    let inner = s.lib.entries.iter().filter_map(|e| e.path.parent()).find(|p| *p != root).map(Path::to_path_buf);
    if let Some(dir) = inner {
        s.shot("libreria-dentro-cartella", &|ui, _| ui.shelf.enter_folder(dir.clone()))?;
    }
    for (name, sort) in [
        ("libreria-per-anno", LibrarySort::Year),
        ("libreria-aggiunti", LibrarySort::Added),
        ("libreria-letti-di-recente", LibrarySort::Read),
    ] {
        s.shot(name, &|ui, _| ui.shelf.set_sort(sort))?;
    }
    s.shot("libreria-frecce", &|ui, ctx| {
        ui.key(Key::Right, ctx);
        ui.key(Key::Down, ctx);
        ui.key(Key::Right, ctx);
    })
}

/// Il mouse sopra una cella e sopra i tasti della barra (cartelle, per
/// cartelle, ordine), il menu dell'ordine e quello di una cella.
fn bar_shots(s: &mut Shelves) -> Result<(), String> {
    let (k, vw) = (s.scale, s.view().0);
    let now = Instant::now();
    s.shot("libreria-sopra-una-cella", &|ui, ctx| {
        ui.motion(100.0 * k, 300.0 * k, ctx, now);
    })?;
    for (i, name) in
        ["libreria-tasto-cartelle", "libreria-tasto-vista", "libreria-tasto-ordine"].into_iter().enumerate()
    {
        let x = vw - 14.0 * k - 40.0 * k * (i + 1) as f32 + 18.0 * k;
        s.shot(name, &|ui, ctx| {
            ui.motion(x, 26.0 * k, ctx, now);
        })?;
    }
    s.shot("libreria-menu-ordine", &|ui, ctx| {
        let _ = ui.click(vw - 14.0 * k - 120.0 * k + 18.0 * k, 26.0 * k, ctx);
    })?;
    s.shot("libreria-menu-cella", &|ui, ctx| ui.open_menu(100.0 * k, 300.0 * k, ctx))
}

/// I moduli: un server (dal menu, come lo apre chi legge), un errore, la
/// password di un PDF, i dati di un volume.
fn form_shots(s: &mut Shelves, root: &Path) -> Result<(), String> {
    let server_form = |ui: &mut Ui, ctx: &Context| {
        ui.open_menu(ctx.view.0 * 0.6, ctx.view.1 * 0.8, ctx);
        for _ in 0..4 {
            ui.key(Key::Down, ctx);
        }
        ui.key(Key::Enter, ctx);
    };
    s.shot("modulo-server", &|ui, ctx| {
        server_form(ui, ctx);
        type_in(ui, ctx, "casa:25600");
        ui.key(Key::Tab, ctx);
        type_in(ui, ctx, "nico");
        ui.key(Key::Tab, ctx);
        type_in(ui, ctx, "segreta");
    })?;
    s.shot("modulo-errore", &|ui, ctx| {
        server_form(ui, ctx);
        ui.key(Key::Enter, ctx);
    })?;
    s.shot("modulo-pdf", &|ui, ctx| {
        ui.ask_password(root.join("Segreto.pdf"), "Segreto".into());
        type_in(ui, ctx, "abc");
    })?;
    s.shot("modulo-volume", &|ui, ctx| {
        let initial = ["Il primo".to_owned(), "Nebbia sul Porto".to_owned(), "1".to_owned()];
        let edit = fumetto_core::edits::VolumeEdit::default();
        ui.edit_volume(root.join("v1.cbz"), "v1.cbz".into(), initial, edit, Some(true));
        for _ in 0..3 {
            ui.key(Key::Tab, ctx);
        }
    })
}
