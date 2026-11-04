//! Cosa mostrare: le sezioni della libreria (continua a leggere, la libreria
//! con le serie in pila, i gruppi, le cartelle, i trovati sui server), con
//! filtro, ricerca e ordine applicati.

use super::*;

impl Shelf {
    /// Le sezioni da mostrare, con filtro, ricerca e serie applicati.
    pub(super) fn sections(&self, d: &ShelfData) -> Vec<Section> {
        let needle = self.search.to_lowercase();
        let found = |e: &Entry| matches(e, &needle);
        let mut visible = self.visible(d, &found);
        let single = |tiles: Vec<Tile>| vec![Section { title: String::new(), tiles, one_row: false }];

        if self.series.is_some() {
            visible.sort_by(|&a, &b| by_number(&d.entries[a], &d.entries[b]));
            return single(visible.into_iter().map(Tile::Volume).collect());
        }
        if let Some((kind, name)) = &self.group {
            // una lista di lettura si legge nel suo ordine, volume per volume
            if *kind == GroupKind::ReadingList {
                sort_members(d, &mut visible, *kind, name);
                return single(visible.into_iter().map(Tile::Volume).collect());
            }
            return single(stacks(d, &visible, self.sort));
        }
        if self.groups {
            return group_sections(d, self.filter, &needle, &found);
        }
        // per cartelle: la ricerca resta un elenco unico, i filtri valgono dentro
        if self.folders && needle.is_empty() {
            let (here, top) = self.folder_here(d);
            let mut sections: Vec<Section> =
                reading_row(d, &visible).filter(|_| top && self.filter == Filter::All).into_iter().collect();
            sections.extend(folder_sections(d, &visible, here.as_deref(), self.sort));
            return sections;
        }
        if self.filter != Filter::All || !needle.is_empty() {
            return self.found_sections(d, visible, !needle.is_empty());
        }
        let mut sections: Vec<Section> = reading_row(d, &visible).into_iter().collect();
        let n = d.library_len();
        let title = if italian() {
            format!("Tutta la libreria \u{00b7} {n}")
        } else {
            format!("The whole library \u{00b7} {n}")
        };
        sections.push(Section { title, tiles: stacks(d, &visible, self.sort), one_row: false });
        sections
    }

    /// I volumi della libreria che passano filtro e ricerca, e che stanno
    /// nella serie o nel gruppo in cui si e'.
    fn visible(&self, d: &ShelfData, found: &dyn Fn(&Entry) -> bool) -> Vec<usize> {
        (0..d.library_len())
            .filter(|&i| self.filter.keeps(d.status[i]) && found(&d.entries[i]))
            .filter(|&i| self.series.is_none() || d.entries[i].series == self.series)
            .filter(|&i| self.group.as_ref().is_none_or(|(k, n)| in_group(&d.entries[i], *k, n)))
            .collect()
    }

    /// Filtrando o cercando: un elenco unico di volumi, senza pile; cercando,
    /// sotto anche quelli trovati sui server.
    fn found_sections(&self, d: &ShelfData, mut visible: Vec<usize>, searching: bool) -> Vec<Section> {
        if self.filter == Filter::Reading {
            visible.sort_by_key(|&i| std::cmp::Reverse(d.read_at[i]));
        }
        let title = if searching { t("Trovati", "Found").to_owned() } else { self.filter.label().to_owned() };
        let tiles = match self.sort {
            LibrarySort::Name => visible.into_iter().map(Tile::Volume).collect(),
            sort => sorted(d, visible.into_iter().map(|i| (Tile::Volume(i), Rank::of(d, &[i]))).collect(), sort),
        };
        let mut sections = vec![Section { title, tiles, one_row: false }];
        if let Some(server) = server_section(d, self.filter).filter(|_| searching) {
            // in libreria niente: resta solo cio' che dice il server
            sections.retain(|s| !s.tiles.is_empty());
            sections.push(server);
        }
        sections
    }
}

/// Il volume corrisponde alla ricerca (gia' in minuscolo): nel titolo, nella
/// serie o negli autori.
fn matches(e: &Entry, needle: &str) -> bool {
    let has = |text: &str| text.to_lowercase().contains(needle);
    needle.is_empty() || has(&e.title) || e.series.as_deref().is_some_and(has) || e.authors.as_deref().is_some_and(has)
}

/// Il nome di una cella, per ordinarla.
pub(super) fn tile_name<'a>(d: &'a ShelfData, t: &'a Tile) -> &'a str {
    match t {
        Tile::Volume(i) => &d.entries[*i].title,
        Tile::Series { name, .. } | Tile::Group { name, .. } | Tile::Folder { name, .. } => name,
    }
}
/// Le serie con piu' volumi diventano una pila, al posto del primo; tutto
/// nell'ordine scelto.
pub(super) fn stacks(d: &ShelfData, visible: &[usize], sort: LibrarySort) -> Vec<Tile> {
    let mut series: HashMap<&str, Vec<usize>> = HashMap::new();
    for &i in visible {
        if let Some(name) = d.entries[i].series.as_deref() {
            series.entry(name).or_default().push(i);
        }
    }
    let mut tiles: Vec<(Tile, Rank)> = Vec::new();
    for &i in visible {
        match d.entries[i].series.as_deref().and_then(|name| Some(name).zip(series.get(name))) {
            Some((name, members)) if members.len() >= 2 => {
                // la pila sta al posto del primo dei suoi volumi
                if members[0] != i {
                    continue;
                }
                let mut members = members.clone();
                members.sort_by(|&a, &b| by_number(&d.entries[a], &d.entries[b]));
                let (cover, done) = cover_and_done(d, &members);
                let rank = Rank::of(d, &members);
                tiles.push((Tile::Series { name: name.to_owned(), cover, count: members.len(), done }, rank));
            }
            _ => tiles.push((Tile::Volume(i), Rank::of(d, &[i]))),
        }
    }
    sorted(d, tiles, sort)
}

/// Cio' che conta per ordinare una cella: dei suoi volumi, il piu' recente
/// fra gli aggiunti e fra i letti, e il primo anno.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Rank {
    added: u64,
    read: u64,
    year: Option<u32>,
}

impl Rank {
    fn of(d: &ShelfData, members: &[usize]) -> Rank {
        members.iter().fold(Rank::default(), |r, &i| Rank {
            added: r.added.max(d.entries[i].added),
            read: r.read.max(d.read_at.get(i).copied().unwrap_or(0)),
            year: match (r.year, d.entries[i].year) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            },
        })
    }
}

/// Le celle nell'ordine scelto: i piu' recenti fra gli aggiunti o i letti
/// prima, gli anni dal piu' vecchio (chi non ne ha in fondo); a parita', e
/// per nome, il nome. Un ordine totale: con uno incoerente `sort_by` si
/// fermerebbe con un panic.
pub(super) fn sorted(d: &ShelfData, mut tiles: Vec<(Tile, Rank)>, sort: LibrarySort) -> Vec<Tile> {
    tiles.sort_by(|(a, ra), (b, rb)| {
        let first = match sort {
            LibrarySort::Name => Ordering::Equal,
            LibrarySort::Added => rb.added.cmp(&ra.added),
            LibrarySort::Read => rb.read.cmp(&ra.read),
            LibrarySort::Year => match (ra.year, rb.year) {
                (Some(x), Some(y)) => x.cmp(&y),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            },
        };
        first.then_with(|| natural_cmp(tile_name(d, a), tile_name(d, b))).then_with(|| a.cover().cmp(&b.cover()))
    });
    tiles.into_iter().map(|(t, _)| t).collect()
}

/// Come si chiama un ordine, nel menu e nel suggerimento.
pub(super) fn sort_label(sort: LibrarySort) -> &'static str {
    match sort {
        LibrarySort::Name => t("Per nome", "By name"),
        LibrarySort::Added => t("Aggiunti di recente", "Recently added"),
        LibrarySort::Read => t("Letti di recente", "Recently read"),
        LibrarySort::Year => t("Per anno di uscita", "By year"),
    }
}

/// La fila dei volumi a meta', dal piu' recente.
pub(super) fn reading_row(d: &ShelfData, visible: &[usize]) -> Option<Section> {
    let mut reading: Vec<usize> =
        visible.iter().copied().filter(|&i| matches!(d.status[i], Status::Reading(..))).collect();
    reading.sort_by_key(|&i| std::cmp::Reverse(d.read_at[i]));
    (!reading.is_empty()).then(|| Section {
        title: t("Continua a leggere", "Continue reading").to_owned(),
        tiles: reading.into_iter().map(Tile::Volume).collect(),
        one_row: true,
    })
}

/// Da dove vengono i volumi, per la vista per cartelle: le cartelle della
/// libreria che ne hanno, poi i server ("http://casa:25600").
pub(super) fn sources(d: &ShelfData) -> Vec<PathBuf> {
    let lib = &d.entries[..d.library_len()];
    let mut out: Vec<PathBuf> = d
        .roots
        .iter()
        .filter(|root| lib.iter().any(|e| library::root_of(&e.path, d.roots) == Some(root.as_path())))
        .cloned()
        .collect();
    for e in lib.iter().filter(|e| remote::is_remote(&e.path)) {
        let server = PathBuf::from(library::origin(&e.path));
        if !out.contains(&server) {
            out.push(server);
        }
    }
    out
}

/// Il nome di una cartella; di un server, l'indirizzo senza "http://".
pub(super) fn folder_name(path: &Path) -> String {
    let s = path.to_string_lossy();
    if remote::is_remote(path) {
        return s.split_once("://").map_or(&*s, |(_, host)| host).to_owned();
    }
    path.file_name().map_or_else(|| s.to_string(), |n| n.to_string_lossy().into_owned())
}

/// La vista per cartelle: in `here` prima le sottocartelle, poi i volumi; un
/// server (non ha cartelle) le sue serie; all'inizio (`None`) le cartelle
/// della libreria e i server. Per nome l'ordine e' quello del disco; con un
/// altro ordine cambia dentro le cartelle e dentro i volumi.
pub(super) fn folder_sections(
    d: &ShelfData, visible: &[usize], here: Option<&Path>, sort: LibrarySort,
) -> Vec<Section> {
    let order = |tiles: Vec<(Tile, Rank)>| match sort {
        LibrarySort::Name => tiles.into_iter().map(|(t, _)| t).collect(),
        sort => sorted(d, tiles, sort),
    };
    let folder_tile = |path: PathBuf, members: &[usize]| {
        let done = members.iter().filter(|&&j| d.status[j] == Status::Done).count();
        // in copertina il primo da leggere: la cartella si riconosce sempre
        Tile::Folder { name: folder_name(&path), cover: members[0], count: members.len(), done, path }
    };
    let section = |title: &str, tiles: Vec<Tile>| Section {
        title: format!("{title} \u{00b7} {}", tiles.len()),
        tiles,
        one_row: false,
    };
    let within = |place: &Path| -> Vec<usize> {
        visible.iter().copied().filter(|&i| d.entries[i].path.starts_with(place)).collect()
    };
    let tiles: Vec<Tile> = match here {
        None => sources(d)
            .into_iter()
            .filter_map(|source| {
                let mut members = within(&source);
                members.sort_by(|&a, &b| library::folder_cmp(&d.entries[a].path, &d.entries[b].path));
                (!members.is_empty()).then(|| folder_tile(source, &members))
            })
            .collect(),
        Some(server) if remote::is_remote(server) => stacks(d, &within(server), sort),
        Some(folder) => {
            let c = library::folder_contents(d.entries, visible, folder);
            let mut out = Vec::new();
            if !c.folders.is_empty() {
                out.push(section(
                    t("Cartelle", "Folders"),
                    order(
                        c.folders
                            .into_iter()
                            .map(|(path, members)| (folder_tile(path, &members), Rank::of(d, &members)))
                            .collect(),
                    ),
                ));
            }
            if !c.volumes.is_empty() {
                let volumes = c.volumes.into_iter().map(|i| (Tile::Volume(i), Rank::of(d, &[i]))).collect();
                out.push(section(t("Volumi", "Volumes"), order(volumes)));
            }
            return out;
        }
    };
    let title = if here.is_none() { t("Cartelle", "Folders") } else { t("Sul server", "On the server") };
    if tiles.is_empty() { Vec::new() } else { vec![section(title, tiles)] }
}

/// In copertina il volume che si sta leggendo, se c'e'; se no il primo. E
/// quanti se ne sono letti.
pub(super) fn cover_and_done(d: &ShelfData, members: &[usize]) -> (usize, usize) {
    let done = members.iter().filter(|&&j| d.status[j] == Status::Done).count();
    let cover = members.iter().copied().find(|&j| matches!(d.status[j], Status::Reading(..))).unwrap_or(members[0]);
    (cover, done)
}

pub(super) fn in_group(e: &Entry, kind: GroupKind, name: &str) -> bool {
    e.groups.iter().any(|g| g.kind == kind && g.name == name)
}

/// I volumi di un gruppo nel loro ordine: quello della lista di lettura, se
/// c'e'; poi per serie e numero.
pub(super) fn sort_members(d: &ShelfData, members: &mut [usize], kind: GroupKind, name: &str) {
    let place = |i: usize| {
        d.entries[i].groups.iter().find(|g| g.kind == kind && g.name == name).and_then(|g| g.order).unwrap_or(u32::MAX)
    };
    members.sort_by(|&a, &b| {
        let (ea, eb) = (&d.entries[a], &d.entries[b]);
        place(a)
            .cmp(&place(b))
            .then_with(|| natural_cmp(ea.series.as_deref().unwrap_or(""), eb.series.as_deref().unwrap_or("")))
            .then_with(|| by_number(ea, eb))
    });
}

/// Il nome di un tipo di gruppo, al plurale (titolo di sezione) o al singolare.
pub(super) fn kind_label(kind: GroupKind, plural: bool) -> &'static str {
    match (kind, plural) {
        (GroupKind::Collection, true) => t("Raccolte", "Collections"),
        (GroupKind::Collection, false) => t("Raccolta", "Collection"),
        (GroupKind::ReadingList, true) => t("Liste di lettura", "Reading lists"),
        (GroupKind::ReadingList, false) => t("Lista di lettura", "Reading list"),
        (GroupKind::Genre, true) => t("Generi", "Genres"),
        (GroupKind::Genre, false) => t("Genere", "Genre"),
        (GroupKind::Publisher, true) => t("Editori", "Publishers"),
        (GroupKind::Publisher, false) => t("Editore", "Publisher"),
    }
}

pub(super) fn groups_title() -> &'static str {
    t("Raccolte e liste", "Collections and lists")
}

/// La vista dei gruppi: una sezione per tipo, una pila per gruppo. Cercando,
/// un gruppo resta se il suo nome corrisponde (con tutti i volumi) o se ci
/// sono volumi che corrispondono (solo quelli).
pub(super) fn group_sections(
    d: &ShelfData, filter: Filter, needle: &str, found: &dyn Fn(&Entry) -> bool,
) -> Vec<Section> {
    // per tipo (raccolte, liste, generi, editori: l'ordine di `GroupKind`) e per nome
    let mut groups: BTreeMap<(GroupKind, &str), Vec<usize>> = BTreeMap::new();
    for (i, e) in d.entries[..d.library_len()].iter().enumerate() {
        if !filter.keeps(d.status[i]) {
            continue;
        }
        let hit = found(e);
        for g in e.groups.iter().filter(|g| hit || g.name.to_lowercase().contains(needle)) {
            let members = groups.entry((g.kind, g.name.as_str())).or_default();
            if members.last() != Some(&i) {
                members.push(i);
            }
        }
    }
    let mut sections: Vec<(GroupKind, Section)> = Vec::new();
    for ((kind, name), mut members) in groups {
        sort_members(d, &mut members, kind, name);
        let (cover, done) = cover_and_done(d, &members);
        let tile = Tile::Group { kind, name: name.to_owned(), cover, count: members.len(), done };
        match sections.last_mut() {
            Some((k, section)) if *k == kind => section.tiles.push(tile),
            _ => sections.push((kind, Section { title: String::new(), tiles: vec![tile], one_row: false })),
        }
    }
    sections
        .into_iter()
        .map(|(kind, mut section)| {
            section.tiles.sort_by(|a, b| natural_cmp(tile_name(d, a), tile_name(d, b)));
            section.title = format!("{} \u{00b7} {}", kind_label(kind, true), section.tiles.len());
            section
        })
        .collect()
}

/// I volumi trovati cercando sui server, che la libreria non ha: per serie e
/// numero. Mentre si aspetta la risposta, solo il titolo che lo dice.
pub(super) fn server_section(d: &ShelfData, filter: Filter) -> Option<Section> {
    let mut found: Vec<usize> = (d.library_len()..d.entries.len()).filter(|&i| filter.keeps(d.status[i])).collect();
    found.sort_by(|&a, &b| {
        let (ea, eb) = (&d.entries[a], &d.entries[b]);
        natural_cmp(ea.series.as_deref().unwrap_or(""), eb.series.as_deref().unwrap_or(""))
            .then_with(|| by_number(ea, eb))
    });
    let title = match (found.len(), d.searching) {
        (0, false) => return None,
        (0, true) => t("Sul server: cerco\u{2026}", "On the server: searching\u{2026}").to_owned(),
        (n, _) if italian() => format!("Sul server \u{b7} {n}"),
        (n, _) => format!("On the server \u{b7} {n}"),
    };
    Some(Section { title, tiles: found.into_iter().map(Tile::Volume).collect(), one_row: false })
}
