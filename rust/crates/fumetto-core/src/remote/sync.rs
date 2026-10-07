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
mod tests {
    use super::*;
    use crate::progress::Saved;

    const KOMGA: &str = "http://casa:25600/opds/v1.2/books/0RV8/pages/{pageNumber}";
    const KAVITA: &str =
        "http://casa:5000/api/opds/KEY/image?libraryId=1&seriesId=2&volumeId=5&chapterId=5&pageNumber={pageNumber}";

    fn path(template: &str) -> PathBuf {
        Volume { template: template.into(), count: 6, title: "Volume 1".into() }.path()
    }

    #[test]
    fn i_server_contano_a_modo_loro() {
        let komga = Flavor::of(KOMGA);
        let kavita = Flavor::of(KAVITA);
        assert_eq!(komga, Flavor::Komga { book: "0RV8".into() });
        assert_eq!(kavita, Flavor::Kavita { key: "KEY".into(), chapter: "5".into() });
        assert_eq!(Flavor::of("http://altro/pse/{pageNumber}"), Flavor::Other);
        // pagina 3 di 6 (la terza, da 0 e' la 2)
        assert_eq!(komga.value(Place::At(2), 6), 3);
        assert_eq!(kavita.value(Place::At(2), 6), 2);
        for f in [&komga, &kavita] {
            assert_eq!(f.value(Place::Done, 6), 6);
            assert_eq!(f.value(Place::New, 6), 0);
            for p in [Place::New, Place::At(1), Place::At(4), Place::Done] {
                assert_eq!(f.place(f.value(p, 6), 6), p, "{f:?} {p:?}");
            }
        }
        assert_eq!(place_of(5, 6), Place::Done, "l'ultima pagina e' la fine");
    }

    #[test]
    fn le_date_dei_server() {
        assert_eq!(unix_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(unix_time("2026-10-07T08:10:56Z"), Some(1_791_360_656));
        assert_eq!(unix_time("2026-10-07T08:10:56.812Z"), Some(1_791_360_656));
        assert_eq!(unix_time("2026-10-07T10:10:56+02:00"), Some(1_791_360_656));
        assert_eq!(unix_time("2026-10-07T08:10:56"), Some(1_791_360_656), "Kavita, senza fuso");
        assert_eq!(unix_time("2024-02-29T00:00:00Z"), Some(1_709_164_800), "l'anno bisestile");
        assert_eq!(unix_time("ieri"), None);
    }

    /// Senza server: solo la decisione di chi ha ragione. Ogni prova ha i suoi
    /// volumi (lo stato e' di tutto il programma).
    #[test]
    fn chi_ha_letto_dove() {
        let mut progress = Progress::in_memory();
        let now = Instant::now();
        let read = |template: &str, last_read, date| Read { path: path(template), last_read, date };

        // mai visto, niente qui: vale il server (Komga dice 3: la terza pagina)
        let a = "http://casa:25600/opds/v1.2/books/A/pages/{pageNumber}";
        assert!(incoming(&[read(a, 3, Some(100))], now, &mut progress, None));
        assert_eq!(progress.get(&path(a)).map(|s| (s.page, s.pages, s.read_at)), Some((2, 6, 100)));

        // il server dice ancora 3, qui si e' andati avanti: niente da prendere
        progress.set(&path(a), Saved { page: 4, pages: 6, ..Saved::default() });
        assert!(!incoming(&[read(a, 3, Some(100))], Instant::now(), &mut progress, None));
        assert_eq!(progress.get(&path(a)).map(|s| s.page), Some(4));
        with(|s| s.pending.clear());

        // il server dice 6 (finito altrove): vale il suo
        assert!(incoming(&[read(a, 6, Some(200))], Instant::now(), &mut progress, None));
        assert_eq!(progress.get(&path(a)).map(|s| s.page), Some(5));

        // ...ma non per il volume aperto: chi legge e' qui
        let b = "http://casa:25600/opds/v1.2/books/B/pages/{pageNumber}";
        progress.set(&path(b), Saved { page: 1, pages: 6, ..Saved::default() });
        with(|s| s.known.insert(b.into(), 2));
        assert!(!incoming(&[read(b, 5, None)], Instant::now(), &mut progress, Some(&path(b))));
        assert_eq!(progress.get(&path(b)).map(|s| s.page), Some(1));

        // Kavita, rimesso da leggere altrove
        let c = "http://casa:5000/api/opds/KEY/image?chapterId=9&pageNumber={pageNumber}";
        progress.set(&path(c), Saved { page: 3, pages: 6, ..Saved::default() });
        with(|s| s.known.insert(c.into(), 3));
        assert!(incoming(&[read(c, 0, None)], Instant::now(), &mut progress, None));
        assert!(progress.get(&path(c)).is_none());

        // un catalogo letto prima dell'ultima scrittura non vale
        let d = "http://casa:25600/opds/v1.2/books/D/pages/{pageNumber}";
        let before = Instant::now();
        with(|s| {
            s.known.insert(d.into(), 4);
            s.written.insert(d.into(), Instant::now());
        });
        assert!(!incoming(&[read(d, 1, None)], before, &mut progress, None));
        assert!(progress.get(&path(d)).is_none());
    }

    #[test]
    fn l_ultima_pagina_di_kavita() {
        let mut progress = Progress::in_memory();
        let f = "http://casa:5000/api/opds/KEY/image?chapterId=11&pageNumber={pageNumber}";
        // Kavita: all'ultima pagina (5 di 6, da 0), non ancora finito; qui e' letto
        let read = Read { path: path(f), last_read: 5, date: None };
        assert!(incoming(std::slice::from_ref(&read), Instant::now(), &mut progress, None));
        assert!(!incoming(&[read], Instant::now(), &mut progress, None));
        assert_eq!(with(|s| s.pending.remove(f)), None, "nessuna fine scritta al server");
    }

    #[test]
    fn letto_qui_senza_rete_si_manda_dopo() {
        let mut progress = Progress::in_memory();
        let e = "http://casa:25600/opds/v1.2/books/E/pages/{pageNumber}";
        with(|s| s.known.insert(e.into(), 2));
        progress.set(&path(e), Saved { page: 4, pages: 6, ..Saved::default() });
        // il server dice ancora 2: e' rimasto indietro, gli si scrive 5
        assert!(!incoming(&[Read { path: path(e), last_read: 2, date: None }], Instant::now(), &mut progress, None));
        let pending = with(|s| s.pending.remove(e).map(|(_, v)| v));
        assert_eq!(pending, Some(5));
    }
}
