//! Cosa va a schermo, e dove; cosa preparare in anticipo.

use super::*;

impl Reader {
    /// Pronta da disegnare: preparata e di misura nota, o illeggibile (nera).
    pub(super) fn showable(&self, pages: &impl Pages, i: usize) -> bool {
        self.broken[i] || (pages.has(i) && self.native[i].is_some())
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

    pub(super) fn paged_items(&self, start: usize) -> Vec<Item> {
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
}
