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

// i disegni dell'interfaccia, divisi per pezzo; la logica (Ui) resta qui
mod bar;
mod menu;
mod panels;
mod server;

use bar::{Hover, hud, hud_hit};
pub(crate) use bar::{Icon, draw_icon, tooltip};
use menu::{main_rows, zoom_rows};
use panels::{caption, goto, lens_ring, ruler, to_u8, toast_only, welcome};
use server::{Outcome, ServerForm};

pub(crate) const INK: [u8; 4] = [0xED, 0xE8, 0xDF, 0xFF];
pub(crate) const MUTED: [u8; 4] = [0x9D, 0x97, 0x8B, 0xFF];
pub(crate) const ACCENT_TEXT: [u8; 4] = [0xE0, 0x6A, 0x50, 0xFF];
pub(crate) const HAIR: [f32; 4] = rgb(0x3B, 0x38, 0x33);
pub(crate) const TICK: [f32; 4] = rgb(0x5F, 0x5A, 0x52);
pub(crate) const ACCENT: [f32; 4] = rgb(0xC9, 0x55, 0x3C);
pub(crate) const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// Quanto resta la didascalia dopo l'ultimo movimento del mouse.
const CAPTION_HOLD: Duration = Duration::from_millis(2400);
const TOAST_HOLD: Duration = Duration::from_millis(1600);
const FADE_IN: f32 = 0.14;
const FADE_OUT: f32 = 0.5;

pub(crate) const fn rgb(r: u8, g: u8, b: u8) -> [f32; 4] {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
}

fn fade(c: [u8; 4], a: f32) -> [u8; 4] {
    [c[0], c[1], c[2], (c[3] as f32 * a).round() as u8]
}

pub(crate) fn alpha(c: [f32; 4], a: f32) -> [f32; 4] {
    [c[0], c[1], c[2], c[3] * a]
}

/// Maiuscolo spaziato, per le etichette.
pub(crate) fn caps(text: &str, size: f32, color: [u8; 4]) -> Text {
    Text::new(text.to_uppercase(), Face::Sans, size, color).weight(500).tracking(0.16)
}

/// Il testo com'e', o tagliato con i puntini perche' stia in `room` pixel.
pub(crate) fn fit(m: &mut dyn Measure, t: Text, room: f32) -> Text {
    if m.width(&t) <= room {
        return t;
    }
    let chars: Vec<char> = t.text.chars().collect();
    let cut = |n: usize| {
        let kept: String = chars[..n].iter().collect();
        Text { text: format!("{}\u{2026}", kept.trim_end()), ..t.clone() }
    };
    // quante lettere tenere: la piu' lunga che ci sta, per bisezione
    let (mut fits, mut too_long) = (0, chars.len());
    while too_long - fits > 1 {
        let mid = (fits + too_long) / 2;
        if m.width(&cut(mid)) <= room { fits = mid } else { too_long = mid }
    }
    cut(fits)
}

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

/// Un menu aperto: dove, le sue righe, quale e' scelta.
struct Menu {
    x: f32,
    y: f32,
    selected: Option<usize>,
    rows: Vec<Row>,
    width: f32,
}

#[derive(Clone, Debug)]
pub(crate) enum Row {
    Item {
        label: &'static str,
        key: String,
        on: bool,
        cmd: Command,
    },
    Recent {
        title: String,
        place: String,
        cmd: Command,
    },
    /// Una cartella o un server della libreria: come si mostra, e un clic
    /// lo toglie.
    Folder {
        label: String,
        cmd: Command,
    },
    Head(&'static str),
    Sep,
    /// Da qui una colonna nuova, accanto.
    Break,
}

impl Row {
    /// Una voce senza tasto ne' segno.
    pub(crate) fn plain(label: &'static str, cmd: Command) -> Row {
        Row::Item { label, key: String::new(), on: false, cmd }
    }

    /// Una voce senza tasto, con il segno se `on`.
    pub(crate) fn check(label: &'static str, on: bool, cmd: Command) -> Row {
        Row::Item { label, key: String::new(), on, cmd }
    }

    fn height(&self) -> f32 {
        match self {
            Row::Item { .. } | Row::Recent { .. } | Row::Folder { .. } => 34.0,
            Row::Head(_) => 30.0,
            Row::Sep => 13.0,
            Row::Break => 0.0,
        }
    }

    fn command(&self) -> Option<&Command> {
        match self {
            Row::Item { cmd, .. } | Row::Recent { cmd, .. } | Row::Folder { cmd, .. } => Some(cmd),
            _ => None,
        }
    }
}

const MENU_W: f32 = 316.0;
const ZOOM_MENU_W: f32 = 260.0;
/// Altezza della barra in alto, e dei suoi tasti.
const HUD_H: f32 = 52.0;
const BTN: f32 = 36.0;
/// Corpo del numero di pagina nella didascalia.
const FOLIO: f32 = 54.0;
const WELCOME_W: f32 = 540.0;
const WELCOME_ROW: f32 = 46.0;

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
        self.menu = Some(Menu { x, y, selected: None, rows, width: MENU_W * ctx.scale });
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
            Command::SortMenu(x, y) => {
                let width = 300.0 * ctx.scale;
                self.menu = Some(Menu {
                    x: x - width + 36.0 * ctx.scale,
                    y: y + 6.0 * ctx.scale,
                    selected: None,
                    rows: Shelf::sort_menu(self.shelf.sort),
                    width,
                });
                None
            }
            Command::Sort(sort) => {
                self.shelf.set_sort(sort);
                None
            }
            Command::FoldersMenu(x, y) => {
                let d = ctx.shelf.as_ref()?;
                let width = 380.0 * ctx.scale;
                self.menu = Some(Menu {
                    x: x - width + 36.0 * ctx.scale,
                    y: y + 6.0 * ctx.scale,
                    selected: None,
                    rows: Shelf::folders_menu(d, &ctx.settings.servers),
                    width,
                });
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
        if self.shelf.gliding() || self.thumbs.gliding() {
            return Some(now);
        }
        [self.caption.as_ref(), self.toast.as_ref().map(|t| &t.1)]
            .into_iter()
            .flatten()
            .filter_map(|s| s.wake(now))
            .min()
    }

    /// La rotella: nella libreria scorre; `false` se non e' affar suo.
    pub fn wheel(&mut self, pixels: f32, ctx: &Context) -> bool {
        if self.menu.is_some() {
            return false;
        }
        if let Some(p) = &mut self.prefs {
            if Prefs::covers(ctx.view, ctx.scale, self.pointer.0) {
                p.wheel(ctx.settings, ctx.keys, ctx.view, ctx.scale, pixels);
            }
            return true;
        }
        if ctx.thumbs.is_some() {
            self.thumbs.wheel(pixels);
            return true;
        }
        if ctx.shelf.is_some() {
            self.shelf.wheel(pixels);
            return true;
        }
        false
    }

    /// Il mouse si e' mosso: `true` se va ridisegnato.
    pub fn motion(&mut self, x: f32, y: f32, ctx: &Context, now: Instant) -> bool {
        self.pointer = (x, y);
        let hover = if self.modal() { None } else { hud_hit(ctx, x, y).filter(|b| b.cmd.is_some()).map(|b| b.id()) };
        let hover_changed = hover != self.hover;
        self.hover = hover;
        if let Some(i) = self.menu_hit(ctx, x, y)
            && let Some(m) = &mut self.menu
            && m.rows[i].command().is_some()
            && m.selected != Some(i)
        {
            m.selected = Some(i);
            return true;
        }
        if self.menu.is_none()
            && self.goto.is_none()
            && let Some(p) = &mut self.prefs
        {
            return p.motion(ctx.settings, ctx.keys, ctx.view, ctx.scale, x, y);
        }
        if self.modal() {
            return false;
        }
        if let Some(d) = &ctx.thumbs {
            return self.thumbs.motion(d, ctx.view, ctx.scale, x, y);
        }
        if let Some(d) = &ctx.shelf {
            return self.shelf.motion(d, ctx.view, ctx.scale, x, y);
        }
        if ctx.book.is_some() {
            if ctx.bar() {
                return hover_changed; // titolo e pagina sono gia' nella barra
            }
            let was = self.caption.as_ref().is_some_and(|s| s.alpha(now) > 0.99);
            self.poke(now);
            return !was || hover_changed;
        }
        let hover = self.welcome_hit(ctx, x, y);
        let changed = hover != self.selected;
        if hover.is_some() {
            self.selected = hover;
        }
        changed
    }

    pub fn click(&mut self, x: f32, y: f32, ctx: &Context) -> Handled {
        if let Some(f) = &mut self.server {
            // fuori dai campi non si chiude: si perderebbe cio' che si e' scritto
            f.click(ctx, x, y);
            return Handled::Yes(None);
        }
        if let Some(m) = &self.menu {
            let hit = self.menu_hit(ctx, x, y);
            let cmd = hit.and_then(|i| m.rows[i].command().cloned());
            // un clic sul menu che non sceglie niente (un titolo, una riga) lo lascia aperto
            if cmd.is_some() || hit.is_none() {
                self.menu = None;
            }
            return Handled::Yes(self.internal(cmd, ctx));
        }
        if let Some(g) = &mut self.goto {
            let (x0, w, y0) = ruler(ctx);
            if (y - y0).abs() < 24.0 * ctx.scale && x >= x0 - 8.0 && x <= x0 + w + 8.0 {
                let f = ((x - x0) / w).clamp(0.0, 1.0);
                g.typed = (1 + (f * (g.pages - 1) as f32).round() as usize).to_string();
                return Handled::Yes(None);
            }
            self.goto = None;
            return Handled::Yes(None);
        }
        if let Some(p) = &mut self.prefs {
            if Prefs::covers(ctx.view, ctx.scale, x) {
                return p.click(ctx.settings, ctx.keys, ctx.view, ctx.scale, x, y);
            }
            // un clic sulle pagine chiude il foglio, come fuori da un menu
            return Handled::Yes(Some(Command::Act(Action::ToggleSettings)));
        }
        if let Some(d) = &ctx.thumbs {
            return self.thumbs.click(d, ctx.view, ctx.scale, x, y);
        }
        if ctx.bar() && y < hud_height(ctx.scale) {
            let Some(b) = hud_hit(ctx, x, y) else { return Handled::Yes(None) };
            if b.id() == Hover::Zoom && b.cmd.is_some() {
                // la percentuale apre i livelli di zoom, appena sotto
                let x = (b.x + b.w / 2.0 - ZOOM_MENU_W * ctx.scale / 2.0).round();
                self.menu = Some(Menu {
                    x,
                    y: hud_height(ctx.scale) + 6.0 * ctx.scale,
                    selected: None,
                    rows: zoom_rows(ctx),
                    width: ZOOM_MENU_W * ctx.scale,
                });
                return Handled::Yes(None);
            }
            return Handled::Yes(b.cmd);
        }
        if let Some(d) = &ctx.shelf {
            let handled = self.shelf.click(d, ctx.view, ctx.scale, x, y);
            return match handled {
                Handled::Yes(cmd) => Handled::Yes(self.internal(cmd, ctx)),
                Handled::No => Handled::No,
            };
        }
        if ctx.book.is_none() {
            if Self::welcome_cta_hit(ctx, y) {
                return Handled::Yes(Some(Command::Act(Action::AddLibraryFolder)));
            }
            return Handled::Yes(Some(match self.welcome_hit(ctx, x, y) {
                Some(i) => Command::Open(ctx.recent[i].path.clone()),
                None => Command::Act(Action::Open),
            }));
        }
        Handled::No
    }

    pub fn key(&mut self, key: Key, ctx: &Context) -> Handled {
        if let Some(f) = &mut self.server {
            return Handled::Yes(match f.key(key) {
                Outcome::Stay => None,
                Outcome::Close => {
                    self.server = None;
                    None
                }
                Outcome::Submit(cmd) => Some(cmd),
            });
        }
        // Tab serve solo al modulo: altrove resta ai tasti scelti da chi legge
        if key == Key::Tab {
            return Handled::No;
        }
        if let Some(g) = &mut self.goto {
            match key {
                Key::Digit(d) => {
                    if g.typed.len() < g.pages.to_string().len() && !(g.typed.is_empty() && d == 0) {
                        g.typed.push((b'0' + d) as char);
                    }
                }
                Key::Backspace => {
                    g.typed.pop();
                }
                Key::Up => g.step(1),
                Key::Down => g.step(-1),
                Key::PageUp => g.step(10),
                Key::PageDown => g.step(-10),
                Key::Home => g.typed = "1".into(),
                Key::End => g.typed = g.pages.to_string(),
                Key::Escape => self.goto = None,
                Key::Enter => {
                    if g.valid() {
                        let page = g.page() - 1;
                        self.goto = None;
                        return Handled::Yes(Some(Command::Act(Action::GoTo(page))));
                    }
                }
                Key::Left | Key::Right | Key::Tab | Key::Char(_) => {}
            }
            return Handled::Yes(None);
        }
        if let Some(m) = &self.menu {
            let rows = m.rows.clone();
            let choosable: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].command().is_some()).collect();
            let at = m.selected.and_then(|s| choosable.iter().position(|&i| i == s));
            let pick = |d: isize| {
                let n = choosable.len() as isize;
                at.map_or(if d > 0 { 0 } else { n - 1 }, |a| (a as isize + d).rem_euclid(n)) as usize
            };
            match key {
                Key::Down | Key::Up if !choosable.is_empty() => {
                    let i = choosable[pick(if key == Key::Down { 1 } else { -1 })];
                    if let Some(m) = &mut self.menu {
                        m.selected = Some(i);
                    }
                }
                Key::Enter => {
                    let cmd = m.selected.and_then(|i| rows[i].command().cloned());
                    self.menu = None;
                    return Handled::Yes(self.internal(cmd, ctx));
                }
                Key::Escape => self.menu = None,
                _ => {}
            }
            return Handled::Yes(None);
        }
        if let Some(p) = &mut self.prefs {
            return p.key(key, ctx.settings, ctx.keys, ctx.view, ctx.scale);
        }
        if let Some(d) = &ctx.thumbs {
            return self.thumbs.key(key, d, ctx.view, ctx.scale);
        }
        if let Some(d) = &ctx.shelf {
            let handled = self.shelf.key(key, d, ctx.view, ctx.scale);
            return match handled {
                Handled::Yes(cmd) => Handled::Yes(self.internal(cmd, ctx)),
                Handled::No => Handled::No,
            };
        }
        if ctx.book.is_none() && !ctx.recent.is_empty() {
            let n = ctx.recent.len();
            match key {
                Key::Down => self.selected = Some(self.selected.map_or(0, |i| (i + 1) % n)),
                Key::Up => self.selected = Some(self.selected.map_or(n - 1, |i| (i + n - 1) % n)),
                Key::Enter => {
                    return Handled::Yes(self.selected.map(|i| Command::Open(ctx.recent[i].path.clone())));
                }
                _ => return Handled::No,
            }
            return Handled::Yes(None);
        }
        Handled::No
    }

    /// Cosa disegnare sopra le pagine, adesso.
    pub fn scene(&mut self, ctx: &Context, now: Instant, m: &mut dyn Measure) -> Scene {
        if self.caption.as_ref().is_some_and(|s| s.gone(now)) {
            self.caption = None;
        }
        if self.toast.as_ref().is_some_and(|t| t.1.gone(now)) {
            self.toast = None;
        }
        let mut scene = Scene::default();
        if let Some((x, y, r)) = ctx.lens {
            scene.layers.push(lens_ring(x, y, r, ctx.scale));
        }
        if let Some(d) = &ctx.thumbs {
            self.thumbs.advance(d, ctx.view, ctx.scale, now);
            scene.layers.extend(self.thumbs.layers(d, ctx.view, ctx.scale, m));
            let toast = self.toast.as_ref().map(|(text, s)| (text.as_str(), s.alpha(now)));
            if let Some((text, a)) = toast {
                scene.layers.push(toast_only(ctx, text, a, m));
            }
            self.push_prefs(&mut scene, ctx, m);
            if let Some(menu) = &self.menu {
                scene.layers.push(self.menu_layer(ctx, menu, m));
            }
            return scene;
        }
        if let Some(d) = &ctx.shelf {
            self.shelf.advance(d, ctx.view, ctx.scale, now);
            scene.layers.extend(self.shelf.layers(d, ctx.view, ctx.scale, m));
            let toast = self.toast.as_ref().map(|(text, s)| (text.as_str(), s.alpha(now)));
            if let Some((text, a)) = toast {
                scene.layers.push(toast_only(ctx, text, a, m));
            }
            self.push_prefs(&mut scene, ctx, m);
            if let Some(menu) = &self.menu {
                scene.layers.push(self.menu_layer(ctx, menu, m));
            }
            return scene;
        }
        match &ctx.book {
            Some(book) => {
                // con la barra, titolo e pagina stanno li': in basso solo gli avvisi
                let a = if ctx.bar() { 0.0 } else { self.caption.as_ref().map_or(0.0, |s| s.alpha(now)) };
                let toast = self.toast.as_ref().map(|(text, s)| (text.as_str(), s.alpha(now)));
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
            }
            None => scene.layers.push(welcome(ctx, self.selected, m)),
        }
        self.push_prefs(&mut scene, ctx, m);
        if let Some(menu) = &self.menu {
            scene.layers.push(self.menu_layer(ctx, menu, m));
        }
        scene
    }

    fn push_prefs(&self, scene: &mut Scene, ctx: &Context, m: &mut dyn Measure) {
        if let Some(p) = &self.prefs {
            scene.layers.extend(p.layers(ctx.settings, ctx.keys, ctx.view, ctx.scale, m));
        }
        if let Some(f) = &self.server {
            scene.layers.push(f.layer(ctx, m));
        }
    }

    // -- il menu ---------------------------------------------------------

    /// Il riquadro del menu e l'angolo (sinistra, cima) di ogni riga, in
    /// pixel. Le righe vanno in colonne affiancate, separate da `Row::Break`:
    /// un menu lungo resta basso, e si legge come le due pagine di un libro.
    fn menu_frame(&self, ctx: &Context, m: &Menu, rows: &[Row]) -> (Rect, Vec<(f32, f32)>) {
        let s = ctx.scale;
        let (vw, vh) = ctx.view;
        let columns = rows.split(|r| matches!(r, Row::Break));
        let tallest = columns.clone().map(|c| c.iter().map(Row::height).sum::<f32>()).fold(0.0, f32::max);
        let w = m.width * columns.count() as f32;
        let h = (tallest + 16.0) * s;
        let x = if m.x + w > vw - 8.0 * s { (m.x - w).max(8.0 * s) } else { m.x };
        let y = if m.y + h > vh - 8.0 * s { (vh - 8.0 * s - h).max(8.0 * s) } else { m.y };
        let mut spots = Vec::with_capacity(rows.len());
        let (mut left, mut at) = (x.round(), y + 8.0 * s);
        for r in rows {
            if let Row::Break = r {
                (left, at) = ((left + m.width).round(), y + 8.0 * s);
            }
            spots.push((left, at));
            at += r.height() * s;
        }
        (Rect::new(x.round(), y.round(), w.round(), h.round(), BLACK), spots)
    }

    fn menu_hit(&self, ctx: &Context, x: f32, y: f32) -> Option<usize> {
        let m = self.menu.as_ref()?;
        let (frame, spots) = self.menu_frame(ctx, m, &m.rows);
        if x < frame.x || x > frame.x + frame.w || y < frame.y || y > frame.y + frame.h {
            return None;
        }
        spots.iter().zip(&m.rows).position(|(&(left, top), r)| {
            x >= left && x < left + m.width && y >= top && y < top + r.height() * ctx.scale
        })
    }

    fn menu_layer(&self, ctx: &Context, menu: &Menu, m: &mut dyn Measure) -> Layer {
        let s = ctx.scale;
        let rows = &menu.rows;
        let (f, spots) = self.menu_frame(ctx, menu, rows);
        let mut l = Layer::default();
        // ombra morbida (si vede sulle pagine chiare), filo, fondo
        l.rects.push(Rect::new(f.x, f.y + 10.0 * s, f.w, f.h, [0.0, 0.0, 0.0, 0.55]).radius(12.0 * s).blur(26.0 * s));
        l.rects.push(Rect::new(f.x, f.y, f.w, f.h, [1.0, 1.0, 1.0, 0.09]).radius(12.0 * s));
        l.rects.push(
            Rect::new(f.x + s, f.y + s, f.w - 2.0 * s, f.h - 2.0 * s, alpha(rgb(0x11, 0x10, 0x0E), 0.98))
                .radius(11.0 * s),
        );
        let cw = menu.width;
        for (i, (row, &(x, top))) in rows.iter().zip(&spots).enumerate() {
            let h = row.height() * s;
            let left = x + 34.0 * s;
            let right_box = |text: Text, base: f32| {
                text.boxed(110.0 * s, Align::Right).on_baseline(x + cw - 18.0 * s - 110.0 * s, top + base * s)
            };
            if menu.selected == Some(i) {
                l.rects.push(
                    Rect::new(x + 6.0 * s, top + 2.0 * s, cw - 12.0 * s, h - 4.0 * s, [1.0, 1.0, 1.0, 0.065])
                        .radius(7.0 * s),
                );
            }
            match row {
                Row::Item { label, key, on, .. } => {
                    if *on {
                        let d = 6.0 * s;
                        l.rects
                            .push(Rect::new(x + 17.0 * s - d / 2.0, top + h / 2.0 - d / 2.0, d, d, ACCENT).radius(d));
                    }
                    l.texts.push(
                        Text::new(*label, Face::Sans, 13.5 * s, INK).weight(450).on_baseline(left, top + 22.0 * s),
                    );
                    l.texts.push(right_box(Text::new(key.as_str(), Face::Sans, 12.0 * s, MUTED).tabular(), 22.0));
                }
                Row::Recent { title, place, .. } => {
                    let title = Text::new(title.as_str(), Face::Serif, 17.0 * s, INK);
                    l.texts.push(fit(m, title, cw - 34.0 * s - 100.0 * s).on_baseline(left, top + 23.0 * s));
                    l.texts.push(right_box(Text::new(place.as_str(), Face::Sans, 12.0 * s, MUTED).tabular(), 22.0));
                }
                Row::Folder { label, .. } => {
                    let shown = Text::new(label.as_str(), Face::Sans, 13.0 * s, INK);
                    l.texts.push(fit(m, shown, cw - 34.0 * s - 70.0 * s).on_baseline(left, top + 22.0 * s));
                    let hint = if menu.selected == Some(i) { t("togli", "remove") } else { "" };
                    l.texts.push(right_box(Text::new(hint, Face::Sans, 12.0 * s, ACCENT_TEXT), 22.0));
                }
                Row::Head(text) => l.texts.push(caps(text, 10.0 * s, MUTED).on_baseline(left, top + 21.0 * s)),
                Row::Sep => {
                    l.rects.push(Rect::new(x + 18.0 * s, (top + 6.0 * s).round(), cw - 36.0 * s, s.round(), HAIR))
                }
                // fra le colonne un filo, come la piega fra due pagine
                Row::Break => l.rects.push(Rect::new(x, f.y + 18.0 * s, s.round(), f.h - 36.0 * s, HAIR)),
            }
        }
        l
    }

    // -- la galleria vuota -------------------------------------------------

    fn welcome_top(ctx: &Context) -> f32 {
        let s = ctx.scale;
        let n = ctx.recent.len().min(6) as f32;
        let height = if n > 0.0 { 276.0 + n * WELCOME_ROW } else { 170.0 } * s;
        ((ctx.view.1 - height) / 2.0).max(24.0 * s)
    }

    /// La cima della prima riga degli ultimi letti.
    fn welcome_rows_top(ctx: &Context) -> f32 {
        Self::welcome_top(ctx) + 276.0 * ctx.scale
    }

    /// La linea di base dell'invito a creare la libreria.
    fn welcome_cta(ctx: &Context) -> f32 {
        Self::welcome_top(ctx) + 152.0 * ctx.scale
    }

    fn welcome_cta_hit(ctx: &Context, y: f32) -> bool {
        (y - (Self::welcome_cta(ctx) - 5.0 * ctx.scale)).abs() < 16.0 * ctx.scale
    }

    fn welcome_hit(&self, ctx: &Context, x: f32, y: f32) -> Option<usize> {
        let s = ctx.scale;
        let x0 = (ctx.view.0 - WELCOME_W * s) / 2.0;
        let top = Self::welcome_rows_top(ctx);
        if x < x0 || x > x0 + WELCOME_W * s || y < top {
            return None;
        }
        let i = ((y - top) / (WELCOME_ROW * s)) as usize;
        (i < ctx.recent.len().min(6)).then_some(i)
    }
}

pub(crate) const INK_F: [f32; 4] = rgb(0xED, 0xE8, 0xDF);

pub(crate) const MUTED_F: [f32; 4] = rgb(0x9D, 0x97, 0x8B);

#[cfg(test)]
mod tests;
