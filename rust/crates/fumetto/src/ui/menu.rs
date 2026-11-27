//! I menu: quello del tasto destro, quello dello zoom, e i menu della
//! libreria che arrivano da `shelf`; come sono fatti, dove stanno, come si
//! disegnano.

use std::ops::Range;

use super::*;

pub(super) const MENU_W: f32 = 316.0;
const ZOOM_MENU_W: f32 = 260.0;

/// Un menu aperto: dove, le sue righe, quale e' scelta.
pub(super) struct Menu {
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) selected: Option<usize>,
    pub(super) rows: Vec<Row>,
    pub(super) width: f32,
}

impl Menu {
    /// Il menu del tasto destro, con l'angolo in (`x`, `y`).
    pub(super) fn at(x: f32, y: f32, rows: Vec<Row>, ctx: &Context) -> Menu {
        Menu { x, y, selected: None, rows, width: MENU_W * ctx.scale }
    }

    /// Un menu che scende da un tasto della barra in `x`, largo `width`,
    /// allineato al bordo destro del tasto.
    pub(super) fn below(x: f32, bar: f32, width: f32, rows: Vec<Row>, ctx: &Context) -> Menu {
        let s = ctx.scale;
        let width = width * s;
        Menu { x: x - width + BTN * s, y: bar + 6.0 * s, selected: None, rows, width }
    }

    /// I livelli di zoom, centrati sotto la percentuale (che va da `x` a
    /// `x + w` nella barra).
    pub(super) fn zoom(x: f32, w: f32, ctx: &Context) -> Menu {
        let s = ctx.scale;
        let x = (x + w / 2.0 - ZOOM_MENU_W * s / 2.0).round();
        Menu { x, y: hud_height(s) + 6.0 * s, selected: None, rows: zoom_rows(ctx), width: ZOOM_MENU_W * s }
    }

    /// Il riquadro del menu e l'angolo (sinistra, cima) di ogni riga, in
    /// pixel; `None` per un separatore che resta in cima a una colonna, e non
    /// si disegna. Le righe vanno in una colonna; se la finestra e' troppo
    /// bassa, in colonne affiancate, divise fra una sezione e l'altra.
    pub(super) fn frame(&self, ctx: &Context) -> (Rect, Vec<Option<(f32, f32)>>) {
        let s = ctx.scale;
        let (vw, vh) = ctx.view;
        let columns = columns(&self.rows, (vh / s - 32.0).max(120.0));
        let tallest = columns.iter().map(|c| c.height).fold(0.0, f32::max);
        let w = self.width * columns.len() as f32;
        let h = (tallest + 16.0) * s;
        let x = if self.x + w > vw - 8.0 * s { (self.x - w).max(8.0 * s) } else { self.x };
        let y = if self.y + h > vh - 8.0 * s { (vh - 8.0 * s - h).max(8.0 * s) } else { self.y };
        let mut spots = vec![None; self.rows.len()];
        for (n, column) in columns.iter().enumerate() {
            let left = (x + n as f32 * self.width).round();
            let mut at = y + 8.0 * s;
            for i in column.rows.clone() {
                let row = &self.rows[i];
                if i == column.rows.start && matches!(row, Row::Sep) {
                    continue;
                }
                spots[i] = Some((left, at));
                at += row.height() * s;
            }
        }
        (Rect::new(x.round(), y.round(), w.round(), h.round(), BLACK), spots)
    }

    /// La riga in (`x`, `y`), se e' sul menu.
    pub(super) fn hit(&self, ctx: &Context, x: f32, y: f32) -> Option<usize> {
        let (frame, spots) = self.frame(ctx);
        if x < frame.x || x > frame.x + frame.w || y < frame.y || y > frame.y + frame.h {
            return None;
        }
        spots.iter().zip(&self.rows).position(|(spot, r)| {
            spot.is_some_and(|(left, top)| {
                x >= left && x < left + self.width && y >= top && y < top + r.height() * ctx.scale
            })
        })
    }

    /// Le righe che si possono scegliere.
    pub(super) fn choosable(&self) -> impl Iterator<Item = usize> + '_ {
        self.rows.iter().enumerate().filter(|(_, r)| r.command().is_some()).map(|(i, _)| i)
    }

    /// Il comando della riga scelta.
    pub(super) fn chosen(&self) -> Option<Command> {
        self.selected.and_then(|i| self.rows[i].command().cloned())
    }

    /// Con le frecce: la riga da scegliere dopo (`down`) o prima, in giro.
    pub(super) fn step(&mut self, down: bool) {
        let choosable: Vec<usize> = self.choosable().collect();
        let n = choosable.len();
        if n == 0 {
            return;
        }
        let at = self.selected.and_then(|s| choosable.iter().position(|&i| i == s));
        let next = match (at, down) {
            (None, true) => 0,
            (None, false) => n - 1,
            (Some(a), true) => (a + 1) % n,
            (Some(a), false) => (a + n - 1) % n,
        };
        self.selected = Some(choosable[next]);
    }

    pub(super) fn layer(&self, ctx: &Context, m: &mut dyn Measure) -> Layer {
        let s = ctx.scale;
        let (f, spots) = self.frame(ctx);
        let mut l = Layer::default();
        // ombra morbida (si vede sulle pagine chiare), filo, fondo
        l.rects.push(Rect::new(f.x, f.y + 10.0 * s, f.w, f.h, [0.0, 0.0, 0.0, 0.55]).radius(12.0 * s).blur(26.0 * s));
        l.rects.push(Rect::new(f.x, f.y, f.w, f.h, [1.0, 1.0, 1.0, 0.09]).radius(12.0 * s));
        l.rects.push(
            Rect::new(f.x + s, f.y + s, f.w - 2.0 * s, f.h - 2.0 * s, alpha(rgb(0x11, 0x10, 0x0E), 0.98))
                .radius(11.0 * s),
        );
        for (i, (row, spot)) in self.rows.iter().zip(&spots).enumerate() {
            let Some((x, top)) = *spot else { continue };
            let selected = self.selected == Some(i);
            if selected {
                let (cw, h) = (self.width, row.height() * s);
                l.rects.push(
                    Rect::new(x + 6.0 * s, top + 2.0 * s, cw - 12.0 * s, h - 4.0 * s, [1.0, 1.0, 1.0, 0.065])
                        .radius(7.0 * s),
                );
            }
            self.draw_row(&mut l, row, (x, top), selected, s, m);
        }
        // fra le colonne un filo, come la piega fra due pagine
        let mut fold = f.x + self.width;
        while fold < f.x + f.w - 1.0 {
            l.rects.push(Rect::new(fold.round(), f.y + 18.0 * s, s.round(), f.h - 36.0 * s, HAIR));
            fold += self.width;
        }
        l
    }

    /// Una riga del menu, con l'angolo in `(x, top)`.
    fn draw_row(&self, l: &mut Layer, row: &Row, (x, top): (f32, f32), selected: bool, s: f32, m: &mut dyn Measure) {
        let cw = self.width;
        let h = row.height() * s;
        let left = x + 34.0 * s;
        // a destra: il tasto, il luogo, "togli"
        let right_box =
            |text: Text| text.boxed(110.0 * s, Align::Right).on_baseline(x + cw - 18.0 * s - 110.0 * s, top + 22.0 * s);
        match row {
            Row::Item { label, key, on, .. } => {
                if *on {
                    let d = 6.0 * s;
                    l.rects.push(Rect::new(x + 17.0 * s - d / 2.0, top + h / 2.0 - d / 2.0, d, d, ACCENT).radius(d));
                }
                l.texts
                    .push(Text::new(*label, Face::Sans, 13.5 * s, INK).weight(450).on_baseline(left, top + 22.0 * s));
                l.texts.push(right_box(Text::new(key.as_str(), Face::Sans, 12.0 * s, MUTED).tabular()));
            }
            Row::Recent { title, place, .. } => {
                let title = Text::new(title.as_str(), Face::Serif, 17.0 * s, INK);
                l.texts.push(fit(m, title, cw - 34.0 * s - 100.0 * s).on_baseline(left, top + 23.0 * s));
                l.texts.push(right_box(Text::new(place.as_str(), Face::Sans, 12.0 * s, MUTED).tabular()));
            }
            Row::Folder { label, .. } => {
                let shown = Text::new(label.as_str(), Face::Sans, 13.0 * s, INK);
                l.texts.push(fit(m, shown, cw - 34.0 * s - 70.0 * s).on_baseline(left, top + 22.0 * s));
                let hint = if selected { t("togli", "remove") } else { "" };
                l.texts.push(right_box(Text::new(hint, Face::Sans, 12.0 * s, ACCENT_TEXT)));
            }
            Row::Head(text) => l.texts.push(caps(text, 10.0 * s, MUTED).on_baseline(left, top + 21.0 * s)),
            Row::Sep => l.rects.push(Rect::new(x + 18.0 * s, (top + 6.0 * s).round(), cw - 36.0 * s, s.round(), HAIR)),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Row {
    Item {
        label: &'static str,
        key: String,
        on: bool,
        cmd: Command,
    },
    Recent {
        title: String,
        place: String,
        cmd: Command,
    },
    /// Una cartella o un server della libreria: come si mostra, e un clic
    /// lo toglie.
    Folder {
        label: String,
        cmd: Command,
    },
    Head(&'static str),
    Sep,
}

impl Row {
    /// Una voce senza tasto ne' segno.
    pub(crate) fn plain(label: &'static str, cmd: Command) -> Row {
        Row::Item { label, key: String::new(), on: false, cmd }
    }

    /// Una voce senza tasto, con il segno se `on`.
    pub(crate) fn check(label: &'static str, on: bool, cmd: Command) -> Row {
        Row::Item { label, key: String::new(), on, cmd }
    }

    fn height(&self) -> f32 {
        match self {
            Row::Item { .. } | Row::Recent { .. } | Row::Folder { .. } => 34.0,
            Row::Head(_) => 30.0,
            Row::Sep => 13.0,
        }
    }

    pub(super) fn command(&self) -> Option<&Command> {
        match self {
            Row::Item { cmd, .. } | Row::Recent { cmd, .. } | Row::Folder { cmd, .. } => Some(cmd),
            _ => None,
        }
    }
}

/// Una voce di menu con il suo tasto, quello che vale adesso.
fn item(ctx: &Context, label: &'static str, bind: Bind, on: bool, action: Action) -> Row {
    Row::Item { label, key: ctx.keys.label(bind), on, cmd: Command::Act(action) }
}

/// Una colonna del menu: quali righe, e quanto e' alta in punti (senza il
/// separatore con cui puo' cominciare).
struct Column {
    rows: Range<usize>,
    height: f32,
}

/// Le righe in colonne alte al piu' `room` punti, andando a capo solo fra
/// una sezione e l'altra (a un separatore, o a un titolo che non lo segue):
/// una colonna sola, se ci stanno.
fn columns(rows: &[Row], room: f32) -> Vec<Column> {
    let starts = (0..rows.len()).filter(|&i| {
        i == 0 || matches!(rows[i], Row::Sep) || matches!(rows[i], Row::Head(_)) && !matches!(rows[i - 1], Row::Sep)
    });
    let starts: Vec<usize> = starts.chain([rows.len()]).collect();
    let mut out: Vec<Column> = Vec::new();
    for w in starts.windows(2) {
        let section = w[0]..w[1];
        let all: f32 = rows[section.clone()].iter().map(Row::height).sum();
        match out.last_mut() {
            Some(c) if c.height + all <= room => {
                c.rows.end = section.end;
                c.height += all;
            }
            _ => {
                let lead = if matches!(rows[section.start], Row::Sep) { Row::Sep.height() } else { 0.0 };
                out.push(Column { rows: section, height: all - lead });
            }
        }
    }
    if out.is_empty() {
        out.push(Column { rows: 0..0, height: 0.0 });
    }
    out
}

/// Il menu del tasto destro fuori dalle copertine: nel lettore le cose da
/// fare con la pagina e con il volume (aprire, la libreria, lo schermo
/// intero stanno nella barra); altrimenti aprire e i recenti.
pub(super) fn main_rows(ctx: &Context) -> Vec<Row> {
    match &ctx.book {
        Some(b) if ctx.shelf.is_none() => reader_rows(ctx, b),
        _ => general_rows(ctx),
    }
}

/// Senza un volume davanti: aprire, la libreria, i recenti. Le cartelle e i
/// server della libreria si aggiungono dal suo tasto delle cartelle.
fn general_rows(ctx: &Context) -> Vec<Row> {
    let i = |label, bind, on, action| item(ctx, label, bind, on, action);
    let mut rows = vec![
        i(t("Apri\u{2026}", "Open\u{2026}"), Bind::Open, false, Action::Open),
        i(t("Apri cartella\u{2026}", "Open folder\u{2026}"), Bind::OpenFolder, false, Action::OpenFolder),
        i(t("Libreria", "Library"), Bind::Library, ctx.shelf.is_some(), Action::ToggleLibrary),
    ];
    if !ctx.recent.is_empty() {
        rows.push(Row::Sep);
        rows.push(Row::Head(t("Recenti", "Recent")));
        rows.extend(ctx.recent.iter().take(RECENT).map(|r| Row::Recent {
            title: r.title.clone(),
            place: r.place.clone(),
            cmd: Command::Open(r.path.clone()),
        }));
    }
    rows.push(Row::Sep);
    rows.push(i(t("Impostazioni\u{2026}", "Settings\u{2026}"), Bind::Settings, false, Action::ToggleSettings));
    rows
}

/// Quanti volumi recenti, e quanti segnalibri (i piu' vicini alla pagina).
const RECENT: usize = 5;
const MARKS: usize = 4;

/// Nel lettore: la pagina, i segnalibri, i volumi accanto; come si legge;
/// cosa fare della pagina; e chiudere.
fn reader_rows(ctx: &Context, b: &BookInfo) -> Vec<Row> {
    let i = |label, bind, on, action| item(ctx, label, bind, on, action);
    let mut rows = vec![
        i(t("Vai a pagina\u{2026}", "Go to page\u{2026}"), Bind::GoTo, false, Action::AskPage),
        i(t("Segna la pagina", "Bookmark the page"), Bind::Bookmark, b.bookmarked, Action::ToggleBookmark),
        i(t("Tag del volume\u{2026}", "Volume tags\u{2026}"), Bind::Tags, false, Action::EditTags),
    ];
    if !b.bookmarks.is_empty() {
        rows.push(Row::Head(t("Segnalibri", "Bookmarks")));
        let mut near: Vec<usize> = b.bookmarks.to_vec();
        near.sort_by_key(|p| p.abs_diff(b.here));
        near.truncate(MARKS);
        near.sort_unstable();
        for p in near {
            let title = if italian() { format!("Pagina {}", p + 1) } else { format!("Page {}", p + 1) };
            let place = if p == b.here { t("qui", "here").to_owned() } else { String::new() };
            rows.push(Row::Recent { title, place, cmd: Command::Act(Action::GoTo(p)) });
        }
    }
    let neighbours = [(b.prev, t("prima", "before")), (b.next, t("dopo", "after"))];
    if neighbours.iter().any(|(v, _)| v.is_some()) {
        rows.push(Row::Head(t("Nella serie", "In the series")));
        for (volume, place) in neighbours {
            let Some((path, title)) = volume else { continue };
            let (title, place) = (title.to_owned(), place.to_owned());
            rows.push(Row::Recent { title, place, cmd: Command::Open(path.to_owned()) });
        }
    }
    rows.extend([Row::Sep, i(t("Doppia pagina", "Two pages"), Bind::Double, b.double, Action::ToggleDouble)]);
    if b.double {
        rows.push(i(t("Copertina da sola", "Cover alone"), Bind::Cover, b.cover_alone, Action::ToggleCover));
    }
    rows.extend([
        i(t("Nastro", "Strip"), Bind::Strip, b.strip, Action::ToggleStrip),
        i(t("Da destra a sinistra", "Right to left"), Bind::Manga, b.manga, Action::ToggleManga),
        Row::Sep,
        i(t("Ruota", "Rotate"), Bind::RotateRight, false, Action::Rotate(true)),
        i(t("Rifila i margini", "Trim margins"), Bind::Trim, b.trim, Action::ToggleTrim),
        i(t("Lente", "Magnifier"), Bind::Lens, ctx.lens.is_some(), Action::ToggleLens),
        i(t("Presentazione", "Slideshow"), Bind::Slideshow, b.slideshow, Action::ToggleSlideshow),
        Row::Sep,
        i(t("Salva la pagina\u{2026}", "Save the page\u{2026}"), Bind::Save, false, Action::SavePage),
        i(t("Copia la pagina", "Copy the page"), Bind::Copy, false, Action::CopyPage),
        Row::Sep,
        i(t("Impostazioni\u{2026}", "Settings\u{2026}"), Bind::Settings, false, Action::ToggleSettings),
        i(t("Chiudi il volume", "Close the volume"), Bind::Close, false, Action::Close),
    ]);
    rows
}

/// Le voci del menu dei livelli di zoom.
pub(super) fn zoom_rows(ctx: &Context) -> Vec<Row> {
    let i = |label, bind, action| item(ctx, label, bind, false, action);
    vec![
        Row::Head(t("Zoom", "Zoom")),
        i(t("Pagina intera", "Whole page"), Bind::ZoomPage, Action::ZoomTo(Zoom::Page)),
        i(t("Larga quanto la finestra", "Fit width"), Bind::ZoomWidth, Action::ZoomTo(Zoom::Width)),
        i(t("100%, pixel reali", "100%, actual pixels"), Bind::ZoomActual, Action::ZoomTo(Zoom::Actual)),
        Row::Sep,
        i(t("Ingrandisci", "Zoom in"), Bind::ZoomIn, Action::ZoomIn),
        i(t("Riduci", "Zoom out"), Bind::ZoomOut, Action::ZoomOut),
        Row::Sep,
        i(t("Lente", "Magnifier"), Bind::Lens, Action::ToggleLens),
    ]
}
