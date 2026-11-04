use super::*;
use crate::progress::Saved;

const KOMGA: &str = "http://casa:25600/opds/v1.2/books/0RV8/pages/{pageNumber}";
const KAVITA: &str =
    "http://casa:5000/api/opds/KEY/image?libraryId=1&seriesId=2&volumeId=5&chapterId=5&pageNumber={pageNumber}";

fn path(template: &str) -> PathBuf {
    Volume { template: template.into(), count: 6, title: "Volume 1".into() }.path()
}

#[test]
fn i_server_contano_a_modo_loro() {
    let komga = Flavor::of(KOMGA);
    let kavita = Flavor::of(KAVITA);
    assert_eq!(komga, Flavor::Komga { book: "0RV8".into() });
    assert_eq!(kavita, Flavor::Kavita { key: "KEY".into(), chapter: "5".into() });
    assert_eq!(Flavor::of("http://altro/pse/{pageNumber}"), Flavor::Other);
    // pagina 3 di 6 (la terza, da 0 e' la 2)
    assert_eq!(komga.value(Place::At(2), 6), 3);
    assert_eq!(kavita.value(Place::At(2), 6), 2);
    for f in [&komga, &kavita] {
        assert_eq!(f.value(Place::Done, 6), 6);
        assert_eq!(f.value(Place::New, 6), 0);
        for p in [Place::New, Place::At(1), Place::At(4), Place::Done] {
            assert_eq!(f.place(f.value(p, 6), 6), p, "{f:?} {p:?}");
        }
    }
    assert_eq!(place_of(5, 6), Place::Done, "l'ultima pagina e' la fine");
}

#[test]
fn le_date_dei_server() {
    assert_eq!(unix_time("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(unix_time("2026-10-07T08:10:56Z"), Some(1_791_360_656));
    assert_eq!(unix_time("2026-10-07T08:10:56.812Z"), Some(1_791_360_656));
    assert_eq!(unix_time("2026-10-07T10:10:56+02:00"), Some(1_791_360_656));
    assert_eq!(unix_time("2026-10-07T08:10:56"), Some(1_791_360_656), "Kavita, senza fuso");
    assert_eq!(unix_time("2024-02-29T00:00:00Z"), Some(1_709_164_800), "l'anno bisestile");
    assert_eq!(unix_time("ieri"), None);
}

/// Senza server: solo la decisione di chi ha ragione. Ogni prova ha i suoi
/// volumi (lo stato e' di tutto il programma).
#[test]
fn chi_ha_letto_dove() {
    let mut progress = Progress::in_memory();
    let now = Instant::now();
    let read = |template: &str, last_read, date| Read { path: path(template), last_read, date };

    // mai visto, niente qui: vale il server (Komga dice 3: la terza pagina)
    let a = "http://casa:25600/opds/v1.2/books/A/pages/{pageNumber}";
    assert!(incoming(&[read(a, 3, Some(100))], now, &mut progress, None));
    assert_eq!(progress.get(&path(a)).map(|s| (s.page, s.pages, s.read_at)), Some((2, 6, 100)));

    // il server dice ancora 3, qui si e' andati avanti: niente da prendere
    progress.set(&path(a), Saved { page: 4, pages: 6, ..Saved::default() });
    assert!(!incoming(&[read(a, 3, Some(100))], Instant::now(), &mut progress, None));
    assert_eq!(progress.get(&path(a)).map(|s| s.page), Some(4));
    with(|s| s.pending.clear());

    // il server dice 6 (finito altrove): vale il suo
    assert!(incoming(&[read(a, 6, Some(200))], Instant::now(), &mut progress, None));
    assert_eq!(progress.get(&path(a)).map(|s| s.page), Some(5));

    // ...ma non per il volume aperto: chi legge e' qui
    let b = "http://casa:25600/opds/v1.2/books/B/pages/{pageNumber}";
    progress.set(&path(b), Saved { page: 1, pages: 6, ..Saved::default() });
    with(|s| s.known.insert(b.into(), 2));
    assert!(!incoming(&[read(b, 5, None)], Instant::now(), &mut progress, Some(&path(b))));
    assert_eq!(progress.get(&path(b)).map(|s| s.page), Some(1));

    // Kavita, rimesso da leggere altrove
    let c = "http://casa:5000/api/opds/KEY/image?chapterId=9&pageNumber={pageNumber}";
    progress.set(&path(c), Saved { page: 3, pages: 6, ..Saved::default() });
    with(|s| s.known.insert(c.into(), 3));
    assert!(incoming(&[read(c, 0, None)], Instant::now(), &mut progress, None));
    assert!(progress.get(&path(c)).is_none());

    // un catalogo letto prima dell'ultima scrittura non vale
    let d = "http://casa:25600/opds/v1.2/books/D/pages/{pageNumber}";
    let before = Instant::now();
    with(|s| {
        s.known.insert(d.into(), 4);
        s.written.insert(d.into(), Instant::now());
    });
    assert!(!incoming(&[read(d, 1, None)], before, &mut progress, None));
    assert!(progress.get(&path(d)).is_none());
}

#[test]
fn l_ultima_pagina_di_kavita() {
    let mut progress = Progress::in_memory();
    let f = "http://casa:5000/api/opds/KEY/image?chapterId=11&pageNumber={pageNumber}";
    // Kavita: all'ultima pagina (5 di 6, da 0), non ancora finito; qui e' letto
    let read = Read { path: path(f), last_read: 5, date: None };
    assert!(incoming(std::slice::from_ref(&read), Instant::now(), &mut progress, None));
    assert!(!incoming(&[read], Instant::now(), &mut progress, None));
    assert_eq!(with(|s| s.pending.remove(f)), None, "nessuna fine scritta al server");
}

#[test]
fn letto_qui_senza_rete_si_manda_dopo() {
    let mut progress = Progress::in_memory();
    let e = "http://casa:25600/opds/v1.2/books/E/pages/{pageNumber}";
    with(|s| s.known.insert(e.into(), 2));
    progress.set(&path(e), Saved { page: 4, pages: 6, ..Saved::default() });
    // il server dice ancora 2: e' rimasto indietro, gli si scrive 5
    assert!(!incoming(&[Read { path: path(e), last_read: 2, date: None }], Instant::now(), &mut progress, None));
    let pending = with(|s| s.pending.remove(e).map(|(_, v)| v));
    assert_eq!(pending, Some(5));
}
