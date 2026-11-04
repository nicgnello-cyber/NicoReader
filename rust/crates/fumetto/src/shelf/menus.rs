//! I menu della libreria: il tasto destro su una cella, le cartelle, l'ordine.

use super::*;
use crate::ui::Row;

impl Shelf {
    /// Le righe del menu del tasto destro su una cella.
    pub(crate) fn tile_menu(d: &ShelfData, tile: &Tile) -> Vec<Row> {
        match tile {
            Tile::Series { name, .. } => vec![
                Row::Head(t("Serie", "Series")),
                Row::plain(t("Apri la serie", "Open the series"), Command::Series(name.clone())),
            ],
            Tile::Group { kind, name, .. } => vec![
                Row::Head(kind_label(*kind, false)),
                Row::plain(t("Apri", "Open"), Command::Group(*kind, name.clone())),
            ],
            Tile::Folder { path, .. } => folder_menu(path),
            Tile::Volume(i) => volume_menu(d, *i),
        }
    }

    /// Le righe del menu delle cartelle della libreria.
    pub(crate) fn folders_menu(d: &ShelfData, servers: &[Server]) -> Vec<Row> {
        let mut rows = Vec::new();
        if !d.roots.is_empty() || servers.is_empty() {
            rows.push(Row::Head(t("Cartelle della libreria", "Library folders")));
        }
        for root in d.roots {
            let label = root.to_string_lossy().into_owned();
            rows.push(Row::Folder { label, cmd: Command::RemoveFolder(root.clone()) });
        }
        if !servers.is_empty() {
            rows.push(Row::Head(t("Server", "Servers")));
        }
        for s in servers {
            rows.push(Row::Folder { label: remote::shown(&s.url), cmd: Command::RemoveServer(s.url.clone()) });
        }
        rows.extend([
            Row::Sep,
            Row::plain(
                t("Aggiungi una cartella\u{2026}", "Add a folder\u{2026}"),
                Command::Act(Action::AddLibraryFolder),
            ),
            Row::plain(t("Aggiungi un server\u{2026}", "Add a server\u{2026}"), Command::AskServer),
        ]);
        rows
    }

    /// Le righe del menu dell'ordine: quello scelto ha il segno.
    pub(crate) fn sort_menu(current: LibrarySort) -> Vec<Row> {
        let mut rows = vec![Row::Head(t("Ordina la libreria", "Sort the library"))];
        rows.extend(
            LibrarySort::ALL.into_iter().map(|sort| Row::check(sort_label(sort), sort == current, Command::Sort(sort))),
        );
        rows
    }
}

fn folder_menu(path: &Path) -> Vec<Row> {
    let mut rows = vec![
        Row::Head(t("Cartella", "Folder")),
        Row::plain(t("Apri la cartella", "Open the folder"), Command::Folder(path.to_path_buf())),
    ];
    if !remote::is_remote(path) {
        let label = t("Mostra nel gestore dei file", "Show in the file manager");
        rows.push(Row::plain(label, Command::Reveal(path.to_path_buf())));
    }
    rows
}

/// Leggere, segnare letto o da leggere, correggere i dati; poi, per un
/// volume su disco, la cartella e il cestino; per uno su un server (che non
/// ha ne' l'una ne' l'altro), la copia da leggere senza rete.
fn volume_menu(d: &ShelfData, i: usize) -> Vec<Row> {
    let path = &d.entries[i].path;
    let mut rows = vec![Row::plain(t("Leggi", "Read"), Command::Open(path.clone())), Row::Sep];
    if d.status[i] != Status::Done {
        rows.push(Row::plain(t("Segna come letto", "Mark as read"), Command::MarkRead(path.clone(), true)));
    }
    if d.status[i] != Status::New {
        rows.push(Row::plain(t("Segna come da leggere", "Mark as unread"), Command::MarkRead(path.clone(), false)));
    }
    rows.push(Row::plain(t("Modifica i dati\u{2026}", "Edit details\u{2026}"), Command::EditVolume(path.clone())));
    if d.edited.get(i) == Some(&true) {
        let label = t("Torna ai dati del file", "Back to the file's details");
        rows.push(Row::plain(label, Command::ResetVolume(path.clone())));
    }
    if remote::is_remote(path) {
        rows.extend(offline_row(path).map(|row| [Row::Sep, row]).into_iter().flatten());
        return rows;
    }
    rows.extend([
        Row::Sep,
        Row::plain(t("Mostra nella cartella", "Show in folder"), Command::Reveal(path.clone())),
        Row::plain(t("Sposta nel cestino\u{2026}", "Move to trash\u{2026}"), Command::Trash(path.clone())),
    ]);
    rows
}

/// Per un volume su un server: scaricarlo per leggerlo senza rete, o
/// togliere la copia scaricata. `None`: il server non lo permette.
fn offline_row(path: &Path) -> Option<Row> {
    let path = path.to_path_buf();
    if remote::offline::downloaded(&path).is_some() {
        Some(Row::plain(t("Togli la copia scaricata", "Remove the downloaded copy"), Command::ForgetDownload(path)))
    } else if remote::offline::can_download(&path) {
        Some(Row::plain(t("Scarica per leggere senza rete", "Download to read offline"), Command::Download(path)))
    } else {
        None
    }
}
