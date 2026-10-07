//! I server di fumetti: Komga, Kavita e gli altri che offrono un catalogo
//! OPDS con le pagine una per una (OPDS-PSE, "Page Streaming Extension").
//!
//! Il catalogo e' un albero di feed Atom: dalla radice si scende alle serie, e
//! da li' ai volumi. Ogni volume ha un indirizzo con dentro `{pageNumber}`: le
//! pagine si chiedono una alla volta (contando da 0) senza scaricare
//! l'archivio, e si legge subito anche un volume da 300 MB.
//!
//! Come per le versioni nuove, le richieste le fa curl. Nome e password non
//! vanno sulla riga di comando (la vedono gli altri programmi): curl li legge
//! dallo stdin, nel formato dei suoi file di configurazione.
//!
//! Un volume remoto ha un "percorso" come gli altri, perche' progressi,
//! copertine e libreria lo trattino come un file: l'indirizzo delle pagine,
//! poi `#`, il numero di pagine, `:` e il titolo
//! (`http://casa:25600/opds/v1.2/books/0RV8/pages/{pageNumber}#6:Volume 1`).
//! Nome e password restano nelle impostazioni, non nel percorso.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

use quick_xml::events::{BytesStart, Event};
use quick_xml::name::{Namespace, ResolveResult};
use quick_xml::{NsReader, XmlVersion};
use serde::{Deserialize, Serialize};

use crate::library::{Entry, parse_series};
use crate::lingua::t;

const ATOM: &[u8] = b"http://www.w3.org/2005/Atom";
const PSE: &[u8] = b"http://vaemendis.net/opds-pse/ns";
const STREAM: &str = "http://vaemendis.net/opds-pse/stream";
const PAGE: &str = "{pageNumber}";

/// Dove comincia la ricerca dei volumi: la lista di tutte le serie (Komga) o
/// quella delle librerie (Kavita). Le altre voci della radice ("in lettura",
/// "aggiunti di recente", le collezioni) ripetono gli stessi volumi.
const STARTS: &[&str] = &["allSeries", "allLibraries"];
/// Quanti feed si scende al massimo dalla radice (le pagine successive dello
/// stesso feed non contano), quante richieste in tutto, quante insieme.
const MAX_DEPTH: usize = 5;
const MAX_REQUESTS: usize = 5000;
const PARALLEL: usize = 6;
/// Per quanto vale il catalogo gia' letto: aprire e chiudere la libreria non
/// rilegge ogni volta tutte le serie del server.
const FRESH: Duration = Duration::from_secs(5 * 60);

/// Un server nella libreria.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Server {
    /// L'indirizzo del catalogo OPDS (per Kavita, con dentro la chiave), o
    /// solo quello del server per Komga.
    pub url: String,
    /// Nome e password, se il server li chiede (Komga si'; Kavita no, la
    /// chiave e' nell'indirizzo).
    pub user: String,
    pub password: String,
}

/// I server conosciuti, per sapere a chi dare nome e password quando si
/// chiede una pagina.
static SERVERS: RwLock<Vec<Server>> = RwLock::new(Vec::new());

/// Le copertine gia' pronte sul server (piu' leggere della prima pagina), per
/// indirizzo delle pagine. Le trova la lettura del catalogo.
static COVERS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// I cataloghi letti di recente.
static CRAWLED: Mutex<Vec<(Server, Instant, Vec<Entry>)>> = Mutex::new(Vec::new());

/// Da chiamare all'avvio e a ogni cambio dei server nelle impostazioni.
pub fn set_servers(servers: &[Server]) {
    *SERVERS.write().unwrap_or_else(|e| e.into_inner()) = servers.to_vec();
}

fn login_for(url: &str) -> Option<(String, String)> {
    let servers = SERVERS.read().unwrap_or_else(|e| e.into_inner());
    servers
        .iter()
        .find(|s| !s.user.is_empty() && origin(&s.url).eq_ignore_ascii_case(origin(url)))
        .map(|s| (s.user.clone(), s.password.clone()))
}

/// L'indirizzo di un server da mostrare: senza la chiave che Kavita mette
/// nell'indirizzo (chi guarda lo schermo non deve poterla copiare).
pub fn shown(url: &str) -> String {
    let start = origin(url).len();
    let masked: Vec<&str> = url[start..]
        .split('/')
        .map(|part| {
            let key = part.len() >= 20 && part.chars().all(|c| c.is_ascii_alphanumeric());
            if key { "\u{2022}\u{2022}\u{2022}\u{2022}" } else { part }
        })
        .collect();
    format!("{}{}", &url[..start], masked.join("/"))
}

/// Un volume sta su un server (il suo percorso e' un indirizzo web).
pub fn is_remote(path: &Path) -> bool {
    path.to_str().is_some_and(|s| starts_with_ignore_case(s, "http://") || starts_with_ignore_case(s, "https://"))
}

fn starts_with_ignore_case(s: &str, prefix: &str) -> bool {
    s.get(..prefix.len()).is_some_and(|h| h.eq_ignore_ascii_case(prefix))
}

/// Un volume su un server: l'indirizzo delle pagine, quante sono, il titolo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Volume {
    pub template: String,
    pub count: usize,
    pub title: String,
}

impl Volume {
    /// Il percorso che lo rappresenta nella libreria e nei progressi.
    pub fn path(&self) -> PathBuf {
        PathBuf::from(format!("{}#{}:{}", self.template, self.count, self.title))
    }

    /// Il volume dal suo percorso; `None` se non e' un volume remoto.
    pub fn from_path(path: &Path) -> Option<Volume> {
        if !is_remote(path) {
            return None;
        }
        // nell'indirizzo il # non c'e' (e' la fine dell'indirizzo); nel titolo si'
        let (template, rest) = path.to_str()?.split_once('#')?;
        let (count, title) = rest.split_once(':')?;
        let count = count.parse().ok().filter(|&n| n > 0)?;
        template.contains(PAGE).then(|| Volume { template: template.to_owned(), count, title: title.to_owned() })
    }

    /// L'indirizzo della pagina `index` (da 0).
    pub fn page_url(&self, index: usize) -> String {
        self.template.replace(PAGE, &index.to_string())
    }

    /// I byte della pagina `index`. `quick`: per sapere se il server c'e', si
    /// aspetta poco.
    pub fn page(&self, index: usize, quick: bool) -> Result<Vec<u8>, String> {
        let url = self.page_url(index);
        let login = login_for(&url);
        fetch(&url, login.as_ref().map(|(u, p)| (u.as_str(), p.as_str())), if quick { 20 } else { 90 })
    }
}

/// L'indirizzo della copertina gia' pronta sul server, se il catalogo ne ha
/// una per questo volume.
pub fn cover_url(path: &Path) -> Option<String> {
    let v = Volume::from_path(path)?;
    COVERS.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.get(&v.template).cloned()
}

/// I byte della copertina gia' pronta sul server.
pub fn cover(path: &Path) -> Option<Vec<u8>> {
    let url = cover_url(path)?;
    let login = login_for(&url);
    fetch(&url, login.as_ref().map(|(u, p)| (u.as_str(), p.as_str())), 30).ok()
}

/// I volumi di un server, come voci della libreria. Il catalogo letto da meno
/// di cinque minuti non si rilegge.
pub fn volumes(server: &Server) -> Result<Vec<Entry>, String> {
    {
        let crawled = CRAWLED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, _, found)) = crawled.iter().find(|(s, at, _)| s == server && at.elapsed() < FRESH) {
            return Ok(found.clone());
        }
    }
    let found = crawl(server)?;
    let mut crawled = CRAWLED.lock().unwrap_or_else(|e| e.into_inner());
    crawled.retain(|(s, _, _)| s != server);
    crawled.push((server.clone(), Instant::now(), found.clone()));
    Ok(found)
}

/// Legge il catalogo, dalla radice ai volumi.
fn crawl(server: &Server) -> Result<Vec<Entry>, String> {
    let login = (!server.user.is_empty()).then_some((server.user.as_str(), server.password.as_str()));
    let (root_url, root) = root(server.url.trim(), login)?;
    let mut found = Found::default();
    found.add(&root, None);
    // la lista di tutte le serie, se c'e'; altrimenti ogni voce della radice
    let start: Vec<String> = match STARTS.iter().find_map(|id| root.entries.iter().find(|e| e.id == *id)) {
        Some(e) => e.nav.iter().cloned().collect(),
        None => root.entries.iter().filter_map(|e| e.nav.clone()).collect(),
    };
    let mut seen: HashSet<String> = HashSet::from([root_url]);
    let mut level: Vec<Visit> =
        start.into_iter().filter(|u| seen.insert(u.clone())).map(|url| Visit { url, series: None, depth: 1 }).collect();
    let mut requests = 1;
    let mut error = None;
    while !level.is_empty() && requests < MAX_REQUESTS {
        level.truncate(MAX_REQUESTS - requests);
        requests += level.len();
        let feeds = fetch_feeds(&level, login);
        let mut next = Vec::new();
        for (visit, feed) in level.iter().zip(feeds) {
            let feed = match feed {
                Ok(Some(feed)) => feed,
                Ok(None) => continue,
                Err(e) => {
                    error.get_or_insert(e);
                    continue;
                }
            };
            found.add(&feed, visit.series.as_deref());
            // la pagina dopo dello stesso feed: stessa serie, stessa profondita'
            if let Some(url) = feed.next.clone().filter(|u| seen.insert(u.clone())) {
                next.push(Visit { url, ..visit.clone() });
            }
            if visit.depth >= MAX_DEPTH {
                continue;
            }
            for e in &feed.entries {
                if let Some(url) = e.nav.clone().filter(|u| e.stream.is_none() && seen.insert(u.clone())) {
                    next.push(Visit { url, series: Some(e.title.clone()), depth: visit.depth + 1 });
                }
            }
        }
        level = next;
    }
    if found.entries.is_empty()
        && let Some(e) = error
    {
        return Err(e);
    }
    let mut covers = COVERS.lock().unwrap_or_else(|e| e.into_inner());
    covers.get_or_insert_with(HashMap::new).extend(found.covers);
    Ok(found.entries)
}

/// Un feed da leggere: dove, e la serie a cui appartengono i suoi volumi (il
/// titolo della voce che ci ha portato li').
#[derive(Clone)]
struct Visit {
    url: String,
    series: Option<String>,
    depth: usize,
}

#[derive(Default)]
struct Found {
    entries: Vec<Entry>,
    /// Per indirizzo delle pagine: dove sta gia' il volume fra quelli trovati.
    at: HashMap<String, usize>,
    covers: HashMap<String, String>,
}

impl Found {
    fn add(&mut self, feed: &Feed, series: Option<&str>) {
        for e in &feed.entries {
            let Some((template, count)) = &e.stream else { continue };
            let title = clean_title(&e.title, series);
            let entry = remote_entry(template, *count, title, series, &e.authors);
            if let Some(cover) = &e.cover {
                self.covers.insert(template.clone(), cover.clone());
            }
            match self.at.get(template) {
                // lo stesso volume due volte (Kavita mette in cima "Continue
                // Reading from: ..."): vale il titolo piu' corto
                Some(&i) => {
                    if entry.title.len() < self.entries[i].title.len() {
                        self.entries[i] = entry;
                    }
                }
                None => {
                    self.at.insert(template.clone(), self.entries.len());
                    self.entries.push(entry);
                }
            }
        }
    }
}

fn remote_entry(template: &str, count: usize, title: String, series: Option<&str>, authors: &[String]) -> Entry {
    let path = Volume { template: template.to_owned(), count, title: title.clone() }.path();
    let (name, number) = parse_series(&title);
    let (series, number) = match series {
        // nella serie del catalogo, "Volume 3" e' il terzo
        Some(s) => (Some(s.to_owned()), number.or_else(|| first_number(&title))),
        // un catalogo senza serie: come per i file, la serie viene dal nome
        None => (number.map(|_| name), number),
    };
    let authors = (!authors.is_empty()).then(|| authors.join(", "));
    Entry { path, title, series, number, authors }
}

/// Il primo numero nel titolo ("Volume 3", "Capitolo 12 - Il ritorno").
fn first_number(title: &str) -> Option<u32> {
    let start = title.find(|c: char| c.is_ascii_digit())?;
    let digits: String = title[start..].chars().take_while(char::is_ascii_digit).take(6).collect();
    digits.parse().ok()
}

/// Il titolo come lo si mostra: senza il segno di Kavita davanti (letto,
/// non letto, a meta') e senza il nome della serie, che la libreria mostra
/// gia' ("Nebbia sul Porto - Volume 1" e' "Volume 1" nella serie).
fn clean_title(title: &str, series: Option<&str>) -> String {
    let mut s = title.trim();
    if let Some((mark, rest)) = s.split_once(' ')
        && mark.chars().count() == 1
        && mark.chars().all(|c| matches!(c, '\u{25a0}'..='\u{25ff}' | '\u{2b00}'..='\u{2bff}'))
    {
        s = rest.trim_start();
    }
    if let Some(series) = series
        && let Some(rest) = s.strip_prefix(series)
        && let Some(rest) = rest.trim_start().strip_prefix(['-', '\u{2013}', ':'])
        && !rest.trim().is_empty()
    {
        s = rest.trim();
    }
    s.to_owned()
}

/// La radice del catalogo. Per Komga basta l'indirizzo del server: se li'
/// non c'e' un catalogo, si prova dove Komga lo tiene.
fn root(url: &str, login: Option<(&str, &str)>) -> Result<(String, Feed), String> {
    let not_opds =
        || t("a questo indirizzo non c'\u{e8} un catalogo OPDS", "there is no OPDS catalog at this address").to_owned();
    if !is_remote(Path::new(url)) {
        return Err(t(
            "l'indirizzo deve cominciare con http:// o https://",
            "the address must start with http:// or https://",
        )
        .into());
    }
    if let Some(feed) = parse_feed(&fetch(url, login, 30)?, url) {
        return Ok((url.to_owned(), feed));
    }
    let komga = format!("{}/opds/v1.2/catalog", url.trim_end_matches('/'));
    match fetch(&komga, login, 30) {
        Ok(bytes) => parse_feed(&bytes, &komga).map(|feed| (komga, feed)).ok_or_else(not_opds),
        // la pagina di Komga si apre a tutti, il catalogo no: qui si scopre
        // se nome e password vanno bene
        Err(e) if e.contains("error: 401") || e.contains("error: 403") => Err(e),
        Err(_) => Err(not_opds()),
    }
}

/// Un feed chiesto: `None` se la risposta non era un feed.
type Fetched = Result<Option<Feed>, String>;

/// Molti feed insieme, `PARALLEL` richieste alla volta.
fn fetch_feeds(visits: &[Visit], login: Option<(&str, &str)>) -> Vec<Fetched> {
    let next = AtomicUsize::new(0);
    let results: Vec<Mutex<Option<Fetched>>> = visits.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|s| {
        for _ in 0..PARALLEL.min(visits.len()) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(v) = visits.get(i) else { break };
                    let feed = fetch(&v.url, login, 60).map(|bytes| parse_feed(&bytes, &v.url));
                    *results[i].lock().unwrap_or_else(|e| e.into_inner()) = Some(feed);
                }
            });
        }
    });
    results.into_iter().map(|r| r.into_inner().unwrap_or_else(|e| e.into_inner()).unwrap_or(Ok(None))).collect()
}

/// Chiede un indirizzo con curl. `max_time`: i secondi oltre i quali si
/// rinuncia.
pub fn fetch(url: &str, login: Option<(&str, &str)>, max_time: u32) -> Result<Vec<u8>, String> {
    let mut config = format!("url = {}\n", quoted(url));
    if let Some((user, password)) = login {
        config += &format!("user = {}\n", quoted(&format!("{user}:{password}")));
    }
    let mut curl = Command::new("curl");
    // solo http e https, anche dopo un rinvio: un catalogo non puo' far
    // leggere a curl un file del computer o parlare altri protocolli
    curl.args(["--config", "-", "--globoff", "--location", "--fail", "--silent", "--show-error"])
        .args(["--proto", "=http,https", "--proto-redir", "=http,https"])
        .args(["--connect-timeout", "10", "--max-time", &max_time.to_string()])
        .args(["--user-agent", concat!("NicoReader/", env!("CARGO_PKG_VERSION"))])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::upscale::hide_window(&mut curl);
    let mut child = curl.spawn().map_err(|e| format!("curl: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        // curl legge tutta la configurazione prima di cominciare; chiuso lo
        // stdin (qui, alla fine del blocco) sa che non c'e' altro
        let _ = stdin.write_all(config.as_bytes());
    }
    let out = child.wait_with_output().map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(explain(&String::from_utf8_lossy(&out.stderr)));
    }
    Ok(out.stdout)
}

/// Un valore fra virgolette per la configurazione di curl.
fn quoted(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '\n' | '\r' => {}
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// L'errore di curl detto per chi legge.
fn explain(stderr: &str) -> String {
    let raw = stderr.trim().trim_start_matches("curl: ").to_owned();
    let code = |c: &str| raw.contains(&format!("error: {c}"));
    let text = if code("401") || code("403") {
        t("nome o password sbagliati", "wrong user name or password")
    } else if code("404") {
        t("non trovato sul server", "not found on the server")
    } else if raw.starts_with("(6)") {
        t("indirizzo sconosciuto", "unknown address")
    } else if raw.starts_with("(7)") {
        t("il server non risponde: \u{e8} acceso?", "the server doesn't answer: is it on?")
    } else if raw.starts_with("(28)") {
        t("il server ci mette troppo a rispondere", "the server takes too long to answer")
    } else {
        return raw;
    };
    // "(7) Failed to connect...": il numero di curl non dice niente a chi legge
    let detail = raw.split_once(") ").filter(|(code, _)| code.starts_with('(')).map_or(raw.as_str(), |(_, d)| d);
    format!("{text} ({detail})")
}

/// "http://casa:8080/opds?x" -> "http://casa:8080".
fn origin(url: &str) -> &str {
    let after = url.find("://").map_or(0, |i| i + 3);
    let end = url[after..].find(['/', '?', '#']).map_or(url.len(), |i| after + i);
    &url[..end]
}

/// Un indirizzo del feed (anche relativo, come li da' Kavita) reso completo.
fn join(base: &str, href: &str) -> String {
    let href = href.trim();
    if is_remote(Path::new(href)) {
        return href.to_owned();
    }
    if let Some(rest) = href.strip_prefix("//") {
        let scheme = base.split_once("://").map_or("http", |(s, _)| s);
        return format!("{scheme}://{rest}");
    }
    if href.starts_with('/') {
        return format!("{}{href}", origin(base));
    }
    let base = base.split(['#']).next().unwrap_or(base);
    if href.starts_with('?') {
        return format!("{}{href}", base.split('?').next().unwrap_or(base));
    }
    let path = base.split('?').next().unwrap_or(base);
    let dir =
        if path.len() > origin(path).len() { &path[..path.rfind('/').map_or(path.len(), |i| i + 1)] } else { path };
    let sep = if dir.ends_with('/') { "" } else { "/" };
    format!("{dir}{sep}{}", href.trim_start_matches("./"))
}

/// Un feed del catalogo: le voci, e la pagina dopo se la lista continua.
#[derive(Debug, Default)]
struct Feed {
    entries: Vec<FeedEntry>,
    next: Option<String>,
}

#[derive(Debug, Default, PartialEq)]
struct FeedEntry {
    id: String,
    title: String,
    authors: Vec<String>,
    /// Un feed piu' in giu': una serie, una libreria.
    nav: Option<String>,
    /// Le pagine: l'indirizzo con `{pageNumber}` e quante sono.
    stream: Option<(String, usize)>,
    cover: Option<String>,
}

/// Legge un feed Atom; `None` se non lo e' (una pagina web, un errore in
/// JSON). Gli indirizzi diventano completi, rispetto a `base`.
fn parse_feed(xml: &[u8], base: &str) -> Option<Feed> {
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
            Event::GeneralRef(g) => match g.resolve_char_ref() {
                Ok(Some(c)) => buf.push(c),
                _ => buf.push_str(match g.decode().ok()?.as_ref() {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "apos" => "'",
                    _ => "",
                }),
            },
            Event::End(_) => {
                let (atom, name) = open.pop()?;
                let text = std::mem::take(&mut buf);
                let parent = open.last().filter(|p| p.0).map(|p| p.1.as_slice());
                match (&mut entry, atom, name.as_slice(), parent) {
                    (Some(e), true, b"title", Some(b"entry")) => e.title = text.trim().to_owned(),
                    (Some(e), true, b"id", Some(b"entry")) => e.id = text.trim().to_owned(),
                    (Some(e), true, b"name", Some(b"author")) if open.len() == 3 => {
                        if !text.trim().is_empty() {
                            e.authors.push(text.trim().to_owned());
                        }
                    }
                    (Some(_), true, b"entry", Some(b"feed")) => feed.entries.extend(entry.take()),
                    _ => {}
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

/// Un `<link>`: dove porta, se e' nel feed o in una voce.
fn link(r: &NsReader<&[u8]>, e: &BytesStart, base: &str, entry: Option<&mut FeedEntry>, feed: &mut Feed) {
    let (mut rel, mut href, mut kind, mut count) = (String::new(), None, String::new(), None);
    for a in e.attributes().flatten() {
        let (ns, local) = r.resolver().resolve_attribute(a.key);
        let Ok(value) = a.normalized_value(XmlVersion::Implicit1_0) else { continue };
        match ns {
            ResolveResult::Bound(Namespace(n)) if n == PSE && local.as_ref() == b"count" => {
                count = value.trim().parse::<usize>().ok()
            }
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
        if rel == "next" {
            feed.next = Some(href);
        }
        return;
    };
    match rel.as_str() {
        STREAM => {
            if let Some(n) = count.filter(|&n| n > 0 && href.contains(PAGE)) {
                e.stream = Some((href, n));
            }
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

#[cfg(test)]
mod tests {
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
    <link rel="http://vaemendis.net/opds-pse/stream" type="image/jpeg" href="/api/opds/KEY/image?libraryId=1&amp;seriesId=1&amp;volumeId=1&amp;chapterId=1&amp;pageNumber={pageNumber}" p5:count="6" p5:lastRead="6" xmlns:p5="http://vaemendis.net/opds-pse/ns" />
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

        let mut found = Found::default();
        found.add(&feed, Some("Nebbia sul Porto"));
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
            explain("curl: (7) Failed to connect to casa port 25600")
                .ends_with("(Failed to connect to casa port 25600)")
        );
        assert_eq!(explain("curl: (35) TLS"), "(35) TLS");
        assert_eq!(quoted(r#"io:pa"ss\"#), r#""io:pa\"ss\\""#);
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
        let found = volumes(&server).unwrap();
        for e in &found {
            println!("{:?} / {} ({:?})", e.series, e.title, e.number);
        }
        assert!(!found.is_empty());
        let v = Volume::from_path(&found[0].path).unwrap();
        let page = v.page(0, true).unwrap();
        assert!(crate::decode::dimensions(&page).is_some(), "la prima pagina e' un'immagine");
        assert!(cover(&found[0].path).is_some_and(|c| !c.is_empty()));
    }
}
