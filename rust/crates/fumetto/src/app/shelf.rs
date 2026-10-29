//! La libreria: cercare i volumi nelle sue cartelle, e dove si e' arrivati
//! in ciascuno.

use super::*;

impl App {
    /// La libreria e' a schermo: aperta sopra un volume, o nessun volume e
    /// delle cartelle da mostrare.
    pub(super) fn shelf_shown(&self) -> bool {
        self.library_open || (self.book.is_none() && self.has_library())
    }

    /// La libreria ha qualcosa da mostrare: delle cartelle o dei server.
    pub(super) fn has_library(&self) -> bool {
        !self.settings.library.is_empty() || !self.settings.servers.is_empty()
    }

    /// Cerca i volumi nelle cartelle della libreria e sui server, in
    /// sottofondo. Prima le cartelle: i server (la rete) possono metterci di
    /// piu', e la libreria intanto c'e' gia'.
    pub(super) fn rescan(&mut self) {
        if !self.has_library() {
            self.lib = Lib::default();
            return;
        }
        self.lib.scanning = true;
        let roots = self.settings.library.clone();
        let servers = self.settings.servers.clone();
        let proxy = self.proxy.clone();
        let cache = self.info_cache.clone();
        std::thread::Builder::new()
            .name("libreria".into())
            .spawn(move || {
                let local = library::scan_cached(&roots, &cache);
                let mut scan = Scan { roots, servers, local, remote: None, reads: Vec::new(), failed: Vec::new() };
                if scan.servers.is_empty() {
                    scan.remote = Some(Vec::new());
                    let _ = proxy.send_event(UserEvent::Scanned(Box::new(scan)));
                    return;
                }
                let first = Scan {
                    roots: scan.roots.clone(),
                    servers: scan.servers.clone(),
                    local: scan.local.clone(),
                    remote: None,
                    reads: Vec::new(),
                    failed: Vec::new(),
                };
                let _ = proxy.send_event(UserEvent::Scanned(Box::new(first)));
                let mut remote = Vec::new();
                for s in &scan.servers {
                    match remote::volumes(s) {
                        Ok(catalog) => {
                            // senza rete: i volumi dell'ultima volta (gli scaricati si leggono)
                            if let Some(e) = &catalog.stale {
                                scan.failed.push(format!("{}: {e}", remote::shown(&s.url)));
                            }
                            remote.extend(catalog.entries);
                            scan.reads.push((catalog.reads, catalog.fetched));
                        }
                        Err(e) => scan.failed.push(format!("{}: {e}", remote::shown(&s.url))),
                    }
                }
                scan.remote = Some(remote);
                let _ = proxy.send_event(UserEvent::Scanned(Box::new(scan)));
            })
            .expect("thread della libreria");
    }

    /// Cosa ha trovato la scansione: le cartelle subito, i server quando
    /// arrivano (fino ad allora restano i volumi trovati la volta prima).
    pub(super) fn scanned(&mut self, scan: Scan) {
        // cartelle o server cambiati nel frattempo: vale la scansione dopo
        if scan.roots != self.settings.library || scan.servers != self.settings.servers {
            return;
        }
        if let Some(remote) = scan.remote {
            self.lib.remote = remote;
            self.lib.scanning = false;
        }
        // dove si e' arrivati letto altrove (sul telefono, nel browser)
        let open = self.book.as_ref().map(|b| b.path.clone());
        let mut moved = false;
        for (reads, fetched) in &scan.reads {
            moved |= remote::sync::incoming(reads, *fetched, &mut self.progress, open.as_deref());
        }
        if moved {
            self.refresh_recent();
        }
        let mut entries = scan.local;
        entries.extend(self.lib.remote.iter().cloned());
        self.lib.raw = entries;
        self.apply_edits();
        if !scan.failed.is_empty() && self.shelf_shown() {
            let head = t("Server non raggiungibile", "Server unreachable");
            self.ui.toast(format!("{head}: {}", scan.failed.join("; ")), Instant::now());
        }
        self.refresh_shelf();
        self.changed();
    }

    /// Scarica un volume del server, in sottofondo.
    pub(super) fn download(&mut self, path: PathBuf) {
        let title = fumetto_core::title_of(&path);
        let note = if fumetto_core::lingua::italian() {
            format!("Scarico \u{ab}{title}\u{bb}\u{2026}")
        } else {
            format!("Downloading \u{201c}{title}\u{201d}\u{2026}")
        };
        self.ui.toast(note, Instant::now());
        let proxy = self.proxy.clone();
        std::thread::Builder::new()
            .name("scaricamento".into())
            .spawn(move || {
                let result = remote::offline::download(&path).map(|_| ());
                let _ = proxy.send_event(UserEvent::Downloaded(path, result));
            })
            .expect("thread dello scaricamento");
        self.changed();
    }

    pub(super) fn downloaded(&mut self, path: &Path, result: Result<(), String>) {
        let title = fumetto_core::title_of(path);
        match result {
            Ok(()) => {
                let note = if fumetto_core::lingua::italian() {
                    format!("\u{ab}{title}\u{bb} si legge anche senza rete")
                } else {
                    format!("\u{201c}{title}\u{201d} can now be read offline")
                };
                self.ui.toast(note, Instant::now());
                self.refresh_shelf();
            }
            Err(e) => {
                let head = t("Impossibile scaricare", "Can't download");
                self.notify(format!("{head} \u{ab}{title}\u{bb}.\n\n{e}"));
            }
        }
        self.changed();
    }

    /// Toglie la copia scaricata (il volume resta nella libreria, dal server).
    pub(super) fn forget_download(&mut self, path: &Path) {
        // aperto, il file e' in uso (e Windows non lo lascia cancellare)
        if self.book.as_ref().is_some_and(|b| b.path == path) {
            self.close_book();
        }
        match remote::offline::forget(path) {
            Ok(()) => {
                self.ui.toast(t("Copia scaricata tolta", "Downloaded copy removed"), Instant::now());
                self.refresh_shelf();
            }
            Err(e) => self.notify(format!("{}\n\n{e}", t("Impossibile togliere la copia.", "Can't remove the copy."))),
        }
        self.changed();
    }

    /// Prova il collegamento a un server, in sottofondo: entra nella
    /// libreria solo se risponde e ha dei volumi.
    pub(super) fn check_server(&mut self, server: Server) {
        let proxy = self.proxy.clone();
        std::thread::Builder::new()
            .name("server".into())
            .spawn(move || {
                // l'elenco dell'ultima volta non vale: deve rispondere adesso
                let found = remote::volumes(&server).and_then(|c| match c.stale {
                    Some(e) => Err(e),
                    None => Ok(c.entries.len()),
                });
                let _ = proxy.send_event(UserEvent::ServerChecked(server, found));
            })
            .expect("thread del server");
    }

    /// Com'e' andata la prova.
    pub(super) fn server_checked(&mut self, server: Server, found: Result<usize, String>) {
        let found = found.and_then(|n| {
            if n > 0 {
                Ok(n)
            } else {
                Err(t(
                    "il server risponde, ma non ha volumi da sfogliare pagina per pagina (OPDS-PSE)",
                    "the server answers, but has no volumes to read page by page (OPDS-PSE)",
                )
                .to_owned())
            }
        });
        match found {
            Ok(n) => {
                self.ui.server_checked(Ok(()));
                self.settings.servers.retain(|s| s.url != server.url);
                self.settings.servers.push(server);
                remote::set_servers(&self.settings.servers);
                self.save_settings();
                self.library_open = self.book.is_some();
                self.rescan();
                let note = if fumetto_core::lingua::italian() {
                    format!("Server aggiunto: {n} volumi")
                } else {
                    format!("Server added: {n} volumes")
                };
                self.ui.toast(note, Instant::now());
            }
            Err(e) => self.ui.server_checked(Err(e)),
        }
        self.changed();
    }

    /// Dove si e' arrivati in ogni volume della libreria.
    /// La libreria con i dati corretti a mano, in ordine di titolo.
    pub(super) fn apply_edits(&mut self) {
        let mut entries = self.lib.raw.clone();
        self.edits.apply(&mut entries);
        entries.sort_by(|a, b| fumetto_core::natural_cmp(&a.title, &b.title).then_with(|| a.path.cmp(&b.path)));
        self.lib.found_from = entries.len();
        self.lib.entries = entries;
        // dopo la libreria, i volumi trovati sui server che lei non ha
        self.show_found();
        self.refresh_next();
    }

    /// Apre il modulo per correggere i dati di un volume.
    pub(super) fn edit_volume(&mut self, path: PathBuf) {
        let here = fumetto_core::absolute(&path);
        let entry = self.lib.entries.iter().find(|e| fumetto_core::absolute(&e.path) == here);
        let (title, series, number) = match entry {
            Some(e) => (e.title.clone(), e.series.clone().unwrap_or_default(), e.number),
            None => (fumetto_core::title_of(&path), String::new(), None),
        };
        let existing = self.edits.get(&path).cloned().unwrap_or_default();
        let number = number.map(|n| n.to_string()).unwrap_or_default();
        let name = match &path.file_name() {
            Some(n) if !remote::is_remote(&path) => n.to_string_lossy().into_owned(),
            _ => title.clone(),
        };
        let rtl = existing.right_to_left;
        self.ui.edit_volume(path, name, [title, series, number], existing, rtl);
        self.changed();
    }

    /// Salva i dati corretti di un volume (vuoti: tornano quelli del file).
    pub(super) fn save_volume(&mut self, path: &Path, edit: fumetto_core::edits::VolumeEdit) {
        let reset = edit.is_empty();
        let had_rtl = self.edits.get(path).and_then(|e| e.right_to_left).is_some();
        let rtl = edit.right_to_left;
        match self.edits.set(path, edit) {
            Ok(()) => {
                self.ui.server_checked(Ok(()));
                self.apply_edits();
                self.refresh_shelf();
                // il volume aperto: il verso scelto vale subito; tolto, torna quello del file
                if let (Some(book), Some(reader)) = (&self.book, &mut self.reader)
                    && fumetto_core::absolute(&book.path) == fumetto_core::absolute(path)
                    && (rtl.is_some() || had_rtl)
                {
                    reader.manga = rtl.or(book.info.as_ref().and_then(|i| i.right_to_left)).unwrap_or(false);
                }
                let note = if reset {
                    t("Tornati i dati del file", "Back to the file's details")
                } else {
                    t("Dati del volume salvati", "Volume details saved")
                };
                self.ui.toast(note, Instant::now());
            }
            Err(e) => self.ui.server_checked(Err(e.to_string())),
        }
        self.changed();
    }

    /// La vista della libreria (per cartelle, e quale) si ricorda fra un
    /// avvio e l'altro: se e' cambiata, le impostazioni vanno riscritte.
    pub(super) fn keep_library_view(&mut self) {
        let shelf = &self.ui.shelf;
        if (shelf.folders, &shelf.folder) != (self.settings.library_folders, &self.settings.library_folder) {
            (self.settings.library_folders, self.settings.library_folder) = (shelf.folders, shelf.folder.clone());
            self.settings_dirty = true;
        }
    }

    pub(super) fn refresh_shelf(&mut self) {
        let edits = &self.edits;
        self.lib.edited = self.lib.entries.iter().map(|e| edits.get(&e.path).is_some()).collect();
        let progress = &self.progress;
        self.lib.status = self.lib.entries.iter().map(|e| Status::of(progress, &e.path)).collect();
        self.lib.read_at = self.lib.entries.iter().map(|e| progress.get(&e.path).map_or(0, |s| s.read_at)).collect();
        let downloads = remote::offline::downloads();
        self.lib.offline = self.lib.entries.iter().map(|e| downloads.has(&e.path)).collect();
    }
}

/// La libreria come la vede l'interfaccia.
pub(super) fn shelf_data<'a>(
    lib: &'a Lib, roots: &'a [PathBuf], covered: &'a dyn Fn(&Path) -> bool, reading: bool, searching: bool,
) -> ShelfData<'a> {
    ShelfData {
        entries: &lib.entries,
        found_from: lib.found_from,
        searching,
        status: &lib.status,
        offline: &lib.offline,
        edited: &lib.edited,
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
