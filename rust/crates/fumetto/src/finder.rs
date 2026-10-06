//! macOS: i volumi aperti dal Finder (doppio clic, "Apri con", trascinati
//! sull'icona nel Dock).
//!
//! Non arrivano fra gli argomenti del programma ma come Apple Event, che
//! AppKit passa al delegato dell'applicazione con `application:openURLs:`.
//! Il delegato e' quello di winit, che quel metodo non ce l'ha (senza, macOS
//! dice che NicoReader non sa aprire quel tipo di file): glielo si aggiunge
//! qui, prima che il programma parta.
//!
//! I file aperti al lancio arrivano prima della finestra: stanno in `PENDING`,
//! e l'applicazione li prende quando e' pronta (`take`). Quelli aperti dopo
//! svegliano l'applicazione con `UserEvent::Finder`.

use std::path::PathBuf;
use std::sync::Mutex;

use objc2::ffi;
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::sel;
use objc2_foundation::{NSArray, NSURL};
use winit::event_loop::EventLoopProxy;

use crate::app::UserEvent;

/// - (void)application:(NSApplication *)app openURLs:(NSArray<NSURL *> *)urls
type OpenUrls = unsafe extern "C-unwind" fn(&AnyObject, Sel, &AnyObject, &NSArray<NSURL>);

static PENDING: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static PROXY: Mutex<Option<EventLoopProxy<UserEvent>>> = Mutex::new(None);

/// Insegna al delegato di winit ad aprire i file. Va chiamata dopo aver
/// creato l'EventLoop (che installa il delegato) e prima di farlo partire.
pub fn install(proxy: EventLoopProxy<UserEvent>) {
    *PROXY.lock().unwrap_or_else(|e| e.into_inner()) = Some(proxy);
    let Some(class) = AnyClass::get(c"WinitApplicationDelegate") else {
        eprintln!("delegato di winit non trovato: i file del Finder non si aprono");
        return;
    };
    // SAFETY: la firma della funzione e' quella del metodo (OpenUrls), cioe'
    // "v@:@@"; se la classe l'avesse gia', class_addMethod non tocca niente
    let added = unsafe {
        ffi::class_addMethod(
            class as *const AnyClass as *mut AnyClass,
            sel!(application:openURLs:),
            std::mem::transmute::<OpenUrls, Imp>(open_urls),
            c"v@:@@".as_ptr(),
        )
    };
    if !added.as_bool() {
        eprintln!("application:openURLs: non aggiunto al delegato di winit");
    }
}

/// I file arrivati dal Finder e non ancora aperti.
pub fn take() -> Vec<PathBuf> {
    std::mem::take(&mut *PENDING.lock().unwrap_or_else(|e| e.into_inner()))
}

unsafe extern "C-unwind" fn open_urls(_this: &AnyObject, _cmd: Sel, _app: &AnyObject, urls: &NSArray<NSURL>) {
    let paths: Vec<PathBuf> = urls.iter().filter_map(|url| url.path()).map(|p| PathBuf::from(p.to_string())).collect();
    if paths.is_empty() {
        return;
    }
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).extend(paths);
    if let Some(proxy) = &*PROXY.lock().unwrap_or_else(|e| e.into_inner()) {
        let _ = proxy.send_event(UserEvent::Finder);
    }
}
