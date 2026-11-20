//! I dati di un volume corretti a mano: titolo, serie, numero, verso di
//! lettura. Servono quando il nome del file e' disordinato e la ComicInfo.xml
//! manca (o sbaglia). E i tag messi da chi legge (un personaggio, una
//! categoria, un'etichetta), o tolti a quelli della scheda. Stanno in un file a parte nella cartella dei dati
//! (`correzioni.json`): i volumi non si toccano, e quelli di un server non si
//! potrebbero toccare.
//!
//! Ogni campo e' una correzione a se': `None` lascia quello che dicono il
//! file e la sua scheda.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::library::{Entry, Group};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct VolumeEdit {
    pub title: Option<String>,
    /// `Some("")`: nessuna serie (un volume unico).
    pub series: Option<String>,
    /// `Some(None)`: nessun numero.
    pub number: Option<Option<u32>>,
    pub right_to_left: Option<bool>,
    /// I tag aggiunti a mano.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<Group>,
    /// I tag della scheda o del server tolti a mano.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub untags: Vec<Group>,
}

impl VolumeEdit {
    pub fn is_empty(&self) -> bool {
        *self == VolumeEdit::default()
    }

    /// Sono corretti i dati (titolo, serie, numero, verso), non solo i tag.
    pub fn has_details(&self) -> bool {
        VolumeEdit { tags: Vec::new(), untags: Vec::new(), ..self.clone() } != VolumeEdit::default()
    }

    /// Senza le correzioni dei dati: restano i tag.
    pub fn only_tags(&self) -> VolumeEdit {
        VolumeEdit { tags: self.tags.clone(), untags: self.untags.clone(), ..VolumeEdit::default() }
    }

    /// Aggiunge un tag. Uno della scheda tolto prima torna; uno che il volume
    /// ha gia' (`has`) non si ripete.
    pub fn add_tag(&mut self, tag: Group, has: &[Group]) {
        let was_untagged = self.untags.len();
        self.untags.retain(|u| !u.same(&tag));
        if self.untags.len() == was_untagged
            && !has.iter().any(|g| g.same(&tag))
            && !self.tags.iter().any(|g| g.same(&tag))
        {
            self.tags.push(tag);
        }
    }

    /// Toglie un tag: se l'aveva messo chi legge sparisce, se viene dalla
    /// scheda o dal server lo si ricorda come tolto.
    pub fn remove_tag(&mut self, tag: &Group) {
        let was_added = self.tags.len();
        self.tags.retain(|g| !g.same(tag));
        if self.tags.len() == was_added && !self.untags.iter().any(|u| u.same(tag)) {
            self.untags.push(Group { order: None, ..tag.clone() });
        }
    }

    /// I tag di un volume con queste correzioni: quelli della scheda meno i
    /// tolti, piu' gli aggiunti.
    pub fn apply_tags(&self, groups: &mut Vec<Group>) {
        groups.retain(|g| !self.untags.iter().any(|u| u.same(g)));
        for tag in &self.tags {
            if !groups.iter().any(|g| g.same(tag)) {
                groups.push(tag.clone());
            }
        }
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
        self.apply_tags(&mut e.groups);
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
            ..Entry::default()
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
            ..VolumeEdit::default()
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
        let unique = VolumeEdit {
            title: Some(" ".into()),
            series: Some(String::new()),
            number: Some(None),
            ..VolumeEdit::default()
        };
        unique.apply(&mut e);
        assert_eq!((e.title.as_str(), e.series, e.number), ("scan_0003_final", None, None));

        // tutto vuoto: la correzione se ne va
        edits.set(Path::new("/fumetti/scan_0003_final.cbz"), VolumeEdit::default()).unwrap();
        assert!(Edits::load(file).get(Path::new("/fumetti/scan_0003_final.cbz")).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn tag_aggiunti_e_tolti() {
        use crate::library::GroupKind;
        let tag = |kind, name: &str| Group::new(kind, name, None);
        // dalla scheda: un personaggio e una categoria
        let from_file = vec![tag(GroupKind::Character, "Batman"), tag(GroupKind::Genre, "Giallo")];
        let mut edit = VolumeEdit::default();
        edit.add_tag(tag(GroupKind::Character, "robin"), &from_file);
        edit.add_tag(tag(GroupKind::Character, "BATMAN"), &from_file);
        assert_eq!(edit.tags, [tag(GroupKind::Character, "robin")], "Batman c'era gia'");
        edit.remove_tag(&tag(GroupKind::Genre, "giallo"));
        assert_eq!(edit.untags.len(), 1, "un tag della scheda si ricorda come tolto");
        let mut groups = from_file.clone();
        edit.apply_tags(&mut groups);
        assert_eq!(groups, [tag(GroupKind::Character, "Batman"), tag(GroupKind::Character, "robin")]);
        // rimesso: torna quello della scheda, senza doppi
        edit.add_tag(tag(GroupKind::Genre, "Giallo"), &from_file);
        assert!(edit.untags.is_empty() && edit.tags.len() == 1);
        // tolto un tag messo a mano: sparisce e basta
        edit.remove_tag(&tag(GroupKind::Character, "Robin"));
        assert!(edit.is_empty());
        // stessa categoria conta: "Giallo" come etichetta e' un altro tag
        edit.add_tag(tag(GroupKind::Tag, "Giallo"), &from_file);
        assert_eq!(edit.tags.len(), 1);
        assert!(!edit.has_details(), "solo tag, nessun dato corretto");
        let json = serde_json::to_string(&VolumeEdit { title: Some("X".into()), ..VolumeEdit::default() }).unwrap();
        assert!(!json.contains("tags"), "senza tag il file non cambia: {json}");
    }
}
