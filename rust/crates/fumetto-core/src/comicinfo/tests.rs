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
fn raccolte_generi_e_storie() {
    let xml = "<ComicInfo><Series>X-Men</Series><Number>141</Number>\
                   <StoryArcNumber>2, 5</StoryArcNumber><StoryArc>Giorni di un futuro passato, Saga</StoryArc>\
                   <SeriesGroup>Universo Marvel, Mutanti</SeriesGroup><Genre>Supereroi, Fantascienza, supereroi</Genre>\
                   <Publisher>Marvel</Publisher></ComicInfo>";
    let info = ComicInfo::parse(xml.as_bytes()).unwrap();
    assert_eq!(info.publisher.as_deref(), Some("Marvel"));
    assert_eq!(info.genres, ["Supereroi", "Fantascienza"], "senza ripetere");
    assert_eq!(info.series_group, ["Universo Marvel", "Mutanti"]);
    assert_eq!(info.story_arcs, [("Giorni di un futuro passato".to_owned(), Some(2)), ("Saga".to_owned(), Some(5))]);
    // una scheda vecchia in cache, senza i campi nuovi
    let old: ComicInfo = serde_json::from_str(r#"{"series":"A","number":"1"}"#).unwrap();
    assert!(old.genres.is_empty() && old.publisher.is_none());
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
    assert_eq!(
        info(Some("Solo"), Some("12"), None, Some("Ch. 012 - Re")).display_title().as_deref(),
        Some("Solo: Ch. 012 - Re")
    );
    assert_eq!(
        info(Some("Solo"), Some("12"), None, Some("Chapter 120")).display_title().as_deref(),
        Some("Solo 12: Chapter 120")
    );
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

#[test]
fn personaggi_gruppi_luoghi_e_lingua() {
    let xml = "<ComicInfo><Series>X-Men</Series><Writer>Chris Claremont</Writer><Penciller>John Byrne</Penciller>\
               <Characters>Wolverine, Tempesta, wolverine</Characters><Teams>X-Men, Hellfire Club</Teams>\
               <Locations>New York</Locations><Tags>da rileggere</Tags><Format>Graphic Novel</Format>\
               <LanguageISO>it</LanguageISO><Manga>Yes</Manga><Genre>Supereroi</Genre></ComicInfo>";
    let info = ComicInfo::parse(xml.as_bytes()).unwrap();
    assert_eq!(info.characters, ["Wolverine", "Tempesta"], "senza ripetere");
    assert_eq!(info.teams, ["X-Men", "Hellfire Club"]);
    assert_eq!(info.locations, ["New York"]);
    assert_eq!(info.tags, ["da rileggere"]);
    assert_eq!(info.format.as_deref(), Some("Graphic Novel"));
    assert_eq!(info.language.as_deref(), Some("it"));
    assert!(info.manga && info.right_to_left.is_none(), "manga, ma il verso non e' detto");
    use crate::library::{Group, GroupKind, groups_of, language_name};
    let groups = groups_of(&info);
    let has = |kind, name: &str| groups.iter().any(|g| g.same(&Group::new(kind, name, None)));
    assert!(has(GroupKind::Author, "Chris Claremont") && has(GroupKind::Author, "John Byrne"));
    assert!(has(GroupKind::Character, "Tempesta") && has(GroupKind::Team, "Hellfire Club"));
    assert!(has(GroupKind::Format, "Manga") && has(GroupKind::Format, "Graphic Novel"));
    assert!(has(GroupKind::Location, "New York") && has(GroupKind::Tag, "da rileggere"));
    assert!(has(GroupKind::Language, &language_name("it")));
    assert_eq!(groups.iter().filter(|g| g.kind == GroupKind::Character).count(), 2);
    assert_eq!(language_name("XX"), "XX", "un codice sconosciuto resta il codice");
    assert_eq!(language_name("Klingon"), "Klingon", "un nome per esteso resta com'e'");
    assert_eq!(language_name("en-US"), language_name("eng"));
}
