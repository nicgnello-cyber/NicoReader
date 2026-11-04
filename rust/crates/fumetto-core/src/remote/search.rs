//! La ricerca sul server (OpenSearch): dove si cerca, la domanda, i volumi
//! trovati.

use super::*;

/// Cerca sul server, come la sua pagina di ricerca: i volumi delle serie
/// trovate (e quelli trovati da soli), come voci della libreria. La
/// libreria ha gia' tutto il catalogo, ma un server enorme supera i limiti
/// della lettura, e il server cerca anche dove l'app non guarda (riassunti,
/// etichette). Niente, se il server non offre la ricerca.
pub fn search(server: &Server, query: &str) -> Result<Vec<Entry>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let login = server.login();
    let Some(template) = search_template(server)? else { return Ok(Vec::new()) };
    let url = fill_search(&template, query);
    let mut found = Found::default();
    let mut seen = HashSet::from([url.clone()]);
    let mut level = vec![Visit { origin: url.clone(), url, series: None, depth: 0 }];
    let (mut requests, mut series) = (0, 0);
    while !level.is_empty() && requests < MAX_SEARCH_REQUESTS {
        level.truncate(MAX_SEARCH_REQUESTS - requests);
        let first_round = requests == 0;
        requests += level.len();
        let mut next = Vec::new();
        for (visit, feed) in level.iter().zip(fetch_feeds(&level, login)) {
            let feed = match feed {
                Ok(Some(feed)) => feed,
                // la ricerca stessa non risponde: lo si dice; una serie, si salta
                Err(e) if first_round => return Err(e),
                _ => continue,
            };
            found.add(&feed, visit.series.as_deref(), None);
            if let Some(url) = feed.next.clone().filter(|u| seen.insert(u.clone())) {
                next.push(Visit { url, ..visit.clone() });
            }
            // dai risultati si scende nelle serie, non oltre
            if visit.depth > 0 {
                continue;
            }
            for e in feed.entries.iter().filter(|e| e.stream.is_none()) {
                if series < MAX_SEARCH_SERIES
                    && let Some(url) = e.nav.clone().filter(|u| seen.insert(u.clone()))
                {
                    series += 1;
                    next.push(Visit { origin: url.clone(), url, series: Some(e.title.clone()), depth: 1 });
                }
            }
        }
        level = next;
    }
    remember_links(&found.links);
    Ok(found.entries)
}

/// L'indirizzo di ricerca del server, con `{searchTerms}`: dalla radice del
/// catalogo, o dalla descrizione OpenSearch a cui la radice rimanda.
pub(super) fn search_template(server: &Server) -> Result<Option<String>, String> {
    {
        let known = SEARCH.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, template)) = known.iter().find(|(s, _)| s == server) {
            return Ok(template.clone());
        }
    }
    let login = server.login();
    let (_, root) = root(server.url.trim(), login)?;
    let template = match root.search {
        Some(href) if href.contains(SEARCH_TERMS) => Some(href),
        Some(href) => opensearch_template(&fetch(&href, login, 30)?, &href),
        None => None,
    };
    SEARCH.lock().unwrap_or_else(|e| e.into_inner()).push((server.clone(), template.clone()));
    Ok(template)
}

/// L'indirizzo di ricerca in una descrizione OpenSearch: quello che da' un
/// feed Atom (il catalogo), se ce n'e' piu' d'uno.
pub(super) fn opensearch_template(xml: &[u8], base: &str) -> Option<String> {
    let mut r = quick_xml::Reader::from_reader(xml);
    let mut candidates: Vec<(bool, String)> = Vec::new();
    loop {
        match r.read_event().ok()? {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"Url" => {
                let (mut kind, mut template) = (String::new(), None);
                for a in e.attributes().flatten() {
                    let Ok(value) = a.normalized_value(XmlVersion::Implicit1_0) else { continue };
                    match a.key.local_name().as_ref() {
                        b"type" => kind = value.to_ascii_lowercase(),
                        b"template" => template = Some(value.into_owned()),
                        _ => {}
                    }
                }
                if let Some(t) = template.filter(|t| t.contains(SEARCH_TERMS)) {
                    candidates.push((kind.starts_with("application/atom+xml"), join(base, &t)));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    candidates.sort_by_key(|(atom, _)| !atom);
    candidates.into_iter().next().map(|(_, t)| t)
}

/// L'indirizzo della ricerca di `query`: le parole al posto di
/// `{searchTerms}`, i parametri facoltativi (`{startPage?}`) tolti.
pub(super) fn fill_search(template: &str, query: &str) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}').map(|c| open + c) else { break };
        out.push_str(&rest[..open]);
        let name = &rest[open..=close];
        if name == SEARCH_TERMS {
            out.push_str(&percent_encode(query));
        } else if !name.ends_with("?}") {
            out.push_str(name);
        }
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
}

/// Le parole cercate dentro un indirizzo: tutto tranne lettere, cifre e
/// `-_.~` diventa `%XX` (in UTF-8).
pub(super) fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
