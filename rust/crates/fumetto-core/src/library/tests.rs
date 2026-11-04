use super::scan::{entry, scan_with};
use super::*;

#[test]
fn il_volume_dopo_e_prima() {
    let vol = |path: &str, series: Option<(&str, u32)>, list: Option<u32>| Entry {
        path: PathBuf::from(path),
        title: title_of(Path::new(path)),
        series: series.map(|s| s.0.to_owned()),
        number: series.map(|s| s.1),
        authors: None,
        groups: list.map(|n| Group::new(GroupKind::ReadingList, "Nebbia", Some(n))).into_iter().collect(),
        ..Entry::default()
    };
    let server = "http://casa:25600/opds/v1.2/books/";
    let entries = vec![
        vol("/l/Porto v02.cbz", Some(("Porto", 2)), Some(3)),
        vol("/l/Porto v10.cbz", Some(("Porto", 10)), None),
        vol("/l/Porto v01.cbz", Some(("Porto", 1)), Some(1)),
        vol("/l/Orbita v01.cbz", Some(("Orbita", 1)), Some(2)),
        vol(&format!("{server}a/pages/{{n}}#10:Porto v01"), Some(("Porto", 1)), None),
        vol(&format!("{server}b/pages/{{n}}#10:Porto v02"), Some(("Porto", 2)), None),
    ];
    let roots = [PathBuf::from("/l")];
    let next = |p: &str, order| neighbour(&entries, &roots, Path::new(p), order, true);
    let prev = |p: &str, order| neighbour(&entries, &roots, Path::new(p), order, false);
    let series = Order::Series;
    assert_eq!(next("/l/Porto v01.cbz", series), Some("/l/Porto v02.cbz".into()), "per numero, non per titolo");
    assert_eq!(next("/l/Porto v02.cbz", series), Some("/l/Porto v10.cbz".into()));
    assert_eq!(next("/l/Porto v10.cbz", series), None, "l'ultimo; quelli del server non contano");
    assert_eq!(prev("/l/Porto v10.cbz", series), Some("/l/Porto v02.cbz".into()));
    assert_eq!(prev("/l/Porto v01.cbz", series), None, "il primo");
    assert_eq!(
        next(&entries[4].path.to_string_lossy(), series),
        Some(entries[5].path.clone()),
        "dal server, il server"
    );
    assert_eq!(
        next(&entries[4].path.to_string_lossy(), Order::Folders),
        Some(entries[5].path.clone()),
        "il server non ha cartelle: la serie"
    );
    let list = Order::List("Nebbia");
    assert_eq!(next("/l/Porto v01.cbz", list), Some("/l/Orbita v01.cbz".into()), "la lista di lettura");
    assert_eq!(next("/l/Orbita v01.cbz", list), Some("/l/Porto v02.cbz".into()));
    assert_eq!(prev("/l/Porto v02.cbz", list), Some("/l/Orbita v01.cbz".into()));
    assert_eq!(next("/l/Porto v10.cbz", list), None, "fuori dalla lista: la serie");
    // le cartelle: per nome, non per numero
    assert_eq!(next("/l/Orbita v01.cbz", Order::Folders), Some("/l/Porto v01.cbz".into()));
    assert_eq!(next("/l/Porto v02.cbz", Order::Folders), Some("/l/Porto v10.cbz".into()));

    // un volume fuori dalla libreria: la cartella
    let dir = std::env::temp_dir().join(format!("fumetto-dopo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["cap 2.cbz", "cap 10.cbz", "cap 1.cbz", "note.txt"] {
        std::fs::write(dir.join(name), b"x").unwrap();
    }
    let alone = |p: &str, forward| neighbour(&[], &[], &dir.join(p), Order::Series, forward);
    assert_eq!(alone("cap 2.cbz", true), Some(dir.join("cap 10.cbz")));
    assert_eq!(alone("cap 10.cbz", true), None);
    assert_eq!(alone("cap 2.cbz", false), Some(dir.join("cap 1.cbz")));
    let _ = std::fs::remove_dir_all(dir);
}

/// Un ordine incoerente fa fermare `sort_by` con un panic (Rust 1.81):
/// cosi' NicoReader si chiudeva caricando le cartelle, e di nuovo a ogni
/// avvio riprendendo l'ultimo volume.
fn totale<T: std::fmt::Debug>(items: &[T], cmp: impl Fn(&T, &T) -> Ordering) {
    for a in items {
        assert_eq!(cmp(a, a), Ordering::Equal, "{a:?}");
        for b in items {
            assert_eq!(cmp(a, b), cmp(b, a).reverse(), "{a:?} {b:?}");
            for c in items {
                if cmp(a, b).is_le() && cmp(b, c).is_le() {
                    assert!(cmp(a, c).is_le(), "{a:?} <= {b:?} <= {c:?}, ma non {a:?} <= {c:?}");
                }
            }
        }
    }
}

/// Un generatore di numeri a caso, sempre lo stesso (xorshift): le prove
/// sono ripetibili e un errore si ritrova.
struct Caso(u64);

impl Caso {
    fn n(&mut self, max: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % max as u64) as usize
    }

    fn scegli<'a>(&mut self, v: &[&'a str]) -> &'a str {
        v[self.n(v.len())]
    }
}

/// Nomi difficili: numeri con e senza zeri, capitoli, parentesi, accenti,
/// punti, nomi uguali a meno delle maiuscole.
const NOMI: &[&str] = &[
    "Batman",
    "batman",
    "Batman (2011)",
    "Vol 1",
    "Vol 10",
    "Vol 2",
    "Ch 3",
    "01",
    "1",
    "10",
    "X",
    "Xa",
    "Ça va",
    "v1.5",
    "Nebbia sul Porto",
    "Nebbia sul Porto v03",
    "Saga 01",
    "[Gruppo] Saga",
    "Episode 5",
    "a.b",
    "Ω",
];

/// Un albero di cartelle a caso dentro `dir`: volumi vuoti (la scansione
/// guarda solo i nomi), immagini sciolte, cartelle di immagini che sono
/// volumi e a volte ne contengono altri.
fn albero(caso: &mut Caso, dir: &Path, depth: usize) {
    std::fs::create_dir_all(dir).unwrap();
    let quanti = if depth == 0 { 6 + caso.n(10) } else { 1 + caso.n(6) };
    for _ in 0..quanti {
        let name = caso.scegli(NOMI);
        match caso.n(6) {
            0 | 1 => {
                let _ = std::fs::write(dir.join(format!("{name}.cbz")), b"");
            }
            2 => {
                let _ = std::fs::write(dir.join(format!("{name} {}.cbr", caso.n(30))), b"");
            }
            3 => {
                for k in 0..caso.n(7) {
                    let _ = std::fs::write(dir.join(format!("p{k}.jpg")), b"");
                }
            }
            4 | 5 if depth < 4 => albero(caso, &dir.join(name), depth + 1),
            _ => {}
        }
    }
}

#[test]
fn librerie_a_caso() {
    let base = std::env::temp_dir().join(format!("fumetto-a-caso-{}", std::process::id()));
    let mut caso = Caso(0x9e37_79b9_7f4a_7c15);
    let mut volumi = 0;
    for giro in 0..40 {
        let root = base.join(format!("g{giro}"));
        albero(&mut caso, &root, 0);
        let roots = [root.clone()];
        let found = scan_with(&roots, &mut |_| None);
        let paths: Vec<&Path> = found.iter().map(|e| e.path.as_path()).collect();
        volumi += paths.len();

        // gli ordini sono totali anche qui: ordinare non si ferma
        let mut v = paths.clone();
        v.sort_by(|a, b| folder_cmp(a, b));
        assert!(v.windows(2).all(|w| folder_cmp(w[0], w[1]).is_le()), "giro {giro}");
        let mut e: Vec<&Entry> = found.iter().collect();
        e.sort_by(|a, b| by_number(a, b));

        // ogni cartella: cio' che contiene c'e' una volta sola
        let all: Vec<usize> = (0..found.len()).collect();
        let mut dirs: Vec<&Path> = paths.iter().filter_map(|p| p.parent()).collect();
        dirs.push(&root);
        for dir in dirs {
            let c = folder_contents(&found, &all, dir);
            let inside = c.volumes.len() + c.folders.iter().map(|f| f.1.len()).sum::<usize>();
            assert_eq!(inside, paths.iter().filter(|p| p.starts_with(dir) && **p != dir).count(), "{dir:?}");
        }

        // dopo e prima: chi viene dopo ha come precedente quello di prima
        for order in [Order::Series, Order::Folders] {
            for p in &paths {
                if let Some(next) = neighbour(&found, &roots, p, order, true) {
                    assert!(found.iter().any(|e| e.path == next), "{next:?} non e' nella libreria");
                    if order == Order::Folders {
                        let back = neighbour(&found, &roots, &next, order, false);
                        assert_eq!(back.as_deref(), Some(*p), "giro {giro}: {p:?} -> {next:?}");
                    }
                }
                let _ = neighbour(&found, &roots, p, order, false);
            }
        }
    }
    assert!(volumi > 1000, "librerie troppo piccole per provare qualcosa: {volumi} volumi");
    let _ = std::fs::remove_dir_all(&base);
}
#[test]
fn gli_ordini_sono_totali() {
    // "/f/X" e' una cartella di immagini (un volume) con altri volumi dentro
    let paths: Vec<PathBuf> = [
        "/f/X",
        "/f/X/y.cbz",
        "/f/X/Y/z.cbz",
        "/f/Xa/q.cbz",
        "/f/W/z.cbz",
        "/f/X.cbz",
        "/f/x",
        "/f/a.cbz",
        "/f/01.cbz",
        "/f/1.cbz",
        "/f/10.cbz",
        "/f/2",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    totale(&paths, |a, b| folder_cmp(a, b));

    let vol = |path: &str, title: &str, number: Option<u32>| Entry {
        path: PathBuf::from(path),
        title: title.into(),
        series: Some("Serie".into()),
        number,
        authors: None,
        groups: Vec::new(),
        ..Entry::default()
    };
    let entries = [
        vol("/a", "Z", Some(1)),
        vol("/b", "M", None),
        vol("/c", "A", Some(2)),
        vol("/d", "A", Some(2)),
        vol("/e", "Vol 1 \u{00b7} Ch 2", None),
        vol("/f", "Vol 1 \u{00b7} Ch 10", None),
    ];
    totale(&entries, by_number);

    // e ordinando una libreria grande non ci si ferma
    let mut many: Vec<PathBuf> = (0..3000)
        .map(|i| match i % 3 {
            0 => PathBuf::from(format!("/f/S{}", i % 40)),
            1 => PathBuf::from(format!("/f/S{}/v{}.cbz", i % 40, i)),
            _ => PathBuf::from(format!("/f/S{}a/v{}.cbz", i % 40, i)),
        })
        .collect();
    many.sort_by(|a, b| folder_cmp(a, b));
    assert!(many.windows(2).all(|w| folder_cmp(&w[0], &w[1]).is_le()));
}
#[test]
fn le_cartelle_in_ordine() {
    let p = |s: &str| PathBuf::from(s);
    let mut paths = vec![
        p("/f/Moebius/Arzach.cbz"),
        p("/f/Manga/Berserk/Vol 10/Ch 2.cbz"),
        p("/f/Manga/Berserk/Vol 2/Ch 10.cbz"),
        p("/f/Manga/Berserk/Vol 2/Ch 9.cbz"),
        p("/f/Sciolto.cbz"),
        p("/f/Moebius/Incal/Incal 1.cbz"),
        p("/f/Manga/Akira.cbz"),
    ];
    paths.sort_by(|a, b| folder_cmp(a, b));
    assert_eq!(
        paths,
        [
            p("/f/Manga/Berserk/Vol 2/Ch 9.cbz"),
            p("/f/Manga/Berserk/Vol 2/Ch 10.cbz"),
            p("/f/Manga/Berserk/Vol 10/Ch 2.cbz"),
            p("/f/Manga/Akira.cbz"),
            p("/f/Moebius/Incal/Incal 1.cbz"),
            p("/f/Moebius/Arzach.cbz"),
            p("/f/Sciolto.cbz"),
        ],
        "prima le cartelle, poi i volumi; 2 prima di 10"
    );

    let entries: Vec<Entry> = paths.iter().map(|path| entry(path.clone())).collect();
    let all: Vec<usize> = (0..entries.len()).collect();
    let top = folder_contents(&entries, &all, Path::new("/f"));
    let names: Vec<_> = top.folders.iter().map(|(f, m)| (f.clone(), m.len())).collect();
    assert_eq!(names, [(p("/f/Manga"), 4), (p("/f/Moebius"), 2)]);
    assert_eq!(top.folders[0].1[0], 0, "la copertina: il primo da leggere");
    assert_eq!(top.volumes, [6]);
    let manga = folder_contents(&entries, &all, Path::new("/f/Manga"));
    assert_eq!(manga.folders.len(), 1);
    assert_eq!(manga.volumes, [3]);
    assert_eq!(folder_contents(&entries, &[6], Path::new("/f/Manga")), FolderContents::default());
}

#[test]
fn serie_dai_nomi() {
    for (title, name, n) in [
        ("Nebbia sul Porto v03", "Nebbia sul Porto", Some(3)),
        ("Orbita Bassa #12", "Orbita Bassa", Some(12)),
        ("Lanterne - 07", "Lanterne", Some(7)),
        ("Kaiju Diner vol. 2 (2021)", "Kaiju Diner", Some(2)),
        ("One Piece v101 [Gruppo] (2022)", "One Piece", Some(101)),
        ("Sette Lune 7", "Sette Lune", Some(7)),
        ("Berserk_ch.120", "Berserk", Some(120)),
        ("Kaiju8", "Kaiju8", None),
        ("2001", "2001", None),
        ("Vol.01 Ch.001 - Screw", "Vol.01 Ch.001 - Screw", None),
        ("Acqua Alta", "Acqua Alta", None),
        // le parole di capitolo da sole non sono una serie
        ("Episode 5", "Episode 5", None),
        ("Volume 3", "Volume 3", None),
        ("Tower of God Episode 5", "Tower of God", Some(5)),
        ("Dylan Dog n. 12", "Dylan Dog", Some(12)),
        ("Asterix Tome 3", "Asterix", Some(3)),
        ("Counterpart 2", "Counterpart", Some(2)),
    ] {
        assert_eq!(parse_series(title), (name.to_owned(), n), "{title}");
    }
}

#[test]
fn scansione_archivi_cartelle_e_serie() {
    let root = std::env::temp_dir().join(format!("fumetto-libreria-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mk = |p: &str| {
        let p = root.join(p);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    };
    mk("Nebbia sul Porto v01.cbz");
    mk("Nebbia sul Porto v02.cbz");
    mk("note.txt");
    mk("sciolta.jpg"); // un'immagine nella cartella della libreria: non e' un volume
    mk("Robo/Cap 1 - Vite/001.jpg");
    mk("Robo/Cap 2 - Dado/001.jpg");
    mk("Singolo/pagina.png");
    let found = scan(std::slice::from_ref(&root));
    let got: Vec<(&str, Option<&str>)> = found.iter().map(|e| (e.title.as_str(), e.series.as_deref())).collect();
    assert_eq!(
        got,
        [
            ("Cap 1 - Vite", Some("Robo")),
            ("Cap 2 - Dado", Some("Robo")),
            ("Nebbia sul Porto v01", Some("Nebbia sul Porto")),
            ("Nebbia sul Porto v02", Some("Nebbia sul Porto")),
            ("Singolo", None),
        ]
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn capitoli_e_copertine_nelle_sottocartelle() {
    let root = std::env::temp_dir().join(format!("fumetto-libreria-strutture-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mk = |p: &str| {
        let p = root.join(p);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    };
    // volumi numerati, con la copertina della serie accanto
    mk("Manga/One Piece/One Piece v01.cbz");
    mk("Manga/One Piece/One Piece v02.cbz");
    mk("Manga/One Piece/cover.jpg");
    // webtoon a capitoli, con la copertina nella cartella della serie
    mk("Webtoon/Solo Leveling/cover.jpg");
    mk("Webtoon/Solo Leveling/Chapter 1/001.jpg");
    mk("Webtoon/Solo Leveling/Chapter 2/001.jpg");
    // "Episode" non e' una serie: senza la correzione, i due webtoon finivano insieme
    mk("Webtoon/Tower of God/Episode 1/001.jpg");
    mk("Webtoon/Tower of God/Episode 2/001.jpg");
    mk("Webtoon/Lore Olympus/Episode 1/001.jpg");
    mk("Webtoon/Lore Olympus/Episode 2/001.jpg");
    // capitoli dentro i volumi: la serie e' Berserk, non "Vol 1"
    mk("Manga/Berserk/Vol 1/Ch 1/001.jpg");
    mk("Manga/Berserk/Vol 1/Ch 2/001.jpg");
    mk("Manga/Berserk/Vol 2/Ch 1/001.jpg");
    mk("Manga/Vagabond/Vol 1/Ch 1/001.jpg");
    mk("Manga/Vagabond/Vol 1/Ch 2/001.jpg");
    // tante immagini accanto ai capitoli: sono pagine, la cartella resta un volume
    for p in ["01", "02", "03", "04"] {
        mk(&format!("Artbook/{p}.jpg"));
    }
    mk("Artbook/Extra 1/001.jpg");
    let found = scan(std::slice::from_ref(&root));
    let mut got: Vec<(&str, Option<&str>)> = found.iter().map(|e| (e.title.as_str(), e.series.as_deref())).collect();
    got.sort();
    assert_eq!(
        got,
        [
            ("Artbook", None),
            ("Chapter 1", Some("Solo Leveling")),
            ("Chapter 2", Some("Solo Leveling")),
            ("Episode 1", Some("Lore Olympus")),
            ("Episode 1", Some("Tower of God")),
            ("Episode 2", Some("Lore Olympus")),
            ("Episode 2", Some("Tower of God")),
            ("Extra 1", Some("Extra")), // dal nome, come prima; da solo resta una copertina singola
            ("One Piece v01", Some("One Piece")),
            ("One Piece v02", Some("One Piece")),
            ("Vol 1 \u{00b7} Ch 1", Some("Berserk")),
            ("Vol 1 \u{00b7} Ch 1", Some("Vagabond")),
            ("Vol 1 \u{00b7} Ch 2", Some("Berserk")),
            ("Vol 1 \u{00b7} Ch 2", Some("Vagabond")),
            ("Vol 2 \u{00b7} Ch 1", Some("Berserk")),
        ]
    );
    // nella serie, i capitoli nei volumi in ordine
    let mut berserk: Vec<&Entry> = found.iter().filter(|e| e.series.as_deref() == Some("Berserk")).collect();
    berserk.sort_by(|a, b| by_number(a, b));
    let order: Vec<&str> = berserk.iter().map(|e| e.title.as_str()).collect();
    assert_eq!(order, ["Vol 1 \u{00b7} Ch 1", "Vol 1 \u{00b7} Ch 2", "Vol 2 \u{00b7} Ch 1"]);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn serie_con_lo_stesso_nome_in_cartelle_diverse() {
    let roots = [PathBuf::from("/f")];
    let mut found: Vec<Entry> = [
        "/f/Batman (2011)/Batman v01.cbz",
        "/f/Batman (2011)/Batman v02.cbz",
        "/f/Batman (2016)/Batman v01.cbz",
        "/f/DC/Saga/Saga 01.cbz",
        "/f/Image/Saga/Saga 01.cbz",
        "/f/Dylan Dog/001-100/Dylan Dog 001.cbz",
        "/f/Dylan Dog/101-200/Dylan Dog 101.cbz",
        "/f/Sparsi/Lupo 01.cbz",
        "/f/Altri/Lupo 02.cbz",
    ]
    .iter()
    .map(|p| entry(PathBuf::from(p)))
    .collect();
    split_same_name_series(&mut found, &roots);
    let series: Vec<&str> = found.iter().map(|e| e.series.as_deref().unwrap_or("")).collect();
    assert_eq!(
        series,
        [
            "Batman (2011)",
            "Batman (2011)",
            "Batman (2016)",
            "Saga (DC)",
            "Saga (Image)",
            "Dylan Dog",
            "Dylan Dog",
            "Lupo",
            "Lupo",
        ],
        "una serie per cartella; le sottocartelle di una serie e i volumi sciolti restano insieme"
    );
}

#[test]
fn le_cartelle_degli_autori_sono_raccolte() {
    let root = std::env::temp_dir().join(format!("fumetto-libreria-autori-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mk = |p: &str| {
        let p = root.join(p);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    };
    // un autore: due opere sciolte e una serie in una sua cartella
    mk("Moebius/Arzach.cbz");
    mk("Moebius/Il garage ermetico.cbz");
    mk("Moebius/Incal/Incal v01.cbz");
    mk("Moebius/Incal/Incal v02.cbz");
    // una serie senza numeri: i titoli cominciano con il nome della cartella
    mk("Akira/Akira parte uno.cbz");
    mk("Akira/Akira parte due.cbz");
    // una cartella che ordina: due serie
    mk("Manga/One Piece/One Piece v01.cbz");
    mk("Manga/Berserk/Berserk v01.cbz");
    // un volume da solo nella sua cartella: niente raccolta
    mk("Sciolti/Unico.cbz");
    let found = scan(std::slice::from_ref(&root));
    let get = |title: &str| found.iter().find(|e| e.title == title).unwrap();
    let collections = |title: &str| -> Vec<String> {
        get(title).groups.iter().filter(|g| g.kind == GroupKind::Collection).map(|g| g.name.clone()).collect()
    };
    assert_eq!(get("Arzach").series, None, "opere diverse non sono una serie");
    assert_eq!(get("Il garage ermetico").series, None);
    assert_eq!(collections("Arzach"), ["Moebius"]);
    assert_eq!(collections("Incal v02"), ["Moebius"], "anche la serie dentro la cartella dell'autore");
    assert_eq!(get("Akira parte uno").series.as_deref(), Some("Akira"));
    assert!(collections("Akira parte uno").is_empty(), "la cartella della serie non e' una raccolta");
    assert_eq!(collections("One Piece v01"), ["Manga"]);
    assert_eq!(collections("Berserk v01"), ["Manga"]);
    assert!(collections("Unico").is_empty() && get("Unico").series.is_none());
    let _ = std::fs::remove_dir_all(&root);
}

/// Un CBZ con una pagina finta e, se data, la sua ComicInfo.xml.
fn cbz(path: &Path, info: Option<&str>) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    if let Some(xml) = info {
        zip.start_file("ComicInfo.xml", stored).unwrap();
        std::io::Write::write_all(&mut zip, xml.as_bytes()).unwrap();
    }
    zip.start_file("001.jpg", stored).unwrap();
    std::io::Write::write_all(&mut zip, b"x").unwrap();
    zip.finish().unwrap();
}

fn scheda(series: &str, number: &str, extra: &str) -> String {
    format!("<ComicInfo><Series>{series}</Series><Number>{number}</Number>{extra}</ComicInfo>")
}

#[test]
fn la_comicinfo_vince_sul_nome() {
    let root = std::env::temp_dir().join(format!("fumetto-libreria-scheda-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    // nomi che da soli non direbbero niente, o direbbero altro
    cbz(
        &root.join("scaricati/op_c001_hq [scan].cbz"),
        Some(&scheda("One Piece", "1", "<Title>Romance Dawn</Title><Writer>Eiichiro Oda</Writer>")),
    );
    cbz(&root.join("scaricati/zzz.cbz"), Some(&scheda("One Piece", "2", "")));
    cbz(&root.join("Nebbia sul Porto v01.cbz"), None);
    cbz(&root.join("Nebbia sul Porto v02.cbz"), Some("<ComicInfo><Summary>niente</Summary></ComicInfo>"));
    // un capitolo scaricato da Mihon: una cartella con la sua scheda
    let chapter = root.join("Mihon/Solo Leveling/Capitolo dodici");
    std::fs::create_dir_all(&chapter).unwrap();
    std::fs::write(chapter.join("001.jpg"), b"x").unwrap();
    std::fs::write(chapter.join("ComicInfo.xml"), scheda("Solo Leveling", "12", "<Title>Chapter 12</Title>")).unwrap();

    let found = scan(std::slice::from_ref(&root));
    let got: Vec<_> = found.iter().map(|e| (e.title.as_str(), e.series.as_deref(), e.number)).collect();
    assert_eq!(
        got,
        [
            ("Nebbia sul Porto v01", Some("Nebbia sul Porto"), Some(1)),
            ("Nebbia sul Porto v02", Some("Nebbia sul Porto"), Some(2)),
            ("One Piece 1: Romance Dawn", Some("One Piece"), Some(1)),
            ("One Piece 2", Some("One Piece"), Some(2)),
            ("Solo Leveling: Chapter 12", Some("Solo Leveling"), Some(12)),
        ]
    );
    assert_eq!(found[2].authors.as_deref(), Some("Eiichiro Oda"));

    // la cache: la seconda volta le schede vengono da li'
    let cache = root.join("dati/comicinfo.json");
    assert_eq!(scan_cached(std::slice::from_ref(&root), &cache), found);
    let json = std::fs::read_to_string(&cache).unwrap();
    assert!(json.contains("Romance Dawn") && !json.contains("Mihon"), "solo gli archivi: {json}");
    std::fs::write(&cache, json.replace("Romance Dawn", "Dalla cache")).unwrap();
    let again = scan_cached(std::slice::from_ref(&root), &cache);
    assert_eq!(again[2].title, "One Piece 1: Dalla cache");

    // un volume cambiato si rilegge, uno sparito esce dalla cache
    cbz(&root.join("scaricati/op_c001_hq [scan].cbz"), Some(&scheda("One Piece", "1", "<Title>Nuova</Title>")));
    std::fs::remove_file(root.join("scaricati/zzz.cbz")).unwrap();
    let after = scan_cached(std::slice::from_ref(&root), &cache);
    assert!(after.iter().any(|e| e.title == "One Piece 1: Nuova"));
    let json = std::fs::read_to_string(&cache).unwrap();
    assert!(!json.contains("zzz") && !json.contains("Dalla cache"), "{json}");
    let _ = std::fs::remove_dir_all(&root);
}
