//! Le impostazioni: un foglio sul lato destro della finestra, come la scheda
//! accanto a un quadro. Le pagine restano visibili a sinistra, cosi'
//! luminosita', contrasto e gamma si vedono cambiare mentre li si regola.
//!
//! Quattro sezioni: Immagine, Lettura, Lente, Tasti. Come il resto
//! dell'interfaccia, qui si decide cosa sta dove e cosa fanno tasti e clic;
//! le scelte escono come [`Pref`] e le applica (e salva) l'app.

use fumetto_core::Settings;
use fumetto_core::lingua::{italian, t};
use fumetto_render::{Align, Face, Layer, Measure, Rect, Text};

use crate::keys::{Bind, Combo, Keymap};
use crate::reader::Action;
use crate::ui::{
    ACCENT, Command, HAIR, Handled, INK, INK_F, Icon, Key, MUTED, MUTED_F, TICK, alpha, caps, draw_icon, fit, rgb,
};

mod draw;
mod rows;
#[cfg(test)]
mod tests;

/// Una scelta fatta nelle impostazioni.
#[derive(Clone, Debug, PartialEq)]
pub enum Pref {
    Brightness(i32),
    Contrast(i32),
    Gamma(i32),
    /// Luminosita', contrasto e gamma di serie.
    ResetImage,
    Webtoon(bool),
    Trim(bool),
    Upscale(bool),
    Hud(bool),
    /// Chiedere a GitHub se c'e' una versione nuova.
    CheckUpdates(bool),
    /// Secondi della presentazione.
    Slideshow(u32),
    LensZoom(f32),
    /// Il raggio della lente, in punti.
    LensSize(f32),
    /// Un tasto per un comando (tolto a chi l'aveva).
    Bind(Bind, Combo),
    /// Il comando resta senza tasti.
    Unbind(Bind),
    /// Tutti i tasti di serie.
    ResetKeys,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Image,
    Reading,
    Lens,
    Keys,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Image, Tab::Reading, Tab::Lens, Tab::Keys];

    fn label(self) -> &'static str {
        match self {
            Tab::Image => t("Immagine", "Image"),
            Tab::Reading => t("Lettura", "Reading"),
            Tab::Lens => t("Lente", "Magnifier"),
            Tab::Keys => t("Tasti", "Keys"),
        }
    }
}

/// Le regolazioni che si fanno scorrendo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slider {
    Brightness,
    Contrast,
    Gamma,
    LensZoom,
    LensSize,
}

impl Slider {
    /// (minimo, massimo, passo di tastiera, dove comincia il riempimento)
    fn range(self) -> (f32, f32, f32, f32) {
        match self {
            Slider::Brightness | Slider::Contrast | Slider::Gamma => (-100.0, 100.0, 5.0, 0.0),
            Slider::LensZoom => (1.5, 6.0, 0.25, 1.5),
            Slider::LensSize => (80.0, 320.0, 10.0, 80.0),
        }
    }

    fn value(self, s: &Settings) -> f32 {
        match self {
            Slider::Brightness => s.brightness as f32,
            Slider::Contrast => s.contrast as f32,
            Slider::Gamma => s.gamma as f32,
            Slider::LensZoom => s.lens_zoom,
            Slider::LensSize => s.lens_size,
        }
    }

    fn pref(self, v: f32) -> Pref {
        let (lo, hi, step, _) = self.range();
        let v = ((v / step).round() * step).clamp(lo, hi);
        match self {
            Slider::Brightness => Pref::Brightness(v as i32),
            Slider::Contrast => Pref::Contrast(v as i32),
            Slider::Gamma => Pref::Gamma(v as i32),
            Slider::LensZoom => Pref::LensZoom(v),
            Slider::LensSize => Pref::LensSize(v),
        }
    }

    fn shown(self, v: f32) -> String {
        match self {
            Slider::Brightness | Slider::Contrast | Slider::Gamma if v > 0.0 => format!("+{}", v as i32),
            Slider::Brightness | Slider::Contrast | Slider::Gamma => format!("{}", v as i32),
            Slider::LensZoom => {
                let s = format!("{v:.2}");
                let s = s.trim_end_matches('0').trim_end_matches('.');
                let s = if italian() { s.replace('.', ",") } else { s.to_owned() };
                format!("{s}\u{00d7}")
            }
            Slider::LensSize => format!("{}", v as i32),
        }
    }
}

/// Una riga del foglio.
#[derive(Clone, Debug)]
enum Row {
    Head(&'static str),
    Slider { label: &'static str, which: Slider },
    Toggle { label: &'static str, on: bool, pref: Pref },
    Stepper { label: &'static str, value: String, less: Pref, more: Pref },
    Button { label: &'static str, pref: Pref },
    Key { bind: Bind, keys: Vec<String> },
    Note(&'static str),
}

impl Row {
    fn height(&self) -> f32 {
        match self {
            Row::Head(_) => 44.0,
            Row::Slider { .. } => 66.0,
            Row::Toggle { .. } | Row::Stepper { .. } | Row::Button { .. } => 46.0,
            Row::Key { .. } => 38.0,
            Row::Note(_) => 56.0,
        }
    }

    fn selectable(&self) -> bool {
        !matches!(self, Row::Head(_) | Row::Note(_))
    }
}

/// Cio' che sta sotto il mouse.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Hit {
    Tab(Tab),
    Close,
    Row(usize),
    /// Il meno e il piu' di una riga a passi.
    Less(usize),
    More(usize),
}

const WIDTH: f32 = 440.0;
/// Dove cominciano le righe, sotto titolo e sezioni.
const HEADER: f32 = 124.0;

/// Le misure del foglio per questa finestra.
struct Geo {
    s: f32,
    view: (f32, f32),
    x: f32,
    w: f32,
    pad: f32,
}

impl Geo {
    fn new(view: (f32, f32), s: f32) -> Geo {
        let w = (WIDTH * s).min(view.0).round();
        Geo { s, view, x: (view.0 - w).round(), w, pad: (32.0 * s).round() }
    }

    /// La pista di una regolazione: (x, larghezza, y).
    fn track(&self, top: f32) -> (f32, f32, f32) {
        (self.x + self.pad, self.w - 2.0 * self.pad, (top + 48.0 * self.s).round())
    }

    /// Il bordo destro del contenuto.
    fn right(&self) -> f32 {
        self.x + self.w - self.pad
    }

    fn tab_x(&self, i: usize) -> (f32, f32) {
        let tw = (self.w - 2.0 * self.pad) / Tab::ALL.len() as f32;
        (self.x + self.pad + i as f32 * tw, tw)
    }
}

#[derive(Default)]
pub struct Prefs {
    tab: Tab,
    scroll: f32,
    hover: Option<Hit>,
    /// La riga scelta con la tastiera.
    selected: Option<usize>,
    /// Aspetta il tasto nuovo per questo comando.
    capture: Option<Bind>,
    /// La regolazione che si sta trascinando.
    drag: Option<Slider>,
}

impl Prefs {
    pub fn open(tab: Tab) -> Prefs {
        Prefs { tab, ..Prefs::default() }
    }

    /// Il comando che aspetta un tasto nuovo.
    pub fn capturing(&self) -> Option<Bind> {
        self.capture
    }

    /// E' arrivato il tasto (o `None`: Esc, si lascia com'era; il tasto
    /// cancella toglie quello che c'era lo decide l'app).
    pub fn captured(&mut self, combo: Option<Combo>) -> Option<Command> {
        let bind = self.capture.take()?;
        combo.map(|c| Command::Pref(Pref::Bind(bind, c)))
    }

    pub fn unbind_captured(&mut self) -> Option<Command> {
        self.capture.take().map(|b| Command::Pref(Pref::Unbind(b)))
    }

    /// Il foglio copre questo punto (i clic li' sono suoi).
    pub fn covers(view: (f32, f32), s: f32, x: f32) -> bool {
        x >= Geo::new(view, s).x
    }

    /// La cima di ogni riga, in pixel della finestra (gia' scorse).
    fn tops(&self, rows: &[Row], g: &Geo) -> Vec<f32> {
        let mut at = HEADER * g.s - self.scroll;
        rows.iter()
            .map(|r| {
                let top = at;
                at += r.height() * g.s;
                top
            })
            .collect()
    }

    fn max_scroll(&self, rows: &[Row], g: &Geo) -> f32 {
        let content: f32 = rows.iter().map(Row::height).sum::<f32>() * g.s;
        (content + 40.0 * g.s - (g.view.1 - HEADER * g.s)).max(0.0)
    }

    fn hit(&self, rows: &[Row], g: &Geo, x: f32, y: f32) -> Option<Hit> {
        if x < g.x {
            return None;
        }
        let s = g.s;
        let close_x = g.x + g.w - g.pad - 30.0 * s;
        if y < HEADER * s {
            if x >= close_x - 8.0 * s && y < 70.0 * s {
                return Some(Hit::Close);
            }
            let tab = (0..Tab::ALL.len()).find(|&i| {
                let (tx, tw) = g.tab_x(i);
                x >= tx && x < tx + tw && (72.0 * s..HEADER * s).contains(&y)
            });
            return tab.map(|i| Hit::Tab(Tab::ALL[i]));
        }
        let tops = self.tops(rows, g);
        let i = tops.iter().zip(rows).position(|(&top, r)| y >= top && y < top + r.height() * s)?;
        if let Row::Stepper { .. } = rows[i] {
            let right = g.x + g.w - g.pad;
            if x >= right - 28.0 * s {
                return Some(Hit::More(i));
            }
            if x >= right - 110.0 * s && x < right - 76.0 * s {
                return Some(Hit::Less(i));
            }
        }
        rows[i].selectable().then_some(Hit::Row(i))
    }

    pub fn motion(&mut self, settings: &Settings, keys: &Keymap, view: (f32, f32), s: f32, x: f32, y: f32) -> bool {
        let g = Geo::new(view, s);
        let hover = self.hit(&self.rows(settings, keys), &g, x, y);
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    /// Il tasto del mouse premuto: su una regolazione, la sposta subito (e
    /// comincia a trascinarla).
    pub fn press(
        &mut self, settings: &Settings, keys: &Keymap, view: (f32, f32), s: f32, x: f32, y: f32,
    ) -> Option<Command> {
        let g = Geo::new(view, s);
        let rows = self.rows(settings, keys);
        let Some(Hit::Row(i)) = self.hit(&rows, &g, x, y) else { return None };
        let Row::Slider { which, .. } = rows[i] else { return None };
        self.drag = Some(which);
        self.selected = Some(i);
        self.slide(&g, &rows, i, x)
    }

    /// Il mouse si muove con il tasto premuto.
    pub fn drag(&mut self, settings: &Settings, keys: &Keymap, view: (f32, f32), s: f32, x: f32) -> Option<Command> {
        let which = self.drag?;
        let g = Geo::new(view, s);
        let rows = self.rows(settings, keys);
        let i = rows.iter().position(|r| matches!(r, Row::Slider { which: w, .. } if *w == which))?;
        self.slide(&g, &rows, i, x)
    }

    fn slide(&self, g: &Geo, rows: &[Row], i: usize, x: f32) -> Option<Command> {
        let Row::Slider { which, .. } = rows[i] else { return None };
        let (tx, tw, _) = g.track(0.0);
        let (lo, hi, _, _) = which.range();
        let f = ((x - tx) / tw).clamp(0.0, 1.0);
        Some(Command::Pref(which.pref(lo + f * (hi - lo))))
    }

    /// Un clic (tasto rilasciato senza trascinare).
    pub fn click(&mut self, settings: &Settings, keys: &Keymap, view: (f32, f32), s: f32, x: f32, y: f32) -> Handled {
        let g = Geo::new(view, s);
        let rows = self.rows(settings, keys);
        let was_drag = self.drag.take().is_some();
        let cmd = match self.hit(&rows, &g, x, y) {
            Some(Hit::Close) => Some(Command::Act(Action::ToggleSettings)),
            Some(Hit::Tab(tab)) => {
                self.switch(tab);
                None
            }
            Some(Hit::Less(i)) => match &rows[i] {
                Row::Stepper { less, .. } => Some(Command::Pref(less.clone())),
                _ => None,
            },
            Some(Hit::More(i)) => match &rows[i] {
                Row::Stepper { more, .. } => Some(Command::Pref(more.clone())),
                _ => None,
            },
            Some(Hit::Row(i)) if !was_drag => {
                self.selected = Some(i);
                self.choose(&rows[i])
            }
            _ => None,
        };
        Handled::Yes(cmd)
    }

    /// Invio (o un clic) su una riga.
    fn choose(&mut self, row: &Row) -> Option<Command> {
        match row {
            Row::Toggle { pref, .. } | Row::Button { pref, .. } => Some(Command::Pref(pref.clone())),
            Row::Key { bind, .. } => {
                self.capture = Some(*bind);
                None
            }
            _ => None,
        }
    }

    fn switch(&mut self, tab: Tab) {
        *self = Prefs { tab, ..Prefs::default() };
    }

    pub fn wheel(&mut self, settings: &Settings, keys: &Keymap, view: (f32, f32), s: f32, pixels: f32) {
        let g = Geo::new(view, s);
        let rows = self.rows(settings, keys);
        self.scroll = (self.scroll + pixels).clamp(0.0, self.max_scroll(&rows, &g));
    }

    pub fn key(&mut self, key: Key, settings: &Settings, keys: &Keymap, view: (f32, f32), s: f32) -> Handled {
        let g = Geo::new(view, s);
        let rows = self.rows(settings, keys);
        let choosable: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].selectable()).collect();
        let at = self.selected.and_then(|sel| choosable.iter().position(|&i| i == sel));
        let step_tab = |d: isize| {
            let i = Tab::ALL.iter().position(|&x| x == self.tab).unwrap_or(0) as isize;
            Tab::ALL[(i + d).rem_euclid(Tab::ALL.len() as isize) as usize]
        };
        let cmd = match key {
            Key::Down | Key::Up if !choosable.is_empty() => {
                let n = choosable.len() as isize;
                let d = if key == Key::Down { 1 } else { -1 };
                let next = at.map_or(if d > 0 { 0 } else { n - 1 }, |a| (a as isize + d).clamp(0, n - 1));
                self.selected = Some(choosable[next as usize]);
                None
            }
            Key::PageDown => {
                self.switch(step_tab(1));
                None
            }
            Key::PageUp => {
                self.switch(step_tab(-1));
                None
            }
            Key::Left | Key::Right => {
                let more = key == Key::Right;
                match self.selected.map(|i| &rows[i]) {
                    Some(Row::Slider { which, .. }) => {
                        let (_, _, step, _) = which.range();
                        let v = which.value(settings) + if more { step } else { -step };
                        Some(Command::Pref(which.pref(v)))
                    }
                    Some(Row::Stepper { less, more: plus, .. }) => {
                        Some(Command::Pref(if more { plus.clone() } else { less.clone() }))
                    }
                    Some(Row::Toggle { on, pref, .. }) if *on != more => Some(Command::Pref(pref.clone())),
                    _ => None,
                }
            }
            Key::Enter => {
                let row = self.selected.map(|i| rows[i].clone());
                row.and_then(|r| self.choose(&r))
            }
            Key::Escape => Some(Command::Act(Action::ToggleSettings)),
            _ => None,
        };
        self.reveal_selected(&rows, &g);
        Handled::Yes(cmd)
    }

    /// La riga scelta con la tastiera si porta tutta a schermo.
    fn reveal_selected(&mut self, rows: &[Row], g: &Geo) {
        let Some(i) = self.selected else { return };
        let top = self.tops(rows, g)[i];
        let h = rows[i].height() * g.s;
        let (lo, hi) = (HEADER * g.s, g.view.1 - 16.0 * g.s);
        let delta = if top < lo {
            top - lo
        } else if top + h > hi {
            top + h - hi
        } else {
            0.0
        };
        self.scroll = (self.scroll + delta).clamp(0.0, self.max_scroll(rows, g));
    }
}
