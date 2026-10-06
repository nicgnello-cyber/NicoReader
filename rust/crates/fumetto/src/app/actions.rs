//! Cosa fanno azioni, comandi e regolazioni; da quali tasti e clic
//! vengono.

use super::*;

impl App {
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
                self.keymap = Keymap::new(&self.settings.keys);
                self.save_settings();
                let text = match taken {
                    Some(from) if fumetto_core::lingua::italian() => {
                        format!("{} ora fa \u{ab}{}\u{bb}, non più \u{ab}{}\u{bb}", combo.shown(), bind.label(), from.label())
                    }
                    Some(from) => format!("{} now does \u{201c}{}\u{201d}, no longer \u{201c}{}\u{201d}", combo.shown(),
                                          bind.label(), from.label()),
                    None => format!("{}: {}", bind.label(), combo.shown()),
                };
                self.ui.toast(text, now);
            }
            Pref::Unbind(bind) => {
                keys::clear(&mut s.keys, bind);
                self.keymap = Keymap::new(&self.settings.keys);
                self.save_settings();
                self.ui.toast(format!("{}: {}", bind.label(), t("nessun tasto", "no key")), now);
            }
            Pref::ResetKeys => {
                s.keys.clear();
                self.keymap = Keymap::new(&self.settings.keys);
                self.save_settings();
                self.ui.toast(t("Tutti i tasti di serie", "All default keys"), now);
            }
        }
        // le regolazioni trascinate cambiano a ogni movimento: si scrivono dopo
        self.settings_dirty = true;
        self.request_redraw();
    }

    pub(super) fn update_title(&mut self) {
        let Some(w) = &self.window else { return };
        let title = match (&self.book, &self.reader) {
            _ if self.shelf_shown() => format!("NicoReader \u{2014} {}", t("Libreria", "Library")),
            (Some(book), Some(reader)) => reader.title(&book.title),
            _ => "NicoReader".to_owned(),
        };
        if title != self.title {
            w.set_title(&title);
            self.title = title;
        }
    }

    pub fn act(&mut self, action: Action, event_loop: &ActiveEventLoop) {
        // aprire, chiudere, la libreria, vai a pagina: le miniature si chiudono
        if matches!(action, Action::Open | Action::OpenFolder | Action::Close | Action::ToggleLibrary | Action::AskPage) {
            self.thumbs_open = false;
        }
        match action {
            Action::Quit => return event_loop.exit(),
            Action::ToggleThumbs => return self.toggle_thumbs(),
            Action::ToggleSettings => {
                self.ui.toggle_prefs();
                return self.request_redraw();
            }
            Action::ToggleLens => return self.toggle_lens(),
            Action::ToggleSlideshow => return self.toggle_slideshow(),
            Action::ToggleUpscale => return self.toggle_upscale(),
            Action::ToggleWebtoon => {
                self.settings.webtoon = !self.settings.webtoon;
                self.save_settings();
                let note = if self.settings.webtoon {
                    t("I webtoon si leggeranno a nastro", "Webtoons will open as a strip")
                } else {
                    t("Webtoon: nessun riconoscimento", "Webtoons: no detection")
                };
                self.ui.toast(note, Instant::now());
                return self.request_redraw();
            }
            Action::SavePage => return self.ask_save(),
            Action::CopyPage => return self.copy_page(),
            Action::ToggleFullscreen => {
                if let Some(w) = &self.window {
                    w.set_fullscreen(match w.fullscreen() {
                        Some(_) => None,
                        None => Some(Fullscreen::Borderless(None)),
                    });
                }
                return;
            }
            Action::Open | Action::OpenFolder => return self.ask(action == Action::OpenFolder),
            Action::Close => return self.close_book(),
            Action::ToggleHud => {
                self.settings.hud = !self.settings.hud;
                if let Some(path) = &self.settings_path
                    && let Err(e) = self.settings.save(path)
                {
                    eprintln!("impostazioni non salvate: {e}");
                }
                self.fit_view();
                let note = if self.settings.hud {
                    t("Barra in alto", "Top bar")
                } else {
                    t("Barra nascosta: H per riaverla", "Bar hidden: H brings it back")
                };
                self.ui.toast(note, Instant::now());
                return self.changed();
            }
            Action::ToggleLibrary => {
                // senza un volume aperto la libreria e' gia' li' (se ci sono cartelle)
                if self.book.is_some() {
                    self.library_open = !self.library_open;
                }
                if self.shelf_shown() {
                    self.refresh_shelf();
                    self.rescan();
                } else if self.book.is_none() {
                    self.ask_library_folder();
                }
                self.ui.reset();
                return self.changed();
            }
            Action::AddLibraryFolder => return self.ask_library_folder(),
            Action::AskPage => {
                if let Some(r) = &self.reader {
                    self.ui.ask_page(r.here(), r.pages(), r.bookmarks());
                }
                return self.request_redraw();
            }
            _ => {}
        }
        let Some(reader) = &mut self.reader else { return };
        let now = Instant::now();
        if reader.act(action, now) {
            self.pending_turn = Some(PendingTurn { since: now, missed: false });
        }
        if action == Action::ToggleTrim {
            self.settings.trim = reader.trim;
        }
        // chi gira pagina a mano ha tutto il tempo della presentazione per la nuova
        if self.slideshow.is_some()
            && matches!(action, Action::Next | Action::Prev | Action::First | Action::Last | Action::GoTo(_) | Action::Scroll(_))
        {
            self.slideshow = Some(now + Duration::from_secs(self.settings.slideshow.max(1) as u64));
        }
        let reader = self.reader.as_ref().expect("appena usato");
        // cio' che cambia il modo lo dice una riga: a schermo intero il
        // titolo della finestra non si vede
        if let Some(note) = note(action, reader) {
            self.ui.toast(note, now);
        }
        if action == Action::ToggleTrim {
            self.save_settings();
        }
        self.changed();
    }

    /// Esegue cio' che l'interfaccia ha deciso.
    pub(super) fn run(&mut self, cmd: Command, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        match cmd {
            Command::Act(a) => self.act(a, event_loop),
            Command::Open(path) => self.open(&path),
            Command::MarkRead(path, read) => {
                // per segnarlo letto serve sapere quante pagine ha
                let pages = self.progress.get(&path).map(|s| s.pages).filter(|&n| n > 0)
                    .or_else(|| Book::open(&path).ok().map(|b| b.len()));
                match pages {
                    Some(n) => {
                        self.progress.mark(&path, read, n);
                        // il volume aperto: da qui il lettore non lo riscrive, finche' non ci si muove
                        let same = |a: &Path| std::path::absolute(a).ok() == std::path::absolute(&path).ok();
                        if let (Some(book), Some(reader)) = (&self.book, &self.reader)
                            && same(&book.path)
                        {
                            self.marked = Some((book.path.clone(), reader.snapshot()));
                        }
                        self.refresh_shelf();
                        self.refresh_recent();
                        self.ui.toast(if read { t("Segnato come letto", "Marked as read") } else { t("Di nuovo da leggere", "Unread again") }, now);
                    }
                    None => self.notify(t("Questo volume non si apre: non so quante pagine abbia.",
                                          "This volume doesn't open: its page count is unknown.").to_owned()),
                }
            }
            Command::Reveal(path) => {
                if let Err(e) = system::reveal(&path) {
                    eprintln!("mostra nella cartella: {e}");
                }
            }
            Command::Trash(path) => {
                if let Some(window) = &self.window
                    && !self.dialog
                {
                    dialog::confirm_trash(window, path, self.proxy.clone());
                    self.dialog = true;
                }
            }
            Command::RemoveFolder(root) => {
                self.settings.library.retain(|r| *r != root);
                self.save_settings();
                self.rescan();
                self.ui.toast(t("Cartella tolta dalla libreria", "Folder removed from the library"), now);
                self.changed();
            }
            Command::Pref(p) => self.apply_pref(p, event_loop),
            Command::Page(p) => {
                self.thumbs_open = false;
                self.act(Action::GoTo(p), event_loop);
            }
            // quelli interni li ha gia' fatti l'interfaccia
            Command::Series(_) | Command::FoldersMenu(..) => {}
        }
        self.request_redraw();
    }

    /// Il comando di un tasto premuto, dalla mappa dei tasti, e la sua azione adesso.
    pub(super) fn key(&self, event: &winit::event::KeyEvent) -> Option<Action> {
        let bind = self.keymap.lookup(&combo_of(event, self.modifiers)?)?;
        // con la lente accesa, Esc la spegne
        if bind == Bind::Quit && self.lens {
            return Some(Action::ToggleLens);
        }
        let reader = self.reader.as_ref();
        let k = KeyContext {
            manga: reader.is_some_and(|r| r.manga),
            zoomed: reader.is_some_and(|r| r.zoomed()),
            fullscreen: self.window.as_ref().and_then(|w| w.fullscreen()).is_some(),
            view: (self.view_size().0 as f32, self.view_size().1 as f32),
        };
        Some(keys::action(bind, &k))
    }

    /// Un clic dove non gira pagina (il terzo centrale, il nastro, la pagina
    /// ingrandita): se e' il secondo di un doppio clic, schermo intero.
    pub(super) fn double_click(&mut self, x: f32, y: f32) -> Option<Action> {
        let now = Instant::now();
        let double = self.last_click.take().is_some_and(|(t, px, py)| {
            now - t <= DOUBLE_CLICK && (x - px).abs() + (y - py).abs() < 10.0 * self.scale()
        });
        if double {
            return Some(Action::ToggleFullscreen);
        }
        self.last_click = Some((now, x, y));
        None
    }

    pub(super) fn click(&self, x: f32) -> Option<Action> {
        let reader = self.reader.as_ref()?;
        if reader.mode == Mode::Strip || reader.zoomed() {
            return None;
        }
        let third = self.view_size().0 as f32 / 3.0;
        let (right, left) = if reader.manga { (Action::Prev, Action::Next) } else { (Action::Next, Action::Prev) };
        match x {
            x if x < third => Some(left),
            x if x > 2.0 * third => Some(right),
            _ => None,
        }
    }
}

/// Dove si era arrivati in un volume: "24 / 212", o "letto".
pub(super) fn place(s: &Saved) -> String {
    match s.pages {
        0 => String::new(),
        n if s.page + 1 >= n => t("letto", "read").to_owned(),
        n => format!("{} / {n}", s.page + 1),
    }
}

/// La riga che dice cosa ha cambiato un tasto.
fn note(action: Action, r: &Reader) -> Option<String> {
    let pick = |on: bool, yes: &str, no: &str| (if on { yes } else { no }).to_owned();
    Some(match action {
        Action::ToggleDouble => pick(r.mode == Mode::Double, t("Doppia pagina", "Two pages"), t("Pagina singola", "Single page")),
        Action::ToggleStrip => pick(r.mode == Mode::Strip, t("Nastro", "Strip"), t("Pagina singola", "Single page")),
        Action::ToggleManga => pick(r.manga, t("Da destra a sinistra", "Right to left"), t("Da sinistra a destra", "Left to right")),
        Action::ToggleCover if r.mode == Mode::Double => {
            pick(r.cover_alone(), t("Copertina da sola", "Cover alone"), t("Copertina in coppia", "Cover paired"))
        }
        Action::ToggleLinear => pick(r.linear, t("Luce lineare", "Linear light"), t("Gamma, per confronto", "Gamma, for comparison")),
        Action::Rotate(true) => t("Girata a destra", "Rotated right").to_owned(),
        Action::Rotate(false) => t("Girata a sinistra", "Rotated left").to_owned(),
        Action::ToggleTrim if r.mode == Mode::Strip => {
            pick(r.trim, t("Margini rifilati (non nel nastro)", "Margins trimmed (not in the strip)"), t("Pagine intere", "Whole pages"))
        }
        Action::ToggleTrim => pick(r.trim, t("Margini rifilati", "Margins trimmed"), t("Pagine intere", "Whole pages")),
        Action::ToggleBookmark => pick(r.bookmarked(), t("Pagina segnata", "Page bookmarked"), t("Segno tolto", "Bookmark removed")),
        Action::Zoom { .. } | Action::ZoomIn | Action::ZoomOut | Action::ZoomTo(_) | Action::ZoomReset
        | Action::StripWider(_) => format!("Zoom {}%", r.zoom_percent()?),
        _ => return None,
    })
}

/// Il tasto premuto come lo conosce la mappa dei tasti; `None` per i
/// modificatori da soli e i tasti senza nome. Con Ctrl o Alt conta il tasto
/// senza modificatori (Ctrl+Maiusc+O e' "O", non un carattere di controllo);
/// senza, il carattere scritto ("+" anche se sulla tastiera e' Maiusc+=).
pub(super) fn combo_of(event: &winit::event::KeyEvent, m: ModifiersState) -> Option<Combo> {
    let ctrl = if cfg!(target_os = "macos") { m.super_key() } else { m.control_key() };
    let alt = m.alt_key();
    let key = if ctrl || alt { event.key_without_modifiers() } else { event.logical_key.clone() };
    let name = match &key {
        Key::Character(c) => c.to_string(),
        Key::Named(n) => match n {
            NamedKey::ArrowRight => "Right".into(),
            NamedKey::ArrowLeft => "Left".into(),
            NamedKey::ArrowUp => "Up".into(),
            NamedKey::ArrowDown => "Down".into(),
            NamedKey::Space => "Space".into(),
            NamedKey::PageUp | NamedKey::PageDown | NamedKey::Home | NamedKey::End | NamedKey::Escape
            | NamedKey::Backspace | NamedKey::Enter | NamedKey::Delete | NamedKey::Insert | NamedKey::Tab => format!("{n:?}"),
            // F1..F24: il nome e' gia' quello
            other => {
                let name = format!("{other:?}");
                let f_key = name.strip_prefix('F').is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()));
                if !f_key {
                    return None;
                }
                name
            }
        },
        _ => return None,
    };
    Combo::new(&name, ctrl, m.shift_key(), alt)
}

/// I tasti che l'interfaccia capisce.
pub(super) fn ui_key(key: &Key) -> Option<ui::Key> {
    Some(match key {
        Key::Named(NamedKey::ArrowUp) => ui::Key::Up,
        Key::Named(NamedKey::ArrowDown) => ui::Key::Down,
        Key::Named(NamedKey::PageUp) => ui::Key::PageUp,
        Key::Named(NamedKey::PageDown) => ui::Key::PageDown,
        Key::Named(NamedKey::Home) => ui::Key::Home,
        Key::Named(NamedKey::End) => ui::Key::End,
        Key::Named(NamedKey::Enter) => ui::Key::Enter,
        Key::Named(NamedKey::Escape) => ui::Key::Escape,
        Key::Named(NamedKey::Backspace) => ui::Key::Backspace,
        Key::Named(NamedKey::ArrowLeft) => ui::Key::Left,
        Key::Named(NamedKey::ArrowRight) => ui::Key::Right,
        Key::Named(NamedKey::Space) => ui::Key::Char(' '),
        // le cifre valgono per "vai a pagina"; le lettere per cercare nella libreria
        Key::Character(c) => {
            let ch = c.chars().next()?;
            match ch.to_digit(10) {
                Some(d) => ui::Key::Digit(d as u8),
                None => ui::Key::Char(ch),
            }
        }
        _ => return None,
    })
}
