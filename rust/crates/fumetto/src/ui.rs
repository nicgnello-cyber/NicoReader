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
    /// Dalle miniature: va alla pagina (da 0) e torna a leggere.
    Page(usize),
    /// Una scelta fatta nelle impostazioni.
    Pref(crate::prefs::Pref),
    /// Interni all'interfaccia: aprire una serie, il menu delle cartelle.
    Series(String),
    FoldersMenu(f32, f32),
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
    Item { label: &'static str, key: String, on: bool, cmd: Command },
    Recent { title: String, place: String, cmd: Command },
    /// Una cartella della libreria: il suo percorso, e un clic la toglie.
    Folder { path: PathBuf, cmd: Command },
    Head(&'static str),
    Sep,
    /// Da qui una colonna nuova, accanto.
    Break,
}

impl Row {
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
        self.goto.is_some() || self.menu.is_some() || self.prefs.is_some()
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
        if self.menu.is_some() || self.goto.is_some() {
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
        let since = match &self.toast {
            Some((_, s)) if !s.gone(now) => s.since,
            _ => now,
        };
        self.toast = Some((text.into(), Show { since, until: now + TOAST_HOLD }));
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
            Command::FoldersMenu(x, y) => {
                let d = ctx.shelf.as_ref()?;
                let width = 380.0 * ctx.scale;
                self.menu = Some(Menu { x: x - width + 36.0 * ctx.scale, y: y + 6.0 * ctx.scale, selected: None,
                                        rows: Shelf::folders_menu(d), width });
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
        if self.menu.is_none() && self.goto.is_none()
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
                self.menu = Some(Menu { x, y: hud_height(ctx.scale) + 6.0 * ctx.scale, selected: None, rows: zoom_rows(ctx),
                                        width: ZOOM_MENU_W * ctx.scale });
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
                Key::Left | Key::Right | Key::Char(_) => {}
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
                scene.layers.push(toast_only(ctx, text, a));
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
                scene.layers.push(toast_only(ctx, text, a));
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
        l.rects.push(Rect::new(f.x + s, f.y + s, f.w - 2.0 * s, f.h - 2.0 * s, alpha(rgb(0x11, 0x10, 0x0E), 0.98))
            .radius(11.0 * s));
        let cw = menu.width;
        for (i, (row, &(x, top))) in rows.iter().zip(&spots).enumerate() {
            let h = row.height() * s;
            let left = x + 34.0 * s;
            let right_box = |text: Text, base: f32| {
                text.boxed(110.0 * s, Align::Right).on_baseline(x + cw - 18.0 * s - 110.0 * s, top + base * s)
            };
            if menu.selected == Some(i) {
                l.rects.push(Rect::new(x + 6.0 * s, top + 2.0 * s, cw - 12.0 * s, h - 4.0 * s, [1.0, 1.0, 1.0, 0.065])
                    .radius(7.0 * s));
            }
            match row {
                Row::Item { label, key, on, .. } => {
                    if *on {
                        let d = 6.0 * s;
                        l.rects.push(Rect::new(x + 17.0 * s - d / 2.0, top + h / 2.0 - d / 2.0, d, d, ACCENT).radius(d));
                    }
                    l.texts.push(Text::new(*label, Face::Sans, 13.5 * s, INK).weight(450).on_baseline(left, top + 22.0 * s));
                    l.texts.push(right_box(Text::new(key.as_str(), Face::Sans, 12.0 * s, MUTED).tabular(), 22.0));
                }
                Row::Recent { title, place, .. } => {
                    let title = Text::new(title.as_str(), Face::Serif, 17.0 * s, INK);
                    l.texts.push(fit(m, title, cw - 34.0 * s - 100.0 * s).on_baseline(left, top + 23.0 * s));
                    l.texts.push(right_box(Text::new(place.as_str(), Face::Sans, 12.0 * s, MUTED).tabular(), 22.0));
                }
                Row::Folder { path, .. } => {
                    let shown = Text::new(path.to_string_lossy(), Face::Sans, 13.0 * s, INK);
                    l.texts.push(fit(m, shown, cw - 34.0 * s - 70.0 * s).on_baseline(left, top + 22.0 * s));
                    let hint = if menu.selected == Some(i) { t("togli", "remove") } else { "" };
                    l.texts.push(right_box(Text::new(hint, Face::Sans, 12.0 * s, ACCENT_TEXT), 22.0));
                }
                Row::Head(text) => l.texts.push(caps(text, 10.0 * s, MUTED).on_baseline(left, top + 21.0 * s)),
                Row::Sep => l.rects.push(Rect::new(x + 18.0 * s, (top + 6.0 * s).round(), cw - 36.0 * s, s.round(), HAIR)),
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

/// Un nastrino rosso lacca: la pagina e' segnata. Largo `w`, alto `h`, con
/// la coda a V. Il corpo e' un rettangolo; le due punte della coda si
/// disegnano una riga di pixel alla volta, sempre piu' strette: a questa
/// misura la scaletta non si vede, e il fondo resta quello che e'.
fn ribbon(l: &mut Layer, x: f32, y: f32, w: f32, h: f32, a: f32) {
    let notch = (w * 0.7).round().max(2.0);
    let body = h - notch;
    l.rects.push(Rect::new(x, y, w, body, alpha(ACCENT, a)));
    for k in 0..notch as usize {
        let leg = (w / 2.0 * (1.0 - (k as f32 + 0.5) / notch)).max(0.5);
        let row = y + body + k as f32;
        l.rects.push(Rect::new(x, row, leg, 1.0, alpha(ACCENT, a)));
        l.rects.push(Rect::new(x + w - leg, row, leg, 1.0, alpha(ACCENT, a)));
    }
}

/// Titolo e pagina nei margini; un avviso sopra il numero di pagina.
fn caption(ctx: &Context, b: &BookInfo, a: f32, toast: Option<(&str, f32)>, m: &mut dyn Measure) -> Layer {
    let s = ctx.scale;
    let (vw, vh) = ctx.view;
    let pad = 40.0 * s;
    let bottom = vh - 38.0 * s;
    let mut l = Layer::default();

    let title = Text::new(b.title, Face::Serif, 26.0 * s, fade(INK, a));
    let modes = caps(&b.modes.join("  \u{00b7}  "), 11.5 * s, fade(MUTED, a));
    let folio = Text::new(b.folio.as_str(), Face::Serif, FOLIO * s, fade(INK, a));
    let of = caps(&if italian() { format!("di {}", b.pages) } else { format!("of {}", b.pages) }, 11.5 * s,
                  fade(MUTED, a)).tabular();
    // nei margini se ci stanno, il titolo anche su due righe come
    // un'etichetta da museo; se no le pagine arrivano ai bordi, e una
    // sfumatura dal basso rende leggibile cio' che ci si scrive sopra
    let margin_room = b.margins.0 - pad - 20.0 * s;
    let wrapped = title.clone().boxed(margin_room.max(1.0), Align::Left);
    let title_lines = if m.width(&title) <= margin_room { 1 } else { m.lines(&wrapped) };
    let left_fits = title_lines <= 2 && m.width(&modes) <= margin_room;
    let right_fits = m.width(&folio).max(m.width(&of)) + pad + 24.0 * s <= b.margins.1;
    let banded = !(left_fits && right_fits);
    let visible = a.max(toast.map_or(0.0, |t| t.1));
    if banded {
        let h = 240.0 * s;
        l.rects.push(Rect::new(0.0, vh - h, vw, h, [0.0, 0.0, 0.0, 0.0]).to([0.0, 0.0, 0.0, 0.9 * visible]));
    }

    if a > 0.0 {
        let base = bottom - 25.0 * s;
        if banded {
            l.texts.push(fit(m, title, vw * 0.45).on_baseline(pad, base));
            l.texts.push(fit(m, modes, vw * 0.45).on_baseline(pad, bottom));
        } else {
            // due righe: la prima una riga sopra, l'ultima al solito posto
            let up = if title_lines == 2 { (title.size * 1.2).ceil() } else { 0.0 };
            l.texts.push(wrapped.on_baseline(pad, base - up));
            l.texts.push(modes.on_baseline(pad, bottom));
        }
        let bw = 300.0 * s;
        let bx = vw - pad - bw;
        if b.bookmarked {
            // accanto al numero di pagina, alto quanto le sue cifre
            let fw = m.width(&folio);
            let rh = (FOLIO * s * 0.62).round();
            ribbon(&mut l, (vw - pad - fw - 26.0 * s).round(), (bottom - 24.0 * s - rh).round(), (9.0 * s).round(), rh, a);
        }
        l.texts.push(folio.boxed(bw, Align::Right).on_baseline(bx, bottom - 24.0 * s));
        l.texts.push(of.boxed(bw, Align::Right).on_baseline(bx, bottom));
    }
    if let Some((text, ta)) = toast {
        // sopra il numero di pagina, separato da un filo
        let line_y = (bottom - 24.0 * s - FOLIO * s * 0.72 - 22.0 * s).round();
        let lw = 40.0 * s;
        l.rects.push(Rect::new((vw - pad - lw).round(), line_y, lw.round(), s.round(), alpha(TICK, ta)));
        let bw = 400.0 * s;
        l.texts.push(Text::new(text, Face::Sans, 15.0 * s, fade(INK, ta)).weight(500)
            .boxed(bw, Align::Right).on_baseline(vw - pad - bw, line_y - 15.0 * s));
    }
    l
}

/// Il righello di "vai a pagina": (x, larghezza, y) in pixel.
fn ruler(ctx: &Context) -> (f32, f32, f32) {
    let s = ctx.scale;
    let (vw, vh) = ctx.view;
    let w = (560.0 * s).min(vw * 0.6).round();
    (((vw - w) / 2.0).round(), w, (vh / 2.0 + 70.0 * s).round())
}

fn goto(ctx: &Context, g: &GoTo, m: &mut dyn Measure) -> Layer {
    let s = ctx.scale;
    let (vw, vh) = ctx.view;
    let cx = vw / 2.0;
    let base = vh / 2.0 + 10.0 * s;
    let mut l = Layer::default();
    l.rects.push(Rect::new(0.0, 0.0, vw, vh, [0.0, 0.0, 0.0, 0.9]));

    l.texts.push(caps(t("Vai a pagina", "Go to page"), 11.0 * s, MUTED).boxed(400.0 * s, Align::Center)
        .on_baseline(cx - 200.0 * s, base - 150.0 * s));
    // numero, cursore e "di 212" centrati insieme, misurati
    let (digits, color) = if g.typed.is_empty() { (g.page().to_string(), MUTED) } else { (g.typed.clone(), INK) };
    let size = 156.0 * s;
    let number = Text::new(digits, Face::Serif, size, color);
    let of = Text::new(if italian() { format!("di {}", g.pages) } else { format!("of {}", g.pages) },
                       Face::Serif, 30.0 * s, MUTED);
    let (wn, wo, gap) = (m.width(&number), m.width(&of), 34.0 * s);
    let x0 = (cx - (wn + gap + wo) / 2.0).round();
    l.texts.push(number.on_baseline(x0, base));
    let caret_h = (size * 0.66).round();
    l.rects.push(Rect::new((x0 + wn + 12.0 * s).round(), (base - caret_h).round(), (2.0 * s).round(), caret_h, ACCENT));
    l.texts.push(of.on_baseline(x0 + wn + gap, base));

    let (x0, w, y) = ruler(ctx);
    let last = (g.pages.max(2) - 1) as f32;
    let at = |page: usize| (x0 + w * page as f32 / last).round();
    l.rects.push(Rect::new(x0, y, w, s.round(), HAIR));
    // una tacca per pagina se c'e' posto, altrimenti ogni 2, 5, 10...
    let every = [1usize, 2, 5, 10, 20, 50, 100, 200, 500]
        .into_iter()
        .find(|&k| w / last * k as f32 >= 4.0 * s)
        .unwrap_or(1000);
    for p in (0..g.pages).step_by(every) {
        let major = (p + 1) % (every * 10) == 0 || p == 0;
        let h = if major { 9.0 } else { 5.0 } * s;
        l.rects.push(Rect::new(at(p), y - h, s.round(), h, if major { TICK } else { HAIR }));
    }
    let label_every = [10usize, 20, 25, 50, 100, 200, 250, 500, 1000, 2000]
        .into_iter()
        .find(|&k| w / last * k as f32 >= 64.0 * s)
        .unwrap_or(5000);
    for p in (label_every..=g.pages).step_by(label_every) {
        l.texts.push(Text::new(p.to_string(), Face::Sans, 10.5 * s, MUTED).tabular().boxed(60.0 * s, Align::Center)
            .on_baseline(at(p - 1) - 30.0 * s, y + 26.0 * s));
    }
    // le pagine segnate: nastrini sotto il righello
    for &p in &g.bookmarks {
        ribbon(&mut l, (at(p) - 3.0 * s).round(), y + 5.0 * s, (7.0 * s).round(), (13.0 * s).round(), 1.0);
    }
    // dove si e' adesso, e dove si andra'
    let here = at(g.here);
    l.rects.push(Rect::new(here, y - 16.0 * s, s.round(), 16.0 * s, INK_F));
    l.texts.push(caps(t("qui", "here"), 9.0 * s, INK).boxed(60.0 * s, Align::Center).on_baseline(here - 30.0 * s, y - 22.0 * s));
    if g.valid() && g.page() - 1 != g.here {
        let x = at(g.page() - 1);
        l.rects.push(Rect::new(x - s, y - 24.0 * s, (2.0 * s).round(), 24.0 * s, ACCENT));
        let d = 7.0 * s;
        l.rects.push(Rect::new(x - d / 2.0, y - 24.0 * s - d / 2.0, d, d, ACCENT).radius(d));
    }
    let (hint, color) = if g.valid() {
        (t("Invio per andare   \u{00b7}   Esc per restare", "Enter to go   \u{00b7}   Esc to stay").to_owned(), MUTED)
    } else if italian() {
        (format!("Il volume ha {} pagine", g.pages), ACCENT_TEXT)
    } else {
        (format!("This volume has {} pages", g.pages), ACCENT_TEXT)
    };
    l.texts.push(Text::new(hint, Face::Sans, 12.5 * s, color).boxed(500.0 * s, Align::Center)
        .on_baseline(cx - 250.0 * s, y + 70.0 * s));
    l
}

pub(crate) const INK_F: [f32; 4] = rgb(0xED, 0xE8, 0xDF);

/// La galleria vuota: il nome, cosa si puo' fare, gli ultimi letti.
fn welcome(ctx: &Context, selected: Option<usize>, m: &mut dyn Measure) -> Layer {
    let s = ctx.scale;
    let (vw, vh) = ctx.view;
    let cx = vw / 2.0;
    let top = Ui::welcome_top(ctx);
    let mut l = Layer::default();
    let wide = 900.0 * s;
    l.texts.push(Text::new("NicoReader", Face::Serif, 84.0 * s, INK).boxed(wide, Align::Center)
        .on_baseline(cx - wide / 2.0, top + 70.0 * s));
    l.texts.push(caps(t("Trascina qui un volume, o fai clic per aprirne uno",
                        "Drop a volume here, or click to open one"), 10.5 * s, MUTED)
        .boxed(wide, Align::Center).on_baseline(cx - wide / 2.0, top + 112.0 * s));
    // l'invito a creare la libreria, in rosso lacca: e' la cosa da fare
    l.texts.push(caps(t("+   Aggiungi una cartella alla libreria", "+   Add a folder to the library"), 11.0 * s, ACCENT_TEXT)
        .boxed(wide, Align::Center).on_baseline(cx - wide / 2.0, Ui::welcome_cta(ctx)));
    let recent = &ctx.recent[..ctx.recent.len().min(6)];
    if !recent.is_empty() {
        l.rects.push(Rect::new((cx - 20.0 * s).round(), (top + 190.0 * s).round(), (40.0 * s).round(), s.round(), TICK));
        l.texts.push(caps(t("Ultimi letti", "Recently read"), 10.0 * s, MUTED).boxed(wide, Align::Center)
            .on_baseline(cx - wide / 2.0, top + 234.0 * s));
        let x0 = cx - WELCOME_W * s / 2.0;
        let w = WELCOME_W * s;
        let rows_top = Ui::welcome_rows_top(ctx);
        for (i, r) in recent.iter().enumerate() {
            let y = rows_top + i as f32 * WELCOME_ROW * s;
            if selected == Some(i) {
                l.rects.push(Rect::new(x0 - 14.0 * s, y + 3.0 * s, w + 28.0 * s, (WELCOME_ROW - 6.0) * s, [1.0, 1.0, 1.0, 0.05])
                    .radius(8.0 * s));
            }
            if i > 0 {
                l.rects.push(Rect::new(x0, y.round(), w, s.round(), alpha(HAIR, 0.7)));
            }
            let title = Text::new(r.title.as_str(), Face::Serif, 22.0 * s, INK);
            l.texts.push(fit(m, title, w - 130.0 * s).on_baseline(x0, y + 30.0 * s));
            l.texts.push(Text::new(r.place.as_str(), Face::Sans, 12.5 * s, MUTED).tabular().boxed(110.0 * s, Align::Right)
                .on_baseline(x0 + w - 110.0 * s, y + 29.0 * s));
        }
    }
    let k = |b: Bind| ctx.keys.label(b);
    let keys = if italian() {
        format!("{} apri   \u{00b7}   {} cartella   \u{00b7}   {} impostazioni   \u{00b7}   tasto destro per il menu",
                k(Bind::Open), k(Bind::OpenFolder), k(Bind::Settings))
    } else {
        format!("{} open   \u{00b7}   {} folder   \u{00b7}   {} settings   \u{00b7}   right click for the menu",
                k(Bind::Open), k(Bind::OpenFolder), k(Bind::Settings))
    };
    l.texts.push(Text::new(keys, Face::Sans, 12.0 * s, fade(MUTED, 0.75)).boxed(wide, Align::Center)
        .on_baseline(cx - wide / 2.0, vh - 36.0 * s));
    l
}

/// Una voce di menu con il suo tasto, quello che vale adesso.
fn item(ctx: &Context, label: &'static str, bind: Bind, on: bool, action: Action) -> Row {
    Row::Item { label, key: ctx.keys.label(bind), on, cmd: Command::Act(action) }
}

/// Il menu del tasto destro, fuori dalle copertine. Con un volume aperto, in
/// due colonne: a sinistra i volumi e le pagine, a destra come si legge.
fn main_rows(ctx: &Context) -> Vec<Row> {
    let i = |label, bind, on, action| item(ctx, label, bind, on, action);
    let mut rows = vec![
        i(t("Apri\u{2026}", "Open\u{2026}"), Bind::Open, false, Action::Open),
        i(t("Apri cartella\u{2026}", "Open folder\u{2026}"), Bind::OpenFolder, false, Action::OpenFolder),
        i(t("Libreria", "Library"), Bind::Library, ctx.shelf.is_some(), Action::ToggleLibrary),
    ];
    let recent = if ctx.book.is_some() { 3 } else { 5 };
    if !ctx.recent.is_empty() {
        rows.push(Row::Sep);
        rows.push(Row::Head(t("Recenti", "Recent")));
        for r in ctx.recent.iter().take(recent) {
            rows.push(Row::Recent { title: r.title.clone(), place: r.place.clone(), cmd: Command::Open(r.path.clone()) });
        }
    }
    let Some(b) = &ctx.book else {
        rows.push(Row::Sep);
        rows.push(i(t("Impostazioni\u{2026}", "Settings\u{2026}"), Bind::Settings, false, Action::ToggleSettings));
        return rows;
    };
    rows.extend([
        Row::Sep,
        i(t("Vai a pagina\u{2026}", "Go to page\u{2026}"), Bind::GoTo, false, Action::AskPage),
        i(t("Miniature", "Thumbnails"), Bind::Thumbs, ctx.thumbs.is_some(), Action::ToggleThumbs),
        i(t("Segna la pagina", "Bookmark the page"), Bind::Bookmark, b.bookmarked, Action::ToggleBookmark),
    ]);
    if !b.bookmarks.is_empty() {
        rows.push(Row::Head(t("Segnalibri", "Bookmarks")));
        // i segni piu' vicini alla pagina che si legge
        let mut near: Vec<usize> = b.bookmarks.to_vec();
        near.sort_by_key(|p| p.abs_diff(b.here));
        near.truncate(6);
        near.sort_unstable();
        for p in near {
            let title = if italian() { format!("Pagina {}", p + 1) } else { format!("Page {}", p + 1) };
            let place = if p == b.here { t("qui", "here").to_owned() } else { String::new() };
            rows.push(Row::Recent { title, place, cmd: Command::Act(Action::GoTo(p)) });
        }
    }
    rows.extend([
        Row::Break,
        i(t("Doppia pagina", "Two pages"), Bind::Double, b.double, Action::ToggleDouble),
        i(t("Copertina da sola", "Cover alone"), Bind::Cover, b.double && b.cover_alone, Action::ToggleCover),
        i(t("Nastro", "Strip"), Bind::Strip, b.strip, Action::ToggleStrip),
        i(t("Da destra a sinistra", "Right to left"), Bind::Manga, b.manga, Action::ToggleManga),
        Row::Sep,
        i(t("Ruota a destra", "Rotate right"), Bind::RotateRight, false, Action::Rotate(true)),
        i(t("Ruota a sinistra", "Rotate left"), Bind::RotateLeft, false, Action::Rotate(false)),
        i(t("Rifila i margini", "Trim margins"), Bind::Trim, b.trim, Action::ToggleTrim),
        i(t("Lente", "Magnifier"), Bind::Lens, ctx.lens.is_some(), Action::ToggleLens),
        i(t("Presentazione", "Slideshow"), Bind::Slideshow, b.slideshow, Action::ToggleSlideshow),
        Row::Sep,
        i(t("Salva la pagina\u{2026}", "Save the page\u{2026}"), Bind::Save, false, Action::SavePage),
        i(t("Copia la pagina", "Copy the page"), Bind::Copy, false, Action::CopyPage),
        Row::Sep,
        i(t("Schermo intero", "Full screen"), Bind::Fullscreen, ctx.fullscreen, Action::ToggleFullscreen),
        i(t("Barra in alto", "Top bar"), Bind::Hud, ctx.hud, Action::ToggleHud),
        i(t("Impostazioni\u{2026}", "Settings\u{2026}"), Bind::Settings, false, Action::ToggleSettings),
        Row::Sep,
        i(t("Chiudi", "Close"), Bind::Close, false, Action::Close),
    ]);
    rows
}

/// Le voci del menu dei livelli di zoom.
fn zoom_rows(ctx: &Context) -> Vec<Row> {
    let i = |label, bind, action| item(ctx, label, bind, false, action);
    vec![
        Row::Head(t("Zoom", "Zoom")),
        i(t("Pagina intera", "Whole page"), Bind::ZoomPage, Action::ZoomTo(Zoom::Page)),
        i(t("Larga quanto la finestra", "Fit width"), Bind::ZoomWidth, Action::ZoomTo(Zoom::Width)),
        i(t("100%, pixel reali", "100%, actual pixels"), Bind::ZoomActual, Action::ZoomTo(Zoom::Actual)),
        Row::Sep,
        i(t("Ingrandisci", "Zoom in"), Bind::ZoomIn, Action::ZoomIn),
        i(t("Riduci", "Zoom out"), Bind::ZoomOut, Action::ZoomOut),
        Row::Sep,
        i(t("Lente", "Magnifier"), Bind::Lens, Action::ToggleLens),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Icon {
    Search,
    Close,
    Open,
    Gallery,
    First,
    Prev,
    Next,
    Last,
    Single,
    Double,
    Strip,
    LeftToRight,
    RightToLeft,
    Minus,
    Plus,
    Full,
    FullExit,
    Hide,
    Thumbs,
    Bookmark,
}

/// Cosa mostra un tasto della barra, e quindi quale tasto e'.
#[derive(Clone, Debug, PartialEq)]
enum Look {
    Icon(Icon),
    /// La pagina: "11–12 / 82".
    Folio(String),
    /// Lo zoom: "72%".
    Zoom(String),
}

/// Un tasto della barra: dove sta, cosa mostra, cosa fa.
struct Button {
    face: Look,
    x: f32,
    w: f32,
    cmd: Option<Command>,
    on: bool,
    tip: String,
}

impl Button {
    /// Chi e': l'icona, o quale scritta (per ricordare quale ha il mouse sopra).
    fn id(&self) -> Hover {
        match &self.face {
            Look::Icon(i) => Hover::Icon(*i),
            Look::Folio(_) => Hover::Folio,
            Look::Zoom(_) => Hover::Zoom,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hover {
    Icon(Icon),
    Folio,
    Zoom,
}

fn tip(what: &str, key: &str) -> String {
    format!("{what}   \u{00b7}   {key}")
}

/// I tasti della barra, da sinistra a destra.
fn hud_buttons(ctx: &Context, b: &BookInfo) -> Vec<Button> {
    let s = ctx.scale;
    let vw = ctx.view.0;
    let btn = BTN * s;
    let k = |b: Bind| ctx.keys.label(b);
    let act = |a| Some(Command::Act(a));
    let mut v = Vec::new();
    let mut push = |x: f32, w: f32, face: Look, cmd: Option<Command>, on: bool, tip: String| {
        v.push(Button { face, x: x.round(), w: w.round(), cmd, on, tip });
    };
    let icon = Look::Icon;

    // a sinistra: aprire e tornare alla galleria
    let mut x = 10.0 * s;
    push(x, btn, icon(Icon::Open), act(Action::Open), false, tip(t("Apri", "Open"), &k(Bind::Open)));
    x += btn + 2.0 * s;
    push(x, btn, icon(Icon::Gallery), act(Action::ToggleLibrary), false,
         tip(t("Libreria", "Library"), &k(Bind::Library)));
    x += btn + 2.0 * s;
    push(x, btn, icon(Icon::Thumbs), act(Action::ToggleThumbs), false, tip(t("Miniature", "Thumbnails"), &k(Bind::Thumbs)));

    // a destra, dal bordo verso il centro
    let mut r = vw - 10.0 * s;
    let mut left_of = |w: f32| {
        r -= w;
        r
    };
    let hide = left_of(btn);
    let full = left_of(btn + 2.0 * s);
    let sep1 = left_of(17.0 * s);
    let plus = left_of(btn);
    let zoom = left_of(64.0 * s);
    let minus = left_of(btn);
    let sep2 = left_of(17.0 * s);
    let dir = left_of(btn);
    let strip = left_of(btn + 6.0 * s);
    let double = left_of(btn);
    let single = left_of(btn);
    let sep3 = left_of(17.0 * s);
    let mark = left_of(btn);
    let right_start = r;
    let _ = (sep1, sep2, sep3);

    // al centro: la pagina e come muoversi; si scosta a sinistra se non c'e' posto
    let folio_w = 150.0 * s;
    let center_w = 4.0 * btn + folio_w;
    let cx0 = ((vw - center_w) / 2.0).min(right_start - 24.0 * s - center_w);
    let (back, forth) = if b.manga { (Action::Next, Action::Prev) } else { (Action::Prev, Action::Next) };
    let (start, end) = if b.manga { (Action::Last, Action::First) } else { (Action::First, Action::Last) };
    let page_keys = if b.manga { (k(Bind::Right), k(Bind::Left)) } else { (k(Bind::Left), k(Bind::Right)) };
    let (first_key, last_key) = if b.manga { (k(Bind::Last), k(Bind::First)) } else { (k(Bind::First), k(Bind::Last)) };
    push(cx0, btn, icon(Icon::First), act(start), false, tip(t("Inizio", "Start"), &first_key));
    push(cx0 + btn, btn, icon(Icon::Prev), act(back), false, tip(t("Indietro", "Back"), &page_keys.0));
    push(cx0 + 2.0 * btn, folio_w, Look::Folio(format!("{}  /  {}", b.folio, b.pages)), act(Action::AskPage), false,
         tip(t("Vai a pagina", "Go to page"), &k(Bind::GoTo)));
    push(cx0 + 2.0 * btn + folio_w, btn, icon(Icon::Next), act(forth), false,
         tip(t("Avanti", "Forward"), &page_keys.1));
    push(cx0 + 3.0 * btn + folio_w, btn, icon(Icon::Last), act(end), false, tip(t("Fine", "End"), &last_key));

    push(mark, btn, icon(Icon::Bookmark), act(Action::ToggleBookmark), b.bookmarked,
         tip(if b.bookmarked { t("Pagina segnata", "Bookmarked") } else { t("Segna la pagina", "Bookmark the page") },
             &k(Bind::Bookmark)));
    let single_cmd = if b.double { act(Action::ToggleDouble) } else if b.strip { act(Action::ToggleStrip) } else { None };
    let is_single = !b.double && !b.strip;
    push(single, btn, icon(Icon::Single), single_cmd.or(act(Action::ZoomTo(Zoom::Page))), is_single,
         tip(t("Pagina singola", "Single page"), &k(if b.double { Bind::Double } else { Bind::Strip })));
    push(double, btn, icon(Icon::Double), act(Action::ToggleDouble), b.double,
         tip(t("Doppia pagina", "Two pages"), &k(Bind::Double)));
    push(strip, btn, icon(Icon::Strip), act(Action::ToggleStrip), b.strip, tip(t("Nastro", "Strip"), &k(Bind::Strip)));
    push(dir, btn, icon(if b.manga { Icon::RightToLeft } else { Icon::LeftToRight }), act(Action::ToggleManga),
         b.manga, tip(if b.manga { t("Da destra a sinistra", "Right to left") } else { t("Da sinistra a destra", "Left to right") }, &k(Bind::Manga)));
    let zoomable = b.zoom.is_some();
    push(minus, btn, icon(Icon::Minus), zoomable.then_some(Command::Act(Action::ZoomOut)), false,
         tip(t("Riduci", "Zoom out"), &k(Bind::ZoomOut)));
    push(zoom, 64.0 * s, Look::Zoom(b.zoom.map_or("\u{2014}".into(), |z| format!("{z}%"))),
         zoomable.then_some(Command::Act(Action::ZoomTo(Zoom::Page))), false,
         tip(t("Livelli di zoom", "Zoom levels"), &format!("{} {} {}", k(Bind::ZoomPage), k(Bind::ZoomWidth), k(Bind::ZoomActual))));
    push(plus, btn, icon(Icon::Plus), zoomable.then_some(Command::Act(Action::ZoomIn)), false,
         tip(t("Ingrandisci", "Zoom in"), &k(Bind::ZoomIn)));
    push(full + 2.0 * s, btn, icon(if ctx.fullscreen { Icon::FullExit } else { Icon::Full }),
         act(Action::ToggleFullscreen), false,
         tip(if ctx.fullscreen { t("Esci dallo schermo intero", "Exit full screen") } else { t("Schermo intero", "Full screen") }, &k(Bind::Fullscreen)));
    push(hide, btn, icon(Icon::Hide), act(Action::ToggleHud), false,
         tip(t("Nascondi la barra", "Hide the bar"), &k(Bind::Hud)));
    v
}

/// Il tasto della barra sotto il punto (x, y), se c'e'.
fn hud_hit(ctx: &Context, x: f32, y: f32) -> Option<Button> {
    let b = ctx.book.as_ref().filter(|_| ctx.bar())?;
    let h = hud_height(ctx.scale);
    if y < 0.0 || y >= h {
        return None;
    }
    hud_buttons(ctx, b).into_iter().find(|btn| x >= btn.x && x < btn.x + btn.w)
}

/// La barra in alto: nera come la galleria, un filo la separa dalle pagine.
fn hud(ctx: &Context, b: &BookInfo, hover: Option<Hover>, m: &mut dyn Measure) -> Layer {
    let s = ctx.scale;
    let vw = ctx.view.0;
    let h = hud_height(s);
    let mut l = Layer::default();
    l.rects.push(Rect::new(0.0, 0.0, vw, h, BLACK));
    l.rects.push(Rect::new(0.0, h - s.round(), vw, s.round(), alpha(HAIR, 0.8)));
    let buttons = hud_buttons(ctx, b);
    let (by, bh) = (((h - BTN * s) / 2.0).round(), (BTN * s).round());
    let cy = by + bh / 2.0;

    // i divisori fra i gruppi di destra: dove fra due tasti resta il loro spazio
    for w in buttons.windows(2) {
        let gap = w[1].x - (w[0].x + w[0].w);
        if (12.0 * s..30.0 * s).contains(&gap) {
            let x = (w[0].x + w[0].w + gap / 2.0).round();
            l.rects.push(Rect::new(x, (cy - 9.0 * s).round(), s.round(), (18.0 * s).round(), HAIR));
        }
    }
    let center_start = buttons.iter().find(|b| b.face == Look::Icon(Icon::First)).map_or(vw, |b| b.x);

    // il titolo, fra i tasti di sinistra e quelli del centro
    let title_x = buttons[2].x + buttons[2].w + 16.0 * s;
    let room = center_start - 24.0 * s - title_x;
    if room > 60.0 * s {
        let title = Text::new(b.title, Face::Serif, 19.0 * s, INK);
        l.texts.push(fit(m, title, room).on_baseline(title_x, cy + 6.5 * s));
    }

    for btn in &buttons {
        let enabled = btn.cmd.is_some();
        let hovered = hover == Some(btn.id()) && enabled;
        if hovered {
            l.rects.push(Rect::new(btn.x, by, btn.w, bh, [1.0, 1.0, 1.0, 0.075]).radius(8.0 * s));
        }
        let color = if !enabled { alpha(MUTED_F, 0.35) } else if btn.on || hovered { INK_F } else { MUTED_F };
        let cx = btn.x + btn.w / 2.0;
        match &btn.face {
            Look::Icon(icon) => draw_icon(&mut l, *icon, cx.round(), cy.round(), s, color),
            Look::Folio(label) => l.texts.push(Text::new(label.as_str(), Face::Serif, 19.0 * s, INK)
                .boxed(btn.w, Align::Center).on_baseline(btn.x, cy + 6.5 * s)),
            Look::Zoom(label) => l.texts.push(Text::new(label.as_str(), Face::Sans, 12.5 * s, to_u8(color)).weight(500)
                .tabular().boxed(btn.w, Align::Center).on_baseline(btn.x, cy + 4.5 * s)),
        }
        if btn.on {
            // la voce scelta: un punto rosso sotto
            let d = 4.0 * s;
            l.rects.push(Rect::new(cx - d / 2.0, by + bh - 1.0 * s, d, d, ACCENT).radius(d));
        }
    }

    // il nome del tasto sotto il mouse, con la sua scorciatoia
    if let Some(btn) = buttons.iter().find(|b| hover == Some(b.id()) && b.cmd.is_some()) {
        tooltip(&mut l, m, &btn.tip, btn.x + btn.w / 2.0, h, vw, s);
    }
    l
}

/// Un'etichetta sotto un tasto della barra, centrata in `cx`.
pub(crate) fn tooltip(l: &mut Layer, m: &mut dyn Measure, tip: &str, cx: f32, bar_h: f32, vw: f32, s: f32) {
    let text = Text::new(tip, Face::Sans, 12.5 * s, INK).weight(450);
    let w = (m.width(&text) + 24.0 * s).round();
    let x = (cx - w / 2.0).clamp(8.0 * s, vw - w - 8.0 * s).round();
    let y = (bar_h + 8.0 * s).round();
    let th = (30.0 * s).round();
    l.rects.push(Rect::new(x, y + 4.0 * s, w, th, [0.0, 0.0, 0.0, 0.5]).radius(8.0 * s).blur(10.0 * s));
    l.rects.push(Rect::new(x, y, w, th, [1.0, 1.0, 1.0, 0.1]).radius(8.0 * s));
    l.rects.push(Rect::new(x + s, y + s, w - 2.0 * s, th - 2.0 * s, alpha(rgb(0x14, 0x13, 0x11), 0.98)).radius(7.0 * s));
    l.texts.push(text.boxed(w, Align::Center).on_baseline(x, y + th / 2.0 + 4.5 * s));
}

/// Il bordo della lente: un filo chiaro, e fuori un'ombra che la stacca
/// dalla pagina (il vetro, dentro, non si tocca).
fn lens_ring(x: f32, y: f32, r: f32, s: f32) -> Layer {
    let mut l = Layer::default();
    let ring = |grow: f32, width: f32, color: [f32; 4], blur: f32| {
        let rr = r + grow;
        Rect::new(x - rr, y - rr, 2.0 * rr, 2.0 * rr, color).radius(rr).stroke(width).blur(blur)
    };
    l.rects.push(ring(6.0 * s, 8.0 * s, [0.0, 0.0, 0.0, 0.45], 8.0 * s));
    l.rects.push(ring(1.5 * s, 2.0 * s, alpha(INK_F, 0.9), 0.0));
    l.rects.push(ring(3.0 * s, 1.0 * s, alpha(BLACK, 0.8), 0.0));
    l
}

/// Nella libreria: solo l'avviso, in basso a destra.
fn toast_only(ctx: &Context, text: &str, a: f32) -> Layer {
    let s = ctx.scale;
    let (vw, vh) = ctx.view;
    let mut l = Layer::default();
    let bw = 500.0 * s;
    let base = vh - 40.0 * s;
    l.rects.push(Rect::new(vw - 460.0 * s, vh - 90.0 * s, 460.0 * s, 90.0 * s, [0.0, 0.0, 0.0, 0.0]).to([0.0, 0.0, 0.0, 0.7 * a]));
    l.texts.push(Text::new(text, Face::Sans, 15.0 * s, fade(INK, a)).weight(500).boxed(bw, Align::Right)
        .on_baseline(vw - 40.0 * s - bw, base));
    l
}

fn to_u8(c: [f32; 4]) -> [u8; 4] {
    c.map(|v| (v * 255.0).round() as u8)
}

/// Un'icona di tratti sottili, centrata in (cx, cy), in un quadrato di 16
/// punti. I tratti sono spessi 1,5 punti, come le aste delle etichette.
pub(crate) fn draw_icon(l: &mut Layer, icon: Icon, cx: f32, cy: f32, s: f32, color: [f32; 4]) {
    let w = (1.5 * s).max(1.0);
    let mut line = |pts: &[(f32, f32)]| {
        for p in pts.windows(2) {
            l.rects.push(Rect::line(cx + p[0].0 * s, cy + p[0].1 * s, cx + p[1].0 * s, cy + p[1].1 * s, w, color));
        }
    };
    let mut boxes: Vec<(f32, f32, f32, f32)> = Vec::new();
    let mut ring: Option<(f32, f32, f32)> = None;
    match icon {
        Icon::Open => {
            boxes.push((-8.0, -4.0, 16.0, 11.0));
            line(&[(-7.5, -4.0), (-7.5, -7.0), (-2.5, -7.0), (-0.5, -4.0)]);
        }
        Icon::Gallery => boxes.extend([(-7.0, -7.0, 6.0, 6.0), (1.0, -7.0, 6.0, 6.0), (-7.0, 1.0, 6.0, 6.0), (1.0, 1.0, 6.0, 6.0)]),
        Icon::Prev => line(&[(3.0, -6.0), (-3.0, 0.0), (3.0, 6.0)]),
        Icon::Next => line(&[(-3.0, -6.0), (3.0, 0.0), (-3.0, 6.0)]),
        Icon::First => {
            line(&[(-5.0, -6.0), (-5.0, 6.0)]);
            line(&[(4.0, -6.0), (-2.0, 0.0), (4.0, 6.0)]);
        }
        Icon::Last => {
            line(&[(5.0, -6.0), (5.0, 6.0)]);
            line(&[(-4.0, -6.0), (2.0, 0.0), (-4.0, 6.0)]);
        }
        Icon::Single => boxes.push((-5.0, -7.5, 10.0, 15.0)),
        Icon::Double => boxes.extend([(-8.5, -6.5, 8.0, 13.0), (0.5, -6.5, 8.0, 13.0)]),
        Icon::Strip => boxes.extend([(-5.0, -8.5, 10.0, 7.5), (-5.0, 1.0, 10.0, 7.5)]),
        Icon::LeftToRight => {
            line(&[(-7.0, 0.0), (7.0, 0.0)]);
            line(&[(3.0, -4.0), (7.0, 0.0), (3.0, 4.0)]);
        }
        Icon::RightToLeft => {
            line(&[(7.0, 0.0), (-7.0, 0.0)]);
            line(&[(-3.0, -4.0), (-7.0, 0.0), (-3.0, 4.0)]);
        }
        Icon::Minus => line(&[(-6.0, 0.0), (6.0, 0.0)]),
        Icon::Plus => {
            line(&[(-6.0, 0.0), (6.0, 0.0)]);
            line(&[(0.0, -6.0), (0.0, 6.0)]);
        }
        Icon::Full => {
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                line(&[(7.0 * sx, 3.0 * sy), (7.0 * sx, 7.0 * sy), (3.0 * sx, 7.0 * sy)]);
            }
        }
        Icon::FullExit => {
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                line(&[(7.0 * sx, 3.0 * sy), (3.0 * sx, 3.0 * sy), (3.0 * sx, 7.0 * sy)]);
            }
        }
        Icon::Hide => line(&[(-6.0, 3.0), (0.0, -3.0), (6.0, 3.0)]),
        // tre pagine in fila, come un provino
        Icon::Thumbs => boxes.extend([(-8.5, -5.5, 4.5, 11.0), (-2.25, -5.5, 4.5, 11.0), (4.0, -5.5, 4.5, 11.0)]),
        // un nastrino con la coda a V
        Icon::Bookmark => line(&[(-4.5, -7.5), (4.5, -7.5), (4.5, 7.5), (0.0, 3.5), (-4.5, 7.5), (-4.5, -7.5)]),
        Icon::Close => {
            line(&[(-5.5, -5.5), (5.5, 5.5)]);
            line(&[(5.5, -5.5), (-5.5, 5.5)]);
        }
        Icon::Search => {
            ring = Some((-7.0, -7.0, 11.0));
            line(&[(2.5, 2.5), (7.0, 7.0)]);
        }
    }
    if let Some((x, y, d)) = ring {
        l.rects.push(Rect::new(cx + x * s, cy + y * s, d * s, d * s, color).radius(d * s / 2.0).stroke(w));
    }
    for (x, y, bw, bh) in boxes {
        l.rects.push(Rect::new(cx + x * s, cy + y * s, bw * s, bh * s, color).radius(1.5 * s).stroke(w));
    }
}

pub(crate) const MUTED_F: [f32; 4] = rgb(0x9D, 0x97, 0x8B);

#[cfg(test)]
mod tests {
    use super::*;

    fn book(pages: usize) -> BookInfo<'static> {
        BookInfo {
            title: "Nebbia sul Porto v03",
            folio: "24".into(),
            pages,
            here: 23,
            modes: vec!["pagina singola"],
            margins: (600.0, 600.0),
            zoom: Some(72),
            double: false,
            strip: false,
            manga: false,
            cover_alone: false,
            bookmarked: false,
            bookmarks: &[],
            trim: false,
            slideshow: false,
        }
    }

    fn ctx<'a>(b: Option<BookInfo<'a>>, recent: &'a [Recent]) -> Context<'a> {
        static SETTINGS: std::sync::LazyLock<fumetto_core::Settings> = std::sync::LazyLock::new(Default::default);
        Context { view: (1920.0, 1080.0), scale: 1.0, book: b, recent, hud: false, fullscreen: false, shelf: None,
                  thumbs: None, settings: &SETTINGS, keys: Keymap::defaults(), lens: None }
    }

    #[test]
    fn i_tasti_mostrati_sono_quelli_scelti() {
        let mut chosen = std::collections::BTreeMap::new();
        crate::keys::assign(&mut chosen, Bind::Double, &crate::keys::Combo::parse("P").unwrap());
        let keys = Keymap::new(&chosen);
        let c = Context { keys: &keys, ..ctx(Some(book(10)), &[]) };
        let rows = main_rows(&c);
        let double = rows.iter().find_map(|r| match r {
            Row::Item { key, cmd: Command::Act(Action::ToggleDouble), .. } => Some(key.clone()),
            _ => None,
        });
        assert_eq!(double.as_deref(), Some("P"));
    }

    #[test]
    fn le_impostazioni_si_aprono_e_un_clic_fuori_le_chiude() {
        let mut ui = Ui::new();
        let c = ctx(Some(book(10)), &[]);
        ui.toggle_prefs();
        assert!(ui.modal());
        assert_eq!(ui.click(300.0, 500.0, &c), Handled::Yes(Some(Command::Act(Action::ToggleSettings))));
    }

    #[test]
    fn vai_a_pagina_si_scrive_e_si_conferma() {
        let mut ui = Ui::new();
        let c = ctx(Some(book(212)), &[]);
        ui.ask_page(23, 212, &[]);
        for d in [1, 4, 7] {
            ui.key(Key::Digit(d), &c);
        }
        ui.key(Key::Digit(9), &c); // oltre le cifre del volume: ignorata
        assert_eq!(ui.key(Key::Enter, &c), Handled::Yes(Some(Command::Act(Action::GoTo(146)))));
        assert!(!ui.modal());
    }

    #[test]
    fn vai_a_pagina_rifiuta_le_pagine_che_non_ci_sono() {
        let mut ui = Ui::new();
        let c = ctx(Some(book(48)), &[]);
        ui.ask_page(0, 48, &[]);
        ui.key(Key::Digit(9), &c);
        ui.key(Key::Digit(9), &c);
        assert_eq!(ui.key(Key::Enter, &c), Handled::Yes(None), "99 di 48: si resta");
        assert!(ui.modal());
        ui.key(Key::Backspace, &c);
        assert_eq!(ui.key(Key::Enter, &c), Handled::Yes(Some(Command::Act(Action::GoTo(8)))));
    }

    #[test]
    fn vai_a_pagina_con_le_frecce_parte_da_qui() {
        let mut ui = Ui::new();
        let c = ctx(Some(book(212)), &[]);
        ui.ask_page(23, 212, &[]);
        ui.key(Key::Up, &c);
        assert_eq!(ui.key(Key::Enter, &c), Handled::Yes(Some(Command::Act(Action::GoTo(24)))));
    }

    #[test]
    fn il_menu_salta_titoli_e_separatori() {
        let recent = [Recent { path: "a.cbz".into(), title: "A".into(), place: "3 / 10".into() }];
        let mut ui = Ui::new();
        let c = ctx(Some(book(10)), &recent);
        ui.open_menu(100.0, 100.0, &c);
        ui.key(Key::Down, &c); // Apri
        ui.key(Key::Down, &c); // Apri cartella
        ui.key(Key::Down, &c); // Libreria
        ui.key(Key::Down, &c); // oltre separatore e titolo: il volume recente
        assert_eq!(ui.key(Key::Enter, &c), Handled::Yes(Some(Command::Open("a.cbz".into()))));
        ui.open_menu(100.0, 100.0, &c);
        ui.key(Key::Up, &c); // dal fondo: Chiudi
        assert_eq!(ui.key(Key::Enter, &c), Handled::Yes(Some(Command::Act(Action::Close))));
    }

    /// Il menu lungo va in due colonne: un clic nella seconda sceglie una sua voce.
    #[test]
    fn il_menu_in_due_colonne() {
        let mut ui = Ui::new();
        let c = ctx(Some(book(10)), &[]);
        ui.open_menu(100.0, 100.0, &c);
        let m = ui.menu.as_ref().unwrap();
        let (frame, spots) = ui.menu_frame(&c, m, &m.rows);
        assert!((frame.w - 2.0 * MENU_W).abs() < 1.0, "due colonne: {}", frame.w);
        let double = m.rows.iter().position(|r| matches!(r, Row::Item { cmd: Command::Act(Action::ToggleDouble), .. })).unwrap();
        let (x, y) = spots[double];
        assert!(x > frame.x + MENU_W - 1.0, "la seconda colonna");
        assert_eq!(ui.click(x + 50.0, y + 10.0, &c), Handled::Yes(Some(Command::Act(Action::ToggleDouble))));
    }

    #[test]
    fn i_segnalibri_nel_menu() {
        let marks = [2, 7];
        let mut ui = Ui::new();
        let c = ctx(Some(BookInfo { bookmarks: &marks, ..book(10) }), &[]);
        ui.open_menu(100.0, 100.0, &c);
        let rows = &ui.menu.as_ref().unwrap().rows;
        let jumps: Vec<&Command> = rows.iter().filter_map(|r| match r {
            Row::Recent { cmd: cmd @ Command::Act(Action::GoTo(_)), .. } => Some(cmd),
            _ => None,
        }).collect();
        assert_eq!(jumps, [&Command::Act(Action::GoTo(2)), &Command::Act(Action::GoTo(7))]);
    }

    #[test]
    fn la_didascalia_compare_e_poi_svanisce() {
        let mut ui = Ui::new();
        let c = ctx(Some(book(10)), &[]);
        let t0 = Instant::now();
        assert_eq!(ui.wake(t0), None, "all'inizio non c'e' niente da ridisegnare");
        ui.motion(10.0, 10.0, &c, t0);
        assert_eq!(ui.wake(t0), Some(t0), "sta comparendo");
        let still = t0 + Duration::from_millis(500);
        assert_eq!(ui.wake(still), Some(t0 + CAPTION_HOLD), "ferma fino alla scadenza");
        let gone = t0 + CAPTION_HOLD + Duration::from_secs(1);
        assert!(ui.scene(&c, gone, &mut fumetto_render::Estimate).is_empty());
        assert_eq!(ui.wake(gone), None);
    }

    #[test]
    fn un_titolo_troppo_lungo_si_taglia_con_i_puntini() {
        let t = Text::new("Il Guardiano del Faro e la Lanterna Perduta", Face::Serif, 20.0, INK);
        let cut = fit(&mut fumetto_render::Estimate, t, 200.0);
        assert!(cut.text.ends_with('\u{2026}'), "{}", cut.text);
        assert!(fumetto_render::Estimate.width(&cut) <= 200.0);
        let short = fit(&mut fumetto_render::Estimate, Text::new("Lanterne", Face::Serif, 20.0, INK), 200.0);
        assert_eq!(short.text, "Lanterne");
    }

    #[test]
    fn la_barra_cambia_modo_e_non_gira_pagina() {
        let mut ui = Ui::new();
        let mut c = ctx(Some(book(10)), &[]);
        c.hud = true;
        let b = c.book.as_ref().unwrap();
        let buttons = hud_buttons(&c, b);
        let double = buttons.iter().find(|b| b.face == Look::Icon(Icon::Double)).unwrap();
        let (x, y) = (double.x + double.w / 2.0, 20.0);
        assert_eq!(ui.click(x, y, &c), Handled::Yes(Some(Command::Act(Action::ToggleDouble))));
        assert_eq!(ui.click(5.0 * 60.0, 20.0, &c), Handled::Yes(None), "fra i tasti: niente, e nessun giro di pagina");
        assert_eq!(ui.click(x, 500.0, &c), Handled::No, "sotto la barra: le pagine");
    }

    #[test]
    fn la_percentuale_apre_i_livelli_di_zoom() {
        let mut ui = Ui::new();
        let mut c = ctx(Some(book(10)), &[]);
        c.hud = true;
        let b = c.book.as_ref().unwrap();
        let zoom = hud_buttons(&c, b).into_iter().find(|b| b.id() == Hover::Zoom).unwrap();
        ui.click(zoom.x + 5.0, 20.0, &c);
        assert!(ui.modal());
        ui.key(Key::Down, &c);
        ui.key(Key::Down, &c);
        assert_eq!(ui.key(Key::Enter, &c), Handled::Yes(Some(Command::Act(Action::ZoomTo(Zoom::Width)))));
    }

    #[test]
    fn nella_doppia_pagina_lo_zoom_e_spento() {
        let mut c = ctx(Some(BookInfo { zoom: None, double: true, ..book(10) }), &[]);
        c.hud = true;
        let b = c.book.as_ref().unwrap();
        let buttons = hud_buttons(&c, b);
        assert!(buttons.iter().filter(|b| matches!(b.face, Look::Icon(Icon::Plus | Icon::Minus))).all(|b| b.cmd.is_none()));
    }

    #[test]
    fn nella_galleria_vuota_un_clic_apre() {
        let recent = [Recent { path: "a.cbz".into(), title: "A".into(), place: "letto".into() }];
        let mut ui = Ui::new();
        let c = ctx(None, &recent);
        let y = Ui::welcome_rows_top(&c) + 10.0;
        assert_eq!(ui.click(960.0, y, &c), Handled::Yes(Some(Command::Open("a.cbz".into()))));
        assert_eq!(ui.click(960.0, 20.0, &c), Handled::Yes(Some(Command::Act(Action::Open))));
    }
}
