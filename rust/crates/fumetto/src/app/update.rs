//! L'avviso di una versione nuova: una volta al giorno al massimo si chiede a
//! GitHub qual e' l'ultima Release (in sottofondo, senza fermare niente) e, se
//! e' piu' nuova di questa, la si propone una volta sola: installarla adesso
//! (dove si puo', vedi update::installa), leggerne le novita', o lasciar
//! stare. Si spegne dalle impostazioni.

use super::*;
use fumetto_core::update::{self, installa};

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
        // l'installer scaricato la volta prima non serve piu'
        std::thread::spawn(installa::cleanup);
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
        let can_install = installa::plan(&release).is_some();
        dialog::offer_update(window, &release, can_install, self.proxy.clone());
        self.dialog = true;
        self.settings.update_offered = release.version;
        self.save_settings();
    }

    /// Si', installala: si scarica in sottofondo (si continua a leggere), poi
    /// si installa e NicoReader si riapre.
    pub(super) fn install_update(&mut self, release: Release) {
        self.dialog = false;
        if self.updating {
            return;
        }
        let Some(plan) = installa::plan(&release) else { return self.open_release_page(&release.url) };
        self.updating = true;
        let note = if fumetto_core::lingua::italian() {
            format!("Scarico NicoReader {}\u{2026}", release.version)
        } else {
            format!("Downloading NicoReader {}\u{2026}", release.version)
        };
        self.ui.toast(note, Instant::now());
        self.changed();
        let proxy = self.proxy.clone();
        std::thread::Builder::new()
            .name("aggiornamento".into())
            .spawn(move || {
                let file = installa::download(plan.asset());
                let _ = proxy.send_event(UserEvent::UpdateDownloaded(Box::new(plan), release, file));
            })
            .expect("thread dell'aggiornamento");
    }

    /// Scaricata e controllata: si salva tutto, si installa e ci si chiude.
    /// Se non va, lo si dice e si propone la pagina della Release.
    pub(super) fn update_downloaded(
        &mut self, event_loop: &ActiveEventLoop, plan: installa::Plan, release: Release, file: Result<PathBuf, String>,
    ) {
        self.updating = false;
        self.save_progress();
        fumetto_core::remote::sync::flush();
        match file.and_then(|f| installa::install(&plan, &f)) {
            Ok(()) => event_loop.exit(),
            Err(e) => {
                let head = if fumetto_core::lingua::italian() {
                    format!("Impossibile installare NicoReader {}.", release.version)
                } else {
                    format!("Can't install NicoReader {}.", release.version)
                };
                let page = t("La si scarica dalla sua pagina:", "It can be downloaded from its page:");
                self.notify(format!("{head}\n\n{e}\n\n{page}\n{}", release.url));
            }
        }
    }

    pub(super) fn open_release_page(&mut self, url: &str) {
        self.dialog = false;
        if let Err(e) = system::open_url(url) {
            self.notify(format!("{}\n\n{url}\n\n{e}", t("Impossibile aprire la pagina.", "Can't open the page.")));
        }
    }

    /// Al primo avvio di una versione nuova: si ricorda, e a finestra aperta
    /// lo si dice.
    pub(super) fn note_version(&mut self) {
        let this = env!("CARGO_PKG_VERSION");
        if self.settings.last_version == this || self.settings_path.is_none() {
            return;
        }
        self.just_updated = update::newer(this, &self.settings.last_version);
        self.settings.last_version = this.to_owned();
        self.save_settings();
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}
