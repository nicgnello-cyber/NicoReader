//! Quando il programma si ferma per un errore (un panic): cosa si annota, e
//! come lo si dice la volta dopo.
//!
//! Senza console un panic non lascia traccia: la finestra sparisce e basta.
//! Qui ogni panic finisce in `errori.txt`, nella cartella dei dati, con il
//! messaggio, il punto del codice, il thread, la versione e il sistema
//! (l'eseguibile del pacchetto e' senza simboli: un backtrace sarebbe solo
//! indirizzi). Se e' sul thread principale il programma si chiude: resta un
//! segno, e alla prossima apertura un avviso dice cosa e' successo e dove sta
//! il rapporto. Niente dialogo durante il panic: dentro il ciclo degli eventi
//! non e' sicuro (vedi `dialog`).

use std::io::Write;
use std::path::{Path, PathBuf};

use fumetto_core::lingua::t;

const LOG: &str = "errori.txt";
/// Il segno lasciato da un panic che ha chiuso il programma.
const FLAG: &str = "chiuso-per-errore";
/// Oltre questa misura il registro ricomincia (il vecchio resta in errori.old.txt).
const MAX_LOG: u64 = 256 * 1024;

/// Annota d'ora in poi ogni panic in `dir`, prima del messaggio solito.
pub fn install(dir: PathBuf) {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("senza nome");
        let what = info.payload_as_str().unwrap_or("(nessun messaggio)");
        let at = info.location().map_or_else(String::new, |l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
        // se non si riesce a scrivere, resta il messaggio solito
        let _ = record(&dir, &entry(name, what, &at), name == "main");
        default(info);
    }));
}

/// L'avviso da mostrare se l'ultima volta il programma si e' chiuso per un
/// errore; il segno si toglie, cosi' l'avviso compare una volta sola.
pub fn last_crash(dir: &Path) -> Option<String> {
    let reason = std::fs::read_to_string(dir.join(FLAG)).ok()?;
    let _ = std::fs::remove_file(dir.join(FLAG));
    Some(format!(
        "{}\n\n{}\n\n{} {}",
        t("L'ultima volta NicoReader si è chiuso per un errore.", "Last time NicoReader closed because of an error."),
        reason.trim(),
        t("I dettagli sono in", "The details are in"),
        dir.join(LOG).display()
    ))
}

fn entry(thread: &str, what: &str, at: &str) -> String {
    format!(
        "=== {} · NicoReader {} · {} {} · thread {thread}\n{what}\n{at}\n\n",
        utc_now(),
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

fn record(dir: &Path, text: &str, fatal: bool) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let log = dir.join(LOG);
    if std::fs::metadata(&log).is_ok_and(|m| m.len() > MAX_LOG) {
        std::fs::rename(&log, dir.join("errori.old.txt"))?;
    }
    std::fs::OpenOptions::new().create(true).append(true).open(&log)?.write_all(text.as_bytes())?;
    if fatal {
        // solo il messaggio: per l'avviso, il resto e' nel registro
        let what = text.lines().nth(1).unwrap_or_default();
        std::fs::write(dir.join(FLAG), what)?;
    }
    Ok(())
}

/// Adesso, in UTC: "2026-10-06 08:12:03".
fn utc_now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (y, m, d) = civil_date((secs / 86_400) as i64);
    let s = secs % 86_400;
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC", s / 3600, s / 60 % 60, s % 60)
}

/// Anno, mese e giorno dei giorni dal 1970-01-01 (l'algoritmo di Howard
/// Hinnant, calendario gregoriano).
fn civil_date(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date() {
        assert_eq!(civil_date(0), (1970, 1, 1));
        assert_eq!(civil_date(11_016), (2000, 2, 29));
        assert_eq!(civil_date(20_732), (2026, 10, 6));
        assert_eq!(civil_date(-1), (1969, 12, 31));
    }

    #[test]
    fn un_panic_si_annota_e_si_dice_una_volta() {
        let dir = std::env::temp_dir().join(format!("fumetto-crash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        record(&dir, &entry("lettore 1", "indice fuori dai limiti", "src/a.rs:1:2"), false).unwrap();
        assert_eq!(last_crash(&dir), None, "un thread di lavoro non chiude il programma");
        record(&dir, &entry("main", "pagina rotta", "src/b.rs:3:4"), true).unwrap();
        let log = std::fs::read_to_string(dir.join(LOG)).unwrap();
        assert!(log.contains("thread lettore 1\nindice fuori dai limiti\nsrc/a.rs:1:2"), "{log}");
        assert!(log.contains("thread main\npagina rotta\nsrc/b.rs:3:4"), "{log}");
        let notice = last_crash(&dir).unwrap();
        assert!(notice.contains("pagina rotta") && notice.contains(LOG), "{notice}");
        assert_eq!(last_crash(&dir), None, "una volta sola");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
