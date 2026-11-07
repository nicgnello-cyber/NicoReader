//! I comandi dell'interfaccia (menu, libreria, moduli) e le scelte fatte
//! nelle impostazioni.

use super::*;

impl App {
    /// Esegue cio' che l'interfaccia ha deciso.
    pub(super) fn run(&mut self, cmd: Command, event_loop: &ActiveEventLoop) {
        match cmd {
            Command::Act(a) => self.act(a, event_loop),
            Command::Open(path) => self.open(&path),
            Command::MarkRead(path, read) => self.mark_read(&path, read),
            Command::Reveal(path) => {
                if let Err(e) = system::reveal(&path) {
                    eprintln!("mostra nella cartella: {e}");
                }
            }
            Command::Trash(path) => self.confirm_trash(path),
            Command::RemoveFolder(root) => self.remove_folder(&root),
            Command::Pref(p) => self.apply_pref(p, event_loop),
            Command::Page(p) => {
                self.thumbs_open = false;
                self.act(Action::GoTo(p), event_loop);
            }
            Command::ThumbSize(size) => {
                self.settings.thumb_size = size;
                self.settings_dirty = true;
            }
            Command::AddServer(server) => self.check_server(server),
            Command::OpenWithPassword(path, password) => self.open_with_password(&path, &password),
            Command::RemoveServer(url) => self.remove_server(&url),
            Command::Download(path) => self.download(path),
            Command::ForgetDownload(path) => self.forget_download(&path),
            Command::EditVolume(path) => self.edit_volume(path),
            Command::SaveVolume(path, edit) => self.save_volume(&path, edit),
            Command::ResetVolume(path) => self.save_volume(&path, fumetto_core::edits::VolumeEdit::default()),
            // quelli interni li ha gia' fatti l'interfaccia
            Command::Series(_)
            | Command::Group(..)
            | Command::Folder(_)
            | Command::SortMenu(..)
            | Command::Sort(_)
            | Command::FoldersMenu(..)
            | Command::AskServer => {}
        }
        self.request_redraw();
    }

    /// Segna un volume letto o da leggere (anche sul server). Serve sapere
    /// quante pagine ha: dai progressi, o aprendolo.
    fn mark_read(&mut self, path: &Path, read: bool) {
        let pages = self
            .progress
            .get(path)
            .map(|s| s.pages)
            .filter(|&n| n > 0)
            .or_else(|| Book::open(path).ok().map(|b| b.len()));
        let Some(pages) = pages else {
            return self.notify(
                t(
                    "Questo volume non si apre: non so quante pagine abbia.",
                    "This volume doesn't open: its page count is unknown.",
                )
                .to_owned(),
            );
        };
        self.progress.mark(path, read, pages);
        remote::sync::report_mark(path, read);
        // il volume aperto: da qui il lettore non lo riscrive, finche' non ci si muove
        if let (Some(book), Some(reader)) = (&self.book, &self.reader)
            && fumetto_core::absolute(&book.path) == fumetto_core::absolute(path)
        {
            self.marked = Some((book.path.clone(), reader.snapshot()));
        }
        self.refresh_shelf();
        self.refresh_recent();
        let note =
            if read { t("Segnato come letto", "Marked as read") } else { t("Di nuovo da leggere", "Unread again") };
        self.ui.toast(note, Instant::now());
    }

    /// Prima di spostare un volume nel cestino si chiede (una finestra alla volta).
    fn confirm_trash(&mut self, path: PathBuf) {
        if let Some(window) = &self.window
            && !self.dialog
        {
            dialog::confirm_trash(window, path, self.proxy.clone());
            self.dialog = true;
        }
    }

    fn remove_folder(&mut self, root: &Path) {
        self.settings.library.retain(|r| r != root);
        self.save_settings();
        self.rescan();
        self.ui.toast(t("Cartella tolta dalla libreria", "Folder removed from the library"), Instant::now());
        self.changed();
    }

    /// Toglie un server dalla libreria; la sua password esce anche dal portachiavi.
    fn remove_server(&mut self, url: &str) {
        for s in self.settings.servers.iter().filter(|s| s.url == url && !s.user.is_empty()) {
            fumetto_core::keychain::delete(&s.url, &s.user);
        }
        self.settings.servers.retain(|s| s.url != url);
        remote::set_servers(&self.settings.servers);
        self.save_settings();
        self.rescan();
        self.ui.toast(t("Server tolto dalla libreria", "Server removed from the library"), Instant::now());
        self.changed();
    }

    /// Applica una scelta fatta nelle impostazioni.
    fn apply_pref(&mut self, p: Pref, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let s = &mut self.settings;
        match p {
            Pref::Brightness(v) => s.brightness = v,
            Pref::Contrast(v) => s.contrast = v,
            Pref::Gamma(v) => s.gamma = v,
            Pref::ResetImage => (s.brightness, s.contrast, s.gamma) = (0, 0, 0),
            Pref::LensZoom(v) => s.lens_zoom = v,
            Pref::LensSize(v) => s.lens_size = v,
            Pref::Slideshow(v) => {
                s.slideshow = v;
                if self.slideshow.is_some() {
                    self.slideshow = Some(now + Duration::from_secs(v as u64));
                }
            }
            Pref::Webtoon(on) => {
                s.webtoon = on;
                self.save_settings();
            }
            Pref::CheckUpdates(on) => {
                s.check_updates = on;
                self.save_settings();
                self.check_update();
            }
            // queste passano per le loro azioni: fanno anche il resto
            // (rifare le pagine, spostarle sotto la barra, scaricare l'AI)
            Pref::Trim(on) => match &self.reader {
                Some(r) if r.trim != on => self.act(Action::ToggleTrim, event_loop),
                Some(_) => {}
                None => {
                    s.trim = on;
                    self.save_settings();
                }
            },
            Pref::Hud(on) if on != s.hud => self.act(Action::ToggleHud, event_loop),
            Pref::Hud(_) => {}
            Pref::Upscale(on) if on != s.upscale => self.toggle_upscale(),
            Pref::Upscale(_) => {}
            Pref::Bind(bind, combo) => {
                let taken = keys::assign(&mut s.keys, bind, &combo);
                self.keys_changed(bound_note(bind, &combo, taken));
            }
            Pref::Unbind(bind) => {
                keys::clear(&mut s.keys, bind);
                self.keys_changed(format!("{}: {}", bind.label(), t("nessun tasto", "no key")));
            }
            Pref::ResetKeys => {
                s.keys.clear();
                self.keys_changed(t("Tutti i tasti di serie", "All default keys").to_owned());
            }
        }
        // le regolazioni trascinate cambiano a ogni movimento: si scrivono dopo
        self.settings_dirty = true;
        self.request_redraw();
    }

    /// I tasti sono cambiati: la mappa si rifa', si salva, e una riga dice come.
    fn keys_changed(&mut self, note: String) {
        self.keymap = Keymap::new(&self.settings.keys);
        self.save_settings();
        self.ui.toast(note, Instant::now());
    }
}

/// "Ctrl+J ora fa «Avanti», non più «Indietro»"; o, se il tasto era libero,
/// "Avanti: Ctrl+J".
fn bound_note(bind: Bind, combo: &Combo, taken: Option<Bind>) -> String {
    match taken {
        Some(from) if fumetto_core::lingua::italian() => {
            format!("{} ora fa \u{ab}{}\u{bb}, non più \u{ab}{}\u{bb}", combo.shown(), bind.label(), from.label())
        }
        Some(from) => format!(
            "{} now does \u{201c}{}\u{201d}, no longer \u{201c}{}\u{201d}",
            combo.shown(),
            bind.label(),
            from.label()
        ),
        None => format!("{}: {}", bind.label(), combo.shown()),
    }
}
