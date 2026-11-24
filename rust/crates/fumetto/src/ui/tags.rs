//! I tag di un volume, dal lettore (E) o dalla libreria: chi c'e' (i
//! personaggi, i gruppi), chi l'ha fatto, di che categoria e tipologia e', in
//! che lingua, dove si svolge, e le etichette di chi legge. Si sceglie la
//! categoria, si scrive (i tag gia' usati nella libreria si suggeriscono),
//! Invio lo aggiunge. Un clic su un tag porta in libreria, fra gli altri
//! volumi che ce l'hanno: "questo personaggio c'e' anche in...".
//!
//! Un volume di una serie ha due ambiti: lui solo, o tutta la serie (Tab, o
//! un clic sopra le categorie); dal menu di una pila, solo la serie. Un tag
//! che hanno solo alcuni volumi lo dice ("3/12"), e si puo' dare a tutti.
//!
//! Il modulo non salva niente: chiede all'app di aggiungere o togliere, e
//! l'app gli rimanda i tag come sono dopo (`update`).

use std::cell::{Cell, RefCell};
use std::path::PathBuf;

use fumetto_core::library::{Group, GroupKind, same_name};
use fumetto_core::natural_cmp;

use super::form::Outcome;
use super::*;
use crate::shelf::kind_label;

/// Le categorie, nell'ordine in cui si scelgono.
pub(super) const KINDS: [GroupKind; 11] = [
    GroupKind::Character,
    GroupKind::Team,
    GroupKind::Author,
    GroupKind::Genre,
    GroupKind::Format,
    GroupKind::Language,
    GroupKind::Location,
    GroupKind::Publisher,
    GroupKind::Collection,
    GroupKind::ReadingList,
    GroupKind::Tag,
];

/// Quanti suggerimenti sotto il campo.
const SUGGESTIONS: usize = 6;
/// Quanto puo' essere lungo un tag.
const MAX_TAG: usize = 200;

/// Di chi sono i tag aperti: da qui l'app li ricalcola dopo ogni cambio.
#[derive(Clone, Debug, PartialEq)]
pub enum TagsOf {
    Volume(PathBuf),
    Series(String),
    /// I volumi scelti in libreria con Ctrl+clic.
    Volumes(Vec<PathBuf>),
}

/// A chi vanno i tag di un ambito.
#[derive(Clone, Debug, PartialEq)]
pub enum Reach {
    /// Il volume solo.
    Volume,
    /// Tutti i volumi della serie.
    Series(String),
    /// I volumi scelti.
    Picked,
}

/// Un ambito: i volumi a cui vanno i tag, e i tag che hanno, con quanti
/// volumi ce l'hanno.
#[derive(Clone, Debug)]
pub struct TagScope {
    pub reach: Reach,
    pub paths: Vec<PathBuf>,
    pub tags: Vec<(Group, usize)>,
}

impl TagScope {
    /// Un tag che hanno tutti i volumi dell'ambito.
    fn complete(&self, count: usize) -> bool {
        count >= self.paths.len()
    }

    fn has_all(&self, g: &Group) -> bool {
        self.tags.iter().any(|(t, n)| t.same(g) && self.complete(*n))
    }
}

/// Cio' che si puo' cliccare.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Hit {
    /// Un ambito: il volume, la serie.
    Scope(usize),
    /// Una categoria (indice in `KINDS`).
    Kind(usize),
    Suggestion(usize),
    /// Un tag del volume: il nome (porta agli altri volumi), la crocetta.
    Show(usize),
    Remove(usize),
}

pub(crate) struct TagsForm {
    of: TagsOf,
    title: String,
    /// Il volume e la sua serie, o la serie sola; mai vuoto.
    scopes: Vec<TagScope>,
    /// L'ambito scelto (indice in `scopes`).
    scope: usize,
    /// I tag della libreria, con quanti volumi li hanno: i suggerimenti.
    known: Vec<(Group, usize)>,
    /// La categoria scelta (indice in `KINDS`).
    kind: usize,
    typed: String,
    /// Il suggerimento scelto con le frecce.
    pick: Option<usize>,
    hover: Option<Hit>,
    /// Dove sta cio' che si clicca: lo scrive `layer`, che misura i testi.
    hits: RefCell<Vec<([f32; 4], Hit)>>,
    /// Quanti suggerimenti ci stanno sotto il campo (in una finestra bassa
    /// meno, perche' si vedano i tag del volume): lo scrive `layer`.
    rows: Cell<usize>,
}

impl TagsForm {
    /// `scopes` non vuoto: il primo e' quello che si vede all'inizio.
    pub(super) fn new(
        of: TagsOf, title: String, scopes: Vec<TagScope>, known: Vec<(Group, usize)>, kind: usize,
    ) -> TagsForm {
        debug_assert!(!scopes.is_empty());
        TagsForm {
            of,
            title,
            scopes,
            scope: 0,
            known,
            kind: kind.min(KINDS.len() - 1),
            typed: String::new(),
            pick: None,
            hover: None,
            hits: RefCell::new(Vec::new()),
            rows: Cell::new(SUGGESTIONS),
        }
    }

    /// La categoria scelta, da ricordare per la prossima volta.
    pub(super) fn kind_index(&self) -> usize {
        self.kind
    }

    pub(super) fn of(&self) -> &TagsOf {
        &self.of
    }

    /// I tag come sono dopo un cambio; l'ambito scelto resta.
    pub(super) fn update(&mut self, scopes: Vec<TagScope>, known: Vec<(Group, usize)>) {
        if scopes.is_empty() {
            return;
        }
        self.scope = self.scope.min(scopes.len() - 1);
        (self.scopes, self.known) = (scopes, known);
        self.pick = None;
        self.hover = None;
    }

    fn current(&self) -> &TagScope {
        &self.scopes[self.scope]
    }

    fn choose_scope(&mut self, scope: usize) {
        if scope < self.scopes.len() {
            self.scope = scope;
            self.pick = None;
        }
    }

    /// I tag della libreria della categoria scelta che non tutti i volumi
    /// dell'ambito hanno, che contengono cio' che si e' scritto: prima quelli
    /// che cominciano cosi', poi i piu' usati.
    fn suggestions(&self) -> Vec<(&Group, usize)> {
        let kind = KINDS[self.kind];
        let needle = self.typed.trim().to_lowercase();
        let starts = |g: &Group| g.name.to_lowercase().starts_with(&needle);
        // mentre si scrive, anche quelli delle altre categorie: dopo, con la
        // loro categoria accanto ("wolverine" fra i gruppi e' un personaggio)
        let mut v: Vec<(&Group, usize)> = self
            .known
            .iter()
            .filter(|(g, _)| (g.kind == kind || !needle.is_empty()) && !self.current().has_all(g))
            .filter(|(g, _)| needle.is_empty() || g.name.to_lowercase().contains(&needle))
            .map(|(g, n)| (g, *n))
            .collect();
        v.sort_by(|a, b| {
            (b.0.kind == kind)
                .cmp(&(a.0.kind == kind))
                .then(starts(b.0).cmp(&starts(a.0)))
                .then(b.1.cmp(&a.1))
                .then_with(|| natural_cmp(&a.0.name, &b.0.name))
        });
        v.truncate(self.rows.get());
        v
    }

    pub(super) fn key(&mut self, key: Key) -> Outcome {
        let n = self.suggestions().len();
        match key {
            Key::Escape => return Outcome::Close,
            Key::Enter => return self.add(),
            Key::Left => self.choose_kind((self.kind + KINDS.len() - 1) % KINDS.len()),
            Key::Right => self.choose_kind((self.kind + 1) % KINDS.len()),
            Key::Tab if self.scopes.len() > 1 => self.choose_scope((self.scope + 1) % self.scopes.len()),
            Key::Tab => self.choose_kind((self.kind + 1) % KINDS.len()),
            Key::Down if n > 0 => self.pick = Some(self.pick.map_or(0, |i| (i + 1).min(n - 1))),
            Key::Up => self.pick = self.pick.and_then(|i| i.checked_sub(1)),
            Key::Backspace => {
                self.typed.pop();
                self.pick = None;
            }
            Key::Char(c) => self.type_text(&c.to_string()),
            Key::Digit(d) => self.type_text(&d.to_string()),
            _ => {}
        }
        Outcome::Stay
    }

    fn choose_kind(&mut self, kind: usize) {
        self.kind = kind;
        self.pick = None;
    }

    /// Testo scritto o incollato, senza a capo.
    pub(super) fn type_text(&mut self, text: &str) {
        for c in text.chars().filter(|c| !c.is_control()) {
            if self.typed.chars().count() < MAX_TAG {
                self.typed.push(c);
            }
        }
        self.pick = None;
    }

    /// Aggiunge il suggerimento scelto, o cio' che si e' scritto (con il nome
    /// come e' gia' scritto in libreria, se c'e': "batman" e' "Batman").
    fn add(&mut self) -> Outcome {
        let kind = KINDS[self.kind];
        // un suggerimento scelto porta la sua categoria
        let picked = self.pick.and_then(|i| self.suggestions().get(i).map(|(g, _)| (g.kind, g.name.clone())));
        let (kind, name) = match picked {
            Some(picked) => picked,
            None => {
                let typed = self.typed.trim();
                if typed.is_empty() {
                    return Outcome::Stay;
                }
                let known = self.known.iter().find(|(g, _)| g.kind == kind && same_name(&g.name, typed));
                (kind, known.map_or_else(|| typed.to_owned(), |(g, _)| g.name.clone()))
            }
        };
        self.typed.clear();
        self.pick = None;
        let tag = Group::new(kind, name, None);
        let scope = &mut self.scopes[self.scope];
        if scope.has_all(&tag) {
            return Outcome::Stay; // ce l'hanno gia'
        }
        // si vede subito; l'app poi rimanda i tag come sono davvero
        let all = scope.paths.len();
        match scope.tags.iter_mut().find(|(t, _)| t.same(&tag)) {
            Some((_, n)) => *n = all,
            None => scope.tags.push((tag.clone(), all)),
        }
        Outcome::Submit(Command::AddTag(scope.paths.clone(), tag))
    }

    fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        let hits = self.hits.borrow();
        hits.iter().find(|([hx, hy, hw, hh], _)| x >= *hx && x < hx + hw && y >= *hy && y < hy + hh).map(|(_, h)| *h)
    }

    /// Il mouse si e' mosso: `true` se va ridisegnato.
    pub(super) fn motion(&mut self, x: f32, y: f32) -> bool {
        let hover = self.hit(x, y);
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    pub(super) fn click(&mut self, x: f32, y: f32) -> Outcome {
        match self.hit(x, y) {
            Some(Hit::Scope(i)) => self.choose_scope(i),
            Some(Hit::Kind(k)) => self.choose_kind(k),
            Some(Hit::Suggestion(i)) => {
                self.pick = Some(i);
                return self.add();
            }
            Some(Hit::Show(i)) if i < self.current().tags.len() => {
                let g = &self.current().tags[i].0;
                return Outcome::Submit(Command::ShowGroup(g.kind, g.name.clone()));
            }
            Some(Hit::Remove(i)) if i < self.current().tags.len() => {
                let scope = &mut self.scopes[self.scope];
                let (tag, _) = scope.tags.remove(i);
                self.hover = None;
                return Outcome::Submit(Command::RemoveTag(scope.paths.clone(), tag));
            }
            Some(Hit::Show(_) | Hit::Remove(_)) | None => {}
        }
        Outcome::Stay
    }

    pub(super) fn layer(&self, ctx: &Context, m: &mut dyn Measure) -> Layer {
        let s = ctx.scale;
        let (vw, vh) = ctx.view;
        let w = (680.0 * s).min(vw - 48.0 * s).round();
        let x0 = ((vw - w) / 2.0).round();
        let mut l = Layer::default();
        let mut hits = Vec::new();
        // nero pieno: le trasparenze si mescolano in luce lineare, e anche a
        // 0,99 il testo chiaro di una pagina si leggerebbe sotto i tag
        l.rects.push(Rect::new(0.0, 0.0, vw, vh, BLACK));
        // in una finestra bassa, piu' raccolto: senza la riga d'aiuto
        let compact = vh / s < 620.0;
        let top = if compact { 34.0 * s } else { (vh / 2.0 - 300.0 * s).max(40.0 * s) }.round();
        // con due ambiti lo dicono le pastiglie sotto
        let n = volumes(self.current().paths.len());
        let head = match &self.current().reach {
            _ if self.scopes.len() > 1 => t("Tag", "Tags").to_owned(),
            Reach::Volume => t("Tag del volume", "Volume tags").to_owned(),
            Reach::Series(_) => format!("{} \u{00b7} {n}", t("Tag della serie", "Series tags")),
            Reach::Picked => format!("{} \u{00b7} {n}", t("Tag dei volumi scelti", "Tags of the picked volumes")),
        };
        l.texts.push(caps(&head, 11.0 * s, MUTED).on_baseline(x0, top));
        let title = fit(m, Text::new(self.title.as_str(), Face::Serif, 32.0 * s, INK), w);
        l.texts.push(title.on_baseline(x0, top + 40.0 * s));
        if !compact {
            let help = t(
                "Chi c\u{2019}\u{e8}, chi l\u{2019}ha fatto, di che genere \u{e8}: per ritrovare gli altri volumi con lo stesso tag.",
                "Who\u{2019}s in it, who made it, what kind it is: to find the other volumes with the same tag.",
            );
            l.texts.push(fit(m, Text::new(help, Face::Sans, 13.0 * s, MUTED), w).on_baseline(x0, top + 66.0 * s));
        }
        let mut kinds_y = top + if compact { 62.0 } else { 92.0 } * s;
        if self.scopes.len() > 1 {
            kinds_y = self.draw_scopes(&mut l, &mut hits, (x0, w, kinds_y), s, m) + 18.0 * s;
        }
        let y = self.draw_kinds(&mut l, &mut hits, (x0, w, kinds_y), s, m);
        let field_y = y + 64.0 * s;
        // i suggerimenti che ci stanno lasciando posto ad almeno due righe di
        // tag; se non ce n'e', solo mentre si scrive
        let room = (vh - 64.0 * s - (field_y + 22.0 * s) - 130.0 * s) / (30.0 * s).round();
        let least = if self.typed.is_empty() { 0 } else { 2 };
        self.rows.set((room.max(0.0) as usize).clamp(least, SUGGESTIONS));
        self.draw_field(&mut l, (x0, w, field_y), s, m);
        let y = self.draw_suggestions(&mut l, &mut hits, (x0, w, field_y + 22.0 * s), s, m);
        self.draw_tags(&mut l, &mut hits, (x0, w, y + 30.0 * s), vh - 64.0 * s, s, m);
        let keys = match (self.scopes.len() > 1, italian()) {
            (true, true) => {
                "Tab volume o serie   \u{00b7}   \u{2190} \u{2192} la categoria   \u{00b7}   \u{2191} \u{2193} i suggerimenti   \u{00b7}   Invio aggiunge   \u{00b7}   Esc chiude"
            }
            (true, false) => {
                "Tab volume or series   \u{00b7}   \u{2190} \u{2192} category   \u{00b7}   \u{2191} \u{2193} suggestions   \u{00b7}   Enter adds   \u{00b7}   Esc closes"
            }
            (false, true) => {
                "\u{2190} \u{2192} la categoria   \u{00b7}   \u{2191} \u{2193} i suggerimenti   \u{00b7}   Invio aggiunge   \u{00b7}   un clic su un tag mostra gli altri volumi   \u{00b7}   Esc chiude"
            }
            (false, false) => {
                "\u{2190} \u{2192} category   \u{00b7}   \u{2191} \u{2193} suggestions   \u{00b7}   Enter adds   \u{00b7}   click a tag to see the other volumes   \u{00b7}   Esc closes"
            }
        };
        let keys = fit(m, Text::new(keys, Face::Sans, 12.0 * s, fade(MUTED, 0.8)), vw - 48.0 * s);
        l.texts.push(keys.boxed(vw - 48.0 * s, Align::Center).on_baseline(24.0 * s, vh - 28.0 * s));
        *self.hits.borrow_mut() = hits;
        l
    }

    /// I due ambiti, come le categorie ma piu' grandi: "Questo volume",
    /// "Tutta la serie · 12 volumi". Restituisce dove finiscono.
    fn draw_scopes(
        &self, l: &mut Layer, hits: &mut Vec<([f32; 4], Hit)>, (x0, w, y): (f32, f32, f32), s: f32, m: &mut dyn Measure,
    ) -> f32 {
        let (h, pad, gap) = ((34.0 * s).round(), 16.0 * s, 10.0 * s);
        let mut x = x0;
        for (i, scope) in self.scopes.iter().enumerate() {
            let on = i == self.scope;
            let hovered = self.hover == Some(Hit::Scope(i));
            let n = volumes(scope.paths.len());
            let label = match &scope.reach {
                Reach::Volume => t("Questo volume", "This volume").to_owned(),
                Reach::Series(_) => format!("{} \u{00b7} {n}", t("Tutta la serie", "The whole series")),
                Reach::Picked => format!("{} \u{00b7} {n}", t("I volumi scelti", "The picked volumes")),
            };
            let color = if on {
                INK
            } else if hovered {
                fade(INK, 0.9)
            } else {
                MUTED
            };
            let text = fit(m, Text::new(label, Face::Sans, 13.5 * s, color), w / 2.0 - 2.0 * pad);
            let cw = (m.width(&text) + 2.0 * pad).round();
            let fill = if on { [1.0, 1.0, 1.0, 0.12] } else { [1.0, 1.0, 1.0, if hovered { 0.06 } else { 0.0 }] };
            l.rects.push(Rect::new(x, y, cw, h, fill).radius(h / 2.0));
            if on {
                l.rects.push(Rect::new(x, y, cw, h, alpha(ACCENT, 0.9)).radius(h / 2.0).stroke(s.round().max(1.0)));
            } else {
                l.rects.push(Rect::new(x, y, cw, h, alpha(HAIR, 0.9)).radius(h / 2.0).stroke(s.round().max(1.0)));
            }
            l.texts.push(text.on_baseline(x + pad, y + h / 2.0 + 4.5 * s));
            hits.push(([x, y, cw, h], Hit::Scope(i)));
            x += cw + gap;
        }
        y + h
    }

    /// Le categorie, come pastiglie che vanno a capo; quella scelta accesa.
    /// Restituisce dove finiscono.
    fn draw_kinds(
        &self, l: &mut Layer, hits: &mut Vec<([f32; 4], Hit)>, (x0, w, y0): (f32, f32, f32), s: f32,
        m: &mut dyn Measure,
    ) -> f32 {
        let (h, pad, gap) = ((30.0 * s).round(), 14.0 * s, 8.0 * s);
        let (mut x, mut y) = (x0, y0);
        for (k, kind) in KINDS.iter().enumerate() {
            let on = k == self.kind;
            let hovered = self.hover == Some(Hit::Kind(k));
            let text = caps(kind_label(*kind, false), 10.0 * s, if on || hovered { INK } else { MUTED });
            let cw = (m.width(&text) + 2.0 * pad).round();
            if x + cw > x0 + w && x > x0 {
                (x, y) = (x0, y + h + gap);
            }
            let fill = if on { [1.0, 1.0, 1.0, 0.12] } else { [1.0, 1.0, 1.0, if hovered { 0.06 } else { 0.0 }] };
            l.rects.push(Rect::new(x, y, cw, h, fill).radius(h / 2.0));
            if !on {
                l.rects.push(Rect::new(x, y, cw, h, alpha(HAIR, 0.9)).radius(h / 2.0).stroke(s.round().max(1.0)));
            }
            l.texts.push(text.on_baseline(x + pad, y + h / 2.0 + 3.5 * s));
            hits.push(([x, y, cw, h], Hit::Kind(k)));
            x += cw + gap;
        }
        y + h
    }

    /// Il campo: il nome della categoria sopra, il testo, la riga rossa, il cursore.
    fn draw_field(&self, l: &mut Layer, (x0, w, y): (f32, f32, f32), s: f32, m: &mut dyn Measure) {
        let label = if italian() {
            format!("Aggiungi: {}", kind_label(KINDS[self.kind], false).to_lowercase())
        } else {
            format!("Add: {}", kind_label(KINDS[self.kind], false).to_lowercase())
        };
        l.texts.push(caps(&label, 9.5 * s, INK).on_baseline(x0, y - 30.0 * s));
        let text = fit(m, Text::new(self.typed.as_str(), Face::Sans, 17.0 * s, INK), w - 12.0 * s);
        let tw = if self.typed.is_empty() { 0.0 } else { m.width(&text) };
        l.texts.push(text.on_baseline(x0, y));
        l.rects.push(Rect::new(x0, (y + 9.0 * s).round(), w, (2.0 * s).round(), ACCENT));
        let h = (20.0 * s).round();
        l.rects.push(Rect::new((x0 + tw + 2.0 * s).round(), (y - h + 4.0 * s).round(), s.round().max(1.0), h, ACCENT));
    }

    /// Sotto il campo, i tag della libreria che vanno bene (con quanti volumi
    /// li hanno); o, se non ce ne sono, cosa fara' Invio. Restituisce dove
    /// finiscono: lo spazio e' sempre quello di tutte le righe, perche' i tag
    /// del volume sotto non saltino mentre si scrive.
    fn draw_suggestions(
        &self, l: &mut Layer, hits: &mut Vec<([f32; 4], Hit)>, (x0, w, y0): (f32, f32, f32), s: f32,
        m: &mut dyn Measure,
    ) -> f32 {
        let row = (30.0 * s).round();
        let found = self.suggestions();
        if found.is_empty() && self.rows.get() > 0 {
            let typed = self.typed.trim();
            let line = match (typed.is_empty(), italian()) {
                (true, true) => "Nessun tag di questa categoria in libreria: scrivine uno nuovo.".to_owned(),
                (true, false) => "No tags of this kind in the library yet: type a new one.".to_owned(),
                (false, true) => format!("Invio per il nuovo tag \u{201c}{typed}\u{201d}"),
                (false, false) => format!("Enter for the new tag \u{201c}{typed}\u{201d}"),
            };
            let text = fit(m, Text::new(line, Face::Sans, 13.5 * s, MUTED), w);
            l.texts.push(text.on_baseline(x0, y0 + row * 0.7));
        }
        for (i, (g, count)) in found.iter().enumerate() {
            let y = y0 + i as f32 * row;
            let on = self.pick == Some(i) || self.hover == Some(Hit::Suggestion(i));
            if on {
                l.rects.push(Rect::new(x0 - 10.0 * s, y, w + 20.0 * s, row, [1.0, 1.0, 1.0, 0.07]).radius(6.0 * s));
            }
            let name =
                fit(m, Text::new(g.name.as_str(), Face::Sans, 14.5 * s, if on { INK } else { MUTED }), w - 190.0 * s);
            l.texts.push(name.on_baseline(x0, y + row * 0.68));
            // di un'altra categoria: lo dice, accanto al numero
            let n = match g.kind == KINDS[self.kind] {
                true => volumes(*count),
                false => format!("{} \u{00b7} {}", kind_label(g.kind, false), volumes(*count)),
            };
            let n = Text::new(n, Face::Sans, 11.5 * s, fade(MUTED, 0.8)).tabular().boxed(180.0 * s, Align::Right);
            l.texts.push(n.on_baseline(x0 + w - 180.0 * s, y + row * 0.68));
            hits.push(([x0 - 10.0 * s, y, w + 20.0 * s, row], Hit::Suggestion(i)));
        }
        // lo spazio di quanti suggerimenti ha la categoria a campo vuoto: mentre
        // si scrive diventano meno, e i tag del volume sotto non saltano
        let all = self.known.iter().filter(|(g, _)| g.kind == KINDS[self.kind] && !self.current().has_all(g));
        y0 + all.count().clamp(1, self.rows.get().max(1)) as f32 * row
    }

    /// I tag del volume, per categoria: il nome (porta agli altri volumi) e la
    /// crocetta per toglierlo. Fino a `bottom`; quelli che non ci stanno, contati.
    fn draw_tags(
        &self, l: &mut Layer, hits: &mut Vec<([f32; 4], Hit)>, (x0, w, y0): (f32, f32, f32), bottom: f32, s: f32,
        m: &mut dyn Measure,
    ) {
        let scope = self.current();
        let tags = &scope.tags;
        let head = match (&scope.reach, italian()) {
            (Reach::Volume, true) => format!("Su questo volume \u{00b7} {}", tags.len()),
            (Reach::Volume, false) => format!("On this volume \u{00b7} {}", tags.len()),
            (Reach::Series(_), true) => format!("Sui volumi della serie \u{00b7} {}", tags.len()),
            (Reach::Series(_), false) => format!("On the volumes of the series \u{00b7} {}", tags.len()),
            (Reach::Picked, true) => format!("Sui volumi scelti \u{00b7} {}", tags.len()),
            (Reach::Picked, false) => format!("On the picked volumes \u{00b7} {}", tags.len()),
        };
        l.texts.push(caps(&head, 10.0 * s, MUTED).on_baseline(x0, y0));
        l.rects.push(Rect::new(x0, (y0 + 10.0 * s).round(), w, s.round(), alpha(HAIR, 0.7)));
        if tags.is_empty() {
            let none = t("Nessun tag, per ora.", "No tags yet.");
            l.texts.push(Text::new(none, Face::Sans, 13.5 * s, MUTED).on_baseline(x0, y0 + 40.0 * s));
            return;
        }
        let (h, gap, label_w) = ((28.0 * s).round(), 8.0 * s, (128.0 * s).round());
        let mut y = y0 + 24.0 * s;
        let mut hidden = 0;
        for kind in KINDS {
            let mine: Vec<usize> = (0..tags.len()).filter(|&i| tags[i].0.kind == kind).collect();
            if mine.is_empty() {
                continue;
            }
            if y + h > bottom {
                hidden += mine.len();
                continue;
            }
            let label = caps(kind_label(kind, mine.len() > 1), 9.5 * s, MUTED);
            l.texts.push(fit(m, label, label_w - 10.0 * s).on_baseline(x0, y + h / 2.0 + 3.5 * s));
            let mut x = x0 + label_w;
            for i in mine {
                let (shown, close) = (Hit::Show(i), Hit::Remove(i));
                let name_color = if self.hover == Some(shown) { INK } else { fade(INK, 0.9) };
                let (tag, count) = &tags[i];
                let name = fit(m, Text::new(tag.name.as_str(), Face::Sans, 13.5 * s, name_color), w * 0.6);
                // su quanti volumi dell'ambito, se non tutti
                let part = (!scope.complete(*count)).then(|| {
                    let n = format!("{count}/{}", scope.paths.len());
                    Text::new(n, Face::Sans, 11.0 * s, fade(MUTED, 0.9)).tabular()
                });
                let pw = part.as_ref().map_or(0.0, |p| m.width(p) + 7.0 * s);
                let (nw, xw) = (m.width(&name), (22.0 * s).round());
                let cw = (12.0 * s + nw + pw + 6.0 * s + xw).round();
                if x + cw > x0 + w && x > x0 + label_w {
                    (x, y) = (x0 + label_w, y + h + gap);
                    if y + h > bottom {
                        hidden += 1;
                        continue;
                    }
                }
                let hovered = matches!(self.hover, Some(Hit::Show(j) | Hit::Remove(j)) if j == i);
                let fill = [1.0, 1.0, 1.0, if hovered { 0.08 } else { 0.04 }];
                l.rects.push(Rect::new(x, y, cw, h, fill).radius(h / 2.0));
                l.texts.push(name.on_baseline(x + 12.0 * s, y + h / 2.0 + 4.5 * s));
                if let Some(part) = part {
                    l.texts.push(part.on_baseline(x + 12.0 * s + nw + 7.0 * s, y + h / 2.0 + 4.0 * s));
                }
                let xx = x + cw - xw;
                let x_color = if self.hover == Some(close) { INK_F } else { MUTED_F };
                draw_icon(l, Icon::Close, (xx + xw / 2.0 - 3.0 * s).round(), (y + h / 2.0).round(), s * 0.6, x_color);
                hits.push(([x, y, cw - xw, h], shown));
                hits.push(([xx, y, xw, h], close));
                x += cw + gap;
            }
            y += h + gap + 4.0 * s;
        }
        if hidden > 0 {
            let more = if italian() { format!("e altri {hidden}") } else { format!("and {hidden} more") };
            l.texts.push(Text::new(more, Face::Sans, 12.5 * s, MUTED).on_baseline(x0 + label_w, y + 14.0 * s));
        }
    }
}

/// "12 volumi", "1 volume".
fn volumes(n: usize) -> String {
    match (n == 1, italian()) {
        (true, _) => "1 volume".to_owned(),
        (false, true) => format!("{n} volumi"),
        (false, false) => format!("{n} volumes"),
    }
}
