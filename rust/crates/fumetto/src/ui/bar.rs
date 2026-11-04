//! La barra in alto: i tasti, le loro icone (tratti disegnati qui, alla
//! scala dello schermo) e i suggerimenti.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Icon {
    Search,
    Close,
    Open,
    Gallery,
    First,
    Prev,
    Next,
    Last,
    Single,
    Double,
    Strip,
    LeftToRight,
    RightToLeft,
    Minus,
    Plus,
    Full,
    FullExit,
    Hide,
    Thumbs,
    Bookmark,
    Shelf,
    /// Le cartelle una dentro l'altra: la vista per cartelle.
    Tree,
    /// Righe sempre piu' corte: l'ordine.
    Sort,
}

/// Cosa mostra un tasto della barra, e quindi quale tasto e'.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Look {
    Icon(Icon),
    /// La pagina: "11–12 / 82".
    Folio(String),
    /// Lo zoom: "72%".
    Zoom(String),
}

/// Un tasto della barra: dove sta, cosa mostra, cosa fa.
pub(super) struct Button {
    pub(super) face: Look,
    pub(super) x: f32,
    pub(super) w: f32,
    pub(super) cmd: Option<Command>,
    on: bool,
    tip: String,
}

impl Button {
    /// Chi e': l'icona, o quale scritta (per ricordare quale ha il mouse sopra).
    pub(super) fn id(&self) -> Hover {
        match &self.face {
            Look::Icon(i) => Hover::Icon(*i),
            Look::Folio(_) => Hover::Folio,
            Look::Zoom(_) => Hover::Zoom,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Hover {
    Icon(Icon),
    Folio,
    Zoom,
}

fn tip(what: &str, key: &str) -> String {
    format!("{what}   \u{00b7}   {key}")
}

/// I tasti della barra, da sinistra a destra.
pub(super) fn hud_buttons(ctx: &Context, b: &BookInfo) -> Vec<Button> {
    let (right, right_start) = right_buttons(ctx, b);
    let mut v = left_buttons(ctx);
    v.extend(center_buttons(ctx, b, right_start));
    v.extend(right);
    v
}

fn act(a: Action) -> Option<Command> {
    Some(Command::Act(a))
}

impl Button {
    fn new(face: Look, x: f32, w: f32, cmd: Option<Command>, on: bool, tip: String) -> Button {
        Button { face, x: x.round(), w: w.round(), cmd, on, tip }
    }

    fn icon(icon: Icon, x: f32, w: f32, cmd: Option<Command>, on: bool, tip: String) -> Button {
        Button::new(Look::Icon(icon), x, w, cmd, on, tip)
    }
}

/// A sinistra: aprire, la libreria, le miniature.
fn left_buttons(ctx: &Context) -> Vec<Button> {
    let s = ctx.scale;
    let btn = BTN * s;
    [
        (Icon::Open, Action::Open, t("Apri", "Open"), Bind::Open),
        (Icon::Gallery, Action::ToggleLibrary, t("Libreria", "Library"), Bind::Library),
        (Icon::Thumbs, Action::ToggleThumbs, t("Miniature", "Thumbnails"), Bind::Thumbs),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (icon, action, what, bind))| {
        let x = 10.0 * s + i as f32 * (btn + 2.0 * s);
        Button::icon(icon, x, btn, act(action), false, tip(what, &ctx.keys.label(bind)))
    })
    .collect()
}

/// Al centro: la pagina e come muoversi (nei manga al contrario: avanti e'
/// a sinistra). Si scosta a sinistra se a destra non c'e' posto.
fn center_buttons(ctx: &Context, b: &BookInfo, right_start: f32) -> Vec<Button> {
    let s = ctx.scale;
    let btn = BTN * s;
    let k = |b: Bind| ctx.keys.label(b);
    let folio_w = 150.0 * s;
    let center_w = 4.0 * btn + folio_w;
    let x0 = ((ctx.view.0 - center_w) / 2.0).min(right_start - 24.0 * s - center_w);
    let (back, forth, start, end) = if b.manga {
        (Action::Next, Action::Prev, Action::Last, Action::First)
    } else {
        (Action::Prev, Action::Next, Action::First, Action::Last)
    };
    let (back_key, forth_key, start_key, end_key) = if b.manga {
        (k(Bind::Right), k(Bind::Left), k(Bind::Last), k(Bind::First))
    } else {
        (k(Bind::Left), k(Bind::Right), k(Bind::First), k(Bind::Last))
    };
    let folio = Look::Folio(format!("{}  /  {}", b.folio, b.pages));
    vec![
        Button::icon(Icon::First, x0, btn, act(start), false, tip(t("Inizio", "Start"), &start_key)),
        Button::icon(Icon::Prev, x0 + btn, btn, act(back), false, tip(t("Indietro", "Back"), &back_key)),
        Button::new(
            folio,
            x0 + 2.0 * btn,
            folio_w,
            act(Action::AskPage),
            false,
            tip(t("Vai a pagina", "Go to page"), &k(Bind::GoTo)),
        ),
        Button::icon(
            Icon::Next,
            x0 + 2.0 * btn + folio_w,
            btn,
            act(forth),
            false,
            tip(t("Avanti", "Forward"), &forth_key),
        ),
        Button::icon(Icon::Last, x0 + 3.0 * btn + folio_w, btn, act(end), false, tip(t("Fine", "End"), &end_key)),
    ]
}

/// A destra, dal bordo verso il centro: nascondere la barra, lo schermo
/// intero; lo zoom; il verso e i modi; il segno. I tasti (da sinistra), e
/// dove comincia il gruppo.
fn right_buttons(ctx: &Context, b: &BookInfo) -> (Vec<Button>, f32) {
    let s = ctx.scale;
    let btn = BTN * s;
    let mut r = ctx.view.0 - 10.0 * s;
    let mut left_of = |w: f32| {
        r -= w;
        r
    };
    let hide = left_of(btn);
    let full = left_of(btn + 2.0 * s);
    left_of(17.0 * s); // fra i gruppi, un po' d'aria
    let plus = left_of(btn);
    let zoom = left_of(64.0 * s);
    let minus = left_of(btn);
    left_of(17.0 * s);
    let dir = left_of(btn);
    let strip = left_of(btn + 6.0 * s);
    let double = left_of(btn);
    let single = left_of(btn);
    left_of(17.0 * s);
    let mark = left_of(btn);

    let bookmark =
        if b.bookmarked { t("Pagina segnata", "Bookmarked") } else { t("Segna la pagina", "Bookmark the page") };
    let mut v = vec![Button::icon(
        Icon::Bookmark,
        mark,
        btn,
        act(Action::ToggleBookmark),
        b.bookmarked,
        tip(bookmark, &ctx.keys.label(Bind::Bookmark)),
    )];
    v.extend(mode_buttons(ctx, b, [single, double, strip, dir]));
    v.extend(zoom_buttons(ctx, b, [minus, zoom, plus]));
    v.extend(window_buttons(ctx, [full + 2.0 * s, hide]));
    (v, mark)
}

/// Pagina singola, doppia, nastro, e il verso di lettura.
fn mode_buttons(ctx: &Context, b: &BookInfo, [single, double, strip, dir]: [f32; 4]) -> [Button; 4] {
    let btn = BTN * ctx.scale;
    let k = |b: Bind| ctx.keys.label(b);
    // la pagina singola si raggiunge lasciando il modo in cui si e'; gia' li', torna alla pagina intera
    let single_cmd = if b.double {
        Action::ToggleDouble
    } else if b.strip {
        Action::ToggleStrip
    } else {
        Action::ZoomTo(Zoom::Page)
    };
    let single_key = k(if b.double { Bind::Double } else { Bind::Strip });
    let (dir_icon, dir_tip) = if b.manga {
        (Icon::RightToLeft, t("Da destra a sinistra", "Right to left"))
    } else {
        (Icon::LeftToRight, t("Da sinistra a destra", "Left to right"))
    };
    [
        Button::icon(
            Icon::Single,
            single,
            btn,
            act(single_cmd),
            !b.double && !b.strip,
            tip(t("Pagina singola", "Single page"), &single_key),
        ),
        Button::icon(
            Icon::Double,
            double,
            btn,
            act(Action::ToggleDouble),
            b.double,
            tip(t("Doppia pagina", "Two pages"), &k(Bind::Double)),
        ),
        Button::icon(
            Icon::Strip,
            strip,
            btn,
            act(Action::ToggleStrip),
            b.strip,
            tip(t("Nastro", "Strip"), &k(Bind::Strip)),
        ),
        Button::icon(dir_icon, dir, btn, act(Action::ToggleManga), b.manga, tip(dir_tip, &k(Bind::Manga))),
    ]
}

/// Riduci, la percentuale (apre i livelli), ingrandisci; spenti dove lo
/// zoom non si regola.
fn zoom_buttons(ctx: &Context, b: &BookInfo, [minus, zoom, plus]: [f32; 3]) -> [Button; 3] {
    let s = ctx.scale;
    let btn = BTN * s;
    let k = |b: Bind| ctx.keys.label(b);
    let when = |a: Action| b.zoom.is_some().then_some(Command::Act(a));
    let percent = Look::Zoom(b.zoom.map_or("\u{2014}".into(), |z| format!("{z}%")));
    let levels = format!("{} {} {}", k(Bind::ZoomPage), k(Bind::ZoomWidth), k(Bind::ZoomActual));
    [
        Button::icon(
            Icon::Minus,
            minus,
            btn,
            when(Action::ZoomOut),
            false,
            tip(t("Riduci", "Zoom out"), &k(Bind::ZoomOut)),
        ),
        Button::new(
            percent,
            zoom,
            64.0 * s,
            when(Action::ZoomTo(Zoom::Page)),
            false,
            tip(t("Livelli di zoom", "Zoom levels"), &levels),
        ),
        Button::icon(
            Icon::Plus,
            plus,
            btn,
            when(Action::ZoomIn),
            false,
            tip(t("Ingrandisci", "Zoom in"), &k(Bind::ZoomIn)),
        ),
    ]
}

/// Lo schermo intero e la barra da nascondere.
fn window_buttons(ctx: &Context, [full, hide]: [f32; 2]) -> [Button; 2] {
    let btn = BTN * ctx.scale;
    let k = |b: Bind| ctx.keys.label(b);
    let (full_icon, full_tip) = if ctx.fullscreen {
        (Icon::FullExit, t("Esci dallo schermo intero", "Exit full screen"))
    } else {
        (Icon::Full, t("Schermo intero", "Full screen"))
    };
    [
        Button::icon(full_icon, full, btn, act(Action::ToggleFullscreen), false, tip(full_tip, &k(Bind::Fullscreen))),
        Button::icon(
            Icon::Hide,
            hide,
            btn,
            act(Action::ToggleHud),
            false,
            tip(t("Nascondi la barra", "Hide the bar"), &k(Bind::Hud)),
        ),
    ]
}

/// Il tasto della barra sotto il punto (x, y), se c'e'.
pub(super) fn hud_hit(ctx: &Context, x: f32, y: f32) -> Option<Button> {
    let b = ctx.book.as_ref().filter(|_| ctx.bar())?;
    let h = hud_height(ctx.scale);
    if y < 0.0 || y >= h {
        return None;
    }
    hud_buttons(ctx, b).into_iter().find(|btn| x >= btn.x && x < btn.x + btn.w)
}

/// La barra in alto: nera come la galleria, un filo la separa dalle pagine.
pub(super) fn hud(ctx: &Context, b: &BookInfo, hover: Option<Hover>, m: &mut dyn Measure) -> Layer {
    let s = ctx.scale;
    let vw = ctx.view.0;
    let h = hud_height(s);
    let mut l = Layer::default();
    l.rects.push(Rect::new(0.0, 0.0, vw, h, BLACK));
    l.rects.push(Rect::new(0.0, h - s.round(), vw, s.round(), alpha(HAIR, 0.8)));
    let buttons = hud_buttons(ctx, b);
    let (by, bh) = (((h - BTN * s) / 2.0).round(), (BTN * s).round());
    let cy = by + bh / 2.0;

    // i divisori fra i gruppi di destra: dove fra due tasti resta il loro spazio
    for w in buttons.windows(2) {
        let gap = w[1].x - (w[0].x + w[0].w);
        if (12.0 * s..30.0 * s).contains(&gap) {
            let x = (w[0].x + w[0].w + gap / 2.0).round();
            l.rects.push(Rect::new(x, (cy - 9.0 * s).round(), s.round(), (18.0 * s).round(), HAIR));
        }
    }
    let center_start = buttons.iter().find(|b| b.face == Look::Icon(Icon::First)).map_or(vw, |b| b.x);

    // il titolo, fra i tasti di sinistra e quelli del centro
    let title_x = buttons[2].x + buttons[2].w + 16.0 * s;
    let room = center_start - 24.0 * s - title_x;
    if room > 60.0 * s {
        let title = Text::new(b.title, Face::Serif, 19.0 * s, INK);
        l.texts.push(fit(m, title, room).on_baseline(title_x, cy + 6.5 * s));
    }

    for btn in &buttons {
        let enabled = btn.cmd.is_some();
        let hovered = hover == Some(btn.id()) && enabled;
        if hovered {
            l.rects.push(Rect::new(btn.x, by, btn.w, bh, [1.0, 1.0, 1.0, 0.075]).radius(8.0 * s));
        }
        let color = if !enabled {
            alpha(MUTED_F, 0.35)
        } else if btn.on || hovered {
            INK_F
        } else {
            MUTED_F
        };
        let cx = btn.x + btn.w / 2.0;
        match &btn.face {
            Look::Icon(icon) => draw_icon(&mut l, *icon, cx.round(), cy.round(), s, color),
            Look::Folio(label) => l.texts.push(
                Text::new(label.as_str(), Face::Serif, 19.0 * s, INK)
                    .boxed(btn.w, Align::Center)
                    .on_baseline(btn.x, cy + 6.5 * s),
            ),
            Look::Zoom(label) => l.texts.push(
                Text::new(label.as_str(), Face::Sans, 12.5 * s, to_u8(color))
                    .weight(500)
                    .tabular()
                    .boxed(btn.w, Align::Center)
                    .on_baseline(btn.x, cy + 4.5 * s),
            ),
        }
        if btn.on {
            // la voce scelta: un punto rosso sotto
            let d = 4.0 * s;
            l.rects.push(Rect::new(cx - d / 2.0, by + bh - 1.0 * s, d, d, ACCENT).radius(d));
        }
    }

    // il nome del tasto sotto il mouse, con la sua scorciatoia
    if let Some(btn) = buttons.iter().find(|b| hover == Some(b.id()) && b.cmd.is_some()) {
        tooltip(&mut l, m, &btn.tip, btn.x + btn.w / 2.0, h, vw, s);
    }
    l
}

/// Un'etichetta sotto un tasto della barra, centrata in `cx`.
pub(crate) fn tooltip(l: &mut Layer, m: &mut dyn Measure, tip: &str, cx: f32, bar_h: f32, vw: f32, s: f32) {
    let text = Text::new(tip, Face::Sans, 12.5 * s, INK).weight(450);
    let w = (m.width(&text) + 24.0 * s).round();
    let x = (cx - w / 2.0).clamp(8.0 * s, vw - w - 8.0 * s).round();
    let y = (bar_h + 8.0 * s).round();
    let th = (30.0 * s).round();
    l.rects.push(Rect::new(x, y + 4.0 * s, w, th, [0.0, 0.0, 0.0, 0.5]).radius(8.0 * s).blur(10.0 * s));
    l.rects.push(Rect::new(x, y, w, th, [1.0, 1.0, 1.0, 0.1]).radius(8.0 * s));
    l.rects
        .push(Rect::new(x + s, y + s, w - 2.0 * s, th - 2.0 * s, alpha(rgb(0x14, 0x13, 0x11), 0.98)).radius(7.0 * s));
    l.texts.push(text.boxed(w, Align::Center).on_baseline(x, y + th / 2.0 + 4.5 * s));
}

/// Un'icona di tratti sottili, centrata in (cx, cy), in un quadrato di 16
/// punti. I tratti sono spessi 1,5 punti, come le aste delle etichette.
pub(crate) fn draw_icon(l: &mut Layer, icon: Icon, cx: f32, cy: f32, s: f32, color: [f32; 4]) {
    let w = (1.5 * s).max(1.0);
    let mut line = |pts: &[(f32, f32)]| {
        for p in pts.windows(2) {
            l.rects.push(Rect::line(cx + p[0].0 * s, cy + p[0].1 * s, cx + p[1].0 * s, cy + p[1].1 * s, w, color));
        }
    };
    let mut boxes: Vec<(f32, f32, f32, f32)> = Vec::new();
    let mut ring: Option<(f32, f32, f32)> = None;
    match icon {
        Icon::Open => {
            boxes.push((-8.0, -4.0, 16.0, 11.0));
            line(&[(-7.5, -4.0), (-7.5, -7.0), (-2.5, -7.0), (-0.5, -4.0)]);
        }
        Icon::Gallery => {
            boxes.extend([(-7.0, -7.0, 6.0, 6.0), (1.0, -7.0, 6.0, 6.0), (-7.0, 1.0, 6.0, 6.0), (1.0, 1.0, 6.0, 6.0)])
        }
        Icon::Prev => line(&[(3.0, -6.0), (-3.0, 0.0), (3.0, 6.0)]),
        Icon::Next => line(&[(-3.0, -6.0), (3.0, 0.0), (-3.0, 6.0)]),
        Icon::First => {
            line(&[(-5.0, -6.0), (-5.0, 6.0)]);
            line(&[(4.0, -6.0), (-2.0, 0.0), (4.0, 6.0)]);
        }
        Icon::Last => {
            line(&[(5.0, -6.0), (5.0, 6.0)]);
            line(&[(-4.0, -6.0), (2.0, 0.0), (-4.0, 6.0)]);
        }
        Icon::Single => boxes.push((-5.0, -7.5, 10.0, 15.0)),
        Icon::Double => boxes.extend([(-8.5, -6.5, 8.0, 13.0), (0.5, -6.5, 8.0, 13.0)]),
        Icon::Strip => boxes.extend([(-5.0, -8.5, 10.0, 7.5), (-5.0, 1.0, 10.0, 7.5)]),
        Icon::LeftToRight => {
            line(&[(-7.0, 0.0), (7.0, 0.0)]);
            line(&[(3.0, -4.0), (7.0, 0.0), (3.0, 4.0)]);
        }
        Icon::RightToLeft => {
            line(&[(7.0, 0.0), (-7.0, 0.0)]);
            line(&[(-3.0, -4.0), (-7.0, 0.0), (-3.0, 4.0)]);
        }
        Icon::Minus => line(&[(-6.0, 0.0), (6.0, 0.0)]),
        Icon::Plus => {
            line(&[(-6.0, 0.0), (6.0, 0.0)]);
            line(&[(0.0, -6.0), (0.0, 6.0)]);
        }
        Icon::Full => {
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                line(&[(7.0 * sx, 3.0 * sy), (7.0 * sx, 7.0 * sy), (3.0 * sx, 7.0 * sy)]);
            }
        }
        Icon::FullExit => {
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                line(&[(7.0 * sx, 3.0 * sy), (3.0 * sx, 3.0 * sy), (3.0 * sx, 7.0 * sy)]);
            }
        }
        Icon::Hide => line(&[(-6.0, 3.0), (0.0, -3.0), (6.0, 3.0)]),
        // tre pagine in fila, come un provino
        Icon::Thumbs => boxes.extend([(-8.5, -5.5, 4.5, 11.0), (-2.25, -5.5, 4.5, 11.0), (4.0, -5.5, 4.5, 11.0)]),
        // un nastrino con la coda a V
        Icon::Bookmark => line(&[(-4.5, -7.5), (4.5, -7.5), (4.5, 7.5), (0.0, 3.5), (-4.5, 7.5), (-4.5, -7.5)]),
        // due dorsi dritti e uno appoggiato, su una mensola
        Icon::Shelf => {
            boxes.extend([(-8.0, -5.0, 4.0, 11.0), (-3.0, -7.5, 4.0, 13.5)]);
            line(&[(2.0, 6.0), (5.5, -6.0), (8.8, -5.0), (5.6, 6.0)]);
            line(&[(-9.5, 7.75), (9.5, 7.75)]);
        }
        Icon::Sort => {
            line(&[(-8.0, -5.5), (8.0, -5.5)]);
            line(&[(-8.0, 0.0), (3.0, 0.0)]);
            line(&[(-8.0, 5.5), (-2.0, 5.5)]);
        }
        Icon::Tree => {
            boxes.extend([(-8.0, -8.5, 7.0, 5.0), (0.0, -2.5, 8.0, 5.0), (0.0, 4.0, 8.0, 5.0)]);
            line(&[(-4.5, -3.5), (-4.5, 6.5), (0.0, 6.5)]);
            line(&[(-4.5, 0.0), (0.0, 0.0)]);
        }
        Icon::Close => {
            line(&[(-5.5, -5.5), (5.5, 5.5)]);
            line(&[(5.5, -5.5), (-5.5, 5.5)]);
        }
        Icon::Search => {
            ring = Some((-7.0, -7.0, 11.0));
            line(&[(2.5, 2.5), (7.0, 7.0)]);
        }
    }
    if let Some((x, y, d)) = ring {
        l.rects.push(Rect::new(cx + x * s, cy + y * s, d * s, d * s, color).radius(d * s / 2.0).stroke(w));
    }
    for (x, y, bw, bh) in boxes {
        l.rects.push(Rect::new(cx + x * s, cy + y * s, bw * s, bh * s, color).radius(1.5 * s).stroke(w));
    }
}
