//! Cio' che sta sopra la tavola oltre alla barra: la didascalia, il
//! righello, vai a pagina, la galleria vuota, la lente, gli avvisi.

use super::*;

/// Un nastrino rosso lacca: la pagina e' segnata. Largo `w`, alto `h`, con
/// la coda a V. Il corpo e' un rettangolo; le due punte della coda si
/// disegnano una riga di pixel alla volta, sempre piu' strette: a questa
/// misura la scaletta non si vede, e il fondo resta quello che e'.
fn ribbon(l: &mut Layer, x: f32, y: f32, w: f32, h: f32, a: f32) {
    let notch = (w * 0.7).round().max(2.0);
    let body = h - notch;
    l.rects.push(Rect::new(x, y, w, body, alpha(ACCENT, a)));
    for k in 0..notch as usize {
        let leg = (w / 2.0 * (1.0 - (k as f32 + 0.5) / notch)).max(0.5);
        let row = y + body + k as f32;
        l.rects.push(Rect::new(x, row, leg, 1.0, alpha(ACCENT, a)));
        l.rects.push(Rect::new(x + w - leg, row, leg, 1.0, alpha(ACCENT, a)));
    }
}

/// Titolo e pagina nei margini; un avviso sopra il numero di pagina.
pub(super) fn caption(ctx: &Context, b: &BookInfo, a: f32, toast: Option<(&str, f32)>, m: &mut dyn Measure) -> Layer {
    let s = ctx.scale;
    let (vw, vh) = ctx.view;
    let pad = 40.0 * s;
    let bottom = vh - 38.0 * s;
    let mut l = Layer::default();

    let title = Text::new(b.title, Face::Serif, 26.0 * s, fade(INK, a));
    let modes = caps(&b.modes.join("  \u{00b7}  "), 11.5 * s, fade(MUTED, a));
    let folio = Text::new(b.folio.as_str(), Face::Serif, FOLIO * s, fade(INK, a));
    let of = caps(&if italian() { format!("di {}", b.pages) } else { format!("of {}", b.pages) }, 11.5 * s,
                  fade(MUTED, a)).tabular();
    // nei margini se ci stanno, il titolo anche su due righe come
    // un'etichetta da museo; se no le pagine arrivano ai bordi, e una
    // sfumatura dal basso rende leggibile cio' che ci si scrive sopra
    let margin_room = b.margins.0 - pad - 20.0 * s;
    let wrapped = title.clone().boxed(margin_room.max(1.0), Align::Left);
    let title_lines = if m.width(&title) <= margin_room { 1 } else { m.lines(&wrapped) };
    let left_fits = title_lines <= 2 && m.width(&modes) <= margin_room;
    let right_fits = m.width(&folio).max(m.width(&of)) + pad + 24.0 * s <= b.margins.1;
    let banded = !(left_fits && right_fits);
    let visible = a.max(toast.map_or(0.0, |t| t.1));
    if banded {
        let h = 240.0 * s;
        l.rects.push(Rect::new(0.0, vh - h, vw, h, [0.0, 0.0, 0.0, 0.0]).to([0.0, 0.0, 0.0, 0.9 * visible]));
    }

    if a > 0.0 {
        let base = bottom - 25.0 * s;
        if banded {
            l.texts.push(fit(m, title, vw * 0.45).on_baseline(pad, base));
            l.texts.push(fit(m, modes, vw * 0.45).on_baseline(pad, bottom));
        } else {
            // due righe: la prima una riga sopra, l'ultima al solito posto
            let up = if title_lines == 2 { (title.size * 1.2).ceil() } else { 0.0 };
            l.texts.push(wrapped.on_baseline(pad, base - up));
            l.texts.push(modes.on_baseline(pad, bottom));
        }
        let bw = 300.0 * s;
        let bx = vw - pad - bw;
        if b.bookmarked {
            // accanto al numero di pagina, alto quanto le sue cifre
            let fw = m.width(&folio);
            let rh = (FOLIO * s * 0.62).round();
            ribbon(&mut l, (vw - pad - fw - 26.0 * s).round(), (bottom - 24.0 * s - rh).round(), (9.0 * s).round(), rh, a);
        }
        l.texts.push(folio.boxed(bw, Align::Right).on_baseline(bx, bottom - 24.0 * s));
        l.texts.push(of.boxed(bw, Align::Right).on_baseline(bx, bottom));
    }
    if let Some((text, ta)) = toast {
        // sopra il numero di pagina, separato da un filo
        let line_y = (bottom - 24.0 * s - FOLIO * s * 0.72 - 22.0 * s).round();
        let lw = 40.0 * s;
        l.rects.push(Rect::new((vw - pad - lw).round(), line_y, lw.round(), s.round(), alpha(TICK, ta)));
        let bw = 400.0 * s;
        l.texts.push(Text::new(text, Face::Sans, 15.0 * s, fade(INK, ta)).weight(500)
            .boxed(bw, Align::Right).on_baseline(vw - pad - bw, line_y - 15.0 * s));
    }
    l
}

/// Il righello di "vai a pagina": (x, larghezza, y) in pixel.
pub(super) fn ruler(ctx: &Context) -> (f32, f32, f32) {
    let s = ctx.scale;
    let (vw, vh) = ctx.view;
    let w = (560.0 * s).min(vw * 0.6).round();
    (((vw - w) / 2.0).round(), w, (vh / 2.0 + 70.0 * s).round())
}

pub(super) fn goto(ctx: &Context, g: &GoTo, m: &mut dyn Measure) -> Layer {
    let s = ctx.scale;
    let (vw, vh) = ctx.view;
    let cx = vw / 2.0;
    let base = vh / 2.0 + 10.0 * s;
    let mut l = Layer::default();
    l.rects.push(Rect::new(0.0, 0.0, vw, vh, [0.0, 0.0, 0.0, 0.9]));

    l.texts.push(caps(t("Vai a pagina", "Go to page"), 11.0 * s, MUTED).boxed(400.0 * s, Align::Center)
        .on_baseline(cx - 200.0 * s, base - 150.0 * s));
    // numero, cursore e "di 212" centrati insieme, misurati
    let (digits, color) = if g.typed.is_empty() { (g.page().to_string(), MUTED) } else { (g.typed.clone(), INK) };
    let size = 156.0 * s;
    let number = Text::new(digits, Face::Serif, size, color);
    let of = Text::new(if italian() { format!("di {}", g.pages) } else { format!("of {}", g.pages) },
                       Face::Serif, 30.0 * s, MUTED);
    let (wn, wo, gap) = (m.width(&number), m.width(&of), 34.0 * s);
    let x0 = (cx - (wn + gap + wo) / 2.0).round();
    l.texts.push(number.on_baseline(x0, base));
    let caret_h = (size * 0.66).round();
    l.rects.push(Rect::new((x0 + wn + 12.0 * s).round(), (base - caret_h).round(), (2.0 * s).round(), caret_h, ACCENT));
    l.texts.push(of.on_baseline(x0 + wn + gap, base));

    let (x0, w, y) = ruler(ctx);
    let last = (g.pages.max(2) - 1) as f32;
    let at = |page: usize| (x0 + w * page as f32 / last).round();
    l.rects.push(Rect::new(x0, y, w, s.round(), HAIR));
    // una tacca per pagina se c'e' posto, altrimenti ogni 2, 5, 10...
    let every = [1usize, 2, 5, 10, 20, 50, 100, 200, 500]
        .into_iter()
        .find(|&k| w / last * k as f32 >= 4.0 * s)
        .unwrap_or(1000);
    for p in (0..g.pages).step_by(every) {
        let major = (p + 1) % (every * 10) == 0 || p == 0;
        let h = if major { 9.0 } else { 5.0 } * s;
        l.rects.push(Rect::new(at(p), y - h, s.round(), h, if major { TICK } else { HAIR }));
    }
    let label_every = [10usize, 20, 25, 50, 100, 200, 250, 500, 1000, 2000]
        .into_iter()
        .find(|&k| w / last * k as f32 >= 64.0 * s)
        .unwrap_or(5000);
    for p in (label_every..=g.pages).step_by(label_every) {
        l.texts.push(Text::new(p.to_string(), Face::Sans, 10.5 * s, MUTED).tabular().boxed(60.0 * s, Align::Center)
            .on_baseline(at(p - 1) - 30.0 * s, y + 26.0 * s));
    }
    // le pagine segnate: nastrini sotto il righello
    for &p in &g.bookmarks {
        ribbon(&mut l, (at(p) - 3.0 * s).round(), y + 5.0 * s, (7.0 * s).round(), (13.0 * s).round(), 1.0);
    }
    // dove si e' adesso, e dove si andra'
    let here = at(g.here);
    l.rects.push(Rect::new(here, y - 16.0 * s, s.round(), 16.0 * s, INK_F));
    l.texts.push(caps(t("qui", "here"), 9.0 * s, INK).boxed(60.0 * s, Align::Center).on_baseline(here - 30.0 * s, y - 22.0 * s));
    if g.valid() && g.page() - 1 != g.here {
        let x = at(g.page() - 1);
        l.rects.push(Rect::new(x - s, y - 24.0 * s, (2.0 * s).round(), 24.0 * s, ACCENT));
        let d = 7.0 * s;
        l.rects.push(Rect::new(x - d / 2.0, y - 24.0 * s - d / 2.0, d, d, ACCENT).radius(d));
    }
    let (hint, color) = if g.valid() {
        (t("Invio per andare   \u{00b7}   Esc per restare", "Enter to go   \u{00b7}   Esc to stay").to_owned(), MUTED)
    } else if italian() {
        (format!("Il volume ha {} pagine", g.pages), ACCENT_TEXT)
    } else {
        (format!("This volume has {} pages", g.pages), ACCENT_TEXT)
    };
    l.texts.push(Text::new(hint, Face::Sans, 12.5 * s, color).boxed(500.0 * s, Align::Center)
        .on_baseline(cx - 250.0 * s, y + 70.0 * s));
    l
}

/// La galleria vuota: il nome, cosa si puo' fare, gli ultimi letti.
pub(super) fn welcome(ctx: &Context, selected: Option<usize>, m: &mut dyn Measure) -> Layer {
    let s = ctx.scale;
    let (vw, vh) = ctx.view;
    let cx = vw / 2.0;
    let top = Ui::welcome_top(ctx);
    let mut l = Layer::default();
    let wide = 900.0 * s;
    l.texts.push(Text::new("NicoReader", Face::Serif, 84.0 * s, INK).boxed(wide, Align::Center)
        .on_baseline(cx - wide / 2.0, top + 70.0 * s));
    l.texts.push(caps(t("Trascina qui un volume, o fai clic per aprirne uno",
                        "Drop a volume here, or click to open one"), 10.5 * s, MUTED)
        .boxed(wide, Align::Center).on_baseline(cx - wide / 2.0, top + 112.0 * s));
    // l'invito a creare la libreria, in rosso lacca: e' la cosa da fare
    l.texts.push(caps(t("+   Aggiungi una cartella alla libreria", "+   Add a folder to the library"), 11.0 * s, ACCENT_TEXT)
        .boxed(wide, Align::Center).on_baseline(cx - wide / 2.0, Ui::welcome_cta(ctx)));
    let recent = &ctx.recent[..ctx.recent.len().min(6)];
    if !recent.is_empty() {
        l.rects.push(Rect::new((cx - 20.0 * s).round(), (top + 190.0 * s).round(), (40.0 * s).round(), s.round(), TICK));
        l.texts.push(caps(t("Ultimi letti", "Recently read"), 10.0 * s, MUTED).boxed(wide, Align::Center)
            .on_baseline(cx - wide / 2.0, top + 234.0 * s));
        let x0 = cx - WELCOME_W * s / 2.0;
        let w = WELCOME_W * s;
        let rows_top = Ui::welcome_rows_top(ctx);
        for (i, r) in recent.iter().enumerate() {
            let y = rows_top + i as f32 * WELCOME_ROW * s;
            if selected == Some(i) {
                l.rects.push(Rect::new(x0 - 14.0 * s, y + 3.0 * s, w + 28.0 * s, (WELCOME_ROW - 6.0) * s, [1.0, 1.0, 1.0, 0.05])
                    .radius(8.0 * s));
            }
            if i > 0 {
                l.rects.push(Rect::new(x0, y.round(), w, s.round(), alpha(HAIR, 0.7)));
            }
            let title = Text::new(r.title.as_str(), Face::Serif, 22.0 * s, INK);
            l.texts.push(fit(m, title, w - 130.0 * s).on_baseline(x0, y + 30.0 * s));
            l.texts.push(Text::new(r.place.as_str(), Face::Sans, 12.5 * s, MUTED).tabular().boxed(110.0 * s, Align::Right)
                .on_baseline(x0 + w - 110.0 * s, y + 29.0 * s));
        }
    }
    let k = |b: Bind| ctx.keys.label(b);
    let keys = if italian() {
        format!("{} apri   \u{00b7}   {} cartella   \u{00b7}   {} impostazioni   \u{00b7}   tasto destro per il menu",
                k(Bind::Open), k(Bind::OpenFolder), k(Bind::Settings))
    } else {
        format!("{} open   \u{00b7}   {} folder   \u{00b7}   {} settings   \u{00b7}   right click for the menu",
                k(Bind::Open), k(Bind::OpenFolder), k(Bind::Settings))
    };
    l.texts.push(Text::new(keys, Face::Sans, 12.0 * s, fade(MUTED, 0.75)).boxed(wide, Align::Center)
        .on_baseline(cx - wide / 2.0, vh - 36.0 * s));
    l
}

/// Il bordo della lente: un filo chiaro, e fuori un'ombra che la stacca
/// dalla pagina (il vetro, dentro, non si tocca).
pub(super) fn lens_ring(x: f32, y: f32, r: f32, s: f32) -> Layer {
    let mut l = Layer::default();
    let ring = |grow: f32, width: f32, color: [f32; 4], blur: f32| {
        let rr = r + grow;
        Rect::new(x - rr, y - rr, 2.0 * rr, 2.0 * rr, color).radius(rr).stroke(width).blur(blur)
    };
    l.rects.push(ring(6.0 * s, 8.0 * s, [0.0, 0.0, 0.0, 0.45], 8.0 * s));
    l.rects.push(ring(1.5 * s, 2.0 * s, alpha(INK_F, 0.9), 0.0));
    l.rects.push(ring(3.0 * s, 1.0 * s, alpha(BLACK, 0.8), 0.0));
    l
}

/// Nella libreria: solo l'avviso, in basso a destra.
pub(super) fn toast_only(ctx: &Context, text: &str, a: f32) -> Layer {
    let s = ctx.scale;
    let (vw, vh) = ctx.view;
    let mut l = Layer::default();
    let bw = 500.0 * s;
    let base = vh - 40.0 * s;
    l.rects.push(Rect::new(vw - 460.0 * s, vh - 90.0 * s, 460.0 * s, 90.0 * s, [0.0, 0.0, 0.0, 0.0]).to([0.0, 0.0, 0.0, 0.7 * a]));
    l.texts.push(Text::new(text, Face::Sans, 15.0 * s, fade(INK, a)).weight(500).boxed(bw, Align::Right)
        .on_baseline(vw - 40.0 * s - bw, base));
    l
}

pub(super) fn to_u8(c: [f32; 4]) -> [u8; 4] {
    c.map(|v| (v * 255.0).round() as u8)
}
