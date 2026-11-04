//! Lo stato del lettore, senza finestra ne' scheda video: quale pagina, quale
//! modo, dove sta ogni cosa sullo schermo, cosa preparare in anticipo.
//!
//! Tutto qui si prova da solo, con pagine finte: i difetti di tempo e d'ordine
//! (la prima pagina preparata per una finestra da 1x1, le richieste doppie, il
//! lampo nero) diventano test invece di scoperte nella prova automatica.

use std::time::{Duration, Instant};

use fumetto_core::lingua::t;
use fumetto_core::{Fit, Saved, Target, prefetch_order};

use crate::strip::{Strip, glide_step};

mod info;
mod layout;
mod scroll;
#[cfg(test)]
mod tests;
mod zoom;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Single,
    Double,
    Strip,
}

/// Un livello di zoom con un nome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zoom {
    /// Tutta la pagina nella finestra (nel nastro: la larghezza di sempre).
    Page,
    /// La pagina larga quanto la finestra.
    Width,
    /// Un pixel della pagina per un pixel dello schermo.
    Actual,
}

/// Un'azione del lettore, da tastiera, mouse o dalla prova automatica.
/// Le ultime riguardano la finestra e i volumi, e le gestisce l'app.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Next,
    Prev,
    First,
    Last,
    /// Nel nastro scorre; nelle pagine gira pagina.
    Scroll(f32),
    ToggleStrip,
    ToggleDouble,
    /// Doppia pagina: la copertina da sola, e le coppie si sfalsano di una.
    ToggleCover,
    ToggleManga,
    ToggleLinear,
    StripWider(f32),
    /// Ingrandisce di `factor` tenendo fermo il punto (x, y) della finestra.
    Zoom {
        factor: f32,
        x: f32,
        y: f32,
    },
    ZoomReset,
    /// Un passo di zoom al centro della finestra; nel nastro, la sua larghezza.
    ZoomIn,
    ZoomOut,
    ZoomTo(Zoom),
    /// Sposta la pagina ingrandita di (dx, dy) pixel.
    Pan(f32, f32),
    /// Salta a una pagina (contando da 0).
    GoTo(usize),
    /// Chiede a che pagina andare.
    AskPage,
    ToggleFullscreen,
    /// Mostra o nasconde la barra in alto.
    ToggleHud,
    /// La libreria, o di nuovo il volume che si leggeva.
    ToggleLibrary,
    /// Aggiunge una cartella alla libreria (con la finestra di sistema).
    AddLibraryFolder,
    /// Apre un volume con la finestra di sistema (o una cartella).
    Open,
    OpenFolder,
    /// Chiude il volume: resta la galleria vuota.
    Close,
    Quit,
    /// Gira le pagine di un quarto di giro: in senso orario (`true`) o no.
    Rotate(bool),
    /// Rifila i margini uniformi delle scansioni.
    ToggleTrim,
    /// Segna la pagina, o toglie il segno.
    ToggleBookmark,
    /// Le miniature di tutte le pagine.
    ToggleThumbs,
    /// La presentazione: le pagine girano da sole.
    ToggleSlideshow,
    /// L'ingrandimento AI delle scansioni piccole.
    ToggleUpscale,
    /// Il riconoscimento dei webtoon all'apertura.
    ToggleWebtoon,
    /// Salva la pagina in un file, o la copia negli appunti.
    SavePage,
    CopyPage,
    /// La lente d'ingrandimento che segue il mouse.
    ToggleLens,
    /// Il foglio delle impostazioni.
    ToggleSettings,
}

/// Dove sono le pagine pronte non interessa qui: basta sapere per quale
/// misura lo sono.
pub trait Pages {
    fn prepared_for(&self, i: usize) -> Option<Target>;

    fn has(&self, i: usize) -> bool {
        self.prepared_for(i).is_some()
    }
}

/// Una pagina a schermo: quale, e il rettangolo in pixel della finestra.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Item {
    pub page: usize,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

const MAX_ZOOM: f32 = 6.0;
/// Larghezza massima di partenza del nastro, come frazione della finestra:
/// una colonna piu' larga stanca a scorrerla.
const STRIP_FRAC: f32 = 0.6;
/// Nel nastro una striscia si allarga al massimo di tanto, partendo: oltre
/// si vedrebbe sgranata.
const STRIP_GROW: f32 = 1.5;
/// Un webtoon ha strisce almeno tante volte piu' alte che larghe (la mediana).
const WEBTOON_RATIO: f64 = 2.2;

/// Se le pagine misurate sono di un webtoon, la larghezza tipica delle
/// strisce. Ne servono almeno tre: una copertina da sola non decide.
pub fn webtoon_width(sizes: &[(u32, u32)]) -> Option<u32> {
    let mut ratios: Vec<f64> = sizes.iter().filter(|s| s.0 > 0).map(|&(w, h)| h as f64 / w as f64).collect();
    if ratios.len() < 3 {
        return None;
    }
    ratios.sort_by(f64::total_cmp);
    if ratios[ratios.len() / 2] < WEBTOON_RATIO {
        return None;
    }
    let mut widths: Vec<u32> = sizes.iter().map(|s| s.0).collect();
    widths.sort_unstable();
    Some(widths[widths.len() / 2])
}

pub struct Reader {
    len: usize,
    native: Vec<Option<(u32, u32)>>,
    /// Pagine che non si riesce a leggere.
    broken: Vec<bool>,
    /// Somma e numero dei rapporti altezza/larghezza noti: la stima per le
    /// pagine del nastro non ancora decodificate, senza ripassarle tutte.
    ratios: (f64, usize),
    view: (u32, u32),
    pub mode: Mode,
    /// Pagina singola, o prima pagina della coppia.
    page: usize,
    strip: Strip,
    /// Dopo una ripresa: la posizione nel nastro come frazione della pagina
    /// d'ancora, finche' non se ne conosce l'altezza vera. In pixel sarebbe
    /// sbagliata: la stima dell'altezza cambia man mano che arrivano le altre.
    anchor_frac: Option<f32>,
    /// La larghezza del nastro come frazione della finestra, se scelta (o
    /// ripresa); `None`: quella naturale delle strisce, fissata appena se ne
    /// conoscono la misura e la finestra.
    strip_frac: Option<f32>,
    pub manga: bool,
    pub linear: bool,
    /// Senza i margini uniformi (non nel nastro: un webtoon e' una striscia
    /// sola tagliata a pezzi, e il bianco fra le vignette fa parte del disegno).
    pub trim: bool,
    /// Gradi in senso orario: 0, 90, 180, 270.
    rotation: u16,
    /// Le pagine segnate, in ordine.
    bookmarks: Vec<usize>,
    cover_alone: bool,
    /// 1 = pagina intera; solo nella pagina singola.
    zoom: f32,
    /// Il punto della pagina (0..1) che sta al centro della finestra.
    center: (f32, f32),
    forward: bool,
    /// Pixel di scorrimento del nastro ancora da percorrere.
    glide: f32,
    last_turn: Option<Instant>,
    /// Si sfoglia in fretta: la lettura anticipata va piu' lontano.
    hurry: bool,
    /// Appena saltati altrove: prima la pagina a schermo, poi il resto.
    jumped: bool,
    /// Le ultime pagine disegnate: restano finche' le nuove non sono pronte,
    /// invece di un lampo nero.
    last_drawn: Option<usize>,
}

impl Reader {
    pub fn new(len: usize, start: usize) -> Reader {
        let start = start.min(len.saturating_sub(1));
        Reader {
            len,
            native: vec![None; len],
            broken: vec![false; len],
            ratios: (0.0, 0),
            view: (1, 1),
            mode: Mode::Single,
            page: start,
            strip: Strip::at(start),
            anchor_frac: None,
            strip_frac: None,
            manga: false,
            linear: true,
            trim: false,
            rotation: 0,
            bookmarks: Vec::new(),
            cover_alone: false,
            zoom: 1.0,
            center: (0.5, 0.5),
            forward: true,
            glide: 0.0,
            last_turn: None,
            hurry: false,
            jumped: true,
            last_drawn: None,
        }
    }

    /// La pagina che si sta leggendo: quella a schermo, o quella in cima al nastro.
    pub fn here(&self) -> usize {
        match self.mode {
            Mode::Strip => self.strip.anchor,
            _ => self.page,
        }
    }

    pub fn zoomed(&self) -> bool {
        self.mode == Mode::Single && self.zoom > 1.0
    }

    /// Nessun giro di pagina o scorrimento in sospeso.
    pub fn gliding(&self) -> bool {
        self.glide.abs() >= 0.5
    }

    pub fn set_view(&mut self, w: u32, h: u32) {
        let old = self.strip_width();
        self.view = (w.max(1), h.max(1));
        self.rescale_strip(old);
        self.settle_strip_width();
    }

    pub fn target(&self) -> Target {
        let (w, h) = self.view;
        let fit = match self.mode {
            Mode::Single => Fit::Contain {
                width: (w as f32 * self.zoom).round() as u32,
                height: (h as f32 * self.zoom).round() as u32,
            },
            Mode::Double => Fit::Spread { width: w, height: h },
            Mode::Strip => Fit::Width(self.strip_width()),
        };
        Target { fit, linear: self.linear, trim: self.trim && self.mode != Mode::Strip, rotation: self.rotation }
    }

    /// La misura vera di una pagina, appena decodificata.
    pub fn known(&mut self, i: usize, native: (u32, u32)) {
        if i >= self.len || self.native[i] == Some(native) {
            return;
        }
        let old_h = self.strip_height(i);
        // la stima delle pagine ancora da vedere: la media di quelle note
        // (una misura che cambia, girata o rifilata, sostituisce la vecchia)
        if let Some((w, h)) = self.native[i].filter(|_| !self.broken[i]) {
            self.ratios.0 -= h as f64 / w as f64;
            self.ratios.1 -= 1;
        }
        self.ratios.0 += native.1 as f64 / native.0 as f64;
        self.ratios.1 += 1;
        self.native[i] = Some(native);
        if self.mode == Mode::Strip && i == self.strip.anchor {
            if self.anchor_frac.is_some() {
                self.settle_anchor();
            } else {
                self.strip.rescale_anchor(old_h, self.strip_height(i));
            }
        }
        self.settle_strip_width();
    }

    /// Un webtoon riconosciuto all'apertura: si legge a nastro, largo quanto
    /// dicono le sue strisce. `sizes`: le pagine gia' misurate.
    pub fn adopt_webtoon(&mut self, sizes: &[(usize, (u32, u32))]) {
        for &(i, size) in sizes {
            self.known(i, size);
        }
        if self.mode != Mode::Strip {
            self.act(Action::ToggleStrip, Instant::now());
        }
    }

    /// Una pagina che non si riesce a leggere: prende il posto che avrebbe (la
    /// forma media delle altre) e resta nera, e il titolo la segnala. Senza,
    /// non sarebbe mai pronta: a schermo resterebbe la pagina di prima, e dopo
    /// un salto la lettura anticipata aspetterebbe lei per sempre.
    pub fn failed(&mut self, i: usize) {
        if i >= self.len {
            return;
        }
        self.broken[i] = true;
        if self.native[i].is_none() {
            let ratio = if self.ratios.1 == 0 { 1.5 } else { self.ratios.0 / self.ratios.1 as f64 };
            self.native[i] = Some((1000, (1000.0 * ratio).round() as u32));
        }
    }

    fn go_to(&mut self, page: usize) {
        self.page = page;
        self.strip = Strip::at(page);
        self.anchor_frac = None;
    }

    fn landscape(&self, i: usize) -> bool {
        matches!(self.native.get(i), Some(Some((w, h))) if w > h)
    }

    /// Quante pagine forma il gruppo che comincia da `s`: nella doppia pagina
    /// due, tranne la copertina (se da sola), l'ultima, e le tavole gia'
    /// orizzontali, che stanno sempre da sole.
    fn group_len(&self, s: usize) -> usize {
        if self.mode != Mode::Double
            || (s == 0 && self.cover_alone)
            || s + 1 >= self.len
            || self.landscape(s)
            || self.landscape(s + 1)
        {
            1
        } else {
            2
        }
    }

    /// Il gruppo (inizio, quante pagine) che contiene `page`. Si ricalcola
    /// dall'inizio perche' una tavola orizzontale scoperta dopo sposta le
    /// coppie che seguono: con qualche migliaio di pagine e' questione di
    /// microsecondi.
    fn group_of(&self, page: usize) -> (usize, usize) {
        if self.mode != Mode::Double || self.len == 0 {
            return (page, 1);
        }
        let mut s = 0;
        loop {
            let n = self.group_len(s);
            if page < s + n || s + n >= self.len {
                return (s, n);
            }
            s += n;
        }
    }

    /// Le pagine che si vogliono a schermo adesso.
    fn wanted(&self) -> Vec<usize> {
        match self.mode {
            Mode::Strip => self.strip_visible().into_iter().map(|(i, _)| i).collect(),
            _ => {
                let (s, n) = self.group_of(self.page);
                (s..s + n).collect()
            }
        }
    }

    /// Si e' alla fine del volume: avanti non c'e' piu' niente (l'ultima
    /// pagina a schermo; nel nastro, il fondo dell'ultima sul bordo basso).
    pub fn at_end(&self) -> bool {
        if self.len == 0 {
            return false;
        }
        match self.mode {
            Mode::Strip => {
                let mut further = self.strip_now();
                let before = further;
                further.scroll(1.0, |i| self.strip_height(i), self.len, self.view.1 as f32);
                further == before && self.glide <= 0.5
            }
            _ => {
                let (s, n) = self.group_of(self.page);
                s + n >= self.len
            }
        }
    }

    /// Si e' all'inizio del volume: indietro non c'e' piu' niente (la prima
    /// pagina a schermo; nel nastro, la cima della prima sul bordo alto).
    pub fn at_start(&self) -> bool {
        if self.len == 0 {
            return false;
        }
        match self.mode {
            Mode::Strip => {
                let mut further = self.strip_now();
                let before = further;
                further.scroll(-1.0, |i| self.strip_height(i), self.len, self.view.1 as f32);
                further == before && self.glide >= -0.5
            }
            _ => self.group_of(self.page).0 == 0,
        }
    }

    /// Esegue un'azione del lettore. Restituisce `true` se si e' girata
    /// pagina (per misurare quanto ci mette ad arrivare sullo schermo).
    pub fn act(&mut self, action: Action, now: Instant) -> bool {
        let before = (self.page, self.mode);
        match action {
            Action::Next | Action::Prev | Action::First | Action::Last | Action::GoTo(_) => self.navigate(action),
            // nelle pagine la rotella gira pagina
            Action::Scroll(dy) if self.mode != Mode::Strip => {
                return self.act(if dy > 0.0 { Action::Next } else { Action::Prev }, now);
            }
            Action::Scroll(dy) => self.glide += dy,
            Action::ToggleStrip => self.toggle_strip(),
            Action::ToggleDouble => self.toggle_double(),
            Action::ToggleCover => {
                self.cover_alone = !self.cover_alone;
                self.page = self.group_of(self.page).0;
            }
            Action::ToggleManga => self.manga = !self.manga,
            Action::ToggleLinear => self.linear = !self.linear,
            Action::StripWider(k) => self.widen_strip(k),
            Action::Zoom { factor, x, y } => self.zoom_at(factor, x, y),
            Action::ZoomReset => self.zoom_to(Zoom::Page),
            Action::ZoomIn | Action::ZoomOut => {
                let k: f32 = if action == Action::ZoomIn { 1.25 } else { 1.0 / 1.25 };
                match self.mode {
                    Mode::Single => self.zoom_at(k, self.view.0 as f32 / 2.0, self.view.1 as f32 / 2.0),
                    Mode::Strip => self.widen_strip(k.sqrt()),
                    Mode::Double => {}
                }
            }
            Action::ZoomTo(z) => self.zoom_to(z),
            Action::Pan(dx, dy) => {
                if let Some((_, _, w, h)) = self.single_rect(self.page) {
                    self.center.0 -= dx / w;
                    self.center.1 -= dy / h;
                }
            }
            Action::Rotate(clockwise) => self.rotate(if clockwise { 90 } else { 270 }),
            Action::ToggleTrim => self.trim = !self.trim,
            Action::ToggleBookmark => self.toggle_bookmark(),
            // della finestra e dei volumi: le fa l'app
            Action::ToggleFullscreen
            | Action::Open
            | Action::OpenFolder
            | Action::Close
            | Action::AskPage
            | Action::ToggleHud
            | Action::ToggleLibrary
            | Action::AddLibraryFolder
            | Action::Quit
            | Action::ToggleThumbs
            | Action::ToggleSlideshow
            | Action::ToggleUpscale
            | Action::ToggleWebtoon
            | Action::SavePage
            | Action::CopyPage
            | Action::ToggleLens
            | Action::ToggleSettings => {}
        }
        self.clamp_center();
        let turned = self.mode != Mode::Strip && (self.page, self.mode) != before && before.1 == self.mode;
        if turned {
            // in fretta se il giro arriva meno di un quarto di secondo dopo il precedente
            self.hurry = self.last_turn.is_some_and(|t| now - t < Duration::from_millis(250));
            self.last_turn = Some(now);
        }
        turned
    }

    /// Avanti, indietro, all'inizio, alla fine, a una pagina. Nel nastro
    /// avanti e indietro scorrono di quasi una finestra.
    fn navigate(&mut self, action: Action) {
        let last = self.len.saturating_sub(1);
        match action {
            Action::Next | Action::Prev if self.mode == Mode::Strip => {
                let h = self.view.1 as f32 * 0.9;
                self.glide += if action == Action::Next { h } else { -h };
            }
            Action::Next => {
                let (s, n) = self.group_of(self.page);
                if s + n <= last {
                    self.page = s + n;
                    self.center.1 = 0.0; // pagina ingrandita: si riparte dall'alto
                }
                self.forward = true;
            }
            Action::Prev => {
                let (s, _) = self.group_of(self.page);
                if s > 0 {
                    self.page = self.group_of(s - 1).0;
                    self.center.1 = 1.0; // si torna indietro: dal fondo
                }
                self.forward = false;
            }
            Action::First | Action::Last => {
                self.go_to(if action == Action::First { 0 } else { self.group_of(last).0 });
                self.jumped = true;
            }
            Action::GoTo(p) => {
                let p = p.min(last);
                self.forward = p >= self.here();
                self.go_to(self.group_of(p).0);
                self.jumped = true;
            }
            _ => {}
        }
    }

    /// Dal nastro alle pagine (dalla pagina in cima) e viceversa.
    fn toggle_strip(&mut self) {
        self.glide = 0.0;
        self.zoom = 1.0;
        self.mode = match self.mode {
            Mode::Strip => {
                self.page = self.strip.anchor;
                Mode::Single
            }
            _ => {
                self.go_to(self.page);
                Mode::Strip
            }
        };
        self.settle_strip_width();
    }

    /// Dalla pagina singola alla doppia (dall'inizio della coppia) e viceversa.
    fn toggle_double(&mut self) {
        self.zoom = 1.0;
        let here = self.here();
        self.mode = if self.mode == Mode::Double { Mode::Single } else { Mode::Double };
        self.page = if self.mode == Mode::Double { self.group_of(here).0 } else { here };
    }

    /// Segna la pagina, o toglie il segno; le pagine segnate restano in ordine.
    fn toggle_bookmark(&mut self) {
        let here = self.here();
        match self.bookmarks.binary_search(&here) {
            Ok(at) => {
                self.bookmarks.remove(at);
            }
            Err(at) => self.bookmarks.insert(at, here),
        }
    }

    /// Gira le pagine di `degrees` in senso orario. Le misure note restano
    /// quelle delle pagine gia' pronte (non girate): ognuna cambia quando
    /// arriva la sua versione girata, cosi' pagina e misura vanno sempre
    /// insieme, e finche' la nuova non c'e' resta quella di prima.
    fn rotate(&mut self, degrees: u16) {
        self.rotation = (self.rotation + degrees) % 360;
        self.zoom = 1.0;
    }

    /// Altezza / larghezza tipica delle pagine (la media di quelle note).
    pub fn typical_ratio(&self) -> f32 {
        if self.ratios.1 == 0 { 1.5 } else { (self.ratios.0 / self.ratios.1 as f64) as f32 }
    }
}
