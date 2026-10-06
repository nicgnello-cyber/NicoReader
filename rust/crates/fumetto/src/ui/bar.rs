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
    let s = ctx.scale;
    let vw = ctx.view.0;
    let btn = BTN * s;
    let k = |b: Bind| ctx.keys.label(b);
    let act = |a| Some(Command::Act(a));
    let mut v = Vec::new();
    let mut push = |x: f32, w: f32, face: Look, cmd: Option<Command>, on: bool, tip: String| {
        v.push(Button { face, x: x.round(), w: w.round(), cmd, on, tip });
    };
    let icon = Look::Icon;

    // a sinistra: aprire e tornare alla galleria
    let mut x = 10.0 * s;
    push(x, btn, icon(Icon::Open), act(Action::Open), false, tip(t("Apri", "Open"), &k(Bind::Open)));
    x += btn + 2.0 * s;
    push(x, btn, icon(Icon::Gallery), act(Action::ToggleLibrary), false,
         tip(t("Libreria", "Library"), &k(Bind::Library)));
    x += btn + 2.0 * s;
    push(x, btn, icon(Icon::Thumbs), act(Action::ToggleThumbs), false, tip(t("Miniature", "Thumbnails"), &k(Bind::Thumbs)));

    // a destra, dal bordo verso il centro
    let mut r = vw - 10.0 * s;
    let mut left_of = |w: f32| {
        r -= w;
        r
    };
    let hide = left_of(btn);
    let full = left_of(btn + 2.0 * s);
    let sep1 = left_of(17.0 * s);
    let plus = left_of(btn);
    let zoom = left_of(64.0 * s);
    let minus = left_of(btn);
    let sep2 = left_of(17.0 * s);
    let dir = left_of(btn);
    let strip = left_of(btn + 6.0 * s);
    let double = left_of(btn);
    let single = left_of(btn);
    let sep3 = left_of(17.0 * s);
    let mark = left_of(btn);
    let right_start = r;
    let _ = (sep1, sep2, sep3);

    // al centro: la pagina e come muoversi; si scosta a sinistra se non c'e' posto
    let folio_w = 150.0 * s;
    let center_w = 4.0 * btn + folio_w;
    let cx0 = ((vw - center_w) / 2.0).min(right_start - 24.0 * s - center_w);
    let (back, forth) = if b.manga { (Action::Next, Action::Prev) } else { (Action::Prev, Action::Next) };
    let (start, end) = if b.manga { (Action::Last, Action::First) } else { (Action::First, Action::Last) };
    let page_keys = if b.manga { (k(Bind::Right), k(Bind::Left)) } else { (k(Bind::Left), k(Bind::Right)) };
    let (first_key, last_key) = if b.manga { (k(Bind::Last), k(Bind::First)) } else { (k(Bind::First), k(Bind::Last)) };
    push(cx0, btn, icon(Icon::First), act(start), false, tip(t("Inizio", "Start"), &first_key));
    push(cx0 + btn, btn, icon(Icon::Prev), act(back), false, tip(t("Indietro", "Back"), &page_keys.0));
    push(cx0 + 2.0 * btn, folio_w, Look::Folio(format!("{}  /  {}", b.folio, b.pages)), act(Action::AskPage), false,
         tip(t("Vai a pagina", "Go to page"), &k(Bind::GoTo)));
    push(cx0 + 2.0 * btn + folio_w, btn, icon(Icon::Next), act(forth), false,
         tip(t("Avanti", "Forward"), &page_keys.1));
    push(cx0 + 3.0 * btn + folio_w, btn, icon(Icon::Last), act(end), false, tip(t("Fine", "End"), &last_key));

    push(mark, btn, icon(Icon::Bookmark), act(Action::ToggleBookmark), b.bookmarked,
         tip(if b.bookmarked { t("Pagina segnata", "Bookmarked") } else { t("Segna la pagina", "Bookmark the page") },
             &k(Bind::Bookmark)));
    let single_cmd = if b.double { act(Action::ToggleDouble) } else if b.strip { act(Action::ToggleStrip) } else { None };
    let is_single = !b.double && !b.strip;
    push(single, btn, icon(Icon::Single), single_cmd.or(act(Action::ZoomTo(Zoom::Page))), is_single,
         tip(t("Pagina singola", "Single page"), &k(if b.double { Bind::Double } else { Bind::Strip })));
    push(double, btn, icon(Icon::Double), act(Action::ToggleDouble), b.double,
         tip(t("Doppia pagina", "Two pages"), &k(Bind::Double)));
    push(strip, btn, icon(Icon::Strip), act(Action::ToggleStrip), b.strip, tip(t("Nastro", "Strip"), &k(Bind::Strip)));
    push(dir, btn, icon(if b.manga { Icon::RightToLeft } else { Icon::LeftToRight }), act(Action::ToggleManga),
         b.manga, tip(if b.manga { t("Da destra a sinistra", "Right to left") } else { t("Da sinistra a destra", "Left to right") }, &k(Bind::Manga)));
    let zoomable = b.zoom.is_some();
    push(minus, btn, icon(Icon::Minus), zoomable.then_some(Command::Act(Action::ZoomOut)), false,
         tip(t("Riduci", "Zoom out"), &k(Bind::ZoomOut)));
    push(zoom, 64.0 * s, Look::Zoom(b.zoom.map_or("\u{2014}".into(), |z| format!("{z}%"))),
         zoomable.then_some(Command::Act(Action::ZoomTo(Zoom::Page))), false,
         tip(t("Livelli di zoom", "Zoom levels"), &format!("{} {} {}", k(Bind::ZoomPage), k(Bind::ZoomWidth), k(Bind::ZoomActual))));
    push(plus, btn, icon(Icon::Plus), zoomable.then_some(Command::Act(Action::ZoomIn)), false,
         tip(t("Ingrandisci", "Zoom in"), &k(Bind::ZoomIn)));
    push(full + 2.0 * s, btn, icon(if ctx.fullscreen { Icon::FullExit } else { Icon::Full }),
         act(Action::ToggleFullscreen), false,
         tip(if ctx.fullscreen { t("Esci dallo schermo intero", "Exit full screen") } else { t("Schermo intero", "Full screen") }, &k(Bind::Fullscreen)));
    push(hide, btn, icon(Icon::Hide), act(Action::ToggleHud), false,
         tip(t("Nascondi la barra", "Hide the bar"), &k(Bind::Hud)));
    v
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
        let color = if !enabled { alpha(MUTED_F, 0.35) } else if btn.on || hovered { INK_F } else { MUTED_F };
        let cx = btn.x + btn.w / 2.0;
        match &btn.face {
            Look::Icon(icon) => draw_icon(&mut l, *icon, cx.round(), cy.round(), s, color),
            Look::Folio(label) => l.texts.push(Text::new(label.as_str(), Face::Serif, 19.0 * s, INK)
                .boxed(btn.w, Align::Center).on_baseline(btn.x, cy + 6.5 * s)),
            Look::Zoom(label) => l.texts.push(Text::new(label.as_str(), Face::Sans, 12.5 * s, to_u8(color)).weight(500)
                .tabular().boxed(btn.w, Align::Center).on_baseline(btn.x, cy + 4.5 * s)),
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
    l.rects.push(Rect::new(x + s, y + s, w - 2.0 * s, th - 2.0 * s, alpha(rgb(0x14, 0x13, 0x11), 0.98)).radius(7.0 * s));
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
        Icon::Gallery => boxes.extend([(-7.0, -7.0, 6.0, 6.0), (1.0, -7.0, 6.0, 6.0), (-7.0, 1.0, 6.0, 6.0), (1.0, 1.0, 6.0, 6.0)]),
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
