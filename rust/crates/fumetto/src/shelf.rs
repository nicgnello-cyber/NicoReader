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
use std::path::{Path, PathBuf};
use std::time::Instant;

use fumetto_core::library::{self, Entry, GroupKind, Status, by_number};
use fumetto_core::lingua::{italian, t};
use fumetto_core::natural_cmp;
use fumetto_core::remote::{self, Server};
use fumetto_render::{Align, Face, Layer, Measure, Rect, Text};

use crate::reader::Action;
use crate::strip::glide_step;
use crate::ui::{
    self, ACCENT, BLACK, Command, HAIR, Handled, INK, INK_F, Icon, Key, MUTED, MUTED_F, TICK, alpha, caps, draw_icon,
    fit, rgb,
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
    /// La copertina di questo volume e' gia' sulla scheda video.
    pub covered: &'a dyn Fn(&Path) -> bool,
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
}

impl Shelf {
    /// Le sezioni da mostrare, con filtro, ricerca e serie applicati.
    fn sections(&self, d: &ShelfData) -> Vec<Section> {
        let needle = self.search.to_lowercase();
        let found = |e: &Entry| {
            needle.is_empty()
                || e.title.to_lowercase().contains(&needle)
                || e.series.as_ref().is_some_and(|s| s.to_lowercase().contains(&needle))
                || e.authors.as_ref().is_some_and(|a| a.to_lowercase().contains(&needle))
        };
        let mut visible: Vec<usize> = (0..d.library_len())
            .filter(|&i| self.filter.keeps(d.status[i]) && found(&d.entries[i]))
            .filter(|&i| self.series.is_none() || d.entries[i].series == self.series)
            .filter(|&i| self.group.as_ref().is_none_or(|(k, n)| in_group(&d.entries[i], *k, n)))
            .collect();
        let recent_first = |v: &mut Vec<usize>| v.sort_by_key(|&i| std::cmp::Reverse(d.read_at[i]));
        let single = |tiles: Vec<Tile>| vec![Section { title: String::new(), tiles, one_row: false }];

        if self.series.is_some() {
            visible.sort_by(|&a, &b| by_number(&d.entries[a], &d.entries[b]));
            return single(visible.into_iter().map(Tile::Volume).collect());
        }
        if let Some((kind, name)) = &self.group {
            // una lista di lettura si legge nel suo ordine, volume per volume
            if *kind == GroupKind::ReadingList {
                sort_members(d, &mut visible, *kind, name);
                return single(visible.into_iter().map(Tile::Volume).collect());
            }
            return single(stacks(d, &visible));
        }
        if self.groups {
            return group_sections(d, self.filter, &needle, &found);
        }
        // per cartelle: la ricerca resta un elenco unico, i filtri valgono dentro
        if self.folders && needle.is_empty() {
            let mut sections = Vec::new();
            let (here, top) = self.folder_here(d);
            if top && self.filter == Filter::All {
                sections.extend(reading_row(d, &visible));
            }
            sections.extend(folder_sections(d, &visible, here.as_deref()));
            return sections;
        }
        if self.filter != Filter::All || !needle.is_empty() {
            if self.filter == Filter::Reading {
                recent_first(&mut visible);
            }
            let title =
                if needle.is_empty() { self.filter.label().to_owned() } else { t("Trovati", "Found").to_owned() };
            let mut sections =
                vec![Section { title, tiles: visible.into_iter().map(Tile::Volume).collect(), one_row: false }];
            if let Some(server) = server_section(d, self.filter).filter(|_| !needle.is_empty()) {
                // in libreria niente: resta solo cio' che dice il server
                sections.retain(|s| !s.tiles.is_empty());
                sections.push(server);
            }
            return sections;
        }

        let mut sections: Vec<Section> = reading_row(d, &visible).into_iter().collect();
        let n = d.library_len();
        sections.push(Section {
            title: if italian() {
                format!("Tutta la libreria \u{00b7} {n}")
            } else {
                format!("The whole library \u{00b7} {n}")
            },
            tiles: stacks(d, &visible),
            one_row: false,
        });
        sections
    }

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
        for sec in self.sections(d) {
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
        Layout { placed, heads, head, height: y - start + (40.0 * s).round() }
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
            ..Shelf::default()
        }
    }

    /// Dove disegnare le copertine, e quali preparare: quelle a schermo, poi
    /// una fila sopra e due sotto. (percorso, x, y, larghezza, altezza, a schermo).
    pub fn cover_slots(&self, d: &ShelfData, view: (f32, f32), scale: f32) -> Vec<(PathBuf, f32, f32, f32, f32, bool)> {
        let g = Geo::new(view, scale);
        let lay = self.layout(d, &g);
        let mut v: Vec<_> = lay
            .placed
            .iter()
            .enumerate()
            .filter(|(_, p)| p.y + p.h > g.top - g.row_h && p.y < g.view.1 + 2.0 * g.row_h)
            .map(|(k, p)| {
                let lift = if self.hover == Some(Hit::Tile(k)) { (4.0 * scale).round() } else { 0.0 };
                let on_screen = p.y + p.h > g.top && p.y < g.view.1;
                (d.entries[p.tile.cover()].path.clone(), p.x, p.y - lift, p.w, p.h, on_screen)
            })
            .collect();
        // prima quelle a schermo
        v.sort_by_key(|slot| !slot.5);
        v
    }

    fn hit(&self, d: &ShelfData, view: (f32, f32), s: f32, x: f32, y: f32) -> Option<Hit> {
        let g = Geo::new(view, s);
        if y < g.top {
            return Some(bar_hit(d, self, view, s, x).unwrap_or(Hit::Bar));
        }
        let lay = self.layout(d, &g);
        if let Some(top) = lay.head
            && y >= top
            && y < top + 40.0 * s
        {
            // per cartelle, ogni pezzo del percorso porta piu' su
            if self.browsing() {
                let crumbs = self.crumbs.borrow();
                return crumbs.iter().position(|c| x >= c.x0 && x < c.x1).map(Hit::Crumb);
            }
            if x < g.pad + 200.0 * s {
                return Some(Hit::Back);
            }
        }
        lay.placed
            .iter()
            .position(|p| x >= p.x && x < p.x + p.w && y >= p.y && y < p.y + g.row_h - 12.0 * s)
            .map(Hit::Tile)
    }

    /// La cella sotto il mouse, per il menu del tasto destro.
    pub fn tile_at(&self, d: &ShelfData, view: (f32, f32), s: f32, x: f32, y: f32) -> Option<Tile> {
        let Some(Hit::Tile(k)) = self.hit(d, view, s, x, y) else { return None };
        let g = Geo::new(view, s);
        self.layout(d, &g).placed.into_iter().nth(k).map(|p| p.tile)
    }

    pub fn motion(&mut self, d: &ShelfData, view: (f32, f32), s: f32, x: f32, y: f32) -> bool {
        let hover = self.hit(d, view, s, x, y).filter(|h| *h != Hit::Bar);
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    pub fn click(&mut self, d: &ShelfData, view: (f32, f32), s: f32, x: f32, y: f32) -> Handled {
        match self.hit(d, view, s, x, y) {
            Some(Hit::Tile(k)) => {
                let g = Geo::new(view, s);
                let Some(p) = self.layout(d, &g).placed.into_iter().nth(k) else { return Handled::Yes(None) };
                Handled::Yes(self.choose(d, p.tile))
            }
            Some(Hit::Filter(f)) => {
                self.filter = f;
                self.reset_view();
                Handled::Yes(None)
            }
            Some(Hit::ClearSearch) => {
                self.search.clear();
                self.reset_view();
                Handled::Yes(None)
            }
            Some(Hit::Folders) => Handled::Yes(Some(Command::FoldersMenu(x, ui::hud_height(s)))),
            Some(Hit::Groups) => {
                // dai gruppi si torna alla libreria; da dentro, all'elenco dei gruppi
                let home = self.groups && self.group.is_none() && self.series.is_none();
                (self.groups, self.group, self.series) = (!home, None, None);
                self.reset_view();
                Handled::Yes(None)
            }
            Some(Hit::Browse) => {
                // per cartelle o per serie; la cartella in cui si era resta
                (self.folders, self.groups, self.group, self.series) = (!self.browsing(), false, None, None);
                self.reset_view();
                Handled::Yes(None)
            }
            Some(Hit::Crumb(k)) => {
                let to = self.crumbs.borrow().get(k).map(|c| c.to.clone());
                if let Some(to) = to {
                    self.folder = to;
                    self.reset_view();
                }
                Handled::Yes(None)
            }
            Some(Hit::Return) => Handled::Yes(Some(Command::Act(Action::ToggleLibrary))),
            Some(Hit::Back) => {
                self.back(d);
                Handled::Yes(None)
            }
            Some(Hit::Bar) | None => Handled::Yes(None),
        }
    }

    /// Aprire una cella: il volume si legge, la serie si apre.
    fn choose(&mut self, d: &ShelfData, tile: Tile) -> Option<Command> {
        match tile {
            Tile::Volume(i) => Some(Command::Open(d.entries[i].path.clone())),
            Tile::Series { name, .. } => {
                self.enter_series(name);
                None
            }
            Tile::Group { kind, name, .. } => {
                self.enter_group(kind, name);
                None
            }
            Tile::Folder { path, .. } => {
                self.enter_folder(path);
                None
            }
        }
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

    pub fn wheel(&mut self, pixels: f32) {
        self.glide += pixels;
    }

    pub fn key(&mut self, key: Key, d: &ShelfData, view: (f32, f32), s: f32) -> Handled {
        let g = Geo::new(view, s);
        let lay = self.layout(d, &g);
        let n = lay.placed.len();
        let move_to = |from: Option<usize>, dir: (i32, i32)| -> Option<usize> {
            let Some(c) = from.and_then(|k| lay.placed.get(k)) else { return (n > 0).then_some(0) };
            // la cella piu' vicina nella direzione voluta, fra quelle che ci stanno
            lay.placed
                .iter()
                .enumerate()
                .filter(|(_, p)| match dir {
                    (1, 0) => p.y == c.y && p.x > c.x,
                    (-1, 0) => p.y == c.y && p.x < c.x,
                    (0, 1) => p.y > c.y,
                    _ => p.y < c.y,
                })
                .min_by(|(_, a), (_, b)| {
                    let da = ((a.y - c.y).abs(), (a.x - c.x).abs());
                    let db = ((b.y - c.y).abs(), (b.x - c.x).abs());
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(k, _)| k)
                .or(from)
        };
        match key {
            Key::Right => self.selected = move_to(self.selected, (1, 0)),
            Key::Left => self.selected = move_to(self.selected, (-1, 0)),
            Key::Down => self.selected = move_to(self.selected, (0, 1)),
            Key::Up => self.selected = move_to(self.selected, (0, -1)),
            Key::PageDown => self.glide += g.view.1 * 0.8,
            Key::PageUp => self.glide -= g.view.1 * 0.8,
            Key::Home => (self.scroll, self.glide, self.selected) = (0.0, 0.0, (n > 0).then_some(0)),
            Key::End => {
                (self.scroll, self.glide) = (self.max_scroll(d, &g), 0.0);
                self.selected = n.checked_sub(1);
            }
            Key::Enter => {
                let tile = self.selected.and_then(|k| lay.placed.into_iter().nth(k)).map(|p| p.tile);
                return Handled::Yes(tile.and_then(|t| self.choose(d, t)));
            }
            Key::Escape => {
                if !self.search.is_empty() {
                    self.search.clear();
                    self.reset_view();
                } else if self.inside(d) {
                    self.back(d);
                } else if d.reading {
                    return Handled::Yes(Some(Command::Act(Action::ToggleLibrary)));
                } else {
                    return Handled::Yes(None);
                }
            }
            Key::Backspace => {
                if self.search.pop().is_some() {
                    self.reset_view();
                } else if self.inside(d) {
                    self.back(d);
                }
            }
            Key::Char(c) if !c.is_control() => {
                self.search.push(c);
                self.reset_view();
            }
            Key::Digit(dg) => {
                self.search.push((b'0' + dg) as char);
                self.reset_view();
            }
            Key::Char(_) | Key::Tab => {}
        }
        self.reveal_selected(d, &g);
        Handled::Yes(None)
    }

    /// La cella scelta con la tastiera si porta tutta a schermo.
    fn reveal_selected(&mut self, d: &ShelfData, g: &Geo) {
        let Some(k) = self.selected else { return };
        let lay = self.layout(d, g);
        let Some(p) = lay.placed.get(k) else { return };
        let (lo, hi) = (g.top + 20.0 * g.s, g.view.1 - 20.0 * g.s);
        let delta = if p.y < lo {
            p.y - lo
        } else if p.y + g.row_h > hi {
            p.y + g.row_h - hi
        } else {
            0.0
        };
        self.scroll = (self.scroll + delta).clamp(0.0, self.max_scroll(d, g));
        self.glide = 0.0;
    }

    /// Fa avanzare lo scorrimento morbido; `true` se c'e' ancora movimento.
    pub fn advance(&mut self, d: &ShelfData, view: (f32, f32), s: f32, now: Instant) -> bool {
        let dt = self.last.map_or(1.0 / 60.0, |t| (now - t).as_secs_f32()).min(0.05);
        if self.glide.abs() < 0.5 {
            (self.glide, self.last) = (0.0, None);
            return false;
        }
        self.last = Some(now);
        let step = glide_step(self.glide, dt);
        self.glide -= step;
        let max = self.max_scroll(d, &Geo::new(view, s));
        let before = self.scroll;
        self.scroll = (self.scroll + step).clamp(0.0, max);
        if self.scroll == before {
            self.glide = 0.0; // contro il bordo
        }
        true
    }

    pub fn gliding(&self) -> bool {
        self.glide.abs() >= 0.5
    }

    /// Le righe del menu del tasto destro su una cella.
    pub(crate) fn tile_menu(d: &ShelfData, tile: &Tile) -> Vec<ui::Row> {
        match tile {
            Tile::Series { name, .. } => vec![
                ui::Row::Head(t("Serie", "Series")),
                ui::Row::Item {
                    label: t("Apri la serie", "Open the series"),
                    key: String::new(),
                    on: false,
                    cmd: Command::Series(name.clone()),
                },
            ],
            Tile::Group { kind, name, .. } => vec![
                ui::Row::Head(kind_label(*kind, false)),
                ui::Row::Item {
                    label: t("Apri", "Open"),
                    key: String::new(),
                    on: false,
                    cmd: Command::Group(*kind, name.clone()),
                },
            ],
            Tile::Folder { path, .. } => {
                let mut rows = vec![
                    ui::Row::Head(t("Cartella", "Folder")),
                    ui::Row::Item {
                        label: t("Apri la cartella", "Open the folder"),
                        key: String::new(),
                        on: false,
                        cmd: Command::Folder(path.clone()),
                    },
                ];
                if !remote::is_remote(path) {
                    rows.push(ui::Row::Item {
                        label: t("Mostra nel gestore dei file", "Show in the file manager"),
                        key: String::new(),
                        on: false,
                        cmd: Command::Reveal(path.clone()),
                    });
                }
                rows
            }
            Tile::Volume(i) => {
                let path = d.entries[*i].path.clone();
                let mut rows = vec![ui::Row::Item {
                    label: t("Leggi", "Read"),
                    key: String::new(),
                    on: false,
                    cmd: Command::Open(path.clone()),
                }];
                rows.push(ui::Row::Sep);
                if d.status[*i] != Status::Done {
                    rows.push(ui::Row::Item {
                        label: t("Segna come letto", "Mark as read"),
                        key: String::new(),
                        on: false,
                        cmd: Command::MarkRead(path.clone(), true),
                    });
                }
                if d.status[*i] != Status::New {
                    rows.push(ui::Row::Item {
                        label: t("Segna come da leggere", "Mark as unread"),
                        key: String::new(),
                        on: false,
                        cmd: Command::MarkRead(path.clone(), false),
                    });
                }
                rows.push(ui::Row::Item {
                    label: t("Modifica i dati\u{2026}", "Edit details\u{2026}"),
                    key: String::new(),
                    on: false,
                    cmd: Command::EditVolume(path.clone()),
                });
                if d.edited.get(*i) == Some(&true) {
                    rows.push(ui::Row::Item {
                        label: t("Torna ai dati del file", "Back to the file's details"),
                        key: String::new(),
                        on: false,
                        cmd: Command::ResetVolume(path.clone()),
                    });
                }
                // un volume su un server non ha cartella ne' cestino: si puo'
                // scaricare per leggerlo senza rete
                if remote::is_remote(&path) {
                    let (label, cmd) = if remote::offline::downloaded(&path).is_some() {
                        (t("Togli la copia scaricata", "Remove the downloaded copy"), Command::ForgetDownload(path))
                    } else if remote::offline::can_download(&path) {
                        (t("Scarica per leggere senza rete", "Download to read offline"), Command::Download(path))
                    } else {
                        return rows;
                    };
                    rows.push(ui::Row::Sep);
                    rows.push(ui::Row::Item { label, key: String::new(), on: false, cmd });
                    return rows;
                }
                rows.push(ui::Row::Sep);
                rows.push(ui::Row::Item {
                    label: t("Mostra nella cartella", "Show in folder"),
                    key: String::new(),
                    on: false,
                    cmd: Command::Reveal(path.clone()),
                });
                rows.push(ui::Row::Item {
                    label: t("Sposta nel cestino\u{2026}", "Move to trash\u{2026}"),
                    key: String::new(),
                    on: false,
                    cmd: Command::Trash(path),
                });
                rows
            }
        }
    }

    /// Le righe del menu delle cartelle della libreria.
    pub(crate) fn folders_menu(d: &ShelfData, servers: &[Server]) -> Vec<ui::Row> {
        let mut rows = Vec::new();
        if !d.roots.is_empty() || servers.is_empty() {
            rows.push(ui::Row::Head(t("Cartelle della libreria", "Library folders")));
        }
        for root in d.roots {
            let label = root.to_string_lossy().into_owned();
            rows.push(ui::Row::Folder { label, cmd: Command::RemoveFolder(root.clone()) });
        }
        if !servers.is_empty() {
            rows.push(ui::Row::Head(t("Server", "Servers")));
        }
        for s in servers {
            rows.push(ui::Row::Folder { label: remote::shown(&s.url), cmd: Command::RemoveServer(s.url.clone()) });
        }
        rows.push(ui::Row::Sep);
        rows.push(ui::Row::Item {
            label: t("Aggiungi una cartella\u{2026}", "Add a folder\u{2026}"),
            key: String::new(),
            on: false,
            cmd: Command::Act(Action::AddLibraryFolder),
        });
        rows.push(ui::Row::Item {
            label: t("Aggiungi un server\u{2026}", "Add a server\u{2026}"),
            key: String::new(),
            on: false,
            cmd: Command::AskServer,
        });
        rows
    }

    /// Le copertine sono gia' disegnate: qui tutto il resto, sopra di loro.
    pub fn layers(&self, d: &ShelfData, view: (f32, f32), s: f32, m: &mut dyn Measure) -> Vec<Layer> {
        let g = Geo::new(view, s);
        let lay = self.layout(d, &g);
        let mut l = Layer::default();

        let mut crumbs = Vec::new();
        if let Some(top) = lay.head {
            let base = top + 20.0 * s;
            let here = if self.browsing() {
                // il percorso, dall'inizio: ogni pezzo porta li'
                let mut x = g.pad;
                for (k, (name, to)) in self.trail(d).into_iter().enumerate() {
                    let label = if k == 0 { format!("\u{2039}   {name}") } else { name };
                    let color = if self.hover == Some(Hit::Crumb(k)) { INK } else { MUTED };
                    let text = caps(&label, 11.0 * s, color);
                    let w = m.width(&text);
                    l.texts.push(text.on_baseline(x, base));
                    crumbs.push(Crumb { x0: x - 6.0 * s, x1: x + w + 6.0 * s, to });
                    let sep = caps("   \u{203a}   ", 11.0 * s, MUTED);
                    let sep_w = m.width(&sep);
                    l.texts.push(sep.on_baseline(x + w, base));
                    x += w + sep_w;
                }
                self.folder_here(d).0.as_deref().map(folder_name).unwrap_or_default()
            } else {
                // dove si torna, e dove si e'
                let home = match self.folder_here(d).0.filter(|_| self.folders) {
                    Some(folder) => folder_name(&folder),
                    None => t("Libreria", "Library").to_owned(),
                };
                let (to, here) = match (&self.series, &self.group) {
                    (Some(series), Some((_, group))) => (group.clone(), series.clone()),
                    (Some(series), None) => (home, series.clone()),
                    (None, Some((_, group))) => (groups_title().to_owned(), group.clone()),
                    (None, None) => (home, groups_title().to_owned()),
                };
                let color = if self.hover == Some(Hit::Back) { INK } else { MUTED };
                l.texts.push(caps(&format!("\u{2039}   {to}"), 11.0 * s, color).on_baseline(g.pad, base));
                here
            };
            l.texts.push(
                fit(m, Text::new(here, Face::Serif, 44.0 * s, INK), g.view.0 - 2.0 * g.pad)
                    .on_baseline(g.pad, top + 78.0 * s),
            );
            let text = if self.groups && self.series.is_none() && self.group.is_none() {
                t("Dalle schede dei volumi e dai server", "From the volumes\u{2019} metadata and the servers")
                    .to_owned()
            } else {
                let (n, done) =
                    self.sections(d).iter().flat_map(|sec| &sec.tiles).fold((0, 0), |(n, k), tile| match tile {
                        Tile::Volume(i) => (n + 1, k + usize::from(d.status[*i] == Status::Done)),
                        Tile::Series { count, done, .. }
                        | Tile::Group { count, done, .. }
                        | Tile::Folder { count, done, .. } => (n + count, k + done),
                    });
                let count = if italian() {
                    format!("{n} volumi \u{00b7} {done} letti")
                } else {
                    format!("{n} volumes \u{00b7} {done} read")
                };
                match (&self.series, &self.group) {
                    (None, Some((kind, _))) => format!("{} \u{00b7} {count}", kind_label(*kind, false)),
                    _ => count,
                }
            };
            l.texts.push(caps(&text, 11.0 * s, MUTED).on_baseline(g.pad, top + 104.0 * s));
        }
        *self.crumbs.borrow_mut() = crumbs;
        for (title, base) in &lay.heads {
            l.texts.push(caps(title, 11.0 * s, MUTED).on_baseline(g.pad, *base));
        }

        for (k, p) in lay.placed.iter().enumerate() {
            if p.y + g.row_h < g.top || p.y > g.view.1 {
                continue;
            }
            let hovered = self.hover == Some(Hit::Tile(k));
            let lift = if hovered { (4.0 * s).round() } else { 0.0 };
            let (x, y, w, h) = (p.x, p.y - lift, p.w, p.h);
            let e = &d.entries[p.tile.cover()];
            if !(d.covered)(&e.path) {
                // la copertina non e' ancora pronta: una targa con il titolo
                l.rects.push(Rect::new(x, y, w, h, rgb(0x17, 0x16, 0x14)));
                let plate =
                    Text::new(e.title.as_str(), Face::Serif, 18.0 * s, MUTED).boxed(w - 28.0 * s, Align::Center);
                l.texts.push(plate.on_baseline(x + 14.0 * s, y + h / 2.0));
            }
            if let Tile::Series { .. } | Tile::Group { .. } = p.tile {
                // la pila: due bordi che spuntano in alto a destra
                for (k2, a) in [(1.0, 0.55), (2.0, 0.3)] {
                    let o = (5.0 * k2 * s).round();
                    l.rects.push(Rect::line(x + o, y - o, x + w + o, y - o, s.max(1.0), alpha(MUTED_F, a)));
                    l.rects.push(Rect::line(x + w + o, y - o, x + w + o, y + h - o, s.max(1.0), alpha(MUTED_F, a)));
                }
            }
            if let Tile::Folder { .. } = p.tile {
                // la cartella: la linguetta in alto a sinistra, e un bordo dietro
                let (tab_w, tab_h) = ((w * 0.38).round(), (7.0 * s).round());
                l.rects.push(Rect::new(x, y - tab_h, tab_w, tab_h + 2.0 * s, alpha(MUTED_F, 0.7)).radius(2.0 * s));
                l.rects.push(Rect::line(x + tab_w, y - 1.0, x + w, y - 1.0, (2.0 * s).max(1.0), alpha(MUTED_F, 0.7)));
            }
            if self.selected == Some(k) {
                let o = 5.0 * s;
                l.rects.push(
                    Rect::new(x - o, y - o, w + 2.0 * o, h + 2.0 * o, ACCENT).radius(4.0 * s).stroke((2.0 * s).round()),
                );
            }
            // quanto se ne e' letto: un filo sotto la copertina
            let bar_y = (y + h + 7.0 * s).round();
            match (&p.tile, d.status[p.tile.cover()]) {
                (Tile::Volume(_), Status::Reading(page, pages)) if pages > 0 => {
                    l.rects.push(Rect::new(x, bar_y, w, (2.0 * s).round(), HAIR));
                    let f = ((page + 1) as f32 / pages as f32).clamp(0.02, 1.0);
                    l.rects.push(Rect::new(x, bar_y, (w * f).round(), (2.0 * s).round(), INK_F));
                }
                (Tile::Volume(_), Status::Done) => l.rects.push(Rect::new(x, bar_y, w, (2.0 * s).round(), TICK)),
                _ => {}
            }
            let (title, meta) = match &p.tile {
                Tile::Volume(i) => {
                    let meta = meta_of(d.status[*i]);
                    // scaricato: si legge anche senza rete
                    let meta = if d.offline.get(*i) == Some(&true) {
                        format!("{meta} \u{b7} {}", t("scaricato", "downloaded"))
                    } else {
                        meta
                    };
                    (self.volume_title(&d.entries[*i]), meta)
                }
                Tile::Series { name, count, done, .. }
                | Tile::Group { name, count, done, .. }
                | Tile::Folder { name, count, done, .. } => (
                    name.clone(),
                    if italian() {
                        format!("{count} volumi \u{00b7} {done} letti")
                    } else {
                        format!("{count} volumes \u{00b7} {done} read")
                    },
                ),
            };
            l.texts.push(fit(m, Text::new(title, Face::Serif, 18.0 * s, INK), w).on_baseline(x, y + h + 34.0 * s));
            l.texts.push(
                Text::new(meta, Face::Sans, 12.0 * s, if hovered { INK } else { MUTED })
                    .tabular()
                    .on_baseline(x, y + h + 54.0 * s),
            );
        }

        // mentre il server cerca, lo dice gia' il titolo della sua sezione
        if lay.placed.is_empty() && !(d.searching && !self.search.is_empty()) {
            let text = if d.scanning {
                t("Cerco i fumetti\u{2026}", "Looking for comics\u{2026}")
            } else if !self.search.is_empty() || self.filter != Filter::All {
                t("Nessun risultato", "Nothing found")
            } else if self.groups {
                t("Nessuna raccolta, lista, genere o editore", "No collections, lists, genres or publishers")
            } else {
                t("Nessun fumetto in queste cartelle", "No comics in these folders")
            };
            let wide = g.view.0;
            l.texts.push(
                Text::new(text, Face::Serif, 28.0 * s, MUTED)
                    .boxed(wide, Align::Center)
                    .on_baseline(0.0, g.top + (g.view.1 - g.top) * 0.42),
            );
        }
        vec![l, self.bar(d, &g, m)]
    }

    /// Il titolo di un volume nella sua cella: fuori dalla sua serie, se il
    /// titolo non la dice ("Volume 1" dalla scheda), la serie davanti. Per
    /// cartelle, senza le cartelle dei capitoli ("Vol 1 · Ch 2" dentro Vol 1
    /// e' "Ch 2"): si vedono gia' nel percorso.
    fn volume_title(&self, e: &Entry) -> String {
        if self.browsing() {
            let own = fumetto_core::title_of(&e.path);
            return if e.title.ends_with(&format!(" \u{00b7} {own}")) { own } else { e.title.clone() };
        }
        match &e.series {
            Some(series) if self.series.is_none() && !e.title.to_lowercase().contains(&series.to_lowercase()) => {
                format!("{series}: {}", e.title)
            }
            _ => e.title.clone(),
        }
    }

    /// La barra della libreria: il nome, i filtri, la ricerca, le cartelle.
    fn bar(&self, d: &ShelfData, g: &Geo, m: &mut dyn Measure) -> Layer {
        let s = g.s;
        let (vw, h) = (g.view.0, g.top);
        let cy = (h / 2.0).round();
        let mut l = Layer::default();
        l.rects.push(Rect::new(0.0, 0.0, vw, h, BLACK));
        l.rects.push(Rect::new(0.0, h - s.round(), vw, s.round(), alpha(HAIR, 0.8)));
        l.texts.push(Text::new("NicoReader", Face::Serif, 24.0 * s, INK).on_baseline((24.0 * s).round(), cy + 8.0 * s));

        let geo = BarGeo::new(d, vw, s);
        for (f, x) in Filter::ALL.iter().zip(&geo.tabs) {
            let on = self.filter == *f;
            let hovered = self.hover == Some(Hit::Filter(*f));
            let color = if on || hovered { INK } else { MUTED };
            let count = d.status[..d.library_len()].iter().filter(|st| f.keeps(**st)).count();
            // filtri stretti (finestra piccola): lettere un po' piu' piccole
            let size = 11.0 * s * (geo.tab_w / (134.0 * s)).clamp(0.8, 1.0);
            let label = caps(&format!("{}  {count}", f.label()), size, color).tabular();
            l.texts.push(label.boxed(geo.tab_w, Align::Center).on_baseline(*x, cy + 4.0 * s));
            if on {
                let dd = 4.0 * s;
                l.rects.push(Rect::new(x + geo.tab_w / 2.0 - dd / 2.0, h - 10.0 * s, dd, dd, ACCENT).radius(dd));
            }
        }

        // la ricerca: basta scrivere
        let (sx, sw) = (geo.search_x, geo.search_w);
        l.rects.push(Rect::new(
            sx,
            (cy + 13.0 * s).round(),
            sw,
            s.round(),
            if self.search.is_empty() { HAIR } else { TICK },
        ));
        draw_icon(&mut l, Icon::Search, (sx + 9.0 * s).round(), cy, s, MUTED_F);
        if self.search.is_empty() {
            l.texts.push(
                Text::new(t("Scrivi per cercare", "Type to search"), Face::Sans, 13.5 * s, MUTED)
                    .on_baseline(sx + 26.0 * s, cy + 5.0 * s),
            );
        } else {
            let text = fit(m, Text::new(self.search.as_str(), Face::Sans, 14.0 * s, INK).weight(450), sw - 60.0 * s);
            let tw = m.width(&text);
            l.texts.push(text.on_baseline(sx + 26.0 * s, cy + 5.0 * s));
            let caret = (sx + 28.0 * s + tw).round();
            l.rects.push(Rect::new(caret, (cy - 9.0 * s).round(), (2.0 * s).round(), (18.0 * s).round(), ACCENT));
            let color = if self.hover == Some(Hit::ClearSearch) { INK_F } else { MUTED_F };
            draw_icon(&mut l, Icon::Close, (sx + sw - 10.0 * s).round(), cy, s * 0.8, color);
        }

        let buttons = [
            (Hit::Groups, Icon::Shelf, geo.groups_x),
            (Hit::Browse, Icon::Tree, geo.browse_x),
            (Hit::Folders, Icon::Open, geo.folders_x),
            (Hit::Return, Icon::Close, geo.return_x),
        ];
        for (hit, icon, x) in buttons {
            let Some(x) = x else { continue };
            let hovered = self.hover == Some(hit.clone());
            let on = (hit == Hit::Groups && self.groups) || (hit == Hit::Browse && self.browsing());
            let by = ((h - 36.0 * s) / 2.0).round();
            if hovered {
                l.rects.push(Rect::new(x, by, 36.0 * s, 36.0 * s, [1.0, 1.0, 1.0, 0.075]).radius(8.0 * s));
            }
            draw_icon(&mut l, icon, (x + 18.0 * s).round(), cy, s, if hovered || on { INK_F } else { MUTED_F });
            if on {
                let dd = 4.0 * s;
                l.rects.push(Rect::new(x + 18.0 * s - dd / 2.0, h - 10.0 * s, dd, dd, ACCENT).radius(dd));
            }
            if hovered {
                let tip = match hit {
                    Hit::Groups => t("Raccolte, liste, generi, editori", "Collections, lists, genres, publishers"),
                    Hit::Folders => t("Cartelle della libreria", "Library folders"),
                    Hit::Browse if self.browsing() => t("Torna alle serie", "Back to the series"),
                    Hit::Browse => t("Per cartelle, come su disco", "By folder, as on disk"),
                    _ => t("Torna alla lettura   \u{00b7}   Esc", "Back to reading   \u{00b7}   Esc"),
                };
                ui::tooltip(&mut l, m, tip, x + 18.0 * s, h, vw, s);
            }
        }
        l
    }
}

/// Dove stanno i pezzi della barra della libreria (senza misurare i testi).
struct BarGeo {
    tabs: Vec<f32>,
    tab_w: f32,
    search_x: f32,
    search_w: f32,
    folders_x: Option<f32>,
    /// La vista per cartelle.
    browse_x: Option<f32>,
    /// Solo se qualche volume fa parte di un gruppo.
    groups_x: Option<f32>,
    return_x: Option<f32>,
}

impl BarGeo {
    fn new(d: &ShelfData, vw: f32, s: f32) -> BarGeo {
        let mut r = vw - 14.0 * s;
        let return_x = d.reading.then(|| {
            r -= 36.0 * s;
            r.round()
        });
        r -= 36.0 * s + 4.0 * s;
        let folders_x = Some(r.round());
        r -= 36.0 * s + 4.0 * s;
        let browse_x = Some(r.round());
        let groups_x = d.entries[..d.library_len()].iter().any(|e| !e.groups.is_empty()).then(|| {
            r -= 36.0 * s + 4.0 * s;
            r.round()
        });
        // i filtri in mezzo; in una finestra stretta si spostano a sinistra
        // (dopo il nome) e si stringono, per lasciare posto alla ricerca
        let (logo, room) = (150.0 * s, r - 16.0 * s - 120.0 * s - 16.0 * s);
        let mut tab_w = (134.0 * s).round();
        let mut x0 = ((vw - tab_w * 4.0) / 2.0).round();
        if x0 + tab_w * 4.0 > room {
            x0 = (room - tab_w * 4.0).max(logo).round();
            tab_w = tab_w.min(((room - x0) / 4.0).floor()).max(96.0 * s);
        }
        let tabs_w = tab_w * 4.0;
        let search_w = (250.0 * s).round().min((r - 16.0 * s) - (x0 + tabs_w + 16.0 * s)).max(120.0 * s);
        let search_x = (r - 16.0 * s - search_w).round();
        BarGeo {
            tabs: (0..4).map(|k| x0 + k as f32 * tab_w).collect(),
            tab_w,
            search_x,
            search_w,
            folders_x,
            browse_x,
            groups_x,
            return_x,
        }
    }
}

fn bar_hit(d: &ShelfData, shelf: &Shelf, view: (f32, f32), s: f32, x: f32) -> Option<Hit> {
    let g = BarGeo::new(d, view.0, s);
    if let Some(k) = g.tabs.iter().position(|&tx| x >= tx && x < tx + g.tab_w) {
        return Some(Hit::Filter(Filter::ALL[k]));
    }
    if !shelf.search.is_empty() && x >= g.search_x + g.search_w - 24.0 * s && x < g.search_x + g.search_w + 4.0 * s {
        return Some(Hit::ClearSearch);
    }
    let inside = |bx: Option<f32>| bx.is_some_and(|bx| x >= bx && x < bx + 36.0 * s);
    if inside(g.folders_x) {
        return Some(Hit::Folders);
    }
    if inside(g.browse_x) {
        return Some(Hit::Browse);
    }
    if inside(g.groups_x) {
        return Some(Hit::Groups);
    }
    if inside(g.return_x) {
        return Some(Hit::Return);
    }
    None
}

fn tile_name(d: &ShelfData, t: &Tile) -> String {
    match t {
        Tile::Volume(i) => d.entries[*i].title.clone(),
        Tile::Series { name, .. } | Tile::Group { name, .. } | Tile::Folder { name, .. } => name.clone(),
    }
}

/// Le serie con piu' volumi diventano una pila, al posto del primo; tutto in
/// ordine naturale di nome.
fn stacks(d: &ShelfData, visible: &[usize]) -> Vec<Tile> {
    let mut tiles: Vec<Tile> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for &i in visible {
        let Some(name) = d.entries[i].series.as_deref() else {
            tiles.push(Tile::Volume(i));
            continue;
        };
        if seen.contains(&name) {
            continue;
        }
        let mut members: Vec<usize> =
            visible.iter().copied().filter(|&j| d.entries[j].series.as_deref() == Some(name)).collect();
        if members.len() < 2 {
            tiles.push(Tile::Volume(i));
            continue;
        }
        seen.push(name);
        members.sort_by(|&a, &b| by_number(&d.entries[a], &d.entries[b]));
        let (cover, done) = cover_and_done(d, &members);
        tiles.push(Tile::Series { name: name.to_owned(), cover, count: members.len(), done });
    }
    tiles.sort_by(|a, b| natural_cmp(&tile_name(d, a), &tile_name(d, b)));
    tiles
}

/// La fila dei volumi a meta', dal piu' recente.
fn reading_row(d: &ShelfData, visible: &[usize]) -> Option<Section> {
    let mut reading: Vec<usize> =
        visible.iter().copied().filter(|&i| matches!(d.status[i], Status::Reading(..))).collect();
    reading.sort_by_key(|&i| std::cmp::Reverse(d.read_at[i]));
    (!reading.is_empty()).then(|| Section {
        title: t("Continua a leggere", "Continue reading").to_owned(),
        tiles: reading.into_iter().map(Tile::Volume).collect(),
        one_row: true,
    })
}

/// Da dove vengono i volumi, per la vista per cartelle: le cartelle della
/// libreria che ne hanno, poi i server ("http://casa:25600").
fn sources(d: &ShelfData) -> Vec<PathBuf> {
    let lib = &d.entries[..d.library_len()];
    let mut out: Vec<PathBuf> = d
        .roots
        .iter()
        .filter(|root| lib.iter().any(|e| library::root_of(&e.path, d.roots) == Some(root.as_path())))
        .cloned()
        .collect();
    for e in lib.iter().filter(|e| remote::is_remote(&e.path)) {
        let server = PathBuf::from(library::origin(&e.path));
        if !out.contains(&server) {
            out.push(server);
        }
    }
    out
}

/// Il nome di una cartella; di un server, l'indirizzo senza "http://".
fn folder_name(path: &Path) -> String {
    let s = path.to_string_lossy();
    if remote::is_remote(path) {
        return s.split_once("://").map_or(&*s, |(_, host)| host).to_owned();
    }
    path.file_name().map_or_else(|| s.to_string(), |n| n.to_string_lossy().into_owned())
}

/// La vista per cartelle: in `here` prima le sottocartelle, poi i volumi; un
/// server (non ha cartelle) le sue serie; all'inizio (`None`) le cartelle
/// della libreria e i server.
fn folder_sections(d: &ShelfData, visible: &[usize], here: Option<&Path>) -> Vec<Section> {
    let folder_tile = |path: PathBuf, members: &[usize]| {
        let done = members.iter().filter(|&&j| d.status[j] == Status::Done).count();
        // in copertina il primo da leggere: la cartella si riconosce sempre
        Tile::Folder { name: folder_name(&path), cover: members[0], count: members.len(), done, path }
    };
    let section = |title: &str, tiles: Vec<Tile>| Section {
        title: format!("{title} \u{00b7} {}", tiles.len()),
        tiles,
        one_row: false,
    };
    let within = |place: &Path| -> Vec<usize> {
        visible.iter().copied().filter(|&i| d.entries[i].path.starts_with(place)).collect()
    };
    let tiles: Vec<Tile> = match here {
        None => sources(d)
            .into_iter()
            .filter_map(|source| {
                let mut members = within(&source);
                members.sort_by(|&a, &b| library::folder_cmp(&d.entries[a].path, &d.entries[b].path));
                (!members.is_empty()).then(|| folder_tile(source, &members))
            })
            .collect(),
        Some(server) if remote::is_remote(server) => stacks(d, &within(server)),
        Some(folder) => {
            let c = library::folder_contents(d.entries, visible, folder);
            let mut out = Vec::new();
            if !c.folders.is_empty() {
                out.push(section(
                    t("Cartelle", "Folders"),
                    c.folders.into_iter().map(|(path, members)| folder_tile(path, &members)).collect(),
                ));
            }
            if !c.volumes.is_empty() {
                out.push(section(t("Volumi", "Volumes"), c.volumes.into_iter().map(Tile::Volume).collect()));
            }
            return out;
        }
    };
    let title = if here.is_none() { t("Cartelle", "Folders") } else { t("Sul server", "On the server") };
    if tiles.is_empty() { Vec::new() } else { vec![section(title, tiles)] }
}

/// In copertina il volume che si sta leggendo, se c'e'; se no il primo. E
/// quanti se ne sono letti.
fn cover_and_done(d: &ShelfData, members: &[usize]) -> (usize, usize) {
    let done = members.iter().filter(|&&j| d.status[j] == Status::Done).count();
    let cover = members.iter().copied().find(|&j| matches!(d.status[j], Status::Reading(..))).unwrap_or(members[0]);
    (cover, done)
}

fn in_group(e: &Entry, kind: GroupKind, name: &str) -> bool {
    e.groups.iter().any(|g| g.kind == kind && g.name == name)
}

/// I volumi di un gruppo nel loro ordine: quello della lista di lettura, se
/// c'e'; poi per serie e numero.
fn sort_members(d: &ShelfData, members: &mut [usize], kind: GroupKind, name: &str) {
    let place = |i: usize| {
        d.entries[i].groups.iter().find(|g| g.kind == kind && g.name == name).and_then(|g| g.order).unwrap_or(u32::MAX)
    };
    members.sort_by(|&a, &b| {
        let (ea, eb) = (&d.entries[a], &d.entries[b]);
        place(a)
            .cmp(&place(b))
            .then_with(|| natural_cmp(ea.series.as_deref().unwrap_or(""), eb.series.as_deref().unwrap_or("")))
            .then_with(|| by_number(ea, eb))
    });
}

const KINDS: [GroupKind; 4] = [GroupKind::Collection, GroupKind::ReadingList, GroupKind::Genre, GroupKind::Publisher];

/// Il nome di un tipo di gruppo, al plurale (titolo di sezione) o al singolare.
fn kind_label(kind: GroupKind, plural: bool) -> &'static str {
    match (kind, plural) {
        (GroupKind::Collection, true) => t("Raccolte", "Collections"),
        (GroupKind::Collection, false) => t("Raccolta", "Collection"),
        (GroupKind::ReadingList, true) => t("Liste di lettura", "Reading lists"),
        (GroupKind::ReadingList, false) => t("Lista di lettura", "Reading list"),
        (GroupKind::Genre, true) => t("Generi", "Genres"),
        (GroupKind::Genre, false) => t("Genere", "Genre"),
        (GroupKind::Publisher, true) => t("Editori", "Publishers"),
        (GroupKind::Publisher, false) => t("Editore", "Publisher"),
    }
}

fn groups_title() -> &'static str {
    t("Raccolte e liste", "Collections and lists")
}

/// La vista dei gruppi: una sezione per tipo, una pila per gruppo. Cercando,
/// un gruppo resta se il suo nome corrisponde (con tutti i volumi) o se ci
/// sono volumi che corrispondono (solo quelli).
fn group_sections(d: &ShelfData, filter: Filter, needle: &str, found: &dyn Fn(&Entry) -> bool) -> Vec<Section> {
    let mut sections = Vec::new();
    for kind in KINDS {
        let mut by_name: Vec<(&str, Vec<usize>)> = Vec::new();
        for (i, e) in d.entries[..d.library_len()].iter().enumerate() {
            if !filter.keeps(d.status[i]) {
                continue;
            }
            for g in e.groups.iter().filter(|g| g.kind == kind) {
                if !found(e) && !g.name.to_lowercase().contains(needle) {
                    continue;
                }
                match by_name.iter_mut().find(|(n, _)| *n == g.name) {
                    Some((_, members)) if members.last() != Some(&i) => members.push(i),
                    Some(_) => {}
                    None => by_name.push((&g.name, vec![i])),
                }
            }
        }
        let mut tiles: Vec<Tile> = by_name
            .into_iter()
            .map(|(name, mut members)| {
                sort_members(d, &mut members, kind, name);
                let (cover, done) = cover_and_done(d, &members);
                Tile::Group { kind, name: name.to_owned(), cover, count: members.len(), done }
            })
            .collect();
        if tiles.is_empty() {
            continue;
        }
        tiles.sort_by(|a, b| natural_cmp(&tile_name(d, a), &tile_name(d, b)));
        sections.push(Section {
            title: format!("{} \u{00b7} {}", kind_label(kind, true), tiles.len()),
            tiles,
            one_row: false,
        });
    }
    sections
}

/// I volumi trovati cercando sui server, che la libreria non ha: per serie e
/// numero. Mentre si aspetta la risposta, solo il titolo che lo dice.
fn server_section(d: &ShelfData, filter: Filter) -> Option<Section> {
    let mut found: Vec<usize> = (d.library_len()..d.entries.len()).filter(|&i| filter.keeps(d.status[i])).collect();
    found.sort_by(|&a, &b| {
        let (ea, eb) = (&d.entries[a], &d.entries[b]);
        natural_cmp(ea.series.as_deref().unwrap_or(""), eb.series.as_deref().unwrap_or(""))
            .then_with(|| by_number(ea, eb))
    });
    let title = match (found.len(), d.searching) {
        (0, false) => return None,
        (0, true) => t("Sul server: cerco\u{2026}", "On the server: searching\u{2026}").to_owned(),
        (n, _) if italian() => format!("Sul server \u{b7} {n}"),
        (n, _) => format!("On the server \u{b7} {n}"),
    };
    Some(Section { title, tiles: found.into_iter().map(Tile::Volume).collect(), one_row: false })
}

fn meta_of(s: Status) -> String {
    match s {
        Status::New => t("nuovo", "new").to_owned(),
        Status::Reading(page, pages) => format!("{} / {pages}", page + 1),
        Status::Done => t("letto", "read").to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(title: &str, series: Option<(&str, u32)>) -> Entry {
        Entry {
            path: PathBuf::from(format!("{title}.cbz")),
            title: title.to_owned(),
            series: series.map(|s| s.0.to_owned()),
            number: series.map(|s| s.1),
            authors: None,
            groups: Vec::new(),
        }
    }

    fn data<'a>(entries: &'a [Entry], status: &'a [Status], read_at: &'a [u64]) -> ShelfData<'a> {
        ShelfData {
            entries,
            found_from: entries.len(),
            searching: false,
            status,
            read_at,
            offline: &[],
            edited: &[],
            scanning: false,
            roots: &[],
            covered: &|_| true,
            reading: false,
        }
    }

    fn library() -> (Vec<Entry>, Vec<Status>, Vec<u64>) {
        let entries = vec![
            entry("Acqua Alta", None),
            entry("Nebbia sul Porto v01", Some(("Nebbia sul Porto", 1))),
            entry("Nebbia sul Porto v02", Some(("Nebbia sul Porto", 2))),
            entry("Orbita Bassa #12", Some(("Orbita Bassa", 12))),
        ];
        let status = vec![Status::New, Status::Done, Status::Reading(10, 200), Status::Reading(3, 40)];
        (entries, status, vec![0, 5, 20, 10])
    }

    #[test]
    fn la_libreria_per_cartelle() {
        let at = |path: &str, series: Option<(&str, u32)>| Entry {
            path: PathBuf::from(path),
            title: fumetto_core::title_of(Path::new(path)),
            series: series.map(|s| s.0.to_owned()),
            number: series.map(|s| s.1),
            authors: None,
            groups: Vec::new(),
        };
        let server = "http://casa:25600/opds/v1.2/books";
        let e = vec![
            at("/f/Manga/Berserk/Vol 2/Ch 10.cbz", None),
            at("/f/Manga/Berserk/Vol 2/Ch 9.cbz", None),
            at("/f/Manga/Akira.cbz", None),
            at("/f/Moebius/Arzach.cbz", None),
            at("/f/Sciolto.cbz", None),
            at(&format!("{server}/a/pages/{{n}}#3:Porto v01"), Some(("Porto", 1))),
            at(&format!("{server}/b/pages/{{n}}#3:Porto v02"), Some(("Porto", 2))),
        ];
        let st = vec![Status::New; e.len()];
        let read_at = vec![0; e.len()];
        let roots = [PathBuf::from("/f")];
        let d = ShelfData { roots: &roots, ..data(&e, &st, &read_at) };
        let folder = |path: &str, cover: usize, count: usize| Tile::Folder {
            path: PathBuf::from(path),
            name: folder_name(Path::new(path)),
            cover,
            count,
            done: 0,
        };
        let tiles = |shelf: &Shelf| -> Vec<Vec<Tile>> { shelf.sections(&d).into_iter().map(|s| s.tiles).collect() };
        let mut shelf = Shelf { folders: true, ..Shelf::default() };

        // una cartella e un server: si parte da loro
        assert_eq!(tiles(&shelf), [vec![folder("/f", 1, 5), folder("http://casa:25600", 5, 2)]]);
        assert!(!shelf.inside(&d));
        shelf.enter_folder(PathBuf::from("/f"));
        assert_eq!(
            tiles(&shelf),
            [vec![folder("/f/Manga", 1, 3), folder("/f/Moebius", 3, 1)], vec![Tile::Volume(4)]],
            "prima le cartelle, poi i volumi; in copertina il primo da leggere"
        );
        shelf.enter_folder(PathBuf::from("/f/Manga"));
        assert_eq!(tiles(&shelf), [vec![folder("/f/Manga/Berserk", 1, 2)], vec![Tile::Volume(2)]]);
        assert_eq!(
            shelf.trail(&d),
            [(t("Cartelle", "Folders").to_owned(), None), ("f".to_owned(), Some(PathBuf::from("/f")))]
        );
        shelf.enter_folder(PathBuf::from("/f/Manga/Berserk/Vol 2"));
        assert_eq!(tiles(&shelf), [vec![Tile::Volume(1), Tile::Volume(0)]], "9 prima di 10");
        let view = (1920.0, 1080.0);
        shelf.key(Key::Backspace, &d, view, 1.0);
        assert_eq!(shelf.folder, Some(PathBuf::from("/f/Manga/Berserk")), "indietro: la cartella sopra");
        shelf.key(Key::Escape, &d, view, 1.0);
        shelf.key(Key::Escape, &d, view, 1.0);
        shelf.key(Key::Escape, &d, view, 1.0);
        assert_eq!(shelf.folder, None, "fino all'inizio");

        // un server non ha cartelle: le sue serie, e dalla serie si torna li'
        shelf.enter_folder(PathBuf::from("http://casa:25600"));
        assert_eq!(tiles(&shelf), [vec![Tile::Series { name: "Porto".into(), cover: 5, count: 2, done: 0 }]]);
        shelf.enter_series("Porto".into());
        shelf.key(Key::Escape, &d, view, 1.0);
        assert_eq!((shelf.series.as_deref(), shelf.folder.as_deref()), (None, Some(Path::new("http://casa:25600"))));

        // i filtri valgono dentro le cartelle; cercando, un elenco solo
        shelf.enter_folder(PathBuf::from("/f/Manga"));
        shelf.filter = Filter::Done;
        assert!(tiles(&shelf).is_empty());
        shelf.filter = Filter::All;
        shelf.search = "arz".into();
        assert_eq!(tiles(&shelf), [vec![Tile::Volume(3)]]);
        shelf.search.clear();

        // con una cartella sola si parte da dentro, e il percorso comincia da lei
        let local = &e[..5];
        let d = ShelfData { roots: &roots, ..data(local, &st[..5], &read_at[..5]) };
        let mut shelf = Shelf { folders: true, ..Shelf::default() };
        assert_eq!(shelf.sections(&d).len(), 2, "le cartelle e i volumi di /f");
        assert!(!shelf.inside(&d));
        shelf.enter_folder(PathBuf::from("/f/Manga"));
        assert_eq!(shelf.trail(&d), [("f".to_owned(), None)]);
        shelf.key(Key::Escape, &d, view, 1.0);
        assert_eq!(shelf.folder, None);
    }
    #[test]
    fn continua_a_leggere_poi_la_libreria_con_le_serie() {
        let (e, st, at) = library();
        let d = data(&e, &st, &at);
        let secs = Shelf::default().sections(&d);
        assert_eq!(secs.len(), 2);
        assert_eq!(secs[0].tiles, [Tile::Volume(2), Tile::Volume(3)], "in lettura, dal piu' recente");
        assert_eq!(
            secs[1].tiles,
            [
                Tile::Volume(0),
                Tile::Series { name: "Nebbia sul Porto".into(), cover: 2, count: 2, done: 1 },
                Tile::Volume(3), // una serie di un volume solo resta un volume
            ]
        );
    }

    #[test]
    fn cercare_filtrare_e_aprire_una_serie() {
        let (e, st, at) = library();
        let d = data(&e, &st, &at);
        let mut shelf = Shelf::default();
        let view = (1920.0, 1080.0);
        for c in "nebbia".chars() {
            shelf.key(Key::Char(c), &d, view, 1.0);
        }
        assert_eq!(shelf.sections(&d)[0].tiles, [Tile::Volume(1), Tile::Volume(2)], "cercando non si raggruppa");
        shelf.key(Key::Escape, &d, view, 1.0);
        assert!(shelf.search.is_empty());
        shelf.filter = Filter::Done;
        assert_eq!(shelf.sections(&d)[0].tiles, [Tile::Volume(1)]);
        shelf.filter = Filter::All;
        shelf.enter_series("Nebbia sul Porto".into());
        assert_eq!(shelf.sections(&d)[0].tiles, [Tile::Volume(1), Tile::Volume(2)], "in ordine di numero");
        shelf.key(Key::Escape, &d, view, 1.0);
        assert!(shelf.series.is_none(), "Esc esce dalla serie");
    }

    #[test]
    fn raccolte_liste_e_generi() {
        use fumetto_core::library::Group;
        let (mut e, st, at) = library();
        let arc = |n| Group::new(GroupKind::ReadingList, "La grande nebbia", Some(n));
        let porto = Group::new(GroupKind::Collection, "Classici del porto", None);
        let giallo = Group::new(GroupKind::Genre, "Giallo", None);
        e[1].groups = vec![porto.clone(), arc(3), giallo.clone()];
        e[2].groups = vec![porto.clone(), giallo.clone()];
        e[3].groups = vec![arc(1)];
        e[0].groups = vec![Group::new(GroupKind::Genre, "Avventura", None), giallo];
        let d = data(&e, &st, &at);
        let view = (1920.0, 1080.0);
        let mut shelf = Shelf::default();
        assert!(BarGeo::new(&d, view.0, 1.0).groups_x.is_some(), "c'e' il tasto dei gruppi");
        shelf.groups = true;
        let secs = shelf.sections(&d);
        let titles: Vec<&str> = secs.iter().map(|s| s.title.as_str()).collect();
        let head = |kind, n| format!("{} \u{b7} {n}", kind_label(kind, true));
        assert_eq!(
            titles,
            [head(GroupKind::Collection, 1), head(GroupKind::ReadingList, 1), head(GroupKind::Genre, 2)]
        );
        assert_eq!(
            secs[1].tiles,
            [Tile::Group {
                kind: GroupKind::ReadingList,
                name: "La grande nebbia".into(),
                cover: 3, // in lettura
                count: 2,
                done: 1
            }]
        );
        assert!(matches!(&secs[2].tiles[1], Tile::Group { name, count: 3, .. } if name == "Giallo"));
        // nella lista i volumi vanno nel suo ordine, non per serie
        shelf.enter_group(GroupKind::ReadingList, "La grande nebbia".into());
        assert_eq!(shelf.sections(&d)[0].tiles, [Tile::Volume(3), Tile::Volume(1)]);
        // nella raccolta le serie restano pile
        shelf.enter_group(GroupKind::Collection, "Classici del porto".into());
        assert!(matches!(&shelf.sections(&d)[0].tiles[..], [Tile::Series { count: 2, .. }]));
        shelf.enter_series("Nebbia sul Porto".into());
        shelf.key(Key::Escape, &d, view, 1.0);
        assert_eq!(shelf.group.as_ref().map(|g| g.1.as_str()), Some("Classici del porto"), "dalla serie al gruppo");
        shelf.key(Key::Escape, &d, view, 1.0);
        assert!(shelf.groups && shelf.group.is_none(), "dal gruppo all'elenco");
        // cercando: il nome del gruppo, o i volumi dentro
        for c in "orbita".chars() {
            shelf.key(Key::Char(c), &d, view, 1.0);
        }
        assert_eq!(shelf.sections(&d).len(), 1);
        assert!(matches!(&shelf.sections(&d)[0].tiles[..], [Tile::Group { count: 1, .. }]));
        shelf.search = "avv".into();
        assert!(matches!(&shelf.sections(&d)[0].tiles[..], [Tile::Group { count: 1, .. }]));
        shelf.search.clear();
        shelf.key(Key::Escape, &d, view, 1.0);
        assert!(!shelf.groups, "dall'elenco alla libreria");
    }

    #[test]
    fn trovati_sul_server() {
        let (mut e, mut st, mut at) = library();
        // l'ultimo l'ha trovato solo la ricerca sul server
        e.push(entry("Mare Aperto 7", Some(("Mare Aperto", 7))));
        st.push(Status::New);
        at.push(0);
        let d = ShelfData { found_from: 4, ..data(&e, &st, &at) };
        let view = (1920.0, 1080.0);
        let mut shelf = Shelf::default();
        let all = shelf.sections(&d);
        assert!(all.iter().all(|s| !s.tiles.contains(&Tile::Volume(4))), "nella libreria non c'e'");
        assert_eq!(
            all.last().unwrap().title,
            if italian() { "Tutta la libreria \u{b7} 4" } else { "The whole library \u{b7} 4" }
        );
        for c in "mare".chars() {
            shelf.key(Key::Char(c), &d, view, 1.0);
        }
        let secs = shelf.sections(&d);
        assert_eq!(secs.len(), 1, "in libreria niente: niente sezione vuota");
        assert_eq!(secs[0].tiles, [Tile::Volume(4)], "sul server si'");
        shelf.key(Key::Right, &d, view, 1.0);
        assert_eq!(
            shelf.key(Key::Enter, &d, view, 1.0),
            Handled::Yes(Some(Command::Open("Mare Aperto 7.cbz".into()))),
            "e si apre"
        );
        // qui "Mare Aperto 7" e' della libreria: mentre si aspetta il
        // server, la sua sezione lo dice, sotto i trovati
        let waiting = ShelfData { found_from: 5, searching: true, ..data(&e, &st, &at) };
        let secs = shelf.sections(&waiting);
        assert_eq!(secs.len(), 2);
        assert_eq!(secs[0].tiles, [Tile::Volume(4)]);
        assert!(secs[1].tiles.is_empty() && !secs[1].title.is_empty());
        let answered = shelf.sections(&ShelfData { found_from: 5, ..data(&e, &st, &at) });
        assert_eq!(answered.len(), 1, "il server non ha altro: niente sezione");
        // con dei risultati in libreria, quelli del server vengono dopo
        shelf.search = "a".into();
        let both = shelf.sections(&d);
        assert_eq!(both.len(), 2);
        assert!(!both[0].tiles.is_empty() && both[1].tiles == [Tile::Volume(4)]);
    }

    #[test]
    fn con_le_frecce_e_invio_si_apre_un_volume() {
        let (e, st, at) = library();
        let d = data(&e, &st, &at);
        let mut shelf = Shelf::default();
        let view = (1920.0, 1080.0);
        shelf.key(Key::Right, &d, view, 1.0); // la prima cella: Nebbia v02, in lettura
        shelf.key(Key::Right, &d, view, 1.0); // Orbita Bassa
        assert_eq!(
            shelf.key(Key::Enter, &d, view, 1.0),
            Handled::Yes(Some(Command::Open("Orbita Bassa #12.cbz".into())))
        );
        shelf.key(Key::Down, &d, view, 1.0); // sotto: la pila di Nebbia sul Porto
        assert_eq!(shelf.key(Key::Enter, &d, view, 1.0), Handled::Yes(None), "la serie si apre, non si legge");
        assert_eq!(shelf.series.as_deref(), Some("Nebbia sul Porto"));
    }
}
