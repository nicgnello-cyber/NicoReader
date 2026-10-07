//! Le password dei server nel portachiavi del sistema (Gestione credenziali
//! su Windows, Portachiavi su macOS), non nel file delle impostazioni.
//!
//! Le impostazioni si scrivono senza la password quando il portachiavi l'ha
//! presa; se non la prende (un portachiavi bloccato, Linux), resta nel file,
//! che allora e' leggibile solo da chi lo possiede: una password persa sarebbe
//! peggio.

use std::collections::HashSet;
use std::sync::Mutex;

/// Le password gia' messe nel portachiavi in questo avvio: non si riscrivono
/// a ogni salvataggio delle impostazioni.
static STORED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn account(url: &str, user: &str) -> String {
    format!("{user} @ {url}")
}

/// Mette la password nel portachiavi. Falso se non si puo' (allora resta
/// nelle impostazioni).
pub fn store(url: &str, user: &str, password: &str) -> bool {
    let key = format!("{url}\n{user}\n{password}");
    let mut stored = STORED.lock().unwrap_or_else(|e| e.into_inner());
    let stored = stored.get_or_insert_with(HashSet::new);
    if stored.contains(&key) {
        return true;
    }
    let ok = imp::store(&account(url, user), password);
    if ok {
        stored.insert(key);
    }
    ok
}

/// La password dal portachiavi, se c'e'.
pub fn load(url: &str, user: &str) -> Option<String> {
    imp::load(&account(url, user))
}

/// Toglie la password dal portachiavi (il server e' uscito dalla libreria).
pub fn delete(url: &str, user: &str) {
    imp::delete(&account(url, user));
    if let Some(stored) = STORED.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        stored.retain(|k| !k.starts_with(&format!("{url}\n{user}\n")));
    }
}

#[cfg(any(windows, target_os = "macos"))]
mod imp {
    const SERVICE: &str = "NicoReader";

    fn entry(account: &str) -> Option<keyring::Entry> {
        keyring::Entry::new(SERVICE, account).ok()
    }

    pub fn store(account: &str, password: &str) -> bool {
        entry(account).is_some_and(|e| e.set_password(password).is_ok())
    }

    pub fn load(account: &str) -> Option<String> {
        entry(account)?.get_password().ok()
    }

    pub fn delete(account: &str) {
        if let Some(e) = entry(account) {
            let _ = e.delete_credential();
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    pub fn store(_: &str, _: &str) -> bool {
        false
    }

    pub fn load(_: &str) -> Option<String> {
        None
    }

    pub fn delete(_: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sul sistema vero (Windows, macOS; altrove il portachiavi non c'e'):
    /// dentro, fuori, via.
    #[test]
    fn dentro_e_fuori_dal_portachiavi() {
        let url = format!("http://prova-portachiavi-{}:25600", std::process::id());
        // senza un portachiavi usabile (Linux; una macchina di prova senza
        // sessione) la password resta nelle impostazioni: niente da provare
        if !store(&url, "io@casa", "segreta \"123\"") {
            eprintln!("portachiavi non disponibile qui");
            return;
        }
        assert_eq!(load(&url, "io@casa").as_deref(), Some("segreta \"123\""));
        assert_eq!(load(&url, "altro"), None);
        delete(&url, "io@casa");
        assert_eq!(load(&url, "io@casa"), None);
    }
}
