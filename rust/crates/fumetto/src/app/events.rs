//! Gli eventi di winit: la finestra, i tasti, il mouse, i messaggi dai
//! thread di lavoro.

use super::*;
use winit::event::KeyEvent;

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes().with_title("NicoReader").with_maximized(true).with_visible(false);
        // su Linux il nome con cui il desktop trova nicoreader.desktop, e con lui
        // icona e nome: su Wayland senza non c'e' (su X11 sarebbe il nome
        // dell'eseguibile, qui uguale)
        #[cfg(all(unix, not(target_os = "macos")))]
        let attrs = winit::platform::wayland::WindowAttributesExtWayland::with_name(attrs, "nicoreader", "nicoreader");
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                crate::fatal(&format!("{}\n\n{e}", t("Impossibile creare la finestra.", "Can't create the window.")))
            }
        };
        #[cfg(windows)]
        set_icons(&window);
        if let Some(mhz) = window.current_monitor().and_then(|m| m.refresh_rate_millihertz()) {
            self.stats.refresh_ms = 1_000_000.0 / mhz as f32;
        }
        if self.script.is_some() {
            // lanciata da un altro programma, Windows la aprirebbe dietro la
            // finestra attiva, e una finestra coperta viene disegnata al rallentatore
            window.set_window_level(winit::window::WindowLevel::AlwaysOnTop);
            window.focus_window();
        }
        self.window = Some(window.clone());
        self.stats.window_ms = Some(self.stats.now_ms());
        match CpuView::new(window) {
            Ok(cpu) => self.cpu = Some(cpu),
            Err(e) => eprintln!("copia dal processore non disponibile: {e}"),
        }
        self.update_title();
        // solo adesso la scheda video: caricare i suoi driver blocca il
        // caricamento di ogni altra libreria (loader lock di Windows), e partendo
        // per prima rallentava l'apertura della finestra. La prima pagina non la
        // aspetta: la mostra il processore.
        gpu_start::spawn(self.proxy.clone());
        // una finestra invisibile non riceve mai la richiesta di ridisegno: il
        // primo fotogramma (nero) si disegna subito, la finestra appare con
        // quello, prende la sua misura vera, e parte la lettura anticipata
        self.redraw();
        #[cfg(target_os = "macos")]
        self.open_from_finder();
        if let Some(path) = self.resume_later.take() {
            self.resume(&path);
        }
    }

    // su macOS Cmd+Q (o "Esci" dal Dock) chiude il programma da dentro AppKit,
    // senza tornare da run_app: il punto di lettura si salva qui
    fn exiting(&mut self, _: &ActiveEventLoop) {
        self.save_progress();
        remote::sync::flush();
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Loaded(loaded) => self.arrived(loaded),
            #[cfg(target_os = "macos")]
            UserEvent::Finder => self.open_from_finder(),
            UserEvent::Gpu(start) => match *start {
                Ok(start) => self.gpu_arrived(start),
                Err(e) => eprintln!("scheda video: {e}; si continua copiando dal processore"),
            },
            UserEvent::Chosen(path, to_library) => self.chosen(path, to_library),
            UserEvent::DialogClosed => self.dialog = false,
            UserEvent::Confirmed(path) => self.trash_confirmed(&path),
            UserEvent::Scanned(scan) => self.scanned(*scan),
            UserEvent::ServerChecked(server, found) => self.server_checked(server, found),
            UserEvent::Opened(path, book, quiet) => self.opened(&path, *book, quiet),
            UserEvent::Downloaded(path, result) => self.downloaded(&path, result),
            UserEvent::Cover(c) => self.cover_arrived(c),
            UserEvent::Thumb(loaded) => self.thumb_arrived(loaded),
            UserEvent::LensPage(loaded) => self.lens_page_arrived(loaded),
            UserEvent::Upscaled(u) => self.upscaled(u),
            UserEvent::Update(release) => self.update_arrived(release),
            UserEvent::OpenUpdate(url) => self.open_release_page(&url),
            UserEvent::InstallUpdate(release) => self.install_update(release),
            UserEvent::UpdateDownloaded(plan, release, file) => {
                self.update_downloaded(event_loop, *plan, release, file)
            }
            UserEvent::SaveTo(path) => self.save_to(path),
            UserEvent::Searched(query, found, failed) => self.searched(query, found, failed),
            UserEvent::Forwarded(path) => self.forwarded(path),
            UserEvent::Done(result) => self.done(result),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if !self.shelf_shown() {
            return self.handle_window_event(event_loop, event);
        }
        // un errore nel codice della libreria non chiude il programma: si
        // annota (errori.txt) e la libreria torna all'inizio
        let handled =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.handle_window_event(event_loop, event)));
        if handled.is_err() {
            self.shelf_failed();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if self.progress.is_dirty() && now - self.last_save >= SAVE_EVERY {
            self.save_progress();
        }
        self.keep_library_view();
        if self.settings_dirty && now - self.settings_saved_at >= SAVE_EVERY {
            self.settings_dirty = false;
            self.settings_saved_at = now;
            self.save_settings();
        }
        self.show_pending();
        let mut wake: Option<Instant> = None;
        if let Some(at) = self.retry_at {
            if now >= at {
                self.retry_at = None;
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            } else {
                wake = Some(at);
            }
        }
        // c'e' qualcosa da salvare: ci si risveglia per farlo anche se non
        // succede nient'altro
        if self.progress.is_dirty() {
            let at = self.last_save + SAVE_EVERY;
            wake = Some(wake.map_or(at, |w| w.min(at)));
        }
        if let Some(at) = self.search_tick(now) {
            wake = Some(wake.map_or(at, |w| w.min(at)));
        }
        if let Some(at) = self.slideshow_tick(now, event_loop) {
            wake = Some(wake.map_or(at, |w| w.min(at)));
        }
        // la didascalia che compare o svanisce vuole i suoi fotogrammi
        if let Some(at) = self.ui.wake(now) {
            if at <= now {
                self.request_redraw();
            } else {
                wake = Some(wake.map_or(at, |w| w.min(at)));
            }
        }
        if let Some(mut script) = self.script.take() {
            match script.step(self) {
                Step::Do(a) => self.act(a, event_loop),
                Step::Wait => {}
                Step::Done => return self.act(Action::Quit, event_loop),
            }
            self.script = Some(script);
            let next = now + Duration::from_millis(4);
            wake = Some(wake.map_or(next, |w| w.min(next)));
        }
        event_loop.set_control_flow(wake.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }
}

impl App {
    fn handle_window_event(&mut self, event_loop: &ActiveEventLoop, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => self.resized(size.width, size.height),
            WindowEvent::RedrawRequested => self.redraw(),
            #[cfg(windows)]
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(w) = &self.window {
                    set_icons(w);
                }
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                self.key_pressed(&event, event_loop)
            }
            WindowEvent::MouseWheel { delta, .. } => self.wheel(delta, event_loop),
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer_moved(position.x as f32, position.y as f32, event_loop)
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => match state {
                ElementState::Pressed => self.left_down(event_loop),
                ElementState::Released => self.left_up(event_loop),
            },
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Right, .. } => {
                self.context_menu();
            }
            WindowEvent::Touch(touch) => self.touch(touch, event_loop),
            WindowEvent::PinchGesture { delta, .. } => self.pinch_gesture(delta as f32, event_loop),
            WindowEvent::DoubleTapGesture { .. } => self.double_tap(self.cursor.0, self.cursor.1, event_loop),
            WindowEvent::DroppedFile(path) => self.open(&path),
            WindowEvent::Focused(on) => {
                self.stats.note(if on { "finestra in primo piano" } else { "finestra sullo sfondo" })
            }
            _ => {}
        }
    }

    fn resized(&mut self, width: u32, height: u32) {
        if let Some(gfx) = &mut self.gfx {
            gfx.config.width = width.max(1);
            gfx.config.height = height.max(1);
            gfx.surface.configure(&gfx.gpu.device, &gfx.config);
        }
        self.fit_view();
        self.changed();
    }

    /// Un tasto premuto: alle impostazioni che ne aspettano uno nuovo, al
    /// modulo (Ctrl+V incolla), all'interfaccia, al menu (il tasto del menu),
    /// poi al lettore.
    fn key_pressed(&mut self, event: &KeyEvent, event_loop: &ActiveEventLoop) {
        if self.ui.capturing().is_some() {
            return self.capture_key(event, event_loop);
        }
        let command = if cfg!(target_os = "macos") { self.modifiers.super_key() } else { self.modifiers.control_key() };
        // il modulo del server: Ctrl+V incolla, e Ctrl+Alt (AltGr su
        // Windows, per la @ della tastiera italiana) scrive
        if self.ui.server_open() && command && !self.modifiers.alt_key() {
            if event.key_without_modifiers() == Key::Character("v".into())
                && let Ok(text) = arboard::Clipboard::new().and_then(|mut c| c.get_text())
            {
                self.ui.paste(&text);
                self.request_redraw();
            }
            return;
        }
        let command = command && !self.ui.server_open();
        if !command && self.ui_key_first(event, event_loop) {
            return;
        }
        if event.logical_key == Key::Named(NamedKey::ContextMenu) {
            let (w, h) = self.view_size();
            let ctx = input_context!(self);
            self.ui.open_menu(w as f32 / 2.0, h as f32 / 3.0, &ctx);
            return self.request_redraw();
        }
        if let Some(a) = self.key(event) {
            self.act(a, event_loop);
        }
    }

    /// Le impostazioni aspettano un tasto nuovo: questo e' per loro (Esc
    /// lascia com'era, Canc lo toglie). Un modificatore da solo non conta.
    fn capture_key(&mut self, event: &KeyEvent, event_loop: &ActiveEventLoop) {
        let Some(combo) = combo_of(event, self.modifiers) else { return };
        let plain = !combo.ctrl && !combo.alt && !combo.shift;
        let cmd = match combo.key.as_str() {
            "Escape" if plain => self.ui.captured(None, false),
            "Backspace" | "Delete" if plain => self.ui.captured(None, true),
            _ => self.ui.captured(Some(combo), false),
        };
        self.request_redraw();
        if let Some(cmd) = cmd {
            self.run(cmd, event_loop);
        }
    }

    /// Prima l'interfaccia: con un menu aperto, nella galleria vuota, nella
    /// libreria e fra le miniature le frecce e Invio sono sue. `true` se il
    /// tasto non va oltre.
    fn ui_key_first(&mut self, event: &KeyEvent, event_loop: &ActiveEventLoop) -> bool {
        let covering = self.ui.modal() || self.shelf_shown() || self.thumbs_shown();
        if !covering && self.reader.is_some() {
            return false;
        }
        let ctx = input_context!(self);
        match ui_key(&event.logical_key).map_or(Handled::No, |k| self.ui.key(k, &ctx)) {
            Handled::Yes(cmd) => {
                self.request_redraw();
                if let Some(cmd) = cmd {
                    self.run(cmd, event_loop);
                }
                true
            }
            // con un menu aperto, nella libreria o fra le miniature, nessun
            // altro tasto arriva al lettore
            Handled::No => covering,
        }
    }

    /// La rotella: all'interfaccia (libreria, miniature, impostazioni), alla
    /// finestra aperta, alla lente; poi zoom (con Ctrl), spostamento della
    /// pagina ingrandita, o pagine.
    fn wheel(&mut self, delta: MouseScrollDelta, event_loop: &ActiveEventLoop) {
        let (lines, pixels) = match delta {
            MouseScrollDelta::LineDelta(_, y) => (y, -y * self.view_size().1 as f32 * 0.15),
            MouseScrollDelta::PixelDelta(p) => (p.y as f32 / 120.0, -p.y as f32),
        };
        let ctx = input_context!(self);
        if self.ui.wheel(pixels, &ctx) {
            return self.request_redraw();
        }
        if self.ui.modal() {
            // "vai a pagina": la rotella sfoglia il numero; nel menu, le voci
            self.ui.key(if lines > 0.0 { ui::Key::Up } else { ui::Key::Down }, &ctx);
            return self.request_redraw();
        }
        if self.lens_circle().is_some() && !self.modifiers.control_key() {
            return self.lens_wheel(lines);
        }
        let zoomed = self.reader.as_ref().is_some_and(|r| r.zoomed());
        let action = if self.modifiers.control_key() {
            Action::Zoom { factor: 1.2f32.powf(lines), x: self.cursor.0, y: self.cursor.1 - self.top() as f32 }
        } else if zoomed {
            Action::Pan(0.0, -pixels)
        } else {
            Action::Scroll(pixels)
        };
        self.act(action, event_loop);
    }

    /// Con la lente, la rotella ne cambia l'ingrandimento (a quarti, da 1,5 a 6).
    fn lens_wheel(&mut self, lines: f32) {
        let zoom = (self.settings.lens_zoom * 1.12f32.powf(lines)).clamp(1.5, 6.0);
        self.settings.lens_zoom = (zoom * 4.0).round() / 4.0;
        self.settings_dirty = true;
        let text = format!("{} {}\u{00d7}", t("Lente", "Magnifier"), self.settings.lens_zoom);
        let text = if fumetto_core::lingua::italian() { text.replace('.', ",") } else { text };
        self.ui.toast(text, Instant::now());
        self.request_redraw();
    }

    /// Il puntatore (mouse, o un dito) si e' mosso.
    pub(super) fn pointer_moved(&mut self, x: f32, y: f32, event_loop: &ActiveEventLoop) {
        let (dx, dy) = (x - self.cursor.0, y - self.cursor.1);
        self.cursor = (x, y);
        let ctx = input_context!(self);
        if self.ui.motion(x, y, &ctx, Instant::now()) || self.lens_circle().is_some() {
            self.request_redraw();
        }
        if self.press.is_some() {
            let ctx = input_context!(self);
            if let Some(cmd) = self.ui.drag(x, &ctx) {
                if let Some(p) = &mut self.press {
                    p.dragged = true;
                }
                return self.run(cmd, event_loop);
            }
        }
        if let Some(press) = &mut self.press {
            // oltre qualche pixel non e' piu' un clic: si trascina la pagina
            press.dragged |= (x - press.x).abs() + (y - press.y).abs() > 6.0;
            if press.dragged && !self.ui.modal() && self.reader.as_ref().is_some_and(|r| r.zoomed()) {
                self.act(Action::Pan(dx, dy), event_loop);
            }
        }
    }

    /// Il tasto sinistro (o un dito) giu'.
    pub(super) fn left_down(&mut self, event_loop: &ActiveEventLoop) {
        self.press = Some(Press { x: self.cursor.0, y: self.cursor.1, dragged: false });
        let ctx = input_context!(self);
        if let Some(cmd) = self.ui.press(self.cursor.0, self.cursor.1, &ctx) {
            self.run(cmd, event_loop);
        }
    }

    /// Il tasto sinistro (o un dito) su: se non si e' trascinato, un clic.
    pub(super) fn left_up(&mut self, event_loop: &ActiveEventLoop) {
        let press = self.press.take();
        if press.as_ref().is_some_and(|p| p.dragged) && self.ui.prefs.is_some() {
            // fine del trascinamento di una regolazione
            let ctx = input_context!(self);
            let _ = self.ui.click(self.cursor.0, self.cursor.1, &ctx);
        }
        if let Some(press) = press
            && !press.dragged
        {
            let ctx = input_context!(self);
            match self.ui.click(press.x, press.y, &ctx) {
                Handled::Yes(cmd) => {
                    self.request_redraw();
                    if let Some(cmd) = cmd {
                        self.run(cmd, event_loop);
                    }
                }
                Handled::No => {
                    let action = self.click(press.x).or_else(|| self.double_click(press.x, press.y));
                    if let Some(a) = action {
                        self.act(a, event_loop);
                    }
                }
            }
        }
    }

    /// Il menu del tasto destro (o del dito tenuto giu'), dove sta il puntatore.
    pub(super) fn context_menu(&mut self) {
        let ctx = input_context!(self);
        self.ui.open_menu(self.cursor.0, self.cursor.1, &ctx);
        self.request_redraw();
    }
}

/// L'icona dell'eseguibile anche sulla finestra: piccola nella barra del
/// titolo, grande per Alt+Tab e la barra delle applicazioni, ciascuna alla
/// misura della scala dello schermo (Windows la sceglie tra quelle nel file).
#[cfg(windows)]
fn set_icons(window: &Window) {
    use winit::dpi::PhysicalSize;
    use winit::platform::windows::{IconExtWindows, WindowExtWindows};
    use winit::window::Icon;
    let icon = |side: f64| {
        let side = (side * window.scale_factor()).round() as u32;
        Icon::from_resource(1, Some(PhysicalSize::new(side, side))).ok()
    };
    window.set_window_icon(icon(16.0));
    window.set_taskbar_icon(icon(32.0));
}
