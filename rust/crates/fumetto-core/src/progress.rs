//! Dove si era rimasti, volume per volume.
//!
//! Un file JSON leggibile, nella cartella dei dati dell'utente. Si scrive
//! sempre su un file temporaneo che poi si rinomina: se il programma si chiude
//! a meta' (corrente staccata, crash) resta il file vecchio intero, mai uno
//! troncato. Un file illeggibile si mette da parte con il suffisso
//! `.illeggibile` invece di cancellarlo: e' la memoria di tutto cio' che si e'
//! letto, magari si recupera a mano.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Lo stato di lettura di un volume.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    pub page: usize,
    /// Quante pagine aveva: se il volume e' cambiato, la pagina salvata si
    /// prende con cautela.
    pub pages: usize,
    /// "pagina", "doppia" o "nastro".
    pub mode: String,
    pub manga: bool,
    /// Doppia pagina con la copertina da sola.
    pub cover_alone: bool,
    /// Larghezza del nastro, come frazione della finestra.
    pub strip_width: f32,
    /// Nel nastro: quanta parte della pagina d'ancora era gia' passata (0..1).
    pub strip_offset: f32,
    /// Quando lo si e' letto l'ultima volta, in secondi dal 1970.
    pub read_at: u64,
    /// Rotazione delle pagine in gradi, in senso orario (0, 90, 180, 270).
    pub rotation: u16,
    /// Le pagine segnate (da 0), in ordine.
    pub bookmarks: Vec<usize>,
}

pub struct Progress {
    /// `None`: solo in memoria (per le prove, che non devono toccare i dati veri).
    path: Option<PathBuf>,
    entries: BTreeMap<String, Saved>,
    dirty: bool,
}

impl Progress {
    pub fn load(path: PathBuf) -> Progress {
        let entries = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|_| {
                let aside = path.with_extension("illeggibile");
                let _ = std::fs::rename(&path, &aside);
                BTreeMap::new()
            }),
            Err(_) => BTreeMap::new(),
        };
        Progress { path: Some(path), entries, dirty: false }
    }

    pub fn in_memory() -> Progress {
        Progress { path: None, entries: BTreeMap::new(), dirty: false }
    }

    /// La chiave di un volume: il percorso assoluto, cosi' com'e' scritto.
    fn key(volume: &Path) -> String {
        std::path::absolute(volume).unwrap_or_else(|_| volume.to_owned()).to_string_lossy().into_owned()
    }

    pub fn get(&self, volume: &Path) -> Option<&Saved> {
        self.entries.get(&Self::key(volume))
    }

    pub fn set(&mut self, volume: &Path, mut saved: Saved) {
        let key = Self::key(volume);
        // l'ora di lettura cambia sempre: conta solo se e' cambiato qualcos'altro
        if let Some(old) = self.entries.get(&key) {
            saved.read_at = saved.read_at.max(old.read_at);
            if *old == saved {
                return;
            }
        }
        saved.read_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        self.entries.insert(key, saved);
        self.dirty = true;
    }

    /// I volumi letti, dal piu' recente. Sono i "file recenti": l'ora di
    /// lettura sta gia' in ogni voce, non serve un elenco a parte da tenere
    /// allineato.
    pub fn recent(&self) -> Vec<(PathBuf, &Saved)> {
        // senza pagine: rimesso da leggere, ricordato solo per i segnalibri
        let mut v: Vec<_> = self.entries.iter().filter(|(_, s)| s.pages > 0).map(|(k, s)| (PathBuf::from(k), s)).collect();
        v.sort_by_key(|(_, s)| std::cmp::Reverse(s.read_at));
        v
    }

    /// Letto fino in fondo (`pages` pagine), o di nuovo da leggere. Da
    /// leggere vuol dire mai aperto, ma i segnalibri restano: sono di chi
    /// legge, non della lettura.
    pub fn mark(&mut self, volume: &Path, read: bool, pages: usize) {
        if read {
            let mut saved = self.get(volume).cloned().unwrap_or_default();
            (saved.page, saved.pages) = (pages.saturating_sub(1), pages);
            saved.strip_offset = 0.0;
            self.set(volume, saved);
            return;
        }
        let key = Self::key(volume);
        let Some(old) = self.entries.remove(&key) else { return };
        if !old.bookmarks.is_empty() {
            self.entries.insert(key, Saved { bookmarks: old.bookmarks, ..Saved::default() });
        }
        self.dirty = true;
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Scrive se c'e' qualcosa di nuovo. Prima un temporaneo, poi lo scambio.
    pub fn save(&mut self) -> io::Result<()> {
        let Some(path) = &self.path else { return Ok(()) };
        if !self.dirty {
            return Ok(());
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec_pretty(&self.entries).map_err(io::Error::other)?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)?;
        self.dirty = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fumetto-progressi-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("progressi.json")
    }

    #[test]
    fn salva_e_rilegge() {
        let path = temp("giro");
        let mut p = Progress::load(path.clone());
        let saved = Saved { page: 41, pages: 200, mode: "doppia".into(), manga: true, ..Default::default() };
        p.set(Path::new("volume.cbz"), saved.clone());
        p.save().unwrap();
        assert!(!path.with_extension("tmp").exists(), "temporaneo rimasto");
        let again = Progress::load(path.clone());
        let got = again.get(Path::new("volume.cbz")).unwrap();
        assert_eq!((got.page, got.mode.as_str(), got.manga), (41, "doppia", true));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn niente_da_scrivere_se_nulla_cambia() {
        let mut p = Progress::in_memory();
        let saved = Saved { page: 3, ..Default::default() };
        p.set(Path::new("a"), saved.clone());
        p.save().unwrap();
        p.dirty = false;
        p.set(Path::new("a"), saved);
        assert!(!p.is_dirty(), "stesso stato, nessuna scrittura");
    }

    #[test]
    fn recenti_dal_piu_nuovo() {
        let mut p = Progress::in_memory();
        for (name, at) in [("vecchio", 10), ("nuovo", 30), ("medio", 20)] {
            p.entries.insert(name.into(), Saved { read_at: at, pages: 10, ..Default::default() });
        }
        let names: Vec<_> = p.recent().into_iter().map(|(path, _)| path).collect();
        assert_eq!(names, ["nuovo", "medio", "vecchio"].map(PathBuf::from));
    }

    #[test]
    fn segnare_come_letto_e_non_letto() {
        let mut p = Progress::in_memory();
        p.mark(Path::new("v.cbz"), true, 40);
        assert_eq!(p.get(Path::new("v.cbz")).map(|s| (s.page, s.pages)), Some((39, 40)));
        p.mark(Path::new("v.cbz"), false, 0);
        assert!(p.get(Path::new("v.cbz")).is_none(), "di nuovo da leggere: come mai aperto");
    }

    #[test]
    fn da_leggere_ma_con_i_suoi_segnalibri() {
        let mut p = Progress::in_memory();
        p.set(Path::new("v.cbz"), Saved { page: 12, pages: 40, bookmarks: vec![3, 17], ..Default::default() });
        p.mark(Path::new("v.cbz"), false, 0);
        let s = p.get(Path::new("v.cbz")).unwrap();
        assert_eq!((s.page, s.pages, s.bookmarks.as_slice()), (0, 0, &[3, 17][..]));
        assert!(p.recent().is_empty(), "non e' fra gli ultimi letti");
    }

    #[test]
    fn file_rovinato_messo_da_parte() {
        let path = temp("rovinato");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{ questo non e' json").unwrap();
        let p = Progress::load(path.clone());
        assert!(p.get(Path::new("x")).is_none());
        assert!(path.with_extension("illeggibile").exists(), "il file rovinato va conservato");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
