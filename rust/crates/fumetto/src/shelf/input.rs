//! Tasti, clic, rotella e scorrimento morbido della libreria.

use super::*;

impl Shelf {
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
            Some(Hit::Sort) => Handled::Yes(Some(Command::SortMenu(x, ui::hud_height(s)))),
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
    pub(super) fn choose(&mut self, d: &ShelfData, tile: Tile) -> Option<Command> {
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

    pub fn wheel(&mut self, pixels: f32) {
        self.glide += pixels;
    }

    pub fn key(&mut self, key: Key, d: &ShelfData, view: (f32, f32), s: f32) -> Handled {
        let g = Geo::new(view, s);
        let lay = self.layout(d, &g);
        let n = lay.placed.len();
        match key {
            Key::Right => self.selected = nearest(&lay.placed, self.selected, Dir::Right),
            Key::Left => self.selected = nearest(&lay.placed, self.selected, Dir::Left),
            Key::Down => self.selected = nearest(&lay.placed, self.selected, Dir::Down),
            Key::Up => self.selected = nearest(&lay.placed, self.selected, Dir::Up),
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
}

#[derive(Clone, Copy)]
enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// La cella da scegliere con una freccia: la piu' vicina nella direzione
/// voluta (a destra e a sinistra, nella stessa fila). Se non ce n'e' si resta
/// dove si e'; senza una cella scelta si parte dalla prima.
fn nearest(placed: &[Placed], from: Option<usize>, dir: Dir) -> Option<usize> {
    let Some(c) = from.and_then(|k| placed.get(k)) else { return (!placed.is_empty()).then_some(0) };
    placed
        .iter()
        .enumerate()
        .filter(|(_, p)| match dir {
            Dir::Right => p.y == c.y && p.x > c.x,
            Dir::Left => p.y == c.y && p.x < c.x,
            Dir::Down => p.y > c.y,
            Dir::Up => p.y < c.y,
        })
        .min_by(|(_, a), (_, b)| {
            let da = ((a.y - c.y).abs(), (a.x - c.x).abs());
            let db = ((b.y - c.y).abs(), (b.x - c.x).abs());
            da.partial_cmp(&db).unwrap_or(Ordering::Equal)
        })
        .map(|(k, _)| k)
        .or(from)
}
