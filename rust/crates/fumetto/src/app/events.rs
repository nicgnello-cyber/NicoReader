//! Gli eventi di winit: la finestra, i tasti, il mouse, i messaggi dai
//! thread di lavoro.

use super::*;

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
            self.open(&path);
        }
    }

    // su macOS Cmd+Q (o "Esci" dal Dock) chiude il programma da dentro AppKit,
    // senza tornare da run_app: il punto di lettura si salva qui
    fn exiting(&mut self, _: &ActiveEventLoop) {
        self.save_progress();
    }

    fn user_event(&mut self, _: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Loaded(loaded) => self.arrived(loaded),
            #[cfg(target_os = "macos")]
            UserEvent::Finder => self.open_from_finder(),
            UserEvent::Gpu(start) => match *start {
                Ok(start) => self.gpu_arrived(start),
                Err(e) => eprintln!("scheda video: {e}; si continua copiando dal processore"),
            },
            UserEvent::Chosen(path, to_library) => {
                self.dialog = false;
                match (path, to_library) {
                    (Some(path), true) => {
                        if !self.settings.library.contains(&path) {
                            self.settings.library.push(path);
                            self.save_settings();
                        }
                        self.rescan();
                        self.changed();
                    }
                    (Some(path), false) => self.open(&path),
                    (None, _) => {}
                }
            }
            UserEvent::DialogClosed => self.dialog = false,
            UserEvent::Confirmed(path) => {
                self.dialog = false;
                match system::trash(&path) {
                    Ok(()) => {
                        self.ui.toast(t("Spostato nel cestino", "Moved to the trash"), Instant::now());
                        self.rescan();
                    }
                    Err(e) => self.notify(format!(
                        "{}\n\n{e}",
                        t("Impossibile spostarlo nel cestino.", "Can't move it to the trash.")
                    )),
                }
            }
            UserEvent::Scanned(roots, entries) => {
                // una scansione di cartelle che nel frattempo sono cambiate non vale
                if roots == self.settings.library {
                    self.lib.entries = entries;
                    self.lib.scanning = false;
                    self.refresh_shelf();
                    self.changed();
                }
            }
            UserEvent::Cover(c) => match (&self.gfx, c.page) {
                (Some(gfx), Ok(page)) => {
                    if let Ok(image) = gfx.renderer.upload(&page) {
                        self.covers.insert(c.path, (image, c.size));
                        self.request_redraw();
                    }
                }
                // senza scheda video non si tengono: si richiedono quando c'e'
                (None, Ok(_)) => self.cover_wanted.clear(),
                (_, Err(e)) => {
                    eprintln!("copertina di {}: {e}", c.path.display());
                    self.covers_failed.insert(c.path);
                    self.request_redraw();
                }
            },
            UserEvent::Thumb(loaded) => {
                if loaded.generation != self.thumb_generation {
                    return;
                }
                match (&self.gfx, loaded.page) {
                    (Some(gfx), Ok(page)) => {
                        if let Ok(image) = gfx.renderer.upload(&page) {
                            self.thumb_images.insert(loaded.index, (image, loaded.target));
                            self.request_redraw();
                        }
                    }
                    // senza scheda video non si tengono: si richiedono quando c'e'
                    (None, Ok(_)) => self.thumb_wanted = (None, Vec::new()),
                    (_, Err(e)) => eprintln!("miniatura {}: {e}", loaded.index + 1),
                }
            }
            UserEvent::LensPage(loaded) => {
                if loaded.generation != self.lens_generation {
                    return;
                }
                match (&self.gfx, loaded.page) {
                    (Some(gfx), Ok(page)) => match gfx.renderer.upload(&page) {
                        Ok(image) => {
                            self.lens_images.insert(loaded.index, (image, loaded.target));
                            // ai loro pixel pesano: se ne tengono poche, le piu' vicine
                            let here = self.reader.as_ref().map_or(0, |r| r.here());
                            while self.lens_images.len() > 4 {
                                let far = *self.lens_images.keys().max_by_key(|i| i.abs_diff(here)).expect("non vuota");
                                self.lens_images.remove(&far);
                            }
                            self.request_redraw();
                        }
                        Err(e) => eprintln!("lente, pagina {}: {e}", loaded.index + 1),
                    },
                    (None, Ok(_)) => self.lens_wanted = (None, Vec::new()),
                    (_, Err(e)) => eprintln!("lente, pagina {}: {e}", loaded.index + 1),
                }
            }
            UserEvent::Upscaled(u) => {
                let current = self.reader.as_ref().map(|r| r.target());
                if u.generation != self.generation || !self.settings.upscale {
                    return;
                }
                match (u.page, &self.gfx) {
                    (Some(page), Some(gfx)) if current == Some(u.target) => {
                        if let Ok(image) = gfx.renderer.upload(&page) {
                            self.shown.insert(u.index, Shown { image, target: u.target, upscaled: true, stale: false });
                            self.request_redraw();
                        }
                    }
                    (None, _) => {
                        self.upscale_failed.insert((u.index, u.target));
                    }
                    _ => {}
                }
                self.request_upscale();
            }
            UserEvent::DownloadConfirmed(yes) => {
                self.dialog = false;
                if yes {
                    self.installing = true;
                    let dir = self.upscaler.dir();
                    let proxy = self.proxy.clone();
                    std::thread::Builder::new()
                        .name("scarica-ingranditore".into())
                        .spawn(move || {
                            let _ = proxy.send_event(UserEvent::Installed(upscale::install(&dir)));
                        })
                        .expect("thread dello scaricamento");
                    self.ui
                        .toast(t("Scarico l'ingranditore\u{2026}", "Downloading the enhancer\u{2026}"), Instant::now());
                    self.request_redraw();
                }
            }
            UserEvent::Installed(result) => {
                self.installing = false;
                match result {
                    Ok(()) => self.upscale_ready(),
                    Err(e) => self.notify(format!(
                        "{}\n\n{e}",
                        t("Non sono riuscito a scaricare l'ingranditore.", "Couldn't download the enhancer.")
                    )),
                }
            }
            UserEvent::SaveTo(path) => {
                self.dialog = false;
                if let (Some(path), Some((book, index))) = (path, self.to_save.take()) {
                    let proxy = self.proxy.clone();
                    std::thread::Builder::new()
                        .name("salva-pagina".into())
                        .spawn(move || {
                            let done = save_page(&book, index, &path).map(|_| t("Pagina salvata", "Page saved"));
                            let _ = proxy.send_event(UserEvent::Done(done));
                        })
                        .expect("thread del salvataggio");
                }
            }
            UserEvent::Done(result) => match result {
                Ok(text) => {
                    self.ui.toast(text, Instant::now());
                    self.request_redraw();
                }
                Err(e) => self.notify(e),
            },
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gfx) = &mut self.gfx {
                    gfx.config.width = size.width.max(1);
                    gfx.config.height = size.height.max(1);
                    gfx.surface.configure(&gfx.gpu.device, &gfx.config);
                }
                self.fit_view();
                self.changed();
            }
            WindowEvent::RedrawRequested => self.redraw(),
            #[cfg(windows)]
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(w) = &self.window {
                    set_icons(w);
                }
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                // le impostazioni aspettano un tasto nuovo: questo e' per loro
                if self.ui.capturing().is_some() {
                    let Some(combo) = combo_of(&event, self.modifiers) else { return }; // un modificatore da solo
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
                    return;
                }
                // prima l'interfaccia: con un menu aperto o nella galleria
                // vuota le frecce e Invio sono sue
                let command =
                    if cfg!(target_os = "macos") { self.modifiers.super_key() } else { self.modifiers.control_key() };
                if !command && (self.ui.modal() || self.reader.is_none() || self.shelf_shown() || self.thumbs_shown()) {
                    let ctx = input_context!(self);
                    let handled = ui_key(&event.logical_key).map_or(Handled::No, |k| self.ui.key(k, &ctx));
                    match handled {
                        Handled::Yes(cmd) => {
                            self.request_redraw();
                            if let Some(cmd) = cmd {
                                self.run(cmd, event_loop);
                            }
                            return;
                        }
                        // con un menu aperto, nella libreria o fra le miniature, nessun
                        // altro tasto arriva al lettore
                        Handled::No if self.ui.modal() || self.shelf_shown() || self.thumbs_shown() => return,
                        Handled::No => {}
                    }
                }
                if event.logical_key == Key::Named(NamedKey::ContextMenu) {
                    let (w, h) = self.view_size();
                    let ctx = input_context!(self);
                    self.ui.open_menu(w as f32 / 2.0, h as f32 / 3.0, &ctx);
                    return self.request_redraw();
                }
                if let Some(a) = self.key(&event) {
                    self.act(a, event_loop);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
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
                    let zoom = (self.settings.lens_zoom * 1.12f32.powf(lines)).clamp(1.5, 6.0);
                    self.settings.lens_zoom = (zoom * 4.0).round() / 4.0;
                    self.settings_dirty = true;
                    let text = format!("{} {}\u{00d7}", t("Lente", "Magnifier"), self.settings.lens_zoom);
                    let text = if fumetto_core::lingua::italian() { text.replace('.', ",") } else { text };
                    self.ui.toast(text, Instant::now());
                    return self.request_redraw();
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
            WindowEvent::CursorMoved { position, .. } => {
                let (x, y) = (position.x as f32, position.y as f32);
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
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => match state {
                ElementState::Pressed => {
                    self.press = Some(Press { x: self.cursor.0, y: self.cursor.1, dragged: false });
                    let ctx = input_context!(self);
                    if let Some(cmd) = self.ui.press(self.cursor.0, self.cursor.1, &ctx) {
                        self.run(cmd, event_loop);
                    }
                }
                ElementState::Released => {
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
            },
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Right, .. } => {
                let ctx = input_context!(self);
                self.ui.open_menu(self.cursor.0, self.cursor.1, &ctx);
                self.request_redraw();
            }
            WindowEvent::DroppedFile(path) => self.open(&path),
            WindowEvent::Focused(on) => {
                self.stats.note(if on { "finestra in primo piano" } else { "finestra sullo sfondo" })
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if self.progress.is_dirty() && now - self.last_save >= SAVE_EVERY {
            self.save_progress();
        }
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
