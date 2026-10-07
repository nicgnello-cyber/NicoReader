//! Leggere senza rete: un volume del server scaricato intero, e il catalogo
//! dell'ultima volta tenuto su disco.
//!
//! Il file scaricato sta nella cartella dei dati (`scaricati`), con un nome
//! che viene dall'indirizzo delle pagine: aprendo il volume remoto, se la copia
//! c'e', si legge quella, e progressi e copertine restano gli stessi (il
//! percorso e' sempre quello remoto). Il catalogo sta nella cache: se il server
//! non risponde la libreria mostra i suoi volumi come l'ultima volta, e quelli
//! scaricati si aprono.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::{Catalog, Links, Request, Server, Volume, curl, download_url, login_for};
use crate::library::Entry;
use crate::lingua::t;

/// La cartella dei dati (per i volumi scaricati) e quella della cache (per i
/// cataloghi). Senza (le prove) non si scarica e non si ricorda niente.
static FOLDERS: Mutex<(Option<PathBuf>, Option<PathBuf>)> = Mutex::new((None, None));

pub fn set_folders(data: PathBuf, cache: PathBuf) {
    *FOLDERS.lock().unwrap_or_else(|e| e.into_inner()) = (Some(data.join("scaricati")), Some(cache));
}

fn downloads_dir() -> Option<PathBuf> {
    FOLDERS.lock().unwrap_or_else(|e| e.into_inner()).0.clone()
}

fn cache_dir() -> Option<PathBuf> {
    FOLDERS.lock().unwrap_or_else(|e| e.into_inner()).1.clone()
}

/// FNV-1a a 64 bit: un nome di file stabile fra una versione e l'altra.
fn hash(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// La copia scaricata del volume remoto, se c'e'.
pub fn downloaded(path: &Path) -> Option<PathBuf> {
    let v = Volume::from_path(path)?;
    let name = hash(&v.template);
    std::fs::read_dir(downloads_dir()?)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.file_stem().is_some_and(|s| s == name.as_str()) && p.extension().is_some_and(|e| e != "parziale"))
}

/// I volumi scaricati, guardando la cartella una volta sola (per la libreria,
/// che lo chiede per ogni volume).
pub fn downloads() -> Downloads {
    let names = downloads_dir()
        .and_then(|d| std::fs::read_dir(d).ok())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let done = p.extension().is_some_and(|e| e != "parziale");
            done.then(|| p.file_stem().map(|s| s.to_string_lossy().into_owned())).flatten()
        })
        .collect();
    Downloads(names)
}

pub struct Downloads(std::collections::HashSet<String>);

impl Downloads {
    pub fn has(&self, path: &Path) -> bool {
        Volume::from_path(path).is_some_and(|v| self.0.contains(&hash(&v.template)))
    }
}

/// Il volume si puo' scaricare: il catalogo offre il file intero.
pub fn can_download(path: &Path) -> bool {
    downloads_dir().is_some() && download_url(path).is_some()
}

/// Scarica il volume intero (puo' metterci minuti: da un thread a parte).
pub fn download(path: &Path) -> Result<PathBuf, String> {
    let (Some(v), Some(url), Some(dir)) = (Volume::from_path(path), download_url(path), downloads_dir()) else {
        return Err(t("il server non offre il file da scaricare", "the server doesn't offer the file").into());
    };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let target = dir.join(format!("{}.{}", hash(&v.template), extension(&url)));
    let part = target.with_extension("parziale");
    let login = login_for(&url);
    let login = login.as_ref().map(|(u, p)| (u.as_str(), p.as_str()));
    let got = curl(&Request { url: &url, login, output: Some(&part), ..Request::default() }, None);
    if let Err(e) = got {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    std::fs::rename(&part, &target).map_err(|e| e.to_string())?;
    Ok(target)
}

/// L'estensione del file dal suo indirizzo (".../file/Nebbia%20v01.cbz"),
/// o cbz: NicoReader riconosce il formato dai primi byte, l'estensione serve
/// solo a chi guarda la cartella.
fn extension(url: &str) -> String {
    let name = url.split(['?', '#']).next().unwrap_or(url).rsplit('/').next().unwrap_or("");
    match name.rsplit_once('.') {
        Some((_, ext)) if (1..=4).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) => {
            ext.to_ascii_lowercase()
        }
        _ => "cbz".into(),
    }
}

/// Toglie la copia scaricata.
pub fn forget(path: &Path) -> std::io::Result<()> {
    match downloaded(path) {
        Some(file) => std::fs::remove_file(file),
        None => Ok(()),
    }
}

/// Il catalogo su disco: le voci e i collegamenti, non i punti di lettura
/// (quelli valgono solo appena letti).
#[derive(Serialize, Deserialize)]
struct Saved {
    entries: Vec<SavedEntry>,
    links: HashMap<String, Links>,
}

#[derive(Serialize, Deserialize)]
struct SavedEntry {
    path: PathBuf,
    title: String,
    series: Option<String>,
    number: Option<u32>,
    authors: Option<String>,
}

fn catalog_file(server: &Server) -> Option<PathBuf> {
    Some(cache_dir()?.join(format!("server-{}.json", hash(&format!("{}\n{}", server.url, server.user)))))
}

pub(super) fn save_catalog(server: &Server, c: &Catalog) {
    let Some(file) = catalog_file(server) else { return };
    let entries = c
        .entries
        .iter()
        .map(|e| SavedEntry {
            path: e.path.clone(),
            title: e.title.clone(),
            series: e.series.clone(),
            number: e.number,
            authors: e.authors.clone(),
        })
        .collect();
    let Ok(json) = serde_json::to_vec(&Saved { entries, links: c.links.clone() }) else { return };
    // la cache e' un di piu': se non si scrive, pazienza
    let tmp = file.with_extension("tmp");
    let _ = file
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|_| std::fs::write(&tmp, json))
        .and_then(|_| std::fs::rename(&tmp, &file));
}

pub(super) fn load_catalog(server: &Server) -> Option<Catalog> {
    let saved: Saved = serde_json::from_slice(&std::fs::read(catalog_file(server)?).ok()?).ok()?;
    let entries = saved
        .entries
        .into_iter()
        .map(|e| Entry { path: e.path, title: e.title, series: e.series, number: e.number, authors: e.authors })
        .collect();
    Some(Catalog { entries, reads: Vec::new(), links: saved.links, fetched: Instant::now(), stale: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estensioni_dagli_indirizzi() {
        assert_eq!(extension("http://casa/opds/v1.2/books/X/file/Nebbia%20sul%20Porto%20v01.cbz"), "cbz");
        assert_eq!(extension("http://casa/api/opds/K/series/1/volume/1/chapter/1/download/a.CBR"), "cbr");
        assert_eq!(extension("http://casa/api/download?id=3"), "cbz");
        assert_eq!(extension("http://casa/file/titolo.con.punti.pdf"), "pdf");
        assert_eq!(extension("http://casa/file/Vol. 1 (2021)"), "cbz");
    }
}
