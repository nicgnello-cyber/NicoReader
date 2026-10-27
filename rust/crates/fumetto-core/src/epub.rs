//! Gli EPUB fatti di immagini: i manga e i fumetti venduti come ebook, e quelli
//! che Komga e Kavita tengono in libreria.
//!
//! Un EPUB e' uno zip. META-INF/container.xml dice dov'e' il pacchetto (il
//! file .opf); il pacchetto elenca i file (manifest) e l'ordine di lettura
//! (spine). Ogni voce dello spine e' di solito una paginetta XHTML con dentro
//! un'immagine (<img> o, nei layout fissi, <svg><image>): le pagine sono
//! quelle immagini, in quell'ordine, non in ordine di nome.
//!
//! Gli EPUB di testo (i romanzi) non sono per NicoReader: se la maggior parte
//! delle voci non ha immagini, lo si dice invece di mostrare la sola copertina.
//!
//! Dal pacchetto vengono anche titolo, autori, serie e numero (Calibre e
//! EPUB 3), editore, generi e il verso di lettura (page-progression-direction).

use std::collections::{HashMap, HashSet};
use std::io::Read;

use quick_xml::events::Event;
use quick_xml::{NsReader, XmlVersion};

use crate::book::is_image;
use crate::comicinfo::ComicInfo;

/// Oltre questa misura un file del pacchetto non si legge (non e' una pagina).
const MAX_DOC: u64 = 4 << 20;

/// Le pagine e la scheda di un EPUB.
#[derive(Debug, Default, PartialEq)]
pub struct Epub {
    /// I nomi delle immagini nello zip, in ordine di lettura.
    pub pages: Vec<String>,
    pub info: Option<ComicInfo>,
}

/// Un EPUB di testo: niente pagine da mostrare.
#[derive(Debug, PartialEq, Eq)]
pub struct TextOnly;

/// Lo zip e' un EPUB (ha il suo container.xml).
pub fn is_epub<R: Read + std::io::Seek>(zip: &zip::ZipArchive<R>) -> bool {
    zip.index_for_name("META-INF/container.xml").is_some()
}

/// Per la libreria, senza decomprimere niente: un file che non e' un EPUB, o
/// un EPUB con almeno un'immagine ogni due paginette, e' un fumetto; un
/// romanzo ha molte paginette e poche immagini.
pub fn looks_like_comic(path: &std::path::Path) -> bool {
    if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("epub")) {
        return true;
    }
    let Ok(zip) = std::fs::File::open(path).map_err(|_| ()).and_then(|f| zip::ZipArchive::new(f).map_err(|_| ()))
    else {
        // rovinato: lo si mostra, e aprendolo si dira' cosa non va
        return true;
    };
    let (mut images, mut docs) = (0, 0);
    for name in zip.file_names() {
        let lower = name.to_ascii_lowercase();
        if is_image(name) {
            images += 1;
        } else if [".xhtml", ".html", ".htm"].iter().any(|e| lower.ends_with(e)) {
            docs += 1;
        }
    }
    images > 0 && images * 2 >= docs
}

/// Le pagine e la scheda dell'EPUB. `Err(TextOnly)` se e' un libro di testo;
/// `Ok(None)` se il pacchetto non si legge (allora si sfogliano le immagini in
/// ordine di nome, come un CBZ).
pub fn read<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>) -> Result<Option<Epub>, TextOnly> {
    let Some(opf_path) = file(zip, "META-INF/container.xml").and_then(|c| rootfile(&c)) else { return Ok(None) };
    let Some(opf) = file(zip, &opf_path) else { return Ok(None) };
    let Some(package) = parse_package(&opf) else { return Ok(None) };
    let base = dir_of(&opf_path);
    let names: HashSet<String> = zip.file_names().map(str::to_owned).collect();
    let mut pages: Vec<String> = Vec::new();
    let (mut documents, mut with_images) = (0, 0);
    for idref in &package.spine {
        let Some((href, media)) = package.manifest.get(idref) else { continue };
        let path = resolve(&base, href);
        if media.starts_with("image/") || is_image(&path) {
            push(&mut pages, &names, path);
            continue;
        }
        documents += 1;
        let Some(doc) = file(zip, &path) else { continue };
        let found = images_in(&doc);
        if !found.is_empty() {
            with_images += 1;
        }
        let doc_dir = dir_of(&path);
        for src in found {
            push(&mut pages, &names, resolve(&doc_dir, &src));
        }
    }
    // la meta' o piu' delle paginette senza immagini: e' un libro di testo
    if pages.is_empty() || (documents > 0 && with_images * 2 < documents) {
        return Err(TextOnly);
    }
    Ok(Some(Epub { pages, info: package.info }))
}

/// Aggiunge la pagina se c'e' nello zip e non e' quella appena messa (la
/// stessa immagine di sfondo in due paginette di fila).
fn push(pages: &mut Vec<String>, names: &HashSet<String>, path: String) {
    if is_image(&path) && names.contains(&path) && pages.last() != Some(&path) {
        pages.push(path);
    }
}

/// Un file dello zip come testo (UTF-8), se non e' troppo grande.
fn file<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Option<String> {
    let f = zip.by_name(name).ok()?;
    if f.size() > MAX_DOC {
        return None;
    }
    let mut out = Vec::with_capacity(f.size() as usize);
    f.take(MAX_DOC).read_to_end(&mut out).ok()?;
    String::from_utf8(out).ok()
}

/// La cartella di un percorso dello zip ("OEBPS/content.opf" -> "OEBPS/").
fn dir_of(path: &str) -> String {
    path.rfind('/').map_or(String::new(), |i| path[..=i].to_owned())
}

/// Un indirizzo relativo del pacchetto come nome nello zip: decodificato
/// (%20), senza #frammento, con . e .. risolti.
fn resolve(base: &str, href: &str) -> String {
    let href = href.split('#').next().unwrap_or(href);
    let href = percent_decode(href);
    let joined = if let Some(abs) = href.strip_prefix('/') { abs.to_owned() } else { format!("{base}{href}") };
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Dove sta il pacchetto, da META-INF/container.xml.
fn rootfile(container: &str) -> Option<String> {
    let mut r = NsReader::from_str(container);
    loop {
        match r.read_event().ok()? {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"rootfile" => {
                return attr(&e, b"full-path");
            }
            Event::Eof => return None,
            _ => {}
        }
    }
}

/// Il valore di un attributo, per nome locale (senza prefisso).
fn attr(e: &quick_xml::events::BytesStart, name: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == name)
        .and_then(|a| a.normalized_value(XmlVersion::Implicit1_0).ok())
        .map(|v| v.into_owned())
}

#[derive(Debug, Default)]
struct Package {
    /// id -> (indirizzo, tipo)
    manifest: HashMap<String, (String, String)>,
    /// Gli id delle voci, in ordine di lettura.
    spine: Vec<String>,
    info: Option<ComicInfo>,
}

/// Il pacchetto (.opf): manifest, spine e i metadati che servono.
fn parse_package(opf: &str) -> Option<Package> {
    let mut r = NsReader::from_str(opf);
    let mut p = Package::default();
    let mut info = ComicInfo::default();
    let (mut creators, mut subjects) = (Vec::new(), Vec::new());
    // EPUB 3: <meta property="belongs-to-collection" id="c1">Serie</meta>,
    // con <meta refines="#c1" property="group-position">3</meta>
    let mut collections: Vec<(Option<String>, String)> = Vec::new();
    let mut refines: HashMap<String, Vec<(String, String)>> = HashMap::new();
    // l'elemento aperto di cui serve il testo
    let mut open: Option<(Vec<u8>, Option<String>, Option<String>)> = None;
    let mut text = String::new();
    loop {
        match r.read_event().ok()? {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"item" => {
                if let (Some(id), Some(href)) = (attr(&e, b"id"), attr(&e, b"href")) {
                    let media = attr(&e, b"media-type").unwrap_or_default().to_ascii_lowercase();
                    p.manifest.insert(id, (href, media));
                }
            }
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"itemref" => {
                if attr(&e, b"linear").as_deref() != Some("no")
                    && let Some(id) = attr(&e, b"idref")
                {
                    p.spine.push(id);
                }
            }
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"spine" => {
                if let Some(dir) = attr(&e, b"page-progression-direction") {
                    info.right_to_left = match dir.as_str() {
                        "rtl" => Some(true),
                        "ltr" => Some(false),
                        _ => None,
                    };
                }
            }
            // Calibre (EPUB 2): <meta name="calibre:series" content="..."/>
            Event::Empty(e) if e.local_name().as_ref() == b"meta" && attr(&e, b"name").is_some() => {
                let value = attr(&e, b"content").unwrap_or_default();
                match attr(&e, b"name").as_deref() {
                    Some("calibre:series") => info.series = Some(value),
                    Some("calibre:series_index") => info.number = Some(trim_index(&value)),
                    _ => {}
                }
            }
            Event::Start(e) => {
                let name = e.local_name().as_ref().to_vec();
                let wanted = matches!(name.as_slice(), b"title" | b"creator" | b"subject" | b"publisher" | b"meta");
                open = wanted.then(|| (name, attr(&e, b"property"), attr(&e, b"id").or_else(|| attr(&e, b"refines"))));
                text.clear();
            }
            Event::Text(t) => text.push_str(&t.xml10_content().ok()?),
            Event::GeneralRef(g) => {
                if let Ok(Some(c)) = g.resolve_char_ref() {
                    text.push(c);
                } else if let Ok(name) = g.decode() {
                    text.push_str(match name.as_ref() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => "",
                    });
                }
            }
            Event::End(_) => {
                let value = text.trim().to_owned();
                text.clear();
                let Some((name, property, id)) = open.take() else { continue };
                if value.is_empty() {
                    continue;
                }
                match (name.as_slice(), property.as_deref()) {
                    (b"title", _) if info.title.is_none() => info.title = Some(value),
                    (b"creator", _) => creators.push(value),
                    (b"subject", _) => subjects.push(value),
                    (b"publisher", _) => info.publisher = Some(value),
                    (b"meta", Some("belongs-to-collection")) => collections.push((id, value)),
                    (b"meta", Some(prop)) => {
                        if let Some(target) = id.as_deref().and_then(|i| i.strip_prefix('#')) {
                            refines.entry(target.to_owned()).or_default().push((prop.to_owned(), value));
                        }
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    // la collezione di tipo "series" e' la serie, le altre raccolte
    for (id, name) in collections {
        let props = id.as_deref().and_then(|i| refines.get(i)).cloned().unwrap_or_default();
        let prop = |k: &str| props.iter().find(|(p, _)| p == k).map(|(_, v)| v.clone());
        if prop("collection-type").as_deref() == Some("series")
            || (info.series.is_none() && prop("group-position").is_some())
        {
            if info.series.is_none() {
                info.series = Some(name);
                info.number = prop("group-position").map(|n| trim_index(&n)).or(info.number.take());
            }
        } else if !info.series_group.contains(&name) {
            info.series_group.push(name);
        }
    }
    let mut authors: Vec<String> = Vec::new();
    for c in creators {
        if !authors.iter().any(|a| a.eq_ignore_ascii_case(&c)) {
            authors.push(c);
        }
    }
    info.authors = (!authors.is_empty()).then(|| authors.join(", "));
    for s in subjects.iter().flat_map(|s| s.split([',', ';'])) {
        let s = s.trim();
        if !s.is_empty() && !info.genres.iter().any(|g| g.eq_ignore_ascii_case(s)) {
            info.genres.push(s.to_owned());
        }
    }
    p.info = (info != ComicInfo::default()).then_some(info);
    Some(p)
}

/// "3.0" -> "3", "2.5" resta: Calibre scrive gli indici con la virgola mobile.
fn trim_index(n: &str) -> String {
    let n = n.trim();
    n.strip_suffix(".0").unwrap_or(n).to_owned()
}

/// Le immagini di una paginetta XHTML: <img src>, <image href> e
/// <image xlink:href> (SVG), nell'ordine in cui compaiono.
fn images_in(doc: &str) -> Vec<String> {
    let mut r = NsReader::from_str(doc);
    let mut out = Vec::new();
    while let Ok(event) = r.read_event() {
        match event {
            Event::Start(e) | Event::Empty(e) => {
                let src = match e.local_name().as_ref() {
                    b"img" => attr(&e, b"src"),
                    b"image" => attr(&e, b"href"),
                    _ => None,
                };
                out.extend(src.filter(|s| !s.starts_with("data:")));
            }
            Event::Eof => break,
            _ => {}
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
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
}
