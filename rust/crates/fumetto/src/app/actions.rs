//! Cosa fanno le azioni; da quali tasti e clic
//! vengono.

use super::*;

impl App {
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
        if matches!(action, Action::Open | Action::OpenFolder | Action::Close | Action::ToggleLibrary | Action::AskPage)
        {
            self.thumbs_open = false;
        }
        if action == Action::Quit {
            return event_loop.exit();
        }
        if self.app_action(action) || self.end_of_volume(action) {
            return;
        }
        self.reader_action(action);
    }

    /// Le azioni della finestra, dei volumi e degli strumenti: `false` se
    /// l'azione e' del lettore.
    fn app_action(&mut self, action: Action) -> bool {
        match action {
            Action::ToggleThumbs => self.toggle_thumbs(),
            Action::ToggleSettings => {
                self.ui.toggle_prefs();
                self.request_redraw();
            }
            Action::ToggleLens => self.toggle_lens(),
            Action::ToggleSlideshow => self.toggle_slideshow(),
            Action::ToggleUpscale => self.toggle_upscale(),
            Action::ToggleWebtoon => self.toggle_webtoon(),
            Action::SavePage => self.ask_save(),
            Action::CopyPage => self.copy_page(),
            Action::ToggleFullscreen => {
                if let Some(w) = &self.window {
                    w.set_fullscreen(match w.fullscreen() {
                        Some(_) => None,
                        None => Some(Fullscreen::Borderless(None)),
                    });
                }
            }
            Action::Open | Action::OpenFolder => self.ask(action == Action::OpenFolder),
            Action::Close => self.close_book(),
            Action::ToggleHud => self.toggle_hud(),
            Action::ToggleLibrary => self.toggle_library(),
            Action::AddLibraryFolder => self.ask_library_folder(),
            Action::AskPage => {
                if let Some(r) = &self.reader {
                    self.ui.ask_page(r.here(), r.pages(), r.bookmarks());
                }
                self.request_redraw();
            }
            _ => return false,
        }
        true
    }

    /// Un'azione del lettore: girare pagina, cambiare modo, lo zoom. Cio'
    /// che cambia il modo lo dice una riga: a schermo intero il titolo della
    /// finestra non si vede.
    fn reader_action(&mut self, action: Action) {
        let Some(reader) = &mut self.reader else { return };
        let now = Instant::now();
        if reader.act(action, now) {
            self.pending_turn = Some(PendingTurn { since: now, missed: false });
        }
        if action == Action::ToggleTrim {
            self.settings.trim = reader.trim;
            self.save_settings();
        }
        if action == Action::ToggleManga {
            self.remember_direction();
        }
        // chi gira pagina a mano ha tutto il tempo della presentazione per la nuova
        if self.slideshow.is_some()
            && matches!(
                action,
                Action::Next | Action::Prev | Action::First | Action::Last | Action::GoTo(_) | Action::Scroll(_)
            )
        {
            self.slideshow = Some(now + Duration::from_secs(self.settings.slideshow.max(1) as u64));
        }
        if let Some(note) = self.reader.as_ref().and_then(|r| note(action, r)) {
            self.ui.toast(note, now);
        }
        self.changed();
    }

    /// Il verso corretto a mano vale a ogni apertura: cambiandolo, cambia
    /// anche nella correzione.
    fn remember_direction(&mut self) {
        let (Some(book), Some(reader)) = (&self.book, &self.reader) else { return };
        let Some(mut edit) = self.edits.get(&book.path).filter(|e| e.right_to_left.is_some()).cloned() else { return };
        edit.right_to_left = Some(reader.manga);
        if let Err(e) = self.edits.set(&book.path, edit) {
            eprintln!("correzioni non salvate: {e}");
        }
    }

    fn toggle_webtoon(&mut self) {
        self.settings.webtoon = !self.settings.webtoon;
        self.save_settings();
        let note = if self.settings.webtoon {
            t("I webtoon si leggeranno a nastro", "Webtoons will open as a strip")
        } else {
            t("Webtoon: nessun riconoscimento", "Webtoons: no detection")
        };
        self.ui.toast(note, Instant::now());
        self.request_redraw();
    }

    /// La barra in alto: le pagine si spostano sotto di lei, o tornano su.
    fn toggle_hud(&mut self) {
        self.settings.hud = !self.settings.hud;
        self.save_settings();
        self.fit_view();
        let note = if self.settings.hud {
            t("Barra in alto", "Top bar")
        } else {
            t("Barra nascosta: H per riaverla", "Bar hidden: H brings it back")
        };
        self.ui.toast(note, Instant::now());
        self.changed();
    }

    /// La libreria, o di nuovo il volume. Senza un volume aperto la libreria
    /// e' gia' li' (se ci sono cartelle; se no, si chiede quale aggiungere).
    fn toggle_library(&mut self) {
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
        self.changed();
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
        Action::ToggleDouble => {
            pick(r.mode == Mode::Double, t("Doppia pagina", "Two pages"), t("Pagina singola", "Single page"))
        }
        Action::ToggleStrip => pick(r.mode == Mode::Strip, t("Nastro", "Strip"), t("Pagina singola", "Single page")),
        Action::ToggleManga => {
            pick(r.manga, t("Da destra a sinistra", "Right to left"), t("Da sinistra a destra", "Left to right"))
        }
        Action::ToggleCover if r.mode == Mode::Double => {
            pick(r.cover_alone(), t("Copertina da sola", "Cover alone"), t("Copertina in coppia", "Cover paired"))
        }
        Action::ToggleLinear => {
            pick(r.linear, t("Luce lineare", "Linear light"), t("Gamma, per confronto", "Gamma, for comparison"))
        }
        Action::Rotate(true) => t("Girata a destra", "Rotated right").to_owned(),
        Action::Rotate(false) => t("Girata a sinistra", "Rotated left").to_owned(),
        Action::ToggleTrim if r.mode == Mode::Strip => pick(
            r.trim,
            t("Margini rifilati (non nel nastro)", "Margins trimmed (not in the strip)"),
            t("Pagine intere", "Whole pages"),
        ),
        Action::ToggleTrim => pick(r.trim, t("Margini rifilati", "Margins trimmed"), t("Pagine intere", "Whole pages")),
        Action::ToggleBookmark => {
            pick(r.bookmarked(), t("Pagina segnata", "Page bookmarked"), t("Segno tolto", "Bookmark removed"))
        }
        Action::Zoom { .. }
        | Action::ZoomIn
        | Action::ZoomOut
        | Action::ZoomTo(_)
        | Action::ZoomReset
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
            NamedKey::PageUp
            | NamedKey::PageDown
            | NamedKey::Home
            | NamedKey::End
            | NamedKey::Escape
            | NamedKey::Backspace
            | NamedKey::Enter
            | NamedKey::Delete
            | NamedKey::Insert
            | NamedKey::Tab => format!("{n:?}"),
            // F1..F24: il nome e' gia' quello
            other => {
                let name = format!("{other:?}");
                let f_key =
                    name.strip_prefix('F').is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()));
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
        Key::Named(NamedKey::Tab) => ui::Key::Tab,
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
