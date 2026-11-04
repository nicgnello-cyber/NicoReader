//! Il punto di lettura, uguale sul server e qui: si legge un capitolo sul
//! telefono (con l'app di Komga o Kavita) e NicoReader riparte da li', e
//! viceversa.
//!
//! Il catalogo dice per ogni volume dove si e' arrivati (`pse:lastRead`, con
//! la data). Le date non bastano a decidere chi ha ragione (quelle di Kavita
//! non dicono il fuso orario): si ricorda invece l'ultimo valore visto sul
//! server, volume per volume. Se il server dice ancora quello, nessuno ha
//! letto altrove, e vale cio' che si e' letto qui (anche senza rete, dai
//! volumi scaricati: lo si manda appena il server risponde). Se dice altro,
//! qualcuno ha letto altrove, e vale il suo.
//!
//! Komga conta le pagine da 1, e "finito" e' a parte; Kavita da 0, e il
//! numero di pagine vuol dire finito. Si scrive con l'API di Komga e, per
//! Kavita, chiedendo dal catalogo la pagina a cui si e' (lui la segna).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use super::{Flavor, Request, Volume, curl, login_for, origin};
use crate::progress::Progress;

/// Quanto si aspetta prima di scrivere al server: sfogliando, al massimo una
/// richiesta ogni tre secondi, con la pagina a cui si e' arrivati.
const QUIET: Duration = Duration::from_secs(3);

/// Dove era arrivato chi legge, secondo il catalogo del server.
#[derive(Clone, Debug, PartialEq)]
pub struct Read {
    pub path: PathBuf,
    /// Come lo dice il server (0: mai aperto).
    pub last_read: u32,
    /// Quando, in secondi dal 1970.
    pub date: Option<u64>,
}

/// Il punto di lettura detto senza il modo di contare di ogni server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    New,
    /// Alla pagina (da 0).
    At(usize),
    Done,
}

impl Flavor {
    /// Il valore che il server scrive per `place` in un volume di `count` pagine.
    fn value(&self, place: Place, count: usize) -> u32 {
        let v = match (self, place) {
            (_, Place::New) => 0,
            (_, Place::Done) => count,
            (Flavor::Kavita { .. }, Place::At(i)) => i,
            (_, Place::At(i)) => i + 1,
        };
        v.min(count) as u32
    }

    fn place(&self, value: u32, count: usize) -> Place {
        let v = value as usize;
        match self {
            _ if v == 0 => Place::New,
            _ if v >= count => Place::Done,
            Flavor::Kavita { .. } => Place::At(v),
            _ => Place::At(v - 1),
        }
    }

    fn writable(&self) -> bool {
        !matches!(self, Flavor::Other)
    }
}

/// Il punto di lettura in un volume, come lo tengono i progressi.
fn place_of(page: usize, pages: usize) -> Place {
    if pages > 0 && page + 1 >= pages { Place::Done } else { Place::At(page) }
}

#[derive(Default)]
struct State {
    /// L'ultimo valore visto sul server (o scritto), per indirizzo delle pagine.
    known: BTreeMap<String, u32>,
    /// Quando lo si e' scritto: un catalogo letto prima non vale piu'.
    written: HashMap<String, Instant>,
    /// Da scrivere: il volume e il valore.
    pending: HashMap<String, (Volume, u32)>,
    file: Option<PathBuf>,
    worker: bool,
}

impl State {
    fn save(&self) {
        let Some(file) = &self.file else { return };
        let Ok(json) = serde_json::to_vec(&self.known) else { return };
        let tmp = file.with_extension("tmp");
        let _ = file
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|_| std::fs::write(&tmp, json))
            .and_then(|_| std::fs::rename(&tmp, file));
    }
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static WAKE: Condvar = Condvar::new();

fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(s.get_or_insert_with(State::default))
}

/// Dove ricordare i valori visti sul server fra un avvio e l'altro. Senza
/// (le prove) si ricordano solo finche' il programma e' aperto.
pub fn set_file(file: PathBuf) {
    let known = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    with(|s| {
        s.known = known;
        s.file = Some(file);
    });
}

/// Si e' arrivati a `page` (da 0) di `pages` in un volume: lo si dira' al
/// server, se e' uno dei suoi.
pub fn report(path: &Path, page: usize, pages: usize) {
    queue(path, |count| (place_of(page, pages), count));
}

/// Il volume e' di nuovo da leggere (o segnato come letto).
pub fn report_mark(path: &Path, read: bool) {
    queue(path, |count| (if read { Place::Done } else { Place::New }, count));
}

fn queue(path: &Path, place: impl FnOnce(usize) -> (Place, usize)) {
    let Some(v) = Volume::from_path(path) else { return };
    let flavor = Flavor::of(&v.template);
    if !flavor.writable() {
        return;
    }
    let (place, count) = place(v.count);
    let value = flavor.value(place, count);
    let start = with(|s| {
        if s.known.get(&v.template) == Some(&value) {
            s.pending.remove(&v.template);
            return false;
        }
        s.pending.insert(v.template.clone(), (v, value));
        !std::mem::replace(&mut s.worker, true)
    });
    WAKE.notify_all();
    if start {
        let _ = std::thread::Builder::new().name("punto di lettura".into()).spawn(worker);
    }
}

/// Scrive al server cio' che aspetta, `QUIET` dopo il primo giro di pagina
/// (intanto conta solo l'ultimo).
fn worker() {
    loop {
        {
            let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
            while s.as_ref().is_none_or(|s| s.pending.is_empty()) {
                s = WAKE.wait(s).unwrap_or_else(|e| e.into_inner());
            }
        }
        std::thread::sleep(QUIET);
        send_pending(30);
    }
}

/// Scrive subito cio' che aspetta: alla chiusura del programma.
pub fn flush() {
    send_pending(5);
}

fn send_pending(max_time: u32) {
    let batch = with(|s| std::mem::take(&mut s.pending));
    for (template, (volume, value)) in batch {
        // senza rete non si insiste: lo si manda quando il server risponde
        // (vedi `incoming`)
        if send(&volume, value, max_time).is_ok() {
            with(|s| {
                s.known.insert(template.clone(), value);
                s.written.insert(template, Instant::now());
                s.save();
            });
        }
    }
}

fn send(v: &Volume, value: u32, max_time: u32) -> Result<(), String> {
    let login = login_for(&v.template);
    let login = login.as_ref().map(|(u, p)| (u.as_str(), p.as_str()));
    match Flavor::of(&v.template) {
        Flavor::Komga { book } => {
            let url = format!("{}/api/v1/books/{book}/read-progress", origin(&v.template));
            let json = format!(r#"{{"page":{value},"completed":{}}}"#, value as usize >= v.count);
            let r = if value == 0 {
                Request { url: &url, login, method: Some("DELETE"), ..Request::default() }
            } else {
                Request { url: &url, login, method: Some("PATCH"), json: Some(&json), ..Request::default() }
            };
            curl(&r, Some(max_time)).map(|_| ())
        }
        // Kavita segna la pagina che gli si chiede dal catalogo; il numero di
        // pagine vuol dire finito
        Flavor::Kavita { .. } => {
            let url = v.page_url(value as usize);
            curl(&Request { url: &url, login, ..Request::default() }, Some(max_time)).map(|_| ())
        }
        Flavor::Other => Ok(()),
    }
}

/// Il catalogo appena letto dice dove si e' arrivati sul server: si
/// aggiornano i progressi dove qualcuno ha letto altrove, e si scrive al
/// server cio' che si e' letto qui senza che lui lo sapesse. `open`: il
/// volume aperto, che resta dov'e' (chi legge e' qui). Vero se i progressi
/// sono cambiati.
pub fn incoming(reads: &[Read], fetched: Instant, progress: &mut Progress, open: Option<&Path>) -> bool {
    let mut changed = false;
    let mut to_send = Vec::new();
    for r in reads {
        let Some(v) = Volume::from_path(&r.path) else { continue };
        let flavor = Flavor::of(&v.template);
        let (known, stale) = with(|s| {
            let stale = s.written.get(&v.template).is_some_and(|&w| w > fetched);
            (s.known.get(&v.template).copied(), stale)
        });
        // un catalogo letto prima dell'ultima scrittura dice cose vecchie
        if stale {
            continue;
        }
        let local = progress.get(&r.path).filter(|s| s.pages > 0);
        let here = local.map_or(Place::New, |s| place_of(s.page, s.pages));
        // il posto del server come lo intende NicoReader: per Kavita l'ultima
        // pagina non e' ancora "finito", qui si'. Uguali cosi', non c'e' niente
        // da fare (e non si scrive al server una fine che nessuno ha deciso)
        let there = match flavor.place(r.last_read, v.count) {
            Place::At(i) => place_of(i, v.count),
            p => p,
        };
        let elsewhere = match known {
            // il server dice ancora l'ultimo valore visto: nessuno ha letto altrove
            Some(k) => k != r.last_read,
            // mai visto (il primo avvio dopo averlo aggiunto): vale il piu' recente
            None => match (local, r.date) {
                (None, _) => true,
                (Some(s), Some(date)) => date > s.read_at,
                (Some(_), None) => false,
            },
        };
        with(|s| s.known.insert(v.template.clone(), r.last_read));
        if here == there {
            continue;
        }
        if elsewhere {
            if open != Some(r.path.as_path()) {
                changed |= adopt(progress, &r.path, flavor.place(r.last_read, v.count), v.count, r.date);
            }
        } else if flavor.writable() {
            to_send.push((r.path.clone(), here));
        }
    }
    with(|s| s.save());
    for (path, place) in to_send {
        queue(&path, |count| (place, count));
    }
    changed
}

/// Mette nei progressi il punto di lettura venuto dal server.
fn adopt(progress: &mut Progress, path: &Path, place: Place, count: usize, date: Option<u64>) -> bool {
    match place {
        Place::New => {
            let had = progress.get(path).is_some();
            progress.mark(path, false, count);
            had
        }
        Place::At(i) => progress.adopt(path, i, count, date),
        Place::Done => progress.adopt(path, count.saturating_sub(1), count, date),
    }
}

/// "2026-10-07T08:10:56Z", "2026-10-07T08:10:56.812+02:00", o senza fuso
/// (Kavita: lo si prende per UTC) in secondi dal 1970.
pub fn unix_time(s: &str) -> Option<u64> {
    let num = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, sec) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if s.get(4..5) != Some("-") || s.get(10..11).is_none_or(|t| t != "T" && t != " ") {
        return None;
    }
    // il fuso, dopo i secondi e le loro frazioni
    let rest = s[19..].trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    let offset = match rest.as_bytes().first() {
        Some(b'+' | b'-') if rest.len() >= 6 => {
            let sign = if rest.starts_with('-') { -1 } else { 1 };
            sign * (rest.get(1..3)?.parse::<i64>().ok()? * 3600 + rest.get(4..6)?.parse::<i64>().ok()? * 60)
        }
        _ => 0,
    };
    // giorni dal 1970 (Howard Hinnant, days_from_civil)
    let y = if mo <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + h * 3600 + mi * 60 + sec - offset).ok()
}

#[cfg(test)]
mod tests;
