//! I tag dei volumi: aprirli dal lettore (E) o dalla libreria, aggiungerne e
//! toglierne (stanno nelle correzioni: il file non si tocca), andare in
//! libreria fra i volumi che hanno un tag.

use fumetto::ui::{Reach, TagScope, TagsOf};
use fumetto_core::edits::VolumeEdit;
use fumetto_core::library::{Group, GroupKind, groups_of};
use fumetto_core::lingua::italian;

use super::*;

/// Quanti cambi ai tag si possono annullare (Ctrl+Z), uno dopo l'altro.
const UNDO_KEPT: usize = 30;

/// Un cambio ai tag di alcuni volumi.
#[derive(Clone)]
pub(super) enum TagChange {
    Add(Group),
    /// Solo dai volumi che ce l'hanno: sugli altri non resta niente.
    Remove(Group),
    /// Il vecchio, il nuovo; anche qui solo sui volumi che hanno il vecchio.
    Rename(Group, Group),
}

impl App {
    /// I tag del volume aperto.
    pub(super) fn edit_open_tags(&mut self) {
        if let Some(path) = self.book.as_ref().map(|b| b.path.clone()) {
            self.edit_tags(TagsOf::Volume(path));
        }
    }

    /// Apre i tag di un volume (e della sua serie), di una serie o dei
    /// volumi scelti.
    pub(super) fn edit_tags(&mut self, of: TagsOf) {
        let title = match &of {
            TagsOf::Volume(path) => self.tags_title(path),
            TagsOf::Series(name) => name.clone(),
            TagsOf::Volumes(paths) => match paths.as_slice() {
                [one] => self.tags_title(one),
                _ if italian() => format!("{} volumi scelti", paths.len()),
                _ => format!("{} picked volumes", paths.len()),
            },
        };
        let (scopes, known) = (self.tag_scopes(&of), self.known_tags());
        self.ui.edit_tags(of, title, scopes, known);
        self.changed();
    }

    /// Cambia i tag di questi volumi, lo salva, e rimanda al modulo dei tag
    /// (se e' aperto) i tag come sono. `false` se non si e' potuto salvare.
    /// Si puo' annullare con Ctrl+Z.
    pub(super) fn change_tag(&mut self, paths: &[PathBuf], change: TagChange) -> bool {
        let before = self.edits.snapshot(paths);
        let (lib, book) = (&self.lib, self.book.as_deref());
        let saved = self.edits.update(paths, |path, edit| {
            let base = file_tags(lib, book, path);
            match &change {
                TagChange::Add(tag) => edit.add_tag(tag.clone(), &base),
                TagChange::Remove(tag) if has_tag(&base, edit, tag) => edit.remove_tag(tag),
                TagChange::Rename(old, new) if has_tag(&base, edit, old) => edit.rename_tag(old, new.clone(), &base),
                TagChange::Remove(_) | TagChange::Rename(..) => {}
            }
        });
        match &saved {
            Ok(()) => {
                if self.tag_undo.len() == UNDO_KEPT {
                    self.tag_undo.remove(0);
                }
                self.tag_undo.push((before, change));
            }
            Err(e) => {
                let head = t("Impossibile salvare i tag", "Can't save the tags");
                self.ui.toast(format!("{head}: {e}"), Instant::now());
            }
        }
        self.tags_saved();
        saved.is_ok()
    }

    /// Annulla l'ultimo cambio ai tag (Ctrl+Z): le correzioni di quei
    /// volumi tornano com'erano. `false` se non c'e' niente da annullare.
    pub(super) fn undo_tags(&mut self) -> bool {
        let Some((before, change)) = self.tag_undo.pop() else { return false };
        if let Err(e) = self.edits.restore(&before) {
            let head = t("Impossibile annullare", "Can't undo");
            self.ui.toast(format!("{head}: {e}"), Instant::now());
            return true;
        }
        let what = match &change {
            TagChange::Add(tag) => tag.name.clone(),
            TagChange::Remove(tag) => tag.name.clone(),
            TagChange::Rename(old, new) => {
                // chi e' dentro il tag torna al nome di prima
                self.ui.shelf.group_renamed(new.kind, &new.name, Some(old.name.clone()));
                old.name.clone()
            }
        };
        self.tags_saved();
        let text = if italian() { format!("Annullato: «{what}»") } else { format!("Undone: \u{201c}{what}\u{201d}") };
        self.ui.toast(text, Instant::now());
        true
    }

    /// Le correzioni sono cambiate: la libreria e il modulo dei tag (se e'
    /// aperto) le mostrano.
    fn tags_saved(&mut self) {
        self.apply_edits();
        self.refresh_shelf();
        if let Some(of) = self.ui.tags_of().cloned() {
            let (scopes, known) = (self.tag_scopes(&of), self.known_tags());
            self.ui.tags_changed(scopes, known);
        }
        self.changed();
    }

    /// Salva una ricerca della libreria con un nome: sta fra i tag.
    pub(super) fn save_search(&mut self, name: &str, query: &str) {
        self.settings.save_search(name, query);
        self.save_settings();
        self.ui.form_done(Ok(()));
        let text = if italian() {
            format!("Ricerca salvata: «{}» sta fra i tag", name.trim())
        } else {
            format!("Search saved: \u{201c}{}\u{201d} is among the tags", name.trim())
        };
        self.ui.toast(text, Instant::now());
        self.changed();
    }

    /// Toglie una ricerca dalle salvate.
    pub(super) fn forget_search(&mut self, name: &str) {
        self.settings.forget_search(name);
        self.save_settings();
        let text = if italian() {
            format!("«{name}» tolta dalle ricerche salvate")
        } else {
            format!("\u{201c}{name}\u{201d} removed from the saved searches")
        };
        self.ui.toast(text, Instant::now());
        self.changed();
    }

    /// Il nome nuovo di un tag, su tutti i volumi della libreria che lo
    /// hanno; se un tag con quel nome c'e' gia', i due diventano uno.
    pub(super) fn rename_tag(&mut self, kind: GroupKind, old: String, new: String) {
        let (old, new) = (Group::new(kind, old, None), Group::new(kind, new.trim(), None));
        let paths = self.tagged(&old);
        let merged = !old.same(&new) && self.lib.owned().iter().any(|e| e.groups.iter().any(|g| g.same(&new)));
        if !self.change_tag(&paths, TagChange::Rename(old.clone(), new.clone())) {
            let error = t("Impossibile salvare il nome nuovo.", "Can't save the new name.");
            return self.ui.form_done(Err(error.into()));
        }
        self.ui.form_done(Ok(()));
        // chi era dentro il tag ci resta, con il nome nuovo
        self.ui.shelf.group_renamed(kind, &old.name, Some(new.name.clone()));
        let text = match (merged, italian()) {
            (false, true) => format!("Tag rinominato in «{}»", new.name),
            (false, false) => format!("Tag renamed to \u{201c}{}\u{201d}", new.name),
            (true, true) => format!("Tag unito a «{}»", new.name),
            (true, false) => format!("Tag merged into \u{201c}{}\u{201d}", new.name),
        };
        self.toast_undoable(text);
    }

    /// Chiede se togliere un tag da tutti i volumi che lo hanno.
    pub(super) fn confirm_untag(&mut self, kind: GroupKind, name: String) {
        let volumes = self.tagged(&Group::new(kind, name.as_str(), None)).len();
        if volumes == 0 {
            return;
        }
        if let Some(window) = &self.window
            && !self.dialog
        {
            dialog::confirm_untag(window, kind, name, volumes, self.proxy.clone());
            self.dialog = true;
        }
    }

    /// Toglie un tag da tutti i volumi della libreria che lo hanno.
    pub(super) fn untag_everywhere(&mut self, kind: GroupKind, name: String) {
        let tag = Group::new(kind, name, None);
        let paths = self.tagged(&tag);
        if !self.change_tag(&paths, TagChange::Remove(tag.clone())) {
            return;
        }
        self.ui.shelf.group_renamed(kind, &tag.name, None);
        let text = match (paths.len() == 1, italian()) {
            (true, true) => "Tag tolto da 1 volume".to_owned(),
            (false, true) => format!("Tag tolto da {} volumi", paths.len()),
            (true, false) => "Tag removed from 1 volume".to_owned(),
            (false, false) => format!("Tag removed from {} volumes", paths.len()),
        };
        self.toast_undoable(text);
    }

    /// Un tag tolto dal modulo: lo si dice, con come rimetterlo.
    pub(super) fn tag_removed(&mut self, paths: &[PathBuf], tag: Group) {
        let name = tag.name.clone();
        if self.change_tag(paths, TagChange::Remove(tag)) {
            let text = if italian() { format!("Tolto «{name}»") } else { format!("Removed \u{201c}{name}\u{201d}") };
            self.toast_undoable(text);
        }
    }

    /// Un avviso, con il tasto per annullare.
    fn toast_undoable(&mut self, text: String) {
        let key = if cfg!(target_os = "macos") { "\u{2318}Z" } else { "Ctrl+Z" };
        let hint = if italian() { format!("{key} per annullare") } else { format!("{key} to undo") };
        self.ui.toast(format!("{text}   \u{00b7}   {hint}"), Instant::now());
    }

    /// I volumi della libreria che hanno questo tag.
    fn tagged(&self, tag: &Group) -> Vec<PathBuf> {
        self.lib.owned().iter().filter(|e| e.groups.iter().any(|g| g.same(tag))).map(|e| e.path.clone()).collect()
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
        if let Some(e) = self.lib.entry(path) {
            return e.title.clone();
        }
        match &self.book {
            Some(b) if fumetto_core::absolute(&b.path) == fumetto_core::absolute(path) => b.title.clone(),
            _ => fumetto_core::title_of(path),
        }
    }

    /// Gli ambiti del modulo dei tag: il volume, e la sua serie se in
    /// libreria e' una pila; o la serie sola; o i volumi scelti. Vuoto per
    /// una serie sparita.
    fn tag_scopes(&self, of: &TagsOf) -> Vec<TagScope> {
        let mut scopes = Vec::new();
        let series = match of {
            TagsOf::Volume(path) => {
                let paths = vec![path.clone()];
                let tags = self.volume_tags(path).into_iter().map(|g| (g, 1)).collect();
                scopes.push(TagScope { reach: Reach::Volume, paths, tags });
                find_entry(self.lib.owned(), path).and_then(|e| e.series.clone())
            }
            TagsOf::Series(name) => Some(name.clone()),
            TagsOf::Volumes(paths) => {
                let all: Vec<Group> = paths.iter().flat_map(|p| self.volume_tags(p)).collect();
                let tags = count_tags(all.iter());
                scopes.push(TagScope { reach: Reach::Picked, paths: paths.clone(), tags });
                None
            }
        };
        if let Some(name) = series {
            let members: Vec<&Entry> = self.lib.owned().iter().filter(|e| e.series.as_ref() == Some(&name)).collect();
            // un volume solo non fa una serie: basta il primo ambito
            if members.len() >= 2 || (members.len() == 1 && scopes.is_empty()) {
                let paths = members.iter().map(|e| e.path.clone()).collect();
                let tags = count_tags(members.iter().flat_map(|e| &e.groups));
                scopes.push(TagScope { reach: Reach::Series(name), paths, tags });
            }
        }
        scopes
    }

    /// I tag del volume adesso: i suoi, con quelli aggiunti e tolti a mano.
    fn volume_tags(&self, path: &Path) -> Vec<Group> {
        let mut tags = file_tags(&self.lib, self.book.as_deref(), path);
        if let Some(edit) = self.edits.get(path) {
            edit.apply_tags(&mut tags);
        }
        tags
    }

    /// Tutti i tag della libreria, con quanti volumi li hanno: i suggerimenti.
    fn known_tags(&self) -> Vec<(Group, usize)> {
        count_tags(self.lib.owned().iter().flat_map(|e| &e.groups))
    }
}

/// I tag che il volume ha di suo: dalla scheda o dal server, come li ha
/// trovati la scansione; per un volume fuori dalla libreria, dalla scheda del
/// volume aperto.
fn file_tags(lib: &Lib, book: Option<&Book>, path: &Path) -> Vec<Group> {
    if let Some(e) = lib.raw_entry(path) {
        return e.groups.clone();
    }
    match book {
        Some(b) if fumetto_core::absolute(&b.path) == fumetto_core::absolute(path) => {
            b.info.as_ref().map(groups_of).unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

/// Il volume ha il tag, con le correzioni.
fn has_tag(base: &[Group], edit: &VolumeEdit, tag: &Group) -> bool {
    let mut tags = base.to_vec();
    edit.apply_tags(&mut tags);
    tags.iter().any(|g| g.same(tag))
}

/// I tag, ciascuno una volta (senza badare alle maiuscole), con quante volte
/// ci sono: nell'ordine in cui si incontrano.
fn count_tags<'a>(groups: impl Iterator<Item = &'a Group>) -> Vec<(Group, usize)> {
    let mut counted: Vec<(Group, usize)> = Vec::new();
    let mut at: HashMap<(GroupKind, String), usize> = HashMap::new();
    for g in groups {
        let key = (g.kind, g.name.to_lowercase());
        match at.get(&key) {
            Some(&i) => counted[i].1 += 1,
            None => {
                at.insert(key, counted.len());
                counted.push((Group::new(g.kind, g.name.clone(), None), 1));
            }
        }
    }
    counted
}
