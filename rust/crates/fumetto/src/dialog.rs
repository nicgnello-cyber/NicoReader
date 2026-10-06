//! Le finestre di sistema: scegliere un volume, avvisare di un errore.
//!
//! Non si aprono dentro il ciclo degli eventi. Una finestra modale ha un
//! ciclo suo, e se nel frattempo Windows chiede di ridisegnare la nostra
//! (basta spostarci sopra il dialogo) winit rientra nel gestore gia' occupato
//! e va in panico. Girano quindi su un thread a parte: la finestra del lettore
//! ne e' la proprietaria e resta disabilitata finche' sono aperte, la risposta
//! torna come evento. Su macOS si creano comunque qui, sul thread principale,
//! come vuole AppKit.

use std::path::Path;

use fumetto_core::lingua::t;
use rfd::{AsyncFileDialog, AsyncMessageDialog, MessageButtons, MessageLevel};
use winit::event_loop::EventLoopProxy;
use winit::window::Window;

use crate::app::UserEvent;

/// Le estensioni proposte. Il tipo vero si riconosce dal contenuto: "Tutti i
/// file" apre anche un CBZ che si chiama .jpg.
const EXTENSIONS: &[&str] = &[
    "cbz", "cbr", "cb7", "cbt", "zip", "rar", "7z", "tar", "pdf", "jpg", "jpeg", "png", "webp", "gif", "bmp", "avif",
    "jxl",
];

/// Chiede un volume (o una cartella). `near`: dove cominciare; senza, il
/// sistema riparte dall'ultima cartella usata in questa finestra.
/// `library`: la cartella scelta va nella libreria, non si apre.
pub fn open(window: &Window, folder: bool, near: Option<&Path>, library: bool, proxy: EventLoopProxy<UserEvent>) {
    let mut d = AsyncFileDialog::new().set_parent(window);
    if let Some(dir) = near {
        d = d.set_directory(dir);
    }
    if folder {
        let title =
            if library { t("Aggiungi alla libreria", "Add to the library") } else { t("Apri cartella", "Open folder") };
        let chosen = d.set_title(title).pick_folder();
        run(async move { chosen.await.map(|f| f.path().to_owned()) }, move |p| UserEvent::Chosen(p, library), proxy);
    } else {
        let chosen = d
            .set_title(t("Apri fumetto", "Open comic"))
            .add_filter(t("Fumetti e immagini", "Comics and images"), EXTENSIONS)
            .add_filter(t("Tutti i file", "All files"), &["*"])
            .pick_file();
        run(async move { chosen.await.map(|f| f.path().to_owned()) }, move |p| UserEvent::Chosen(p, false), proxy);
    }
}

/// Chiede dove salvare una pagina: `name` e' il nome proposto, `near` la
/// cartella da cui partire.
pub fn save(window: &Window, name: String, near: Option<&Path>, proxy: EventLoopProxy<UserEvent>) {
    let ext = Path::new(&name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let mut d =
        AsyncFileDialog::new().set_parent(window).set_title(t("Salva la pagina", "Save the page")).set_file_name(name);
    if let Some(dir) = near {
        d = d.set_directory(dir);
    }
    if !ext.is_empty() {
        d = d.add_filter(ext.to_uppercase(), &[ext.as_str()]);
    }
    let chosen = d.add_filter("PNG", &["png"]).add_filter("JPEG", &["jpg", "jpeg"]).save_file();
    run(async move { chosen.await.map(|f| f.path().to_owned()) }, UserEvent::SaveTo, proxy);
}

/// Propone di aprire la pagina di una versione nuova; se si', torna
/// `OpenUpdate` con il suo indirizzo.
pub fn offer_update(window: &Window, release: &fumetto_core::update::Release, proxy: EventLoopProxy<UserEvent>) {
    let (new, this) = (&release.version, env!("CARGO_PKG_VERSION"));
    let text = if fumetto_core::lingua::italian() {
        format!(
            "È uscito NicoReader {new} (questo è il {this}).\n\nApro la sua pagina su GitHub, con le novità e \
             i file da scaricare?\n\nL'avviso si può spegnere nelle impostazioni (Lettura)."
        )
    } else {
        format!(
            "NicoReader {new} is out (this is {this}).\n\nOpen its page on GitHub, with what's new and the \
             files to download?\n\nThis notice can be turned off in the settings (Reading)."
        )
    };
    let asked = AsyncMessageDialog::new()
        .set_parent(window)
        .set_level(MessageLevel::Info)
        .set_title("NicoReader")
        .set_description(text)
        .set_buttons(MessageButtons::YesNo)
        .show();
    let url = release.url.clone();
    run(
        asked,
        move |r| if r == rfd::MessageDialogResult::Yes { UserEvent::OpenUpdate(url) } else { UserEvent::DialogClosed },
        proxy,
    );
}

/// Un avviso con il solo tasto OK.
pub fn error(window: &Window, text: String, proxy: EventLoopProxy<UserEvent>) {
    let shown = AsyncMessageDialog::new()
        .set_parent(window)
        .set_level(MessageLevel::Error)
        .set_title("NicoReader")
        .set_description(text)
        .set_buttons(MessageButtons::Ok)
        .show();
    run(shown, |_| UserEvent::DialogClosed, proxy);
}

/// Chiede se spostare un volume nel cestino; se si', torna `Confirmed`.
pub fn confirm_trash(window: &Window, path: std::path::PathBuf, proxy: EventLoopProxy<UserEvent>) {
    let name = path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned();
    let text = if fumetto_core::lingua::italian() {
        format!("Spostare «{name}» nel cestino?\n\nDal cestino si può sempre recuperare.")
    } else {
        format!("Move \u{201c}{name}\u{201d} to the trash?\n\nIt can always be restored from the trash.")
    };
    let asked = AsyncMessageDialog::new()
        .set_parent(window)
        .set_level(MessageLevel::Warning)
        .set_title("NicoReader")
        .set_description(text)
        .set_buttons(MessageButtons::YesNo)
        .show();
    run(
        asked,
        move |r| if r == rfd::MessageDialogResult::Yes { UserEvent::Confirmed(path) } else { UserEvent::DialogClosed },
        proxy,
    );
}

/// Il testo per un volume che non si apre.
pub fn cant_open(path: &Path, e: &fumetto_core::Error) -> String {
    let name = path.file_name().unwrap_or(path.as_os_str()).to_string_lossy();
    let head = if fumetto_core::lingua::italian() {
        format!("Impossibile aprire «{name}».")
    } else {
        format!("Can't open \u{201c}{name}\u{201d}.")
    };
    format!("{head}\n\n{e}")
}

fn run<T>(
    dialog: impl Future<Output = T> + Send + 'static, answer: impl FnOnce(T) -> UserEvent + Send + 'static,
    proxy: EventLoopProxy<UserEvent>,
) {
    std::thread::Builder::new()
        .name("dialogo".into())
        .spawn(move || {
            let _ = proxy.send_event(answer(pollster::block_on(dialog)));
        })
        .expect("thread del dialogo");
}
