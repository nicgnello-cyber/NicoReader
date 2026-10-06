//! La libreria: cercare i volumi nelle sue cartelle, e dove si e' arrivati
//! in ciascuno.

use super::*;

impl App {
    /// La libreria e' a schermo: aperta sopra un volume, o nessun volume e
    /// delle cartelle da mostrare.
    pub(super) fn shelf_shown(&self) -> bool {
        self.library_open || (self.book.is_none() && !self.settings.library.is_empty())
    }

    /// Cerca i volumi nelle cartelle della libreria, in sottofondo.
    pub(super) fn rescan(&mut self) {
        if self.settings.library.is_empty() {
            self.lib = Lib::default();
            return;
        }
        self.lib.scanning = true;
        let roots = self.settings.library.clone();
        let proxy = self.proxy.clone();
        let cache = self.info_cache.clone();
        std::thread::Builder::new()
            .name("libreria".into())
            .spawn(move || {
                let found = library::scan_cached(&roots, &cache);
                let _ = proxy.send_event(UserEvent::Scanned(roots, found));
            })
            .expect("thread della libreria");
    }

    /// Dove si e' arrivati in ogni volume della libreria.
    pub(super) fn refresh_shelf(&mut self) {
        let progress = &self.progress;
        self.lib.status = self.lib.entries.iter().map(|e| Status::of(progress, &e.path)).collect();
        self.lib.read_at = self.lib.entries.iter().map(|e| progress.get(&e.path).map_or(0, |s| s.read_at)).collect();
    }
}

/// La libreria come la vede l'interfaccia.
pub(super) fn shelf_data<'a>(lib: &'a Lib, roots: &'a [PathBuf], covered: &'a dyn Fn(&Path) -> bool, reading: bool) -> ShelfData<'a> {
    ShelfData {
        entries: &lib.entries,
        status: &lib.status,
        read_at: &lib.read_at,
        scanning: lib.scanning,
        roots,
        covered,
        reading,
    }
}

/// Per i tasti e i clic le copertine non contano.
pub(super) fn always(_: &Path) -> bool {
    true
}
