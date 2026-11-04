use super::crawl::{clean_title, remote_entry};
use super::feed::FeedEntry;
use super::http::{explain, quoted};
use super::search::{fill_search, opensearch_template};
use super::*;

/// Una serie di Komga, com'e' (accorciata a un volume).
const KOMGA: &str = r#"<feed xmlns="http://www.w3.org/2005/Atom"><id>0RV8BZTHNG14F</id><title>Nebbia sul Porto</title><link type="application/atom+xml;profile=opds-catalog;kind=navigation" rel="self" href="http://casa:25600/opds/v1.2/series/0RV8BZTHNG14F"/><link type="application/atom+xml;profile=opds-catalog;kind=navigation" rel="next" href="http://casa:25600/opds/v1.2/series/0RV8BZTHNG14F?page=1"/><entry><title>Volume 1</title><updated>2026-10-06T19:00:43.812Z</updated><id>0RV8BZTHSG7VG</id><content>cbz - 136.8 KiB</content><author><name>Prova</name></author><link type="image/jpeg" rel="http://opds-spec.org/image/thumbnail" href="http://casa:25600/opds/v1.2/books/0RV8BZTHSG7VG/thumbnail/small"/><link type="image/jpeg" rel="http://opds-spec.org/image" href="http://casa:25600/opds/v1.2/books/0RV8BZTHSG7VG/thumbnail"/><link type="application/zip" rel="http://opds-spec.org/acquisition" href="http://casa:25600/opds/v1.2/books/0RV8BZTHSG7VG/file/Nebbia%20sul%20Porto%20v01.cbz"/><link href="http://casa:25600/opds/v1.2/books/0RV8BZTHSG7VG/pages/{pageNumber}" xmlns:pse="http://vaemendis.net/opds-pse/ns" pse:count="6" type="image/jpeg" rel="http://vaemendis.net/opds-pse/stream"/></entry></feed>"#;

/// Una serie di Kavita: indirizzi relativi, &amp; negli attributi, il
/// segno di lettura davanti al titolo, il volume ripetuto in cima.
const KAVITA: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns="http://www.w3.org/2005/Atom">
  <id>series-1</id>
  <title>Nebbia sul Porto - Storyline</title>
  <link rel="self" type="application/atom+xml;profile=opds-catalog;kind=acquisition" href="/api/opds/KEY/series/1" />
  <entry>
    <id>2</id>
    <title>Continue Reading from: &#x2B58; Nebbia sul Porto - Volume 2</title>
    <link rel="http://vaemendis.net/opds-pse/stream" type="image/jpeg" href="/api/opds/KEY/image?libraryId=1&amp;seriesId=1&amp;volumeId=2&amp;chapterId=2&amp;pageNumber={pageNumber}" p5:count="6" xmlns:p5="http://vaemendis.net/opds-pse/ns" />
  </entry>
  <entry>
    <id>1</id>
    <title>⬤ Nebbia sul Porto - Volume 1</title>
    <link rel="http://opds-spec.org/image" type="image/jpeg" href="/api/image/chapter-cover?chapterId=1&amp;apiKey=KEY" />
    <link rel="http://opds-spec.org/acquisition/open-access" type="application/x-cbz" href="/api/opds/KEY/series/1/volume/1/chapter/1/download/x.cbz" p5:count="6" xmlns:p5="http://vaemendis.net/opds-pse/ns" />
    <link rel="http://vaemendis.net/opds-pse/stream" type="image/jpeg" href="/api/opds/KEY/image?libraryId=1&amp;seriesId=1&amp;volumeId=1&amp;chapterId=1&amp;pageNumber={pageNumber}" p5:count="6" p5:lastRead="6" p5:lastReadDate="2026-10-06T19:03:41" xmlns:p5="http://vaemendis.net/opds-pse/ns" />
  </entry>
  <entry>
    <id>2</id>
    <title>⭘ Nebbia sul Porto - Volume 2</title>
    <link rel="http://vaemendis.net/opds-pse/stream" type="image/jpeg" href="/api/opds/KEY/image?libraryId=1&amp;seriesId=1&amp;volumeId=2&amp;chapterId=2&amp;pageNumber={pageNumber}" p5:count="6" xmlns:p5="http://vaemendis.net/opds-pse/ns" />
  </entry>
</feed>"#;

/// La radice di Kavita: voci di navigazione.
const KAVITA_ROOT: &str = r#"<feed xmlns="http://www.w3.org/2005/Atom"><id>root</id><title>Kavita</title>
  <entry><id>onDeck</id><title>On Deck</title><link rel="subsection" type="application/atom+xml;profile=opds-catalog;kind=navigation" href="/api/opds/KEY/on-deck" /></entry>
  <entry><id>allLibraries</id><title>All Libraries</title><link rel="subsection" type="application/atom+xml;profile=opds-catalog;kind=navigation" href="/api/opds/KEY/libraries" /></entry>
</feed>"#;

#[test]
fn un_feed_di_komga() {
    let feed = parse_feed(KOMGA.as_bytes(), "http://casa:25600/opds/v1.2/series/0RV8BZTHNG14F").unwrap();
    assert_eq!(feed.next.as_deref(), Some("http://casa:25600/opds/v1.2/series/0RV8BZTHNG14F?page=1"));
    assert_eq!(
        feed.entries,
        [FeedEntry {
            id: "0RV8BZTHSG7VG".into(),
            title: "Volume 1".into(),
            authors: vec!["Prova".into()],
            nav: None,
            stream: Some(("http://casa:25600/opds/v1.2/books/0RV8BZTHSG7VG/pages/{pageNumber}".into(), 6)),
            cover: Some("http://casa:25600/opds/v1.2/books/0RV8BZTHSG7VG/thumbnail".into()),
            download: Some(
                "http://casa:25600/opds/v1.2/books/0RV8BZTHSG7VG/file/Nebbia%20sul%20Porto%20v01.cbz".into()
            ),
            last_read: None,
            read_date: None,
        }]
    );
}

#[test]
fn un_feed_di_kavita() {
    let base = "http://casa:5000/api/opds/KEY/series/1";
    let feed = parse_feed(KAVITA.as_bytes(), base).unwrap();
    assert_eq!(feed.entries.len(), 3);
    assert_eq!(feed.entries[0].title, "Continue Reading from: \u{2b58} Nebbia sul Porto - Volume 2");
    let one = &feed.entries[1];
    assert_eq!(
        one.stream,
        Some((
            "http://casa:5000/api/opds/KEY/image?libraryId=1&seriesId=1&volumeId=1&chapterId=1&pageNumber={pageNumber}"
                .into(),
            6
        ))
    );
    assert_eq!(one.cover.as_deref(), Some("http://casa:5000/api/image/chapter-cover?chapterId=1&apiKey=KEY"));
    assert_eq!(one.nav, None, "lo scaricamento non e' un feed");
    assert_eq!(
        one.download.as_deref(),
        Some("http://casa:5000/api/opds/KEY/series/1/volume/1/chapter/1/download/x.cbz")
    );
    assert_eq!((one.last_read, one.read_date), (Some(6), sync::unix_time("2026-10-06T19:03:41")));

    let mut found = Found::default();
    found.add(&feed, Some("Nebbia sul Porto"), None);
    let titles: Vec<_> = found.entries.iter().map(|e| e.title.as_str()).collect();
    assert_eq!(titles, ["Volume 2", "Volume 1"], "il volume ripetuto una volta sola, col titolo pulito");
    assert!(found.entries.iter().all(|e| e.series.as_deref() == Some("Nebbia sul Porto")));
    let v = Volume::from_path(&found.entries[1].path).unwrap();
    assert_eq!(
        v.page_url(0),
        "http://casa:5000/api/opds/KEY/image?libraryId=1&seriesId=1&volumeId=1&chapterId=1&pageNumber=0"
    );
}

#[test]
fn la_radice_porta_alle_librerie() {
    let feed = parse_feed(KAVITA_ROOT.as_bytes(), "http://casa:5000/api/opds/KEY").unwrap();
    let start = STARTS.iter().find_map(|id| feed.entries.iter().find(|e| e.id == *id)).unwrap();
    assert_eq!(start.nav.as_deref(), Some("http://casa:5000/api/opds/KEY/libraries"));
}

#[test]
fn non_e_un_feed() {
    assert!(parse_feed(b"<!doctype html><html><body>Komga</body></html>", "http://x/").is_none());
    assert!(parse_feed(br#"{"status":401}"#, "http://x/").is_none());
    assert!(parse_feed(b"<feed><entry>", "http://x/").is_none(), "Atom senza il suo namespace");
    assert!(parse_feed(b"", "http://x/").is_none());
}

#[test]
fn indirizzi_relativi() {
    let base = "http://casa:5000/api/opds/KEY/series/1?page=2";
    assert_eq!(join(base, "/api/x"), "http://casa:5000/api/x");
    assert_eq!(join(base, "https://altro/y"), "https://altro/y");
    assert_eq!(join(base, "//altro/y"), "http://altro/y");
    assert_eq!(join(base, "2"), "http://casa:5000/api/opds/KEY/series/2");
    assert_eq!(join(base, "?page=3"), "http://casa:5000/api/opds/KEY/series/1?page=3");
    assert_eq!(join("http://casa:5000", "opds"), "http://casa:5000/opds");
    assert_eq!(origin("https://Casa.lan:8443/opds?x=1"), "https://Casa.lan:8443");
    assert_eq!(origin("http://casa"), "http://casa");
    assert_eq!(
        shown("http://casa:5000/api/opds/gqOv6PtKcWdfyyx24b2rsUaQUgydcmMY"),
        "http://casa:5000/api/opds/\u{2022}\u{2022}\u{2022}\u{2022}"
    );
    assert_eq!(shown("https://komga.casa-mia-lunghissima.lan"), "https://komga.casa-mia-lunghissima.lan");
}

#[test]
fn il_percorso_di_un_volume() {
    let v = Volume {
        template: "http://casa/b/1/pages/{pageNumber}".into(),
        count: 12,
        title: "Orbita Bassa #3: fine".into(),
    };
    let path = v.path();
    assert!(is_remote(&path));
    assert_eq!(Volume::from_path(&path), Some(v.clone()), "il # e i : nel titolo non confondono");
    assert_eq!(v.page_url(11), "http://casa/b/1/pages/11");
    assert!(Volume::from_path(Path::new("/fumetti/Orbita Bassa #3.cbz")).is_none());
    assert!(Volume::from_path(Path::new("http://casa/b/1/pages/{pageNumber}#0:vuoto")).is_none());
    assert!(Volume::from_path(Path::new("http://casa/b/1/file#3:senza pagine")).is_none());
    assert!(is_remote(Path::new("HTTPS://casa/x")));
    assert!(!is_remote(Path::new("httpd/x.cbz")));
}

#[test]
fn titoli_puliti() {
    assert_eq!(clean_title("⭘ Nebbia sul Porto - Volume 2", Some("Nebbia sul Porto")), "Volume 2");
    assert_eq!(clean_title("◔ Kaiju: Capitolo 3", Some("Kaiju")), "Capitolo 3");
    assert_eq!(clean_title("Volume 1", Some("Nebbia sul Porto")), "Volume 1");
    assert_eq!(clean_title("Nebbia sul Porto", Some("Nebbia sul Porto")), "Nebbia sul Porto", "non resta vuoto");
    assert_eq!(clean_title("A Volume 1", None), "A Volume 1", "una lettera non e' un segno");
    // senza serie dal catalogo, la serie si indovina dal titolo come per i file
    let e = remote_entry("http://x/{pageNumber}", 3, "Orbita Bassa v03".into(), None, &[]);
    assert_eq!((e.series.as_deref(), e.number), (Some("Orbita Bassa"), Some(3)));
    let e = remote_entry("http://x/{pageNumber}", 3, "Volume 12".into(), Some("Kaiju"), &[]);
    assert_eq!((e.series.as_deref(), e.number), (Some("Kaiju"), Some(12)));
}

#[test]
fn errori_di_curl() {
    assert!(
        explain("curl: (22) The requested URL returned error: 401").starts_with(t("nome o password", "wrong user"))
    );
    assert!(
        explain("curl: (7) Failed to connect to casa port 25600").ends_with("(Failed to connect to casa port 25600)")
    );
    assert_eq!(explain("curl: (35) TLS"), "(35) TLS");
    assert_eq!(quoted(r#"io:pa"ss\"#), r#""io:pa\"ss\\""#);
}

#[test]
fn dove_si_cerca() {
    let root = r#"<feed xmlns="http://www.w3.org/2005/Atom"><id>root</id>
            <link type="application/opensearchdescription+xml" rel="search" href="/opds/v1.2/search"/>
            <entry><id>allSeries</id><title>All series</title></entry></feed>"#;
    let feed = parse_feed(root.as_bytes(), "http://casa:25600/opds/v1.2/catalog").unwrap();
    assert_eq!(feed.search.as_deref(), Some("http://casa:25600/opds/v1.2/search"));
    assert!(feed.entries[0].nav.is_none(), "il link di ricerca non e' di una voce");

    let komga = r#"<OpenSearchDescription xmlns="http://a9.com/-/spec/opensearch/1.1/"><ShortName>Search</ShortName>
            <Url template="http://casa:25600/opds/v1.2/series?search={searchTerms}" type="application/atom+xml;profile=opds-catalog;kind=acquisition"/></OpenSearchDescription>"#;
    assert_eq!(
        opensearch_template(komga.as_bytes(), "http://casa:25600/opds/v1.2/search").as_deref(),
        Some("http://casa:25600/opds/v1.2/series?search={searchTerms}")
    );
    // Kavita: indirizzo relativo, e prima una pagina web da scartare
    let kavita = r#"<?xml version="1.0"?><OpenSearchDescription xmlns="http://a9.com/-/spec/opensearch/1.1/">
            <Url type="text/html" template="/search?q={searchTerms}"/>
            <Url type="application/atom+xml" template="/api/opds/KEY/series?query={searchTerms}&amp;page={startPage?}"/></OpenSearchDescription>"#;
    let template = opensearch_template(kavita.as_bytes(), "http://casa:5000/api/opds/KEY/search").unwrap();
    assert_eq!(template, "http://casa:5000/api/opds/KEY/series?query={searchTerms}&page={startPage?}");
    assert_eq!(
        fill_search(&template, "città & mare"),
        "http://casa:5000/api/opds/KEY/series?query=citt%C3%A0%20%26%20mare&page="
    );
    assert_eq!(fill_search("http://x/s?q={searchTerms}&n={count}", "a"), "http://x/s?q=a&n={count}");
    assert_eq!(opensearch_template(b"<html>nope</html>", "http://x/"), None);
}

/// Con un server vero: FUMETTO_PROVA_OPDS="indirizzo|nome|password".
#[test]
#[ignore]
fn un_server_vero() {
    let spec = std::env::var("FUMETTO_PROVA_OPDS").expect("FUMETTO_PROVA_OPDS");
    let mut parts = spec.split('|');
    let server = Server {
        url: parts.next().unwrap().into(),
        user: parts.next().unwrap_or("").into(),
        password: parts.next().unwrap_or("").into(),
    };
    set_servers(std::slice::from_ref(&server));
    let dir = std::env::temp_dir().join(format!("fumetto-opds-{}", std::process::id()));
    offline::set_folders(dir.join("dati"), dir.join("cache"));
    let catalog = volumes(&server).unwrap();
    assert!(catalog.stale.is_none());
    let found = &catalog.entries;
    // la ricerca del server: i volumi della prima serie, cercandone il nome
    if let Some(series) = found.iter().find_map(|e| e.series.clone()) {
        let hits = search(&server, &series).unwrap();
        println!("cercando {series:?}: {:?}", hits.iter().map(|e| &e.title).collect::<Vec<_>>());
        assert!(hits.iter().any(|e| e.series.as_ref() == Some(&series)), "la serie si trova");
        assert!(hits.iter().all(|e| catalog.entries.iter().any(|c| c.path == e.path)), "gli stessi volumi");
    }
    for r in &catalog.reads {
        println!("letto: {} {} {:?}", r.path.display(), r.last_read, r.date);
    }
    for e in found {
        println!("{:?} / {} ({:?}) {:?}", e.series, e.title, e.number, e.groups);
    }
    assert!(!found.is_empty());
    let v = Volume::from_path(&found[0].path).unwrap();
    let page = v.page(0, true).unwrap();
    assert!(crate::decode::dimensions(&page).is_some(), "la prima pagina e' un'immagine");
    assert!(cover(&found[0].path).is_some_and(|c| !c.is_empty()));
    // scaricato, il volume si apre dalla copia: uguale, pagina per pagina
    let file = offline::download(&found[0].path).unwrap();
    assert_eq!(offline::downloaded(&found[0].path), Some(file));
    let book = crate::Book::open(&found[0].path).unwrap();
    assert_eq!(book.path, found[0].path);
    assert_eq!(book.len(), v.count);
    offline::forget(&found[0].path).unwrap();
    assert_eq!(offline::downloaded(&found[0].path), None);
    // il catalogo e' rimasto su disco: senza server (qui: un altro
    // indirizzo con lo stesso file) lo si ritrova
    let saved = offline::load_catalog(&server).unwrap();
    assert_eq!(saved.entries, catalog.entries);
    // il punto di lettura arriva al server: alla terza pagina, poi di
    // nuovo da leggere
    let last = &found[found.len() - 1].path;
    let template = Volume::from_path(last).unwrap().template;
    let on_server = || crawl(&server).unwrap().reads.into_iter().find(|r| &r.path == last).unwrap().last_read;
    sync::report(last, 2, 6);
    sync::flush();
    let third = if matches!(Flavor::of(&template), Flavor::Kavita { .. }) { 2 } else { 3 };
    assert_eq!(on_server(), third);
    sync::report_mark(last, false);
    sync::flush();
    assert_eq!(on_server(), 0);
    let _ = std::fs::remove_dir_all(dir);
}
