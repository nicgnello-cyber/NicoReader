//! ComicInfo.xml: la scheda che ComicRack, ComicTagger, Komga, Kavita e Mihon
//! mettono dentro i CBZ (o nella cartella del capitolo). Dice serie, numero,
//! titolo, autori e se e' un manga da leggere da destra a sinistra, meglio di
//! quanto si indovini dal nome del file.
//!
//! Il file e' piatto (un elemento <ComicInfo> con dentro solo testo), quindi
//! basta cercare i tag: niente libreria XML.

use serde::{Deserialize, Serialize};

/// Oltre questa misura non e' una scheda: non si legge.
pub const MAX_BYTES: u64 = 1 << 20;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ComicInfo {
    pub series: Option<String>,
    /// Com'e' scritto ("12", "12.5", "1/2"): per ordinare, `number_value`.
    pub number: Option<String>,
    /// Il volume, quando manca il numero (nei manga di solito c'e' questo).
    pub volume: Option<u32>,
    pub title: Option<String>,
    /// Sceneggiatori e disegnatori, senza ripetizioni, separati da virgole.
    pub authors: Option<String>,
    /// `Some(true)`: da destra a sinistra (Manga = YesAndRightToLeft).
    /// `Some(false)`: dichiarato non manga, o manga gia' da sinistra a destra.
    pub right_to_left: Option<bool>,
}

impl ComicInfo {
    /// La scheda dai byte del file. `None` se non e' una ComicInfo o non dice
    /// niente di utile.
    pub fn parse(bytes: &[u8]) -> Option<ComicInfo> {
        let text = decode_text(bytes)?;
        let root = text.find("<ComicInfo")?;
        let text = &text[root..];
        let tag = |name: &str| element(text, name).map(|s| unescape(&s)).map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
        let mut authors: Vec<String> = Vec::new();
        for role in ["Writer", "Penciller", "Artist"] {
            for name in tag(role).iter().flat_map(|s| s.split(',')) {
                let name = name.trim();
                if !name.is_empty() && !authors.iter().any(|a| a.eq_ignore_ascii_case(name)) {
                    authors.push(name.to_owned());
                }
            }
        }
        let info = ComicInfo {
            series: tag("Series"),
            number: tag("Number"),
            volume: tag("Volume").and_then(|v| v.parse().ok()),
            title: tag("Title"),
            authors: (!authors.is_empty()).then(|| authors.join(", ")),
            right_to_left: tag("Manga").and_then(|m| match m.as_str() {
                "YesAndRightToLeft" => Some(true),
                "No" => Some(false),
                // "Yes" dice manga ma non il verso: chi l'ha tradotto puo'
                // averlo gia' girato; e "Unknown" non dice niente
                _ => None,
            }),
        };
        (info != ComicInfo::default()).then_some(info)
    }

    /// Il numero per ordinare: la parte intera ("12.5" -> 12).
    /// Senza numero, il volume.
    pub fn number_value(&self) -> Option<u32> {
        match &self.number {
            Some(n) => {
                let digits: String = n.trim().chars().take_while(char::is_ascii_digit).take(6).collect();
                if digits.is_empty() { None } else { digits.parse().ok() }
            }
            None => self.volume,
        }
    }

    /// Il nome da mostrare: "Serie 12: Titolo", "Serie 12", o il titolo da
    /// solo. Se il titolo ha gia' il numero ("Chapter 12", come lo scrive
    /// Mihon), "Serie: Chapter 12". `None` se la scheda non ha ne' serie ne'
    /// titolo.
    pub fn display_title(&self) -> Option<String> {
        let number = self.number.clone().or_else(|| self.volume.map(|v| format!("Vol. {v}")));
        let title = self.title.as_deref().filter(|t| !self.series.as_deref().is_some_and(|s| t.eq_ignore_ascii_case(s)));
        match (self.series.as_deref(), number, title) {
            (Some(s), Some(n), Some(t)) if has_number(t, n.trim_start_matches("Vol. ")) => Some(format!("{s}: {t}")),
            (Some(s), Some(n), Some(t)) => Some(format!("{s} {n}: {t}")),
            (Some(s), Some(n), None) => Some(format!("{s} {n}")),
            (Some(s), None, Some(t)) => Some(format!("{s}: {t}")),
            (Some(s), None, None) => Some(s.to_owned()),
            (None, _, _) => self.title.clone(),
        }
    }
}

/// `number` dentro `text` come numero a se' ("12" in "Chapter 12", non in
/// "Chapter 120"), anche con zeri davanti ("Ch. 012").
fn has_number(text: &str, number: &str) -> bool {
    let want = number.trim_start_matches('0');
    let mut digits = String::new();
    for c in text.chars().chain([' ']) {
        if c.is_ascii_digit() {
            digits.push(c);
        } else if !digits.is_empty() {
            if digits.trim_start_matches('0') == want {
                return true;
            }
            digits.clear();
        }
    }
    false
}

/// Il testo del file: UTF-8 (con o senza BOM) o UTF-16 con il suo BOM, come
/// lo salva qualche programma di Windows.
fn decode_text(bytes: &[u8]) -> Option<String> {
    let utf16 = |be: bool| {
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| if be { u16::from_be_bytes(c) } else { u16::from_le_bytes(c) })
            .collect();
        String::from_utf16(&units).ok()
    };
    match bytes {
        [0xFF, 0xFE, ..] => utf16(false),
        [0xFE, 0xFF, ..] => utf16(true),
        [0xEF, 0xBB, 0xBF, rest @ ..] => Some(String::from_utf8_lossy(rest).into_owned()),
        _ => Some(String::from_utf8_lossy(bytes).into_owned()),
    }
}

/// Il contenuto del primo <name>...</name> (anche con attributi, anche
/// <name/>), cosi' com'e' nel file.
fn element(text: &str, name: &str) -> Option<String> {
    let mut from = 0;
    while let Some(at) = text[from..].find('<').map(|i| from + i) {
        let after = &text[at + 1..];
        from = at + 1;
        let Some(rest) = after.strip_prefix(name) else { continue };
        // "<Series" non deve prendere "<SeriesGroup"
        match rest.chars().next() {
            Some('>') => {}
            Some(c) if c.is_whitespace() || c == '/' => {}
            _ => continue,
        }
        let open_end = rest.find('>')?;
        if rest[..open_end].ends_with('/') {
            return Some(String::new());
        }
        let body = &rest[open_end + 1..];
        let close = body.find(&format!("</{name}"))?;
        let body = &body[..close];
        return Some(match body.trim().strip_prefix("<![CDATA[").and_then(|b| b.strip_suffix("]]>")) {
            // nel CDATA niente entita': lo si segna perche' unescape non lo tocchi
            Some(raw) => raw.replace('&', "&amp;"),
            None => body.to_owned(),
        });
    }
    None
}

/// Le entita' dell'XML: le cinque con nome e quelle numeriche.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let Some(semi) = rest.find(';').filter(|&i| i <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..semi];
        let ch = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .map(|h| u32::from_str_radix(h, 16))
                .or_else(|| entity.strip_prefix('#').map(str::parse))
                .and_then(Result::ok)
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Il nome di una ComicInfo dentro un archivio o una cartella: il file, a
/// qualsiasi profondita', con le maiuscole che vuole.
pub fn is_comic_info(name: &str) -> bool {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    file.eq_ignore_ascii_case("ComicInfo.xml")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ESEMPIO: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<ComicInfo xmlns:xsd="http://www.w3.org/2001/XMLSchema" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Title>Romance Dawn</Title>
  <Series>One Piece</Series>
  <SeriesGroup>Shonen Jump</SeriesGroup>
  <Number>1</Number>
  <Volume>1</Volume>
  <Summary>Luffy &amp; co. &lt;3 &#233;&#x00E8;</Summary>
  <Writer>Eiichiro Oda</Writer>
  <Penciller>Eiichiro Oda</Penciller>
  <Manga>YesAndRightToLeft</Manga>
</ComicInfo>"#;

    #[test]
    fn la_scheda_completa() {
        let info = ComicInfo::parse(ESEMPIO.as_bytes()).unwrap();
        assert_eq!(info.series.as_deref(), Some("One Piece"));
        assert_eq!(info.number.as_deref(), Some("1"));
        assert_eq!(info.volume, Some(1));
        assert_eq!(info.title.as_deref(), Some("Romance Dawn"));
        assert_eq!(info.authors.as_deref(), Some("Eiichiro Oda"), "lo stesso autore una volta sola");
        assert_eq!(info.right_to_left, Some(true));
        assert_eq!(info.number_value(), Some(1));
        assert_eq!(info.display_title().as_deref(), Some("One Piece 1: Romance Dawn"));
    }

    #[test]
    fn entita_cdata_e_tag_simili() {
        let xml = "<ComicInfo><SeriesGroup>No</SeriesGroup><Series><![CDATA[Tom & Jerry]]></Series>\
                   <Title>Gatto &amp; topo &#x2013; 1</Title><Writer>A. Uno, B. Due</Writer><Artist>B. Due</Artist>\
                   <Number/><Manga>Yes</Manga></ComicInfo>";
        let info = ComicInfo::parse(xml.as_bytes()).unwrap();
        assert_eq!(info.series.as_deref(), Some("Tom & Jerry"));
        assert_eq!(info.title.as_deref(), Some("Gatto & topo \u{2013} 1"));
        assert_eq!(info.number, None, "<Number/> vuoto");
        assert_eq!(info.authors.as_deref(), Some("A. Uno, B. Due"));
        assert_eq!(info.right_to_left, None, "Yes non dice il verso");
        assert_eq!(info.display_title().as_deref(), Some("Tom & Jerry: Gatto & topo \u{2013} 1"));
    }

    #[test]
    fn numeri_e_titoli() {
        let info = |series: Option<&str>, number: Option<&str>, volume, title: Option<&str>| ComicInfo {
            series: series.map(str::to_owned),
            number: number.map(str::to_owned),
            volume,
            title: title.map(str::to_owned),
            ..ComicInfo::default()
        };
        assert_eq!(info(Some("Dylan Dog"), Some("12.5"), None, None).number_value(), Some(12));
        assert_eq!(info(Some("Dylan Dog"), Some("XX"), None, None).number_value(), None);
        assert_eq!(info(Some("Berserk"), None, Some(3), None).number_value(), Some(3));
        assert_eq!(info(Some("Berserk"), None, Some(3), None).display_title().as_deref(), Some("Berserk Vol. 3"));
        assert_eq!(info(Some("Saga"), None, None, None).display_title().as_deref(), Some("Saga"));
        assert_eq!(info(Some("Saga"), Some("1"), None, Some("Saga")).display_title().as_deref(), Some("Saga 1"));
        assert_eq!(info(Some("Solo"), Some("12"), None, Some("Ch. 012 - Re")).display_title().as_deref(), Some("Solo: Ch. 012 - Re"));
        assert_eq!(info(Some("Solo"), Some("12"), None, Some("Chapter 120")).display_title().as_deref(), Some("Solo 12: Chapter 120"));
        assert_eq!(info(Some("Saga"), None, None, Some("Uno")).display_title().as_deref(), Some("Saga: Uno"));
        assert_eq!(info(None, None, None, None).display_title(), None);
    }

    #[test]
    fn utf16_e_file_che_non_sono_schede() {
        let mut bytes = vec![0xFF, 0xFE];
        for u in "<ComicInfo><Series>Città</Series></ComicInfo>".encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(ComicInfo::parse(&bytes).unwrap().series.as_deref(), Some("Città"));
        assert_eq!(ComicInfo::parse(b"<html><Series>no</Series></html>"), None);
        assert_eq!(ComicInfo::parse(b"<ComicInfo><Summary>solo trama</Summary></ComicInfo>"), None);
        assert_eq!(ComicInfo::parse(&[0xFF, 0xFE, 0x00]), None, "UTF-16 troncato");
    }

    #[test]
    fn il_nome_del_file() {
        assert!(is_comic_info("ComicInfo.xml"));
        assert!(is_comic_info("Vol 1/comicinfo.XML"));
        assert!(!is_comic_info("NotComicInfo.xml"));
        assert!(!is_comic_info("ComicInfo.xml.bak"));
    }
}
