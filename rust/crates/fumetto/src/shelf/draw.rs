//! Il disegno della libreria sopra le copertine: l'intestazione, i titoli,
//! le celle e la barra in alto.

use super::*;

impl Shelf {
    /// Le copertine sono gia' disegnate: qui tutto il resto, sopra di loro.
    pub fn layers(&self, d: &ShelfData, view: (f32, f32), s: f32, m: &mut dyn Measure) -> Vec<Layer> {
        let g = Geo::new(view, s);
        let lay = self.layout(d, &g);
        let mut l = Layer::default();
        let crumbs = match lay.head {
            Some(top) => self.draw_head(&mut l, d, &g, top, lay.totals, m),
            None => Vec::new(),
        };
        *self.crumbs.borrow_mut() = crumbs;
        for (title, base) in &lay.heads {
            l.texts.push(caps(title, 11.0 * s, MUTED).on_baseline(g.pad, *base));
        }
        for (k, p) in lay.placed.iter().enumerate() {
            if p.y + g.row_h >= g.top && p.y <= g.view.1 {
                self.draw_tile(&mut l, d, &g, k, p, m);
            }
        }
        // mentre il server cerca, lo dice gia' il titolo della sua sezione
        if lay.placed.is_empty() && !(d.searching && !self.search.is_empty()) {
            self.draw_empty(&mut l, d, &g);
        }
        vec![l, self.bar(d, &g, m)]
    }

    /// L'intestazione dentro una serie, un gruppo, fra i gruppi o in una
    /// cartella: da dove si torna (o il percorso), dove si e', quanti volumi.
    /// Restituisce i pezzi del percorso, per i clic.
    fn draw_head(
        &self, l: &mut Layer, d: &ShelfData, g: &Geo, top: f32, totals: (usize, usize), m: &mut dyn Measure,
    ) -> Vec<Crumb> {
        let s = g.s;
        let base = top + 20.0 * s;
        let mut crumbs = Vec::new();
        let here = if self.browsing() {
            crumbs = self.draw_trail(l, d, g.pad, base, s, m);
            self.folder_here(d).0.as_deref().map(folder_name).unwrap_or_default()
        } else {
            let (to, here) = self.back_and_here(d);
            let color = if self.hover == Some(Hit::Back) { INK } else { MUTED };
            l.texts.push(caps(&format!("\u{2039}   {to}"), 11.0 * s, color).on_baseline(g.pad, base));
            here
        };
        l.texts.push(
            fit(m, Text::new(here, Face::Serif, 44.0 * s, INK), g.view.0 - 2.0 * g.pad)
                .on_baseline(g.pad, top + 78.0 * s),
        );
        let text = if self.groups && self.series.is_none() && self.group.is_none() {
            t("Dalle schede dei volumi e dai server", "From the volumes\u{2019} metadata and the servers").to_owned()
        } else {
            let count = volumes_read(totals.0, totals.1);
            match (&self.series, &self.group) {
                (None, Some((kind, _))) => format!("{} \u{00b7} {count}", kind_label(*kind, false)),
                _ => count,
            }
        };
        l.texts.push(caps(&text, 11.0 * s, MUTED).on_baseline(g.pad, top + 104.0 * s));
        crumbs
    }

    /// Il percorso dall'inizio, nella vista per cartelle: ogni pezzo porta li'.
    fn draw_trail(&self, l: &mut Layer, d: &ShelfData, pad: f32, base: f32, s: f32, m: &mut dyn Measure) -> Vec<Crumb> {
        let mut crumbs = Vec::new();
        let mut x = pad;
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
        crumbs
    }

    /// Fuori dalla vista per cartelle: dove si torna, e dove si e'.
    fn back_and_here(&self, d: &ShelfData) -> (String, String) {
        let home = match self.folder_here(d).0.filter(|_| self.folders) {
            Some(folder) => folder_name(&folder),
            None => t("Libreria", "Library").to_owned(),
        };
        match (&self.series, &self.group) {
            (Some(series), Some((_, group))) => (group.clone(), series.clone()),
            (Some(series), None) => (home, series.clone()),
            (None, Some((_, group))) => (groups_title().to_owned(), group.clone()),
            (None, None) => (home, groups_title().to_owned()),
        }
    }

    /// Una cella: la copertina (o la sua targa, finche' la copertina non
    /// c'e'), la pila o la cartella, la scelta, quanto se ne e' letto, il
    /// titolo e sotto i dettagli.
    fn draw_tile(&self, l: &mut Layer, d: &ShelfData, g: &Geo, k: usize, p: &Placed, m: &mut dyn Measure) {
        let s = g.s;
        let hovered = self.hover == Some(Hit::Tile(k));
        let (x, y, w, h) = (p.x, p.y - self.lift(k, s), p.w, p.h);
        let r = corner(s);
        let e = &d.entries[p.tile.cover()];
        let shown = (d.cover_shown)(&e.path).clamp(0.0, 1.0);
        if shown < 1.0 {
            // la targa con il titolo, che svanisce mentre la copertina compare
            let a = 1.0 - shown;
            l.rects.push(Rect::new(x, y, w, h, alpha(PLATE, a)).radius(r));
            let plate =
                Text::new(e.title.as_str(), Face::Serif, 18.0 * s, fade(MUTED, a)).boxed(w - 28.0 * s, Align::Center);
            l.texts.push(plate.on_baseline(x + 14.0 * s, y + h / 2.0));
        }
        // un filo di luce sul bordo: le copertine scure non si perdono nel nero
        l.rects.push(Rect::new(x, y, w, h, [1.0, 1.0, 1.0, 0.07]).radius(r).stroke(s.round().max(1.0)));
        draw_shape(l, &p.tile, x, y, w, h, s);
        if self.selected == Some(k) {
            let o = 5.0 * s;
            l.rects.push(
                Rect::new(x - o, y - o, w + 2.0 * o, h + 2.0 * o, ACCENT).radius(r + o).stroke((2.0 * s).round()),
            );
        }
        if let Tile::Volume(i) = p.tile {
            draw_progress(l, d.status[i], x, (y + h + 7.0 * s).round(), w, s);
        }
        let (title, meta) = self.caption(d, &p.tile);
        l.texts.push(fit(m, Text::new(title, Face::Serif, 18.0 * s, INK), w).on_baseline(x, y + h + 34.0 * s));
        l.texts.push(
            Text::new(meta, Face::Sans, 12.0 * s, if hovered { INK } else { MUTED })
                .tabular()
                .on_baseline(x, y + h + 54.0 * s),
        );
    }

    /// Sotto una cella: il titolo, e dove si e' arrivati (di un volume) o
    /// quanti volumi e quanti letti (di una pila o di una cartella).
    fn caption(&self, d: &ShelfData, tile: &Tile) -> (String, String) {
        match tile {
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
            | Tile::Folder { name, count, done, .. } => (name.clone(), volumes_read(*count, *done)),
        }
    }

    /// Niente da mostrare: perche'.
    fn draw_empty(&self, l: &mut Layer, d: &ShelfData, g: &Geo) {
        let text = if d.scanning {
            t("Cerco i fumetti\u{2026}", "Looking for comics\u{2026}")
        } else if !self.search.is_empty() || self.filter != Filter::All {
            t("Nessun risultato", "Nothing found")
        } else if self.groups {
            t("Nessuna raccolta, lista, genere o editore", "No collections, lists, genres or publishers")
        } else {
            t("Nessun fumetto in queste cartelle", "No comics in these folders")
        };
        l.texts.push(
            Text::new(text, Face::Serif, 28.0 * g.s, MUTED)
                .boxed(g.view.0, Align::Center)
                .on_baseline(0.0, g.top + (g.view.1 - g.top) * 0.42),
        );
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

    /// La barra della libreria: il nome, i filtri, la ricerca, i tasti.
    fn bar(&self, d: &ShelfData, g: &Geo, m: &mut dyn Measure) -> Layer {
        let s = g.s;
        let (vw, h) = (g.view.0, g.top);
        let cy = (h / 2.0).round();
        let mut l = Layer::default();
        l.rects.push(Rect::new(0.0, 0.0, vw, h, BLACK));
        l.rects.push(Rect::new(0.0, h - s.round(), vw, s.round(), alpha(HAIR, 0.8)));
        let geo = BarGeo::new(d, vw, s, !self.search.is_empty());
        if geo.logo {
            l.texts.push(
                Text::new("NicoReader", Face::Serif, 24.0 * s, INK).on_baseline((24.0 * s).round(), cy + 8.0 * s),
            );
        }
        self.draw_filters(&mut l, d, &geo, h, s);
        self.draw_search(&mut l, &geo, cy, s, m);
        self.draw_buttons(&mut l, &geo, vw, h, s, m);
        l
    }

    /// I filtri, ognuno con quanti volumi tiene (se c'e' posto); quello
    /// scelto ha il punto.
    fn draw_filters(&self, l: &mut Layer, d: &ShelfData, geo: &BarGeo, h: f32, s: f32) {
        let cy = (h / 2.0).round();
        let mut counts = [0usize; 4];
        for st in &d.status[..d.library_len()] {
            for (n, f) in counts.iter_mut().zip(Filter::ALL) {
                *n += usize::from(f.keeps(*st));
            }
        }
        // filtri stretti (finestra piccola): lettere un po' piu' piccole
        let size = 11.0 * s * (geo.tab_w / (134.0 * s)).clamp(0.8, 1.0);
        for ((f, x), count) in Filter::ALL.iter().zip(&geo.tabs).zip(counts) {
            let on = self.filter == *f;
            let hovered = self.hover == Some(Hit::Filter(*f));
            let color = if on || hovered { INK } else { MUTED };
            let text = if geo.counts { format!("{}  {count}", f.label()) } else { f.label().to_owned() };
            let label = caps(&text, size, color).tabular();
            l.texts.push(label.boxed(geo.tab_w, Align::Center).on_baseline(*x, cy + 4.0 * s));
            if on {
                let dd = 4.0 * s;
                l.rects.push(Rect::new(x + geo.tab_w / 2.0 - dd / 2.0, h - 10.0 * s, dd, dd, ACCENT).radius(dd));
            }
        }
    }

    /// La ricerca: basta scrivere. Con del testo, il cursore e la crocetta;
    /// senza, in una finestra molto stretta, solo la lente.
    fn draw_search(&self, l: &mut Layer, geo: &BarGeo, cy: f32, s: f32, m: &mut dyn Measure) {
        let (sx, sw) = (geo.search_x, geo.search_w);
        if geo.search_icon {
            draw_icon(l, Icon::Search, (sx + sw / 2.0).round(), cy, s, MUTED_F);
            return;
        }
        let line = if self.search.is_empty() { HAIR } else { TICK };
        l.rects.push(Rect::new(sx, (cy + 13.0 * s).round(), sw, s.round(), line));
        draw_icon(l, Icon::Search, (sx + 9.0 * s).round(), cy, s, MUTED_F);
        if self.search.is_empty() {
            l.texts.push(
                Text::new(t("Scrivi per cercare", "Type to search"), Face::Sans, 13.5 * s, MUTED)
                    .on_baseline(sx + 26.0 * s, cy + 5.0 * s),
            );
            return;
        }
        let text = fit(m, Text::new(self.search.as_str(), Face::Sans, 14.0 * s, INK).weight(450), sw - 60.0 * s);
        let tw = m.width(&text);
        l.texts.push(text.on_baseline(sx + 26.0 * s, cy + 5.0 * s));
        let caret = (sx + 28.0 * s + tw).round();
        l.rects.push(Rect::new(caret, (cy - 9.0 * s).round(), (2.0 * s).round(), (18.0 * s).round(), ACCENT));
        let color = if self.hover == Some(Hit::ClearSearch) { INK_F } else { MUTED_F };
        draw_icon(l, Icon::Close, (sx + sw - 10.0 * s).round(), cy, s * 0.8, color);
    }

    /// I tasti a destra: gruppi, cartelle, ordine, cartelle della libreria,
    /// ritorno alla lettura. Sotto il mouse, il suggerimento.
    fn draw_buttons(&self, l: &mut Layer, geo: &BarGeo, vw: f32, h: f32, s: f32, m: &mut dyn Measure) {
        let cy = (h / 2.0).round();
        let by = ((h - 36.0 * s) / 2.0).round();
        for (hit, icon, x) in geo.buttons() {
            let hovered = self.hover.as_ref() == Some(&hit);
            let on = (hit == Hit::Groups && self.groups) || (hit == Hit::Browse && self.browsing());
            if hovered {
                l.rects.push(Rect::new(x, by, 36.0 * s, 36.0 * s, [1.0, 1.0, 1.0, 0.075]).radius(8.0 * s));
            }
            draw_icon(l, icon, (x + 18.0 * s).round(), cy, s, if hovered || on { INK_F } else { MUTED_F });
            if on {
                let dd = 4.0 * s;
                l.rects.push(Rect::new(x + 18.0 * s - dd / 2.0, h - 10.0 * s, dd, dd, ACCENT).radius(dd));
            }
            if hovered {
                ui::tooltip(l, m, &self.button_tip(&hit), x + 18.0 * s, h, vw, s);
            }
        }
    }

    /// Il suggerimento di un tasto della barra.
    fn button_tip(&self, hit: &Hit) -> String {
        match hit {
            Hit::Groups => t("Raccolte, liste, generi, editori", "Collections, lists, genres, publishers").to_owned(),
            Hit::Folders => t("Cartelle della libreria", "Library folders").to_owned(),
            Hit::Browse if self.browsing() => t("Torna alle serie", "Back to the series").to_owned(),
            Hit::Browse => t("Per cartelle, come su disco", "By folder, as on disk").to_owned(),
            Hit::Sort => format!("{}: {}", t("Ordine", "Order"), sort_label(self.sort).to_lowercase()),
            _ => t("Torna alla lettura   \u{00b7}   Esc", "Back to reading   \u{00b7}   Esc").to_owned(),
        }
    }
}

/// La forma di una cella che non e' un volume: la pila (due bordi che
/// spuntano in alto a destra) o la cartella (la linguetta e un bordo dietro).
fn draw_shape(l: &mut Layer, tile: &Tile, x: f32, y: f32, w: f32, h: f32, s: f32) {
    match tile {
        Tile::Series { .. } | Tile::Group { .. } => {
            for (k, a) in [(1.0, 0.55), (2.0, 0.3)] {
                let o = (5.0 * k * s).round();
                l.rects.push(Rect::line(x + o, y - o, x + w + o, y - o, s.max(1.0), alpha(MUTED_F, a)));
                l.rects.push(Rect::line(x + w + o, y - o, x + w + o, y + h - o, s.max(1.0), alpha(MUTED_F, a)));
            }
        }
        Tile::Folder { .. } => {
            let (tab_w, tab_h) = ((w * 0.38).round(), (7.0 * s).round());
            l.rects.push(Rect::new(x, y - tab_h, tab_w, tab_h + 2.0 * s, alpha(MUTED_F, 0.7)).radius(2.0 * s));
            l.rects.push(Rect::line(x + tab_w, y - 1.0, x + w, y - 1.0, (2.0 * s).max(1.0), alpha(MUTED_F, 0.7)));
        }
        Tile::Volume(_) => {}
    }
}

/// Quanto se ne e' letto: un filo sotto la copertina, pieno se finito.
fn draw_progress(l: &mut Layer, status: Status, x: f32, y: f32, w: f32, s: f32) {
    let thick = (2.0 * s).round();
    match status {
        Status::Reading(page, pages) if pages > 0 => {
            l.rects.push(Rect::new(x, y, w, thick, HAIR));
            let f = ((page + 1) as f32 / pages as f32).clamp(0.02, 1.0);
            l.rects.push(Rect::new(x, y, (w * f).round(), thick, INK_F));
        }
        Status::Done => l.rects.push(Rect::new(x, y, w, thick, TICK)),
        _ => {}
    }
}

/// "12 volumi · 3 letti".
fn volumes_read(n: usize, done: usize) -> String {
    if italian() { format!("{n} volumi \u{00b7} {done} letti") } else { format!("{n} volumes \u{00b7} {done} read") }
}

fn meta_of(s: Status) -> String {
    match s {
        Status::New => t("nuovo", "new").to_owned(),
        Status::Reading(page, pages) => format!("{} / {pages}", page + 1),
        Status::Done => t("letto", "read").to_owned(),
    }
}

/// Dove stanno i pezzi della barra della libreria (senza misurare i testi).
pub(super) struct BarGeo {
    /// Il nome a sinistra: in una finestra stretta lascia il posto.
    pub(super) logo: bool,
    pub(super) tabs: Vec<f32>,
    pub(super) tab_w: f32,
    /// Quanti volumi tiene ogni filtro, accanto al suo nome.
    pub(super) counts: bool,
    pub(super) search_x: f32,
    pub(super) search_w: f32,
    /// Della ricerca, senza testo, si vede solo la lente.
    pub(super) search_icon: bool,
    pub(super) folders_x: Option<f32>,
    /// La vista per cartelle.
    pub(super) browse_x: Option<f32>,
    /// L'ordine.
    pub(super) sort_x: Option<f32>,
    /// Solo se qualche volume fa parte di un gruppo.
    pub(super) groups_x: Option<f32>,
    pub(super) return_x: Option<f32>,
}

/// Come stringere la barra quando la finestra si stringe, un passo dopo
/// l'altro: prima filtri e ricerca piu' stretti, poi via il nome, poi via i
/// numeri dei filtri, infine la ricerca si riduce alla lente (si cerca
/// comunque scrivendo; mentre si cerca, il testo prende il posto dei filtri).
struct Squeeze {
    logo: bool,
    counts: bool,
    /// La larghezza minima di un filtro, in punti.
    tab_min: f32,
    search_icon: bool,
}

const SQUEEZES: [Squeeze; 4] = [
    Squeeze { logo: true, counts: true, tab_min: 96.0, search_icon: false },
    Squeeze { logo: false, counts: true, tab_min: 96.0, search_icon: false },
    Squeeze { logo: false, counts: false, tab_min: 72.0, search_icon: false },
    Squeeze { logo: false, counts: false, tab_min: 72.0, search_icon: true },
];

impl BarGeo {
    /// `searching`: c'e' del testo nella ricerca.
    pub(super) fn new(d: &ShelfData, vw: f32, s: f32, searching: bool) -> BarGeo {
        let step = 36.0 * s + 4.0 * s;
        let mut r = vw - 14.0 * s;
        let return_x = d.reading.then(|| {
            r -= 36.0 * s;
            r.round()
        });
        let mut next = || {
            r -= step;
            r.round()
        };
        let (folders_x, browse_x, sort_x) = (Some(next()), Some(next()), Some(next()));
        let groups_x = d.entries[..d.library_len()].iter().any(|e| !e.groups.is_empty()).then(&mut next);
        let mut geo = BarGeo {
            logo: true,
            tabs: Vec::new(),
            tab_w: (134.0 * s).round(),
            counts: true,
            search_x: 0.0,
            search_w: 0.0,
            search_icon: false,
            folders_x,
            browse_x,
            sort_x,
            groups_x,
            return_x,
        };
        geo.place_middle(vw, r - 16.0 * s, s, searching);
        geo
    }

    /// I filtri e la ricerca, fra il nome e i tasti (la ricerca finisce in
    /// `right`). Con spazio, i filtri al centro della finestra; se no, il
    /// primo passo di `SQUEEZES` con cui ci stanno, subito prima della
    /// ricerca. Se non ci stanno neanche cosi', restano la ricerca e i tasti.
    fn place_middle(&mut self, vw: f32, right: f32, s: f32, searching: bool) {
        let (gap, search_min, icon_w) = (16.0 * s, 120.0 * s, 28.0 * s);
        let full = self.tab_w;
        let centered = ((vw - full * 4.0) / 2.0).round();
        let x0 = if centered + full * 4.0 <= right - search_min - gap {
            centered
        } else {
            // quanto resta ai filtri, da dove cominciano
            let room = |q: &Squeeze| {
                let left = if q.logo { 150.0 * s } else { 24.0 * s };
                let search = if q.search_icon { icon_w } else { search_min };
                (left, right - search - gap - left)
            };
            let last = &SQUEEZES[SQUEEZES.len() - 1];
            let q = SQUEEZES.iter().find(|q| room(q).1 >= q.tab_min * s * 4.0).unwrap_or(last);
            let (left, space) = room(q);
            (self.logo, self.counts, self.search_icon) = (q.logo, q.counts, q.search_icon);
            self.tab_w = if space >= q.tab_min * s * 4.0 { full.min((space / 4.0).floor()) } else { 0.0 };
            (left + space - self.tab_w * 4.0).round().max(left)
        };
        self.tabs = if self.tab_w > 0.0 { (0..4).map(|k| x0 + k as f32 * self.tab_w).collect() } else { Vec::new() };
        if self.search_icon && searching {
            // mentre si cerca, il testo prende il posto dei filtri
            let left = if self.tab_w > 0.0 { x0 } else { 24.0 * s };
            (self.tabs, self.search_icon, self.search_x, self.search_w) =
                (Vec::new(), false, left, (right - left).max(0.0));
            return;
        }
        // senza filtri (una finestra strettissima) la ricerca comincia dal margine
        let tabs_end = if self.tabs.is_empty() { 24.0 * s - gap } else { x0 + self.tab_w * 4.0 };
        let room = (right - (tabs_end + gap)).max(0.0);
        self.search_w =
            if self.search_icon { if room >= icon_w { icon_w } else { 0.0 } } else { (250.0 * s).round().min(room) };
        self.search_x = (right - self.search_w).round();
    }

    /// I tasti che ci sono, con la loro icona e dove stanno.
    pub(super) fn buttons(&self) -> impl Iterator<Item = (Hit, Icon, f32)> + use<> {
        [
            (Hit::Groups, Icon::Shelf, self.groups_x),
            (Hit::Browse, Icon::Tree, self.browse_x),
            (Hit::Sort, Icon::Sort, self.sort_x),
            (Hit::Folders, Icon::Open, self.folders_x),
            (Hit::Return, Icon::Close, self.return_x),
        ]
        .into_iter()
        .filter_map(|(hit, icon, x)| Some((hit, icon, x?)))
    }
}

/// Cosa c'e' nella barra in `x`.
pub(super) fn bar_hit(d: &ShelfData, shelf: &Shelf, view: (f32, f32), s: f32, x: f32) -> Option<Hit> {
    let g = BarGeo::new(d, view.0, s, !shelf.search.is_empty());
    if let Some(k) = g.tabs.iter().position(|&tx| x >= tx && x < tx + g.tab_w) {
        return Some(Hit::Filter(Filter::ALL[k]));
    }
    if !shelf.search.is_empty() && x >= g.search_x + g.search_w - 24.0 * s && x < g.search_x + g.search_w + 4.0 * s {
        return Some(Hit::ClearSearch);
    }
    g.buttons().find(|&(_, _, bx)| x >= bx && x < bx + 36.0 * s).map(|(hit, _, _)| hit)
}
