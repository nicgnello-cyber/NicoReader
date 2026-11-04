//! Le righe di ogni sezione del foglio.

use super::*;

impl Prefs {
    pub(super) fn rows(&self, settings: &Settings, keys: &Keymap) -> Vec<Row> {
        match self.tab {
            Tab::Image => image_rows(),
            Tab::Reading => reading_rows(settings),
            Tab::Lens => lens_rows(),
            Tab::Keys => key_rows(keys),
        }
    }
}

/// Un interruttore: il clic lo gira (`pref` con il valore nuovo).
fn toggle(label: &'static str, on: bool, pref: fn(bool) -> Pref) -> Row {
    Row::Toggle { label, on, pref: pref(!on) }
}

fn image_rows() -> Vec<Row> {
    vec![
        Row::Slider { label: t("Luminosità", "Brightness"), which: Slider::Brightness },
        Row::Slider { label: t("Contrasto", "Contrast"), which: Slider::Contrast },
        Row::Slider { label: "Gamma", which: Slider::Gamma },
        Row::Button { label: t("Ripristina", "Reset"), pref: Pref::ResetImage },
        Row::Note(t(
            "Valgono per tutte le pagine e si vedono subito. Si applicano dopo il rimpicciolimento in luce \
             lineare, che resta com'è.",
            "They apply to every page, live, after the linear-light resampling, which stays as it is.",
        )),
    ]
}

fn reading_rows(settings: &Settings) -> Vec<Row> {
    let secs = settings.slideshow.max(1);
    vec![
        toggle(t("Riconosci i webtoon", "Detect webtoons"), settings.webtoon, Pref::Webtoon),
        Row::Note(t(
            "Un volume mai aperto con le pagine a strisce si apre a nastro.",
            "A never-opened volume made of strips opens as a strip.",
        )),
        toggle(t("Rifila i margini", "Trim margins"), settings.trim, Pref::Trim),
        toggle(t("Migliora le scansioni (AI)", "Enhance scans (AI)"), settings.upscale, Pref::Upscale),
        Row::Note(t(
            "Real-ESRGAN, sulla scheda video, rifà le pagine mostrate più grandi dei loro pixel.",
            "Real-ESRGAN, on the graphics card, redoes pages shown larger than their pixels.",
        )),
        toggle(t("Barra in alto", "Top bar"), settings.hud, Pref::Hud),
        Row::Stepper {
            label: t("Presentazione, una pagina ogni", "Slideshow, a page every"),
            value: format!("{secs} s"),
            less: Pref::Slideshow(secs.saturating_sub(1).max(1)),
            more: Pref::Slideshow((secs + 1).min(120)),
        },
        toggle(
            t("Avvisa delle versioni nuove", "Tell me about new versions"),
            settings.check_updates,
            Pref::CheckUpdates,
        ),
        Row::Note(t(
            "A ogni avvio chiede a GitHub qual è l'ultima versione. Non manda nient'altro.",
            "At each launch it asks GitHub which version is the latest. Nothing else is sent.",
        )),
    ]
}

fn lens_rows() -> Vec<Row> {
    vec![
        Row::Slider { label: t("Ingrandimento", "Magnification"), which: Slider::LensZoom },
        Row::Slider { label: t("Grandezza", "Size"), which: Slider::LensSize },
        Row::Note(t(
            "La lente mostra i pixel veri della pagina. Con la lente accesa la rotella cambia l'ingrandimento.",
            "The magnifier shows the page's real pixels. While it is on, the wheel changes the magnification.",
        )),
    ]
}

/// Ogni comando con i suoi tasti, per gruppi.
fn key_rows(keys: &Keymap) -> Vec<Row> {
    let mut rows = vec![Row::Note(t(
        "Fai clic su un comando e premi il tasto nuovo: Esc lascia com'era, Backspace toglie il tasto.",
        "Click a command and press the new key: Esc keeps it, Backspace removes it.",
    ))];
    let mut group = "";
    for b in Bind::ALL {
        if b.group() != group {
            group = b.group();
            rows.push(Row::Head(group));
        }
        rows.push(Row::Key { bind: b, keys: keys.keys(b).iter().map(Combo::shown).collect() });
    }
    rows.push(Row::Button { label: t("Tutti i tasti di serie", "All default keys"), pref: Pref::ResetKeys });
    rows
}
