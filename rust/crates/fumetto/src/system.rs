//! Cio' che si chiede al sistema: mostrare un file nella sua cartella, e
//! spostarlo nel cestino (mai cancellarlo: dal cestino si recupera).

use std::path::Path;

/// Apre il gestore dei file sulla cartella del volume, con il volume scelto.
pub fn reveal(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let spawned = {
        use std::os::windows::process::CommandExt;
        // "/select," vuole il percorso attaccato, fra virgolette
        std::process::Command::new("explorer").raw_arg(format!("/select,\"{}\"", path.display())).spawn()
    };
    #[cfg(target_os = "macos")]
    let spawned = std::process::Command::new("open").arg("-R").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let spawned = std::process::Command::new("xdg-open").arg(path.parent().unwrap_or(path)).spawn();
    spawned.map(|_| ()).map_err(|e| e.to_string())
}

/// Apre un indirizzo nel browser di sistema.
pub fn open_url(url: &str) -> Result<(), String> {
    #[cfg(windows)]
    let spawned = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn();
    #[cfg(target_os = "macos")]
    let spawned = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let spawned = std::process::Command::new("xdg-open").arg(url).spawn();
    spawned.map(|_| ()).map_err(|e| e.to_string())
}

/// Sposta un file o una cartella nel cestino.
#[cfg(windows)]
pub fn trash(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::{
        FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, SHFILEOPSTRUCTW, SHFileOperationW,
    };
    let abs = std::path::absolute(path).map_err(|e| e.to_string())?;
    // l'elenco dei file finisce con due zeri
    let from: Vec<u16> = abs.as_os_str().encode_wide().chain([0, 0]).collect();
    let mut op = SHFILEOPSTRUCTW {
        hwnd: std::ptr::null_mut(),
        wFunc: FO_DELETE,
        pFrom: from.as_ptr(),
        pTo: std::ptr::null(),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI) as u16,
        fAnyOperationsAborted: 0,
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: std::ptr::null(),
    };
    // SAFETY: `from` vive fino alla fine della chiamata ed e' terminato da due zeri.
    let code = unsafe { SHFileOperationW(&mut op) };
    if code != 0 || op.fAnyOperationsAborted != 0 {
        return Err(format!("codice {code}"));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn trash(path: &Path) -> Result<(), String> {
    trash::delete(path).map_err(|e| e.to_string())
}
