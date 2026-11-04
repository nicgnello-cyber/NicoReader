//! La lettura del catalogo: dalla radice alle serie e ai volumi, piu' feed
//! insieme, con i gruppi (raccolte, liste, editori).

use super::*;

/// Legge il catalogo, dalla radice ai volumi.
pub(super) fn crawl(server: &Server) -> Result<Catalog, String> {
    let login = server.login();
    let (root_url, root) = root(server.url.trim(), login)?;
    let mut found = Found::default();
    found.add(&root, None, None);
    let mut seen: HashSet<String> = HashSet::from([root_url]);
    let mut level: Vec<Visit> = start_urls(&root)
        .into_iter()
        .filter(|u| seen.insert(u.clone()))
        .map(|url| Visit { origin: url.clone(), url, series: None, depth: 1 })
        .collect();
    let (mut requests, mut error) = (1, None);
    while !level.is_empty() && requests < MAX_REQUESTS {
        level.truncate(MAX_REQUESTS - requests);
        requests += level.len();
        let mut next = Vec::new();
        for (visit, feed) in level.iter().zip(fetch_feeds(&level, login)) {
            match feed {
                Ok(Some(feed)) => {
                    found.add(&feed, visit.series.as_deref(), Some(&visit.origin));
                    next.extend(further(visit, &feed, &mut seen));
                }
                Ok(None) => {}
                Err(e) => {
                    error.get_or_insert(e);
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
    found.read_groups(&root, login);
    let reads = found.known_reads();
    Ok(Catalog { entries: found.entries, reads, links: found.links, fetched: Instant::now(), stale: None })
}

/// Da dove si comincia: la lista di tutte le serie, se c'e'; altrimenti ogni
/// voce della radice.
fn start_urls(root: &Feed) -> Vec<String> {
    match STARTS.iter().find_map(|id| root.entries.iter().find(|e| e.id == *id)) {
        Some(e) => e.nav.iter().cloned().collect(),
        None => root.entries.iter().filter_map(|e| e.nav.clone()).collect(),
    }
}

/// Dove andare dopo un feed: la sua pagina dopo (stessa serie, stessa
/// profondita') e, se non si e' gia' troppo in fondo, le voci che portano a
/// un altro feed (non i volumi). Ogni indirizzo una volta sola.
fn further(visit: &Visit, feed: &Feed, seen: &mut HashSet<String>) -> Vec<Visit> {
    let mut next = Vec::new();
    if let Some(url) = feed.next.clone().filter(|u| seen.insert(u.clone())) {
        next.push(Visit { url, ..visit.clone() });
    }
    if visit.depth >= MAX_DEPTH {
        return next;
    }
    for e in &feed.entries {
        if let Some(url) = e.nav.clone().filter(|u| e.stream.is_none() && seen.insert(u.clone())) {
            let series = Some(e.title.clone());
            next.push(Visit { origin: url.clone(), url, series, depth: visit.depth + 1 });
        }
    }
    next
}

/// Un feed da leggere: dove, e la serie a cui appartengono i suoi volumi (il
/// titolo della voce che ci ha portato li').
#[derive(Clone)]
pub(super) struct Visit {
    pub(super) url: String,
    /// La prima pagina del feed: e' l'indirizzo con cui lo nominano i gruppi.
    pub(super) origin: String,
    pub(super) series: Option<String>,
    pub(super) depth: usize,
}

#[derive(Default)]
pub(super) struct Found {
    pub(super) entries: Vec<Entry>,
    /// Per indirizzo delle pagine: dove sta gia' il volume fra quelli trovati.
    pub(super) at: HashMap<String, usize>,
    pub(super) links: HashMap<String, Links>,
    /// Dove e' arrivato chi legge, secondo il server: pagina (come la dice
    /// lui) e quando.
    pub(super) reads: HashMap<String, (u32, Option<u64>)>,
    /// Per feed (la sua prima pagina): i volumi che ci si sono trovati. Una
    /// raccolta nomina le serie con l'indirizzo del loro feed.
    pub(super) by_feed: HashMap<String, Vec<usize>>,
}

impl Found {
    /// Il punto di lettura che il server dice, per i volumi trovati.
    fn known_reads(&self) -> Vec<sync::Read> {
        self.entries
            .iter()
            .filter_map(|e| {
                let v = Volume::from_path(&e.path)?;
                let &(last_read, date) = self.reads.get(&v.template)?;
                Some(sync::Read { path: e.path.clone(), last_read, date })
            })
            .collect()
    }

    pub(super) fn add(&mut self, feed: &Feed, series: Option<&str>, origin: Option<&str>) {
        for e in &feed.entries {
            let Some((template, count)) = &e.stream else { continue };
            let title = clean_title(&e.title, series);
            let entry = remote_entry(template, *count, title, series, &e.authors);
            if e.cover.is_some() || e.download.is_some() {
                let links = Links { cover: e.cover.clone(), download: e.download.clone() };
                self.links.insert(template.clone(), links);
            }
            self.reads.insert(template.clone(), (e.last_read.unwrap_or(0), e.read_date));
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
            if let (Some(origin), Some(&i)) = (origin, self.at.get(template)) {
                self.by_feed.entry(origin.to_owned()).or_default().push(i);
            }
        }
    }

    /// Le raccolte, le liste di lettura e gli editori del server (vedi
    /// `GROUP_ROOTS`), assegnati ai volumi gia' trovati. Quello che non
    /// risponde si salta: i volumi ci sono lo stesso.
    pub(super) fn read_groups(&mut self, root: &Feed, login: Option<(&str, &str)>) {
        for (kind, ids, ends) in GROUP_ROOTS {
            let entry = root.entries.iter().find(|e| {
                ids.contains(&e.id.as_str())
                    || e.nav.as_deref().is_some_and(|u| ends.iter().any(|s| u.trim_end_matches('/').ends_with(s)))
            });
            let Some(url) = entry.and_then(|e| e.nav.clone()) else { continue };
            let lists = pages(&url, login);
            let groups: Vec<(String, String)> = lists
                .iter()
                .flat_map(|f| &f.entries)
                .filter_map(|e| Some((e.title.clone(), e.nav.clone()?)))
                .take(MAX_GROUPS)
                .collect();
            let visits: Vec<Visit> = groups
                .iter()
                .map(|(_, u)| Visit { url: u.clone(), origin: u.clone(), series: None, depth: 0 })
                .collect();
            for ((name, _), first) in groups.iter().zip(fetch_feeds(&visits, login)) {
                let Ok(Some(first)) = first else { continue };
                let mut feeds = vec![first];
                // le pagine dopo, se il gruppo e' lungo
                while let Some(next) = feeds.last().and_then(|f| f.next.clone()).filter(|_| feeds.len() < MAX_PAGES) {
                    match fetch(&next, login, 60).ok().and_then(|b| parse_feed(&b, &next)) {
                        Some(f) => feeds.push(f),
                        None => break,
                    }
                }
                let mut position = 0;
                for e in feeds.iter().flat_map(|f| &f.entries) {
                    if let Some((template, _)) = &e.stream {
                        position += 1;
                        let order = (*kind == GroupKind::ReadingList).then_some(position);
                        if let Some(&i) = self.at.get(template) {
                            push_group(&mut self.entries[i], Group::new(*kind, name, order));
                        }
                    } else if let Some(members) = e.nav.as_ref().and_then(|n| self.by_feed.get(n)) {
                        for &i in members.clone().iter() {
                            push_group(&mut self.entries[i], Group::new(*kind, name, None));
                        }
                    }
                }
            }
        }
    }
}

pub(super) fn push_group(e: &mut Entry, g: Group) {
    if !e.groups.iter().any(|h| h.kind == g.kind && h.name == g.name) {
        e.groups.push(g);
    }
}

/// Un feed con tutte le sue pagine (fino a `MAX_PAGES`).
pub(super) fn pages(url: &str, login: Option<(&str, &str)>) -> Vec<Feed> {
    let mut out: Vec<Feed> = Vec::new();
    let mut next = Some(url.to_owned());
    while let Some(u) = next.take().filter(|_| out.len() < MAX_PAGES) {
        let Some(feed) = fetch(&u, login, 60).ok().and_then(|b| parse_feed(&b, &u)) else { break };
        next = feed.next.clone();
        out.push(feed);
    }
    out
}

pub(super) fn remote_entry(
    template: &str, count: usize, title: String, series: Option<&str>, authors: &[String],
) -> Entry {
    let path = Volume { template: template.to_owned(), count, title: title.clone() }.path();
    let (name, number) = parse_series(&title);
    let (series, number) = match series {
        // nella serie del catalogo, "Volume 3" e' il terzo
        Some(s) => (Some(s.to_owned()), number.or_else(|| first_number(&title))),
        // un catalogo senza serie: come per i file, la serie viene dal nome
        None => (number.map(|_| name), number),
    };
    let authors = (!authors.is_empty()).then(|| authors.join(", "));
    Entry { path, title, series, number, authors, ..Entry::default() }
}

/// Il primo numero nel titolo ("Volume 3", "Capitolo 12 - Il ritorno").
pub(super) fn first_number(title: &str) -> Option<u32> {
    let start = title.find(|c: char| c.is_ascii_digit())?;
    let digits: String = title[start..].chars().take_while(char::is_ascii_digit).take(6).collect();
    digits.parse().ok()
}

/// Il titolo come lo si mostra: senza il segno di Kavita davanti (letto,
/// non letto, a meta') e senza il nome della serie, che la libreria mostra
/// gia' ("Nebbia sul Porto - Volume 1" e' "Volume 1" nella serie).
pub(super) fn clean_title(title: &str, series: Option<&str>) -> String {
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
pub(super) fn root(url: &str, login: Option<(&str, &str)>) -> Result<(String, Feed), String> {
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
pub(super) type Fetched = Result<Option<Feed>, String>;

/// Molti feed insieme, `PARALLEL` richieste alla volta.
pub(super) fn fetch_feeds(visits: &[Visit], login: Option<(&str, &str)>) -> Vec<Fetched> {
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
