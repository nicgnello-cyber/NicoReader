//! I feed del catalogo (Atom con OPDS-PSE): voci, collegamenti, pagine.

use super::*;

/// Un feed del catalogo: le voci, la pagina dopo se la lista continua, e
/// (nella radice) dove si cerca.
#[derive(Debug, Default)]
pub(super) struct Feed {
    pub(super) entries: Vec<FeedEntry>,
    pub(super) next: Option<String>,
    pub(super) search: Option<String>,
}

#[derive(Debug, Default, PartialEq)]
pub(super) struct FeedEntry {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) authors: Vec<String>,
    /// Un feed piu' in giu': una serie, una libreria.
    pub(super) nav: Option<String>,
    /// Le pagine: l'indirizzo con `{pageNumber}` e quante sono.
    pub(super) stream: Option<(String, usize)>,
    pub(super) cover: Option<String>,
    /// Il file intero, da scaricare.
    pub(super) download: Option<String>,
    /// Dove e' arrivato chi legge, come lo dice il server, e quando.
    pub(super) last_read: Option<u32>,
    pub(super) read_date: Option<u64>,
}

/// Legge un feed Atom; `None` se non lo e' (una pagina web, un errore in
/// JSON). Gli indirizzi diventano completi, rispetto a `base`.
pub(super) fn parse_feed(xml: &[u8], base: &str) -> Option<Feed> {
    let text = std::str::from_utf8(xml).ok()?;
    let mut r = NsReader::from_str(text);
    let mut feed = Feed::default();
    // gli elementi aperti: (di Atom?, nome locale)
    let mut open: Vec<(bool, Vec<u8>)> = Vec::new();
    let mut entry: Option<FeedEntry> = None;
    let mut buf = String::new();
    loop {
        let (ns, event) = r.read_resolved_event().ok()?;
        let atom = matches!(ns, ResolveResult::Bound(Namespace(n)) if n == ATOM);
        match event {
            Event::Start(e) | Event::Empty(e) if open.is_empty() => {
                if !(atom && e.local_name().as_ref() == b"feed") {
                    return None;
                }
                open.push((true, b"feed".to_vec()));
            }
            Event::Start(e) => {
                let name = e.local_name().as_ref().to_vec();
                if atom && name == b"entry" && open.len() == 1 {
                    entry = Some(FeedEntry::default());
                } else if atom && name == b"link" {
                    link(&r, &e, base, entry.as_mut(), &mut feed);
                }
                open.push((atom, name));
                buf.clear();
            }
            Event::Empty(e) => {
                if atom && e.local_name().as_ref() == b"link" {
                    link(&r, &e, base, entry.as_mut(), &mut feed);
                }
            }
            Event::Text(t) => buf.push_str(&t.xml10_content().ok()?),
            Event::CData(t) => buf.push_str(&t.xml10_content().ok()?),
            Event::GeneralRef(g) => crate::xml::push_reference(&mut buf, &g)?,
            Event::End(_) => {
                let (atom, name) = open.pop()?;
                let text = std::mem::take(&mut buf);
                let parent = open.last().filter(|p| p.0).map(|p| p.1.as_slice());
                if atom {
                    close_element(&mut entry, &mut feed, &name, parent, open.len(), text);
                }
                if open.is_empty() {
                    return Some(feed);
                }
            }
            Event::Eof => return None,
            _ => {}
        }
    }
}

/// Si chiude un elemento di Atom (`name`, dentro `parent`, a profondita'
/// `depth` contando il feed) con il suo testo: il titolo, l'id e gli autori
/// della voce; la voce finita va nel feed.
fn close_element(
    entry: &mut Option<FeedEntry>, feed: &mut Feed, name: &[u8], parent: Option<&[u8]>, depth: usize, text: String,
) {
    let text = text.trim();
    match (entry.as_mut(), name, parent) {
        (Some(e), b"title", Some(b"entry")) => e.title = text.to_owned(),
        (Some(e), b"id", Some(b"entry")) => e.id = text.to_owned(),
        (Some(e), b"name", Some(b"author")) if depth == 3 && !text.is_empty() => e.authors.push(text.to_owned()),
        (Some(_), b"entry", Some(b"feed")) => feed.entries.extend(entry.take()),
        _ => {}
    }
}

/// Un `<link>`: dove porta, se e' nel feed o in una voce.
pub(super) fn link(r: &NsReader<&[u8]>, e: &BytesStart, base: &str, entry: Option<&mut FeedEntry>, feed: &mut Feed) {
    let (mut rel, mut href, mut kind, mut count) = (String::new(), None, String::new(), None);
    let (mut last_read, mut read_date) = (None, None);
    for a in e.attributes().flatten() {
        let (ns, local) = r.resolver().resolve_attribute(a.key);
        let Ok(value) = a.normalized_value(XmlVersion::Implicit1_0) else { continue };
        match ns {
            ResolveResult::Bound(Namespace(n)) if n == PSE => match local.as_ref() {
                b"count" => count = value.trim().parse::<usize>().ok(),
                b"lastRead" => last_read = value.trim().parse::<u32>().ok(),
                b"lastReadDate" => read_date = sync::unix_time(value.trim()),
                _ => {}
            },
            ResolveResult::Unbound => match local.as_ref() {
                b"rel" => rel = value.into_owned(),
                b"href" => href = Some(join(base, &value)),
                b"type" => kind = value.to_ascii_lowercase(),
                _ => {}
            },
            _ => {}
        }
    }
    let Some(href) = href else { return };
    let Some(e) = entry else {
        match rel.as_str() {
            "next" => feed.next = Some(href),
            "search" => {
                feed.search.get_or_insert(href);
            }
            _ => {}
        }
        return;
    };
    match rel.as_str() {
        STREAM => {
            if let Some(n) = count.filter(|&n| n > 0 && href.contains(PAGE)) {
                e.stream = Some((href, n));
                (e.last_read, e.read_date) = (last_read, read_date);
            }
        }
        _ if rel.starts_with("http://opds-spec.org/acquisition") => {
            e.download.get_or_insert(href);
        }
        "http://opds-spec.org/image" => e.cover = Some(href),
        "http://opds-spec.org/image/thumbnail" => {
            e.cover.get_or_insert(href);
        }
        "self" | "start" | "up" | "search" | "related" | "alternate" => {}
        _ if kind.starts_with("application/atom+xml") && !rel.contains("acquisition") => {
            e.nav.get_or_insert(href);
        }
        _ => {}
    }
}
