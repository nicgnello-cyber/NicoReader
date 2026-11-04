//! La galleria vuota: il nome, cosa si puo' fare, gli ultimi letti; e dove
//! stanno, per i clic.

use super::*;

/// La galleria vuota: il nome, cosa si puo' fare, gli ultimi letti.
pub(super) fn welcome(ctx: &Context, selected: Option<usize>, m: &mut dyn Measure) -> Layer {
    let s = ctx.scale;
    let (vw, vh) = ctx.view;
    let cx = vw / 2.0;
    let top = welcome_top(ctx);
    let wide = 900.0 * s;
    let centered = |text: Text, base: f32| text.boxed(wide, Align::Center).on_baseline(cx - wide / 2.0, base);
    let mut l = Layer::default();
    l.texts.push(centered(Text::new("NicoReader", Face::Serif, 84.0 * s, INK), top + 70.0 * s));
    let drop = t("Trascina qui un volume, o fai clic per aprirne uno", "Drop a volume here, or click to open one");
    l.texts.push(centered(caps(drop, 10.5 * s, MUTED), top + 112.0 * s));
    // l'invito a creare la libreria, in rosso lacca: e' la cosa da fare
    let add = t("+   Aggiungi una cartella alla libreria", "+   Add a folder to the library");
    l.texts.push(centered(caps(add, 11.0 * s, ACCENT_TEXT), welcome_cta(ctx)));
    let recent = &ctx.recent[..ctx.recent.len().min(6)];
    if !recent.is_empty() {
        l.rects.push(Rect::new(
            (cx - 20.0 * s).round(),
            (top + 190.0 * s).round(),
            (40.0 * s).round(),
            s.round(),
            TICK,
        ));
        l.texts.push(centered(caps(t("Ultimi letti", "Recently read"), 10.0 * s, MUTED), top + 234.0 * s));
        draw_recent(&mut l, ctx, recent, selected, m);
    }
    l.texts.push(centered(Text::new(keys_hint(ctx), Face::Sans, 12.0 * s, fade(MUTED, 0.75)), vh - 36.0 * s));
    l
}

/// Gli ultimi letti, uno per riga: il titolo e dove si e' arrivati; quello
/// scelto con le frecce, illuminato.
fn draw_recent(l: &mut Layer, ctx: &Context, recent: &[Recent], selected: Option<usize>, m: &mut dyn Measure) {
    let s = ctx.scale;
    let w = WELCOME_W * s;
    let x0 = ctx.view.0 / 2.0 - w / 2.0;
    let rows_top = welcome_rows_top(ctx);
    for (i, r) in recent.iter().enumerate() {
        let y = rows_top + i as f32 * WELCOME_ROW * s;
        if selected == Some(i) {
            l.rects.push(
                Rect::new(x0 - 14.0 * s, y + 3.0 * s, w + 28.0 * s, (WELCOME_ROW - 6.0) * s, [1.0, 1.0, 1.0, 0.05])
                    .radius(8.0 * s),
            );
        }
        if i > 0 {
            l.rects.push(Rect::new(x0, y.round(), w, s.round(), alpha(HAIR, 0.7)));
        }
        let title = Text::new(r.title.as_str(), Face::Serif, 22.0 * s, INK);
        l.texts.push(fit(m, title, w - 130.0 * s).on_baseline(x0, y + 30.0 * s));
        l.texts.push(
            Text::new(r.place.as_str(), Face::Sans, 12.5 * s, MUTED)
                .tabular()
                .boxed(110.0 * s, Align::Right)
                .on_baseline(x0 + w - 110.0 * s, y + 29.0 * s),
        );
    }
}

/// In fondo, i tasti da sapere.
fn keys_hint(ctx: &Context) -> String {
    let k = |b: Bind| ctx.keys.label(b);
    let (open, folder, settings) = (k(Bind::Open), k(Bind::OpenFolder), k(Bind::Settings));
    if italian() {
        format!(
            "{open} apri   \u{00b7}   {folder} cartella   \u{00b7}   {settings} impostazioni   \u{00b7}   tasto destro per il menu"
        )
    } else {
        format!(
            "{open} open   \u{00b7}   {folder} folder   \u{00b7}   {settings} settings   \u{00b7}   right click for the menu"
        )
    }
}

fn welcome_top(ctx: &Context) -> f32 {
    let s = ctx.scale;
    let n = ctx.recent.len().min(6) as f32;
    let height = if n > 0.0 { 276.0 + n * WELCOME_ROW } else { 170.0 } * s;
    ((ctx.view.1 - height) / 2.0).max(24.0 * s)
}

/// La cima della prima riga degli ultimi letti.
pub(super) fn welcome_rows_top(ctx: &Context) -> f32 {
    welcome_top(ctx) + 276.0 * ctx.scale
}

/// La linea di base dell'invito a creare la libreria.
fn welcome_cta(ctx: &Context) -> f32 {
    welcome_top(ctx) + 152.0 * ctx.scale
}

/// Il clic e' sull'invito a creare la libreria.
pub(super) fn welcome_cta_hit(ctx: &Context, y: f32) -> bool {
    (y - (welcome_cta(ctx) - 5.0 * ctx.scale)).abs() < 16.0 * ctx.scale
}

/// Quale degli ultimi letti e' in (`x`, `y`).
pub(super) fn welcome_hit(ctx: &Context, x: f32, y: f32) -> Option<usize> {
    let s = ctx.scale;
    let x0 = (ctx.view.0 - WELCOME_W * s) / 2.0;
    let top = welcome_rows_top(ctx);
    if x < x0 || x > x0 + WELCOME_W * s || y < top {
        return None;
    }
    let i = ((y - top) / (WELCOME_ROW * s)) as usize;
    (i < ctx.recent.len().min(6)).then_some(i)
}
