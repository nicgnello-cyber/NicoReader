//! La libreria: le copertine in una griglia sul nero, come quadri appesi.
//!
//! In alto la barra della libreria: il nome, i filtri (tutti, in lettura, da
//! leggere, letti), la ricerca (basta scrivere), le cartelle. Sotto, se non
//! si cerca e non si filtra, prima una fila "Continua a leggere", poi tutta
//! la libreria con le serie raccolte in una pila; un clic sulla pila la apre.
//! Un tasto della barra mostra invece i gruppi delle schede e dei server:
//! raccolte, liste di lettura, generi, editori, ognuno una pila da aprire.
//! Un altro mostra la libreria per cartelle, come su disco: prima le
//! sottocartelle (con la copertina del primo volume), poi i volumi, in ordine
//! naturale, e in alto il percorso per risalire.
//! Le celle si allargano per riempire la finestra: niente vuoti a destra.
//!
//! Come `ui`, qui non c'e' grafica: si decide cosa sta dove, cosa scrivere e
//! cosa fare di tasti e clic. Le copertine le disegna la scheda video (sono
//! immagini, come le pagine) nei riquadri che dice [`Shelf::cover_slots`]; il
//! resto e' una [`Layer`] dell'interfaccia sopra di loro.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::Instant;

use fumetto_core::LibrarySort;
use fumetto_core::library::{self, Entry, GroupKind, Status, by_number};
use fumetto_core::lingua::{italian, t};
use fumetto_core::natural_cmp;
use fumetto_core::remote::{self, Server};
use fumetto_render::{Align, Face, Layer, Measure, Rect, Text};

use crate::reader::Action;
use crate::strip::glide_step;
use crate::ui::{
    self, ACCENT, BLACK, Command, HAIR, Handled, INK, INK_F, Icon, Key, MUTED, MUTED_F, PLATE, TICK, alpha, caps,
    draw_icon, fade, fit,
};

/// Cio' che la libreria deve sapere, dall'app.
pub struct ShelfData<'a> {
    /// I volumi della libreria e, da `found_from`, quelli trovati cercando sui
    /// server che la libreria non ha.
    pub entries: &'a [Entry],
    pub found_from: usize,
    /// Si aspetta la risposta dei server alla ricerca.
    pub searching: bool,
    /// Allineati con `entries`: dove si e' arrivati, e quando.
    pub status: &'a [Status],
    pub read_at: &'a [u64],
    /// Scaricato dal server: si legge anche senza rete (puo' essere vuoto).
    pub offline: &'a [bool],
    /// I dati sono corretti a mano (puo' essere vuoto).
    pub edited: &'a [bool],
    /// La scansione delle cartelle e' in corso.
    pub scanning: bool,
    pub roots: &'a [PathBuf],
    /// Quanto si vede la copertina di questo volume: 0 non e' ancora sulla
    /// scheda video, 1 del tutto; in mezzo sta comparendo.
    pub cover_shown: &'a dyn Fn(&Path) -> f32,
    /// Dietro la libreria c'e' un volume aperto: si puo' tornare a leggerlo.
    pub reading: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    Reading,
    New,
    Done,
}

impl ShelfData<'_> {
    /// Quanti volumi ha la libreria (senza quelli trovati solo sul server).
    fn library_len(&self) -> usize {
        self.found_from.min(self.entries.len())
    }
}

impl Filter {
    const ALL: [Filter; 4] = [Filter::All, Filter::Reading, Filter::New, Filter::Done];

    fn label(self) -> &'static str {
        match self {
            Filter::All => t("Tutti", "All"),
            Filter::Reading => t("In lettura", "Reading"),
            Filter::New => t("Da leggere", "Unread"),
            Filter::Done => t("Letti", "Read"),
        }
    }

    fn keeps(self, s: Status) -> bool {
        match self {
            Filter::All => true,
            Filter::Reading => matches!(s, Status::Reading(..)),
            Filter::New => s == Status::New,
            Filter::Done => s == Status::Done,
        }
    }
}

/// Una cella: un volume, o una serie raccolta in una pila.
#[derive(Clone, Debug, PartialEq)]
pub enum Tile {
    Volume(usize),
    Series {
        name: String,
        cover: usize,
        count: usize,
        done: usize,
    },
    /// Una raccolta, una lista di lettura, un genere, un editore.
    Group {
        kind: GroupKind,
        name: String,
        cover: usize,
        count: usize,
        done: usize,
    },
    /// Nella vista per cartelle: una cartella, o un server.
    Folder {
        path: PathBuf,
        name: String,
        cover: usize,
        count: usize,
        done: usize,
    },
}

impl Tile {
    /// Il volume di cui si mostra la copertina.
    fn cover(&self) -> usize {
        match self {
            Tile::Volume(i) => *i,
            Tile::Series { cover, .. } | Tile::Group { cover, .. } | Tile::Folder { cover, .. } => *cover,
        }
    }

    /// Quanti volumi ha la cella, e quanti letti.
    fn counts(&self, status: &[Status]) -> (usize, usize) {
        match self {
            Tile::Volume(i) => (1, usize::from(status[*i] == Status::Done)),
            Tile::Series { count, done, .. } | Tile::Group { count, done, .. } | Tile::Folder { count, done, .. } => {
                (*count, *done)
            }
        }
    }
}

/// Cio' che sta sotto il mouse.
#[derive(Clone, Debug, PartialEq)]
pub enum Hit {
    Tile(usize),
    Filter(Filter),
    ClearSearch,
    Folders,
    Groups,
    /// Il tasto della vista per cartelle.
    Browse,
    /// Il tasto dell'ordine.
    Sort,
    /// Un pezzo del percorso, per risalire (indice in `Shelf::crumbs`).
    Crumb(usize),
    Return,
    Back,
    Bar,
}

/// Una cella al suo posto, in pixel della finestra (gia' scorsa).
pub struct Placed {
    pub tile: Tile,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Un pezzo del percorso in cima alla vista per cartelle: da `x0` a `x1`;
/// porta a `to` (`None`: l'inizio).
#[derive(Clone, Debug, Default)]
struct Crumb {
    x0: f32,
    x1: f32,
    to: Option<PathBuf>,
}

struct Section {
    title: String,
    tiles: Vec<Tile>,
    /// Solo una fila (continua a leggere).
    one_row: bool,
}

struct Layout {
    placed: Vec<Placed>,
    /// Titoli delle sezioni: testo e linea di base.
    heads: Vec<(String, f32)>,
    /// Dentro una serie, un gruppo o fra i gruppi: la cima dell'intestazione.
    head: Option<f32>,
    /// Quanto e' alto tutto il contenuto, per fermare lo scorrimento.
    height: f32,
    /// Quanti volumi in tutte le sezioni, e quanti letti.
    totals: (usize, usize),
}

/// Le misure della griglia per questa finestra.
struct Geo {
    s: f32,
    view: (f32, f32),
    top: f32,
    pad: f32,
    gap: f32,
    cols: usize,
    tile_w: f32,
    tile_h: f32,
    row_h: f32,
}

impl Geo {
    fn new(view: (f32, f32), s: f32) -> Geo {
        let pad = (44.0 * s).round();
        let gap = (30.0 * s).round();
        let avail = view.0 - 2.0 * pad;
        let cols = (((avail + gap) / (170.0 * s + gap)).floor() as usize).max(2);
        let tile_w = ((avail - (cols - 1) as f32 * gap) / cols as f32).floor();
        let tile_h = (tile_w * 1.5).round();
        Geo { s, view, top: ui::hud_height(s), pad, gap, cols, tile_w, tile_h, row_h: tile_h + (78.0 * s).round() }
    }
}

/// Dove va una copertina, in pixel della finestra.
#[derive(Clone, Debug, PartialEq)]
pub struct CoverSlot {
    pub path: PathBuf,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Il raggio degli angoli.
    pub radius: f32,
    /// Si vede adesso (le altre si preparano prima che servano).
    pub on_screen: bool,
}

/// Il raggio degli angoli delle copertine: appena arrotondati, come la carta.
fn corner(s: f32) -> f32 {
    (3.0 * s).round()
}

#[derive(Default)]
pub struct Shelf {
    pub filter: Filter,
    pub search: String,
    /// Dentro una serie: il suo nome.
    pub series: Option<String>,
    /// Si guardano i gruppi invece della libreria.
    pub groups: bool,
    /// Dentro un gruppo (si arriva dalla vista dei gruppi).
    pub group: Option<(GroupKind, String)>,
    /// La libreria per cartelle, come su disco, invece che per serie.
    pub folders: bool,
    /// Nella vista per cartelle, la cartella (o il server) che si guarda;
    /// `None`: l'inizio.
    pub folder: Option<PathBuf>,
    /// L'ordine delle copertine.
    pub sort: LibrarySort,
    /// Il percorso disegnato in cima, per i clic: dove sta ogni pezzo e dove
    /// porta. Lo scrive `layers`, che misura i testi.
    crumbs: RefCell<Vec<Crumb>>,
    scroll: f32,
    /// Pixel di scorrimento ancora da percorrere (come nel nastro).
    glide: f32,
    last: Option<Instant>,
    /// La cella scelta con la tastiera (indice fra quelle disposte).
    selected: Option<usize>,
    hover: Option<Hit>,
    /// Le celle che si alzano sotto il mouse, o che tornano giu': quanto
    /// sono alzate, da 0 a 1.
    lifts: Vec<(usize, f32)>,
    lifted_at: Option<Instant>,
}

mod draw;
mod input;
mod menus;
mod sections;
#[cfg(test)]
mod tests;

#[cfg(test)]
use draw::BarGeo;
use draw::bar_hit;
use sections::{folder_name, groups_title, kind_label, sort_label, sources};

impl Shelf {
    /// Non si e' nella libreria intera: c'e' un'intestazione per tornarci.
    fn inside(&self, d: &ShelfData) -> bool {
        self.series.is_some() || self.group.is_some() || self.groups || (self.folders && !self.folder_here(d).1)
    }

    /// Nella vista per cartelle, la cartella (o il server) da mostrare, e se
    /// e' l'inizio. Con una sola cartella della libreria (e nessun server)
    /// l'inizio e' lei; con piu' d'una, l'elenco delle cartelle (`None`).
    /// Una cartella che non e' piu' nella libreria riporta all'inizio.
    fn folder_here(&self, d: &ShelfData) -> (Option<PathBuf>, bool) {
        let sources = sources(d);
        let only = (sources.len() == 1).then(|| sources[0].clone());
        match &self.folder {
            Some(p) if sources.iter().any(|s| p.starts_with(s)) && Some(p) != only.as_ref() => (Some(p.clone()), false),
            _ => (only, true),
        }
    }

    /// La cartella sopra quella che si guarda (`None`: l'inizio).
    fn parent_folder(&self, d: &ShelfData) -> Option<PathBuf> {
        let (Some(here), false) = self.folder_here(d) else { return None };
        let sources = sources(d);
        let parent = here.parent().filter(|_| !sources.contains(&here))?;
        // la cartella sola della libreria e' l'inizio
        (sources.len() > 1 || parent != sources[0]).then(|| parent.to_path_buf())
    }

    /// Il percorso dall'inizio alla cartella sopra quella che si guarda:
    /// nome e dove porta ciascun pezzo.
    fn trail(&self, d: &ShelfData) -> Vec<(String, Option<PathBuf>)> {
        let sources = sources(d);
        let (Some(here), false) = self.folder_here(d) else { return Vec::new() };
        let mut trail = Vec::new();
        if sources.len() > 1 {
            trail.push((t("Cartelle", "Folders").to_owned(), None));
        }
        if let Some(source) = sources.iter().find(|s| here.starts_with(s)) {
            let mut up: Vec<&Path> = here.ancestors().skip(1).take_while(|a| a.starts_with(source)).collect();
            up.reverse();
            for a in up {
                let to = (sources.len() > 1 || a != source.as_path()).then(|| a.to_path_buf());
                trail.push((folder_name(a), to));
            }
        }
        trail
    }

    pub fn enter_folder(&mut self, path: PathBuf) {
        (self.folders, self.folder, self.series) = (true, Some(path), None);
        self.reset_view();
    }

    fn layout(&self, d: &ShelfData, g: &Geo) -> Layout {
        let s = g.s;
        let mut y = g.top + (30.0 * s).round() - self.scroll;
        let start = y;
        let mut placed = Vec::new();
        let mut heads = Vec::new();
        let mut head = None;
        if self.inside(d) {
            head = Some(y);
            y += (128.0 * s).round();
        }
        let mut totals = (0, 0);
        for sec in self.sections(d) {
            for tile in &sec.tiles {
                let (n, done) = tile.counts(d.status);
                totals = (totals.0 + n, totals.1 + done);
            }
            if !sec.title.is_empty() {
                heads.push((sec.title.clone(), y + (18.0 * s).round()));
                y += (44.0 * s).round();
            }
            let n = if sec.one_row { sec.tiles.len().min(g.cols) } else { sec.tiles.len() };
            for (k, tile) in sec.tiles.into_iter().take(n).enumerate() {
                let (row, col) = (k / g.cols, k % g.cols);
                placed.push(Placed {
                    tile,
                    x: g.pad + col as f32 * (g.tile_w + g.gap),
                    y: y + row as f32 * g.row_h,
                    w: g.tile_w,
                    h: g.tile_h,
                });
            }
            y += n.div_ceil(g.cols) as f32 * g.row_h + (18.0 * s).round();
        }
        Layout { placed, heads, head, height: y - start + (40.0 * s).round(), totals }
    }

    fn max_scroll(&self, d: &ShelfData, g: &Geo) -> f32 {
        let at = Shelf { scroll: 0.0, ..self.clone_view() };
        (at.layout(d, g).height - (g.view.1 - g.top)).max(0.0)
    }

    /// Una copia con gli stessi filtri (per misurare senza toccare lo scorrimento).
    fn clone_view(&self) -> Shelf {
        Shelf {
            filter: self.filter,
            search: self.search.clone(),
            series: self.series.clone(),
            groups: self.groups,
            group: self.group.clone(),
            folders: self.folders,
            folder: self.folder.clone(),
            sort: self.sort,
            ..Shelf::default()
        }
    }

    /// Dove disegnare le copertine, e quali preparare: quelle a schermo, poi
    /// una fila sopra e due sotto.
    pub fn cover_slots(&self, d: &ShelfData, view: (f32, f32), scale: f32) -> Vec<CoverSlot> {
        let g = Geo::new(view, scale);
        let lay = self.layout(d, &g);
        let mut v: Vec<_> = lay
            .placed
            .iter()
            .enumerate()
            .filter(|(_, p)| p.y + p.h > g.top - g.row_h && p.y < g.view.1 + 2.0 * g.row_h)
            .map(|(k, p)| CoverSlot {
                path: d.entries[p.tile.cover()].path.clone(),
                x: p.x,
                y: p.y - self.lift(k, scale),
                w: p.w,
                h: p.h,
                radius: corner(scale),
                on_screen: p.y + p.h > g.top && p.y < g.view.1,
            })
            .collect();
        // prima quelle a schermo
        v.sort_by_key(|slot| !slot.on_screen);
        v
    }

    /// Si guardano le cartelle: la vista per cartelle, non dentro una serie
    /// (quelle dei server) ne' fra i gruppi.
    fn browsing(&self) -> bool {
        self.folders && !self.groups && self.group.is_none() && self.series.is_none()
    }

    pub fn enter_series(&mut self, name: String) {
        self.series = Some(name);
        self.reset_view();
    }

    pub fn enter_group(&mut self, kind: GroupKind, name: String) {
        (self.groups, self.group, self.series) = (true, Some((kind, name)), None);
        self.reset_view();
    }

    /// Un passo indietro: dalla serie al gruppo (o alla libreria, o alla sua
    /// cartella), dal gruppo all'elenco dei gruppi, da questo alla libreria;
    /// per cartelle, alla cartella sopra.
    fn back(&mut self, d: &ShelfData) {
        if self.series.take().is_none() && self.group.take().is_none() {
            if self.groups {
                self.groups = false;
            } else if self.folders {
                self.folder = self.parent_folder(d);
            }
        }
        self.reset_view();
    }

    fn reset_view(&mut self) {
        (self.scroll, self.glide, self.selected) = (0.0, 0.0, None);
    }

    pub fn set_sort(&mut self, sort: LibrarySort) {
        self.sort = sort;
        self.reset_view();
    }
}
