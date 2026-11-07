//! L'interfaccia sopra la tavola, con una voce sola: la didascalia.
//!
//! Come l'etichetta accanto a un quadro: piccola, nel margine nero, e solo
//! quando serve. In alto una barra sottile con i tasti (H la nasconde, e
//! allora titolo e pagina compaiono muovendo il mouse); cambiando modo, una
//! riga dice cosa e' cambiato; Ctrl+G chiede la pagina; il tasto destro apre
//! il menu; senza volume, la galleria mostra gli ultimi letti.
//!
//! Qui non c'e' niente di grafico ne' di winit: si decide cosa mostrare e
//! dove (una [`Scene`] da disegnare) e cosa fare di tasti e clic. Cosi' si
//! prova da solo, e lo stesso codice disegna gli scatti di fumetto-probe.
//!
//! Tipografia: Instrument Serif, tondo, per numeri e titoli; Instrument Sans
//! in maiuscolo spaziato per le etichette. Avorio caldo invece del bianco,
//! che sul nero abbaglia; un solo accento, rosso lacca, per cio' che e'
//! scelto. Le icone sono tratti sottili disegnati qui, alla scala dello
//! schermo: niente immagini, niente caratteri di icone.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use fumetto_core::lingua::{italian, t};
use fumetto_render::{Align, Face, Layer, Measure, Rect, Scene, Text};

use crate::keys::{Bind, Keymap};
use crate::prefs::{Prefs, Tab};
use crate::reader::{Action, Zoom};
use crate::shelf::{Shelf, ShelfData};
use crate::thumbs::{Thumbs, ThumbsData};

// i pezzi dell'interfaccia; lo stato (Ui) resta qui
mod bar;
mod input;
mod menu;
mod motion;
mod panels;
mod server;
mod style;
#[cfg(test)]
mod tests;
mod welcome;

use bar::{Hover, hud, hud_hit};
pub(crate) use bar::{Icon, draw_icon, tooltip};
pub(crate) use menu::Row;
use menu::{Menu, main_rows};
use motion::{Motion, Part, appear, veil};
use panels::{caption, goto, lens_ring, ruler, to_u8, toast_only};
use server::{Outcome, ServerForm};
pub(crate) use style::{
    ACCENT, ACCENT_TEXT, BLACK, HAIR, INK, INK_F, MUTED, MUTED_F, PLATE, TICK, alpha, caps, fade, fit, rgb,
};
pub use welcome::welcome_covers;
use welcome::{recent_shown, welcome, welcome_cta_hit, welcome_hit};

/// Quanto resta la didascalia dopo l'ultimo movimento del mouse.
const CAPTION_HOLD: Duration = Duration::from_millis(2400);
const TOAST_HOLD: Duration = Duration::from_millis(1600);
const FADE_IN: f32 = 0.14;
const FADE_OUT: f32 = 0.5;

/// Un volume letto di recente.
#[derive(Clone, Debug)]
pub struct Recent {
    pub path: PathBuf,
    pub title: String,
    /// "24 / 212", o "letto".
    pub place: String,
}

/// Il volume aperto, come lo vede l'interfaccia.
pub struct BookInfo<'a> {
    pub title: &'a str,
    pub folio: String,
    pub pages: usize,
    pub here: usize,
    pub modes: Vec<&'static str>,
    /// Nero libero a sinistra e a destra delle pagine, in pixel.
    pub margins: (f32, f32),
    /// Lo zoom rispetto ai pixel della pagina; `None` dove non si regola.
    pub zoom: Option<u32>,
    pub double: bool,
    pub strip: bool,
    pub manga: bool,
    pub cover_alone: bool,
    /// La pagina a schermo e' segnata.
    pub bookmarked: bool,
    /// Le pagine segnate, in ordine.
    pub bookmarks: &'a [usize],
    pub trim: bool,
    pub slideshow: bool,
    /// I volumi da leggere dopo e prima di questo: percorso e titolo.
    pub next: Option<(&'a std::path::Path, &'a str)>,
    pub prev: Option<(&'a std::path::Path, &'a str)>,
}

pub struct Context<'a> {
    /// La finestra, in pixel.
    pub view: (f32, f32),
    /// Pixel per punto (1 a 96 DPI, 1,25 al 125%...).
    pub scale: f32,
    pub book: Option<BookInfo<'a>>,
    pub recent: &'a [Recent],
    /// La barra in alto e' mostrata (quando c'e' un volume).
    pub hud: bool,
    pub fullscreen: bool,
    /// La libreria, quando e' lei a schermo.
    pub shelf: Option<ShelfData<'a>>,
    /// Le miniature, quando sono loro a schermo.
    pub thumbs: Option<ThumbsData<'a>>,
    /// Le preferenze di chi legge (per il foglio delle impostazioni).
    pub settings: &'a fumetto_core::Settings,
    /// I tasti che valgono adesso: si mostrano nel menu e nella barra.
    pub keys: &'a Keymap,
    /// La lente, se e' accesa: centro e raggio, in pixel.
    pub lens: Option<(f32, f32, f32)>,
}

impl Context<'_> {
    /// La barra c'e' davvero: mostrata, con un volume aperto e a schermo.
    fn bar(&self) -> bool {
        self.hud && self.book.is_some() && self.shelf.is_none() && self.thumbs.is_none()
    }
}

/// Quanto spazio prende la barra in alto, in pixel.
pub fn hud_height(scale: f32) -> f32 {
    (HUD_H * scale).round()
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Act(Action),
    Open(PathBuf),
    /// Segna un volume come letto (`true`) o da leggere.
    MarkRead(PathBuf, bool),
    /// Mostra il volume nella cartella, con il gestore dei file del sistema.
    Reveal(PathBuf),
    /// Sposta il volume nel cestino (dopo averlo chiesto).
    Trash(PathBuf),
    /// Toglie una cartella dalla libreria (i file restano dove sono).
    RemoveFolder(PathBuf),
    /// Apre un PDF protetto con la password scritta.
    OpenWithPassword(PathBuf, String),
    /// Prova il collegamento a un server e, se risponde, lo aggiunge.
    AddServer(fumetto_core::remote::Server),
    /// Toglie un server dalla libreria (per indirizzo).
    RemoveServer(String),
    /// Scarica un volume del server, per leggerlo senza rete.
    Download(PathBuf),
    /// Toglie la copia scaricata.
    ForgetDownload(PathBuf),
    /// Il modulo per correggere i dati di un volume.
    EditVolume(PathBuf),
    /// Salva i dati corretti di un volume.
    SaveVolume(PathBuf, fumetto_core::edits::VolumeEdit),
    /// Toglie le correzioni: tornano i dati del file.
    ResetVolume(PathBuf),
    /// Dalle miniature: va alla pagina (da 0) e torna a leggere.
    Page(usize),
    /// La misura delle miniature, scelta con + e -.
    ThumbSize(f32),
    /// Una scelta fatta nelle impostazioni.
    Pref(crate::prefs::Pref),
    /// Interni all'interfaccia: aprire una serie, un gruppo o una cartella,
    /// il menu delle cartelle, il modulo per aggiungere un server.
    Series(String),
    Group(fumetto_core::library::GroupKind, String),
    Folder(PathBuf),
    FoldersMenu(f32, f32),
    /// Il menu dell'ordine della libreria, e l'ordine scelto.
    SortMenu(f32, f32),
    Sort(fumetto_core::LibrarySort),
    AskServer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    Enter,
    Escape,
    Backspace,
    Tab,
    Digit(u8),
    /// Un carattere scritto (la ricerca della libreria).
    Char(char),
}

/// Cosa ne e' stato di un tasto o di un clic.
#[derive(Debug, PartialEq)]
pub enum Handled {
    /// Non riguarda l'interfaccia: lo gestisce il lettore.
    No,
    Yes(Option<Command>),
}

/// Qualcosa che compare e poi svanisce.
struct Show {
    since: Instant,
    until: Instant,
}

impl Show {
    fn alpha(&self, now: Instant) -> f32 {
        let fin = now.saturating_duration_since(self.since).as_secs_f32() / FADE_IN;
        let fout = 1.0 - now.saturating_duration_since(self.until).as_secs_f32() / FADE_OUT;
        let a = fin.min(fout).clamp(0.0, 1.0);
        a * a * (3.0 - 2.0 * a)
    }

    fn gone(&self, now: Instant) -> bool {
        now >= self.until + Duration::from_secs_f32(FADE_OUT)
    }

    /// Quando serve il prossimo fotogramma: subito se sta sfumando.
    fn wake(&self, now: Instant) -> Option<Instant> {
        if now < self.since + Duration::from_secs_f32(FADE_IN) || (now >= self.until && !self.gone(now)) {
            Some(now)
        } else if now < self.until {
            Some(self.until)
        } else {
            None
        }
    }
}

struct GoTo {
    typed: String,
    here: usize,
    pages: usize,
    /// Le pagine segnate, per mostrarle sul righello.
    bookmarks: Vec<usize>,
}

impl GoTo {
    /// La pagina scelta, contando da 1; senza cifre, quella di adesso.
    fn page(&self) -> usize {
        self.typed.parse().unwrap_or(self.here + 1)
    }

    fn valid(&self) -> bool {
        (1..=self.pages).contains(&self.page())
    }

    fn step(&mut self, by: isize) {
        let p = (self.page() as isize + by).clamp(1, self.pages as isize);
        self.typed = p.to_string();
    }
}

/// Altezza della barra in alto, e dei suoi tasti.
const HUD_H: f32 = 52.0;
const BTN: f32 = 36.0;
/// Corpo del numero di pagina nella didascalia.
const FOLIO: f32 = 54.0;
const WELCOME_W: f32 = 540.0;
const WELCOME_ROW: f32 = 60.0;

#[derive(Default)]
pub struct Ui {
    caption: Option<Show>,
    toast: Option<(String, Show)>,
    goto: Option<GoTo>,
    menu: Option<Menu>,
    /// Il modulo per aggiungere un server, se e' aperto.
    server: Option<ServerForm>,
    pointer: (f32, f32),
    /// Nella galleria vuota: l'ultimo letto scelto con le frecce.
    selected: Option<usize>,
    /// Il tasto della barra sotto il mouse.
    hover: Option<Hover>,
    /// La libreria: filtri, ricerca, serie aperta, scorrimento.
    pub shelf: Shelf,
    /// Le miniature delle pagine: scorrimento, pagina scelta.
    pub thumbs: Thumbs,
    /// Il foglio delle impostazioni, se e' aperto.
    pub prefs: Option<Prefs>,
    /// Cio' che sta comparendo.
    motion: Motion,
}

impl Ui {
    pub fn new() -> Ui {
        Ui::default()
    }

    /// Una finestra dell'interfaccia e' aperta: tasti e clic vanno a lei.
    pub fn modal(&self) -> bool {
        self.goto.is_some() || self.menu.is_some() || self.prefs.is_some() || self.server.is_some()
    }

    /// Un modulo da scrivere e' aperto (un server, la password di un PDF):
    /// i tasti sono per lui.
    pub fn server_open(&self) -> bool {
        self.server.is_some()
    }

    /// Il modulo per correggere i dati di un volume: i campi con i valori di
    /// adesso (titolo, serie, numero), le correzioni che c'erano, il verso.
    pub fn edit_volume(
        &mut self, path: PathBuf, name: String, initial: [String; 3], existing: fumetto_core::edits::VolumeEdit,
        right_to_left: Option<bool>,
    ) {
        self.menu = None;
        self.goto = None;
        self.prefs = None;
        self.server = Some(ServerForm::volume(server::VolumeData { path, name, initial, existing, right_to_left }));
    }

    /// Chiede la password di un PDF protetto, per aprirlo.
    pub fn ask_password(&mut self, path: PathBuf, title: String) {
        self.menu = None;
        self.goto = None;
        self.prefs = None;
        self.server = Some(ServerForm::pdf(path, title));
    }

    /// Com'e' andata la password: giusta, il modulo si chiude; sbagliata, lo dice.
    pub fn password_checked(&mut self, result: Result<(), String>) {
        if self.server.as_ref().is_some_and(ServerForm::is_pdf) {
            self.server_checked(result);
        }
    }

    /// Testo incollato (Ctrl+V), nel campo scelto del modulo.
    pub fn paste(&mut self, text: &str) {
        if let Some(f) = &mut self.server {
            f.type_text(text);
        }
    }

    /// Com'e' andata la prova del collegamento: se bene il modulo si chiude,
    /// se no dice perche'.
    pub fn server_checked(&mut self, result: Result<(), String>) {
        match result {
            Ok(()) => self.server = None,
            Err(e) => {
                if let Some(f) = &mut self.server {
                    f.failed(e);
                }
            }
        }
    }

    /// Apre o chiude le impostazioni.
    pub fn toggle_prefs(&mut self) {
        self.menu = None;
        self.goto = None;
        self.prefs = match self.prefs.take() {
            Some(_) => None,
            None => Some(Prefs::open(Tab::Image)),
        };
    }

    /// Le impostazioni aspettano un tasto nuovo per un comando.
    pub fn capturing(&self) -> Option<Bind> {
        self.prefs.as_ref().and_then(Prefs::capturing)
    }

    /// Il tasto nuovo (o `None`: Esc, si lascia com'era; `unbind`: il tasto
    /// cancella, il comando resta senza).
    pub fn captured(&mut self, combo: Option<crate::keys::Combo>, unbind: bool) -> Option<Command> {
        let p = self.prefs.as_mut()?;
        if unbind { p.unbind_captured() } else { p.captured(combo) }
    }

    /// Il tasto del mouse premuto: nelle impostazioni puo' cominciare a
    /// trascinare una regolazione.
    pub fn press(&mut self, x: f32, y: f32, ctx: &Context) -> Option<Command> {
        if self.menu.is_some() || self.goto.is_some() || self.server.is_some() {
            return None;
        }
        self.prefs.as_mut()?.press(ctx.settings, ctx.keys, ctx.view, ctx.scale, x, y)
    }

    /// Il mouse si muove con il tasto premuto.
    pub fn drag(&mut self, x: f32, ctx: &Context) -> Option<Command> {
        self.prefs.as_mut()?.drag(ctx.settings, ctx.keys, ctx.view, ctx.scale, x)
    }

    /// Fa comparire la didascalia (o la tiene a schermo).
    pub fn poke(&mut self, now: Instant) {
        let since = match &self.caption {
            Some(s) if !s.gone(now) => s.since.min(now),
            _ => now,
        };
        self.caption = Some(Show { since, until: now + CAPTION_HOLD });
    }

    /// Una riga che dice cosa e' appena cambiato.
    pub fn toast(&mut self, text: impl Into<String>, now: Instant) {
        self.toast_for(text, now, TOAST_HOLD);
    }

    /// Toglie la riga (ha detto qualcosa che non vale piu').
    pub fn clear_toast(&mut self) {
        self.toast = None;
    }

    /// Come `toast`, per una riga che resta di piu' (`hold`).
    pub fn toast_for(&mut self, text: impl Into<String>, now: Instant, hold: Duration) {
        let since = match &self.toast {
            Some((_, s)) if !s.gone(now) => s.since,
            _ => now,
        };
        self.toast = Some((text.into(), Show { since, until: now + hold }));
    }

    pub fn ask_page(&mut self, here: usize, pages: usize, bookmarks: &[usize]) {
        self.menu = None;
        self.goto = Some(GoTo { typed: String::new(), here, pages, bookmarks: bookmarks.to_vec() });
    }

    /// Il menu del tasto destro: su una copertina il suo, altrimenti quello
    /// generale.
    pub fn open_menu(&mut self, x: f32, y: f32, ctx: &Context) {
        self.goto = None;
        let rows = match &ctx.shelf {
            Some(d) => match self.shelf.tile_at(d, ctx.view, ctx.scale, x, y) {
                Some(tile) => Shelf::tile_menu(d, &tile),
                None => main_rows(ctx),
            },
            None => main_rows(ctx),
        };
        self.menu = Some(Menu::at(x, y, rows, ctx));
    }

    /// I comandi che l'interfaccia fa da se'; gli altri passano all'app.
    fn internal(&mut self, cmd: Option<Command>, ctx: &Context) -> Option<Command> {
        match cmd? {
            Command::Series(name) => {
                self.shelf.enter_series(name);
                None
            }
            Command::Group(kind, name) => {
                self.shelf.enter_group(kind, name);
                None
            }
            Command::Folder(path) => {
                self.shelf.enter_folder(path);
                None
            }
            Command::SortMenu(x, bar) => {
                self.menu = Some(Menu::below(x, bar, 300.0, Shelf::sort_menu(self.shelf.sort), ctx));
                None
            }
            Command::Sort(sort) => {
                self.shelf.set_sort(sort);
                None
            }
            Command::FoldersMenu(x, bar) => {
                let d = ctx.shelf.as_ref()?;
                self.menu = Some(Menu::below(x, bar, 380.0, Shelf::folders_menu(d, &ctx.settings.servers), ctx));
                None
            }
            Command::AskServer => {
                self.menu = None;
                self.goto = None;
                self.prefs = None;
                self.server = Some(ServerForm::new());
                None
            }
            other => Some(other),
        }
    }

    /// Chiude cio' che e' aperto (cambio di volume).
    pub fn reset(&mut self) {
        self.goto = None;
        self.menu = None;
        self.selected = None;
    }

    /// Quando ridisegnare, se non succede nient'altro: `now` mentre qualcosa
    /// sfuma, il momento in cui comincera' a sfumare, `None` se e' tutto fermo.
    pub fn wake(&self, now: Instant) -> Option<Instant> {
        if self.shelf.moving() || self.thumbs.gliding() || self.motion.moving(now) {
            return Some(now);
        }
        [self.caption.as_ref(), self.toast.as_ref().map(|t| &t.1)]
            .into_iter()
            .flatten()
            .filter_map(|s| s.wake(now))
            .min()
    }

    /// Cosa disegnare sopra le pagine, adesso.
    pub fn scene(&mut self, ctx: &Context, now: Instant, m: &mut dyn Measure) -> Scene {
        if self.caption.as_ref().is_some_and(|s| s.gone(now)) {
            self.caption = None;
        }
        if self.toast.as_ref().is_some_and(|t| t.1.gone(now)) {
            self.toast = None;
        }
        let visible = [self.menu.is_some(), self.prefs.is_some(), ctx.shelf.is_some(), ctx.thumbs.is_some()];
        self.motion.frame(visible, now);
        let shown = |part| self.motion.shown(part, now);
        let s = ctx.scale;
        let mut scene = Scene::default();
        if let Some((x, y, r)) = ctx.lens {
            scene.layers.push(lens_ring(x, y, r, s));
        }
        let toast = self.toast.as_ref().map(|(text, s)| (text.as_str(), s.alpha(now)));
        // le miniature e la libreria coprono la pagina (e compaiono dal nero):
        // sopra, solo gli avvisi
        let covering = if let Some(d) = &ctx.thumbs {
            self.thumbs.advance(d, ctx.view, s, now);
            scene.layers.extend(self.thumbs.layers(d, ctx.view, s, m));
            scene.layers.extend(veil(ctx.view, shown(Part::Thumbs)));
            true
        } else if let Some(d) = &ctx.shelf {
            self.shelf.advance(d, ctx.view, s, now);
            scene.layers.extend(self.shelf.layers(d, ctx.view, s, m));
            scene.layers.extend(veil(ctx.view, shown(Part::Shelf)));
            true
        } else {
            false
        };
        if covering {
            if let Some((text, a)) = toast {
                scene.layers.push(toast_only(ctx, text, a, m));
            }
        } else if let Some(book) = &ctx.book {
            // con la barra, titolo e pagina stanno li': in basso solo gli avvisi
            let a = if ctx.bar() { 0.0 } else { self.caption.as_ref().map_or(0.0, |s| s.alpha(now)) };
            if a > 0.0 || toast.is_some() {
                scene.layers.push(caption(ctx, book, a, toast, m));
            }
            // con le impostazioni aperte la barra si ritira: il foglio
            // la taglierebbe a meta'
            if ctx.bar() && self.prefs.is_none() {
                scene.layers.push(hud(ctx, book, self.hover, m));
            }
            if let Some(g) = &self.goto {
                scene.layers.push(goto(ctx, g, m));
            }
        } else {
            scene.layers.push(welcome(ctx, self.selected, m));
        }
        if let Some(p) = &self.prefs {
            // il foglio scivola dentro da destra
            let a = shown(Part::Prefs);
            for mut l in p.layers(ctx.settings, ctx.keys, ctx.view, s, m) {
                appear(&mut l, a, 36.0 * s, 0.0);
                scene.layers.push(l);
            }
        }
        if let Some(f) = &self.server {
            scene.layers.push(f.layer(ctx, m));
        }
        if let Some(menu) = &self.menu {
            // il menu scende appena, da sotto il punto da cui si apre
            let mut l = menu.layer(ctx, m);
            appear(&mut l, shown(Part::Menu), 0.0, -6.0 * s);
            scene.layers.push(l);
        }
        scene
    }
}
