//! I dati di un volume corretti a mano: titolo, serie, numero, verso di
//! lettura. Servono quando il nome del file e' disordinato e la ComicInfo.xml
//! manca (o sbaglia). Stanno in un file a parte nella cartella dei dati
//! (`correzioni.json`): i volumi non si toccano, e quelli di un server non si
//! potrebbero toccare.
//!
//! Ogni campo e' una correzione a se': `None` lascia quello che dicono il
//! file e la sua scheda.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::library::Entry;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct VolumeEdit {
    pub title: Option<String>,
    /// `Some("")`: nessuna serie (un volume unico).
    pub series: Option<String>,
    /// `Some(None)`: nessun numero.
    pub number: Option<Option<u32>>,
    pub right_to_left: Option<bool>,
}

impl VolumeEdit {
    pub fn is_empty(&self) -> bool {
        *self == VolumeEdit::default()
    }

    /// Le correzioni applicate a una voce della libreria.
    pub fn apply(&self, e: &mut Entry) {
        if let Some(title) = self.title.as_ref().filter(|t| !t.trim().is_empty()) {
            e.title = title.trim().to_owned();
        }
        if let Some(series) = &self.series {
            let series = series.trim();
            e.series = (!series.is_empty()).then(|| series.to_owned());
        }
        if let Some(number) = self.number {
            e.number = number;
        }
    }
}

pub struct Edits {
    /// `None`: solo in memoria (le prove).
    file: Option<PathBuf>,
    map: BTreeMap<String, VolumeEdit>,
}

impl Edits {
    /// Un file che manca o non si legge: nessuna correzione.
    pub fn load(file: PathBuf) -> Edits {
        let map = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Edits { file: Some(file), map }
    }

    pub fn in_memory() -> Edits {
        Edits { file: None, map: BTreeMap::new() }
    }

    fn key(path: &Path) -> String {
        crate::absolute(path).to_string_lossy().into_owned()
    }

    pub fn get(&self, path: &Path) -> Option<&VolumeEdit> {
        self.map.get(&Self::key(path))
    }

    /// Mette (o toglie, se vuota) la correzione di un volume, e la scrive.
    pub fn set(&mut self, path: &Path, edit: VolumeEdit) -> io::Result<()> {
        if edit.is_empty() {
            self.map.remove(&Self::key(path));
        } else {
            self.map.insert(Self::key(path), edit);
        }
        self.save()
    }

    fn save(&self) -> io::Result<()> {
        let Some(file) = &self.file else { return Ok(()) };
        let json = serde_json::to_vec_pretty(&self.map).map_err(io::Error::other)?;
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = file.with_extension("tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, file)
    }

    /// Le correzioni applicate a tutta la libreria.
    pub fn apply(&self, entries: &mut [Entry]) {
        if self.map.is_empty() {
            return;
        }
        for e in entries {
            if let Some(edit) = self.get(&e.path) {
                edit.apply(e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> Entry {
        Entry {
            path: PathBuf::from("/fumetti/scan_0003_final.cbz"),
            title: "scan_0003_final".into(),
            series: None,
            number: None,
            authors: None,
            groups: Vec::new(),
        }
    }

    #[test]
    fn correzioni_sulla_libreria() {
        let dir = std::env::temp_dir().join(format!("fumetto-correzioni-{}", std::process::id()));
        let file = dir.join("correzioni.json");
        let mut edits = Edits::load(file.clone());
        let fix = VolumeEdit {
            title: Some("  Il ritorno ".into()),
            series: Some("Nebbia sul Porto".into()),
            number: Some(Some(3)),
            right_to_left: Some(true),
        };
        edits.set(Path::new("/fumetti/scan_0003_final.cbz"), fix.clone()).unwrap();
        let again = Edits::load(file.clone());
        assert_eq!(again.get(Path::new("/fumetti/scan_0003_final.cbz")), Some(&fix), "riletta dal file");
        let mut entries = vec![entry()];
        again.apply(&mut entries);
        assert_eq!(entries[0].title, "Il ritorno");
        assert_eq!(entries[0].series.as_deref(), Some("Nebbia sul Porto"));
        assert_eq!(entries[0].number, Some(3));

        // un volume unico: senza serie ne' numero; il titolo vuoto non conta
        let mut e = Entry { series: Some("X".into()), number: Some(1), ..entry() };
        VolumeEdit { title: Some(" ".into()), series: Some(String::new()), number: Some(None), right_to_left: None }
            .apply(&mut e);
        assert_eq!((e.title.as_str(), e.series, e.number), ("scan_0003_final", None, None));

        // tutto vuoto: la correzione se ne va
        edits.set(Path::new("/fumetti/scan_0003_final.cbz"), VolumeEdit::default()).unwrap();
        assert!(Edits::load(file).get(Path::new("/fumetti/scan_0003_final.cbz")).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
