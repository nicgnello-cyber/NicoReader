//! Rotella, mouse, clic e tasti: a chi vanno (il modulo, la pagina da
//! scegliere, il menu, le impostazioni, le miniature, la libreria, la
//! galleria) e cosa ne viene.

use super::*;

impl Ui {
    /// La rotella: nella libreria scorre; `false` se non e' affar suo.
    pub fn wheel(&mut self, pixels: f32, ctx: &Context) -> bool {
        if self.menu.is_some() {
            return false;
        }
        if let Some(p) = &mut self.prefs {
            if Prefs::covers(ctx.view, ctx.scale, self.pointer.0) {
                p.wheel(ctx.settings, ctx.keys, ctx.view, ctx.scale, pixels);
            }
            return true;
        }
        if ctx.thumbs.is_some() {
            self.thumbs.wheel(pixels);
            return true;
        }
        if ctx.shelf.is_some() {
            self.shelf.wheel(pixels);
            return true;
        }
        false
    }

    /// Il mouse si e' mosso: `true` se va ridisegnato.
    pub fn motion(&mut self, x: f32, y: f32, ctx: &Context, now: Instant) -> bool {
        self.pointer = (x, y);
        let hover = if self.modal() { None } else { hud_hit(ctx, x, y).filter(|b| b.cmd.is_some()).map(|b| b.id()) };
        let hover_changed = hover != self.hover;
        self.hover = hover;
        if let Some(m) = &mut self.menu
            && let Some(i) = m.hit(ctx, x, y)
            && m.rows[i].command().is_some()
            && m.selected != Some(i)
        {
            m.selected = Some(i);
            return true;
        }
        if self.menu.is_none()
            && self.goto.is_none()
            && let Some(p) = &mut self.prefs
        {
            return p.motion(ctx.settings, ctx.keys, ctx.view, ctx.scale, x, y);
        }
        if self.modal() {
            return false;
        }
        if let Some(d) = &ctx.thumbs {
            return self.thumbs.motion(d, ctx.view, ctx.scale, x, y);
        }
        if let Some(d) = &ctx.shelf {
            return self.shelf.motion(d, ctx.view, ctx.scale, x, y);
        }
        if ctx.book.is_some() {
            if ctx.bar() {
                return hover_changed; // titolo e pagina sono gia' nella barra
            }
            let was = self.caption.as_ref().is_some_and(|s| s.alpha(now) > 0.99);
            self.poke(now);
            return !was || hover_changed;
        }
        let hover = welcome_hit(ctx, x, y);
        let changed = hover != self.selected;
        if hover.is_some() {
            self.selected = hover;
        }
        changed
    }

    pub fn click(&mut self, x: f32, y: f32, ctx: &Context) -> Handled {
        if let Some(f) = &mut self.server {
            // fuori dai campi non si chiude: si perderebbe cio' che si e' scritto
            f.click(ctx, x, y);
            return Handled::Yes(None);
        }
        if let Some(m) = &self.menu {
            let hit = m.hit(ctx, x, y);
            let cmd = hit.and_then(|i| m.rows[i].command().cloned());
            // un clic sul menu che non sceglie niente (un titolo, una riga) lo lascia aperto
            if cmd.is_some() || hit.is_none() {
                self.menu = None;
            }
            return Handled::Yes(self.internal(cmd, ctx));
        }
        if self.goto.is_some() {
            self.click_goto(x, y, ctx);
            return Handled::Yes(None);
        }
        if let Some(p) = &mut self.prefs {
            if Prefs::covers(ctx.view, ctx.scale, x) {
                return p.click(ctx.settings, ctx.keys, ctx.view, ctx.scale, x, y);
            }
            // un clic sulle pagine chiude il foglio, come fuori da un menu
            return Handled::Yes(Some(Command::Act(Action::ToggleSettings)));
        }
        if let Some(d) = &ctx.thumbs {
            return self.thumbs.click(d, ctx.view, ctx.scale, x, y);
        }
        if ctx.bar() && y < hud_height(ctx.scale) {
            return Handled::Yes(self.click_bar(x, y, ctx));
        }
        if let Some(d) = &ctx.shelf {
            let handled = self.shelf.click(d, ctx.view, ctx.scale, x, y);
            return self.after_shelf(handled, ctx);
        }
        if ctx.book.is_none() {
            if welcome_cta_hit(ctx, y) {
                return Handled::Yes(Some(Command::Act(Action::AddLibraryFolder)));
            }
            return Handled::Yes(Some(match welcome_hit(ctx, x, y) {
                Some(i) => Command::Open(ctx.recent[i].path.clone()),
                None => Command::Act(Action::Open),
            }));
        }
        Handled::No
    }

    /// Un clic con la pagina da scegliere aperta: sul righello sceglie la
    /// pagina, altrove chiude.
    fn click_goto(&mut self, x: f32, y: f32, ctx: &Context) {
        let Some(g) = &mut self.goto else { return };
        let (x0, w, y0) = ruler(ctx);
        if (y - y0).abs() < 24.0 * ctx.scale && x >= x0 - 8.0 && x <= x0 + w + 8.0 {
            let f = ((x - x0) / w).clamp(0.0, 1.0);
            g.typed = (1 + (f * (g.pages - 1) as f32).round() as usize).to_string();
        } else {
            self.goto = None;
        }
    }

    /// Un clic sulla barra in alto: il comando del tasto; la percentuale
    /// apre i livelli di zoom, appena sotto.
    fn click_bar(&mut self, x: f32, y: f32, ctx: &Context) -> Option<Command> {
        let b = hud_hit(ctx, x, y)?;
        if b.id() == Hover::Zoom && b.cmd.is_some() {
            self.menu = Some(Menu::zoom(b.x, b.w, ctx));
            return None;
        }
        b.cmd
    }

    /// Cio' che la libreria chiede: alcuni comandi li fa l'interfaccia.
    fn after_shelf(&mut self, handled: Handled, ctx: &Context) -> Handled {
        match handled {
            Handled::Yes(cmd) => Handled::Yes(self.internal(cmd, ctx)),
            Handled::No => Handled::No,
        }
    }

    pub fn key(&mut self, key: Key, ctx: &Context) -> Handled {
        if let Some(f) = &mut self.server {
            return Handled::Yes(match f.key(key) {
                Outcome::Stay => None,
                Outcome::Close => {
                    self.server = None;
                    None
                }
                Outcome::Submit(cmd) => Some(cmd),
            });
        }
        // Tab serve solo al modulo: altrove resta ai tasti scelti da chi legge
        if key == Key::Tab {
            return Handled::No;
        }
        if self.goto.is_some() {
            return Handled::Yes(self.key_goto(key));
        }
        if self.menu.is_some() {
            return Handled::Yes(self.key_menu(key, ctx));
        }
        if let Some(p) = &mut self.prefs {
            return p.key(key, ctx.settings, ctx.keys, ctx.view, ctx.scale);
        }
        if let Some(d) = &ctx.thumbs {
            return self.thumbs.key(key, d, ctx.view, ctx.scale);
        }
        if let Some(d) = &ctx.shelf {
            let handled = self.shelf.key(key, d, ctx.view, ctx.scale);
            return self.after_shelf(handled, ctx);
        }
        if ctx.book.is_none() && !ctx.recent.is_empty() {
            return self.key_welcome(key, ctx);
        }
        Handled::No
    }

    /// Un tasto con la pagina da scegliere aperta: cifre, frecce, Invio.
    fn key_goto(&mut self, key: Key) -> Option<Command> {
        let g = self.goto.as_mut()?;
        match key {
            Key::Digit(d) => {
                if g.typed.len() < g.pages.to_string().len() && !(g.typed.is_empty() && d == 0) {
                    g.typed.push((b'0' + d) as char);
                }
            }
            Key::Backspace => {
                g.typed.pop();
            }
            Key::Up => g.step(1),
            Key::Down => g.step(-1),
            Key::PageUp => g.step(10),
            Key::PageDown => g.step(-10),
            Key::Home => g.typed = "1".into(),
            Key::End => g.typed = g.pages.to_string(),
            Key::Escape => self.goto = None,
            Key::Enter if g.valid() => {
                let page = g.page() - 1;
                self.goto = None;
                return Some(Command::Act(Action::GoTo(page)));
            }
            Key::Enter | Key::Left | Key::Right | Key::Tab | Key::Char(_) => {}
        }
        None
    }

    /// Un tasto con un menu aperto: le frecce scelgono, Invio esegue.
    fn key_menu(&mut self, key: Key, ctx: &Context) -> Option<Command> {
        let m = self.menu.as_mut()?;
        match key {
            Key::Down | Key::Up => m.step(key == Key::Down),
            Key::Enter => {
                let cmd = m.chosen();
                self.menu = None;
                return self.internal(cmd, ctx);
            }
            Key::Escape => self.menu = None,
            _ => {}
        }
        None
    }

    /// Nella galleria vuota: le frecce scelgono fra gli ultimi letti, Invio
    /// lo apre.
    fn key_welcome(&mut self, key: Key, ctx: &Context) -> Handled {
        let n = ctx.recent.len();
        match key {
            Key::Down => self.selected = Some(self.selected.map_or(0, |i| (i + 1) % n)),
            Key::Up => self.selected = Some(self.selected.map_or(n - 1, |i| (i + n - 1) % n)),
            Key::Enter => return Handled::Yes(self.selected.map(|i| Command::Open(ctx.recent[i].path.clone()))),
            _ => return Handled::No,
        }
        Handled::Yes(None)
    }
}
