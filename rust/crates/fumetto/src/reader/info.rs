//! Cio' che il lettore dice di se': il titolo, i modi, le pagine segnate,
//! lo stato da ricordare e da riprendere.

use super::*;

impl Reader {
    /// Il titolo della finestra, l'unica cosa scritta: volume, pagina, modi.
    pub fn title(&self, name: &str) -> String {
        let mut title = format!("{name} \u{2014} {} / {}", self.folio(), self.len);
        let mut extra = self.modes();
        if self.mode == Mode::Single {
            extra.remove(0); // la pagina singola e' il modo di sempre: non si scrive
        }
        if self.broken_here() {
            extra.push(t("pagina illeggibile", "unreadable page"));
        }
        for what in extra {
            title += " \u{00b7} ";
            title += what;
        }
        title
    }

    pub fn pages(&self) -> usize {
        self.len
    }

    /// Le pagine a schermo, per chi legge: "24" o "24–25".
    pub fn folio(&self) -> String {
        let (s, n) = self.group_of(self.here());
        if n == 2 { format!("{}\u{2013}{}", s + 1, s + 2) } else { format!("{}", self.here() + 1) }
    }

    /// Come si sta leggendo: prima il modo, poi cio' che vi si aggiunge.
    pub fn modes(&self) -> Vec<&'static str> {
        let mut v = vec![match self.mode {
            Mode::Single => t("pagina singola", "single page"),
            Mode::Double => t("doppia pagina", "two pages"),
            Mode::Strip => t("nastro", "strip"),
        }];
        for (on, what) in [
            (self.mode == Mode::Double && self.cover_alone, t("copertina sola", "cover alone")),
            (self.manga, "manga"),
            (self.zoom > 1.0, "zoom"),
            (self.trim && self.mode != Mode::Strip, t("rifilata", "trimmed")),
            (self.rotation != 0, t("girata", "rotated")),
            (!self.linear, "gamma"),
        ] {
            if on {
                v.push(what);
            }
        }
        v
    }

    /// Fra le pagine a schermo ce n'e' una che non si riesce a leggere.
    pub fn broken_here(&self) -> bool {
        self.wanted().iter().any(|&i| self.broken[i])
    }

    pub fn cover_alone(&self) -> bool {
        self.cover_alone
    }

    pub fn bookmarks(&self) -> &[usize] {
        &self.bookmarks
    }

    /// La pagina a schermo e' segnata (nella doppia pagina, una delle due;
    /// nel nastro, quella in cima).
    pub fn bookmarked(&self) -> bool {
        let here = match self.mode {
            Mode::Strip => vec![self.strip.anchor],
            _ => self.wanted(),
        };
        here.iter().any(|p| self.bookmarks.binary_search(p).is_ok())
    }

    /// Le pagine di cui si vede almeno un pezzo.
    pub fn on_screen(&self) -> Vec<usize> {
        self.wanted()
    }

    /// La misura vera di una pagina (rifilata e girata), se la si conosce.
    pub fn native(&self, i: usize) -> Option<(u32, u32)> {
        self.native.get(i).copied().flatten()
    }

    /// Lo stato da ricordare per la prossima volta.
    pub fn snapshot(&self) -> Saved {
        let strip = self.strip_now();
        let anchor_h = self.strip_height(strip.anchor).max(1.0);
        Saved {
            page: self.here(),
            pages: self.len,
            mode: match self.mode {
                Mode::Single => "pagina",
                Mode::Double => "doppia",
                Mode::Strip => "nastro",
            }
            .into(),
            manga: self.manga,
            cover_alone: self.cover_alone,
            strip_width: self.strip_frac.unwrap_or(0.0),
            strip_offset: (strip.offset / anchor_h).clamp(0.0, 1.0),
            read_at: 0,
            rotation: self.rotation,
            bookmarks: self.bookmarks.clone(),
        }
    }

    /// Riprende da dove si era rimasti. Se il volume nel frattempo ha perso
    /// pagine, si resta dentro i limiti.
    pub fn restore(&mut self, saved: &Saved) {
        if self.len == 0 {
            return;
        }
        self.page = saved.page.min(self.len - 1);
        self.mode = match saved.mode.as_str() {
            "doppia" => Mode::Double,
            "nastro" => Mode::Strip,
            _ => Mode::Single,
        };
        self.manga = saved.manga;
        self.cover_alone = saved.cover_alone;
        if saved.strip_width > 0.0 {
            self.strip_frac = Some(saved.strip_width.clamp(0.2, 1.0));
        }
        self.rotation = saved.rotation % 360 / 90 * 90;
        self.bookmarks = saved.bookmarks.iter().copied().filter(|&p| p < self.len).collect();
        self.bookmarks.sort_unstable();
        self.bookmarks.dedup();
        self.go_to(self.group_of(self.page).0);
        // l'altezza vera della pagina arrivera' dopo: fino ad allora la
        // posizione resta una frazione della pagina (vedi `anchor_frac`)
        self.anchor_frac = Some(saved.strip_offset.clamp(0.0, 1.0));
        self.jumped = true;
    }

    /// Per la prova automatica, quando qualcosa si incastra.
    pub fn describe(&self, pages: &impl Pages) -> String {
        format!(
            "modo {:?}, pagina {}, nastro {:?}, scorrimento da fare {:.1} px, pronta {}",
            self.mode,
            self.page + 1,
            self.strip_now(),
            self.glide,
            self.ready(pages)
        )
    }
}
