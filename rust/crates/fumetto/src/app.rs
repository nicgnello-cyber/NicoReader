//! La finestra: la galleria nera, i tasti, il disegno.
//!
//! Cosa leggere e dove metterlo lo decide il `Reader`; qui c'e' solo quello
//! che tocca il mondo fuori: la finestra, la scheda video, le pagine pronte,
//! il file dei progressi. Il filo degli eventi non decodifica e non
//! rimpicciolisce mai niente: riceve pagine gia' alla misura dello schermo e
//! le disegna. Finche' la scheda video non e' pronta le copia nella finestra
//! dal processore (`cpu_view`), e la finestra appare subito.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use fumetto::keys::{self, Bind, Combo, KeyContext, Keymap};
use fumetto::prefs::Pref;
use fumetto::shelf::ShelfData;
use fumetto::thumbs::{Thumbs, ThumbsData};
use fumetto::ui::{self, BookInfo, Command, Context, Handled, Recent, Ui};
use fumetto_core::covers::{Cover, CoverLoader};
use fumetto_core::library::{self, Entry, Status};
use fumetto_core::lingua::t;
use fumetto_core::update::Release;
use fumetto_core::upscale::{self, Job, Upscaled, Upscaler};
use fumetto_core::{Book, Content, Fit, Loaded, Loader, Page, Progress, Target};
use fumetto_core::{Saved, Settings};
use fumetto_render::{Adjust, Estimate, Gpu, GpuImage, Measure, Overlay, Pass, Placement, Renderer, Scene, wgpu};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{Fullscreen, Window, WindowId};

use crate::cpu_view::{self, CpuView};
use crate::dialog;
use crate::gpu_start::{self, GpuStart};
use crate::script::{Script, Step};
use crate::stats::Stats;
use crate::system;
use fumetto::reader::{Action, Item, Mode, Pages, Reader, webtoon_width};

/// Il contesto dell'interfaccia per un tasto o un clic: senza le pagine a
/// schermo (non servono) e, nella libreria, senza sapere delle copertine.
macro_rules! input_context {
    ($app:ident) => {
        context(
            $app.window.as_deref(),
            &$app.book,
            $app.reader.as_ref(),
            &$app.recent,
            &[],
            $app.settings.hud,
            $app.shelf_shown().then(|| shelf_data(&$app.lib, &$app.settings.library, &always, $app.book.is_some())),
            $app.thumbs_shown().then(|| thumbs_data(&$app.book, $app.reader.as_ref(), &always_ready)).flatten(),
            look!($app),
        )
    };
}

/// Cio' che l'interfaccia vede delle preferenze: impostazioni, tasti,
/// presentazione, lente.
macro_rules! look {
    ($app:ident) => {
        Look {
            settings: &$app.settings,
            keys: &$app.keymap,
            slideshow: $app.slideshow.is_some(),
            lens: $app.lens_circle(),
        }
    };
}

/// Le preferenze come le vede l'interfaccia.
struct Look<'a> {
    settings: &'a Settings,
    keys: &'a Keymap,
    slideshow: bool,
    lens: Option<(f32, f32, f32)>,
}

// App e' divisa per temi, un blocco `impl App` per file. I moduli vengono
// dopo le macro, che cosi' le vedono; le funzioni che servono a piu' d'uno si
// importano qui, e da qui (use super::*) le prendono tutti
mod actions;
mod draw;
mod events;
mod shelf;
mod tools;
mod update;
mod volume;

use actions::{combo_of, place, ui_key};
use draw::{always_ready, context, thumbs_data};
use shelf::{always, shelf_data};
use tools::save_page;

/// Quante miniature si tengono sulla scheda video: a 150x225 pesano 135 KB l'una.
const THUMBS_KEPT: usize = 600;

/// Due clic entro questo tempo, vicini, sono un doppio clic.
const DOUBLE_CLICK: Duration = Duration::from_millis(450);

/// Memoria per le pagine pronte (sulla scheda video o, prima che arrivi, in
/// RAM). Alla misura dello schermo una pagina pesa 3 MB: ce ne stanno centinaia.
const BUDGET: usize = 768 << 20;

/// Ogni quanto, al massimo, si riscrive il file dei progressi.
const SAVE_EVERY: Duration = Duration::from_secs(1);

pub enum UserEvent {
    Loaded(Loaded),
    /// La scheda video e' pronta (o non lo sara' mai).
    Gpu(Box<Result<GpuStart, String>>),
    /// macOS: dei volumi aperti dal Finder (in `finder::take`).
    #[cfg(target_os = "macos")]
    Finder,
    /// La risposta della finestra per aprire un volume (`None`: annullata);
    /// `true` se era una cartella da aggiungere alla libreria.
    Chosen(Option<PathBuf>, bool),
    /// Chiuso un avviso.
    DialogClosed,
    /// Si', spostalo nel cestino.
    Confirmed(PathBuf),
    /// La scansione delle cartelle della libreria (quali, e cosa c'era).
    Scanned(Vec<PathBuf>, Vec<Entry>),
    /// Una copertina pronta.
    Cover(Cover),
    /// Una miniatura pronta.
    Thumb(Loaded),
    /// Una pagina ai suoi pixel, per la lente.
    LensPage(Loaded),
    /// Una pagina migliorata dall'AI (o non migliorabile).
    Upscaled(Upscaled),
    /// L'ultima Release su GitHub (vedi app/update.rs).
    Update(Release),
    /// Si', apri la pagina della versione nuova.
    OpenUpdate(String),
    /// Dove salvare la pagina (`None`: annullato).
    SaveTo(Option<PathBuf>),
    /// Com'e' andato un salvataggio o una copia: la riga da mostrare, o l'errore.
    Done(Result<&'static str, String>),
}

/// La libreria trovata nelle cartelle, e dove si e' arrivati in ogni volume.
#[derive(Default)]
struct Lib {
    entries: Vec<Entry>,
    status: Vec<Status>,
    read_at: Vec<u64>,
    scanning: bool,
}

struct Gfx {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    gpu: Gpu,
    renderer: Renderer,
    overlay: Overlay,
}

struct Shown {
    image: GpuImage,
    target: Target,
    /// Rifatta dall'ingranditore AI: una pagina normale non la sostituisce.
    upscaled: bool,
    /// Da sostituire (l'AI e' stata spenta): resta a schermo finche' non
    /// arriva quella nuova, ma per la lettura anticipata non c'e'.
    stale: bool,
}

/// Un'immagine da disegnare oltre alle pagine: una copertina della libreria
/// o una miniatura.
enum Extra {
    Cover(PathBuf),
    Thumb(usize),
}

/// Le pagine pronte, dovunque siano: sulla scheda video, o in RAM prima che
/// la scheda video arrivi (dopo, la RAM si svuota e resta solo l'una).
struct Prepared<'a> {
    shown: &'a HashMap<usize, Shown>,
    cpu: &'a HashMap<usize, (Page, Target)>,
}

impl Pages for Prepared<'_> {
    fn prepared_for(&self, i: usize) -> Option<Target> {
        match self.shown.get(&i) {
            Some(s) if s.stale => None,
            Some(s) => Some(s.target),
            None => self.cpu.get(&i).map(|p| p.1),
        }
    }

    /// Anche una pagina da sostituire si puo' mostrare, intanto.
    fn has(&self, i: usize) -> bool {
        self.shown.contains_key(&i) || self.cpu.contains_key(&i)
    }
}

/// Il giro di pagina che aspetta di arrivare sullo schermo, per misurarlo.
struct PendingTurn {
    since: Instant,
    missed: bool,
}

/// Il tasto sinistro premuto: un clic, o l'inizio di un trascinamento.
struct Press {
    x: f32,
    y: f32,
    dragged: bool,
}

pub struct App {
    loader: Loader,
    book: Option<Arc<Book>>,
    reader: Option<Reader>,
    generation: u64,
    progress: Progress,
    last_save: Instant,
    /// Il volume aperto appena segnato dal menu come letto o da leggere, con
    /// il punto in cui stava il lettore: vedi `keep_position`.
    marked: Option<(PathBuf, Saved)>,

    window: Option<Arc<Window>>,
    gfx: Option<Gfx>,
    proxy: EventLoopProxy<UserEvent>,
    cpu: Option<CpuView>,
    cpu_pages: HashMap<usize, (Page, Target)>,
    shown: HashMap<usize, Shown>,

    /// Inizio del fotogramma precedente, mentre si scorre.
    last_tick: Option<Instant>,
    /// Consegna del fotogramma precedente, per misurare la fluidita'.
    last_present: Option<Instant>,
    pending_turn: Option<PendingTurn>,
    /// Quando riprovare a disegnare, se la finestra era coperta.
    retry_at: Option<Instant>,
    cursor: (f32, f32),
    press: Option<Press>,
    modifiers: ModifiersState,
    title: String,

    /// Una finestra di sistema e' aperta: la nostra aspetta la risposta.
    dialog: bool,
    /// Avvisi da mostrare, uno alla volta, appena la finestra e' visibile.
    notices: VecDeque<String>,
    /// All'avvio, niente da leggere: appena si vede la finestra, si chiede
    /// cosa aprire.
    pub ask_open: bool,
    /// Il volume da riprendere a finestra aperta, se intanto non se ne e'
    /// chiesto un altro (macOS: dal Finder).
    pub resume_later: Option<PathBuf>,
    /// Una versione nuova da proporre, quando non c'e' altro da mostrare.
    update_offer: Option<Release>,
    /// Il salvataggio dei progressi e' gia' fallito: lo si dice una volta sola.
    save_failed: bool,

    /// Didascalia, avvisi, "vai a pagina", menu, galleria vuota.
    ui: Ui,
    /// Gli ultimi letti (senza quello aperto), per il menu e la galleria.
    recent: Vec<Recent>,
    settings: Settings,
    /// Dove salvarle; `None` nella prova automatica, che non le tocca.
    settings_path: Option<PathBuf>,

    /// La libreria e' stata aperta sopra il volume che si legge (Ctrl+L).
    library_open: bool,
    lib: Lib,
    /// Le schede ComicInfo.xml gia' lette, accanto alla cache delle copertine.
    info_cache: PathBuf,
    /// Le copertine sulla scheda video, alla misura delle celle.
    covers: HashMap<PathBuf, (GpuImage, (u32, u32))>,
    /// Quelle che non si riesce a fare: non si richiedono di nuovo.
    covers_failed: std::collections::HashSet<PathBuf>,
    cover_loader: CoverLoader,
    /// L'ultima richiesta al preparatore, per non ripeterla a ogni fotogramma.
    cover_wanted: Vec<(PathBuf, (u32, u32))>,

    /// Le miniature delle pagine sono a schermo (T).
    thumbs_open: bool,
    thumb_loader: Loader,
    thumb_generation: u64,
    thumb_images: HashMap<usize, (GpuImage, Target)>,
    /// L'ultima richiesta di miniature, per non ripeterla a ogni fotogramma.
    thumb_wanted: (Option<Target>, Vec<usize>),

    upscaler: Upscaler,
    /// Le pagine chieste all'ingranditore, e quelle che non ha potuto fare.
    upscale_wanted: Vec<(usize, Target)>,
    upscale_failed: HashSet<(usize, Target)>,

    /// La presentazione: quando girare la prossima pagina.
    slideshow: Option<Instant>,
    /// Dove si era al giro precedente: se non ci si muove piu', e' finita.
    slide_last: Option<(usize, f32)>,
    /// L'ultimo clic, per riconoscere il doppio clic.
    last_click: Option<(Instant, f32, f32)>,
    /// Il thread degli appunti: tiene la pagina copiata (su Linux gli appunti
    /// vivono finche' vive chi li ha riempiti).
    copier: Option<mpsc::Sender<(Arc<Book>, usize, Target)>>,
    /// La pagina da salvare, mentre si sceglie dove.
    to_save: Option<(Arc<Book>, usize)>,

    /// I tasti che valgono: quelli di serie con le scelte di chi legge.
    keymap: Keymap,
    /// Le impostazioni cambiate e non ancora scritte (trascinando una
    /// regolazione cambiano a ogni movimento: si scrivono una volta al secondo).
    settings_dirty: bool,
    settings_saved_at: Instant,
    /// La lente e' accesa (L).
    lens: bool,
    /// Il puntatore e' nascosto (dalla lente).
    cursor_hidden: bool,
    /// Le pagine sotto la lente ai loro pixel veri: la lente mostra quelli,
    /// non la pagina rimpicciolita e poi ingrandita.
    lens_loader: Loader,
    lens_generation: u64,
    lens_images: HashMap<usize, (GpuImage, Target)>,
    lens_wanted: (Option<Target>, Vec<usize>),

    pub stats: Stats,
    script: Option<Script>,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        proxy: EventLoopProxy<UserEvent>, progress: Progress, settings: Settings, settings_path: Option<PathBuf>,
        covers_dir: PathBuf, started: Instant, script: Option<Script>,
    ) -> App {
        let thumb_loader = {
            let proxy = proxy.clone();
            Loader::new(2, move |loaded| {
                let _ = proxy.send_event(UserEvent::Thumb(loaded));
            })
        };
        let upscaler = {
            let proxy = proxy.clone();
            Upscaler::new(esrgan_engine(), move |u| {
                let _ = proxy.send_event(UserEvent::Upscaled(u));
            })
        };
        let lens_loader = {
            let proxy = proxy.clone();
            Loader::new(2, move |loaded| {
                let _ = proxy.send_event(UserEvent::LensPage(loaded));
            })
        };
        let keymap = Keymap::new(&settings.keys);
        let info_cache = covers_dir.with_file_name("comicinfo.json");
        let cover_loader = {
            let proxy = proxy.clone();
            CoverLoader::new(covers_dir, move |c| {
                let _ = proxy.send_event(UserEvent::Cover(c));
            })
        };
        let deliver = {
            let proxy = proxy.clone();
            move |loaded| {
                let _ = proxy.send_event(UserEvent::Loaded(loaded));
            }
        };
        // ponytail: variabile solo per le prove, la scelta vera arriva dalle misure
        let threads =
            std::env::var("FUMETTO_THREADS").ok().and_then(|t| t.parse().ok()).unwrap_or_else(Loader::default_threads);
        let mut app = App {
            loader: Loader::new(threads, deliver),
            book: None,
            reader: None,
            generation: 0,
            progress,
            last_save: Instant::now(),
            marked: None,
            window: None,
            gfx: None,
            proxy,
            cpu: None,
            cpu_pages: HashMap::new(),
            shown: HashMap::new(),
            last_tick: None,
            last_present: None,
            pending_turn: None,
            retry_at: None,
            cursor: (0.0, 0.0),
            press: None,
            modifiers: ModifiersState::empty(),
            title: String::new(),
            dialog: false,
            notices: VecDeque::new(),
            ask_open: false,
            resume_later: None,
            update_offer: None,
            save_failed: false,
            ui: Ui::new(),
            recent: Vec::new(),
            settings,
            settings_path,
            library_open: false,
            lib: Lib::default(),
            info_cache,
            covers: HashMap::new(),
            covers_failed: Default::default(),
            cover_loader,
            cover_wanted: Vec::new(),
            thumbs_open: false,
            thumb_loader,
            thumb_generation: 0,
            thumb_images: HashMap::new(),
            thumb_wanted: (None, Vec::new()),
            upscaler,
            upscale_wanted: Vec::new(),
            upscale_failed: HashSet::new(),
            slideshow: None,
            slide_last: None,
            last_click: None,
            copier: None,
            to_save: None,
            keymap,
            settings_dirty: false,
            settings_saved_at: Instant::now(),
            lens: false,
            cursor_hidden: false,
            lens_loader,
            lens_generation: 0,
            lens_images: HashMap::new(),
            lens_wanted: (None, Vec::new()),
            stats: Stats::new(started),
            script,
        };
        app.refresh_recent();
        app.rescan();
        app.check_update();
        app
    }

    fn save_settings(&mut self) {
        if let Some(path) = &self.settings_path
            && let Err(e) = self.settings.save(path)
        {
            eprintln!("impostazioni non salvate: {e}");
        }
    }

    fn scale(&self) -> f32 {
        self.window.as_ref().map_or(1.0, |w| w.scale_factor() as f32)
    }

    /// L'altezza della barra in alto, se c'e': le pagine stanno sotto, mai
    /// coperte.
    fn top(&self) -> u32 {
        let scale = self.window.as_ref().map_or(1.0, |w| w.scale_factor() as f32);
        if self.settings.hud && self.book.is_some() { ui::hud_height(scale) as u32 } else { 0 }
    }

    /// Lo spazio per le pagine: la finestra meno la barra.
    fn page_view(&self) -> (u32, u32) {
        let (w, h) = self.view_size();
        (w, h.saturating_sub(self.top()).max(1))
    }

    fn fit_view(&mut self) {
        let (w, h) = self.page_view();
        if let Some(r) = &mut self.reader {
            r.set_view(w, h);
        }
    }

    fn view_size(&self) -> (u32, u32) {
        self.window.as_ref().map_or((1, 1), |w| {
            let s = w.inner_size();
            (s.width.max(1), s.height.max(1))
        })
    }

    fn visible(&self) -> bool {
        self.window.as_ref().and_then(|w| w.is_visible()) == Some(true)
    }

    /// Dopo ogni cambiamento: lettura anticipata, titolo, un nuovo fotogramma.
    fn changed(&mut self) {
        self.prefetch();
        self.request_upscale();
        self.update_title();
        self.remember();
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    /// Le regolazioni dell'immagine di adesso.
    fn adjust(&self) -> Adjust {
        Adjust::from_steps(self.settings.brightness, self.settings.contrast, self.settings.gamma)
    }

    fn prepared(&self) -> Prepared<'_> {
        Prepared { shown: &self.shown, cpu: &self.cpu_pages }
    }

    /// Per la prova automatica, quando qualcosa si incastra.
    pub fn describe_state(&self) -> String {
        let reader = self.reader.as_ref().map_or("nessun volume".into(), |r| r.describe(&self.prepared()));
        format!("{reader}, giro in sospeso {}", self.pending_turn.is_some())
    }

    /// Per la prova automatica: niente giri di pagina o scorrimenti in sospeso.
    pub fn settled(&self) -> bool {
        self.pending_turn.is_none() && !self.reader.as_ref().is_some_and(|r| r.gliding())
    }

    pub fn page_ready(&self) -> bool {
        self.reader.as_ref().is_some_and(|r| r.ready(&self.prepared()))
    }

    pub fn view_height(&self) -> f32 {
        self.view_size().1 as f32
    }
}

/// Il motore dell'ingrandimento: Real-ESRGAN sulla scheda video, preparato al
/// primo uso (dal thread dell'ingrandimento: chiedere la scheda e compilare i
/// programmi richiede un attimo, e chi non lo usa non lo paga).
fn esrgan_engine() -> upscale::Engine {
    let mut net: Option<Result<fumetto_render::esrgan::Esrgan, String>> = None;
    Box::new(move |page, scale, go_on| {
        let software = std::env::var_os("FUMETTO_AI_SOFTWARE").is_some();
        match net.get_or_insert_with(|| pollster::block_on(fumetto_render::esrgan::Esrgan::new(software))) {
            Ok(net) => Ok(net.upscale(page, scale, go_on)),
            Err(e) => Err(e.clone()),
        }
    })
}
