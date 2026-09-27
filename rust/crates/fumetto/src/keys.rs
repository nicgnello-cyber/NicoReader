//! I tasti: quali fanno cosa, quelli di serie e quelli scelti da chi legge.
//!
//! Ogni comando che si puo' dare da tastiera e' un [`Bind`], con un nome
//! stabile (quello che si scrive in `impostazioni.json`) e i suoi tasti di
//! serie. Le scelte di chi legge sostituiscono, comando per comando, quelli
//! di serie; un tasto dato a un comando si toglie a quello che l'aveva.
//!
//! Un tasto si scrive come "Ctrl+Shift+O", "F11", "Right", "+". Ctrl vuol
//! dire Cmd su macOS. Per le lettere conta il Maiusc (R e Maiusc+R sono due
//! tasti); per i simboli no, perche' e' il Maiusc stesso a farli ("+" su una
//! tastiera americana e' Maiusc+=).
//!
//! I tasti di menu, "vai a pagina" e libreria (frecce, Invio, Esc) restano
//! quelli: sono la grammatica dell'interfaccia, non comandi.

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use fumetto_core::lingua::{italian, t};

use crate::reader::{Action, Zoom};

/// Un comando da tastiera.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Bind {
    Right,
    Left,
    Down,
    Up,
    Next,
    Prev,
    First,
    Last,
    GoTo,
    Thumbs,
    Double,
    Cover,
    Strip,
    StripWider,
    StripNarrower,
    Manga,
    ZoomIn,
    ZoomOut,
    ZoomPage,
    ZoomWidth,
    ZoomActual,
    Lens,
    RotateRight,
    RotateLeft,
    Trim,
    Upscale,
    Linear,
    Slideshow,
    Bookmark,
    Save,
    Copy,
    Open,
    OpenFolder,
    Library,
    Close,
    Fullscreen,
    Hud,
    Settings,
    Quit,
}

impl Bind {
    pub const ALL: [Bind; 39] = [
        Bind::Right, Bind::Left, Bind::Down, Bind::Up, Bind::Next, Bind::Prev, Bind::First, Bind::Last, Bind::GoTo,
        Bind::Thumbs, Bind::Double, Bind::Cover, Bind::Strip, Bind::StripWider, Bind::StripNarrower, Bind::Manga,
        Bind::ZoomIn, Bind::ZoomOut, Bind::ZoomPage, Bind::ZoomWidth, Bind::ZoomActual, Bind::Lens, Bind::RotateRight,
        Bind::RotateLeft, Bind::Trim, Bind::Upscale, Bind::Linear, Bind::Slideshow, Bind::Bookmark, Bind::Save,
        Bind::Copy, Bind::Open, Bind::OpenFolder, Bind::Library, Bind::Close, Bind::Fullscreen, Bind::Hud,
        Bind::Settings, Bind::Quit,
    ];

    /// Il nome che si scrive nelle impostazioni: non cambia mai.
    pub fn id(self) -> &'static str {
        match self {
            Bind::Right => "destra",
            Bind::Left => "sinistra",
            Bind::Down => "giu",
            Bind::Up => "su",
            Bind::Next => "avanti",
            Bind::Prev => "indietro",
            Bind::First => "inizio",
            Bind::Last => "fine",
            Bind::GoTo => "vai_a_pagina",
            Bind::Thumbs => "miniature",
            Bind::Double => "doppia_pagina",
            Bind::Cover => "copertina_sola",
            Bind::Strip => "nastro",
            Bind::StripWider => "nastro_largo",
            Bind::StripNarrower => "nastro_stretto",
            Bind::Manga => "manga",
            Bind::ZoomIn => "ingrandisci",
            Bind::ZoomOut => "riduci",
            Bind::ZoomPage => "pagina_intera",
            Bind::ZoomWidth => "larga_come_la_finestra",
            Bind::ZoomActual => "pixel_reali",
            Bind::Lens => "lente",
            Bind::RotateRight => "ruota_a_destra",
            Bind::RotateLeft => "ruota_a_sinistra",
            Bind::Trim => "rifila",
            Bind::Upscale => "migliora",
            Bind::Linear => "luce_lineare",
            Bind::Slideshow => "presentazione",
            Bind::Bookmark => "segnalibro",
            Bind::Save => "salva",
            Bind::Copy => "copia",
            Bind::Open => "apri",
            Bind::OpenFolder => "apri_cartella",
            Bind::Library => "libreria",
            Bind::Close => "chiudi",
            Bind::Fullscreen => "schermo_intero",
            Bind::Hud => "barra",
            Bind::Settings => "impostazioni",
            Bind::Quit => "esci",
        }
    }

    pub fn from_id(id: &str) -> Option<Bind> {
        Bind::ALL.into_iter().find(|b| b.id() == id)
    }

    /// Il nome per chi legge.
    pub fn label(self) -> &'static str {
        match self {
            Bind::Right => t("Destra (pagina o spostamento)", "Right (page or pan)"),
            Bind::Left => t("Sinistra (pagina o spostamento)", "Left (page or pan)"),
            Bind::Down => t("Giù (scorre o sposta)", "Down (scroll or pan)"),
            Bind::Up => t("Su (scorre o sposta)", "Up (scroll or pan)"),
            Bind::Next => t("Pagina avanti", "Next page"),
            Bind::Prev => t("Pagina indietro", "Previous page"),
            Bind::First => t("Prima pagina", "First page"),
            Bind::Last => t("Ultima pagina", "Last page"),
            Bind::GoTo => t("Vai a pagina", "Go to page"),
            Bind::Thumbs => t("Miniature", "Thumbnails"),
            Bind::Double => t("Doppia pagina", "Two pages"),
            Bind::Cover => t("Copertina da sola", "Cover alone"),
            Bind::Strip => t("Nastro", "Strip"),
            Bind::StripWider => t("Nastro più largo", "Wider strip"),
            Bind::StripNarrower => t("Nastro più stretto", "Narrower strip"),
            Bind::Manga => t("Da destra a sinistra", "Right to left"),
            Bind::ZoomIn => t("Ingrandisci", "Zoom in"),
            Bind::ZoomOut => t("Riduci", "Zoom out"),
            Bind::ZoomPage => t("Pagina intera", "Whole page"),
            Bind::ZoomWidth => t("Larga quanto la finestra", "Fit width"),
            Bind::ZoomActual => t("Pixel reali", "Actual pixels"),
            Bind::Lens => t("Lente", "Magnifier"),
            Bind::RotateRight => t("Ruota a destra", "Rotate right"),
            Bind::RotateLeft => t("Ruota a sinistra", "Rotate left"),
            Bind::Trim => t("Rifila i margini", "Trim margins"),
            Bind::Upscale => t("Migliora le scansioni (AI)", "Enhance scans (AI)"),
            Bind::Linear => t("Luce lineare o gamma", "Linear light or gamma"),
            Bind::Slideshow => t("Presentazione", "Slideshow"),
            Bind::Bookmark => t("Segna la pagina", "Bookmark the page"),
            Bind::Save => t("Salva la pagina", "Save the page"),
            Bind::Copy => t("Copia la pagina", "Copy the page"),
            Bind::Open => t("Apri", "Open"),
            Bind::OpenFolder => t("Apri cartella", "Open folder"),
            Bind::Library => t("Libreria", "Library"),
            Bind::Close => t("Chiudi il volume", "Close the volume"),
            Bind::Fullscreen => t("Schermo intero", "Full screen"),
            Bind::Hud => t("Barra in alto", "Top bar"),
            Bind::Settings => t("Impostazioni", "Settings"),
            Bind::Quit => t("Esci", "Quit"),
        }
    }

    /// Il gruppo, nelle impostazioni.
    pub fn group(self) -> &'static str {
        match self {
            Bind::Right | Bind::Left | Bind::Down | Bind::Up | Bind::Next | Bind::Prev | Bind::First | Bind::Last
            | Bind::GoTo | Bind::Thumbs => t("Sfogliare", "Turning pages"),
            Bind::Double | Bind::Cover | Bind::Strip | Bind::StripWider | Bind::StripNarrower | Bind::Manga => {
                t("Modi di lettura", "Reading modes")
            }
            Bind::ZoomIn | Bind::ZoomOut | Bind::ZoomPage | Bind::ZoomWidth | Bind::ZoomActual | Bind::Lens => "Zoom",
            Bind::RotateRight | Bind::RotateLeft | Bind::Trim | Bind::Upscale | Bind::Linear => t("La pagina", "The page"),
            Bind::Slideshow | Bind::Bookmark | Bind::Save | Bind::Copy => t("Altro", "More"),
            Bind::Open | Bind::OpenFolder | Bind::Library | Bind::Close | Bind::Fullscreen | Bind::Hud | Bind::Settings
            | Bind::Quit => t("Volumi e finestra", "Volumes and window"),
        }
    }

    /// I tasti di serie.
    pub fn defaults(self) -> &'static [&'static str] {
        match self {
            Bind::Right => &["Right"],
            Bind::Left => &["Left"],
            Bind::Down => &["Down"],
            Bind::Up => &["Up"],
            Bind::Next => &["Space", "PageDown"],
            Bind::Prev => &["Backspace", "PageUp"],
            Bind::First => &["Home"],
            Bind::Last => &["End"],
            Bind::GoTo => &["Ctrl+G"],
            Bind::Thumbs => &["T"],
            Bind::Double => &["D"],
            Bind::Cover => &["O"],
            Bind::Strip => &["V"],
            Bind::StripWider => &["]"],
            Bind::StripNarrower => &["["],
            Bind::Manga => &["M"],
            Bind::ZoomIn => &["+", "="],
            Bind::ZoomOut => &["-"],
            Bind::ZoomPage => &["0"],
            Bind::ZoomWidth => &["1"],
            Bind::ZoomActual => &["2"],
            Bind::Lens => &["L"],
            Bind::RotateRight => &["R"],
            Bind::RotateLeft => &["Shift+R"],
            Bind::Trim => &["C"],
            Bind::Upscale => &["U"],
            Bind::Linear => &["G"],
            Bind::Slideshow => &["S"],
            Bind::Bookmark => &["Ctrl+B"],
            Bind::Save => &["Ctrl+S"],
            Bind::Copy => &["Ctrl+C"],
            Bind::Open => &["Ctrl+O"],
            Bind::OpenFolder => &["Ctrl+Shift+O"],
            Bind::Library => &["Ctrl+L"],
            Bind::Close => &["Ctrl+W"],
            Bind::Fullscreen => &["F", "F11"],
            Bind::Hud => &["H"],
            Bind::Settings => &["Ctrl+,"],
            Bind::Quit => &["Escape"],
        }
    }
}

/// Un tasto con i suoi modificatori. `key`: una lettera maiuscola, un
/// simbolo, o il nome di un tasto ("Right", "F11", "Space").
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Combo {
    pub key: String,
    /// Ctrl (Cmd su macOS).
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Combo {
    /// Da un tasto premuto. Per i simboli il Maiusc non conta: e' lui a farli.
    pub fn new(key: &str, ctrl: bool, shift: bool, alt: bool) -> Option<Combo> {
        let mut chars = key.chars();
        let key = match (chars.next(), chars.next()) {
            (Some(c), None) if c.is_alphabetic() => c.to_uppercase().collect(),
            (Some(c), None) if c.is_control() || c.is_whitespace() => return None,
            (Some(c), None) => c.to_string(),
            (Some(_), Some(_)) => key.to_owned(),
            (None, _) => return None,
        };
        let letter = key.chars().count() == 1 && key.chars().all(char::is_alphabetic);
        let named = key.chars().count() > 1;
        Some(Combo { shift: shift && (letter || named), key, ctrl, alt })
    }

    /// Da come si scrive nelle impostazioni: "Ctrl+Shift+O", "+", "Ctrl++".
    pub fn parse(s: &str) -> Option<Combo> {
        let (mods, key) = match s.strip_suffix("++") {
            Some(rest) => (rest, "+"),
            None if s == "+" => ("", "+"),
            None => s.rsplit_once('+').unwrap_or(("", s)),
        };
        let (mut ctrl, mut shift, mut alt) = (false, false, false);
        for m in mods.split('+').filter(|m| !m.is_empty()) {
            match m {
                "Ctrl" | "Cmd" => ctrl = true,
                "Shift" => shift = true,
                "Alt" => alt = true,
                _ => return None,
            }
        }
        Combo::new(key, ctrl, shift, alt)
    }

    /// Come si scrive nelle impostazioni.
    pub fn stored(&self) -> String {
        let mut s = String::new();
        for (on, name) in [(self.ctrl, "Ctrl+"), (self.shift, "Shift+"), (self.alt, "Alt+")] {
            if on {
                s += name;
            }
        }
        s + &self.key
    }

    /// Come si mostra: "Ctrl+Maiusc+O", "Cmd+O" su macOS, le frecce come frecce.
    pub fn shown(&self) -> String {
        let mac = cfg!(target_os = "macos");
        let mut s = String::new();
        if self.ctrl {
            s += if mac { "Cmd+" } else { "Ctrl+" };
        }
        if self.shift {
            s += if italian() { "Maiusc+" } else { "Shift+" };
        }
        if self.alt {
            s += if mac { "Opt+" } else { "Alt+" };
        }
        let key = match self.key.as_str() {
            "Right" => "\u{2192}",
            "Left" => "\u{2190}",
            "Up" => "\u{2191}",
            "Down" => "\u{2193}",
            "Space" => t("Spazio", "Space"),
            "PageUp" => t("Pag\u{2191}", "PgUp"),
            "PageDown" => t("Pag\u{2193}", "PgDn"),
            "Home" => t("Inizio", "Home"),
            "End" => t("Fine", "End"),
            "Escape" => "Esc",
            // il simbolo del tasto non c'e' nei caratteri dell'interfaccia
            "Backspace" => "Backspace",
            "Enter" => t("Invio", "Enter"),
            "Delete" => t("Canc", "Del"),
            "Insert" => t("Ins", "Ins"),
            "Tab" => "Tab",
            "-" => "\u{2212}",
            other => other,
        };
        s + key
    }
}

/// I tasti che valgono adesso: quelli di serie, con le scelte di chi legge.
pub struct Keymap {
    by_combo: HashMap<Combo, Bind>,
    by_bind: HashMap<Bind, Vec<Combo>>,
}

impl Default for Keymap {
    fn default() -> Keymap {
        Keymap::new(&BTreeMap::new())
    }
}

impl Keymap {
    /// `chosen`: per nome del comando, i tasti scelti (che sostituiscono
    /// quelli di serie). Nomi e tasti che non si capiscono si ignorano.
    pub fn new(chosen: &BTreeMap<String, Vec<String>>) -> Keymap {
        let mut by_bind: HashMap<Bind, Vec<Combo>> = HashMap::new();
        for b in Bind::ALL {
            let keys: Vec<Combo> = match chosen.get(b.id()) {
                Some(keys) => keys.iter().filter_map(|k| Combo::parse(k)).collect(),
                None => b.defaults().iter().filter_map(|k| Combo::parse(k)).collect(),
            };
            by_bind.insert(b, keys);
        }
        // un tasto scelto vince su uno di serie rimasto a un altro comando
        let mut by_combo = HashMap::new();
        for chosen_first in [true, false] {
            for b in Bind::ALL {
                if chosen.contains_key(b.id()) == chosen_first {
                    for c in &by_bind[&b] {
                        by_combo.entry(c.clone()).or_insert(b);
                    }
                }
            }
        }
        for (b, keys) in by_bind.iter_mut() {
            keys.retain(|c| by_combo.get(c) == Some(b));
        }
        Keymap { by_combo, by_bind }
    }

    /// Quelli di serie, sempre gli stessi: per le prove e gli scatti.
    pub fn defaults() -> &'static Keymap {
        static DEFAULTS: OnceLock<Keymap> = OnceLock::new();
        DEFAULTS.get_or_init(Keymap::default)
    }

    pub fn lookup(&self, combo: &Combo) -> Option<Bind> {
        self.by_combo.get(combo).copied()
    }

    pub fn keys(&self, b: Bind) -> &[Combo] {
        self.by_bind.get(&b).map_or(&[], Vec::as_slice)
    }

    /// Il primo tasto di un comando, come si mostra; vuoto se non ne ha.
    pub fn label(&self, b: Bind) -> String {
        self.keys(b).first().map_or(String::new(), Combo::shown)
    }
}

/// Da' `combo` a `bind`, togliendolo a chi l'aveva (che si restituisce).
/// `chosen` sono le scelte salvate nelle impostazioni.
pub fn assign(chosen: &mut BTreeMap<String, Vec<String>>, bind: Bind, combo: &Combo) -> Option<Bind> {
    let now = Keymap::new(chosen);
    let previous = now.lookup(combo).filter(|&b| b != bind);
    if let Some(p) = previous {
        let rest: Vec<String> = now.keys(p).iter().filter(|c| *c != combo).map(Combo::stored).collect();
        chosen.insert(p.id().to_owned(), rest);
    }
    let mut mine: Vec<String> = now.keys(bind).iter().map(Combo::stored).collect();
    if !mine.contains(&combo.stored()) {
        // il tasto nuovo prende il posto del primo, gli altri restano
        if mine.is_empty() {
            mine.push(combo.stored());
        } else {
            mine[0] = combo.stored();
        }
    }
    chosen.insert(bind.id().to_owned(), mine);
    previous
}

/// Il comando senza tasti.
pub fn clear(chosen: &mut BTreeMap<String, Vec<String>>, bind: Bind) {
    chosen.insert(bind.id().to_owned(), Vec::new());
}

/// Quello che serve sapere per tradurre un comando in un'azione.
pub struct KeyContext {
    pub manga: bool,
    /// La pagina e' ingrandita: le frecce la spostano.
    pub zoomed: bool,
    pub fullscreen: bool,
    /// La finestra, in pixel.
    pub view: (f32, f32),
}

/// L'azione di un comando, adesso.
pub fn action(b: Bind, k: &KeyContext) -> Action {
    let (w, h) = k.view;
    let (right, left) = if k.manga { (Action::Prev, Action::Next) } else { (Action::Next, Action::Prev) };
    match b {
        // pagina ingrandita: le frecce la spostano
        Bind::Right if k.zoomed => Action::Pan(-w * 0.15, 0.0),
        Bind::Left if k.zoomed => Action::Pan(w * 0.15, 0.0),
        Bind::Down if k.zoomed => Action::Pan(0.0, -h * 0.15),
        Bind::Up if k.zoomed => Action::Pan(0.0, h * 0.15),
        Bind::Right => right,
        Bind::Left => left,
        Bind::Down => Action::Scroll(h * 0.2),
        Bind::Up => Action::Scroll(-h * 0.2),
        Bind::Next => Action::Next,
        Bind::Prev => Action::Prev,
        Bind::First => Action::First,
        Bind::Last => Action::Last,
        Bind::GoTo => Action::AskPage,
        Bind::Thumbs => Action::ToggleThumbs,
        Bind::Double => Action::ToggleDouble,
        Bind::Cover => Action::ToggleCover,
        Bind::Strip => Action::ToggleStrip,
        Bind::StripWider => Action::StripWider(1.1),
        Bind::StripNarrower => Action::StripWider(1.0 / 1.1),
        Bind::Manga => Action::ToggleManga,
        Bind::ZoomIn => Action::ZoomIn,
        Bind::ZoomOut => Action::ZoomOut,
        Bind::ZoomPage => Action::ZoomTo(Zoom::Page),
        Bind::ZoomWidth => Action::ZoomTo(Zoom::Width),
        Bind::ZoomActual => Action::ZoomTo(Zoom::Actual),
        Bind::Lens => Action::ToggleLens,
        Bind::RotateRight => Action::Rotate(true),
        Bind::RotateLeft => Action::Rotate(false),
        Bind::Trim => Action::ToggleTrim,
        Bind::Upscale => Action::ToggleUpscale,
        Bind::Linear => Action::ToggleLinear,
        Bind::Slideshow => Action::ToggleSlideshow,
        Bind::Bookmark => Action::ToggleBookmark,
        Bind::Save => Action::SavePage,
        Bind::Copy => Action::CopyPage,
        Bind::Open => Action::Open,
        Bind::OpenFolder => Action::OpenFolder,
        Bind::Library => Action::ToggleLibrary,
        Bind::Close => Action::Close,
        Bind::Fullscreen => Action::ToggleFullscreen,
        Bind::Hud => Action::ToggleHud,
        Bind::Settings => Action::ToggleSettings,
        // a schermo intero Esc prima ne esce
        Bind::Quit if k.fullscreen => Action::ToggleFullscreen,
        Bind::Quit => Action::Quit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn combo(s: &str) -> Combo {
        Combo::parse(s).unwrap()
    }

    #[test]
    fn tutti_i_comandi_hanno_un_nome_unico_e_tasti_di_serie() {
        let mut ids: Vec<&str> = Bind::ALL.iter().map(|b| b.id()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), Bind::ALL.len());
        let km = Keymap::default();
        for b in Bind::ALL {
            assert!(!km.keys(b).is_empty(), "{b:?} senza tasti");
            assert_eq!(Bind::from_id(b.id()), Some(b));
        }
    }

    #[test]
    fn si_scrivono_e_si_rileggono() {
        for s in ["Ctrl+Shift+O", "+", "Ctrl++", "F11", "Right", "Shift+R", "[", "Ctrl+,"] {
            assert_eq!(combo(s).stored(), s);
        }
        assert_eq!(Combo::parse("Pippo+X"), None);
    }

    #[test]
    fn il_maiuscolo_conta_per_le_lettere_non_per_i_simboli() {
        let km = Keymap::default();
        assert_eq!(km.lookup(&Combo::new("r", false, false, false).unwrap()), Some(Bind::RotateRight));
        assert_eq!(km.lookup(&Combo::new("R", false, true, false).unwrap()), Some(Bind::RotateLeft));
        assert_eq!(km.lookup(&Combo::new("+", false, true, false).unwrap()), Some(Bind::ZoomIn), "Maiusc+= fa +");
        assert_eq!(km.lookup(&Combo::new("o", true, true, false).unwrap()), Some(Bind::OpenFolder));
    }

    #[test]
    fn un_tasto_scelto_si_toglie_a_chi_l_aveva() {
        let mut chosen = BTreeMap::new();
        let taken = assign(&mut chosen, Bind::Lens, &combo("D"));
        assert_eq!(taken, Some(Bind::Double));
        let km = Keymap::new(&chosen);
        assert_eq!(km.lookup(&combo("D")), Some(Bind::Lens));
        assert!(km.keys(Bind::Double).is_empty(), "la doppia pagina resta senza tasto");
        assert_eq!(km.lookup(&combo("L")), None, "la L non fa piu' la lente");
        clear(&mut chosen, Bind::Lens);
        assert_eq!(Keymap::new(&chosen).lookup(&combo("D")), None);
    }

    #[test]
    fn un_tasto_scelto_vince_su_uno_di_serie() {
        // scritto a mano nel file: la F per le miniature, la F di serie e' dello schermo intero
        let chosen = BTreeMap::from([("miniature".to_owned(), vec!["F".to_owned()])]);
        let km = Keymap::new(&chosen);
        assert_eq!(km.lookup(&combo("F")), Some(Bind::Thumbs));
        assert_eq!(km.keys(Bind::Fullscreen), [combo("F11")]);
    }

    #[test]
    fn frecce_manga_e_pagina_ingrandita() {
        let k = |manga, zoomed, fullscreen| KeyContext { manga, zoomed, fullscreen, view: (1000.0, 800.0) };
        assert_eq!(action(Bind::Right, &k(false, false, false)), Action::Next);
        assert_eq!(action(Bind::Right, &k(true, false, false)), Action::Prev);
        assert!(matches!(action(Bind::Right, &k(false, true, false)), Action::Pan(..)));
        assert_eq!(action(Bind::Quit, &k(false, false, true)), Action::ToggleFullscreen);
    }
}
