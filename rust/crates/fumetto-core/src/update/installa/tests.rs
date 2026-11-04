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
