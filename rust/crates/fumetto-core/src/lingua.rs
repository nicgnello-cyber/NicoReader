//! Italiano o inglese: lo decide la lingua dell'interfaccia del sistema, una
//! volta sola. `FUMETTO_LANG=it` (o `en`) la impone, per le prove o per gusto.
//!
//! Le scritte sono poche e stanno accanto a chi le usa, in coppia:
//! `t("Apri fumetto", "Open comic")`. Un file di traduzioni a parte servira'
//! quando le lingue saranno piu' di due.

use std::sync::OnceLock;

pub fn italian() -> bool {
    static IT: OnceLock<bool> = OnceLock::new();
    *IT.get_or_init(|| {
        let lang = std::env::var("FUMETTO_LANG").ok().or_else(sys_locale::get_locale).unwrap_or_default();
        lang.to_lowercase().starts_with("it")
    })
}

/// La scritta nella lingua dell'interfaccia.
pub fn t<'a>(it: &'a str, en: &'a str) -> &'a str {
    if italian() { it } else { en }
}
