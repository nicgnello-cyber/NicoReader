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

    fn strip_width(&self) -> u32 {
        ((self.view.0 as f32 * self.strip_frac.unwrap_or(STRIP_FRAC)).round() as u32).max(64)
    }

    /// La larghezza naturale del nastro per queste strisce e questa finestra:
    /// al massimo una volta e mezza la loro misura vera, e al massimo il 60%
    /// della finestra. `None` finche' non si conoscono l'una e l'altra.
    fn natural_strip_frac(&self) -> Option<f32> {
        if self.view.0 <= 1 {
            return None;
        }
        let mut widths: Vec<u32> =
            (0..self.len).filter(|&i| !self.broken[i]).filter_map(|i| self.native[i].map(|n| n.0)).collect();
        if widths.is_empty() {
            return None;
        }
        widths.sort_unstable();
        let typical = widths[widths.len() / 2] as f32;
        Some((STRIP_GROW * typical / self.view.0 as f32).min(STRIP_FRAC).clamp(0.2, 1.0))
    }

    /// Nel nastro, la larghezza non ancora scelta diventa quella naturale
    /// appena la si puo' calcolare, e da li' resta: una colonna che cambiasse
    /// man mano che arrivano le pagine le farebbe rifare tutte.
    fn settle_strip_width(&mut self) {
        if self.mode == Mode::Strip
            && self.strip_frac.is_none()
            && let Some(f) = self.natural_strip_frac()
        {
            let old = self.strip_width();
            self.strip_frac = Some(f);
            self.rescale_strip(old);
        }
    }

    /// La larghezza del nastro e' cambiata: gli scostamenti in pixel si
    /// riscalano, e si resta nello stesso punto della pagina.
    fn rescale_strip(&mut self, old_width: u32) {
        let new = self.strip_width();
        if old_width > 0 && new != old_width {
            self.strip.offset *= new as f32 / old_width as f32;
            self.glide *= new as f32 / old_width as f32;
        }
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

    /// Pronta da disegnare: preparata e di misura nota, o illeggibile (nera).
    fn showable(&self, pages: &impl Pages, i: usize) -> bool {
        self.broken[i] || (pages.has(i) && self.native[i].is_some())
    }

    /// La posizione nel nastro com'e' adesso, anche se ancora una frazione.
    fn strip_now(&self) -> Strip {
        match self.anchor_frac {
            Some(f) => Strip { anchor: self.strip.anchor, offset: f * self.strip_height(self.strip.anchor) },
            None => self.strip,
        }
    }

    /// Da frazione a pixel, con l'altezza migliore che si conosce.
    fn settle_anchor(&mut self) {
        self.strip = self.strip_now();
        self.anchor_frac = None;
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

    /// Altezza a schermo di una pagina nel nastro; se non la si conosce ancora,
    /// quella media delle pagine gia' viste (i webtoon sono quasi tutti uguali).
    fn strip_height(&self, i: usize) -> f32 {
        let w = self.strip_width() as f32;
        match self.native.get(i).copied().flatten() {
            Some((nw, nh)) => nh as f32 * w / nw as f32,
            None if self.ratios.1 == 0 => w * 1.5,
            None => w * (self.ratios.0 / self.ratios.1 as f64) as f32,
        }
    }

    fn strip_visible(&self) -> Vec<(usize, f32)> {
        self.strip_now().visible(|i| self.strip_height(i), self.len, self.view.1 as f32)
    }

    /// Esegue un'azione del lettore. Restituisce `true` se si e' girata
    /// pagina (per misurare quanto ci mette ad arrivare sullo schermo).
    pub fn act(&mut self, action: Action, now: Instant) -> bool {
        let before = (self.page, self.mode);
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
            Action::Scroll(dy) => match self.mode {
                Mode::Strip => self.glide += dy,
                _ => return self.act(if dy > 0.0 { Action::Next } else { Action::Prev }, now),
            },
            Action::ToggleStrip => {
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
            Action::ToggleDouble => {
                self.zoom = 1.0;
                self.mode = if self.mode == Mode::Double { Mode::Single } else { Mode::Double };
                if self.mode == Mode::Double {
                    self.page = self.group_of(self.here()).0;
                } else {
                    self.page = self.here();
                }
            }
            Action::ToggleCover => {
                self.cover_alone = !self.cover_alone;
                self.page = self.group_of(self.page).0;
            }
            Action::ToggleManga => self.manga = !self.manga,
            Action::ToggleLinear => self.linear = !self.linear,
            Action::StripWider(k) => {
                let old = self.strip_width();
                self.strip_frac = Some((self.strip_frac.unwrap_or(STRIP_FRAC) * k).clamp(0.2, 1.0));
                self.rescale_strip(old);
            }
            Action::Zoom { factor, x, y } => self.zoom_at(factor, x, y),
            Action::ZoomReset => return self.act(Action::ZoomTo(Zoom::Page), now),
            Action::ZoomIn | Action::ZoomOut => {
                let k: f32 = if action == Action::ZoomIn { 1.25 } else { 1.0 / 1.25 };
                match self.mode {
                    Mode::Single => self.zoom_at(k, self.view.0 as f32 / 2.0, self.view.1 as f32 / 2.0),
                    Mode::Strip => return self.act(Action::StripWider(k.sqrt()), now),
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
            Action::ToggleBookmark => {
                let here = self.here();
                match self.bookmarks.binary_search(&here) {
                    Ok(at) => {
                        self.bookmarks.remove(at);
                    }
                    Err(at) => self.bookmarks.insert(at, here),
                }
            }
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

    /// Il rettangolo della pagina singola, con lo zoom: (x, y, larghezza, altezza).
    fn single_rect(&self, page: usize) -> Option<(f32, f32, f32, f32)> {
        let (nw, nh) = self.native.get(page).copied().flatten()?;
        let (vw, vh) = (self.view.0 as f32, self.view.1 as f32);
        let (w0, h0) = Fit::Contain { width: self.view.0, height: self.view.1 }.size(nw, nh);
        let (w, h) = (w0 as f32 * self.zoom, h0 as f32 * self.zoom);
        // lungo un asse: centrata se ci sta, altrimenti mai oltre i bordi
        let axis = |view: f32, size: f32, c: f32| {
            if size <= view { ((view - size) / 2.0).round() } else { (view / 2.0 - c * size).clamp(view - size, 0.0) }
        };
        Some((axis(vw, w, self.center.0), axis(vh, h, self.center.1), w, h))
    }

    fn zoom_at(&mut self, factor: f32, px: f32, py: f32) {
        if self.mode != Mode::Single {
            return;
        }
        let Some((x, y, w, h)) = self.single_rect(self.page) else { return };
        // il punto della pagina sotto il puntatore resta sotto il puntatore
        let (u, v) = ((px - x) / w, (py - y) / h);
        let zoom = (self.zoom * factor).clamp(1.0, MAX_ZOOM);
        let k = zoom / self.zoom;
        let (w2, h2) = (w * k, h * k);
        self.zoom = zoom;
        self.center = ((self.view.0 as f32 / 2.0 - px) / w2 + u, (self.view.1 as f32 / 2.0 - py) / h2 + v);
    }

    fn zoom_to(&mut self, z: Zoom) {
        let native = self.native.get(self.here()).copied().flatten();
        match self.mode {
            Mode::Single => {
                let Some((nw, nh)) = native else { return };
                let (w0, _) = Fit::Contain { width: self.view.0, height: self.view.1 }.size(nw, nh);
                self.zoom = match z {
                    Zoom::Page => 1.0,
                    Zoom::Width => self.view.0 as f32 / w0 as f32,
                    Zoom::Actual => nw as f32 / w0 as f32,
                }
                .clamp(1.0, MAX_ZOOM);
                // si parte dall'alto: e' li' che si comincia a leggere
                self.center = (0.5, 0.0);
            }
            Mode::Strip => {
                let old = self.strip_width();
                let natural = self.natural_strip_frac().unwrap_or(STRIP_FRAC);
                self.strip_frac = Some(
                    match (z, native) {
                        (Zoom::Page, _) | (Zoom::Actual, None) => natural,
                        (Zoom::Width, _) => 1.0,
                        (Zoom::Actual, Some((nw, _))) => nw as f32 / self.view.0 as f32,
                    }
                    .clamp(0.2, 1.0),
                );
                self.rescale_strip(old);
            }
            Mode::Double => {}
        }
    }

    /// Quanto e' ingrandita la pagina rispetto ai suoi pixel, in percento
    /// (100: un pixel della pagina per un pixel dello schermo). `None` dove
    /// lo zoom non si regola (la doppia pagina).
    pub fn zoom_percent(&self) -> Option<u32> {
        let (nw, _) = self.native.get(self.here()).copied().flatten()?;
        let shown = match self.mode {
            Mode::Single => self.single_rect(self.page)?.2,
            Mode::Strip => self.strip_width() as f32,
            Mode::Double => return None,
        };
        Some((shown / nw as f32 * 100.0).round() as u32)
    }

    /// Riporta il centro dove la pagina lo lascia davvero, dopo i bordi.
    fn clamp_center(&mut self) {
        if self.zoom <= 1.0 {
            self.zoom = 1.0;
            self.center = (0.5, 0.5);
            return;
        }
        if let Some((x, y, w, h)) = self.single_rect(self.page) {
            self.center = ((self.view.0 as f32 / 2.0 - x) / w, (self.view.1 as f32 / 2.0 - y) / h);
        }
    }

    /// Scorrimento morbido del nastro; `true` se c'e' ancora movimento.
    pub fn advance_glide(&mut self, dt: f32) -> bool {
        if self.mode != Mode::Strip || !self.gliding() {
            self.glide = 0.0;
            return false;
        }
        let step = glide_step(self.glide, dt);
        self.glide -= step;
        self.settle_anchor(); // si scorre: da qui in poi conta la posizione in pixel
        let before = self.strip;
        let heights: Vec<f32> = (0..self.len).map(|i| self.strip_height(i)).collect();
        self.strip.scroll(step, |i| heights[i], self.len, self.view.1 as f32);
        if self.strip == before && step.abs() >= 1.0 {
            self.glide = 0.0; // contro il bordo del volume
        }
        true
    }

    /// Le pagine volute sono tutte pronte (e se ne conosce la misura)?
    pub fn ready(&self, pages: &impl Pages) -> bool {
        let wanted = self.wanted();
        !wanted.is_empty() && wanted.iter().all(|&i| self.showable(pages, i))
    }

    /// Cosa va a schermo, e dove.
    pub fn layout(&self, pages: &impl Pages) -> Vec<Item> {
        match self.mode {
            Mode::Strip => {
                let sw = self.strip_width() as f32;
                let x = ((self.view.0 as f32 - sw) / 2.0).round();
                self.strip_visible()
                    .into_iter()
                    .map(|(i, y)| Item { page: i, x, y: y.round(), w: sw, h: self.strip_height(i).round() })
                    .collect()
            }
            _ => {
                // le pagine nuove non sono pronte: restano quelle di prima, non
                // un lampo nero (niente animazioni, ma nemmeno buchi)
                let ok = |s: usize| {
                    let (s, n) = self.group_of(s);
                    (s..s + n).all(|i| self.showable(pages, i))
                };
                let start = Some(self.page).filter(|&p| ok(p)).or(self.last_drawn.filter(|&p| ok(p)));
                start.map_or(Vec::new(), |s| self.paged_items(s))
            }
        }
    }

    fn paged_items(&self, start: usize) -> Vec<Item> {
        if self.mode == Mode::Single {
            return self.single_rect(start).map(|(x, y, w, h)| Item { page: start, x, y, w, h }).into_iter().collect();
        }
        let (s, n) = self.group_of(start);
        let mut order: Vec<usize> = (s..s + n).collect();
        if self.manga {
            order.reverse(); // da destra a sinistra: la prima pagina sta a destra
        }
        let fit = Fit::Spread { width: self.view.0, height: self.view.1 };
        let sizes: Vec<(u32, u32)> =
            order.iter().filter_map(|&i| self.native[i].map(|(w, h)| fit.size(w, h))).collect();
        let total: u32 = sizes.iter().map(|s| s.0).sum();
        let mut x = self.view.0.saturating_sub(total) / 2;
        order
            .into_iter()
            .zip(sizes)
            .map(|(i, (w, h))| {
                let item = Item {
                    page: i,
                    x: x as f32,
                    y: (self.view.1.saturating_sub(h) / 2) as f32,
                    w: w as f32,
                    h: h as f32,
                };
                x += w;
                item
            })
            .collect()
    }

    /// Dopo aver disegnato: se le pagine volute erano pronte, diventano quelle
    /// che restano a schermo in attesa delle prossime.
    pub fn drawn(&mut self, pages: &impl Pages) {
        if self.mode != Mode::Strip && self.ready(pages) {
            self.last_drawn = Some(self.page);
        }
    }

    /// Cosa preparare, dal piu' urgente, e per quale misura.
    pub fn prefetch(&mut self, pages: &impl Pages) -> (Target, Vec<usize>) {
        let target = self.target();
        let wanted = self.wanted();
        let (first, last) = (*wanted.iter().min().unwrap_or(&self.page), *wanted.iter().max().unwrap_or(&self.page));
        // chi sfoglia in fretta (tasto tenuto premuto: 30 pagine al secondo)
        // consuma 4 pagine di anticipo in 130 ms, quanto serve a prepararne
        // una tavola 4K: l'anticipo cresce con il ritmo
        let ahead = if self.hurry { 12 } else { 4 };
        let (ahead, behind) = match self.mode {
            Mode::Single => (ahead, 2),
            Mode::Double => (ahead * 2, 4),
            Mode::Strip => (ahead + 2, 3),
        };
        // dopo un salto (avvio, Inizio/Fine, nuovo volume) si prepara solo
        // quello che si vede: all'avvio, a batteria, dividere il processore con
        // le pagine successive la faceva arrivare in 600 ms invece di 160. Le
        // altre si chiedono appena arriva: ogni arrivo richiama questa funzione.
        if self.jumped && wanted.iter().all(|&i| pages.has(i) || self.broken[i]) {
            self.jumped = false;
        }
        let mut order: Vec<usize> = prefetch_order(first, last, self.len, self.forward, ahead, behind)
            .into_iter()
            .filter(|&i| !self.broken[i] && pages.prepared_for(i) != Some(target))
            .collect();
        if self.jumped {
            order.retain(|i| wanted.contains(i));
        }
        (target, order)
    }

    /// Il titolo della finestra, l'unica cosa scritta: volume, pagina, modi.
    pub fn title(&self, name: &str) -> String {
        let mut title = format!("{name} \u{2014} {} / {}", self.folio(), self.len);
        let mut extra = self.modes();
        if self.mode == Mode::Single {
            extra.remove(0); // la pagina singola e' il modo di sempre: non si scrive
        }
        if self.broken_here() {
            extra.push(t("pagina illeggibile", "unreadable page"));
        }
        for what in extra {
            title += " \u{00b7} ";
            title += what;
        }
        title
    }

    pub fn pages(&self) -> usize {
        self.len
    }

    /// Le pagine a schermo, per chi legge: "24" o "24–25".
    pub fn folio(&self) -> String {
        let (s, n) = self.group_of(self.here());
        if n == 2 { format!("{}\u{2013}{}", s + 1, s + 2) } else { format!("{}", self.here() + 1) }
    }

    /// Come si sta leggendo: prima il modo, poi cio' che vi si aggiunge.
    pub fn modes(&self) -> Vec<&'static str> {
        let mut v = vec![match self.mode {
            Mode::Single => t("pagina singola", "single page"),
            Mode::Double => t("doppia pagina", "two pages"),
            Mode::Strip => t("nastro", "strip"),
        }];
        for (on, what) in [
            (self.mode == Mode::Double && self.cover_alone, t("copertina sola", "cover alone")),
            (self.manga, "manga"),
            (self.zoom > 1.0, "zoom"),
            (self.trim && self.mode != Mode::Strip, t("rifilata", "trimmed")),
            (self.rotation != 0, t("girata", "rotated")),
            (!self.linear, "gamma"),
        ] {
            if on {
                v.push(what);
            }
        }
        v
    }

    /// Fra le pagine a schermo ce n'e' una che non si riesce a leggere.
    pub fn broken_here(&self) -> bool {
        self.wanted().iter().any(|&i| self.broken[i])
    }

    pub fn cover_alone(&self) -> bool {
        self.cover_alone
    }

    pub fn bookmarks(&self) -> &[usize] {
        &self.bookmarks
    }

    /// La pagina a schermo e' segnata (nella doppia pagina, una delle due;
    /// nel nastro, quella in cima).
    pub fn bookmarked(&self) -> bool {
        let here = match self.mode {
            Mode::Strip => vec![self.strip.anchor],
            _ => self.wanted(),
        };
        here.iter().any(|p| self.bookmarks.binary_search(p).is_ok())
    }

    /// Le pagine di cui si vede almeno un pezzo.
    pub fn on_screen(&self) -> Vec<usize> {
        self.wanted()
    }

    /// La misura vera di una pagina (rifilata e girata), se la si conosce.
    pub fn native(&self, i: usize) -> Option<(u32, u32)> {
        self.native.get(i).copied().flatten()
    }

    /// Lo stato da ricordare per la prossima volta.
    pub fn snapshot(&self) -> Saved {
        let strip = self.strip_now();
        let anchor_h = self.strip_height(strip.anchor).max(1.0);
        Saved {
            page: self.here(),
            pages: self.len,
            mode: match self.mode {
                Mode::Single => "pagina",
                Mode::Double => "doppia",
                Mode::Strip => "nastro",
            }
            .into(),
            manga: self.manga,
            cover_alone: self.cover_alone,
            strip_width: self.strip_frac.unwrap_or(0.0),
            strip_offset: (strip.offset / anchor_h).clamp(0.0, 1.0),
            read_at: 0,
            rotation: self.rotation,
            bookmarks: self.bookmarks.clone(),
        }
    }

    /// Riprende da dove si era rimasti. Se il volume nel frattempo ha perso
    /// pagine, si resta dentro i limiti.
    pub fn restore(&mut self, saved: &Saved) {
        if self.len == 0 {
            return;
        }
        self.page = saved.page.min(self.len - 1);
        self.mode = match saved.mode.as_str() {
            "doppia" => Mode::Double,
            "nastro" => Mode::Strip,
            _ => Mode::Single,
        };
        self.manga = saved.manga;
        self.cover_alone = saved.cover_alone;
        if saved.strip_width > 0.0 {
            self.strip_frac = Some(saved.strip_width.clamp(0.2, 1.0));
        }
        self.rotation = saved.rotation % 360 / 90 * 90;
        self.bookmarks = saved.bookmarks.iter().copied().filter(|&p| p < self.len).collect();
        self.bookmarks.sort_unstable();
        self.bookmarks.dedup();
        self.go_to(self.group_of(self.page).0);
        // l'altezza vera della pagina arrivera' dopo: fino ad allora la
        // posizione resta una frazione della pagina (vedi `anchor_frac`)
        self.anchor_frac = Some(saved.strip_offset.clamp(0.0, 1.0));
        self.jumped = true;
    }

    /// Per la prova automatica, quando qualcosa si incastra.
    pub fn describe(&self, pages: &impl Pages) -> String {
        format!(
            "modo {:?}, pagina {}, nastro {:?}, scorrimento da fare {:.1} px, pronta {}",
            self.mode,
            self.page + 1,
            self.strip_now(),
            self.glide,
            self.ready(pages)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Pagine finte: quali sono pronte e per quale misura.
    #[derive(Default)]
    struct Fake(HashMap<usize, Target>);

    impl Pages for Fake {
        fn prepared_for(&self, i: usize) -> Option<Target> {
            self.0.get(&i).copied()
        }
    }

    /// Un volume di `n` pagine verticali 1000x1500, finestra 1920x1080.
    fn reader(n: usize) -> Reader {
        let mut r = Reader::new(n, 0);
        r.set_view(1920, 1080);
        for i in 0..n {
            r.known(i, (1000, 1500));
        }
        r
    }

    fn ready_all(r: &Reader, pages: impl IntoIterator<Item = usize>) -> Fake {
        Fake(pages.into_iter().map(|i| (i, r.target())).collect())
    }

    fn now() -> Instant {
        Instant::now()
    }

    #[test]
    fn dopo_un_salto_prima_solo_la_pagina_a_schermo() {
        let mut r = reader(50);
        let (_, order) = r.prefetch(&Fake::default());
        assert_eq!(order, [0], "all'avvio si prepara solo la pagina da guardare");
        let pages = ready_all(&r, [0]);
        let (_, order) = r.prefetch(&pages);
        assert_eq!(order, [1, 2, 3, 4], "arrivata la prima, le successive");
    }

    #[test]
    fn le_pagine_pronte_non_si_richiedono() {
        let mut r = reader(50);
        let pages = ready_all(&r, [0, 1, 2]);
        let (_, order) = r.prefetch(&pages);
        assert_eq!(order, [3, 4]);
    }

    #[test]
    fn chi_sfoglia_in_fretta_prepara_piu_lontano() {
        let mut r = reader(50);
        let t = now();
        r.act(Action::Next, t);
        r.act(Action::Next, t + Duration::from_millis(40));
        let pages = ready_all(&r, [2]);
        let (_, order) = r.prefetch(&pages);
        assert_eq!(order.len(), 12 + 2, "12 avanti e 2 indietro");
    }

    #[test]
    fn niente_lampo_nero_se_la_pagina_nuova_non_e_pronta() {
        let mut r = reader(10);
        let pages = ready_all(&r, [0]);
        r.drawn(&pages);
        r.act(Action::Next, now());
        let items = r.layout(&pages);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].page, 0, "resta la pagina di prima");
        assert!(!r.ready(&pages));
    }

    #[test]
    fn pagina_illeggibile_non_blocca_il_lettore() {
        let mut r = Reader::new(10, 0);
        r.set_view(1920, 1080);
        r.failed(0);
        let (_, order) = r.prefetch(&Fake::default());
        assert_eq!(order, [1, 2, 3, 4], "si va oltre, senza richiederla");
        assert!(r.ready(&Fake::default()), "a schermo va lei (nera), non la pagina di prima");
        assert!(r.title("x").ends_with(t("pagina illeggibile", "unreadable page")));
        r.act(Action::ToggleDouble, now());
        r.known(1, (1000, 1500));
        let pages = ready_all(&r, [1]);
        let items = r.layout(&pages);
        assert_eq!(items.iter().map(|it| it.page).collect::<Vec<_>>(), [0, 1], "la compagna resta al suo posto");
    }

    #[test]
    fn vai_a_pagina_nella_doppia_parte_dalla_coppia() {
        let mut r = reader(10);
        r.act(Action::ToggleDouble, now());
        r.act(Action::GoTo(6), now());
        assert_eq!((r.here(), r.folio()), (6, "7\u{2013}8".to_string()), "la coppia che contiene la pagina 7");
        r.act(Action::GoTo(99), now());
        assert_eq!(r.here(), 8, "oltre la fine: l'ultima coppia");
        let (_, order) = r.prefetch(&Fake::default());
        assert_eq!(order, [8, 9], "dopo il salto, solo le pagine a schermo");
    }

    #[test]
    fn zoom_a_larghezza_e_a_pixel_reali() {
        let mut r = reader(10); // pagine 1000x1500 in 1920x1080: intera e' 720 di larghezza
        assert_eq!(r.zoom_percent(), Some(72));
        r.act(Action::ZoomTo(Zoom::Actual), now());
        assert_eq!(r.zoom_percent(), Some(100));
        r.act(Action::ZoomTo(Zoom::Width), now());
        assert_eq!(r.zoom_percent(), Some(192), "larga quanto la finestra");
        r.act(Action::ZoomTo(Zoom::Page), now());
        assert_eq!(r.zoom_percent(), Some(72));
        r.act(Action::ZoomOut, now());
        assert_eq!(r.zoom_percent(), Some(72), "mai piu' piccola della pagina intera");
        r.act(Action::ToggleDouble, now());
        assert_eq!(r.zoom_percent(), None, "nella doppia pagina lo zoom non si regola");
    }

    #[test]
    fn nel_nastro_lo_zoom_e_la_larghezza() {
        let mut r = reader(10);
        r.act(Action::ToggleStrip, now());
        let before = r.zoom_percent().unwrap();
        r.act(Action::ZoomIn, now());
        assert!(r.zoom_percent().unwrap() > before);
        r.act(Action::ZoomTo(Zoom::Actual), now());
        assert_eq!(r.zoom_percent(), Some(100));
    }

    #[test]
    fn doppia_pagina_coppie_e_copertina() {
        let mut r = reader(7);
        r.act(Action::ToggleDouble, now());
        assert_eq!(r.group_of(0), (0, 2));
        assert_eq!(r.group_of(5), (4, 2));
        assert_eq!(r.group_of(6), (6, 1), "l'ultima dispari sta da sola");
        r.act(Action::ToggleCover, now());
        assert_eq!(r.group_of(0), (0, 1));
        assert_eq!(r.group_of(2), (1, 2));
    }

    #[test]
    fn doppia_pagina_tavola_orizzontale_da_sola() {
        let mut r = reader(6);
        r.known(2, (3000, 1500)); // una tavola doppia gia' unita
        r.act(Action::ToggleDouble, now());
        assert_eq!(r.group_of(0), (0, 2));
        assert_eq!(r.group_of(2), (2, 1));
        assert_eq!(r.group_of(3), (3, 2), "le coppie ripartono dopo");
        r.act(Action::Next, now());
        assert_eq!(r.page, 2);
        r.act(Action::Next, now());
        assert_eq!(r.page, 3);
        r.act(Action::Prev, now());
        assert_eq!(r.page, 2);
    }

    #[test]
    fn doppia_pagina_manga_da_destra() {
        let mut r = reader(4);
        r.act(Action::ToggleDouble, now());
        let pages = ready_all(&r, 0..4);
        let normal = r.layout(&pages);
        assert_eq!((normal[0].page, normal[1].page), (0, 1));
        assert_eq!(normal[0].x + normal[0].w, normal[1].x, "affiancate senza fessura");
        r.act(Action::ToggleManga, now());
        let manga = r.layout(&pages);
        assert_eq!((manga[0].page, manga[1].page), (1, 0), "la prima a destra");
    }

    /// Ingrandita tre volte la pagina supera la finestra in entrambe le
    /// direzioni, e un punto non troppo vicino al bordo resta dov'era. (Se la
    /// pagina restasse piu' stretta della finestra starebbe centrata, e il
    /// punto si sposterebbe: giustamente.)
    #[test]
    fn zoom_tiene_fermo_il_punto_sotto_il_puntatore() {
        let mut r = reader(3);
        let (x, y, w, h) = r.single_rect(0).unwrap();
        let (px, py) = (x + w * 0.45, y + h * 0.45);
        r.act(Action::Zoom { factor: 3.0, x: px, y: py }, now());
        let (x2, y2, w2, h2) = r.single_rect(0).unwrap();
        assert!(
            ((px - x2) / w2 - 0.45).abs() < 1e-3 && ((py - y2) / h2 - 0.45).abs() < 1e-3,
            "il punto e' scivolato via"
        );
        assert!(r.zoomed());
    }

    #[test]
    fn zoom_non_mostra_mai_il_vuoto_oltre_i_bordi() {
        let mut r = reader(3);
        r.act(Action::Zoom { factor: 3.0, x: 960.0, y: 540.0 }, now());
        r.act(Action::Pan(-99_999.0, -99_999.0), now());
        let (x, y, w, h) = r.single_rect(0).unwrap();
        assert_eq!(x + w, 1920.0, "bordo destro della pagina sul bordo della finestra");
        assert_eq!(y + h, 1080.0);
        r.act(Action::ZoomReset, now());
        assert!(!r.zoomed());
    }

    #[test]
    fn girando_pagina_ingrandita_si_riparte_dall_alto() {
        let mut r = reader(3);
        r.act(Action::Zoom { factor: 2.0, x: 960.0, y: 540.0 }, now());
        r.act(Action::Pan(0.0, -99_999.0), now());
        r.act(Action::Next, now());
        assert_eq!(r.single_rect(1).unwrap().1, 0.0, "la pagina nuova parte dal suo bordo alto");
    }

    #[test]
    fn allargare_il_nastro_non_sposta_il_punto_di_lettura() {
        let mut r = reader(10);
        r.act(Action::ToggleStrip, now());
        r.strip = Strip { anchor: 3, offset: 500.0 };
        let before = r.strip.offset / r.strip_height(3);
        r.act(Action::StripWider(1.5), now());
        let after = r.strip.offset / r.strip_height(3);
        assert!((before - after).abs() < 1e-4, "da {before} a {after}");
    }

    #[test]
    fn riprende_dove_si_era_rimasti() {
        let mut r = reader(30);
        r.act(Action::ToggleStrip, now());
        r.strip = Strip { anchor: 12, offset: r.strip_height(12) * 0.4 };
        r.manga = true;
        let saved = r.snapshot();

        let mut again = Reader::new(30, 0);
        again.restore(&saved);
        again.set_view(1920, 1080);
        assert_eq!((again.mode, again.here(), again.manga), (Mode::Strip, 12, true));
        again.known(12, (1000, 1500));
        let frac = again.strip.offset / again.strip_height(12);
        assert!((frac - 0.4).abs() < 1e-3, "punto dentro la pagina perso: {frac}");
    }

    /// Il difetto trovato dalla prova: dopo la ripresa la pagina d'ancora non e'
    /// ancora decodificata, e la stima della sua altezza cambia man mano che
    /// arrivano le altre. La posizione deve restare "il 40% della pagina", non
    /// un numero di pixel calcolato sulla prima stima.
    #[test]
    fn ripresa_nel_nastro_con_stime_che_cambiano() {
        let saved = Saved { page: 12, pages: 30, mode: "nastro".into(), strip_offset: 0.4, ..Default::default() };
        let mut r = Reader::new(30, 0);
        r.restore(&saved);
        r.set_view(1920, 1080);
        // arrivano prima altre pagine, orizzontali: la stima cambia molto
        for i in [13, 14, 15] {
            r.known(i, (3840, 2160));
        }
        r.known(12, (3840, 2160));
        let s = r.strip_now();
        let frac = s.offset / r.strip_height(12);
        assert_eq!(s.anchor, 12);
        assert!((frac - 0.4).abs() < 1e-3, "si riparte dal punto sbagliato: {frac}");
    }

    #[test]
    fn volume_accorciato_resta_nei_limiti() {
        let saved = Saved { page: 500, pages: 600, mode: "pagina".into(), ..Default::default() };
        let mut r = Reader::new(20, 0);
        r.restore(&saved);
        assert_eq!(r.here(), 19);
    }

    #[test]
    fn riconosce_i_webtoon() {
        let strips = [(800, 12_000), (800, 11_500), (800, 3000), (720, 900)];
        assert_eq!(webtoon_width(&strips), Some(800), "la mediana e' una striscia");
        assert_eq!(webtoon_width(&[(1000, 1500); 5]), None, "pagine di fumetto");
        assert_eq!(webtoon_width(&[(800, 12_000); 2]), None, "due pagine non bastano");
    }

    /// La larghezza di partenza del nastro: 1,5 volte le strisce, mai oltre
    /// il 60% della finestra. Strisce da 720 in 1920: 1080 invece di 1152.
    #[test]
    fn il_nastro_parte_dalla_misura_delle_strisce() {
        let sizes: Vec<(usize, (u32, u32))> = (0..5).map(|i| (i, (720, 10_000))).collect();
        let mut r = Reader::new(40, 0);
        r.adopt_webtoon(&sizes); // all'apertura la finestra non ha ancora la sua misura
        assert_eq!(r.mode, Mode::Strip);
        r.set_view(1920, 1080);
        assert_eq!(r.strip_width(), 1080);
        let mut wide = Reader::new(40, 0);
        wide.adopt_webtoon(&(0..5).map(|i| (i, (1600, 10_000))).collect::<Vec<_>>());
        wide.set_view(1920, 1080);
        assert_eq!(wide.strip_width(), 1152, "strisce grandi: il 60% della finestra");
        r.act(Action::StripWider(1.2), now());
        r.act(Action::ZoomTo(Zoom::Page), now());
        assert_eq!(r.strip_width(), 1080, "0 torna alla larghezza naturale");
        assert_eq!(r.snapshot().strip_width, 1080.0 / 1920.0);
    }

    #[test]
    fn girare_cambia_il_bersaglio_e_si_ricorda() {
        let mut r = reader(4); // 1000x1500
        let before = r.single_rect(0).unwrap();
        r.act(Action::Rotate(true), now());
        assert_eq!(r.target().rotation, 90);
        assert_eq!(r.single_rect(0).unwrap(), before, "finche' la pagina girata non arriva, resta quella di prima");
        r.known(0, (1500, 1000));
        let after = r.single_rect(0).unwrap();
        assert!(after.2 > after.3 && before.2 < before.3, "ora e' orizzontale: {after:?}");
        let saved = r.snapshot();
        assert_eq!(saved.rotation, 90);
        r.act(Action::Rotate(false), now());
        assert_eq!(r.target().rotation, 0);
        let mut again = Reader::new(4, 0);
        again.restore(&saved);
        assert_eq!(again.target().rotation, 90);
    }

    #[test]
    fn segnalibri() {
        let mut r = reader(10);
        r.act(Action::GoTo(4), now());
        r.act(Action::ToggleBookmark, now());
        r.act(Action::GoTo(1), now());
        r.act(Action::ToggleBookmark, now());
        assert_eq!(r.bookmarks(), [1, 4], "in ordine");
        assert!(r.bookmarked());
        r.act(Action::ToggleBookmark, now());
        assert_eq!(r.bookmarks(), [4], "di nuovo: tolto");
        let mut again = Reader::new(10, 0);
        again.restore(&r.snapshot());
        assert_eq!(again.bookmarks(), [4]);
    }

    #[test]
    fn il_rifilo_non_tocca_il_nastro() {
        let mut r = reader(10);
        r.act(Action::ToggleTrim, now());
        assert!(r.target().trim);
        assert!(r.modes().contains(&t("rifilata", "trimmed")));
        r.act(Action::ToggleStrip, now());
        assert!(!r.target().trim, "nel nastro il bianco fra le vignette fa parte del disegno");
    }
}
