//! La libreria: le copertine in una griglia sul nero, come quadri appesi.
//!
//! In alto la barra della libreria: il nome, i filtri (tutti, in lettura, da
//! leggere, letti), la ricerca (basta scrivere), le cartelle. Sotto, se non
//! si cerca e non si filtra, prima una fila "Continua a leggere", poi tutta
//! la libreria con le serie raccolte in una pila; un clic sulla pila la apre.
//! Le celle si allargano per riempire la finestra: niente vuoti a destra.
//!
//! Come `ui`, qui non c'e' grafica: si decide cosa sta dove, cosa scrivere e
//! cosa fare di tasti e clic. Le copertine le disegna la scheda video (sono
//! immagini, come le pagine) nei riquadri che dice [`Shelf::cover_slots`]; il
//! resto e' una [`Layer`] dell'interfaccia sopra di loro.

use std::path::{Path, PathBuf};
use std::time::Instant;

use fumetto_core::library::{Entry, Status, by_number};
use fumetto_core::lingua::{italian, t};
use fumetto_core::natural_cmp;
use fumetto_render::{Align, Face, Layer, Measure, Rect, Text};

use crate::reader::Action;
use crate::strip::glide_step;
use crate::ui::{self, ACCENT, BLACK, Command, HAIR, Handled, INK, INK_F, Icon, Key, MUTED, MUTED_F, TICK, alpha, caps, draw_icon, fit, rgb};

/// Cio' che la libreria deve sapere, dall'app.
pub struct ShelfData<'a> {
    pub entries: &'a [Entry],
    /// Allineati con `entries`: dove si e' arrivati, e quando.
    pub status: &'a [Status],
    pub read_at: &'a [u64],
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
    Series { name: String, cover: usize, count: usize, done: usize },
}

impl Tile {
    /// Il volume di cui si mostra la copertina.
    fn cover(&self) -> usize {
        match self {
            Tile::Volume(i) => *i,
            Tile::Series { cover, .. } => *cover,
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
    /// Dentro una serie: la cima dell'intestazione.
    series_head: Option<f32>,
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
        let mut visible: Vec<usize> = (0..d.entries.len())
            .filter(|&i| self.filter.keeps(d.status[i]) && found(&d.entries[i]))
            .filter(|&i| self.series.is_none() || d.entries[i].series == self.series)
            .collect();
        let recent_first = |v: &mut Vec<usize>| v.sort_by_key(|&i| std::cmp::Reverse(d.read_at[i]));

        if self.series.is_some() {
            visible.sort_by(|&a, &b| by_number(&d.entries[a], &d.entries[b]));
            return vec![Section { title: String::new(), tiles: visible.into_iter().map(Tile::Volume).collect(), one_row: false }];
        }
        if self.filter != Filter::All || !needle.is_empty() {
            if self.filter == Filter::Reading {
                recent_first(&mut visible);
            }
            let title = if needle.is_empty() { self.filter.label().to_owned() } else { t("Trovati", "Found").to_owned() };
            return vec![Section { title, tiles: visible.into_iter().map(Tile::Volume).collect(), one_row: false }];
        }

        let mut sections = Vec::new();
        let mut reading: Vec<usize> = visible.iter().copied().filter(|&i| matches!(d.status[i], Status::Reading(..))).collect();
        if !reading.is_empty() {
            recent_first(&mut reading);
            sections.push(Section {
                title: t("Continua a leggere", "Continue reading").to_owned(),
                tiles: reading.into_iter().map(Tile::Volume).collect(),
                one_row: true,
            });
        }
        // le serie con piu' volumi diventano una pila, al posto del primo
        let mut tiles: Vec<Tile> = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        for &i in &visible {
            let Some(name) = d.entries[i].series.as_deref() else {
                tiles.push(Tile::Volume(i));
                continue;
            };
            if seen.contains(&name) {
                continue;
            }
            let mut members: Vec<usize> = visible.iter().copied().filter(|&j| d.entries[j].series.as_deref() == Some(name)).collect();
            if members.len() < 2 {
                tiles.push(Tile::Volume(i));
                continue;
            }
            seen.push(name);
            members.sort_by(|&a, &b| by_number(&d.entries[a], &d.entries[b]));
            let done = members.iter().filter(|&&j| d.status[j] == Status::Done).count();
            // in copertina il volume che si sta leggendo, se c'e'; se no il primo
            let cover = members.iter().copied().find(|&j| matches!(d.status[j], Status::Reading(..))).unwrap_or(members[0]);
            tiles.push(Tile::Series { name: name.to_owned(), cover, count: members.len(), done });
        }
        tiles.sort_by(|a, b| natural_cmp(&tile_name(d, a), &tile_name(d, b)));
        let n = d.entries.len();
        sections.push(Section {
            title: if italian() { format!("Tutta la libreria \u{00b7} {n}") } else { format!("The whole library \u{00b7} {n}") },
            tiles,
            one_row: false,
        });
        sections
    }

    fn layout(&self, d: &ShelfData, g: &Geo) -> Layout {
        let s = g.s;
        let mut y = g.top + (30.0 * s).round() - self.scroll;
        let start = y;
        let mut placed = Vec::new();
        let mut heads = Vec::new();
        let mut series_head = None;
        if self.series.is_some() {
            series_head = Some(y);
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
        Layout { placed, heads, series_head, height: y - start + (40.0 * s).round() }
    }

    fn max_scroll(&self, d: &ShelfData, g: &Geo) -> f32 {
        let at = Shelf { scroll: 0.0, ..self.clone_view() };
        (at.layout(d, g).height - (g.view.1 - g.top)).max(0.0)
    }

    /// Una copia con gli stessi filtri (per misurare senza toccare lo scorrimento).
    fn clone_view(&self) -> Shelf {
        Shelf { filter: self.filter, search: self.search.clone(), series: self.series.clone(), ..Shelf::default() }
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
        if let Some(top) = lay.series_head
            && y >= top && y < top + 40.0 * s && x < g.pad + 200.0 * s
        {
            return Some(Hit::Back);
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
            Some(Hit::Return) => Handled::Yes(Some(Command::Act(Action::ToggleLibrary))),
            Some(Hit::Back) => {
                self.leave_series();
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
        }
    }

    pub fn enter_series(&mut self, name: String) {
        self.series = Some(name);
        self.reset_view();
    }

    fn leave_series(&mut self) {
        self.series = None;
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
                } else if self.series.is_some() {
                    self.leave_series();
                } else if d.reading {
                    return Handled::Yes(Some(Command::Act(Action::ToggleLibrary)));
                } else {
                    return Handled::Yes(None);
                }
            }
            Key::Backspace => {
                if self.search.pop().is_some() {
                    self.reset_view();
                } else if self.series.is_some() {
                    self.leave_series();
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
            Key::Char(_) => {}
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
        let delta = if p.y < lo { p.y - lo } else if p.y + g.row_h > hi { p.y + g.row_h - hi } else { 0.0 };
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
                ui::Row::Item { label: t("Apri la serie", "Open the series"), key: String::new(), on: false, cmd: Command::Series(name.clone()) },
            ],
            Tile::Volume(i) => {
                let path = d.entries[*i].path.clone();
                let mut rows = vec![ui::Row::Item { label: t("Leggi", "Read"), key: String::new(), on: false, cmd: Command::Open(path.clone()) }];
                rows.push(ui::Row::Sep);
                if d.status[*i] != Status::Done {
                    rows.push(ui::Row::Item { label: t("Segna come letto", "Mark as read"), key: String::new(), on: false,
                                              cmd: Command::MarkRead(path.clone(), true) });
                }
                if d.status[*i] != Status::New {
                    rows.push(ui::Row::Item { label: t("Segna come da leggere", "Mark as unread"), key: String::new(), on: false,
                                              cmd: Command::MarkRead(path.clone(), false) });
                }
                rows.push(ui::Row::Sep);
                rows.push(ui::Row::Item { label: t("Mostra nella cartella", "Show in folder"), key: String::new(), on: false,
                                          cmd: Command::Reveal(path.clone()) });
                rows.push(ui::Row::Item { label: t("Sposta nel cestino\u{2026}", "Move to trash\u{2026}"), key: String::new(), on: false,
                                          cmd: Command::Trash(path) });
                rows
            }
        }
    }

    /// Le righe del menu delle cartelle della libreria.
    pub(crate) fn folders_menu(d: &ShelfData) -> Vec<ui::Row> {
        let mut rows = vec![ui::Row::Head(t("Cartelle della libreria", "Library folders"))];
        for root in d.roots {
            rows.push(ui::Row::Folder { path: root.clone(), cmd: Command::RemoveFolder(root.clone()) });
        }
        rows.push(ui::Row::Sep);
        rows.push(ui::Row::Item { label: t("Aggiungi una cartella\u{2026}", "Add a folder\u{2026}"), key: String::new(), on: false,
                                  cmd: Command::Act(Action::AddLibraryFolder) });
        rows
    }

    /// Le copertine sono gia' disegnate: qui tutto il resto, sopra di loro.
    pub fn layers(&self, d: &ShelfData, view: (f32, f32), s: f32, m: &mut dyn Measure) -> Vec<Layer> {
        let g = Geo::new(view, s);
        let lay = self.layout(d, &g);
        let mut l = Layer::default();

        if let Some(top) = lay.series_head {
            let back_hover = self.hover == Some(Hit::Back);
            l.texts.push(caps(&format!("\u{2039}   {}", t("Libreria", "Library")), 11.0 * s, if back_hover { INK } else { MUTED })
                .on_baseline(g.pad, top + 20.0 * s));
            let name = self.series.clone().unwrap_or_default();
            l.texts.push(fit(m, Text::new(name, Face::Serif, 44.0 * s, INK), g.view.0 - 2.0 * g.pad).on_baseline(g.pad, top + 78.0 * s));
            if let Some(Section { tiles, .. }) = self.sections(d).into_iter().next() {
                let done = tiles.iter().filter(|t| d.status[t.cover()] == Status::Done).count();
                let text = if italian() { format!("{} volumi \u{00b7} {done} letti", tiles.len()) } else { format!("{} volumes \u{00b7} {done} read", tiles.len()) };
                l.texts.push(caps(&text, 11.0 * s, MUTED).on_baseline(g.pad, top + 104.0 * s));
            }
        }
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
                let plate = Text::new(e.title.as_str(), Face::Serif, 18.0 * s, MUTED).boxed(w - 28.0 * s, Align::Center);
                l.texts.push(plate.on_baseline(x + 14.0 * s, y + h / 2.0));
            }
            if let Tile::Series { .. } = p.tile {
                // la pila: due bordi che spuntano in alto a destra
                for (k2, a) in [(1.0, 0.55), (2.0, 0.3)] {
                    let o = (5.0 * k2 * s).round();
                    l.rects.push(Rect::line(x + o, y - o, x + w + o, y - o, s.max(1.0), alpha(MUTED_F, a)));
                    l.rects.push(Rect::line(x + w + o, y - o, x + w + o, y + h - o, s.max(1.0), alpha(MUTED_F, a)));
                }
            }
            if self.selected == Some(k) {
                let o = 5.0 * s;
                l.rects.push(Rect::new(x - o, y - o, w + 2.0 * o, h + 2.0 * o, ACCENT).radius(4.0 * s).stroke((2.0 * s).round()));
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
                Tile::Volume(i) => (d.entries[*i].title.clone(), meta_of(d.status[*i])),
                Tile::Series { name, count, done, .. } => (
                    name.clone(),
                    if italian() { format!("{count} volumi \u{00b7} {done} letti") } else { format!("{count} volumes \u{00b7} {done} read") },
                ),
            };
            l.texts.push(fit(m, Text::new(title, Face::Serif, 18.0 * s, INK), w).on_baseline(x, y + h + 34.0 * s));
            l.texts.push(Text::new(meta, Face::Sans, 12.0 * s, if hovered { INK } else { MUTED }).tabular()
                .on_baseline(x, y + h + 54.0 * s));
        }

        if lay.placed.is_empty() {
            let text = if d.scanning {
                t("Cerco i fumetti\u{2026}", "Looking for comics\u{2026}")
            } else if !self.search.is_empty() || self.filter != Filter::All {
                t("Nessun risultato", "Nothing found")
            } else {
                t("Nessun fumetto in queste cartelle", "No comics in these folders")
            };
            let wide = g.view.0;
            l.texts.push(Text::new(text, Face::Serif, 28.0 * s, MUTED).boxed(wide, Align::Center)
                .on_baseline(0.0, g.top + (g.view.1 - g.top) * 0.42));
        }
        vec![l, self.bar(d, &g, m)]
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
            let count = d.status.iter().filter(|st| f.keeps(**st)).count();
            let label = caps(&format!("{}  {count}", f.label()), 11.0 * s, color).tabular();
            l.texts.push(label.boxed(geo.tab_w, Align::Center).on_baseline(*x, cy + 4.0 * s));
            if on {
                let dd = 4.0 * s;
                l.rects.push(Rect::new(x + geo.tab_w / 2.0 - dd / 2.0, h - 10.0 * s, dd, dd, ACCENT).radius(dd));
            }
        }

        // la ricerca: basta scrivere
        let (sx, sw) = (geo.search_x, geo.search_w);
        l.rects.push(Rect::new(sx, (cy + 13.0 * s).round(), sw, s.round(), if self.search.is_empty() { HAIR } else { TICK }));
        draw_icon(&mut l, Icon::Search, (sx + 9.0 * s).round(), cy, s, MUTED_F);
        if self.search.is_empty() {
            l.texts.push(Text::new(t("Scrivi per cercare", "Type to search"), Face::Sans, 13.5 * s, MUTED)
                .on_baseline(sx + 26.0 * s, cy + 5.0 * s));
        } else {
            let text = fit(m, Text::new(self.search.as_str(), Face::Sans, 14.0 * s, INK).weight(450), sw - 60.0 * s);
            let tw = m.width(&text);
            l.texts.push(text.on_baseline(sx + 26.0 * s, cy + 5.0 * s));
            let caret = (sx + 28.0 * s + tw).round();
            l.rects.push(Rect::new(caret, (cy - 9.0 * s).round(), (2.0 * s).round(), (18.0 * s).round(), ACCENT));
            let color = if self.hover == Some(Hit::ClearSearch) { INK_F } else { MUTED_F };
            draw_icon(&mut l, Icon::Close, (sx + sw - 10.0 * s).round(), cy, s * 0.8, color);
        }

        for (hit, icon, x) in [(Hit::Folders, Icon::Open, geo.folders_x), (Hit::Return, Icon::Close, geo.return_x)] {
            let Some(x) = x else { continue };
            let hovered = self.hover == Some(hit.clone());
            let by = ((h - 36.0 * s) / 2.0).round();
            if hovered {
                l.rects.push(Rect::new(x, by, 36.0 * s, 36.0 * s, [1.0, 1.0, 1.0, 0.075]).radius(8.0 * s));
            }
            draw_icon(&mut l, icon, (x + 18.0 * s).round(), cy, s, if hovered { INK_F } else { MUTED_F });
            if hovered {
                let tip = match hit {
                    Hit::Folders => t("Cartelle della libreria", "Library folders"),
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
    return_x: Option<f32>,
}

impl BarGeo {
    fn new(d: &ShelfData, vw: f32, s: f32) -> BarGeo {
        let tab_w = (134.0 * s).round();
        let tabs_w = tab_w * 4.0;
        let x0 = ((vw - tabs_w) / 2.0).round();
        let mut r = vw - 14.0 * s;
        let return_x = d.reading.then(|| {
            r -= 36.0 * s;
            r.round()
        });
        r -= 36.0 * s + 4.0 * s;
        let folders_x = Some(r.round());
        let search_w = (250.0 * s).round().min((r - 16.0 * s) - (x0 + tabs_w + 16.0 * s)).max(120.0 * s);
        let search_x = (r - 16.0 * s - search_w).round();
        BarGeo { tabs: (0..4).map(|k| x0 + k as f32 * tab_w).collect(), tab_w, search_x, search_w, folders_x, return_x }
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
    if inside(g.return_x) {
        return Some(Hit::Return);
    }
    None
}

fn tile_name(d: &ShelfData, t: &Tile) -> String {
    match t {
        Tile::Volume(i) => d.entries[*i].title.clone(),
        Tile::Series { name, .. } => name.clone(),
    }
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
        }
    }

    fn data<'a>(entries: &'a [Entry], status: &'a [Status], read_at: &'a [u64]) -> ShelfData<'a> {
        ShelfData { entries, status, read_at, scanning: false, roots: &[], covered: &|_| true, reading: false }
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
    fn continua_a_leggere_poi_la_libreria_con_le_serie() {
        let (e, st, at) = library();
        let d = data(&e, &st, &at);
        let secs = Shelf::default().sections(&d);
        assert_eq!(secs.len(), 2);
        assert_eq!(secs[0].tiles, [Tile::Volume(2), Tile::Volume(3)], "in lettura, dal piu' recente");
        assert_eq!(secs[1].tiles, [
            Tile::Volume(0),
            Tile::Series { name: "Nebbia sul Porto".into(), cover: 2, count: 2, done: 1 },
            Tile::Volume(3), // una serie di un volume solo resta un volume
        ]);
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
    fn con_le_frecce_e_invio_si_apre_un_volume() {
        let (e, st, at) = library();
        let d = data(&e, &st, &at);
        let mut shelf = Shelf::default();
        let view = (1920.0, 1080.0);
        shelf.key(Key::Right, &d, view, 1.0); // la prima cella: Nebbia v02, in lettura
        shelf.key(Key::Right, &d, view, 1.0); // Orbita Bassa
        assert_eq!(shelf.key(Key::Enter, &d, view, 1.0), Handled::Yes(Some(Command::Open("Orbita Bassa #12.cbz".into()))));
        shelf.key(Key::Down, &d, view, 1.0); // sotto: la pila di Nebbia sul Porto
        assert_eq!(shelf.key(Key::Enter, &d, view, 1.0), Handled::Yes(None), "la serie si apre, non si legge");
        assert_eq!(shelf.series.as_deref(), Some("Nebbia sul Porto"));
    }
}
