use super::*;

fn entry(title: &str, series: Option<(&str, u32)>) -> Entry {
    Entry {
        path: PathBuf::from(format!("{title}.cbz")),
        title: title.to_owned(),
        series: series.map(|s| s.0.to_owned()),
        number: series.map(|s| s.1),
        authors: None,
        groups: Vec::new(),
        ..Entry::default()
    }
}

fn data<'a>(entries: &'a [Entry], status: &'a [Status], read_at: &'a [u64]) -> ShelfData<'a> {
    ShelfData {
        entries,
        found_from: entries.len(),
        searching: false,
        status,
        read_at,
        offline: &[],
        edited: &[],
        scanning: false,
        roots: &[],
        cover_shown: &|_| 1.0,
        reading: false,
    }
}

/// Numeri a caso ripetibili (xorshift).
struct Caso(u64);

impl Caso {
    fn n(&mut self, max: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % max as u64) as usize
    }
}

/// Una libreria a caso: volumi in cartelle annidate (anche cartelle che
/// sono volumi e ne contengono altri), serie con e senza numero, volumi
/// dei server, raccolte e liste, stati, date e anni.
fn libreria_a_caso(caso: &mut Caso) -> (Vec<Entry>, Vec<Status>, Vec<u64>) {
    const PEZZI: &[&str] = &["Manga", "Berserk", "Vol 1", "Vol 10", "X", "Xa", "Ça", "01", "1", "a.b"];
    const SERIE: &[&str] = &["Berserk", "Batman", "Batman (2011)", "Saga"];
    let mut entries = Vec::new();
    for k in 0..20 + caso.n(120) {
        let depth = caso.n(4);
        let mut path = if caso.n(10) == 0 {
            PathBuf::from(format!("http://casa:25600/opds/v1.2/books/{k}/pages/{{n}}#5:Volume {k}"))
        } else {
            let mut p = PathBuf::from(if caso.n(4) == 0 { "/g" } else { "/f" });
            for _ in 0..depth {
                p.push(PEZZI[caso.n(PEZZI.len())]);
            }
            p
        };
        if !remote::is_remote(&path) && caso.n(5) != 0 {
            path.push(format!("{} {k}.cbz", PEZZI[caso.n(PEZZI.len())]));
        }
        let series = (caso.n(3) != 0).then(|| SERIE[caso.n(SERIE.len())].to_owned());
        let mut groups = Vec::new();
        if caso.n(4) == 0 {
            groups.push(library::Group::new(GroupKind::Collection, "Raccolta", None));
        }
        if caso.n(5) == 0 {
            groups.push(library::Group::new(GroupKind::ReadingList, "Lista", Some(caso.n(5) as u32)));
        }
        entries.push(Entry {
            title: fumetto_core::title_of(&path),
            path,
            number: series.as_ref().and_then(|_| (caso.n(3) != 0).then(|| caso.n(20) as u32)),
            series,
            groups,
            added: caso.n(5) as u64,
            year: (caso.n(2) == 0).then(|| 1980 + caso.n(5) as u32),
            ..Entry::default()
        });
    }
    let status = (0..entries.len())
        .map(|_| match caso.n(3) {
            0 => Status::New,
            1 => Status::Done,
            _ => Status::Reading(caso.n(10), 10),
        })
        .collect();
    let read_at = (0..entries.len()).map(|_| caso.n(4) as u64).collect();
    (entries, status, read_at)
}

/// Guarda la libreria come la vede chi la usa: le sezioni, le copertine,
/// il disegno, il mouse sopra; poi entra nelle celle (serie, cartelle,
/// gruppi) e torna indietro, fino a `depth` livelli.
fn esplora(shelf: &mut Shelf, d: &ShelfData, depth: usize) {
    let view = (1280.0, 800.0);
    let _ = shelf.sections(d);
    let _ = shelf.cover_slots(d, view, 1.0);
    let _ = shelf.layers(d, view, 1.0, &mut fumetto_render::Estimate);
    let _ = shelf.cover_slots(d, (420.0, 300.0), 2.0);
    for (x, y) in [(60.0, 100.0), (200.0, 300.0), (60.0, 30.0), (1250.0, 26.0)] {
        shelf.motion(d, view, 1.0, x, y);
    }
    if depth == 0 {
        return;
    }
    let tiles = Geo::new(view, 1.0);
    let n = shelf.layout(d, &tiles).placed.len();
    for k in 0..n.min(4) {
        let tile = shelf.layout(d, &tiles).placed.into_iter().nth(k).map(|p| p.tile);
        if let Some(tile) = tile {
            let _ = Shelf::tile_menu(d, &tile);
            if !matches!(tile, Tile::Volume(_)) {
                let _ = shelf.choose(d, tile);
                esplora(shelf, d, depth - 1);
                shelf.key(Key::Backspace, d, view, 1.0);
            }
        }
    }
    for key in [Key::Right, Key::Down, Key::End, Key::Home, Key::PageDown, Key::Escape] {
        shelf.key(key, d, view, 1.0);
    }
}

#[test]
fn librerie_a_caso() {
    let mut caso = Caso(0x2545_f491_4f6c_dd1d);
    let roots = [PathBuf::from("/f"), PathBuf::from("/g")];
    for _ in 0..15 {
        let (entries, status, read_at) = libreria_a_caso(&mut caso);
        let d = ShelfData { roots: &roots, ..data(&entries, &status, &read_at) };
        for folders in [false, true] {
            for groups in [false, true] {
                for filter in Filter::ALL {
                    for sort in LibrarySort::ALL {
                        for search in ["", "ba", "zz"] {
                            let mut shelf =
                                Shelf { folders, groups, filter, sort, search: search.into(), ..Shelf::default() };
                            esplora(&mut shelf, &d, 3);
                        }
                    }
                }
            }
        }
    }
}
fn library() -> (Vec<Entry>, Vec<Status>, Vec<u64>) {
    let entries = vec![
        entry("Acqua Alta", None),
        entry("Nebbia sul Porto v01", Some(("Nebbia sul Porto", 1))),
        entry("Nebbia sul Porto v02", Some(("Nebbia sul Porto", 2))),
        entry("Orbita Bassa #12", Some(("Orbita Bassa", 12))),
    ];
    let status = vec![Status::New, Status::Done, Status::Reading(10, 200), Status::Reading(3, 40)];
    (entries, status, vec![0, 5, 20, 10])
}

#[test]
fn la_libreria_per_cartelle() {
    let at = |path: &str, series: Option<(&str, u32)>| Entry {
        path: PathBuf::from(path),
        title: fumetto_core::title_of(Path::new(path)),
        series: series.map(|s| s.0.to_owned()),
        number: series.map(|s| s.1),
        authors: None,
        groups: Vec::new(),
        ..Entry::default()
    };
    let server = "http://casa:25600/opds/v1.2/books";
    let e = vec![
        at("/f/Manga/Berserk/Vol 2/Ch 10.cbz", None),
        at("/f/Manga/Berserk/Vol 2/Ch 9.cbz", None),
        at("/f/Manga/Akira.cbz", None),
        at("/f/Moebius/Arzach.cbz", None),
        at("/f/Sciolto.cbz", None),
        at(&format!("{server}/a/pages/{{n}}#3:Porto v01"), Some(("Porto", 1))),
        at(&format!("{server}/b/pages/{{n}}#3:Porto v02"), Some(("Porto", 2))),
    ];
    let st = vec![Status::New; e.len()];
    let read_at = vec![0; e.len()];
    let roots = [PathBuf::from("/f")];
    let d = ShelfData { roots: &roots, ..data(&e, &st, &read_at) };
    let folder = |path: &str, cover: usize, count: usize| Tile::Folder {
        path: PathBuf::from(path),
        name: folder_name(Path::new(path)),
        cover,
        count,
        done: 0,
    };
    let tiles = |shelf: &Shelf| -> Vec<Vec<Tile>> { shelf.sections(&d).into_iter().map(|s| s.tiles).collect() };
    let mut shelf = Shelf { folders: true, ..Shelf::default() };

    // una cartella e un server: si parte da loro
    assert_eq!(tiles(&shelf), [vec![folder("/f", 1, 5), folder("http://casa:25600", 5, 2)]]);
    assert!(!shelf.inside(&d));
    shelf.enter_folder(PathBuf::from("/f"));
    assert_eq!(
        tiles(&shelf),
        [vec![folder("/f/Manga", 1, 3), folder("/f/Moebius", 3, 1)], vec![Tile::Volume(4)]],
        "prima le cartelle, poi i volumi; in copertina il primo da leggere"
    );
    shelf.enter_folder(PathBuf::from("/f/Manga"));
    assert_eq!(tiles(&shelf), [vec![folder("/f/Manga/Berserk", 1, 2)], vec![Tile::Volume(2)]]);
    assert_eq!(
        shelf.trail(&d),
        [(t("Cartelle", "Folders").to_owned(), None), ("f".to_owned(), Some(PathBuf::from("/f")))]
    );
    shelf.enter_folder(PathBuf::from("/f/Manga/Berserk/Vol 2"));
    assert_eq!(tiles(&shelf), [vec![Tile::Volume(1), Tile::Volume(0)]], "9 prima di 10");
    let view = (1920.0, 1080.0);
    shelf.key(Key::Backspace, &d, view, 1.0);
    assert_eq!(shelf.folder, Some(PathBuf::from("/f/Manga/Berserk")), "indietro: la cartella sopra");
    shelf.key(Key::Escape, &d, view, 1.0);
    shelf.key(Key::Escape, &d, view, 1.0);
    shelf.key(Key::Escape, &d, view, 1.0);
    assert_eq!(shelf.folder, None, "fino all'inizio");

    // un server non ha cartelle: le sue serie, e dalla serie si torna li'
    shelf.enter_folder(PathBuf::from("http://casa:25600"));
    assert_eq!(tiles(&shelf), [vec![Tile::Series { name: "Porto".into(), cover: 5, count: 2, done: 0 }]]);
    shelf.enter_series("Porto".into());
    shelf.key(Key::Escape, &d, view, 1.0);
    assert_eq!((shelf.series.as_deref(), shelf.folder.as_deref()), (None, Some(Path::new("http://casa:25600"))));

    // i filtri valgono dentro le cartelle; cercando, un elenco solo
    shelf.enter_folder(PathBuf::from("/f/Manga"));
    shelf.filter = Filter::Done;
    assert!(tiles(&shelf).is_empty());
    shelf.filter = Filter::All;
    shelf.search = "arz".into();
    assert_eq!(tiles(&shelf), [vec![Tile::Volume(3)]]);
    shelf.search.clear();

    // con una cartella sola si parte da dentro, e il percorso comincia da lei
    let local = &e[..5];
    let d = ShelfData { roots: &roots, ..data(local, &st[..5], &read_at[..5]) };
    let mut shelf = Shelf { folders: true, ..Shelf::default() };
    assert_eq!(shelf.sections(&d).len(), 2, "le cartelle e i volumi di /f");
    assert!(!shelf.inside(&d));
    shelf.enter_folder(PathBuf::from("/f/Manga"));
    assert_eq!(shelf.trail(&d), [("f".to_owned(), None)]);
    shelf.key(Key::Escape, &d, view, 1.0);
    assert_eq!(shelf.folder, None);
}
#[test]
fn continua_a_leggere_poi_la_libreria_con_le_serie() {
    let (e, st, at) = library();
    let d = data(&e, &st, &at);
    let secs = Shelf::default().sections(&d);
    assert_eq!(secs.len(), 2);
    assert_eq!(secs[0].tiles, [Tile::Volume(2), Tile::Volume(3)], "in lettura, dal piu' recente");
    assert_eq!(
        secs[1].tiles,
        [
            Tile::Volume(0),
            Tile::Series { name: "Nebbia sul Porto".into(), cover: 2, count: 2, done: 1 },
            Tile::Volume(3), // una serie di un volume solo resta un volume
        ]
    );
}

#[test]
fn l_ordine_della_libreria() {
    let (mut e, st, at) = library();
    for (entry, (added, year)) in
        e.iter_mut().zip([(300, None), (100, Some(1991)), (200, Some(1990)), (50, Some(1985))])
    {
        (entry.added, entry.year) = (added, year);
    }
    let d = data(&e, &st, &at);
    let nebbia = Tile::Series { name: "Nebbia sul Porto".into(), cover: 2, count: 2, done: 1 };
    let whole = |sort| {
        let shelf = Shelf { sort, ..Shelf::default() };
        shelf.sections(&d).pop().unwrap().tiles
    };
    assert_eq!(whole(LibrarySort::Name), [Tile::Volume(0), nebbia.clone(), Tile::Volume(3)]);
    assert_eq!(whole(LibrarySort::Read), [nebbia.clone(), Tile::Volume(3), Tile::Volume(0)], "letti: 20, 10, mai");
    assert_eq!(whole(LibrarySort::Added), [Tile::Volume(0), nebbia.clone(), Tile::Volume(3)], "300, 200, 50");
    assert_eq!(whole(LibrarySort::Year), [Tile::Volume(3), nebbia, Tile::Volume(0)], "1985, 1990, senza anno");
    let menu = Shelf::sort_menu(LibrarySort::Read);
    let on: Vec<bool> =
        menu.iter().filter_map(|r| if let ui::Row::Item { on, .. } = r { Some(*on) } else { None }).collect();
    assert_eq!(on, [false, false, true, false]);
}
#[test]
fn cercare_filtrare_e_aprire_una_serie() {
    let (e, st, at) = library();
    let d = data(&e, &st, &at);
    let mut shelf = Shelf::default();
    let view = (1920.0, 1080.0);
    for c in "nebbia".chars() {
        shelf.key(Key::Char(c), &d, view, 1.0);
    }
    assert_eq!(shelf.sections(&d)[0].tiles, [Tile::Volume(1), Tile::Volume(2)], "cercando non si raggruppa");
    shelf.key(Key::Escape, &d, view, 1.0);
    assert!(shelf.search.is_empty());
    shelf.filter = Filter::Done;
    assert_eq!(shelf.sections(&d)[0].tiles, [Tile::Volume(1)]);
    shelf.filter = Filter::All;
    shelf.enter_series("Nebbia sul Porto".into());
    assert_eq!(shelf.sections(&d)[0].tiles, [Tile::Volume(1), Tile::Volume(2)], "in ordine di numero");
    shelf.key(Key::Escape, &d, view, 1.0);
    assert!(shelf.series.is_none(), "Esc esce dalla serie");
}

#[test]
fn raccolte_liste_e_generi() {
    use fumetto_core::library::Group;
    let (mut e, st, at) = library();
    let arc = |n| Group::new(GroupKind::ReadingList, "La grande nebbia", Some(n));
    let porto = Group::new(GroupKind::Collection, "Classici del porto", None);
    let giallo = Group::new(GroupKind::Genre, "Giallo", None);
    e[1].groups = vec![porto.clone(), arc(3), giallo.clone()];
    e[2].groups = vec![porto.clone(), giallo.clone()];
    e[3].groups = vec![arc(1)];
    e[0].groups = vec![Group::new(GroupKind::Genre, "Avventura", None), giallo];
    let d = data(&e, &st, &at);
    let view = (1920.0, 1080.0);
    let mut shelf = Shelf::default();
    assert!(BarGeo::new(&d, view.0, 1.0, false).groups_x.is_some(), "c'e' il tasto dei gruppi");
    shelf.groups = true;
    let secs = shelf.sections(&d);
    let titles: Vec<&str> = secs.iter().map(|s| s.title.as_str()).collect();
    let head = |kind, n| format!("{} \u{b7} {n}", kind_label(kind, true));
    assert_eq!(titles, [head(GroupKind::Collection, 1), head(GroupKind::ReadingList, 1), head(GroupKind::Genre, 2)]);
    assert_eq!(
        secs[1].tiles,
        [Tile::Group {
            kind: GroupKind::ReadingList,
            name: "La grande nebbia".into(),
            cover: 3, // in lettura
            count: 2,
            done: 1
        }]
    );
    assert!(matches!(&secs[2].tiles[1], Tile::Group { name, count: 3, .. } if name == "Giallo"));
    // nella lista i volumi vanno nel suo ordine, non per serie
    shelf.enter_group(GroupKind::ReadingList, "La grande nebbia".into());
    assert_eq!(shelf.sections(&d)[0].tiles, [Tile::Volume(3), Tile::Volume(1)]);
    // nella raccolta le serie restano pile
    shelf.enter_group(GroupKind::Collection, "Classici del porto".into());
    assert!(matches!(&shelf.sections(&d)[0].tiles[..], [Tile::Series { count: 2, .. }]));
    shelf.enter_series("Nebbia sul Porto".into());
    shelf.key(Key::Escape, &d, view, 1.0);
    assert_eq!(shelf.group.as_ref().map(|g| g.1.as_str()), Some("Classici del porto"), "dalla serie al gruppo");
    shelf.key(Key::Escape, &d, view, 1.0);
    assert!(shelf.groups && shelf.group.is_none(), "dal gruppo all'elenco");
    // cercando: il nome del gruppo, o i volumi dentro
    for c in "orbita".chars() {
        shelf.key(Key::Char(c), &d, view, 1.0);
    }
    assert_eq!(shelf.sections(&d).len(), 1);
    assert!(matches!(&shelf.sections(&d)[0].tiles[..], [Tile::Group { count: 1, .. }]));
    shelf.search = "avv".into();
    assert!(matches!(&shelf.sections(&d)[0].tiles[..], [Tile::Group { count: 1, .. }]));
    shelf.search.clear();
    shelf.key(Key::Escape, &d, view, 1.0);
    assert!(!shelf.groups, "dall'elenco alla libreria");
}

#[test]
fn trovati_sul_server() {
    let (mut e, mut st, mut at) = library();
    // l'ultimo l'ha trovato solo la ricerca sul server
    e.push(entry("Mare Aperto 7", Some(("Mare Aperto", 7))));
    st.push(Status::New);
    at.push(0);
    let d = ShelfData { found_from: 4, ..data(&e, &st, &at) };
    let view = (1920.0, 1080.0);
    let mut shelf = Shelf::default();
    let all = shelf.sections(&d);
    assert!(all.iter().all(|s| !s.tiles.contains(&Tile::Volume(4))), "nella libreria non c'e'");
    assert_eq!(
        all.last().unwrap().title,
        if italian() { "Tutta la libreria \u{b7} 4" } else { "The whole library \u{b7} 4" }
    );
    for c in "mare".chars() {
        shelf.key(Key::Char(c), &d, view, 1.0);
    }
    let secs = shelf.sections(&d);
    assert_eq!(secs.len(), 1, "in libreria niente: niente sezione vuota");
    assert_eq!(secs[0].tiles, [Tile::Volume(4)], "sul server si'");
    shelf.key(Key::Right, &d, view, 1.0);
    assert_eq!(
        shelf.key(Key::Enter, &d, view, 1.0),
        Handled::Yes(Some(Command::Open("Mare Aperto 7.cbz".into()))),
        "e si apre"
    );
    // qui "Mare Aperto 7" e' della libreria: mentre si aspetta il
    // server, la sua sezione lo dice, sotto i trovati
    let waiting = ShelfData { found_from: 5, searching: true, ..data(&e, &st, &at) };
    let secs = shelf.sections(&waiting);
    assert_eq!(secs.len(), 2);
    assert_eq!(secs[0].tiles, [Tile::Volume(4)]);
    assert!(secs[1].tiles.is_empty() && !secs[1].title.is_empty());
    let answered = shelf.sections(&ShelfData { found_from: 5, ..data(&e, &st, &at) });
    assert_eq!(answered.len(), 1, "il server non ha altro: niente sezione");
    // con dei risultati in libreria, quelli del server vengono dopo
    shelf.search = "a".into();
    let both = shelf.sections(&d);
    assert_eq!(both.len(), 2);
    assert!(!both[0].tiles.is_empty() && both[1].tiles == [Tile::Volume(4)]);
}

#[test]
fn con_le_frecce_e_invio_si_apre_un_volume() {
    let (e, st, at) = library();
    let d = data(&e, &st, &at);
    let mut shelf = Shelf::default();
    let view = (1920.0, 1080.0);
    shelf.key(Key::Right, &d, view, 1.0); // la prima cella: Nebbia v02, in lettura
    shelf.key(Key::Right, &d, view, 1.0); // Orbita Bassa
    assert_eq!(shelf.key(Key::Enter, &d, view, 1.0), Handled::Yes(Some(Command::Open("Orbita Bassa #12.cbz".into()))));
    shelf.key(Key::Down, &d, view, 1.0); // sotto: la pila di Nebbia sul Porto
    assert_eq!(shelf.key(Key::Enter, &d, view, 1.0), Handled::Yes(None), "la serie si apre, non si legge");
    assert_eq!(shelf.series.as_deref(), Some("Nebbia sul Porto"));
}

/// Quanto costa disporre una libreria grande (cargo test --release -- --ignored misura).
#[test]
#[ignore]
fn misura_la_disposizione() {
    let mut caso = Caso(7);
    let mut entries = Vec::new();
    for i in 0..5000 {
        let series = format!("Serie {}", caso.n(700));
        let mut e = entry(&format!("{series} v{i}"), Some((series.as_str(), caso.n(60) as u32)));
        e.path = PathBuf::from(format!("/fumetti/{}/{series}/{i}.cbz", caso.n(30)));
        e.added = caso.n(1_000_000) as u64;
        entries.push(e);
    }
    let status: Vec<Status> =
        (0..entries.len()).map(|i| if i % 7 == 0 { Status::Reading(1, 10) } else { Status::New }).collect();
    let read_at: Vec<u64> = (0..entries.len() as u64).collect();
    let roots = [PathBuf::from("/fumetti")];
    let d = ShelfData { roots: &roots, ..data(&entries, &status, &read_at) };
    for (name, shelf) in [
        ("serie", Shelf::default()),
        ("aggiunti", Shelf { sort: LibrarySort::Added, ..Shelf::default() }),
        ("cartelle", Shelf { folders: true, ..Shelf::default() }),
        ("cerca", Shelf { search: "serie 1".into(), ..Shelf::default() }),
    ] {
        let t = Instant::now();
        let n = 20;
        for _ in 0..n {
            std::hint::black_box(shelf.cover_slots(&d, (1920.0, 1080.0), 1.0));
        }
        println!("{name}: {:.2} ms per disposizione", t.elapsed().as_secs_f64() * 1000.0 / n as f64);
    }
}

#[test]
fn la_cella_sotto_il_mouse_sale_e_torna_giu() {
    let entries = [entry("A", None), entry("B", None)];
    let (status, read_at) = ([Status::New; 2], [0; 2]);
    let d = data(&entries, &status, &read_at);
    let view = (1280.0, 800.0);
    let mut shelf = Shelf::default();
    let (x, y) = shelf.layout(&d, &Geo::new(view, 1.0)).placed.first().map(|p| (p.x, p.y)).unwrap();
    let rest = shelf.cover_slots(&d, view, 1.0)[0].y;
    shelf.motion(&d, view, 1.0, x + 10.0, y + 10.0);
    let t0 = Instant::now();
    assert!(shelf.advance(&d, view, 1.0, t0), "comincia a salire");
    assert!(shelf.moving());
    let ms = |n| t0 + std::time::Duration::from_millis(n);
    shelf.advance(&d, view, 1.0, ms(60));
    let half = shelf.cover_slots(&d, view, 1.0)[0].y;
    assert!(half < rest && half > rest - input::LIFT, "a meta' strada: {half}");
    shelf.advance(&d, view, 1.0, ms(400));
    assert_eq!(shelf.cover_slots(&d, view, 1.0)[0].y, rest - input::LIFT, "su del tutto");
    assert!(!shelf.moving(), "poi si ferma");
    // il mouse va via: torna giu'
    shelf.motion(&d, view, 1.0, 5.0, 790.0);
    shelf.advance(&d, view, 1.0, ms(410));
    assert!(shelf.moving());
    shelf.advance(&d, view, 1.0, ms(800));
    assert_eq!(shelf.cover_slots(&d, view, 1.0)[0].y, rest, "di nuovo al suo posto");
    assert!(!shelf.moving());
}

#[test]
fn la_barra_non_si_sovrappone_a_nessuna_larghezza() {
    use fumetto_core::library::Group;
    let (mut e, st, at) = library();
    e[0].groups = vec![Group::new(GroupKind::Genre, "Giallo", None)];
    for reading in [false, true] {
        let d = ShelfData { reading, ..data(&e, &st, &at) };
        for s in [1.0, 1.25, 1.5, 2.0] {
            for vw in (300..2400).step_by(10).map(|w| w as f32) {
                for searching in [false, true] {
                    let g = BarGeo::new(&d, vw, s, searching);
                    let what = format!("larga {vw}, scala {s}, cerca {searching}, legge {reading}");
                    let first_button = g.buttons().map(|b| b.2).fold(f32::MAX, f32::min);
                    if first_button < 24.0 * s {
                        continue; // neanche i tasti ci stanno: niente da disporre
                    }
                    // da sinistra: il nome, i filtri, la ricerca, i tasti
                    let mut x = if g.logo { 150.0 * s } else { 24.0 * s };
                    if let (Some(&t0), Some(&t3)) = (g.tabs.first(), g.tabs.last()) {
                        assert!(t0 >= x - 0.5, "filtri sopra il nome: {what}");
                        assert!(g.tab_w >= 72.0 * s, "filtri troppo stretti: {what}");
                        x = t3 + g.tab_w + 16.0 * s;
                    }
                    assert!(g.search_w == 0.0 || g.search_x >= x - 1.0, "ricerca sopra i filtri: {what}");
                    assert!(g.search_x + g.search_w <= first_button + 0.5, "ricerca sopra i tasti: {what}");
                    if searching {
                        assert!(
                            g.search_w >= 120.0 * s - 1.0 || vw < 500.0 * s,
                            "il testo cercato non ha posto: {what}"
                        );
                    }
                }
            }
        }
    }
    // larga: tutto come prima, i filtri con i numeri e il nome
    let d = data(&e, &st, &at);
    let g = BarGeo::new(&d, 1920.0, 1.0, false);
    assert!(g.logo && g.counts && !g.search_icon && g.tabs.len() == 4);
    // stretta: il nome va via prima dei numeri, la lente per ultima
    let g = BarGeo::new(&d, 800.0, 1.0, false);
    assert!(!g.logo && g.counts && !g.search_icon, "a 800 punti restano i numeri");
    let g = BarGeo::new(&d, 600.0, 1.0, false);
    assert!(!g.counts && g.search_icon && g.tabs.len() == 4, "a 600 punti, la lente");
    let g = BarGeo::new(&d, 600.0, 1.0, true);
    assert!(g.tabs.is_empty() && g.search_w > 200.0, "mentre si cerca, il testo al posto dei filtri");
}
