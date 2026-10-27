//! I moduli da scrivere: aggiungere un server alla libreria (Komga, Kavita:
//! indirizzo, nome e password; Invio prova il collegamento, il server entra
//! nella libreria solo se risponde), la password di un PDF protetto, i dati
//! di un volume corretti a mano.

use std::path::PathBuf;

use super::*;
use fumetto_core::edits::VolumeEdit;
use fumetto_core::remote::Server;

/// Quanto puo' essere lungo un campo: un indirizzo con la chiave di Kavita
/// sta largo dentro.
const MAX_FIELD: usize = 2000;

pub(crate) struct ServerForm {
    /// Indirizzo, nome, password; per un PDF solo la password.
    fields: [String; 3],
    /// La password di un PDF: il volume e il suo titolo.
    pdf: Option<(PathBuf, String)>,
    /// I dati di un volume: titolo, serie e numero nei campi, piu' il verso.
    volume: Option<VolumeData>,
    focus: usize,
    /// Si sta provando il collegamento: si aspetta.
    busy: bool,
    /// Cosa e' andato storto l'ultima volta.
    error: Option<String>,
}

/// Il volume di cui si correggono i dati.
pub(super) struct VolumeData {
    pub path: PathBuf,
    /// Il nome del file, in testa al modulo.
    pub name: String,
    /// I campi come erano all'apertura: si salva solo cio' che cambia.
    pub initial: [String; 3],
    /// Le correzioni che c'erano gia'.
    pub existing: VolumeEdit,
    /// Il verso scelto: `None` come dice il file.
    pub right_to_left: Option<bool>,
}

/// La riga del verso, dopo i tre campi.
const DIRECTION: usize = 3;

/// Cosa fa un tasto nel modulo.
pub(super) enum Outcome {
    Stay,
    Close,
    Submit(Command),
}

impl ServerForm {
    pub(super) fn new() -> ServerForm {
        ServerForm { fields: Default::default(), pdf: None, volume: None, focus: 0, busy: false, error: None }
    }

    /// I dati di un volume da correggere, con i valori di adesso nei campi.
    pub(super) fn volume(data: VolumeData) -> ServerForm {
        ServerForm { fields: data.initial.clone(), volume: Some(data), ..ServerForm::new() }
    }

    /// La password per aprire un PDF protetto.
    pub(super) fn pdf(path: PathBuf, title: String) -> ServerForm {
        ServerForm { pdf: Some((path, title)), ..ServerForm::new() }
    }

    /// E' la richiesta della password di un PDF (non un server).
    pub(super) fn is_pdf(&self) -> bool {
        self.pdf.is_some()
    }

    /// Quanti campi da scrivere ha il modulo.
    fn count(&self) -> usize {
        if self.pdf.is_some() { 1 } else { 3 }
    }

    /// Quante righe si scelgono con Tab: i campi, e il verso per un volume.
    fn rows(&self) -> usize {
        self.count() + usize::from(self.volume.is_some())
    }

    /// Il campo si scrive a pallini.
    fn masked(&self, i: usize) -> bool {
        self.volume.is_none() && i == self.count() - 1
    }

    /// Il verso dopo (o prima): come il file, da sinistra, da destra.
    fn turn_direction(&mut self, forward: bool) {
        let Some(v) = &mut self.volume else { return };
        const ORDER: [Option<bool>; 3] = [None, Some(false), Some(true)];
        let k = ORDER.iter().position(|d| *d == v.right_to_left).unwrap_or(0);
        v.right_to_left = ORDER[(k + if forward { 1 } else { 2 }) % 3];
    }

    pub(super) fn key(&mut self, key: Key) -> Outcome {
        if key == Key::Escape {
            return Outcome::Close;
        }
        if self.busy {
            return Outcome::Stay;
        }
        if self.focus == DIRECTION {
            match key {
                Key::Left | Key::Right | Key::Char(' ') => {
                    self.turn_direction(key != Key::Left);
                    return Outcome::Stay;
                }
                Key::Char(_) | Key::Digit(_) | Key::Backspace => return Outcome::Stay,
                _ => {}
            }
        }
        match key {
            Key::Char(c) => self.type_text(&c.to_string()),
            Key::Digit(d) => self.type_text(&d.to_string()),
            Key::Backspace => {
                self.fields[self.focus].pop();
            }
            Key::Tab | Key::Down => self.focus = (self.focus + 1) % self.rows(),
            Key::Up => self.focus = (self.focus + self.rows() - 1) % self.rows(),
            Key::Enter => return self.submit(),
            _ => {}
        }
        Outcome::Stay
    }

    /// Testo scritto o incollato, nel campo scelto (senza a capo).
    pub(super) fn type_text(&mut self, text: &str) {
        if self.busy || self.focus >= self.count() {
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
        if let Some(v) = &self.volume {
            let [title, series, number] = &self.fields;
            let number = number.trim();
            let parsed = if number.is_empty() { Some(None) } else { number.parse::<u32>().ok().map(Some) };
            let Some(parsed) = parsed else {
                self.error =
                    Some(t("Il numero dev'essere un numero intero.", "The number must be a whole number.").into());
                self.focus = 2;
                return Outcome::Stay;
            };
            // si cambia solo cio' che si e' toccato: il resto resta com'era
            let mut edit = v.existing.clone();
            if *title != v.initial[0] {
                // il titolo vuoto: torna quello del file
                edit.title = Some(title.trim().to_owned()).filter(|t| !t.is_empty());
            }
            if *series != v.initial[1] {
                edit.series = Some(series.trim().to_owned());
            }
            if self.fields[2] != v.initial[2] {
                edit.number = Some(parsed);
            }
            edit.right_to_left = v.right_to_left;
            self.busy = true;
            return Outcome::Submit(Command::SaveVolume(v.path.clone(), edit));
        }
        if let Some((path, _)) = &self.pdf {
            if self.fields[0].is_empty() {
                return Outcome::Stay;
            }
            self.busy = true;
            self.error = None;
            return Outcome::Submit(Command::OpenWithPassword(path.clone(), self.fields[0].clone()));
        }
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
        let server = Server { url, user: self.fields[1].trim().to_owned(), password: self.fields[2].clone() };
        Outcome::Submit(Command::AddServer(server))
    }

    /// La prova del collegamento e' andata male: si dice perche', e si puo'
    /// correggere.
    pub(super) fn failed(&mut self, error: String) {
        self.busy = false;
        self.error = Some(error);
        // la password sbagliata si riscrive da capo
        if self.pdf.is_some() {
            self.fields[0].clear();
        }
    }

    /// Un clic: sceglie il campo sotto il mouse.
    pub(super) fn click(&mut self, ctx: &Context, x: f32, y: f32) {
        let (x0, w, ys) = geo(ctx);
        let s = ctx.scale;
        if let Some(i) = ys[..self.rows()].iter().position(|&fy| y > fy - 44.0 * s && y < fy + 12.0 * s)
            && x >= x0
            && x <= x0 + w
            && !self.busy
        {
            // un clic sul verso gia' scelto lo cambia (si sceglie anche col dito)
            if i == DIRECTION && self.focus == DIRECTION {
                self.turn_direction(true);
            }
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
        if let Some(v) = &self.volume {
            l.texts.push(caps(t("Dati del volume", "Volume details"), 11.0 * s, MUTED).on_baseline(x0, top));
            let name = Text::new(v.name.as_str(), Face::Serif, 40.0 * s, INK);
            l.texts.push(fit(m, name, w).on_baseline(x0, top + 46.0 * s));
            let help = [
                t(
                    "Si correggono per NicoReader: il file resta com'\u{e8}.",
                    "They're corrected for NicoReader: the file stays as it is.",
                ),
                t(
                    "Senza serie \u{e8} un volume unico; il titolo vuoto torna quello del file.",
                    "No series makes it a standalone volume; an empty title goes back to the file's.",
                ),
            ];
            for (k, line) in help.into_iter().enumerate() {
                let text = Text::new(line, Face::Sans, 13.0 * s, MUTED);
                l.texts.push(fit(m, text, w).on_baseline(x0, top + (78.0 + 20.0 * k as f32) * s));
            }
        } else if let Some((_, title)) = &self.pdf {
            l.texts.push(caps(t("PDF protetto", "Protected PDF"), 11.0 * s, MUTED).on_baseline(x0, top));
            let name = Text::new(title.as_str(), Face::Serif, 40.0 * s, INK);
            l.texts.push(fit(m, name, w).on_baseline(x0, top + 46.0 * s));
            let help = [
                t("Per aprirlo serve la sua password.", "It needs its password to open."),
                t(
                    "Su Windows e macOS resta nel portachiavi; su Linux si chiede a ogni avvio.",
                    "Windows and macOS keep it in the keychain; on Linux it is asked at each launch.",
                ),
            ];
            for (k, line) in help.into_iter().enumerate() {
                let text = Text::new(line, Face::Sans, 13.0 * s, MUTED);
                l.texts.push(fit(m, text, w).on_baseline(x0, top + (78.0 + 20.0 * k as f32) * s));
            }
        } else {
            l.texts.push(caps(t("Aggiungi un server", "Add a server"), 11.0 * s, MUTED).on_baseline(x0, top));
            l.texts.push(Text::new("Komga, Kavita", Face::Serif, 40.0 * s, INK).on_baseline(x0, top + 46.0 * s));
        }
        let help = if self.pdf.is_some() || self.volume.is_some() {
            &[][..]
        } else {
            &[
                t(
                    "Komga: l'indirizzo del server (http://casa:25600), con nome e password.",
                    "Komga: the server address (http://home:25600), with user name and password.",
                ),
                t(
                    "Kavita: il link OPDS che c'\u{e8} nelle impostazioni dell'utente; nome e password non servono.",
                    "Kavita: the OPDS link from the user settings; no user name or password needed.",
                ),
            ][..]
        };
        for (k, &line) in help.iter().enumerate() {
            let text = Text::new(line, Face::Sans, 13.0 * s, MUTED);
            l.texts.push(fit(m, text, w).on_baseline(x0, top + (78.0 + 20.0 * k as f32) * s));
        }
        let labels = if self.pdf.is_some() {
            [t("Password", "Password"); 1].to_vec()
        } else if self.volume.is_some() {
            vec![t("Titolo", "Title"), t("Serie", "Series"), t("Numero", "Number")]
        } else {
            vec![t("Indirizzo", "Address"), t("Nome", "User name"), t("Password", "Password")]
        };
        for (i, &y) in ys[..self.count()].iter().enumerate() {
            let focused = i == self.focus && !self.busy;
            l.texts.push(caps(labels[i], 9.5 * s, if focused { INK } else { MUTED }).on_baseline(x0, y - 30.0 * s));
            let shown =
                if self.masked(i) { "\u{2022}".repeat(self.fields[i].chars().count()) } else { self.fields[i].clone() };
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
        if let Some(v) = &self.volume {
            let y = ys[DIRECTION];
            let focused = self.focus == DIRECTION && !self.busy;
            let label = caps(t("Verso di lettura", "Reading direction"), 9.5 * s, if focused { INK } else { MUTED });
            l.texts.push(label.on_baseline(x0, y - 30.0 * s));
            let value = match v.right_to_left {
                None => t("Come dice il file", "As the file says"),
                Some(false) => t("Da sinistra a destra", "Left to right"),
                Some(true) => t("Da destra a sinistra (manga)", "Right to left (manga)"),
            };
            let shown = format!("\u{2039}   {value}   \u{203a}");
            l.texts.push(Text::new(shown, Face::Sans, 17.0 * s, INK).on_baseline(x0, y));
            l.rects.push(Rect::new(
                x0,
                (y + 9.0 * s).round(),
                w,
                if focused { (2.0 * s).round() } else { s.round() },
                if focused { ACCENT } else { HAIR },
            ));
        }
        let below = ys[self.rows() - 1] + 50.0 * s;
        let (line, color) = match (&self.error, self.busy) {
            (_, true) if self.pdf.is_some() => (t("Apertura\u{2026}", "Opening\u{2026}").to_owned(), MUTED),
            (_, true) if self.volume.is_some() => (t("Salvataggio\u{2026}", "Saving\u{2026}").to_owned(), MUTED),
            (_, true) => (t("Collegamento in corso\u{2026}", "Connecting\u{2026}").to_owned(), MUTED),
            (Some(e), _) => (e.clone(), ACCENT_TEXT),
            (None, _) if self.volume.is_some() => (
                t(
                    "Tab per il campo dopo   \u{00b7}   \u{2190} \u{2192} per il verso   \u{00b7}   Invio per salvare   \u{00b7}   Esc per lasciar stare",
                    "Tab for the next field   \u{00b7}   \u{2190} \u{2192} for the direction   \u{00b7}   Enter to save   \u{00b7}   Esc to cancel",
                )
                .to_owned(),
                MUTED,
            ),
            (None, _) if self.pdf.is_some() => (
                t("Invio per aprire   \u{00b7}   Esc per lasciar stare", "Enter to open   \u{00b7}   Esc to cancel")
                    .to_owned(),
                MUTED,
            ),
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

/// Dove sta il modulo: (sinistra, larghezza, linea di base di ogni riga).
fn geo(ctx: &Context) -> (f32, f32, [f32; 4]) {
    let s = ctx.scale;
    let (vw, vh) = ctx.view;
    let w = (560.0 * s).min(vw - 48.0 * s).round();
    let x0 = ((vw - w) / 2.0).round();
    let first = (vh / 2.0 - 20.0 * s).round().max(214.0 * s);
    let gap = (74.0 * s).round();
    (x0, w, [first, first + gap, first + 2.0 * gap, first + 3.0 * gap])
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
