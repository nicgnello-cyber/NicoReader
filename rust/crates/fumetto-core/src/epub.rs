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

use quick_xml::events::{BytesStart, Event};
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
fn attr(e: &BytesStart, name: &[u8]) -> Option<String> {
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
    let mut o = Opf::default();
    // l'elemento aperto di cui serve il testo: nome, proprieta', id
    let mut open: Option<(Vec<u8>, Option<String>, Option<String>)> = None;
    let mut text = String::new();
    loop {
        match r.read_event().ok()? {
            Event::Start(e) | Event::Empty(e) if is_structure(&e) => o.structure(&e),
            // Calibre (EPUB 2): <meta name="calibre:series" content="..."/>
            Event::Empty(e) if e.local_name().as_ref() == b"meta" => o.calibre(&e),
            Event::Start(e) => {
                let name = e.local_name().as_ref().to_vec();
                let wanted =
                    matches!(name.as_slice(), b"title" | b"creator" | b"subject" | b"publisher" | b"date" | b"meta");
                open = wanted.then(|| (name, attr(&e, b"property"), attr(&e, b"id").or_else(|| attr(&e, b"refines"))));
                text.clear();
            }
            Event::Text(t) => text.push_str(&t.xml10_content().ok()?),
            Event::GeneralRef(g) => {
                let _ = crate::xml::push_reference(&mut text, &g);
            }
            Event::End(_) => {
                let value = text.trim().to_owned();
                text.clear();
                if let Some((name, property, id)) = open.take().filter(|_| !value.is_empty()) {
                    o.ended(&name, property.as_deref(), id, value);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Some(o.finish())
}

/// Gli elementi che danno la struttura: voci, ordine di lettura, verso.
fn is_structure(e: &BytesStart) -> bool {
    matches!(e.local_name().as_ref(), b"item" | b"itemref" | b"spine")
}

/// Cio' che si raccoglie leggendo il pacchetto, prima di metterlo insieme.
#[derive(Default)]
struct Opf {
    p: Package,
    info: ComicInfo,
    creators: Vec<String>,
    subjects: Vec<String>,
    /// EPUB 3: <meta property="belongs-to-collection" id="c1">Serie</meta>,
    /// con <meta refines="#c1" property="group-position">3</meta>
    collections: Vec<(Option<String>, String)>,
    refines: HashMap<String, Vec<(String, String)>>,
}

impl Opf {
    /// Le voci del manifest, quelle dello spine (non quelle fuori lettura)
    /// e il verso.
    fn structure(&mut self, e: &BytesStart) {
        match e.local_name().as_ref() {
            b"item" => {
                if let (Some(id), Some(href)) = (attr(e, b"id"), attr(e, b"href")) {
                    let media = attr(e, b"media-type").unwrap_or_default().to_ascii_lowercase();
                    self.p.manifest.insert(id, (href, media));
                }
            }
            b"itemref" => {
                if attr(e, b"linear").as_deref() != Some("no")
                    && let Some(id) = attr(e, b"idref")
                {
                    self.p.spine.push(id);
                }
            }
            b"spine" => {
                if let Some(dir) = attr(e, b"page-progression-direction") {
                    self.info.right_to_left = match dir.as_str() {
                        "rtl" => Some(true),
                        "ltr" => Some(false),
                        _ => None,
                    };
                }
            }
            _ => {}
        }
    }

    /// La serie e il numero come li scrive Calibre.
    fn calibre(&mut self, e: &BytesStart) {
        let value = attr(e, b"content").unwrap_or_default();
        match attr(e, b"name").as_deref() {
            Some("calibre:series") => self.info.series = Some(value),
            Some("calibre:series_index") => self.info.number = Some(trim_index(&value)),
            _ => {}
        }
    }

    /// Un metadato finito, con il suo testo (non vuoto).
    fn ended(&mut self, name: &[u8], property: Option<&str>, id: Option<String>, value: String) {
        let info = &mut self.info;
        match (name, property) {
            (b"title", _) if info.title.is_none() => info.title = Some(value),
            (b"creator", _) => self.creators.push(value),
            (b"subject", _) => self.subjects.push(value),
            (b"publisher", _) => info.publisher = Some(value),
            (b"date", _) if info.year.is_none() => info.year = crate::comicinfo::year_of(&value),
            (b"meta", Some("belongs-to-collection")) => self.collections.push((id, value)),
            (b"meta", Some(prop)) => {
                if let Some(target) = id.as_deref().and_then(|i| i.strip_prefix('#')) {
                    self.refines.entry(target.to_owned()).or_default().push((prop.to_owned(), value));
                }
            }
            _ => {}
        }
    }

    /// Il pacchetto: le collezioni diventano serie o raccolte, gli autori
    /// una riga sola (senza doppi), gli argomenti i generi.
    fn finish(mut self) -> Package {
        let info = &mut self.info;
        // la collezione di tipo "series" e' la serie, le altre raccolte
        for (id, name) in std::mem::take(&mut self.collections) {
            let props = id.as_deref().and_then(|i| self.refines.get(i)).cloned().unwrap_or_default();
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
        for c in self.creators {
            if !authors.iter().any(|a| a.eq_ignore_ascii_case(&c)) {
                authors.push(c);
            }
        }
        info.authors = (!authors.is_empty()).then(|| authors.join(", "));
        for s in self.subjects.iter().flat_map(|s| s.split([',', ';'])) {
            let s = s.trim();
            if !s.is_empty() && !info.genres.iter().any(|g| g.eq_ignore_ascii_case(s)) {
                info.genres.push(s.to_owned());
            }
        }
        let info = std::mem::take(&mut self.info);
        self.p.info = (info != ComicInfo::default()).then_some(info);
        self.p
    }
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
pub(crate) mod tests;
