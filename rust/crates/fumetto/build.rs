//! Su Windows l'eseguibile porta con se' l'icona, il nome che mostrano
//! Esplora file e Gestione attivita', e un manifest:
//!   - percorsi oltre i 260 caratteri (manga/AUTORE/Titolo/capitolo/...),
//!     anche per pdfium, che non li allunga da se' come fa Rust;
//!   - DPI per monitor, dichiarato come chiede Microsoft (winit lo imposta
//!     anche da solo, ma solo dopo l'avvio);
//!   - i controlli comuni moderni, per le finestre di sistema.
//!
//! L'icona si rifa' con `risorse/fai_icona.py`.

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="NicoReader" version="0.0.0.0"/>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
        processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
</assembly>
"#;

fn main() {
    println!("cargo:rerun-if-changed=risorse/fumetto.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("risorse/fumetto.ico")
        .set("FileDescription", "NicoReader")
        .set("ProductName", "NicoReader")
        .set_manifest(MANIFEST);
    if let Err(e) = res.compile() {
        panic!("risorse di Windows (icona, manifest): {e}");
    }
}
