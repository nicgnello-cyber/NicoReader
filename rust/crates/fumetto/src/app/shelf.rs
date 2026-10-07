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
                let mut scan = Scan { roots, servers, local, remote: None, failed: Vec::new() };
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
                    failed: Vec::new(),
                };
                let _ = proxy.send_event(UserEvent::Scanned(Box::new(first)));
                let mut remote = Vec::new();
                for s in &scan.servers {
                    match remote::volumes(s) {
                        Ok(found) => remote.extend(found),
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
        let mut entries = scan.local;
        entries.extend(self.lib.remote.iter().cloned());
        entries.sort_by(|a, b| fumetto_core::natural_cmp(&a.title, &b.title).then_with(|| a.path.cmp(&b.path)));
        self.lib.entries = entries;
        if !scan.failed.is_empty() && self.shelf_shown() {
            let head = t("Server non raggiungibile", "Server unreachable");
            self.ui.toast(format!("{head}: {}", scan.failed.join("; ")), Instant::now());
        }
        self.refresh_shelf();
        self.changed();
    }

    /// Prova il collegamento a un server, in sottofondo: entra nella
    /// libreria solo se risponde e ha dei volumi.
    pub(super) fn check_server(&mut self, server: Server) {
        let proxy = self.proxy.clone();
        std::thread::Builder::new()
            .name("server".into())
            .spawn(move || {
                let found = remote::volumes(&server).map(|v| v.len());
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
    pub(super) fn refresh_shelf(&mut self) {
        let progress = &self.progress;
        self.lib.status = self.lib.entries.iter().map(|e| Status::of(progress, &e.path)).collect();
        self.lib.read_at = self.lib.entries.iter().map(|e| progress.get(&e.path).map_or(0, |s| s.read_at)).collect();
    }
}

/// La libreria come la vede l'interfaccia.
pub(super) fn shelf_data<'a>(
    lib: &'a Lib, roots: &'a [PathBuf], covered: &'a dyn Fn(&Path) -> bool, reading: bool,
) -> ShelfData<'a> {
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
