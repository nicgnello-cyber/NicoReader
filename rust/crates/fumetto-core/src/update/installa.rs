//! Installare da se' la versione nuova: si scarica il file giusto della
//! Release, lo si controlla contro l'impronta SHA-256 che da' GitHub, e lo si
//! installa al posto di questa versione. Poi NicoReader si chiude e si riapre.
//!
//! Si puo' solo dove si sa come e' stato installato e si puo' scrivere:
//! - Windows, con l'installer (accanto all'eseguibile c'e' il disinstallatore
//!   unins000.exe): si lancia l'installer nuovo, che installa senza domande
//!   sopra a questo e alla fine riapre NicoReader;
//! - macOS: si sostituisce NicoReader.app con quella del dmg nuovo;
//! - Linux, la cartella del tar.gz (quella con installa.sh): si sostituisce
//!   la cartella;
//! - Linux, l'AppImage: si sostituisce il file .AppImage (il programma gira
//!   da una cartella montata in sola lettura, che non si tocca).
//!
//! Altrimenti (lo zip di Windows, il .deb installato in /opt, un'app in una
//! cartella in sola lettura) si apre la pagina della Release, come prima.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

use super::{Asset, Release};
use crate::lingua::t;

/// Come si installa la versione nuova qui.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    /// L'installer di Windows, sopra a questa installazione.
    Windows { asset: Asset },
    /// Il dmg: la sua NicoReader.app al posto di `bundle`.
    Mac { asset: Asset, bundle: PathBuf },
    /// Il tar.gz di Linux: la sua cartella al posto di `dir`.
    Folder { asset: Asset, dir: PathBuf },
    /// L'AppImage nuovo al posto di `file`.
    AppImage { asset: Asset, file: PathBuf },
}

impl Plan {
    pub fn asset(&self) -> &Asset {
        match self {
            Plan::Windows { asset }
            | Plan::Mac { asset, .. }
            | Plan::Folder { asset, .. }
            | Plan::AppImage { asset, .. } => asset,
        }
    }
}

/// Il nome del file della Release per questo sistema.
fn wanted() -> String {
    let arch = if cfg!(target_arch = "aarch64") { "arm64" } else { "x64" };
    if cfg!(windows) {
        format!("NicoReader-windows-{arch}-setup.exe")
    } else if cfg!(target_os = "macos") {
        "NicoReader-macos.dmg".into()
    } else {
        format!("NicoReader-linux-{arch}.tar.gz")
    }
}

/// Come installare `release` al posto di questa versione; `None` se qui non
/// si puo' (allora si apre la pagina della Release).
pub fn plan(release: &Release) -> Option<Plan> {
    // dentro un AppImage: il suo file lo dice lui, in APPIMAGE
    if cfg!(target_os = "linux")
        && let Some(file) = std::env::var_os("APPIMAGE")
    {
        let arch = if cfg!(target_arch = "aarch64") { "arm64" } else { "x64" };
        return plan_appimage(release, Path::new(&file), &format!("NicoReader-linux-{arch}.AppImage"));
    }
    let exe = std::env::current_exe().ok()?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    plan_for(release, &exe, &wanted())
}

fn plan_for(release: &Release, exe: &Path, wanted: &str) -> Option<Plan> {
    let asset = release.assets.iter().find(|a| a.name == wanted)?.clone();
    let dir = exe.parent()?;
    if wanted.ends_with("-setup.exe") {
        let installed = std::fs::read_dir(dir).ok()?.flatten().any(|e| {
            let name = e.file_name().to_string_lossy().to_lowercase();
            name.starts_with("unins") && name.ends_with(".exe")
        });
        return installed.then_some(Plan::Windows { asset });
    }
    if wanted.ends_with(".dmg") {
        // .../NicoReader.app/Contents/MacOS/nicoreader
        let bundle = dir.parent()?.parent()?;
        let ok = bundle.extension().is_some_and(|e| e == "app") && writable(bundle.parent()?);
        return ok.then(|| Plan::Mac { asset, bundle: bundle.to_owned() });
    }
    let unpacked = dir.join("installa.sh").is_file() && dir.join("nicoreader.desktop").is_file();
    let ok = unpacked && writable(dir) && writable(dir.parent()?);
    ok.then(|| Plan::Folder { asset, dir: dir.to_owned() })
}

/// L'AppImage si sostituisce se si puo' scrivere accanto a lui.
fn plan_appimage(release: &Release, file: &Path, wanted: &str) -> Option<Plan> {
    let asset = release.assets.iter().find(|a| a.name == wanted)?.clone();
    let ok = file.is_file() && writable(file.parent()?);
    ok.then(|| Plan::AppImage { asset, file: file.to_owned() })
}

/// Si puo' scrivere nella cartella (lo si prova davvero: i permessi da soli
/// non dicono tutto, per esempio su un disco in sola lettura).
fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".nicoreader-prova-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// La cartella degli aggiornamenti scaricati.
fn downloads() -> PathBuf {
    std::env::temp_dir().join("NicoReader-aggiornamento")
}

/// Toglie gli aggiornamenti scaricati le volte prima (gia' installati, o
/// lasciati a meta').
pub fn cleanup() {
    let _ = std::fs::remove_dir_all(downloads());
}

/// Scarica il file nella cartella temporanea e lo controlla: misura e
/// impronta devono essere quelle che dice GitHub. Puo' metterci minuti: da un
/// thread a parte.
pub fn download(asset: &Asset) -> Result<PathBuf, String> {
    let dir = downloads();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join(&asset.name);
    let mut curl = Command::new("curl");
    curl.args(["--location", "--fail", "--silent", "--show-error", "--globoff"])
        .args(["--proto", "=https", "--proto-redir", "=https"])
        .args(["--connect-timeout", "15", "--speed-limit", "1024", "--speed-time", "60"])
        .args(["--user-agent", concat!("NicoReader/", env!("CARGO_PKG_VERSION"))])
        .arg("--output")
        .arg(&file)
        .arg(&asset.url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    crate::upscale::hide_window(&mut curl);
    let out = curl.output().map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        let _ = std::fs::remove_file(&file);
        return Err(String::from_utf8_lossy(&out.stderr).trim().trim_start_matches("curl: ").to_owned());
    }
    if let Err(e) = check(&file, asset) {
        let _ = std::fs::remove_file(&file);
        return Err(e);
    }
    Ok(file)
}

/// Il file e' quello che dice GitHub: stessa misura, stessa impronta.
fn check(file: &Path, asset: &Asset) -> Result<(), String> {
    let damaged =
        || t("il file scaricato non e' quello della Release", "the downloaded file isn't the Release's").to_owned();
    let mut f = std::fs::File::open(file).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buf = vec![0; 1 << 16];
    let mut size = 0u64;
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        size += n as u64;
        hash.update(&buf[..n]);
    }
    if size != asset.size || hash.finalize()[..] != asset.sha256[..] {
        return Err(damaged());
    }
    Ok(())
}

/// Installa il file scaricato. Se torna Ok, NicoReader deve chiudersi subito
/// (dopo aver salvato): l'installer di Windows aspetta di poter sostituire
/// l'eseguibile, e su macOS e Linux un piccolo comando aspetta che si chiuda
/// per riaprirlo, gia' aggiornato.
pub fn install(plan: &Plan, file: &Path) -> Result<(), String> {
    match plan {
        Plan::Windows { .. } => {
            // senza domande ma con la sua finestra di avanzamento; "riapri":
            // alla fine riapre NicoReader (vedi installer/nicoreader.iss)
            Command::new(file)
                .args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-", "/NOCANCEL", "/RIAPRI=si"])
                .spawn()
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
        Plan::Mac { bundle, .. } => {
            install_mac(file, bundle)?;
            relaunch_after_exit(&["open", "-n", &bundle.to_string_lossy()])
        }
        Plan::Folder { dir, .. } => {
            install_folder(file, dir)?;
            relaunch_after_exit(&[&dir.join("nicoreader").to_string_lossy()])
        }
        Plan::AppImage { file: target, .. } => {
            install_appimage(file, target)?;
            relaunch_after_exit(&[&target.to_string_lossy()])
        }
    }
}

/// Il file nuovo accanto al vecchio (la cartella temporanea puo' stare su un
/// altro disco), eseguibile, poi al suo posto in un colpo solo: il programma
/// che gira ha ancora aperto il vecchio, che sparisce quando si chiude.
fn install_appimage(new: &Path, target: &Path) -> Result<(), String> {
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let next = target.with_file_name(format!(".{name}.nuovo"));
    let placed =
        std::fs::copy(new, &next).and_then(|_| make_executable(&next)).and_then(|_| std::fs::rename(&next, target));
    if placed.is_err() {
        let _ = std::fs::remove_file(&next);
    }
    placed.map_err(|e| e.to_string())
}

#[cfg(unix)]
fn make_executable(file: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Esegue un comando e aspetta; l'errore e' quello che scrive.
fn run(cmd: &mut Command) -> Result<(), String> {
    let out = cmd.stdin(Stdio::null()).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
    Err(if err.is_empty() { format!("{:?}: {}", cmd.get_program(), out.status) } else { err })
}

/// Mette `new` al posto di `old`: il vecchio si sposta da parte, il nuovo va
/// al suo posto, il vecchio si cancella. Se lo scambio non riesce, il vecchio
/// torna dov'era.
fn swap(old: &Path, new: &Path) -> Result<(), String> {
    let name = old.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let aside = old.with_file_name(format!(".{name}.vecchio"));
    let _ = std::fs::remove_dir_all(&aside);
    std::fs::rename(old, &aside).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(new, old) {
        let _ = std::fs::rename(&aside, old);
        return Err(e.to_string());
    }
    // il programma che gira ha ancora aperti i suoi file: su macOS e Linux
    // cancellarli non lo disturba
    let _ = std::fs::remove_dir_all(&aside);
    Ok(())
}

/// Riapre `target` appena questo processo si e' chiuso: un `sh` che aspetta.
fn relaunch_after_exit(open: &[&str]) -> Result<(), String> {
    let script = r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; shift; exec "$@""#;
    Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .arg("sh")
        .arg(std::process::id().to_string())
        .args(open)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn install_mac(dmg: &Path, bundle: &Path) -> Result<(), String> {
    let mount = std::env::temp_dir().join(format!("NicoReader-dmg-{}", std::process::id()));
    std::fs::create_dir_all(&mount).map_err(|e| e.to_string())?;
    run(Command::new("hdiutil")
        .args(["attach", "-nobrowse", "-noautoopen", "-readonly", "-quiet", "-mountpoint"])
        .arg(&mount)
        .arg(dmg))?;
    let name = bundle.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let new = bundle.with_file_name(format!(".{name}.nuovo"));
    let _ = std::fs::remove_dir_all(&new);
    let copied = run(Command::new("ditto").arg(mount.join("NicoReader.app")).arg(&new));
    let _ = run(Command::new("hdiutil").args(["detach", "-quiet"]).arg(&mount));
    let _ = std::fs::remove_dir(&mount);
    copied?;
    // scaricato con curl non ha la quarantena; per sicurezza, se ce l'avesse
    let _ = run(Command::new("xattr").args(["-dr", "com.apple.quarantine"]).arg(&new));
    let swapped = swap(bundle, &new);
    if swapped.is_err() {
        let _ = std::fs::remove_dir_all(&new);
    }
    swapped
}

fn install_folder(tgz: &Path, dir: &Path) -> Result<(), String> {
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let work = dir.with_file_name(format!(".{name}.nuovo"));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let unpacked = run(Command::new("tar").arg("-xzf").arg(tgz).arg("-C").arg(&work)).and_then(|_| {
        let new = work.join("NicoReader");
        if new.join("nicoreader").is_file() {
            Ok(new)
        } else {
            Err(t("nel tar.gz manca nicoreader", "nicoreader is missing from the tar.gz").into())
        }
    });
    let result = unpacked.and_then(|new| swap(dir, &new));
    let _ = std::fs::remove_dir_all(&work);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str, bytes: &[u8]) -> Asset {
        let sha256 = Sha256::digest(bytes).into();
        Asset { name: name.into(), url: format!("https://x/{name}"), size: bytes.len() as u64, sha256 }
    }

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fumetto-agg-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn come_si_installa() {
        let release = Release {
            assets: vec![
                asset("NicoReader-windows-x64-setup.exe", b"w"),
                asset("NicoReader-macos.dmg", b"m"),
                asset("NicoReader-linux-x64.tar.gz", b"l"),
            ],
            ..Release::default()
        };
        let root = temp("piano");
        // Windows: con il disinstallatore accanto si', dallo zip no
        let win = root.join("Programs/NicoReader");
        std::fs::create_dir_all(&win).unwrap();
        let exe = win.join("NicoReader.exe");
        assert_eq!(plan_for(&release, &exe, "NicoReader-windows-x64-setup.exe"), None, "lo zip");
        std::fs::write(win.join("unins000.exe"), b"").unwrap();
        assert!(matches!(plan_for(&release, &exe, "NicoReader-windows-x64-setup.exe"), Some(Plan::Windows { .. })));
        assert_eq!(plan_for(&release, &exe, "NicoReader-windows-arm64-setup.exe"), None, "file che manca");
        // macOS: dentro un .app
        let app = root.join("Applications/NicoReader.app/Contents/MacOS");
        std::fs::create_dir_all(&app).unwrap();
        let Some(Plan::Mac { bundle, .. }) = plan_for(&release, &app.join("nicoreader"), "NicoReader-macos.dmg") else {
            panic!("doveva essere un .app")
        };
        assert_eq!(bundle, root.join("Applications/NicoReader.app"));
        assert_eq!(plan_for(&release, &root.join("bin/nicoreader"), "NicoReader-macos.dmg"), None);
        // Linux: la cartella del tar.gz si', /opt (il .deb) no
        let tgz = root.join("NicoReader");
        std::fs::create_dir_all(&tgz).unwrap();
        let exe = tgz.join("nicoreader");
        assert_eq!(plan_for(&release, &exe, "NicoReader-linux-x64.tar.gz"), None, "senza installa.sh");
        std::fs::write(tgz.join("installa.sh"), b"").unwrap();
        std::fs::write(tgz.join("nicoreader.desktop"), b"").unwrap();
        assert_eq!(
            plan_for(&release, &exe, "NicoReader-linux-x64.tar.gz"),
            Some(Plan::Folder { asset: release.assets[2].clone(), dir: tgz })
        );
        // l'AppImage: dove si puo' scrivere accanto a lui
        let mut with_appimage = release.clone();
        with_appimage.assets.push(asset("NicoReader-linux-x64.AppImage", b"a"));
        let image = root.join("NicoReader-linux-x64.AppImage");
        assert_eq!(plan_appimage(&with_appimage, &image, "NicoReader-linux-x64.AppImage"), None, "il file non c'e'");
        std::fs::write(&image, b"vecchio").unwrap();
        assert_eq!(plan_appimage(&release, &image, "NicoReader-linux-x64.AppImage"), None, "la Release non ce l'ha");
        let Some(Plan::AppImage { file, .. }) = plan_appimage(&with_appimage, &image, "NicoReader-linux-x64.AppImage")
        else {
            panic!("doveva essere l'AppImage")
        };
        assert_eq!(file, image);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn l_appimage_si_sostituisce() {
        let root = temp("appimage");
        let (old, new) = (root.join("NicoReader.AppImage"), root.join("scaricato"));
        std::fs::write(&old, b"0.4.1").unwrap();
        std::fs::write(&new, b"0.4.2").unwrap();
        install_appimage(&new, &old).unwrap();
        assert_eq!(std::fs::read(&old).unwrap(), b"0.4.2");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&old).unwrap().permissions().mode() & 0o777, 0o755, "eseguibile");
        }
        assert!(!root.join(".NicoReader.AppImage.nuovo").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn il_file_scaricato_si_controlla() {
        let root = temp("controllo");
        let file = root.join("f");
        std::fs::write(&file, b"NicoReader").unwrap();
        assert!(check(&file, &asset("f", b"NicoReader")).is_ok());
        assert!(check(&file, &asset("f", b"NicoReadeR")).is_err(), "impronta diversa");
        let mut short = asset("f", b"NicoReader");
        short.size = 3;
        assert!(check(&file, &short).is_err(), "misura diversa");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn lo_scambio_delle_cartelle() {
        let root = temp("scambio");
        let (old, new) = (root.join("NicoReader"), root.join("nuova"));
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(old.join("v"), b"0.3").unwrap();
        std::fs::write(new.join("v"), b"0.4").unwrap();
        swap(&old, &new).unwrap();
        assert_eq!(std::fs::read(old.join("v")).unwrap(), b"0.4");
        assert!(!new.exists() && !root.join(".NicoReader.vecchio").exists());
        // la nuova che non c'e': la vecchia resta al suo posto
        assert!(swap(&old, &root.join("manca")).is_err());
        assert_eq!(std::fs::read(old.join("v")).unwrap(), b"0.4");
        let _ = std::fs::remove_dir_all(root);
    }

    /// Con la Release vera e la cartella del tar.gz: scarica, controlla,
    /// sostituisce. FUMETTO_PROVA_AGGIORNAMENTO=1 (va in rete).
    #[test]
    #[ignore]
    fn aggiornamento_vero_su_linux() {
        if std::env::var_os("FUMETTO_PROVA_AGGIORNAMENTO").is_none() || !cfg!(target_os = "linux") {
            return;
        }
        let release = super::super::latest().unwrap();
        let root = temp("vero");
        let dir = root.join("NicoReader");
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["installa.sh", "nicoreader.desktop", "nicoreader"] {
            std::fs::write(dir.join(f), b"vecchio").unwrap();
        }
        let plan = plan_for(&release, &dir.join("nicoreader"), &wanted()).expect("la cartella del tar.gz");
        let file = download(plan.asset()).unwrap();
        install_folder(&file, &dir).unwrap();
        let exe = std::fs::read(dir.join("nicoreader")).unwrap();
        let version = format!("NicoReader/{}", release.version);
        assert!(exe.windows(version.len()).any(|w| w == version.as_bytes()), "e' la versione {}", release.version);
        assert!(dir.join("libpdfium.so").is_file());
        let _ = std::fs::remove_dir_all(root);
    }
}
