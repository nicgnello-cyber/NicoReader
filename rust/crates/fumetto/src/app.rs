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

use fumetto_core::lingua::t;
use fumetto_core::upscale::{self, Job, Upscaled, Upscaler};
use fumetto_core::{Book, Content, Fit, Loaded, Loader, Page, Progress, Target};
use fumetto::shelf::ShelfData;
use fumetto::keys::{self, Bind, Combo, KeyContext, Keymap};
use fumetto::prefs::Pref;
use fumetto::thumbs::{Thumbs, ThumbsData};
use fumetto::ui::{self, BookInfo, Command, Context, Handled, Recent, Ui};
use fumetto_core::covers::{Cover, CoverLoader};
use fumetto_core::library::{self, Entry, Status};
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
use fumetto::reader::{Action, Item, Mode, Pages, Reader, webtoon_width};
use crate::script::{Script, Step};
use crate::stats::Stats;
use crate::system;

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
    /// Si' (o no), scarica l'ingranditore.
    DownloadConfirmed(bool),
    /// L'ingranditore e' stato scaricato, o perche' no.
    Installed(Result<(), String>),
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
    /// Si sta scaricando l'ingranditore.
    installing: bool,

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
    pub fn new(proxy: EventLoopProxy<UserEvent>, progress: Progress, settings: Settings, settings_path: Option<PathBuf>,
               covers_dir: PathBuf, upscaler_dir: PathBuf, started: Instant, script: Option<Script>) -> App {
        let thumb_loader = {
            let proxy = proxy.clone();
            Loader::new(2, move |loaded| {
                let _ = proxy.send_event(UserEvent::Thumb(loaded));
            })
        };
        let upscaler = {
            let proxy = proxy.clone();
            Upscaler::new(upscaler_dir, move |u| {
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
        let threads = std::env::var("FUMETTO_THREADS").ok().and_then(|t| t.parse().ok())
            .unwrap_or_else(Loader::default_threads);
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
            installing: false,
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
        app
    }

    /// La libreria e' a schermo: aperta sopra un volume, o nessun volume e
    /// delle cartelle da mostrare.
    fn shelf_shown(&self) -> bool {
        self.library_open || (self.book.is_none() && !self.settings.library.is_empty())
    }

    /// Le miniature sono a schermo: aperte, su un volume, fuori dalla libreria.
    fn thumbs_shown(&self) -> bool {
        self.thumbs_open && self.reader.is_some() && !self.shelf_shown()
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

    /// Cerca i volumi nelle cartelle della libreria, in sottofondo.
    fn rescan(&mut self) {
        if self.settings.library.is_empty() {
            self.lib = Lib::default();
            return;
        }
        self.lib.scanning = true;
        let roots = self.settings.library.clone();
        let proxy = self.proxy.clone();
        let cache = self.info_cache.clone();
        std::thread::Builder::new()
            .name("libreria".into())
            .spawn(move || {
                let found = library::scan_cached(&roots, &cache);
                let _ = proxy.send_event(UserEvent::Scanned(roots, found));
            })
            .expect("thread della libreria");
    }

    /// Dove si e' arrivati in ogni volume della libreria.
    fn refresh_shelf(&mut self) {
        let progress = &self.progress;
        self.lib.status = self.lib.entries.iter().map(|e| Status::of(progress, &e.path)).collect();
        self.lib.read_at = self.lib.entries.iter().map(|e| progress.get(&e.path).map_or(0, |s| s.read_at)).collect();
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

    /// Gli ultimi letti, dal piu' recente: quelli che ci sono ancora, e non
    /// quello aperto.
    fn refresh_recent(&mut self) {
        let open = self.book.as_ref().map(|b| std::path::absolute(&b.path).unwrap_or_else(|_| b.path.clone()));
        self.recent = self
            .progress
            .recent()
            .into_iter()
            .filter(|(path, _)| Some(path) != open.as_ref() && path.exists())
            .take(6)
            .map(|(path, saved)| Recent {
                title: fumetto_core::title_of(&path),
                place: place(saved),
                path,
            })
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
    fn close_book(&mut self) {
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
    fn ask(&mut self, folder: bool) {
        let Some(window) = &self.window else { return };
        if self.dialog {
            return;
        }
        let near = self.book.as_ref().and_then(|b| std::path::absolute(&b.path).ok());
        dialog::open(window, folder, near.as_deref().and_then(Path::parent), false, self.proxy.clone());
        self.dialog = true;
    }

    /// La finestra di sistema per scegliere una cartella per la libreria.
    fn ask_library_folder(&mut self) {
        let Some(window) = &self.window else { return };
        if self.dialog {
            return;
        }
        dialog::open(window, true, None, true, self.proxy.clone());
        self.dialog = true;
    }

    /// Un avviso per chi legge: in una finestra di sistema, e sulla console.
    fn notify(&mut self, text: String) {
        eprintln!("{text}");
        self.notices.push_back(text);
    }

    /// Mostra cio' che aspetta, prima gli avvisi e poi la richiesta di cosa
    /// aprire: solo a finestra visibile, e un dialogo alla volta.
    fn show_pending(&mut self) {
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
    fn remember(&mut self) {
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

    fn prefetch(&mut self) {
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

    /// Chiede all'ingranditore le pagine a schermo che ne guadagnano: quelle
    /// mostrate piu' grandi dei loro pixel, non ancora migliorate.
    fn request_upscale(&mut self) {
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
            let jobs = wanted.iter().map(|&(index, target)| Job { book: book.clone(), generation: self.generation, index, target });
            self.upscaler.request(jobs.collect());
            self.upscale_wanted = wanted;
        }
    }

    /// Accende o spegne l'ingranditore; la prima volta, chiede di scaricarlo.
    fn toggle_upscale(&mut self) {
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
        if self.upscaler.installed() {
            return self.upscale_ready();
        }
        if self.installing {
            return self.ui.toast(t("Sto scaricando l'ingranditore\u{2026}", "Downloading the enhancer\u{2026}"), now);
        }
        if let Some(window) = &self.window
            && !self.dialog
        {
            dialog::confirm_download(window, &self.upscaler.dir(), self.proxy.clone());
            self.dialog = true;
        }
    }

    fn upscale_ready(&mut self) {
        self.settings.upscale = true;
        self.save_settings();
        self.upscale_failed.clear();
        self.ui.toast(t("Scansioni migliorate con l'AI", "Scans enhanced with AI"), Instant::now());
        self.changed();
    }

    /// Apre o chiude le miniature di tutte le pagine.
    fn toggle_thumbs(&mut self) {
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
    fn toggle_slideshow(&mut self) {
        let now = Instant::now();
        if self.slideshow.take().is_some() {
            self.ui.toast(t("Presentazione ferma", "Slideshow stopped"), now);
        } else if self.reader.is_some() {
            let secs = self.settings.slideshow.max(1);
            self.slideshow = Some(now + Duration::from_secs(secs as u64));
            self.slide_last = None;
            let text = if fumetto_core::lingua::italian() { format!("Presentazione: una pagina ogni {secs} s") }
                       else { format!("Slideshow: a page every {secs} s") };
            self.ui.toast(text, now);
        }
        self.update_title();
        self.request_redraw();
    }

    /// La presentazione: e' ora di girare pagina?
    fn slideshow_tick(&mut self, now: Instant, event_loop: &ActiveEventLoop) -> Option<Instant> {
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
        let here = { let s = reader.snapshot(); (s.page, s.strip_offset) };
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
    fn ask_save(&mut self) {
        let (Some(book), Some(reader), Some(window)) = (&self.book, &self.reader, &self.window) else { return };
        if self.dialog {
            return;
        }
        let index = reader.here();
        let ext = Path::new(&book.names[index]).extension().map(|e| e.to_string_lossy().to_lowercase())
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
    fn copy_page(&mut self) {
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
    fn lens_circle(&self) -> Option<(f32, f32, f32)> {
        let on = self.lens && self.reader.is_some() && !self.shelf_shown() && !self.thumbs_shown() && !self.ui.modal();
        on.then(|| (self.cursor.0, self.cursor.1, (self.settings.lens_size * self.scale()).round()))
    }

    fn toggle_lens(&mut self) {
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
    fn lens_items(&mut self, items: &[Item]) -> Option<((f32, f32, f32), Vec<Item>)> {
        let (cx, cy, r) = self.lens_circle()?;
        let m = self.settings.lens_zoom;
        let under: Vec<Item> = items
            .iter()
            .filter(|it| it.x < cx + r && it.x + it.w > cx - r && it.y < cy + r && it.y + it.h > cy - r)
            .map(|it| Item { page: it.page, x: cx + (it.x - cx) * m, y: cy + (it.y - cy) * m, w: it.w * m, h: it.h * m })
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

    /// Le regolazioni dell'immagine di adesso.
    fn adjust(&self) -> Adjust {
        Adjust::from_steps(self.settings.brightness, self.settings.contrast, self.settings.gamma)
    }

    /// Applica una scelta fatta nelle impostazioni.
    fn apply_pref(&mut self, p: Pref, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let s = &mut self.settings;
        match p {
            Pref::Brightness(v) => s.brightness = v,
            Pref::Contrast(v) => s.contrast = v,
            Pref::Gamma(v) => s.gamma = v,
            Pref::ResetImage => (s.brightness, s.contrast, s.gamma) = (0, 0, 0),
            Pref::LensZoom(v) => s.lens_zoom = v,
            Pref::LensSize(v) => s.lens_size = v,
            Pref::Slideshow(v) => {
                s.slideshow = v;
                if self.slideshow.is_some() {
                    self.slideshow = Some(now + Duration::from_secs(v as u64));
                }
            }
            Pref::Webtoon(on) => {
                s.webtoon = on;
                self.save_settings();
            }
            // queste passano per le loro azioni: fanno anche il resto
            // (rifare le pagine, spostarle sotto la barra, scaricare l'AI)
            Pref::Trim(on) => match &self.reader {
                Some(r) if r.trim != on => self.act(Action::ToggleTrim, event_loop),
                Some(_) => {}
                None => {
                    s.trim = on;
                    self.save_settings();
                }
            },
            Pref::Hud(on) if on != s.hud => self.act(Action::ToggleHud, event_loop),
            Pref::Hud(_) => {}
            Pref::Upscale(on) if on != s.upscale => self.toggle_upscale(),
            Pref::Upscale(_) => {}
            Pref::Bind(bind, combo) => {
                let taken = keys::assign(&mut s.keys, bind, &combo);
                self.keymap = Keymap::new(&self.settings.keys);
                self.save_settings();
                let text = match taken {
                    Some(from) if fumetto_core::lingua::italian() => {
                        format!("{} ora fa \u{ab}{}\u{bb}, non più \u{ab}{}\u{bb}", combo.shown(), bind.label(), from.label())
                    }
                    Some(from) => format!("{} now does \u{201c}{}\u{201d}, no longer \u{201c}{}\u{201d}", combo.shown(),
                                          bind.label(), from.label()),
                    None => format!("{}: {}", bind.label(), combo.shown()),
                };
                self.ui.toast(text, now);
            }
            Pref::Unbind(bind) => {
                keys::clear(&mut s.keys, bind);
                self.keymap = Keymap::new(&self.settings.keys);
                self.save_settings();
                self.ui.toast(format!("{}: {}", bind.label(), t("nessun tasto", "no key")), now);
            }
            Pref::ResetKeys => {
                s.keys.clear();
                self.keymap = Keymap::new(&self.settings.keys);
                self.save_settings();
                self.ui.toast(t("Tutti i tasti di serie", "All default keys"), now);
            }
        }
        // le regolazioni trascinate cambiano a ogni movimento: si scrivono dopo
        self.settings_dirty = true;
        self.request_redraw();
    }

    fn update_title(&mut self) {
        let Some(w) = &self.window else { return };
        let title = match (&self.book, &self.reader) {
            _ if self.shelf_shown() => format!("NicoReader \u{2014} {}", t("Libreria", "Library")),
            (Some(book), Some(reader)) => reader.title(&book.title),
            _ => "NicoReader".to_owned(),
        };
        if title != self.title {
            w.set_title(&title);
            self.title = title;
        }
    }

    pub fn act(&mut self, action: Action, event_loop: &ActiveEventLoop) {
        // aprire, chiudere, la libreria, vai a pagina: le miniature si chiudono
        if matches!(action, Action::Open | Action::OpenFolder | Action::Close | Action::ToggleLibrary | Action::AskPage) {
            self.thumbs_open = false;
        }
        match action {
            Action::Quit => return event_loop.exit(),
            Action::ToggleThumbs => return self.toggle_thumbs(),
            Action::ToggleSettings => {
                self.ui.toggle_prefs();
                return self.request_redraw();
            }
            Action::ToggleLens => return self.toggle_lens(),
            Action::ToggleSlideshow => return self.toggle_slideshow(),
            Action::ToggleUpscale => return self.toggle_upscale(),
            Action::ToggleWebtoon => {
                self.settings.webtoon = !self.settings.webtoon;
                self.save_settings();
                let note = if self.settings.webtoon {
                    t("I webtoon si leggeranno a nastro", "Webtoons will open as a strip")
                } else {
                    t("Webtoon: nessun riconoscimento", "Webtoons: no detection")
                };
                self.ui.toast(note, Instant::now());
                return self.request_redraw();
            }
            Action::SavePage => return self.ask_save(),
            Action::CopyPage => return self.copy_page(),
            Action::ToggleFullscreen => {
                if let Some(w) = &self.window {
                    w.set_fullscreen(match w.fullscreen() {
                        Some(_) => None,
                        None => Some(Fullscreen::Borderless(None)),
                    });
                }
                return;
            }
            Action::Open | Action::OpenFolder => return self.ask(action == Action::OpenFolder),
            Action::Close => return self.close_book(),
            Action::ToggleHud => {
                self.settings.hud = !self.settings.hud;
                if let Some(path) = &self.settings_path
                    && let Err(e) = self.settings.save(path)
                {
                    eprintln!("impostazioni non salvate: {e}");
                }
                self.fit_view();
                let note = if self.settings.hud {
                    t("Barra in alto", "Top bar")
                } else {
                    t("Barra nascosta: H per riaverla", "Bar hidden: H brings it back")
                };
                self.ui.toast(note, Instant::now());
                return self.changed();
            }
            Action::ToggleLibrary => {
                // senza un volume aperto la libreria e' gia' li' (se ci sono cartelle)
                if self.book.is_some() {
                    self.library_open = !self.library_open;
                }
                if self.shelf_shown() {
                    self.refresh_shelf();
                    self.rescan();
                } else if self.book.is_none() {
                    self.ask_library_folder();
                }
                self.ui.reset();
                return self.changed();
            }
            Action::AddLibraryFolder => return self.ask_library_folder(),
            Action::AskPage => {
                if let Some(r) = &self.reader {
                    self.ui.ask_page(r.here(), r.pages(), r.bookmarks());
                }
                return self.request_redraw();
            }
            _ => {}
        }
        let Some(reader) = &mut self.reader else { return };
        let now = Instant::now();
        if reader.act(action, now) {
            self.pending_turn = Some(PendingTurn { since: now, missed: false });
        }
        if action == Action::ToggleTrim {
            self.settings.trim = reader.trim;
        }
        // chi gira pagina a mano ha tutto il tempo della presentazione per la nuova
        if self.slideshow.is_some()
            && matches!(action, Action::Next | Action::Prev | Action::First | Action::Last | Action::GoTo(_) | Action::Scroll(_))
        {
            self.slideshow = Some(now + Duration::from_secs(self.settings.slideshow.max(1) as u64));
        }
        let reader = self.reader.as_ref().expect("appena usato");
        // cio' che cambia il modo lo dice una riga: a schermo intero il
        // titolo della finestra non si vede
        if let Some(note) = note(action, reader) {
            self.ui.toast(note, now);
        }
        if action == Action::ToggleTrim {
            self.save_settings();
        }
        self.changed();
    }

    /// Esegue cio' che l'interfaccia ha deciso.
    fn run(&mut self, cmd: Command, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        match cmd {
            Command::Act(a) => self.act(a, event_loop),
            Command::Open(path) => self.open(&path),
            Command::MarkRead(path, read) => {
                // per segnarlo letto serve sapere quante pagine ha
                let pages = self.progress.get(&path).map(|s| s.pages).filter(|&n| n > 0)
                    .or_else(|| Book::open(&path).ok().map(|b| b.len()));
                match pages {
                    Some(n) => {
                        self.progress.mark(&path, read, n);
                        // il volume aperto: da qui il lettore non lo riscrive, finche' non ci si muove
                        let same = |a: &Path| std::path::absolute(a).ok() == std::path::absolute(&path).ok();
                        if let (Some(book), Some(reader)) = (&self.book, &self.reader)
                            && same(&book.path)
                        {
                            self.marked = Some((book.path.clone(), reader.snapshot()));
                        }
                        self.refresh_shelf();
                        self.refresh_recent();
                        self.ui.toast(if read { t("Segnato come letto", "Marked as read") } else { t("Di nuovo da leggere", "Unread again") }, now);
                    }
                    None => self.notify(t("Questo volume non si apre: non so quante pagine abbia.",
                                          "This volume doesn't open: its page count is unknown.").to_owned()),
                }
            }
            Command::Reveal(path) => {
                if let Err(e) = system::reveal(&path) {
                    eprintln!("mostra nella cartella: {e}");
                }
            }
            Command::Trash(path) => {
                if let Some(window) = &self.window
                    && !self.dialog
                {
                    dialog::confirm_trash(window, path, self.proxy.clone());
                    self.dialog = true;
                }
            }
            Command::RemoveFolder(root) => {
                self.settings.library.retain(|r| *r != root);
                self.save_settings();
                self.rescan();
                self.ui.toast(t("Cartella tolta dalla libreria", "Folder removed from the library"), now);
                self.changed();
            }
            Command::Pref(p) => self.apply_pref(p, event_loop),
            Command::Page(p) => {
                self.thumbs_open = false;
                self.act(Action::GoTo(p), event_loop);
            }
            // quelli interni li ha gia' fatti l'interfaccia
            Command::Series(_) | Command::FoldersMenu(..) => {}
        }
        self.request_redraw();
    }

    fn request_redraw(&self) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn arrived(&mut self, loaded: Loaded) {
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
        while total > BUDGET && let Some(far) = far_from(&mut self.shown.keys()) {
            total -= self.shown.remove(&far).map_or(0, |s| s.image.bytes());
        }
        let mut total: usize = self.cpu_pages.values().map(|p| p.0.bytes()).sum();
        while total > BUDGET && let Some(far) = far_from(&mut self.cpu_pages.keys()) {
            total -= self.cpu_pages.remove(&far).map_or(0, |p| p.0.bytes());
        }
    }

    fn redraw(&mut self) {
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
            let ctx = context(self.window.as_deref(), &self.book, None, &self.recent, &[], self.settings.hud, None, None,
                              look!(self));
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
        let ctx = context(self.window.as_deref(), &self.book, self.reader.as_ref(), &self.recent, &items, self.settings.hud,
                          None, None, look!(self));
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
        let ctx = context(self.window.as_deref(), &self.book, self.reader.as_ref(), &self.recent, &[], self.settings.hud,
                          Some(d), None, look!(self));
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
        let wanted: Vec<usize> = slots.iter().map(|s| s.0).filter(|i| images.get(i).is_none_or(|t| t.1 != target)).collect();
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
        let ctx = context(self.window.as_deref(), &self.book, self.reader.as_ref(), &self.recent, &[], self.settings.hud,
                          None, Some(d), look!(self));
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
    fn present_with(&mut self, items: &[Item], covers: &[(Extra, f32, f32, f32, f32)],
                    lens: Option<((f32, f32, f32), Vec<Item>)>, scene: &Scene) -> Option<Instant> {
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

    fn present_gpu(&mut self, items: &[Item], covers: &[(Extra, f32, f32, f32, f32)],
                   lens: Option<((f32, f32, f32), Vec<Item>)>, scene: &Scene) -> Option<Instant> {
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
            gfx.renderer.draw(&mut encoder, &view, gfx.config.format, size, &lens_placements,
                              Pass { clear: false, adjust, clip: Some(circle) });
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
                self.cpu_pages.get(&it.page).map(|(page, _)| cpu_view::Item { page, x: it.x, y: it.y, w: it.w, h: it.h })
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
    fn gpu_arrived(&mut self, start: GpuStart) {
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

    /// Il comando di un tasto premuto, dalla mappa dei tasti, e la sua azione adesso.
    fn key(&self, event: &winit::event::KeyEvent) -> Option<Action> {
        let bind = self.keymap.lookup(&combo_of(event, self.modifiers)?)?;
        // con la lente accesa, Esc la spegne
        if bind == Bind::Quit && self.lens {
            return Some(Action::ToggleLens);
        }
        let reader = self.reader.as_ref();
        let k = KeyContext {
            manga: reader.is_some_and(|r| r.manga),
            zoomed: reader.is_some_and(|r| r.zoomed()),
            fullscreen: self.window.as_ref().and_then(|w| w.fullscreen()).is_some(),
            view: (self.view_size().0 as f32, self.view_size().1 as f32),
        };
        Some(keys::action(bind, &k))
    }

    /// Un clic dove non gira pagina (il terzo centrale, il nastro, la pagina
    /// ingrandita): se e' il secondo di un doppio clic, schermo intero.
    fn double_click(&mut self, x: f32, y: f32) -> Option<Action> {
        let now = Instant::now();
        let double = self.last_click.take().is_some_and(|(t, px, py)| {
            now - t <= DOUBLE_CLICK && (x - px).abs() + (y - py).abs() < 10.0 * self.scale()
        });
        if double {
            return Some(Action::ToggleFullscreen);
        }
        self.last_click = Some((now, x, y));
        None
    }

    fn click(&self, x: f32) -> Option<Action> {
        let reader = self.reader.as_ref()?;
        if reader.mode == Mode::Strip || reader.zoomed() {
            return None;
        }
        let third = self.view_size().0 as f32 / 3.0;
        let (right, left) = if reader.manga { (Action::Prev, Action::Next) } else { (Action::Next, Action::Prev) };
        match x {
            x if x < third => Some(left),
            x if x > 2.0 * third => Some(right),
            _ => None,
        }
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

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("NicoReader")
            .with_maximized(true)
            .with_visible(false);
        // su Linux il nome con cui il desktop trova nicoreader.desktop, e con lui
        // icona e nome: su Wayland senza non c'e' (su X11 sarebbe il nome
        // dell'eseguibile, qui uguale)
        #[cfg(all(unix, not(target_os = "macos")))]
        let attrs = winit::platform::wayland::WindowAttributesExtWayland::with_name(attrs, "nicoreader", "nicoreader");
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => crate::fatal(&format!("{}\n\n{e}", t("Impossibile creare la finestra.", "Can't create the window."))),
        };
        #[cfg(windows)]
        set_icons(&window);
        if let Some(mhz) = window.current_monitor().and_then(|m| m.refresh_rate_millihertz()) {
            self.stats.refresh_ms = 1_000_000.0 / mhz as f32;
        }
        if self.script.is_some() {
            // lanciata da un altro programma, Windows la aprirebbe dietro la
            // finestra attiva, e una finestra coperta viene disegnata al rallentatore
            window.set_window_level(winit::window::WindowLevel::AlwaysOnTop);
            window.focus_window();
        }
        self.window = Some(window.clone());
        self.stats.window_ms = Some(self.stats.now_ms());
        match CpuView::new(window) {
            Ok(cpu) => self.cpu = Some(cpu),
            Err(e) => eprintln!("copia dal processore non disponibile: {e}"),
        }
        self.update_title();
        // solo adesso la scheda video: caricare i suoi driver blocca il
        // caricamento di ogni altra libreria (loader lock di Windows), e partendo
        // per prima rallentava l'apertura della finestra. La prima pagina non la
        // aspetta: la mostra il processore.
        gpu_start::spawn(self.proxy.clone());
        // una finestra invisibile non riceve mai la richiesta di ridisegno: il
        // primo fotogramma (nero) si disegna subito, la finestra appare con
        // quello, prende la sua misura vera, e parte la lettura anticipata
        self.redraw();
    }

    fn user_event(&mut self, _: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Loaded(loaded) => self.arrived(loaded),
            UserEvent::Gpu(start) => match *start {
                Ok(start) => self.gpu_arrived(start),
                Err(e) => eprintln!("scheda video: {e}; si continua copiando dal processore"),
            },
            UserEvent::Chosen(path, to_library) => {
                self.dialog = false;
                match (path, to_library) {
                    (Some(path), true) => {
                        if !self.settings.library.contains(&path) {
                            self.settings.library.push(path);
                            self.save_settings();
                        }
                        self.rescan();
                        self.changed();
                    }
                    (Some(path), false) => self.open(&path),
                    (None, _) => {}
                }
            }
            UserEvent::DialogClosed => self.dialog = false,
            UserEvent::Confirmed(path) => {
                self.dialog = false;
                match system::trash(&path) {
                    Ok(()) => {
                        self.ui.toast(t("Spostato nel cestino", "Moved to the trash"), Instant::now());
                        self.rescan();
                    }
                    Err(e) => self.notify(format!("{}\n\n{e}", t("Impossibile spostarlo nel cestino.", "Can't move it to the trash."))),
                }
            }
            UserEvent::Scanned(roots, entries) => {
                // una scansione di cartelle che nel frattempo sono cambiate non vale
                if roots == self.settings.library {
                    self.lib.entries = entries;
                    self.lib.scanning = false;
                    self.refresh_shelf();
                    self.changed();
                }
            }
            UserEvent::Cover(c) => match (&self.gfx, c.page) {
                (Some(gfx), Ok(page)) => {
                    if let Ok(image) = gfx.renderer.upload(&page) {
                        self.covers.insert(c.path, (image, c.size));
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
            },
            UserEvent::Thumb(loaded) => {
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
            UserEvent::LensPage(loaded) => {
                if loaded.generation != self.lens_generation {
                    return;
                }
                match (&self.gfx, loaded.page) {
                    (Some(gfx), Ok(page)) => match gfx.renderer.upload(&page) {
                        Ok(image) => {
                            self.lens_images.insert(loaded.index, (image, loaded.target));
                            // ai loro pixel pesano: se ne tengono poche, le piu' vicine
                            let here = self.reader.as_ref().map_or(0, |r| r.here());
                            while self.lens_images.len() > 4 {
                                let far = *self.lens_images.keys().max_by_key(|i| i.abs_diff(here)).expect("non vuota");
                                self.lens_images.remove(&far);
                            }
                            self.request_redraw();
                        }
                        Err(e) => eprintln!("lente, pagina {}: {e}", loaded.index + 1),
                    },
                    (None, Ok(_)) => self.lens_wanted = (None, Vec::new()),
                    (_, Err(e)) => eprintln!("lente, pagina {}: {e}", loaded.index + 1),
                }
            }
            UserEvent::Upscaled(u) => {
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
            UserEvent::DownloadConfirmed(yes) => {
                self.dialog = false;
                if yes {
                    self.installing = true;
                    let dir = self.upscaler.dir();
                    let proxy = self.proxy.clone();
                    std::thread::Builder::new()
                        .name("scarica-ingranditore".into())
                        .spawn(move || {
                            let _ = proxy.send_event(UserEvent::Installed(upscale::install(&dir)));
                        })
                        .expect("thread dello scaricamento");
                    self.ui.toast(t("Scarico l'ingranditore\u{2026}", "Downloading the enhancer\u{2026}"), Instant::now());
                    self.request_redraw();
                }
            }
            UserEvent::Installed(result) => {
                self.installing = false;
                match result {
                    Ok(()) => self.upscale_ready(),
                    Err(e) => self.notify(format!("{}\n\n{e}",
                        t("Non sono riuscito a scaricare l'ingranditore.", "Couldn't download the enhancer."))),
                }
            }
            UserEvent::SaveTo(path) => {
                self.dialog = false;
                if let (Some(path), Some((book, index))) = (path, self.to_save.take()) {
                    let proxy = self.proxy.clone();
                    std::thread::Builder::new()
                        .name("salva-pagina".into())
                        .spawn(move || {
                            let done = save_page(&book, index, &path).map(|_| t("Pagina salvata", "Page saved"));
                            let _ = proxy.send_event(UserEvent::Done(done));
                        })
                        .expect("thread del salvataggio");
                }
            }
            UserEvent::Done(result) => match result {
                Ok(text) => {
                    self.ui.toast(text, Instant::now());
                    self.request_redraw();
                }
                Err(e) => self.notify(e),
            },
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gfx) = &mut self.gfx {
                    gfx.config.width = size.width.max(1);
                    gfx.config.height = size.height.max(1);
                    gfx.surface.configure(&gfx.gpu.device, &gfx.config);
                }
                self.fit_view();
                self.changed();
            }
            WindowEvent::RedrawRequested => self.redraw(),
            #[cfg(windows)]
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(w) = &self.window {
                    set_icons(w);
                }
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                // le impostazioni aspettano un tasto nuovo: questo e' per loro
                if self.ui.capturing().is_some() {
                    let Some(combo) = combo_of(&event, self.modifiers) else { return }; // un modificatore da solo
                    let plain = !combo.ctrl && !combo.alt && !combo.shift;
                    let cmd = match combo.key.as_str() {
                        "Escape" if plain => self.ui.captured(None, false),
                        "Backspace" | "Delete" if plain => self.ui.captured(None, true),
                        _ => self.ui.captured(Some(combo), false),
                    };
                    self.request_redraw();
                    if let Some(cmd) = cmd {
                        self.run(cmd, event_loop);
                    }
                    return;
                }
                // prima l'interfaccia: con un menu aperto o nella galleria
                // vuota le frecce e Invio sono sue
                let command = if cfg!(target_os = "macos") { self.modifiers.super_key() } else { self.modifiers.control_key() };
                if !command && (self.ui.modal() || self.reader.is_none() || self.shelf_shown() || self.thumbs_shown()) {
                    let ctx = input_context!(self);
                    let handled = ui_key(&event.logical_key).map_or(Handled::No, |k| self.ui.key(k, &ctx));
                    match handled {
                        Handled::Yes(cmd) => {
                            self.request_redraw();
                            if let Some(cmd) = cmd {
                                self.run(cmd, event_loop);
                            }
                            return;
                        }
                        // con un menu aperto, nella libreria o fra le miniature, nessun
                        // altro tasto arriva al lettore
                        Handled::No if self.ui.modal() || self.shelf_shown() || self.thumbs_shown() => return,
                        Handled::No => {}
                    }
                }
                if event.logical_key == Key::Named(NamedKey::ContextMenu) {
                    let (w, h) = self.view_size();
                    let ctx = input_context!(self);
                    self.ui.open_menu(w as f32 / 2.0, h as f32 / 3.0, &ctx);
                    return self.request_redraw();
                }
                if let Some(a) = self.key(&event) {
                    self.act(a, event_loop);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (lines, pixels) = match delta {
                    MouseScrollDelta::LineDelta(_, y) => (y, -y * self.view_size().1 as f32 * 0.15),
                    MouseScrollDelta::PixelDelta(p) => (p.y as f32 / 120.0, -p.y as f32),
                };
                let ctx = input_context!(self);
                if self.ui.wheel(pixels, &ctx) {
                    return self.request_redraw();
                }
                if self.ui.modal() {
                    // "vai a pagina": la rotella sfoglia il numero; nel menu, le voci
                    self.ui.key(if lines > 0.0 { ui::Key::Up } else { ui::Key::Down }, &ctx);
                    return self.request_redraw();
                }
                if self.lens_circle().is_some() && !self.modifiers.control_key() {
                    let zoom = (self.settings.lens_zoom * 1.12f32.powf(lines)).clamp(1.5, 6.0);
                    self.settings.lens_zoom = (zoom * 4.0).round() / 4.0;
                    self.settings_dirty = true;
                    let text = format!("{} {}\u{00d7}", t("Lente", "Magnifier"), self.settings.lens_zoom);
                    let text = if fumetto_core::lingua::italian() { text.replace('.', ",") } else { text };
                    self.ui.toast(text, Instant::now());
                    return self.request_redraw();
                }
                let zoomed = self.reader.as_ref().is_some_and(|r| r.zoomed());
                let action = if self.modifiers.control_key() {
                    Action::Zoom { factor: 1.2f32.powf(lines), x: self.cursor.0, y: self.cursor.1 - self.top() as f32 }
                } else if zoomed {
                    Action::Pan(0.0, -pixels)
                } else {
                    Action::Scroll(pixels)
                };
                self.act(action, event_loop);
            }
            WindowEvent::CursorMoved { position, .. } => {
                let (x, y) = (position.x as f32, position.y as f32);
                let (dx, dy) = (x - self.cursor.0, y - self.cursor.1);
                self.cursor = (x, y);
                let ctx = input_context!(self);
                if self.ui.motion(x, y, &ctx, Instant::now()) || self.lens_circle().is_some() {
                    self.request_redraw();
                }
                if self.press.is_some() {
                    let ctx = input_context!(self);
                    if let Some(cmd) = self.ui.drag(x, &ctx) {
                        if let Some(p) = &mut self.press {
                            p.dragged = true;
                        }
                        return self.run(cmd, event_loop);
                    }
                }
                if let Some(press) = &mut self.press {
                    // oltre qualche pixel non e' piu' un clic: si trascina la pagina
                    press.dragged |= (x - press.x).abs() + (y - press.y).abs() > 6.0;
                    if press.dragged && !self.ui.modal() && self.reader.as_ref().is_some_and(|r| r.zoomed()) {
                        self.act(Action::Pan(dx, dy), event_loop);
                    }
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => match state {
                ElementState::Pressed => {
                    self.press = Some(Press { x: self.cursor.0, y: self.cursor.1, dragged: false });
                    let ctx = input_context!(self);
                    if let Some(cmd) = self.ui.press(self.cursor.0, self.cursor.1, &ctx) {
                        self.run(cmd, event_loop);
                    }
                }
                ElementState::Released => {
                    let press = self.press.take();
                    if press.as_ref().is_some_and(|p| p.dragged) && self.ui.prefs.is_some() {
                        // fine del trascinamento di una regolazione
                        let ctx = input_context!(self);
                        let _ = self.ui.click(self.cursor.0, self.cursor.1, &ctx);
                    }
                    if let Some(press) = press
                        && !press.dragged
                    {
                        let ctx = input_context!(self);
                        match self.ui.click(press.x, press.y, &ctx) {
                            Handled::Yes(cmd) => {
                                self.request_redraw();
                                if let Some(cmd) = cmd {
                                    self.run(cmd, event_loop);
                                }
                            }
                            Handled::No => {
                                let action = self.click(press.x).or_else(|| self.double_click(press.x, press.y));
                                if let Some(a) = action {
                                    self.act(a, event_loop);
                                }
                            }
                        }
                    }
                }
            },
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Right, .. } => {
                let ctx = input_context!(self);
                self.ui.open_menu(self.cursor.0, self.cursor.1, &ctx);
                self.request_redraw();
            }
            WindowEvent::DroppedFile(path) => self.open(&path),
            WindowEvent::Focused(on) => self.stats.note(if on { "finestra in primo piano" } else { "finestra sullo sfondo" }),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if self.progress.is_dirty() && now - self.last_save >= SAVE_EVERY {
            self.save_progress();
        }
        if self.settings_dirty && now - self.settings_saved_at >= SAVE_EVERY {
            self.settings_dirty = false;
            self.settings_saved_at = now;
            self.save_settings();
        }
        self.show_pending();
        let mut wake: Option<Instant> = None;
        if let Some(at) = self.retry_at {
            if now >= at {
                self.retry_at = None;
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            } else {
                wake = Some(at);
            }
        }
        // c'e' qualcosa da salvare: ci si risveglia per farlo anche se non
        // succede nient'altro
        if self.progress.is_dirty() {
            let at = self.last_save + SAVE_EVERY;
            wake = Some(wake.map_or(at, |w| w.min(at)));
        }
        if let Some(at) = self.slideshow_tick(now, event_loop) {
            wake = Some(wake.map_or(at, |w| w.min(at)));
        }
        // la didascalia che compare o svanisce vuole i suoi fotogrammi
        if let Some(at) = self.ui.wake(now) {
            if at <= now {
                self.request_redraw();
            } else {
                wake = Some(wake.map_or(at, |w| w.min(at)));
            }
        }
        if let Some(mut script) = self.script.take() {
            match script.step(self) {
                Step::Do(a) => self.act(a, event_loop),
                Step::Wait => {}
                Step::Done => return self.act(Action::Quit, event_loop),
            }
            self.script = Some(script);
            let next = now + Duration::from_millis(4);
            wake = Some(wake.map_or(next, |w| w.min(next)));
        }
        event_loop.set_control_flow(wake.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }
}

/// Cio' che l'interfaccia deve sapere, dai pezzi dell'app che servono (e
/// non da tutta l'app, cosi' l'interfaccia si puo' cambiare intanto).
#[allow(clippy::too_many_arguments)]
fn context<'a>(window: Option<&Window>, book: &'a Option<Arc<Book>>, reader: Option<&'a Reader>, recent: &'a [Recent],
               items: &[Item], hud: bool, shelf: Option<ShelfData<'a>>, thumbs: Option<ThumbsData<'a>>,
               look: Look<'a>) -> Context<'a> {
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
    Context { view: (vw, vh), scale, book, recent, hud, fullscreen, shelf, thumbs, settings: look.settings, keys: look.keys,
              lens: look.lens }
}

/// Le preferenze come le vede l'interfaccia.
struct Look<'a> {
    settings: &'a Settings,
    keys: &'a Keymap,
    slideshow: bool,
    lens: Option<(f32, f32, f32)>,
}

/// Le miniature come le vede l'interfaccia.
fn thumbs_data<'a>(book: &'a Option<Arc<Book>>, reader: Option<&'a Reader>, ready: &'a dyn Fn(usize) -> bool)
                   -> Option<ThumbsData<'a>> {
    let (book, r) = (book.as_ref()?, reader?);
    Some(ThumbsData { title: &book.title, pages: r.pages(), here: r.here(), bookmarks: r.bookmarks(),
                      ratio: r.typical_ratio(), ready })
}

/// Per i tasti e i clic le miniature pronte non contano.
fn always_ready(_: usize) -> bool {
    true
}

/// Salva la pagina `index` in `path`. Se l'estensione scelta e' quella della
/// pagina, i suoi byte cosi' come sono nell'archivio (nessuna perdita); se no
/// (o se viene da un PDF) i suoi pixel, nel formato che dice l'estensione.
fn save_page(book: &Book, index: usize, path: &Path) -> Result<(), String> {
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
        let done = done.map(|_| t("Pagina copiata", "Page copied"))
            .map_err(|e| format!("{}\n\n{e}", t("Impossibile copiare la pagina.", "Can't copy the page.")));
        let _ = proxy.send_event(UserEvent::Done(done));
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

/// La libreria come la vede l'interfaccia.
fn shelf_data<'a>(lib: &'a Lib, roots: &'a [PathBuf], covered: &'a dyn Fn(&Path) -> bool, reading: bool) -> ShelfData<'a> {
    ShelfData {
        entries: &lib.entries,
        status: &lib.status,
        read_at: &lib.read_at,
        scanning: lib.scanning,
        roots,
        covered,
        reading,
    }
}

/// Per i tasti e i clic le copertine non contano.
fn always(_: &Path) -> bool {
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

/// Dove si era arrivati in un volume: "24 / 212", o "letto".
fn place(s: &Saved) -> String {
    match s.pages {
        0 => String::new(),
        n if s.page + 1 >= n => t("letto", "read").to_owned(),
        n => format!("{} / {n}", s.page + 1),
    }
}

/// La riga che dice cosa ha cambiato un tasto.
fn note(action: Action, r: &Reader) -> Option<String> {
    let pick = |on: bool, yes: &str, no: &str| (if on { yes } else { no }).to_owned();
    Some(match action {
        Action::ToggleDouble => pick(r.mode == Mode::Double, t("Doppia pagina", "Two pages"), t("Pagina singola", "Single page")),
        Action::ToggleStrip => pick(r.mode == Mode::Strip, t("Nastro", "Strip"), t("Pagina singola", "Single page")),
        Action::ToggleManga => pick(r.manga, t("Da destra a sinistra", "Right to left"), t("Da sinistra a destra", "Left to right")),
        Action::ToggleCover if r.mode == Mode::Double => {
            pick(r.cover_alone(), t("Copertina da sola", "Cover alone"), t("Copertina in coppia", "Cover paired"))
        }
        Action::ToggleLinear => pick(r.linear, t("Luce lineare", "Linear light"), t("Gamma, per confronto", "Gamma, for comparison")),
        Action::Rotate(true) => t("Girata a destra", "Rotated right").to_owned(),
        Action::Rotate(false) => t("Girata a sinistra", "Rotated left").to_owned(),
        Action::ToggleTrim if r.mode == Mode::Strip => {
            pick(r.trim, t("Margini rifilati (non nel nastro)", "Margins trimmed (not in the strip)"), t("Pagine intere", "Whole pages"))
        }
        Action::ToggleTrim => pick(r.trim, t("Margini rifilati", "Margins trimmed"), t("Pagine intere", "Whole pages")),
        Action::ToggleBookmark => pick(r.bookmarked(), t("Pagina segnata", "Page bookmarked"), t("Segno tolto", "Bookmark removed")),
        Action::Zoom { .. } | Action::ZoomIn | Action::ZoomOut | Action::ZoomTo(_) | Action::ZoomReset
        | Action::StripWider(_) => format!("Zoom {}%", r.zoom_percent()?),
        _ => return None,
    })
}

/// Il tasto premuto come lo conosce la mappa dei tasti; `None` per i
/// modificatori da soli e i tasti senza nome. Con Ctrl o Alt conta il tasto
/// senza modificatori (Ctrl+Maiusc+O e' "O", non un carattere di controllo);
/// senza, il carattere scritto ("+" anche se sulla tastiera e' Maiusc+=).
fn combo_of(event: &winit::event::KeyEvent, m: ModifiersState) -> Option<Combo> {
    let ctrl = if cfg!(target_os = "macos") { m.super_key() } else { m.control_key() };
    let alt = m.alt_key();
    let key = if ctrl || alt { event.key_without_modifiers() } else { event.logical_key.clone() };
    let name = match &key {
        Key::Character(c) => c.to_string(),
        Key::Named(n) => match n {
            NamedKey::ArrowRight => "Right".into(),
            NamedKey::ArrowLeft => "Left".into(),
            NamedKey::ArrowUp => "Up".into(),
            NamedKey::ArrowDown => "Down".into(),
            NamedKey::Space => "Space".into(),
            NamedKey::PageUp | NamedKey::PageDown | NamedKey::Home | NamedKey::End | NamedKey::Escape
            | NamedKey::Backspace | NamedKey::Enter | NamedKey::Delete | NamedKey::Insert | NamedKey::Tab => format!("{n:?}"),
            // F1..F24: il nome e' gia' quello
            other => {
                let name = format!("{other:?}");
                let f_key = name.strip_prefix('F').is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()));
                if !f_key {
                    return None;
                }
                name
            }
        },
        _ => return None,
    };
    Combo::new(&name, ctrl, m.shift_key(), alt)
}

/// I tasti che l'interfaccia capisce.
fn ui_key(key: &Key) -> Option<ui::Key> {
    Some(match key {
        Key::Named(NamedKey::ArrowUp) => ui::Key::Up,
        Key::Named(NamedKey::ArrowDown) => ui::Key::Down,
        Key::Named(NamedKey::PageUp) => ui::Key::PageUp,
        Key::Named(NamedKey::PageDown) => ui::Key::PageDown,
        Key::Named(NamedKey::Home) => ui::Key::Home,
        Key::Named(NamedKey::End) => ui::Key::End,
        Key::Named(NamedKey::Enter) => ui::Key::Enter,
        Key::Named(NamedKey::Escape) => ui::Key::Escape,
        Key::Named(NamedKey::Backspace) => ui::Key::Backspace,
        Key::Named(NamedKey::ArrowLeft) => ui::Key::Left,
        Key::Named(NamedKey::ArrowRight) => ui::Key::Right,
        Key::Named(NamedKey::Space) => ui::Key::Char(' '),
        // le cifre valgono per "vai a pagina"; le lettere per cercare nella libreria
        Key::Character(c) => {
            let ch = c.chars().next()?;
            match ch.to_digit(10) {
                Some(d) => ui::Key::Digit(d as u8),
                None => ui::Key::Char(ch),
            }
        }
        _ => return None,
    })
}

/// L'icona dell'eseguibile anche sulla finestra: piccola nella barra del
/// titolo, grande per Alt+Tab e la barra delle applicazioni, ciascuna alla
/// misura della scala dello schermo (Windows la sceglie tra quelle nel file).
#[cfg(windows)]
fn set_icons(window: &Window) {
    use winit::dpi::PhysicalSize;
    use winit::platform::windows::{IconExtWindows, WindowExtWindows};
    use winit::window::Icon;
    let icon = |side: f64| {
        let side = (side * window.scale_factor()).round() as u32;
        Icon::from_resource(1, Some(PhysicalSize::new(side, side))).ok()
    };
    window.set_window_icon(icon(16.0));
    window.set_taskbar_icon(icon(32.0));
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
