//! La ricerca sui server. La libreria ha gia' il catalogo intero, ma un
//! server enorme supera i limiti della lettura, e il server cerca anche dove
//! l'app non guarda (riassunti, etichette). Cosi', scrivendo nella libreria,
//! dopo una pausa si chiede anche ai server; i volumi che la libreria non ha
//! compaiono a parte, "Sul server", e si aprono come gli altri.

use super::*;

/// Quanto aspettare dopo l'ultimo tasto prima di chiedere.
const PAUSE: Duration = Duration::from_millis(600);
/// Con meno lettere non si chiede: i risultati sarebbero troppi.
const MIN_CHARS: usize = 2;

#[derive(Default)]
pub(super) struct ServerSearch {
    /// La ricerca scritta nella libreria, vista l'ultima volta.
    typed: String,
    /// Quando chiedere ai server: dopo la pausa.
    due: Option<Instant>,
    /// La ricerca chiesta, finche' la risposta non arriva.
    asked: Option<String>,
    /// La ricerca a cui rispondono `results`.
    answered: String,
    results: Vec<Entry>,
}

impl ServerSearch {
    /// Si aspetta la risposta per cio' che e' scritto.
    pub(super) fn waiting(&self) -> bool {
        self.due.is_some() || self.asked.as_ref().is_some_and(|q| *q == self.typed)
    }
}

impl App {
    /// Da `about_to_wait`: se la ricerca della libreria e' cambiata, i
    /// risultati vecchi se ne vanno e, dopo la pausa, si chiede ai server.
    /// Quando ricontrollare.
    pub(super) fn search_tick(&mut self, now: Instant) -> Option<Instant> {
        let typed = match self.shelf_shown() && !self.settings.servers.is_empty() {
            true => self.ui.shelf.search.trim().to_owned(),
            false => String::new(),
        };
        if typed != self.search.typed {
            self.search.due = (typed.chars().count() >= MIN_CHARS).then_some(now + PAUSE);
            self.search.typed = typed;
            if !self.search.results.is_empty() {
                self.search.results.clear();
                self.search.answered.clear();
                self.show_found();
            }
        }
        let due = self.search.due?;
        if now < due {
            return Some(due);
        }
        self.search.due = None;
        self.ask_servers(self.search.typed.clone());
        None
    }

    /// Chiede a tutti i server, in sottofondo.
    fn ask_servers(&mut self, query: String) {
        self.search.asked = Some(query.clone());
        let (servers, proxy) = (self.settings.servers.clone(), self.proxy.clone());
        let started = std::thread::Builder::new().name("ricerca".into()).spawn(move || {
            let (mut found, mut failed) = (Vec::new(), Vec::new());
            for s in &servers {
                match remote::search(s, &query) {
                    Ok(entries) => found.extend(entries),
                    Err(e) => failed.push(format!("{}: {e}", remote::shown(&s.url))),
                }
            }
            let _ = proxy.send_event(UserEvent::Searched(query, found, failed));
        });
        if let Err(e) = started {
            eprintln!("ricerca sui server: {e}");
            self.search.asked = None;
        }
        self.changed();
    }

    /// La risposta dei server: vale solo se si cerca ancora la stessa cosa.
    pub(super) fn searched(&mut self, query: String, found: Vec<Entry>, failed: Vec<String>) {
        if self.search.asked.as_ref() == Some(&query) {
            self.search.asked = None;
        }
        if query != self.search.typed {
            return self.changed();
        }
        if found.is_empty() && !failed.is_empty() {
            let head = t("Ricerca sul server non riuscita", "Server search failed");
            self.ui.toast(format!("{head}: {}", failed.join("; ")), Instant::now());
        }
        self.search.answered = query;
        self.search.results = found;
        self.show_found();
    }

    /// I volumi trovati sui server che la libreria non ha, dopo i suoi (da
    /// `lib.found_from`), con le correzioni a mano come gli altri.
    pub(super) fn show_found(&mut self) {
        self.lib.entries.truncate(self.lib.found_from);
        let known: HashSet<&Path> = self.lib.entries.iter().map(|e| e.path.as_path()).collect();
        let mut extra: Vec<Entry> =
            self.search.results.iter().filter(|e| !known.contains(e.path.as_path())).cloned().collect();
        self.edits.apply(&mut extra);
        self.lib.entries.extend(extra);
        self.refresh_shelf();
        self.changed();
    }
}
