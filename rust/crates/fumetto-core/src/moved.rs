//! I volumi spostati o rinominati fuori dall'app (da Esplora file, dal
//! Finder, da un altro programma). Correzioni, tag e punti di lettura stanno
//! sotto il percorso del volume: senza questo modulo, spostato il file, per
//! l'app sarebbe un volume nuovo e quello che si sapeva di lui resterebbe
//! sotto il percorso vecchio.
//!
//! Per ogni volume di cui si ricorda qualcosa si tiene un'impronta: la
//! misura del file e un hash del suo inizio e della sua fine. Alla
//! scansione, un volume ricordato che non c'e' piu' e un volume nuovo con la
//! stessa impronta sono lo stesso volume, e cio' che si sapeva passa al
//! percorso nuovo. Se l'impronta e' di due volumi spariti, o di due nuovi
//! (copie identiche), non si indovina: si lascia com'e'.
//!
//! Le impronte stanno in `impronte.json`, accanto ai progressi. Si legge
//! solo l'inizio e la fine di un file, e solo quando la sua impronta manca o
//! il file e' cambiato (misura o data diverse); un volume nuovo si legge
//! solo se ha la misura di uno sparito.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

/// Quanto si legge dall'inizio e dalla fine del file.
const SAMPLE: u64 = 64 * 1024;

/// L'impronta di un file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Print {
    pub size: u64,
    /// L'ultima modifica, in secondi dal 1970: se cambia, l'impronta si rifa'.
    pub modified: u64,
    pub hash: u64,
}

impl Print {
    /// Due impronte dello stesso contenuto (la data puo' cambiare spostando).
    fn same_content(&self, other: &Print) -> bool {
        self.size == other.size && self.hash == other.hash
    }
}

/// Misura e data di un file; `None` per una cartella, un volume di un
/// server, un file che non c'e'.
fn stat(path: &Path) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(path).ok().filter(|m| m.is_file())?;
    let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    Some((meta.len(), modified))
}

/// L'impronta di un file: la misura e l'hash (FNV-1a, che non cambia fra
/// versioni di Rust) dei primi e degli ultimi 64 KB.
pub fn print_of(path: &Path) -> Option<Print> {
    let (size, modified) = stat(path)?;
    let mut file = File::open(path).ok()?;
    let mut hash = Fnv::default();
    hash.write(&size.to_le_bytes());
    let mut buf = vec![0; SAMPLE.min(size) as usize];
    file.read_exact(&mut buf).ok()?;
    hash.write(&buf);
    let tail = SAMPLE.min(size.saturating_sub(SAMPLE));
    if tail > 0 {
        file.seek(SeekFrom::End(-(tail as i64))).ok()?;
        buf.truncate(tail as usize);
        file.read_exact(&mut buf).ok()?;
        hash.write(&buf);
    }
    Some(Print { size, modified, hash: hash.0 })
}

/// FNV-1a a 64 bit.
struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Fnv {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    }
}

/// La chiave di un volume, come in correzioni e progressi.
fn key(path: &Path) -> String {
    crate::absolute(path).to_string_lossy().into_owned()
}

/// Le impronte dei volumi di cui si ricorda qualcosa.
pub struct Prints {
    /// `None`: solo in memoria (le prove).
    file: Option<PathBuf>,
    map: BTreeMap<String, Print>,
}

impl Prints {
    /// Un file che manca o non si legge: nessuna impronta (si rifanno).
    pub fn load(file: PathBuf) -> Prints {
        let map = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Prints { file: Some(file), map }
    }

    pub fn in_memory() -> Prints {
        Prints { file: None, map: BTreeMap::new() }
    }

    fn save(&self) -> io::Result<()> {
        let Some(file) = &self.file else { return Ok(()) };
        let json = serde_json::to_vec_pretty(&self.map).map_err(io::Error::other)?;
        crate::write_atomic(file, &json)
    }

    /// Dopo una scansione: `known` sono le chiavi dei volumi di cui si
    /// ricorda qualcosa (correzioni, tag, letture), `found` i volumi trovati
    /// adesso nella libreria. Tiene aggiornate le impronte e restituisce i
    /// volumi riconosciuti come spostati: (dove erano, dove sono adesso).
    /// L'impronta del percorso vecchio resta finche' lui e' fra i ricordati:
    /// se i dati non passano (la scansione e' scartata), alla prossima lo si
    /// riconosce di nuovo.
    pub fn relink(&mut self, known: &BTreeSet<String>, found: &[PathBuf]) -> Vec<(PathBuf, PathBuf)> {
        let found: HashMap<String, &PathBuf> = found.iter().map(|p| (key(p), p)).collect();
        let mut changed = false;
        // l'impronta di ogni volume ricordato che c'e', se manca o il file e' cambiato
        for k in known {
            let Some(path) = found.get(k) else { continue };
            let Some((size, modified)) = stat(path) else { continue };
            let fresh = self.map.get(k).is_some_and(|p| p.size == size && p.modified == modified);
            if !fresh && let Some(print) = print_of(path) {
                self.map.insert(k.clone(), print);
                changed = true;
            }
        }
        let moves = self.match_moved(known, &found);
        // le impronte di chi non si ricorda piu' niente se ne vanno
        let before = self.map.len();
        self.map.retain(|k, _| known.contains(k));
        changed |= self.map.len() != before;
        if changed {
            let _ = self.save();
        }
        moves
    }

    /// Gli spariti (ricordati, non trovati, non piu' su disco) e i nuovi
    /// (trovati, senza niente di ricordato) con la stessa impronta, uno a uno.
    fn match_moved(&self, known: &BTreeSet<String>, found: &HashMap<String, &PathBuf>) -> Vec<(PathBuf, PathBuf)> {
        let gone: Vec<(&String, &Print)> = known
            .iter()
            .filter(|k| !found.contains_key(*k))
            .filter_map(|k| self.map.get(k).map(|p| (k, p)))
            .filter(|(k, _)| !Path::new(k.as_str()).exists())
            .collect();
        if gone.is_empty() {
            return Vec::new();
        }
        // dei nuovi si legge solo chi ha la misura di uno sparito
        let sizes: HashSet<u64> = gone.iter().map(|(_, p)| p.size).collect();
        let arrived: Vec<(&PathBuf, Print)> = found
            .iter()
            .filter(|(k, _)| !known.contains(*k))
            .filter(|(_, p)| stat(p).is_some_and(|(size, _)| sizes.contains(&size)))
            .filter_map(|(_, p)| print_of(p).map(|print| (*p, print)))
            .collect();
        let mut moves = Vec::new();
        for (old, print) in &gone {
            let twins = gone.iter().filter(|(_, p)| p.same_content(print)).count();
            let mut news = arrived.iter().filter(|(_, p)| p.same_content(print));
            if let (1, Some((new, _)), None) = (twins, news.next(), news.next()) {
                moves.push((PathBuf::from(old.as_str()), (*new).clone()));
            }
        }
        moves.sort();
        moves
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fumetto-spostati-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Un volume finto: `len` byte diversi da quelli di `seed`.
    fn volume(path: &Path, seed: u8, len: usize) {
        let bytes: Vec<u8> = (0..len).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect();
        std::fs::write(path, bytes).unwrap();
    }

    fn known(paths: &[&Path]) -> BTreeSet<String> {
        paths.iter().map(|p| key(p)).collect()
    }

    #[test]
    fn un_volume_spostato_si_riconosce() {
        let d = dir("uno");
        let (old, new) = (d.join("X-Men 141.cbz"), d.join("serie").join("Uncanny X-Men 141.cbz"));
        volume(&old, 1, 300_000);
        volume(&d.join("altro.cbz"), 2, 300_000);
        let mut prints = Prints::load(d.join("impronte.json"));
        let known = known(&[&old]);
        assert!(prints.relink(&known, &[old.clone(), d.join("altro.cbz")]).is_empty(), "niente di spostato");
        assert!(Prints::load(d.join("impronte.json")).map.contains_key(&key(&old)), "l'impronta e' salvata");
        // spostato e rinominato: lui, non l'altro della stessa misura
        std::fs::create_dir_all(new.parent().unwrap()).unwrap();
        std::fs::rename(&old, &new).unwrap();
        let moves = prints.relink(&known, &[new.clone(), d.join("altro.cbz")]);
        assert_eq!(moves, [(PathBuf::from(key(&old)), new.clone())]);
        // finche' i dati non passano, lo si riconosce di nuovo
        assert_eq!(prints.relink(&known, &[new.clone(), d.join("altro.cbz")]), moves);
        // passati: l'impronta e' del percorso nuovo, la vecchia se ne va
        let after = self::known(&[&new]);
        assert!(prints.relink(&after, &[new.clone(), d.join("altro.cbz")]).is_empty());
        assert!(prints.map.contains_key(&key(&new)) && !prints.map.contains_key(&key(&old)));
    }

    #[test]
    fn copie_identiche_non_si_indovinano() {
        let d = dir("copie");
        let (a, b) = (d.join("a.cbz"), d.join("b.cbz"));
        volume(&a, 7, 200_000);
        volume(&b, 7, 200_000);
        let mut prints = Prints::in_memory();
        let both = known(&[&a, &b]);
        prints.relink(&both, &[a.clone(), b.clone()]);
        // spariti tutti e due, arriva una copia: di chi e'?
        let c = d.join("c.cbz");
        std::fs::rename(&a, &c).unwrap();
        std::fs::remove_file(&b).unwrap();
        assert!(prints.relink(&both, std::slice::from_ref(&c)).is_empty());
        // uno sparito, due copie nuove: neanche
        let d2 = dir("copie-due");
        let (x, y, z) = (d2.join("x.cbz"), d2.join("y.cbz"), d2.join("z.cbz"));
        volume(&x, 9, 100_000);
        let mut prints = Prints::in_memory();
        let only_x = known(&[&x]);
        prints.relink(&only_x, std::slice::from_ref(&x));
        std::fs::copy(&x, &y).unwrap();
        std::fs::rename(&x, &z).unwrap();
        assert!(prints.relink(&only_x, &[y, z]).is_empty());
    }

    #[test]
    fn un_file_cambiato_rifa_l_impronta_e_le_vecchie_se_ne_vanno() {
        let d = dir("cambiato");
        let (a, b) = (d.join("a.cbz"), d.join("b.cbz"));
        volume(&a, 3, 150_000);
        volume(&b, 4, 150_000);
        let mut prints = Prints::in_memory();
        prints.relink(&known(&[&a, &b]), &[a.clone(), b.clone()]);
        let first = prints.map[&key(&a)];
        // riscritto con altro dentro (un'altra scheda, un'altra misura)
        volume(&a, 5, 160_000);
        prints.relink(&known(&[&a]), &[a.clone(), b.clone()]);
        assert_ne!(prints.map[&key(&a)], first, "rifatta");
        assert!(!prints.map.contains_key(&key(&b)), "di b non si ricorda piu' niente");
        // un file piccolo, piu' corto del pezzo che si legge
        let small = d.join("piccolo.cbz");
        volume(&small, 6, 1000);
        assert!(print_of(&small).is_some_and(|p| p.size == 1000));
        assert!(print_of(&d).is_none(), "una cartella non ha impronta");
    }
}
