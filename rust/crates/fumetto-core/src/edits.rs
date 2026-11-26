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

    /// Cambia nome a un tag (`has`: i tag della scheda): il vecchio sparisce,
    /// il nuovo prende il suo posto con il nome come e' scritto, anche se
    /// cambiano solo le maiuscole. Se il volume ha gia' il nuovo, i due
    /// diventano uno.
    pub fn rename_tag(&mut self, old: &Group, new: Group, has: &[Group]) {
        self.tags.retain(|g| !g.same(old) && !g.same(&new));
        if has.iter().any(|g| g.same(old)) && !self.untags.iter().any(|u| u.same(old)) {
            self.untags.push(Group { order: None, ..old.clone() });
        }
        if has.iter().any(|g| g.same(&new) && g.name == new.name) {
            // la scheda lo ha gia' scritto cosi': basta non toglierlo
            self.untags.retain(|u| !u.same(&new));
        } else {
            self.tags.push(new);
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

/// Le correzioni di alcuni volumi in un momento: per annullare un cambio.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot(Vec<(PathBuf, Option<VolumeEdit>)>);

impl Snapshot {
    /// I volumi della fotografia.
    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        self.0.iter().map(|(p, _)| p.as_path())
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

    /// Cambia le correzioni di piu' volumi (una serie, tutti quelli con un
    /// tag) e scrive il file una volta sola. `change` riceve il volume e la
    /// sua correzione (vuota se non ne aveva).
    pub fn update(&mut self, paths: &[PathBuf], mut change: impl FnMut(&Path, &mut VolumeEdit)) -> io::Result<()> {
        for path in paths {
            let key = Self::key(path);
            let mut edit = self.map.remove(&key).unwrap_or_default();
            change(path, &mut edit);
            if !edit.is_empty() {
                self.map.insert(key, edit);
            }
        }
        self.save()
    }

    /// Come sono adesso le correzioni di questi volumi: per tornarci.
    pub fn snapshot(&self, paths: &[PathBuf]) -> Snapshot {
        Snapshot(paths.iter().map(|p| (p.clone(), self.get(p).cloned())).collect())
    }

    /// Rimette le correzioni come erano nella fotografia (annulla cio' che e'
    /// cambiato dopo), e le scrive.
    pub fn restore(&mut self, snapshot: &Snapshot) -> io::Result<()> {
        for (path, edit) in &snapshot.0 {
            let key = Self::key(path);
            match edit.as_ref().filter(|e| !e.is_empty()) {
                Some(edit) => self.map.insert(key, edit.clone()),
                None => self.map.remove(&key),
            };
        }
        self.save()
    }

    fn save(&self) -> io::Result<()> {
        let Some(file) = &self.file else { return Ok(()) };
        let json = serde_json::to_vec_pretty(&self.map).map_err(io::Error::other)?;
        crate::write_atomic(file, &json)
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

    #[test]
    fn piu_volumi_in_un_colpo() {
        use crate::library::{Group, GroupKind};
        let mut edits = Edits::in_memory();
        let paths: Vec<PathBuf> = (1..=3).map(|n| PathBuf::from(format!("/serie/v{n}.cbz"))).collect();
        let tag = Group::new(GroupKind::Character, "Wolverine", None);
        edits.update(&paths, |_, e| e.add_tag(tag.clone(), &[])).unwrap();
        assert!(paths.iter().all(|p| edits.get(p).is_some_and(|e| e.tags == [tag.clone()])));
        // tolto da tutti: le correzioni vuote spariscono
        edits.update(&paths, |_, e| e.remove_tag(&tag)).unwrap();
        assert!(paths.iter().all(|p| edits.get(p).is_none()));
    }

    #[test]
    fn si_torna_alla_fotografia() {
        use crate::library::{Group, GroupKind};
        let mut edits = Edits::in_memory();
        let paths: Vec<PathBuf> = (1..=2).map(|n| PathBuf::from(format!("/serie/v{n}.cbz"))).collect();
        let wolverine = Group::new(GroupKind::Character, "Wolverine", None);
        edits.update(&paths[..1], |_, e| e.add_tag(wolverine.clone(), &[])).unwrap();
        let before = edits.snapshot(&paths);
        assert_eq!(before.paths().count(), 2);
        edits.update(&paths, |_, e| e.add_tag(Group::new(GroupKind::Tag, "Letto", None), &[])).unwrap();
        edits.update(&paths[..1], |_, e| e.remove_tag(&wolverine)).unwrap();
        edits.restore(&before).unwrap();
        assert_eq!(edits.get(&paths[0]).map(|e| e.tags.clone()), Some(vec![wolverine]));
        assert!(edits.get(&paths[1]).is_none(), "chi non aveva correzioni torna a non averne");
    }

    #[test]
    fn rinominare_e_unire_un_tag() {
        use crate::library::{Group, GroupKind};
        let g = |name: &str| Group::new(GroupKind::Character, name, None);
        let shown = |edit: &VolumeEdit, has: &[Group]| {
            let mut tags = has.to_vec();
            edit.apply_tags(&mut tags);
            tags.into_iter().map(|t| t.name).collect::<Vec<_>>()
        };
        // dalla scheda a un nome nuovo
        let has = [g("Logan")];
        let mut edit = VolumeEdit::default();
        edit.rename_tag(&g("Logan"), g("Wolverine"), &has);
        assert_eq!(shown(&edit, &has), ["Wolverine"]);
        // cambiano solo le maiuscole
        let has = [g("wolverine")];
        let mut edit = VolumeEdit::default();
        edit.rename_tag(&g("wolverine"), g("Wolverine"), &has);
        assert_eq!(shown(&edit, &has), ["Wolverine"]);
        // il volume ha gia' il nuovo: ne resta uno, e la correzione sparisce
        let has = [g("Wolverine")];
        let mut edit = VolumeEdit::default();
        edit.add_tag(g("Logan"), &has);
        edit.rename_tag(&g("Logan"), g("Wolverine"), &has);
        assert_eq!(shown(&edit, &has), ["Wolverine"]);
        assert!(edit.is_empty());
        // aggiunto a mano: si rinomina senza ricordare tolti
        let mut edit = VolumeEdit::default();
        edit.add_tag(g("Logan"), &[]);
        edit.rename_tag(&g("Logan"), g("Wolverine"), &[]);
        assert_eq!((edit.tags.len(), edit.untags.len()), (1, 0));
    }
}
