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
//!
//! Il punto di lettura si tiene uguale a quello del server (vedi `sync`), e
//! un volume si puo' scaricare per leggerlo senza rete (vedi `offline`).

mod crawl;
mod feed;
mod http;
pub mod offline;
mod search;
pub mod sync;
#[cfg(test)]
mod tests;

use crawl::{Found, Visit, crawl, fetch_feeds, root};
use feed::{Feed, parse_feed};
pub use http::fetch;
use http::{Request, curl, join, origin};
pub use search::search;

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

use crate::library::{Entry, Group, GroupKind, parse_series};
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

/// Le voci della radice che portano ai gruppi, per tipo: l'id (Komga, Kavita)
/// o la fine dell'indirizzo. Le raccolte e gli editori elencano serie, le
/// liste di lettura volumi, in ordine.
const GROUP_ROOTS: &[(GroupKind, &[&str], &[&str])] = &[
    (GroupKind::Collection, &["allCollections", "collections"], &["/collections"]),
    (GroupKind::ReadingList, &["allReadLists", "readingList"], &["/readlists", "/reading-list"]),
    (GroupKind::Publisher, &["allPublishers"], &["/publishers"]),
];
/// Quanti gruppi per tipo, e quante pagine per feed, al massimo.
const MAX_GROUPS: usize = 300;
const MAX_PAGES: usize = 50;

/// Il segnaposto delle parole cercate negli indirizzi di ricerca (OpenSearch).
const SEARCH_TERMS: &str = "{searchTerms}";
/// Una ricerca sul server: quante serie trovate si aprono per prenderne i
/// volumi, e quante richieste al massimo in tutto.
const MAX_SEARCH_SERIES: usize = 30;
const MAX_SEARCH_REQUESTS: usize = 120;

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

/// Cio' che il catalogo dice di ogni volume oltre alle pagine, per indirizzo
/// delle pagine: la copertina gia' pronta (piu' leggera della prima pagina) e
/// il file intero da scaricare.
static LINKS: Mutex<Option<HashMap<String, Links>>> = Mutex::new(None);

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Links {
    pub cover: Option<String>,
    pub download: Option<String>,
}

/// I cataloghi letti di recente.
static CRAWLED: Mutex<Vec<(Server, Catalog)>> = Mutex::new(Vec::new());

/// L'indirizzo di ricerca di ogni server (con `{searchTerms}`), una volta
/// trovato; `None`: il server non offre la ricerca.
static SEARCH: Mutex<Vec<(Server, Option<String>)>> = Mutex::new(Vec::new());

/// Cio' che si sa di un server: i volumi, dove ciascuno e' arrivato secondo il
/// server, copertine e file.
#[derive(Clone, Debug)]
pub struct Catalog {
    pub entries: Vec<Entry>,
    pub reads: Vec<sync::Read>,
    pub links: HashMap<String, Links>,
    /// Quando lo si e' letto.
    pub fetched: Instant,
    /// Il server non ha risposto (il perche'): e' il catalogo dell'ultima
    /// volta, tenuto su disco, per leggere i volumi scaricati.
    pub stale: Option<String>,
}

fn remember_links(links: &HashMap<String, Links>) {
    let mut all = LINKS.lock().unwrap_or_else(|e| e.into_inner());
    all.get_or_insert_with(HashMap::new).extend(links.iter().map(|(k, v)| (k.clone(), v.clone())));
}

fn links_of(path: &Path) -> Option<Links> {
    let v = Volume::from_path(path)?;
    LINKS.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.get(&v.template).cloned()
}

/// Il file intero del volume sul server, se il catalogo lo offre.
pub fn download_url(path: &Path) -> Option<String> {
    links_of(path)?.download
}

/// Komga, Kavita o un altro server: cambia come si chiede una pagina senza
/// toccare il punto di lettura, e come lo si scrive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Flavor {
    Komga { book: String },
    Kavita { key: String, chapter: String },
    Other,
}

impl Flavor {
    pub(crate) fn of(template: &str) -> Flavor {
        if let Some((_, rest)) = template.split_once("/opds/v1.2/books/")
            && let Some((book, _)) = rest.split_once('/')
        {
            return Flavor::Komga { book: book.to_owned() };
        }
        if let Some((_, rest)) = template.split_once("/api/opds/")
            && let Some((key, rest)) = rest.split_once('/')
            && rest.starts_with("image?")
            && let Some(chapter) = query_value(rest, "chapterId")
        {
            return Flavor::Kavita { key: key.to_owned(), chapter: chapter.to_owned() };
        }
        Flavor::Other
    }
}

/// Il valore di un parametro nell'indirizzo ("chapterId=4").
fn query_value<'a>(url: &'a str, name: &str) -> Option<&'a str> {
    let query = url.split_once('?')?.1;
    query.split('&').find_map(|kv| kv.strip_prefix(name)?.strip_prefix('='))
}

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
        let login = login_for(&self.template);
        let login = login.as_ref().map(|(u, p)| (u.as_str(), p.as_str()));
        let max_time = if quick { 20 } else { 90 };
        // Kavita segna come punto di lettura ogni pagina chiesta dal catalogo:
        // la lettura anticipata e le miniature lo sposterebbero avanti e
        // indietro. Il suo lettore le da' uguali senza segnare niente
        if let Flavor::Kavita { key, chapter } = Flavor::of(&self.template) {
            let quiet =
                format!("{}/api/reader/image?chapterId={chapter}&apiKey={key}&page={index}", origin(&self.template));
            match fetch(&quiet, login, max_time) {
                Ok(bytes) => return Ok(bytes),
                // un Kavita che non lo conosce: si chiede dal catalogo
                Err(e) if e.contains("error: 4") => {}
                Err(e) => return Err(e),
            }
        }
        fetch(&self.page_url(index), login, max_time)
    }
}

/// L'indirizzo della copertina gia' pronta sul server, se il catalogo ne ha
/// una per questo volume.
pub fn cover_url(path: &Path) -> Option<String> {
    links_of(path)?.cover
}

/// I byte della copertina gia' pronta sul server.
pub fn cover(path: &Path) -> Option<Vec<u8>> {
    let url = cover_url(path)?;
    let login = login_for(&url);
    fetch(&url, login.as_ref().map(|(u, p)| (u.as_str(), p.as_str())), 30).ok()
}

/// I volumi di un server, come voci della libreria. Il catalogo letto da meno
/// di cinque minuti non si rilegge. Se il server non risponde, vale quello
/// dell'ultima volta (`stale`), tenuto su disco: i volumi scaricati si
/// leggono anche fuori casa.
pub fn volumes(server: &Server) -> Result<Catalog, String> {
    {
        let crawled = CRAWLED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, c)) = crawled.iter().find(|(s, c)| s == server && c.fetched.elapsed() < FRESH) {
            return Ok(c.clone());
        }
    }
    let catalog = match crawl(server) {
        Ok(catalog) => catalog,
        Err(e) => {
            let mut old = offline::load_catalog(server).ok_or(e.clone())?;
            old.stale = Some(e);
            remember_links(&old.links);
            return Ok(old);
        }
    };
    remember_links(&catalog.links);
    offline::save_catalog(server, &catalog);
    let mut crawled = CRAWLED.lock().unwrap_or_else(|e| e.into_inner());
    crawled.retain(|(s, _)| s != server);
    crawled.push((server.clone(), catalog.clone()));
    Ok(catalog)
}

impl Server {
    /// Nome e password per curl, se il server li chiede.
    fn login(&self) -> Option<(&str, &str)> {
        (!self.user.is_empty()).then_some((self.user.as_str(), self.password.as_str()))
    }
}
