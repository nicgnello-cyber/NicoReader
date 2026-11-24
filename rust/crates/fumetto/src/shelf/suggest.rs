//! Mentre si scrive nella ricerca, sotto si apre una tendina con i tag della
//! libreria che rispondono ("azio" -> Azione, Azioni di guerra...): prima
//! quelli che cominciano cosi', poi i piu' usati, ognuno con la sua
//! categoria e quanti volumi lo hanno. Un clic, o le frecce e Invio, porta
//! fra i volumi di quel tag. I volumi trovati restano sotto, come prima.

use super::draw::BarGeo;
use super::*;
use crate::ui::rgb;

/// Quanti tag nella tendina, al massimo.
const SHOWN: usize = 8;
/// Da quante lettere scritte compare.
const FROM: usize = 2;

/// Un tag suggerito: la categoria, il nome, quanti volumi lo hanno.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Suggestion {
    pub kind: GroupKind,
    pub name: String,
    pub count: usize,
}

/// Dove sta la tendina: a destra allineata alla ricerca, sotto la barra.
pub(super) struct DropGeo {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    /// L'altezza di una riga, e lo spazio sopra la prima (il titolo).
    pub row: f32,
    pub head: f32,
}

impl Shelf {
    /// I tag che rispondono alla ricerca. Nessuno fra i tag (li' la ricerca
    /// filtra gia' i tag) o con meno di due lettere scritte.
    pub(super) fn suggestions(&self, d: &ShelfData) -> Vec<Suggestion> {
        let needle = self.search.trim().to_lowercase();
        if self.groups || needle.chars().count() < FROM {
            return Vec::new();
        }
        // per categoria e nome, senza badare alle maiuscole
        let mut found: HashMap<(GroupKind, String), Suggestion> = HashMap::new();
        for e in &d.entries[..d.library_len()] {
            for g in e.groups.iter().filter(|g| g.name.to_lowercase().contains(&needle)) {
                found
                    .entry((g.kind, g.name.to_lowercase()))
                    .or_insert_with(|| Suggestion { kind: g.kind, name: g.name.clone(), count: 0 })
                    .count += 1;
            }
        }
        let starts = |s: &Suggestion| s.name.to_lowercase().starts_with(&needle);
        let mut v: Vec<Suggestion> = found.into_values().collect();
        v.sort_by(|a, b| {
            starts(b).cmp(&starts(a)).then(b.count.cmp(&a.count)).then_with(|| natural_cmp(&a.name, &b.name))
        });
        v.truncate(SHOWN);
        v
    }

    /// Porta fra i volumi del tag suggerito: la ricerca ha fatto il suo.
    pub(super) fn open_suggestion(&mut self, s: Suggestion) {
        self.search.clear();
        self.suggested = None;
        self.enter_group(s.kind, s.name);
    }

    /// La tendina, se ci sono suggerimenti.
    pub(super) fn dropdown(&self, d: &ShelfData, g: &Geo, m: &mut dyn Measure) -> Option<Layer> {
        let found = self.suggestions(d);
        if found.is_empty() {
            return None;
        }
        let s = g.s;
        let geo = drop_geo(d, self, g.view, s);
        let h = geo.head + found.len() as f32 * geo.row + 8.0 * s;
        let mut l = Layer::default();
        // un'ombra morbida, il fondo, un filo attorno
        l.rects.push(Rect::new(geo.x, geo.y + 6.0 * s, geo.w, h, [0.0, 0.0, 0.0, 0.6]).radius(10.0 * s).blur(14.0 * s));
        l.rects.push(Rect::new(geo.x, geo.y, geo.w, h, alpha(rgb(0x16, 0x15, 0x13), 0.99)).radius(10.0 * s));
        l.rects.push(Rect::new(geo.x, geo.y, geo.w, h, HAIR).radius(10.0 * s).stroke(s.round().max(1.0)));
        let pad = 14.0 * s;
        l.texts.push(caps(t("Tag", "Tags"), 9.5 * s, MUTED).on_baseline(geo.x + pad, geo.y + geo.head - 9.0 * s));
        for (i, sug) in found.iter().enumerate() {
            let y = geo.y + geo.head + i as f32 * geo.row;
            let on = self.suggested == Some(i) || self.hover == Some(Hit::Suggestion(i));
            if on {
                let fill = [1.0, 1.0, 1.0, 0.07];
                l.rects.push(Rect::new(geo.x + 6.0 * s, y, geo.w - 12.0 * s, geo.row, fill).radius(6.0 * s));
            }
            let kind = format!("{} \u{00b7} {}", kind_label(sug.kind, false), sug.count);
            let kind = caps(&kind, 9.0 * s, MUTED).tabular();
            let kind_w = m.width(&kind);
            l.texts.push(kind.on_baseline(geo.x + geo.w - pad - kind_w, y + geo.row * 0.62));
            let room = geo.w - 2.0 * pad - kind_w - 12.0 * s;
            let name = fit(
                m,
                Text::new(sug.name.as_str(), Face::Sans, 14.0 * s, if on { INK } else { fade(INK, 0.85) }),
                room,
            );
            l.texts.push(name.on_baseline(geo.x + pad, y + geo.row * 0.64));
        }
        Some(l)
    }

    /// Il suggerimento sotto il punto, se la tendina e' aperta.
    pub(super) fn suggestion_at(&self, d: &ShelfData, view: (f32, f32), s: f32, x: f32, y: f32) -> Option<Hit> {
        let n = self.suggestions(d).len();
        if n == 0 {
            return None;
        }
        let geo = drop_geo(d, self, view, s);
        let top = geo.y + geo.head;
        let inside = x >= geo.x && x < geo.x + geo.w && y >= geo.y && y < top + n as f32 * geo.row + 8.0 * s;
        if !inside {
            return None;
        }
        // sul titolo o sul bordo: la tendina, ma nessun tag
        let i = ((y - top) / geo.row).floor();
        Some(if i >= 0.0 && (i as usize) < n { Hit::Suggestion(i as usize) } else { Hit::Bar })
    }
}

fn drop_geo(d: &ShelfData, shelf: &Shelf, view: (f32, f32), s: f32) -> DropGeo {
    let bar = BarGeo::new(d, view.0, s, !shelf.search.is_empty());
    let w = (380.0 * s).max(bar.search_w).min(view.0 - 16.0 * s).round();
    let x = (bar.search_x + bar.search_w - w).clamp(8.0 * s, (view.0 - w - 8.0 * s).max(8.0 * s)).round();
    DropGeo { x, y: (ui::hud_height(s) + 6.0 * s).round(), w, row: (36.0 * s).round(), head: (30.0 * s).round() }
}
