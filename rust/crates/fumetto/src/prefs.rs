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

    fn rows(&self, settings: &Settings, keys: &Keymap) -> Vec<Row> {
        match self.tab {
            Tab::Image => vec![
                Row::Slider { label: t("Luminosità", "Brightness"), which: Slider::Brightness },
                Row::Slider { label: t("Contrasto", "Contrast"), which: Slider::Contrast },
                Row::Slider { label: "Gamma", which: Slider::Gamma },
                Row::Button { label: t("Ripristina", "Reset"), pref: Pref::ResetImage },
                Row::Note(t(
                    "Valgono per tutte le pagine e si vedono subito. Si applicano dopo il \
                             rimpicciolimento in luce lineare, che resta com'è.",
                    "They apply to every page, live, after the linear-light resampling, which stays as it is.",
                )),
            ],
            Tab::Reading => {
                let secs = settings.slideshow.max(1);
                vec![
                    Row::Toggle {
                        label: t("Riconosci i webtoon", "Detect webtoons"),
                        on: settings.webtoon,
                        pref: Pref::Webtoon(!settings.webtoon),
                    },
                    Row::Note(t(
                        "Un volume mai aperto con le pagine a strisce si apre a nastro.",
                        "A never-opened volume made of strips opens as a strip.",
                    )),
                    Row::Toggle {
                        label: t("Rifila i margini", "Trim margins"),
                        on: settings.trim,
                        pref: Pref::Trim(!settings.trim),
                    },
                    Row::Toggle {
                        label: t("Migliora le scansioni (AI)", "Enhance scans (AI)"),
                        on: settings.upscale,
                        pref: Pref::Upscale(!settings.upscale),
                    },
                    Row::Note(t(
                        "Real-ESRGAN rifa le pagine mostrate più grandi dei loro pixel. Si scarica \
                                 la prima volta (44 MB).",
                        "Real-ESRGAN redoes pages shown larger than their pixels. Downloaded on first use (44 MB).",
                    )),
                    Row::Toggle {
                        label: t("Barra in alto", "Top bar"),
                        on: settings.hud,
                        pref: Pref::Hud(!settings.hud),
                    },
                    Row::Stepper {
                        label: t("Presentazione, una pagina ogni", "Slideshow, a page every"),
                        value: format!("{secs} s"),
                        less: Pref::Slideshow(secs.saturating_sub(1).max(1)),
                        more: Pref::Slideshow((secs + 1).min(120)),
                    },
                ]
            }
            Tab::Lens => vec![
                Row::Slider { label: t("Ingrandimento", "Magnification"), which: Slider::LensZoom },
                Row::Slider { label: t("Grandezza", "Size"), which: Slider::LensSize },
                Row::Note(t(
                    "La lente mostra i pixel veri della pagina. Con la lente accesa la rotella \
                             cambia l'ingrandimento.",
                    "The magnifier shows the page's real pixels. While it is on, the wheel changes the magnification.",
                )),
            ],
            Tab::Keys => {
                let mut rows = vec![Row::Note(t(
                    "Fai clic su un comando e premi il tasto nuovo: Esc lascia \
                                                 com'era, Backspace toglie il tasto.",
                    "Click a command and press the new key: Esc keeps it, Backspace removes it.",
                ))];
                let mut group = "";
                for b in Bind::ALL {
                    if b.group() != group {
                        group = b.group();
                        rows.push(Row::Head(group));
                    }
                    rows.push(Row::Key { bind: b, keys: keys.keys(b).iter().map(Combo::shown).collect() });
                }
                rows.push(Row::Button {
                    label: t("Tutti i tasti di serie", "All default keys"),
                    pref: Pref::ResetKeys,
                });
                rows
            }
        }
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

    pub fn layers(
        &self, settings: &Settings, keys: &Keymap, view: (f32, f32), s: f32, m: &mut dyn Measure,
    ) -> Vec<Layer> {
        let g = Geo::new(view, s);
        let rows = self.rows(settings, keys);
        let tops = self.tops(&rows, &g);
        let (x0, w, pad) = (g.x, g.w, g.pad);
        let right = x0 + w - pad;
        let mut back = Layer::default();
        // l'ombra verso le pagine, un filo, il fondo
        back.rects.push(Rect::new(x0 - 30.0 * s, 0.0, 60.0 * s, view.1, [0.0, 0.0, 0.0, 0.6]).blur(24.0 * s));
        back.rects.push(Rect::new(x0, 0.0, w, view.1, alpha(rgb(0x11, 0x10, 0x0E), 0.985)));
        back.rects.push(Rect::new(x0, 0.0, s.round(), view.1, alpha(HAIR, 0.9)));

        let mut l = Layer::default();
        for (i, (row, &top)) in rows.iter().zip(&tops).enumerate() {
            let h = row.height() * s;
            if top + h < HEADER * s || top > view.1 {
                continue;
            }
            let hovered = self.hover == Some(Hit::Row(i));
            if (self.selected == Some(i) || hovered) && row.selectable() {
                l.rects.push(
                    Rect::new(x0 + 14.0 * s, top + 2.0 * s, w - 28.0 * s, h - 4.0 * s, [1.0, 1.0, 1.0, 0.055])
                        .radius(8.0 * s),
                );
            }
            let label = |text: &str, base: f32| {
                Text::new(text, Face::Sans, 14.0 * s, INK).weight(450).on_baseline(x0 + pad, top + base * s)
            };
            let value = |text: String, base: f32, color: [u8; 4]| {
                Text::new(text, Face::Sans, 13.0 * s, color)
                    .tabular()
                    .boxed(140.0 * s, Align::Right)
                    .on_baseline(right - 140.0 * s, top + base * s)
            };
            match row {
                Row::Head(text) => l.texts.push(caps(text, 10.5 * s, MUTED).on_baseline(x0 + pad, top + 32.0 * s)),
                Row::Note(text) => {
                    let note = Text::new(*text, Face::Sans, 12.0 * s, MUTED).boxed(w - 2.0 * pad, Align::Left);
                    l.texts.push(note.on_baseline(x0 + pad, top + 20.0 * s));
                }
                Row::Slider { label: name, which } => {
                    let v = which.value(settings);
                    l.texts.push(label(name, 26.0));
                    let changed =
                        v != which.range().3 && matches!(which, Slider::Brightness | Slider::Contrast | Slider::Gamma);
                    l.texts.push(value(which.shown(v), 26.0, if changed { INK } else { MUTED }));
                    let (tx, tw, ty) = g.track(top);
                    let (lo, hi, _, start) = which.range();
                    let at = |v: f32| (tx + (v - lo) / (hi - lo) * tw).round();
                    l.rects.push(Rect::new(tx, ty, tw, s.round().max(1.0), TICK));
                    // il tratto fra il punto di partenza (lo zero) e il valore
                    let (a, b) = (at(start).min(at(v)), at(start).max(at(v)));
                    l.rects.push(Rect::new(a, ty - s.round(), (b - a).max(1.0), (3.0 * s).round(), ACCENT));
                    if start > lo {
                        // lo zero delle regolazioni che vanno nei due sensi: una tacca
                        l.rects.push(Rect::new(at(start), ty - 5.0 * s, s.round(), 11.0 * s, TICK));
                    }
                    let d = (14.0 * s).round();
                    let knob = if self.drag == Some(*which) || hovered { INK_F } else { rgb(0xD6, 0xD0, 0xC5) };
                    l.rects.push(Rect::new(at(v) - d / 2.0, ty - d / 2.0, d, d, knob).radius(d / 2.0));
                }
                Row::Toggle { label: name, on, .. } => {
                    l.texts.push(label(name, 28.0));
                    let (sw, sh) = ((34.0 * s).round(), (18.0 * s).round());
                    let (sx, sy) = (right - sw, (top + (h - sh) / 2.0).round());
                    l.rects.push(
                        Rect::new(sx, sy, sw, sh, if *on { ACCENT } else { rgb(0x3A, 0x37, 0x32) }).radius(sh / 2.0),
                    );
                    let d = sh - 4.0 * s;
                    let kx = if *on { sx + sw - 2.0 * s - d } else { sx + 2.0 * s };
                    l.rects.push(Rect::new(kx, sy + 2.0 * s, d, d, INK_F).radius(d / 2.0));
                }
                Row::Stepper { label: name, value: v, .. } => {
                    let room = w - 2.0 * pad - 130.0 * s;
                    l.texts.push(
                        fit(m, Text::new(*name, Face::Sans, 14.0 * s, INK).weight(450), room)
                            .on_baseline(x0 + pad, top + 28.0 * s),
                    );
                    for (hit, icon, cx) in
                        [(Hit::Less(i), Icon::Minus, right - 93.0 * s), (Hit::More(i), Icon::Plus, right - 14.0 * s)]
                    {
                        let color = if self.hover == Some(hit) { INK_F } else { MUTED_F };
                        draw_icon(&mut l, icon, cx.round(), (top + h / 2.0).round(), s * 0.8, color);
                    }
                    l.texts.push(
                        Text::new(v.as_str(), Face::Sans, 13.0 * s, INK)
                            .tabular()
                            .boxed(52.0 * s, Align::Center)
                            .on_baseline(right - 80.0 * s, top + 28.0 * s),
                    );
                }
                Row::Button { label: name, .. } => {
                    l.texts.push(
                        caps(
                            name,
                            11.0 * s,
                            if hovered || self.selected == Some(i) { INK } else { crate::ui::ACCENT_TEXT },
                        )
                        .on_baseline(x0 + pad, top + 28.0 * s),
                    );
                }
                Row::Key { bind, keys } => {
                    let name = Text::new(bind.label(), Face::Sans, 13.5 * s, INK).weight(450);
                    l.texts.push(fit(m, name, w - 2.0 * pad - 150.0 * s).on_baseline(x0 + pad, top + 24.0 * s));
                    if self.capture == Some(*bind) {
                        l.texts.push(value(
                            t("premi un tasto\u{2026}", "press a key\u{2026}").to_owned(),
                            24.0,
                            crate::ui::ACCENT_TEXT,
                        ));
                    } else {
                        // i tasti come piccole targhe, da destra
                        let mut kx = right;
                        for k in keys.iter().take(2).rev() {
                            let text = Text::new(k.as_str(), Face::Sans, 12.0 * s, INK).weight(500);
                            let kw = (m.width(&text) + 16.0 * s).round();
                            kx -= kw;
                            let ky = (top + 8.0 * s).round();
                            l.rects
                                .push(Rect::new(kx, ky, kw, (22.0 * s).round(), [1.0, 1.0, 1.0, 0.07]).radius(5.0 * s));
                            l.texts.push(text.boxed(kw, Align::Center).on_baseline(kx, ky + 15.5 * s));
                            kx -= 6.0 * s;
                        }
                        if keys.is_empty() {
                            l.texts.push(value("\u{2014}".to_owned(), 24.0, MUTED));
                        }
                    }
                }
            }
        }

        // in alto, sopra le righe che scorrono: il titolo e le sezioni
        let mut head = Layer::default();
        head.rects.push(Rect::new(x0 + s, 0.0, w - s, HEADER * s, alpha(rgb(0x11, 0x10, 0x0E), 1.0)));
        head.rects.push(Rect::new(x0 + pad, (HEADER * s - 10.0 * s).round(), w - 2.0 * pad, s.round(), HAIR));
        head.texts
            .push(Text::new(t("Impostazioni", "Settings"), Face::Serif, 30.0 * s, INK).on_baseline(x0 + pad, 54.0 * s));
        let close = self.hover == Some(Hit::Close);
        draw_icon(
            &mut head,
            Icon::Close,
            (right - 12.0 * s).round(),
            (44.0 * s).round(),
            s,
            if close { INK_F } else { MUTED_F },
        );
        for (i, tab) in Tab::ALL.iter().enumerate() {
            let (tx, tw) = g.tab_x(i);
            let on = *tab == self.tab;
            let color = if on || self.hover == Some(Hit::Tab(*tab)) { INK } else { MUTED };
            head.texts.push(caps(tab.label(), 10.5 * s, color).boxed(tw, Align::Center).on_baseline(tx, 96.0 * s));
            if on {
                let d = 4.0 * s;
                head.rects.push(Rect::new(tx + tw / 2.0 - d / 2.0, 104.0 * s, d, d, ACCENT).radius(d));
            }
        }
        vec![back, l, head]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: (f32, f32) = (1920.0, 1080.0);

    fn open(tab: Tab) -> (Prefs, Settings, Keymap) {
        (Prefs::open(tab), Settings::default(), Keymap::default())
    }

    #[test]
    fn trascinare_la_luminosita() {
        let (mut p, st, km) = open(Tab::Image);
        let g = Geo::new(VIEW, 1.0);
        let rows = p.rows(&st, &km);
        let top = p.tops(&rows, &g)[0];
        let (tx, tw, ty) = g.track(top);
        // tre quarti della pista: +50
        assert_eq!(p.press(&st, &km, VIEW, 1.0, tx + tw * 0.75, ty), Some(Command::Pref(Pref::Brightness(50))));
        assert_eq!(
            p.drag(&st, &km, VIEW, 1.0, tx - 100.0),
            Some(Command::Pref(Pref::Brightness(-100))),
            "oltre il bordo: il minimo"
        );
        assert_eq!(p.click(&st, &km, VIEW, 1.0, tx, ty), Handled::Yes(None), "rilasciando non cambia altro");
    }

    #[test]
    fn con_la_tastiera() {
        let (mut p, st, km) = open(Tab::Image);
        p.key(Key::Down, &st, &km, VIEW, 1.0); // luminosita'
        assert_eq!(p.key(Key::Right, &st, &km, VIEW, 1.0), Handled::Yes(Some(Command::Pref(Pref::Brightness(5)))));
        p.key(Key::PageDown, &st, &km, VIEW, 1.0); // Lettura
        p.key(Key::Down, &st, &km, VIEW, 1.0); // webtoon, acceso di serie
        assert_eq!(p.key(Key::Enter, &st, &km, VIEW, 1.0), Handled::Yes(Some(Command::Pref(Pref::Webtoon(false)))));
        assert_eq!(p.key(Key::Escape, &st, &km, VIEW, 1.0), Handled::Yes(Some(Command::Act(Action::ToggleSettings))));
    }

    #[test]
    fn un_tasto_nuovo_per_un_comando() {
        let (mut p, st, km) = open(Tab::Keys);
        let g = Geo::new(VIEW, 1.0);
        let rows = p.rows(&st, &km);
        let i = rows.iter().position(|r| matches!(r, Row::Key { bind: Bind::Lens, .. })).unwrap();
        let top = p.tops(&rows, &g)[i];
        p.click(&st, &km, VIEW, 1.0, g.x + 60.0, top + 10.0);
        assert_eq!(p.capturing(), Some(Bind::Lens));
        let k = Combo::parse("Z").unwrap();
        assert_eq!(p.captured(Some(k.clone())), Some(Command::Pref(Pref::Bind(Bind::Lens, k))));
        assert_eq!(p.capturing(), None);
    }

    #[test]
    fn la_presentazione_a_passi() {
        let (mut p, st, km) = open(Tab::Reading);
        let g = Geo::new(VIEW, 1.0);
        let rows = p.rows(&st, &km);
        let i = rows.iter().position(|r| matches!(r, Row::Stepper { .. })).unwrap();
        let top = p.tops(&rows, &g)[i];
        let right = g.x + g.w - g.pad;
        assert_eq!(
            p.click(&st, &km, VIEW, 1.0, right - 10.0, top + 20.0),
            Handled::Yes(Some(Command::Pref(Pref::Slideshow(6))))
        );
        assert_eq!(
            p.click(&st, &km, VIEW, 1.0, right - 95.0, top + 20.0),
            Handled::Yes(Some(Command::Pref(Pref::Slideshow(4))))
        );
    }

    #[test]
    fn i_tasti_scorrono() {
        let (mut p, st, km) = open(Tab::Keys);
        p.wheel(&st, &km, VIEW, 1.0, 99_999.0);
        let g = Geo::new(VIEW, 1.0);
        let rows = p.rows(&st, &km);
        let last = *p.tops(&rows, &g).last().unwrap();
        assert!(last < VIEW.1 && last > VIEW.1 - 200.0, "in fondo si vede l'ultima riga: {last}");
    }
}
