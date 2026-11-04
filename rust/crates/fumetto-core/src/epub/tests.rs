use super::*;

/// Un EPUB in memoria: i file dati (nome, contenuto), senza compressione.
pub(crate) fn epub(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut out);
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("mimetype", stored).unwrap();
    std::io::Write::write_all(&mut zip, b"application/epub+zip").unwrap();
    zip.start_file("META-INF/container.xml", stored).unwrap();
    std::io::Write::write_all(
            &mut zip,
            br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
              <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
            </container>"#,
        )
        .unwrap();
    for (name, bytes) in files {
        zip.start_file(*name, stored).unwrap();
        std::io::Write::write_all(&mut zip, bytes).unwrap();
    }
    zip.finish().unwrap();
    out.into_inner()
}

/// Un manga in EPUB 3 a layout fisso, come lo fa Kindle Comic Converter:
/// le immagini in ordine diverso dal nome, da destra a sinistra, la serie
/// come collezione, la stessa immagine ripetuta, un'immagine fuori spine.
pub(crate) fn manga() -> Vec<u8> {
    let page = |img: &str| {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><html xmlns="http://www.w3.org/1999/xhtml"><body>
                <svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 10 10">
                <image width="10" height="10" xlink:href="{img}"/></svg></body></html>"#
        )
    };
    let opf = r##"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Nebbia sul Porto &amp; altre storie</dc:title>
    <dc:creator id="a1">Mario Rossi</dc:creator>
    <dc:creator>Anna Bianchi</dc:creator>
    <dc:publisher>Edizioni Prova</dc:publisher>
    <dc:subject>Giallo, Avventura</dc:subject>
    <meta property="belongs-to-collection" id="c1">Nebbia sul Porto</meta>
    <meta refines="#c1" property="collection-type">series</meta>
    <meta refines="#c1" property="group-position">3</meta>
    <meta property="belongs-to-collection" id="c2">Gialli italiani</meta>
  </metadata>
  <manifest>
    <item id="p1" href="Text/p%201.xhtml" media-type="application/xhtml+xml"/>
    <item id="p2" href="Text/p2.xhtml" media-type="application/xhtml+xml"/>
    <item id="p3" href="Text/p3.xhtml" media-type="application/xhtml+xml"/>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="i1" href="Images/zz.png" media-type="image/png"/>
    <item id="i2" href="Images/aa.png" media-type="image/png"/>
    <item id="i3" href="Images/fuori.png" media-type="image/png"/>
  </manifest>
  <spine page-progression-direction="rtl">
    <itemref idref="p1"/><itemref idref="p2"/><itemref idref="p3"/>
    <itemref idref="nav" linear="no"/>
  </spine>
</package>"##;
    epub(&[
        ("OEBPS/content.opf", opf.as_bytes()),
        ("OEBPS/Text/p 1.xhtml", page("../Images/zz.png").as_bytes()),
        ("OEBPS/Text/p2.xhtml", page("../Images/aa.png").as_bytes()),
        ("OEBPS/Text/p3.xhtml", page("../Images/aa.png").as_bytes()),
        ("OEBPS/nav.xhtml", b"<html><body><nav>indice</nav></body></html>"),
        ("OEBPS/Images/zz.png", &png()),
        ("OEBPS/Images/aa.png", &png()),
        ("OEBPS/Images/fuori.png", &png()),
    ])
}

/// Un PNG 1x1, abbastanza per essere una pagina.
pub(crate) fn png() -> Vec<u8> {
    let mut out = Vec::new();
    let img = image::RgbaImage::from_pixel(2, 3, image::Rgba([200, 30, 30, 255]));
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
    out
}

fn open(bytes: Vec<u8>) -> zip::ZipArchive<std::io::Cursor<Vec<u8>>> {
    zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap()
}

#[test]
fn le_pagine_nell_ordine_del_libro() {
    let mut zip = open(manga());
    assert!(is_epub(&zip));
    let e = read(&mut zip).unwrap().unwrap();
    assert_eq!(e.pages, ["OEBPS/Images/zz.png", "OEBPS/Images/aa.png"], "spine, %20, .., niente doppioni");
    let info = e.info.unwrap();
    assert_eq!(info.title.as_deref(), Some("Nebbia sul Porto & altre storie"));
    assert_eq!((info.series.as_deref(), info.number.as_deref()), (Some("Nebbia sul Porto"), Some("3")));
    assert_eq!(info.authors.as_deref(), Some("Mario Rossi, Anna Bianchi"));
    assert_eq!(info.right_to_left, Some(true));
    assert_eq!(info.publisher.as_deref(), Some("Edizioni Prova"));
    assert_eq!(info.genres, ["Giallo", "Avventura"]);
    assert_eq!(info.series_group, ["Gialli italiani"]);
}

#[test]
fn le_immagini_nello_spine_e_calibre() {
    let opf = r#"<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:title>Orbita Bassa</dc:title>
    <meta name="calibre:series" content="Orbita Bassa"/>
    <meta name="calibre:series_index" content="2.0"/>
  </metadata>
  <manifest>
    <item id="a" href="b.jpg" media-type="image/jpeg"/>
    <item id="b" href="a.jpg" media-type="image/jpeg"/>
  </manifest>
  <spine><itemref idref="a"/><itemref idref="b"/></spine>
</package>"#;
    let mut zip =
        open(epub(&[("OEBPS/content.opf", opf.as_bytes()), ("OEBPS/b.jpg", b"jpg"), ("OEBPS/a.jpg", b"jpg")]));
    let e = read(&mut zip).unwrap().unwrap();
    assert_eq!(e.pages, ["OEBPS/b.jpg", "OEBPS/a.jpg"]);
    let info = e.info.unwrap();
    assert_eq!((info.series.as_deref(), info.number.as_deref()), (Some("Orbita Bassa"), Some("2")));
    assert_eq!(info.right_to_left, None);
}

#[test]
fn un_romanzo_non_e_un_fumetto() {
    let opf = r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata/>
  <manifest>
    <item id="c" href="cover.xhtml" media-type="application/xhtml+xml"/>
    <item id="t1" href="c1.xhtml" media-type="application/xhtml+xml"/>
    <item id="t2" href="c2.xhtml" media-type="application/xhtml+xml"/>
    <item id="img" href="cover.jpg" media-type="image/jpeg"/>
  </manifest>
  <spine><itemref idref="c"/><itemref idref="t1"/><itemref idref="t2"/></spine></package>"#;
    let mut zip = open(epub(&[
        ("OEBPS/content.opf", opf.as_bytes()),
        ("OEBPS/cover.xhtml", br#"<html><body><img src="cover.jpg"/></body></html>"#),
        ("OEBPS/c1.xhtml", b"<html><body><p>C'era una volta</p></body></html>"),
        ("OEBPS/c2.xhtml", b"<html><body><p>Fine</p></body></html>"),
        ("OEBPS/cover.jpg", b"jpg"),
    ]));
    assert_eq!(read(&mut zip), Err(TextOnly));
}

/// Come volume: pagine nell'ordine del libro, la scheda per la libreria, e
/// il romanzo che non si apre.
#[test]
fn un_epub_come_volume() {
    let dir = std::env::temp_dir().join(format!("fumetto-epub-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Nebbia sul Porto 03.epub");
    std::fs::write(&path, manga()).unwrap();
    let book = crate::Book::open(&path).unwrap();
    assert_eq!(book.names, ["OEBPS/Images/zz.png", "OEBPS/Images/aa.png"]);
    assert_eq!(book.read(0).unwrap(), png());
    assert_eq!(book.info.as_ref().and_then(|i| i.right_to_left), Some(true), "da destra a sinistra");
    let entries = crate::library::scan(std::slice::from_ref(&dir));
    assert_eq!(entries.len(), 1, "l'EPUB e' un volume della libreria");
    assert_eq!(entries[0].series.as_deref(), Some("Nebbia sul Porto"));
    assert_eq!(entries[0].number, Some(3));
    use crate::library::{Group, GroupKind};
    assert_eq!(
        entries[0].groups,
        [
            Group::new(GroupKind::Collection, "Gialli italiani", None),
            Group::new(GroupKind::Genre, "Giallo", None),
            Group::new(GroupKind::Genre, "Avventura", None),
            Group::new(GroupKind::Publisher, "Edizioni Prova", None),
        ],
        "raccolte, generi ed editore dalla scheda"
    );
    let text = dir.join("romanzo.epub");
    let opf = br#"<package xmlns="http://www.idpf.org/2007/opf"><manifest>
            <item id="t" href="t.xhtml" media-type="application/xhtml+xml"/></manifest>
            <spine><itemref idref="t"/></spine></package>"#;
    std::fs::write(&text, epub(&[("OEBPS/content.opf", opf), ("OEBPS/t.xhtml", b"<html><p>testo</p></html>")]))
        .unwrap();
    assert!(matches!(crate::Book::open(&text), Err(crate::Error::TextEpub)));
    assert!(!looks_like_comic(&text));
    assert_eq!(crate::library::scan(std::slice::from_ref(&dir)).len(), 1, "il romanzo non entra in libreria");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn indirizzi() {
    assert_eq!(resolve("OEBPS/Text/", "../Images/a%20b.png#x"), "OEBPS/Images/a b.png");
    assert_eq!(resolve("", "./img/p.jpg"), "img/p.jpg");
    assert_eq!(resolve("OEBPS/", "/img/p.jpg"), "img/p.jpg");
    assert_eq!(percent_decode("%E2%80%94%zz"), "\u{2014}%zz");
}

fn package(metadata: &str, spine: &str) -> Package {
    let opf = format!(
        r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="3.0">
  <metadata>{metadata}</metadata>
  <manifest>
    <item id="p1" href="p1.xhtml" media-type="application/xhtml+xml"/>
    <item id="p2" href="p2.xhtml" media-type="APPLICATION/XHTML+XML"/>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  {spine}
</package>"#
    );
    parse_package(&opf).expect("un pacchetto")
}

#[test]
fn epub3_serie_raccolte_autori_generi_anno() {
    let p = package(
        r##"<dc:title>Il porto</dc:title>
        <dc:title>Sottotitolo</dc:title>
        <dc:creator>Anna Rossi</dc:creator>
        <dc:creator>ANNA ROSSI</dc:creator>
        <dc:creator>Bruno Neri</dc:creator>
        <dc:subject>Azione, Avventura; azione</dc:subject>
        <dc:subject>Mare</dc:subject>
        <dc:publisher>Edizioni Nebbia</dc:publisher>
        <dc:date>2021-05-03</dc:date>
        <meta property="belongs-to-collection" id="c1">Nebbia sul Porto</meta>
        <meta refines="#c1" property="collection-type">series</meta>
        <meta refines="#c1" property="group-position">3</meta>
        <meta property="belongs-to-collection" id="c2">Universo Nebbia</meta>
        <meta property="belongs-to-collection" id="c3">Universo Nebbia</meta>"##,
        r#"<spine page-progression-direction="rtl">
            <itemref idref="p1"/><itemref idref="nav" linear="no"/><itemref idref="p2"/>
        </spine>"#,
    );
    assert_eq!(p.spine, ["p1", "p2"], "fuori lettura no");
    assert_eq!(p.manifest["p2"], ("p2.xhtml".to_owned(), "application/xhtml+xml".to_owned()));
    let info = p.info.expect("i dati");
    assert_eq!(info.title.as_deref(), Some("Il porto"), "il primo titolo");
    assert_eq!(info.series.as_deref(), Some("Nebbia sul Porto"));
    assert_eq!(info.number.as_deref(), Some("3"));
    assert_eq!(info.series_group, ["Universo Nebbia"], "le altre collezioni, una volta");
    assert_eq!(info.authors.as_deref(), Some("Anna Rossi, Bruno Neri"), "senza doppi");
    assert_eq!(info.genres, ["Azione", "Avventura", "Mare"]);
    assert_eq!(info.publisher.as_deref(), Some("Edizioni Nebbia"));
    assert_eq!(info.year, Some(2021));
    assert_eq!(info.right_to_left, Some(true));
}

#[test]
fn una_collezione_con_il_numero_e_la_serie() {
    let p = package(
        r##"<meta property="belongs-to-collection" id="c1">Orbita Bassa</meta>
        <meta refines="#c1" property="group-position">2.0</meta>"##,
        r#"<spine page-progression-direction="ltr"><itemref idref="p1"/></spine>"#,
    );
    let info = p.info.expect("i dati");
    assert_eq!((info.series.as_deref(), info.number.as_deref()), (Some("Orbita Bassa"), Some("2")));
    assert_eq!(info.right_to_left, Some(false));
}

#[test]
fn calibre_serie_e_numero() {
    let p = package(
        r#"<dc:title>Tom &amp; Jerry &#233; &#x41;</dc:title>
        <meta name="calibre:series" content="Gatti"/>
        <meta name="calibre:series_index" content="4.0"/>
        <meta name="cover" content="img"/>"#,
        r#"<spine><itemref idref="p1"/></spine>"#,
    );
    let info = p.info.expect("i dati");
    assert_eq!(info.title.as_deref(), Some("Tom & Jerry \u{e9} A"));
    assert_eq!((info.series.as_deref(), info.number.as_deref()), (Some("Gatti"), Some("4")));
    assert_eq!(info.right_to_left, None);
}

#[test]
fn senza_metadati_niente_scheda() {
    let p = package("", r#"<spine><itemref idref="p1"/><itemref idref="p2"/></spine>"#);
    assert!(p.info.is_none());
    assert_eq!(p.spine, ["p1", "p2"]);
}
