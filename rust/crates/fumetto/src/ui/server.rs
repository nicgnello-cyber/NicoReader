//! Aggiungere un server alla libreria (Komga, Kavita): indirizzo, nome e
//! password, scritti o incollati. Invio prova il collegamento; il server
//! entra nella libreria solo se risponde.

use super::*;
use fumetto_core::remote::Server;

/// Quanto puo' essere lungo un campo: un indirizzo con la chiave di Kavita
/// sta largo dentro.
const MAX_FIELD: usize = 2000;

pub(crate) struct ServerForm {
    /// Indirizzo, nome, password.
    fields: [String; 3],
    focus: usize,
    /// Si sta provando il collegamento: si aspetta.
    busy: bool,
    /// Cosa e' andato storto l'ultima volta.
    error: Option<String>,
}

/// Cosa fa un tasto nel modulo.
pub(super) enum Outcome {
    Stay,
    Close,
    Submit(Server),
}

impl ServerForm {
    pub(super) fn new() -> ServerForm {
        ServerForm { fields: Default::default(), focus: 0, busy: false, error: None }
    }

    pub(super) fn key(&mut self, key: Key) -> Outcome {
        if key == Key::Escape {
            return Outcome::Close;
        }
        if self.busy {
            return Outcome::Stay;
        }
        match key {
            Key::Char(c) => self.type_text(&c.to_string()),
            Key::Digit(d) => self.type_text(&d.to_string()),
            Key::Backspace => {
                self.fields[self.focus].pop();
            }
            Key::Tab | Key::Down => self.focus = (self.focus + 1) % 3,
            Key::Up => self.focus = (self.focus + 2) % 3,
            Key::Enter => return self.submit(),
            _ => {}
        }
        Outcome::Stay
    }

    /// Testo scritto o incollato, nel campo scelto (senza a capo).
    pub(super) fn type_text(&mut self, text: &str) {
        if self.busy {
            return;
        }
        let field = &mut self.fields[self.focus];
        for c in text.chars().filter(|c| !c.is_control()) {
            if field.len() < MAX_FIELD {
                field.push(c);
            }
        }
        self.error = None;
    }

    fn submit(&mut self) -> Outcome {
        let url = self.fields[0].trim();
        if url.is_empty() {
            self.error = Some(t("Manca l'indirizzo del server.", "The server address is missing.").into());
            self.focus = 0;
            return Outcome::Stay;
        }
        // "casa:25600" vale "http://casa:25600"
        let url = if url.contains("://") { url.to_owned() } else { format!("http://{url}") };
        self.busy = true;
        self.error = None;
        Outcome::Submit(Server { url, user: self.fields[1].trim().to_owned(), password: self.fields[2].clone() })
    }

    /// La prova del collegamento e' andata male: si dice perche', e si puo'
    /// correggere.
    pub(super) fn failed(&mut self, error: String) {
        self.busy = false;
        self.error = Some(error);
    }

    /// Un clic: sceglie il campo sotto il mouse.
    pub(super) fn click(&mut self, ctx: &Context, x: f32, y: f32) {
        let (x0, w, ys) = geo(ctx);
        let s = ctx.scale;
        if let Some(i) = ys.iter().position(|&fy| y > fy - 44.0 * s && y < fy + 12.0 * s)
            && x >= x0
            && x <= x0 + w
            && !self.busy
        {
            self.focus = i;
        }
    }

    pub(super) fn layer(&self, ctx: &Context, m: &mut dyn Measure) -> Layer {
        let s = ctx.scale;
        let (vw, vh) = ctx.view;
        let (x0, w, ys) = geo(ctx);
        let mut l = Layer::default();
        l.rects.push(Rect::new(0.0, 0.0, vw, vh, [0.0, 0.0, 0.0, 0.96]));
        let top = ys[0] - 178.0 * s;
        l.texts.push(caps(t("Aggiungi un server", "Add a server"), 11.0 * s, MUTED).on_baseline(x0, top));
        l.texts.push(Text::new("Komga, Kavita", Face::Serif, 40.0 * s, INK).on_baseline(x0, top + 46.0 * s));
        let help = [
            t(
                "Komga: l'indirizzo del server (http://casa:25600), con nome e password.",
                "Komga: the server address (http://home:25600), with user name and password.",
            ),
            t(
                "Kavita: il link OPDS che c'\u{e8} nelle impostazioni dell'utente; nome e password non servono.",
                "Kavita: the OPDS link from the user settings; no user name or password needed.",
            ),
        ];
        for (k, line) in help.into_iter().enumerate() {
            let text = Text::new(line, Face::Sans, 13.0 * s, MUTED);
            l.texts.push(fit(m, text, w).on_baseline(x0, top + (78.0 + 20.0 * k as f32) * s));
        }
        let labels = [t("Indirizzo", "Address"), t("Nome", "User name"), t("Password", "Password")];
        for (i, &y) in ys.iter().enumerate() {
            let focused = i == self.focus && !self.busy;
            l.texts.push(caps(labels[i], 9.5 * s, if focused { INK } else { MUTED }).on_baseline(x0, y - 30.0 * s));
            let shown = if i == 2 { "\u{2022}".repeat(self.fields[i].chars().count()) } else { self.fields[i].clone() };
            let shown = tail(m, &shown, 17.0 * s, w - 12.0 * s);
            let text = Text::new(shown, Face::Sans, 17.0 * s, INK);
            let tw = m.width(&text);
            l.texts.push(text.on_baseline(x0, y));
            l.rects.push(Rect::new(
                x0,
                (y + 9.0 * s).round(),
                w,
                if focused { (2.0 * s).round() } else { s.round() },
                if focused { ACCENT } else { HAIR },
            ));
            if focused {
                let h = (20.0 * s).round();
                l.rects.push(Rect::new(
                    (x0 + tw + 2.0 * s).round(),
                    (y - h + 4.0 * s).round(),
                    s.round().max(1.0),
                    h,
                    ACCENT,
                ));
            }
        }
        let below = ys[2] + 50.0 * s;
        let (line, color) = match (&self.error, self.busy) {
            (_, true) => (t("Collegamento in corso\u{2026}", "Connecting\u{2026}").to_owned(), MUTED),
            (Some(e), _) => (e.clone(), ACCENT_TEXT),
            (None, _) => (
                t(
                    "Tab per il campo dopo   \u{00b7}   Invio per aggiungere   \u{00b7}   Esc per lasciar stare",
                    "Tab for the next field   \u{00b7}   Enter to add   \u{00b7}   Esc to cancel",
                )
                .to_owned(),
                MUTED,
            ),
        };
        let text = Text::new(line, Face::Sans, 12.5 * s, color);
        l.texts.push(fit(m, text, w).on_baseline(x0, below));
        l
    }
}

/// Dove sta il modulo: (sinistra, larghezza, linea di base di ogni campo).
fn geo(ctx: &Context) -> (f32, f32, [f32; 3]) {
    let s = ctx.scale;
    let (vw, vh) = ctx.view;
    let w = (560.0 * s).min(vw - 48.0 * s).round();
    let x0 = ((vw - w) / 2.0).round();
    let first = (vh / 2.0 - 20.0 * s).round().max(214.0 * s);
    let gap = (74.0 * s).round();
    (x0, w, [first, first + gap, first + 2.0 * gap])
}

/// La fine del testo, se tutto non ci sta: mentre si scrive un indirizzo
/// lungo si vede l'ultimo pezzo, quello che si sta scrivendo.
fn tail(m: &mut dyn Measure, text: &str, size: f32, room: f32) -> String {
    if m.width(&Text::new(text, Face::Sans, size, INK)) <= room {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0, chars.len());
    // il primo carattere da cui "…resto" ci sta
    while lo < hi {
        let mid = (lo + hi) / 2;
        let candidate: String = std::iter::once('\u{2026}').chain(chars[mid..].iter().copied()).collect();
        if m.width(&Text::new(candidate, Face::Sans, size, INK)) <= room {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    std::iter::once('\u{2026}').chain(chars[lo..].iter().copied()).collect()
}
