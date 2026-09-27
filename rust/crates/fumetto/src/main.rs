//! Fumetto: un lettore di fumetti, manga e webtoon.
//!
//! La galleria nera: esiste solo la tavola, resa in luce lineare.
//!
//!   fumetto <volume>            apre un archivio (CBZ, CBR, CB7, CBT), un PDF, una cartella o un'immagine
//!   fumetto                     riprende l'ultimo volume letto (o chiede cosa aprire)
//!   fumetto <volume> --prova    legge da solo per mezzo minuto e stampa le misure
//!
//! Tasti: frecce, spazio, pagina su/giu' o clic ai lati per sfogliare; Inizio e
//! Fine; D doppia pagina, O copertina da sola; V nastro, [ e ] la sua
//! larghezza; M manga (da destra a sinistra); + e - zoom (o Ctrl + rotella sul
//! puntatore), 0 pagina intera, frecce o trascinamento per spostarsi; F o F11
//! schermo intero; G luce lineare/gamma (per confronto); Ctrl+O apre un volume,
//! Ctrl+Maiusc+O una cartella, Ctrl+W lo chiude (Cmd su macOS); nella galleria
//! vuota basta un clic per aprire; Ctrl+G vai a pagina; tasto destro, il
//! menu (con i recenti); H mostra o nasconde la barra in alto; 1 larga quanto
//! la finestra, 2 pixel reali; T le miniature di tutte le pagine; R e
//! Maiusc+R girano le pagine; C rifila i margini; S la presentazione; U
//! migliora le scansioni piccole con l'AI (scaricata al primo uso); Ctrl+B
//! segna la pagina; Ctrl+S la salva, Ctrl+C la copia; doppio clic al centro,
//! schermo intero; L la lente; Ctrl+, le impostazioni (luminosita',
//! contrasto, gamma, lente, e tutti questi tasti, che si possono cambiare);
//! Esc esce.
//!
//! I webtoon (strisce molto piu' alte che larghe) si riconoscono all'apertura
//! e si leggono a nastro; l'opzione sta nel menu.
//!
//! Ogni volume riapre dove lo si era lasciato. I progressi stanno in
//! `progressi.json` nella cartella dei dati dell'utente (FUMETTO_DATI per
//! cambiarla). FUMETTO_GPU=dedicata usa la scheda video dedicata;
//! FUMETTO_LANG=it o en sceglie la lingua.

// una finestra, non un programma da console: con il doppio clic non si apre
// anche la finestra nera della console (vedi `attach_console`)
#![windows_subsystem = "windows"]

mod app;
mod cpu_view;
mod dialog;
mod gpu_start;
mod script;
mod stats;
mod system;

use std::path::PathBuf;
use std::time::Instant;

use fumetto_core::{Progress, Settings};
use winit::event_loop::EventLoop;

fn main() {
    let started = Instant::now();
    attach_console();
    let event_loop = match EventLoop::<app::UserEvent>::with_user_event().build() {
        Ok(l) => l,
        Err(e) => fatal(&e.to_string()),
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let prova = args.iter().any(|a| a == "--prova");
    let asked = args.iter().find(|a| !a.starts_with("--")).map(PathBuf::from);
    // la prova automatica non tocca i progressi veri, ne' li usa (a meno di
    // darle una cartella tutta sua con FUMETTO_DATI)
    let own_dir = std::env::var_os("FUMETTO_DATI").is_some();
    let progress = if prova && !own_dir {
        Progress::in_memory()
    } else {
        Progress::load(data_dir().join("progressi.json"))
    };
    let volume = asked.or_else(|| if prova { None } else { resume(&progress) });
    // la prima volta la galleria e' vuota: si chiede subito cosa aprire.
    // Dopo, la galleria mostra gli ultimi letti e si sceglie da li'
    let first_time = progress.recent().is_empty();
    let settings_path = (!prova || own_dir).then(|| data_dir().join("impostazioni.json"));
    let mut settings = settings_path.as_ref().map_or_else(Settings::default, Settings::load);
    if prova {
        // le misure si confrontano fra una prova e l'altra: il volume si apre
        // sempre a pagina singola, anche se fosse un webtoon
        settings.webtoon = false;
    }
    let script = prova.then(script::Script::new);
    let mut app = app::App::new(event_loop.create_proxy(), progress, settings, settings_path, cache_dir(),
                                upscaler_dir(), started, script);
    match volume {
        Some(path) => app.open(&path),
        None => app.ask_open = !prova && first_time,
    }
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("{e}");
    }
    app.save_progress();
    if prova {
        println!("{}", app.stats.report(&app.gpu_name()));
    }
}

/// Avviato senza volume: quale riprendere, tra quelli gia' letti. `None`: si
/// chiede cosa aprire (la finestra di sistema, sopra la galleria vuota).
fn resume(progress: &Progress) -> Option<PathBuf> {
    // l'ultimo letto, anche se finito, e nessun altro: riaprirne uno piu'
    // vecchio sorprenderebbe. Se non c'e' piu' si chiede, e la finestra di
    // sistema riparte dall'ultima cartella usata, dove di solito sta il seguito
    progress.recent().into_iter().next().map(|(path, _)| path).filter(|p| p.exists())
}

/// La cartella di NicoReader dentro `base`. Il programma prima si chiamava
/// Fumetto: la cartella vecchia, se c'e' ancora e la nuova no, diventa la
/// nuova, cosi' progressi, libreria e impostazioni restano.
fn app_dir(base: Option<PathBuf>) -> PathBuf {
    let base = base.unwrap_or_else(std::env::temp_dir);
    let dir = base.join("NicoReader");
    let old = base.join("Fumetto");
    if !dir.exists() && old.is_dir() {
        let _ = std::fs::rename(&old, &dir);
    }
    dir
}

/// %APPDATA%\NicoReader su Windows, ~/Library/Application Support/NicoReader
/// su macOS, ~/.local/share/NicoReader su Linux.
fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("FUMETTO_DATI") {
        return PathBuf::from(dir);
    }
    app_dir(dirs::data_dir())
}

/// Le copertine della libreria: nella cartella della cache del sistema
/// (%LOCALAPPDATA% su Windows), perche' si possono sempre rifare.
fn cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("FUMETTO_DATI") {
        return PathBuf::from(dir).join("copertine");
    }
    app_dir(dirs::cache_dir()).join("copertine")
}

/// L'ingranditore AI, scaricato al primo uso: accanto alle copertine, nella
/// cache del sistema.
fn upscaler_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("FUMETTO_DATI") {
        return PathBuf::from(dir).join("upscaler");
    }
    app_dir(dirs::cache_dir()).join("upscaler")
}

/// Senza console, i messaggi vanno persi. Lanciato da un terminale pero'
/// tornano li' (anche le misure di --prova), a meno che non siano gia'
/// diretti altrove, come in `fumetto --prova > misure.txt`.
#[cfg(windows)]
fn attach_console() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE};
    // SAFETY: nessun puntatore; senza un terminale da cui si e' partiti non fa niente.
    unsafe {
        if GetStdHandle(STD_OUTPUT_HANDLE).is_null() {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

#[cfg(not(windows))]
fn attach_console() {}

/// Un errore che impedisce di andare avanti: senza console va detto in una
/// finestra. Si usa solo quando la nostra finestra non c'e': un dialogo che
/// blocca dentro il ciclo degli eventi e' sicuro solo cosi' (vedi `dialog`).
pub fn fatal(text: &str) -> ! {
    eprintln!("{text}");
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("NicoReader")
        .set_description(text)
        .show();
    std::process::exit(1);
}
