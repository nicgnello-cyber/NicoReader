//! Le voci del menu (tasto destro) e di quello dello zoom.

use super::*;

/// Una voce di menu con il suo tasto, quello che vale adesso.
fn item(ctx: &Context, label: &'static str, bind: Bind, on: bool, action: Action) -> Row {
    Row::Item { label, key: ctx.keys.label(bind), on, cmd: Command::Act(action) }
}

/// Il menu del tasto destro, fuori dalle copertine. Con un volume aperto, in
/// due colonne: a sinistra i volumi e le pagine, a destra come si legge.
pub(super) fn main_rows(ctx: &Context) -> Vec<Row> {
    let i = |label, bind, on, action| item(ctx, label, bind, on, action);
    let mut rows = vec![
        i(t("Apri\u{2026}", "Open\u{2026}"), Bind::Open, false, Action::Open),
        i(t("Apri cartella\u{2026}", "Open folder\u{2026}"), Bind::OpenFolder, false, Action::OpenFolder),
        i(t("Libreria", "Library"), Bind::Library, ctx.shelf.is_some(), Action::ToggleLibrary),
        Row::Item {
            label: t("Aggiungi un server\u{2026}", "Add a server\u{2026}"),
            key: String::new(),
            on: false,
            cmd: Command::AskServer,
        },
    ];
    let recent = if ctx.book.is_some() { 3 } else { 5 };
    if !ctx.recent.is_empty() {
        rows.push(Row::Sep);
        rows.push(Row::Head(t("Recenti", "Recent")));
        for r in ctx.recent.iter().take(recent) {
            rows.push(Row::Recent {
                title: r.title.clone(),
                place: r.place.clone(),
                cmd: Command::Open(r.path.clone()),
            });
        }
    }
    let Some(b) = &ctx.book else {
        rows.push(Row::Sep);
        rows.push(i(t("Impostazioni\u{2026}", "Settings\u{2026}"), Bind::Settings, false, Action::ToggleSettings));
        return rows;
    };
    rows.extend([
        Row::Sep,
        i(t("Vai a pagina\u{2026}", "Go to page\u{2026}"), Bind::GoTo, false, Action::AskPage),
        i(t("Miniature", "Thumbnails"), Bind::Thumbs, ctx.thumbs.is_some(), Action::ToggleThumbs),
        i(t("Segna la pagina", "Bookmark the page"), Bind::Bookmark, b.bookmarked, Action::ToggleBookmark),
    ]);
    if !b.bookmarks.is_empty() {
        rows.push(Row::Head(t("Segnalibri", "Bookmarks")));
        // i segni piu' vicini alla pagina che si legge
        let mut near: Vec<usize> = b.bookmarks.to_vec();
        near.sort_by_key(|p| p.abs_diff(b.here));
        near.truncate(6);
        near.sort_unstable();
        for p in near {
            let title = if italian() { format!("Pagina {}", p + 1) } else { format!("Page {}", p + 1) };
            let place = if p == b.here { t("qui", "here").to_owned() } else { String::new() };
            rows.push(Row::Recent { title, place, cmd: Command::Act(Action::GoTo(p)) });
        }
    }
    rows.extend([
        Row::Break,
        i(t("Doppia pagina", "Two pages"), Bind::Double, b.double, Action::ToggleDouble),
        i(t("Copertina da sola", "Cover alone"), Bind::Cover, b.double && b.cover_alone, Action::ToggleCover),
        i(t("Nastro", "Strip"), Bind::Strip, b.strip, Action::ToggleStrip),
        i(t("Da destra a sinistra", "Right to left"), Bind::Manga, b.manga, Action::ToggleManga),
        Row::Sep,
        i(t("Ruota a destra", "Rotate right"), Bind::RotateRight, false, Action::Rotate(true)),
        i(t("Ruota a sinistra", "Rotate left"), Bind::RotateLeft, false, Action::Rotate(false)),
        i(t("Rifila i margini", "Trim margins"), Bind::Trim, b.trim, Action::ToggleTrim),
        i(t("Lente", "Magnifier"), Bind::Lens, ctx.lens.is_some(), Action::ToggleLens),
        i(t("Presentazione", "Slideshow"), Bind::Slideshow, b.slideshow, Action::ToggleSlideshow),
        Row::Sep,
        i(t("Salva la pagina\u{2026}", "Save the page\u{2026}"), Bind::Save, false, Action::SavePage),
        i(t("Copia la pagina", "Copy the page"), Bind::Copy, false, Action::CopyPage),
        Row::Sep,
        i(t("Schermo intero", "Full screen"), Bind::Fullscreen, ctx.fullscreen, Action::ToggleFullscreen),
        i(t("Barra in alto", "Top bar"), Bind::Hud, ctx.hud, Action::ToggleHud),
        i(t("Impostazioni\u{2026}", "Settings\u{2026}"), Bind::Settings, false, Action::ToggleSettings),
        Row::Sep,
        i(t("Chiudi", "Close"), Bind::Close, false, Action::Close),
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
