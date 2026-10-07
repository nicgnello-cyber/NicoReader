//! Le preferenze che valgono per tutti i volumi, in `impostazioni.json`
//! accanto ai progressi. Come i progressi: si scrive un temporaneo e poi lo si
//! rinomina, e un file illeggibile vale come le preferenze di partenza.

use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// La barra in alto, con i tasti.
    pub hud: bool,
    /// Le cartelle della libreria.
    pub library: Vec<PathBuf>,
    /// I server della libreria (Komga, Kavita), con nome e password.
    pub servers: Vec<crate::remote::Server>,
    /// Riconoscere i webtoon (strisce molto piu' alte che larghe) all'apertura
    /// e leggerli a nastro.
    pub webtoon: bool,
    /// Rifilare i margini uniformi delle scansioni (C).
    pub trim: bool,
    /// Migliorare con Real-ESRGAN le pagine mostrate piu' grandi dei loro
    /// pixel (U). Serve l'ingranditore, scaricato al primo uso.
    pub upscale: bool,
    /// Secondi fra una pagina e la successiva nella presentazione (S).
    pub slideshow: u32,
    /// Luminosita', contrasto e gamma delle pagine, da -100 a 100 (0: come
    /// sono). Li applica la scheda video a ogni fotogramma.
    pub brightness: i32,
    pub contrast: i32,
    pub gamma: i32,
    /// La lente: quante volte ingrandisce, e il suo raggio in punti.
    pub lens_zoom: f32,
    pub lens_size: f32,
    /// I tasti scelti da chi legge, per nome del comando ("lente": ["Z"]);
    /// i comandi che non ci sono hanno i tasti di serie.
    pub keys: BTreeMap<String, Vec<String>>,
    /// Chiedere a GitHub, una volta al giorno, se c'e' una versione nuova.
    pub check_updates: bool,
    /// Quando lo si e' chiesto l'ultima volta (secondi dal 1970).
    pub update_checked: u64,
    /// L'ultima versione nuova gia' proposta: non la si ripropone.
    pub update_offered: String,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            hud: true,
            library: Vec::new(),
            servers: Vec::new(),
            webtoon: true,
            trim: false,
            upscale: false,
            slideshow: 5,
            brightness: 0,
            contrast: 0,
            gamma: 0,
            lens_zoom: 2.5,
            lens_size: 150.0,
            keys: BTreeMap::new(),
            check_updates: true,
            update_checked: 0,
            update_offered: String::new(),
        }
    }
}

impl Settings {
    pub fn load(path: &PathBuf) -> Settings {
        let mut s: Settings =
            std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        // le password dei server stanno nel portachiavi (vedi keychain)
        for server in &mut s.servers {
            if server.password.is_empty()
                && !server.user.is_empty()
                && let Some(password) = crate::keychain::load(&server.url, &server.user)
            {
                server.password = password;
            }
        }
        s
    }

    pub fn save(&self, path: &PathBuf) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // nel file le password che il portachiavi non ha preso, e basta
        let mut written = self.clone();
        for server in &mut written.servers {
            if !server.password.is_empty() && crate::keychain::store(&server.url, &server.user, &server.password) {
                server.password.clear();
            }
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&written).map_err(io::Error::other)?)?;
        // con una password dentro, il file e' solo di chi lo possiede
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(&tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn si_ricorda_la_barra_nascosta() {
        let path = std::env::temp_dir().join(format!("fumetto-impostazioni-{}", std::process::id())).join("i.json");
        assert!(Settings::load(&path).hud, "di partenza la barra c'e'");
        Settings { hud: false, ..Settings::default() }.save(&path).unwrap();
        assert!(!Settings::load(&path).hud);
        std::fs::write(&path, b"rovinato").unwrap();
        assert!(Settings::load(&path).hud, "file illeggibile: preferenze di partenza");
        // un file di una versione precedente, senza le voci nuove: valgono quelle di partenza
        std::fs::write(&path, br#"{"hud": false}"#).unwrap();
        let old = Settings::load(&path);
        assert!(!old.hud && old.webtoon && !old.upscale && old.slideshow == 5);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
