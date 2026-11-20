//! I tag dei volumi: aprirli dal lettore (E) o dalla libreria, aggiungerne e
//! toglierne (stanno nelle correzioni: il file non si tocca), andare in
//! libreria fra i volumi che hanno un tag.

use fumetto_core::library::{Group, GroupKind, groups_of};

use super::*;

/// Un cambio ai tag di un volume.
pub(super) enum TagChange {
    Add(Group),
    Remove(Group),
}

impl App {
    /// I tag del volume aperto.
    pub(super) fn edit_open_tags(&mut self) {
        if let Some(path) = self.book.as_ref().map(|b| b.path.clone()) {
            self.edit_tags(path);
        }
    }

    /// Apre i tag di un volume.
    pub(super) fn edit_tags(&mut self, path: PathBuf) {
        let title = self.tags_title(&path);
        let (tags, known) = (self.volume_tags(&path), self.known_tags());
        self.ui.edit_tags(path, title, tags, known);
        self.changed();
    }

    /// Aggiunge o toglie un tag, lo salva, e rimanda al modulo i tag come sono.
    pub(super) fn change_tag(&mut self, path: &Path, change: TagChange) {
        let base = self.file_tags(path);
        let mut edit = self.edits.get(path).cloned().unwrap_or_default();
        match change {
            TagChange::Add(tag) => edit.add_tag(tag, &base),
            TagChange::Remove(tag) => edit.remove_tag(&tag),
        }
        if let Err(e) = self.edits.set(path, edit) {
            let head = t("Impossibile salvare i tag", "Can't save the tags");
            self.ui.toast(format!("{head}: {e}"), Instant::now());
        }
        self.apply_edits();
        self.refresh_shelf();
        let (tags, known) = (self.volume_tags(path), self.known_tags());
        self.ui.tags_changed(path, tags, known);
        self.changed();
    }

    /// La libreria, fra i volumi che hanno questo tag.
    pub(super) fn show_group(&mut self, kind: GroupKind, name: String) {
        self.ui.close_tags();
        if !self.has_library() {
            let text = t(
                "Aggiungi una cartella alla libreria per ritrovare gli altri volumi con questo tag",
                "Add a folder to the library to find the other volumes with this tag",
            );
            self.ui.toast(text, Instant::now());
            return self.changed();
        }
        self.thumbs_open = false;
        if self.book.is_some() {
            self.library_open = true;
        }
        self.refresh_shelf();
        self.ui.reset();
        self.ui.shelf.enter_group(kind, name);
        self.changed();
    }

    /// Il nome del volume in testa al modulo.
    fn tags_title(&self, path: &Path) -> String {
        let here = fumetto_core::absolute(path);
        if let Some(e) = self.lib.entries.iter().find(|e| fumetto_core::absolute(&e.path) == here) {
            return e.title.clone();
        }
        match &self.book {
            Some(b) if fumetto_core::absolute(&b.path) == here => b.title.clone(),
            _ => fumetto_core::title_of(path),
        }
    }

    /// I tag che il volume ha di suo: dalla scheda o dal server, come li ha
    /// trovati la scansione; per un volume fuori dalla libreria, dalla scheda
    /// del volume aperto.
    fn file_tags(&self, path: &Path) -> Vec<Group> {
        let here = fumetto_core::absolute(path);
        if let Some(e) = self.lib.raw.iter().find(|e| fumetto_core::absolute(&e.path) == here) {
            return e.groups.clone();
        }
        match &self.book {
            Some(b) if fumetto_core::absolute(&b.path) == here => b.info.as_ref().map(groups_of).unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// I tag del volume adesso: i suoi, con quelli aggiunti e tolti a mano.
    fn volume_tags(&self, path: &Path) -> Vec<Group> {
        let mut tags = self.file_tags(path);
        if let Some(edit) = self.edits.get(path) {
            edit.apply_tags(&mut tags);
        }
        tags
    }

    /// Tutti i tag della libreria, con quanti volumi li hanno: i suggerimenti.
    fn known_tags(&self) -> Vec<(Group, usize)> {
        let mut known: Vec<(Group, usize)> = Vec::new();
        let mut at: HashMap<(GroupKind, String), usize> = HashMap::new();
        for e in &self.lib.entries[..self.lib.found_from.min(self.lib.entries.len())] {
            for g in &e.groups {
                let key = (g.kind, g.name.to_lowercase());
                match at.get(&key) {
                    Some(&i) => known[i].1 += 1,
                    None => {
                        at.insert(key, known.len());
                        known.push((Group::new(g.kind, g.name.clone(), None), 1));
                    }
                }
            }
        }
        known
    }
}
