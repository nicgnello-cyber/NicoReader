//! Se c'e' una versione nuova di NicoReader: si chiede a GitHub qual e'
//! l'ultima Release pubblicata e la si confronta con questa.
//!
//! Come per l'ingranditore AI, la richiesta la fa curl (c'e' in Windows 10 e
//! 11, in macOS e in ogni Linux): niente librerie di rete nell'eseguibile per
//! una domanda al giorno. A GitHub arriva solo la richiesta della pagina
//! pubblica delle Release, nient'altro.

use std::process::{Command, Stdio};

/// L'ultima Release pubblicata (le bozze e le "pre-release" non contano).
pub const LATEST: &str = "https://api.github.com/repos/nicgnello-cyber/NicoReader/releases/latest";

/// Le pagine delle Release: solo queste si aprono nel browser.
const PAGES: &str = "https://github.com/nicgnello-cyber/NicoReader/releases/";

/// Una Release: la versione ("0.2.0") e la sua pagina, da aprire nel browser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub url: String,
}

/// L'ultima Release, chiesta a GitHub. Puo' metterci qualche secondo (o
/// non riuscire, senza rete): va chiamata da un thread a parte.
pub fn latest() -> Result<Release, String> {
    let mut curl = Command::new("curl");
    curl.args(["--location", "--fail", "--silent", "--show-error", "--max-time", "15"])
        .args(["--header", "Accept: application/vnd.github+json"])
        .args(["--user-agent", concat!("NicoReader/", env!("CARGO_PKG_VERSION"))])
        .arg(LATEST)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::upscale::hide_window(&mut curl);
    let out = curl.output().map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }
    parse(&out.stdout).ok_or_else(|| "risposta di GitHub senza versione".to_owned())
}

/// La Release dalla risposta di GitHub (tag "v0.2.0" o "0.2.0").
pub fn parse(json: &[u8]) -> Option<Release> {
    let v: serde_json::Value = serde_json::from_slice(json).ok()?;
    if v["draft"].as_bool() == Some(true) || v["prerelease"].as_bool() == Some(true) {
        return None;
    }
    let tag = v["tag_name"].as_str()?;
    let version = tag.strip_prefix('v').unwrap_or(tag).to_owned();
    numbers(&version)?;
    let url = v["html_url"].as_str().filter(|u| u.starts_with(PAGES))?.to_owned();
    Some(Release { version, url })
}

/// `candidate` e' piu' nuova di `current`? Si confrontano i numeri
/// (0.10.0 viene dopo 0.9.0); una versione che non si legge non e' nuova.
pub fn newer(candidate: &str, current: &str) -> bool {
    match (numbers(candidate), numbers(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// "0.2.0" -> [0, 2, 0]; "1.2" vale 1.2.0.
fn numbers(version: &str) -> Option<[u32; 3]> {
    let mut out = [0; 3];
    let mut parts = version.trim().split('.');
    for slot in &mut out {
        match parts.next() {
            Some(p) => *slot = p.parse().ok()?,
            None => break,
        }
    }
    parts.next().is_none().then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_risposta_di_github() {
        let json =
            br#"{"tag_name": "v0.2.0", "html_url": "https://github.com/nicgnello-cyber/NicoReader/releases/tag/v0.2.0",
                        "draft": false, "prerelease": false, "assets": []}"#;
        assert_eq!(
            parse(json),
            Some(Release {
                version: "0.2.0".into(),
                url: "https://github.com/nicgnello-cyber/NicoReader/releases/tag/v0.2.0".into()
            })
        );
        let page = |tag: &str| format!(r#"{{"tag_name": "{tag}", "html_url": "{PAGES}tag/{tag}"}}"#).into_bytes();
        assert_eq!(parse(&page("0.3.0")).map(|r| r.version), Some("0.3.0".into()), "anche senza la v");
        assert_eq!(parse(br#"{"tag_name": "v0.3.0", "html_url": "https://example.com/x"}"#), None, "solo GitHub");
        assert_eq!(parse(br#"{"tag_name": "v0.3.0", "html_url": "x", "prerelease": true}"#), None);
        assert_eq!(parse(br#"{"tag_name": "ultima", "html_url": "x"}"#), None);
        assert_eq!(parse(br#"{"message": "Not Found"}"#), None);
        assert_eq!(parse(b"<html>"), None);
    }

    #[test]
    fn quale_e_piu_nuova() {
        assert!(newer("0.2.0", "0.1.1"));
        assert!(newer("0.10.0", "0.9.3"), "i numeri, non le lettere");
        assert!(newer("1.0", "0.9.9"));
        assert!(!newer("0.1.1", "0.1.1"));
        assert!(!newer("0.1.0", "0.1.1"));
        assert!(!newer("0.2.0-beta", "0.1.1"), "non si legge: non e' nuova");
        assert!(!newer("1.2.3.4", "0.1.0"));
    }
}
