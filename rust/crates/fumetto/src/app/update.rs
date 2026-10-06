//! L'avviso di una versione nuova: una volta al giorno al massimo si chiede a
//! GitHub qual e' l'ultima Release (in sottofondo, senza fermare niente) e, se
//! e' piu' nuova di questa, si propone una volta sola di aprirne la pagina.
//! Si spegne dalle impostazioni.

use super::*;
use fumetto_core::update;

/// Ogni quanto, al massimo, si chiede a GitHub.
const CHECK_EVERY: u64 = 24 * 60 * 60;

impl App {
    /// Se e' il momento, chiede a GitHub l'ultima versione; la risposta torna
    /// come `UserEvent::Update`. Senza rete non torna niente, e si riprova
    /// alla prossima apertura.
    pub(super) fn check_update(&mut self) {
        // la prova automatica non esce sulla rete, e senza impostazioni da
        // salvare chiederebbe a ogni avvio
        if self.script.is_some() || self.settings_path.is_none() || !self.settings.check_updates {
            return;
        }
        if unix_now().saturating_sub(self.settings.update_checked) < CHECK_EVERY {
            return;
        }
        let proxy = self.proxy.clone();
        let _ = std::thread::Builder::new().name("aggiornamenti".into()).spawn(move || match update::latest() {
            Ok(release) => {
                let _ = proxy.send_event(UserEvent::Update(release));
            }
            Err(e) => eprintln!("versione nuova non controllata: {e}"),
        });
    }

    /// La risposta di GitHub: se la versione e' nuova e non la si e' ancora
    /// proposta, la si propone appena non c'e' altro da mostrare.
    pub(super) fn update_arrived(&mut self, release: Release) {
        self.settings.update_checked = unix_now();
        if update::newer(&release.version, env!("CARGO_PKG_VERSION")) && self.settings.update_offered != release.version
        {
            self.update_offer = Some(release);
        }
        self.save_settings();
        self.show_pending();
    }

    /// La proposta, nella finestra di sistema; da qui non si ripropone piu'
    /// questa versione, qualunque sia la risposta.
    pub(super) fn offer_update(&mut self, release: Release) {
        let Some(window) = &self.window else { return };
        dialog::offer_update(window, &release, self.proxy.clone());
        self.dialog = true;
        self.settings.update_offered = release.version;
        self.save_settings();
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}
