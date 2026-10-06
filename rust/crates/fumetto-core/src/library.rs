//! La libreria: i volumi dentro le cartelle scelte, riconosciuti e raggruppati
//! in serie.
//!
//! Un volume e' un archivio (CBZ, CBR, CB7, CBT, ZIP, RAR, 7Z, TAR, PDF) o una
//! cartella con dentro delle immagini (molti tengono cosi' i webtoon e i
//! capitoli dei manga). Si riconosce dall'estensione: aprire ogni file per
//! guardarne il contenuto renderebbe la scansione lenta come la lettura.
//!
//! La serie viene dal nome ("Nebbia sul Porto v03", "Orbita Bassa #12",
//! "Lanterne - 07", "Kaiju vol. 2 (2021)"); se il nome non ha un numero, dalla
//! cartella che lo contiene (manga/AUTORE/Titolo/Capitolo 001: la serie e'
//! "Titolo"), purche' non sia una delle cartelle della libreria.
//!
//! Se il volume ha una ComicInfo.xml, serie, numero e titolo vengono da li':
//! chi l'ha scritta ne sapeva piu' del nome del file. Le schede lette si
//! tengono in una cache su disco, cosi' la scansione dopo la prima non riapre
//! gli archivi.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::book::{is_image, read_info, title_of};
use crate::comicinfo::ComicInfo;
use crate::natural::natural_cmp;
use crate::progress::Progress;

const VOLUME_EXT: &[&str] = &["cbz", "cbr", "cb7", "cbt", "zip", "rar", "7z", "tar", "pdf"];

/// Le parole che in un nome dicono "capitolo", "volume", "numero": prima del
/// numero, non fanno parte della serie. Le piu' lunghe prima ("capitolo"
/// prima di "cap").
const MARKS: &[&str] = &[
    "capitolo", "chapitre", "chapter", "episodio", "episode", "volume", "numero", "issue", "parte", "part", "tome",
    "tomo", "vol.", "cap.", "ch.", "ep.", "no.", "n.", "vol", "cap", "ch", "ep", "v.", "v", "#", "t",
];

/// Una cartella con dentro altri volumi e al massimo tante immagini sciolte
/// (la copertina della serie, un logo) non e' un volume: le immagini non sono
/// pagine.
const LOOSE_IMAGES: usize = 3;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub path: PathBuf,
    pub title: String,
    /// Il nome della serie, se il volume ne fa parte.
    pub series: Option<String>,
    /// Il numero nella serie, per ordinarla.
    pub number: Option<u32>,
    /// Gli autori, dalla ComicInfo.xml: per cercarli.
    pub authors: Option<String>,
}

/// Dove si e' arrivati in un volume.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    New,
    /// Pagina raggiunta (da 0) e pagine in tutto.
    Reading(usize, usize),
    Done,
}

impl Status {
    pub fn of(progress: &Progress, path: &Path) -> Status {
        match progress.get(path) {
            None => Status::New,
            // rimesso da leggere, ricordato solo per i segnalibri
            Some(s) if s.pages == 0 => Status::New,
            Some(s) if s.pages > 0 && s.page + 1 >= s.pages => Status::Done,
            Some(s) => Status::Reading(s.page, s.pages),
        }
    }
}

/// Tutti i volumi dentro le cartelle date, in ordine naturale di titolo.
/// Le cartelle che non si leggono (un disco staccato) si saltano in silenzio.
pub fn scan(roots: &[PathBuf]) -> Vec<Entry> {
    scan_with(roots, &mut |path| read_info(path))
}

/// Come `scan`, con le schede ComicInfo tenute nel file `cache`: si rilegge
/// solo quella dei volumi nuovi o cambiati (misura o data diverse).
pub fn scan_cached(roots: &[PathBuf], cache: &Path) -> Vec<Entry> {
    let mut old = InfoCache::load(cache);
    let mut new = InfoCache::default();
    let mut read = false;
    let found = scan_with(roots, &mut |path| {
        // le cartelle si guardano gia' per trovarle: rileggerle costa poco, e
        // una scheda cambiata non cambierebbe la data della cartella
        let Some(key) = cache_key(path).filter(|_| !path.is_dir()) else { return read_info(path) };
        let info = old.0.remove(&key).unwrap_or_else(|| {
            read = true;
            read_info(path)
        });
        new.0.insert(key, info.clone());
        info
    });
    // dentro solo i volumi visti adesso: quelli spariti o cambiati escono
    if read || !old.0.is_empty() {
        new.save(cache);
    }
    found
}

fn scan_with(roots: &[PathBuf], info: &mut dyn FnMut(&Path) -> Option<ComicInfo>) -> Vec<Entry> {
    let mut found = Vec::new();
    for root in roots {
        walk(root, root, &mut found);
    }
    for e in &mut found {
        if let Some(info) = info(&e.path) {
            apply_info(e, &info);
        }
    }
    found.sort_by(|a: &Entry, b| natural_cmp(&a.title, &b.title).then_with(|| a.path.cmp(&b.path)));
    found.dedup_by(|a, b| a.path == b.path);
    group_by_folder(&mut found, roots);
    found
}

fn walk(root: &Path, dir: &Path, found: &mut Vec<Entry>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let start = found.len();
    let mut images = 0;
    for e in read.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let Ok(kind) = e.file_type() else { continue };
        if kind.is_dir() {
            walk(root, &e.path(), found);
        } else if is_volume_file(&name) {
            found.push(entry(e.path()));
        } else if is_image(&name) {
            images += 1;
        }
    }
    // la cartella della libreria stessa, con immagini sciolte, non e' un
    // volume; e nemmeno una serie con la sua copertina accanto ai volumi
    let volumes_inside = found.len() - start;
    if images > 0 && dir != root && (volumes_inside == 0 || images > LOOSE_IMAGES) {
        found.push(entry(dir.to_owned()));
    }
}

fn is_volume_file(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| VOLUME_EXT.iter().any(|e| ext.eq_ignore_ascii_case(e)))
}

fn entry(path: PathBuf) -> Entry {
    let title = title_of(&path);
    let (name, number) = parse_series(&title);
    Entry { series: number.map(|_| name), number, title, path, authors: None }
}

/// Cio' che dice la ComicInfo.xml vince su cio' che si indovina dal nome. Una
/// scheda senza serie lascia la serie al nome o alla cartella.
fn apply_info(e: &mut Entry, info: &ComicInfo) {
    if let Some(series) = &info.series {
        e.series = Some(series.clone());
        e.number = info.number_value();
    } else if let Some(n) = info.number_value() {
        e.number = Some(n);
    }
    if let Some(title) = info.display_title() {
        e.title = title;
    }
    e.authors = info.authors.clone();
}

/// Le schede gia' lette, per percorso, misura e data del volume (`None`: il
/// volume non ne ha una).
#[derive(Default)]
struct InfoCache(HashMap<String, Option<ComicInfo>>);

impl InfoCache {
    /// Una cache che manca o non si legge e' una cache vuota.
    fn load(file: &Path) -> InfoCache {
        let read = std::fs::read(file).ok().and_then(|b| serde_json::from_slice(&b).ok());
        InfoCache(read.unwrap_or_default())
    }

    /// La cache e' un di piu': se non si scrive, la prossima volta si rilegge.
    fn save(&self, file: &Path) {
        let Ok(json) = serde_json::to_vec(&self.0) else { return };
        let tmp = file.with_extension("tmp");
        let _ = file
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|_| std::fs::write(&tmp, json))
            .and_then(|_| std::fs::rename(&tmp, file));
    }
}

fn cache_key(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
    Some(format!("{}|{}|{modified}", abs.to_string_lossy(), meta.len()))
}

/// I volumi senza numero nel nome, nella stessa cartella (che non sia una
/// della libreria) insieme ad altri: la cartella e' la serie. Una cartella
/// che dice solo "Vol 1" o "Ch 3" non e' la serie, e' un pezzo del volume: la
/// serie e' piu' su (Berserk/Vol 1/Ch 1 e' "Berserk", volume "Vol 1 · Ch 1"),
/// altrimenti i "Vol 1" di tutti i manga diventerebbero una serie sola.
fn group_by_folder(found: &mut [Entry], roots: &[PathBuf]) {
    let anchors: Vec<_> = found.iter().map(|e| series_folder(&e.path, roots)).collect();
    for i in 0..found.len() {
        if found[i].series.is_some() {
            continue;
        }
        let Some((folder, parts)) = &anchors[i] else { continue };
        let siblings = anchors.iter().filter(|a| a.as_ref().is_some_and(|(f, _)| f == folder)).count();
        if siblings < 2 {
            continue;
        }
        found[i].series = Some(title_of(folder));
        if parts.is_empty() {
            found[i].number = found[i].number.or_else(|| first_number(&found[i].title));
        } else {
            // il numero da solo direbbe "Ch 1" di ogni volume: l'ordine lo da' il titolo intero
            found[i].title = format!("{} \u{00b7} {}", parts.join(" \u{00b7} "), found[i].title);
            found[i].number = None;
        }
    }
}

/// La cartella che fa da serie per un volume, saltando quelle che dicono solo
/// "Vol 1" (restituite, dall'alto in basso); `None` se si arriva a una
/// cartella della libreria.
fn series_folder(volume: &Path, roots: &[PathBuf]) -> Option<(PathBuf, Vec<String>)> {
    let mut folder = volume.parent()?;
    let mut parts = Vec::new();
    loop {
        if roots.iter().any(|r| r.as_path() == folder) {
            return None;
        }
        let name = title_of(folder);
        if !chapter_like(&name) {
            return Some((folder.to_owned(), parts));
        }
        parts.insert(0, name);
        folder = folder.parent()?;
    }
}

/// Un nome che dice solo un numero di capitolo o di volume: "Vol 1",
/// "Ch.02", "Episode 5", "03".
fn chapter_like(name: &str) -> bool {
    let s = without_brackets(name);
    let rest = s.trim_end_matches(|c: char| c.is_ascii_digit());
    if rest.len() == s.len() {
        return false;
    }
    let rest = rest.trim_matches(|c: char| c.is_whitespace() || "_-.\u{2013}".contains(c)).to_lowercase();
    rest.is_empty() || MARKS.iter().any(|m| rest == m.trim_end_matches('.'))
}

/// Il nome senza le parentesi in fondo: "(2021)", "[Gruppo]", anche piu' d'una.
fn without_brackets(title: &str) -> &str {
    let mut s = title.trim();
    while let Some(close) = s.chars().last().filter(|c| *c == ')' || *c == ']') {
        let open = if close == ')' { '(' } else { '[' };
        match s.rfind(open) {
            Some(i) => s = s[..i].trim_end(),
            None => break,
        }
    }
    s
}

fn first_number(s: &str) -> Option<u32> {
    let start = s.find(|c: char| c.is_ascii_digit())?;
    let digits: String = s[start..].chars().take_while(char::is_ascii_digit).take(6).collect();
    digits.parse().ok()
}

/// Divide "Nebbia sul Porto v03" in ("Nebbia sul Porto", Some(3)). Senza un
/// numero in fondo, il titolo intero e `None`.
///
/// Il numero sta quasi sempre in fondo, preceduto magari da una delle MARKS
/// (v, vol, #, cap, ch, episode, tome...), e seguito a volte da un (anno) o un
/// [gruppo] fra parentesi. Un nome che e' solo una di quelle parole ("Episode
/// 5") non e' una serie.
pub fn parse_series(title: &str) -> (String, Option<u32>) {
    let s = without_brackets(title);
    let whole = (s.to_owned(), None);
    let digits = s.len() - s.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 || digits > 4 {
        return whole;
    }
    let number: u32 = s[s.len() - digits..].parse().unwrap_or(0);
    let before = &s[..s.len() - digits];
    let head = before.trim_end();
    // un segno prima del numero (MARKS), a sua volta preceduto da un separatore
    let sep = |c: Option<char>| c.is_none_or(|c| c.is_whitespace() || "_.-\u{2013}".contains(c));
    let lower = head.to_lowercase();
    let name = MARKS
        .iter()
        .find_map(|m| {
            let cut = head.len().checked_sub(m.len())?;
            (lower.ends_with(*m) && head.is_char_boundary(cut) && sep(head[..cut].chars().last())).then(|| &head[..cut])
        })
        .or_else(|| (head.len() < before.len() || sep(head.chars().last())).then_some(head));
    match name.map(|n| n.trim_matches(|c: char| c.is_whitespace() || "_-.\u{2013}".contains(c))) {
        Some(n) if !n.is_empty() => (n.to_owned(), Some(number)),
        _ => whole,
    }
}

/// Per ordinare i volumi di una serie: prima il numero, poi il titolo.
pub fn by_number(a: &Entry, b: &Entry) -> Ordering {
    match (a.number, b.number) {
        (Some(x), Some(y)) if x != y => x.cmp(&y),
        _ => natural_cmp(&a.title, &b.title),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serie_dai_nomi() {
        for (title, name, n) in [
            ("Nebbia sul Porto v03", "Nebbia sul Porto", Some(3)),
            ("Orbita Bassa #12", "Orbita Bassa", Some(12)),
            ("Lanterne - 07", "Lanterne", Some(7)),
            ("Kaiju Diner vol. 2 (2021)", "Kaiju Diner", Some(2)),
            ("One Piece v101 [Gruppo] (2022)", "One Piece", Some(101)),
            ("Sette Lune 7", "Sette Lune", Some(7)),
            ("Berserk_ch.120", "Berserk", Some(120)),
            ("Kaiju8", "Kaiju8", None),
            ("2001", "2001", None),
            ("Vol.01 Ch.001 - Screw", "Vol.01 Ch.001 - Screw", None),
            ("Acqua Alta", "Acqua Alta", None),
            // le parole di capitolo da sole non sono una serie
            ("Episode 5", "Episode 5", None),
            ("Volume 3", "Volume 3", None),
            ("Tower of God Episode 5", "Tower of God", Some(5)),
            ("Dylan Dog n. 12", "Dylan Dog", Some(12)),
            ("Asterix Tome 3", "Asterix", Some(3)),
            ("Counterpart 2", "Counterpart", Some(2)),
        ] {
            assert_eq!(parse_series(title), (name.to_owned(), n), "{title}");
        }
    }

    #[test]
    fn scansione_archivi_cartelle_e_serie() {
        let root = std::env::temp_dir().join(format!("fumetto-libreria-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mk = |p: &str| {
            let p = root.join(p);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"x").unwrap();
        };
        mk("Nebbia sul Porto v01.cbz");
        mk("Nebbia sul Porto v02.cbz");
        mk("note.txt");
        mk("sciolta.jpg"); // un'immagine nella cartella della libreria: non e' un volume
        mk("Robo/Cap 1 - Vite/001.jpg");
        mk("Robo/Cap 2 - Dado/001.jpg");
        mk("Singolo/pagina.png");
        let found = scan(std::slice::from_ref(&root));
        let got: Vec<(&str, Option<&str>)> = found.iter().map(|e| (e.title.as_str(), e.series.as_deref())).collect();
        assert_eq!(
            got,
            [
                ("Cap 1 - Vite", Some("Robo")),
                ("Cap 2 - Dado", Some("Robo")),
                ("Nebbia sul Porto v01", Some("Nebbia sul Porto")),
                ("Nebbia sul Porto v02", Some("Nebbia sul Porto")),
                ("Singolo", None),
            ]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn capitoli_e_copertine_nelle_sottocartelle() {
        let root = std::env::temp_dir().join(format!("fumetto-libreria-strutture-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mk = |p: &str| {
            let p = root.join(p);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"x").unwrap();
        };
        // volumi numerati, con la copertina della serie accanto
        mk("Manga/One Piece/One Piece v01.cbz");
        mk("Manga/One Piece/One Piece v02.cbz");
        mk("Manga/One Piece/cover.jpg");
        // webtoon a capitoli, con la copertina nella cartella della serie
        mk("Webtoon/Solo Leveling/cover.jpg");
        mk("Webtoon/Solo Leveling/Chapter 1/001.jpg");
        mk("Webtoon/Solo Leveling/Chapter 2/001.jpg");
        // "Episode" non e' una serie: senza la correzione, i due webtoon finivano insieme
        mk("Webtoon/Tower of God/Episode 1/001.jpg");
        mk("Webtoon/Tower of God/Episode 2/001.jpg");
        mk("Webtoon/Lore Olympus/Episode 1/001.jpg");
        mk("Webtoon/Lore Olympus/Episode 2/001.jpg");
        // capitoli dentro i volumi: la serie e' Berserk, non "Vol 1"
        mk("Manga/Berserk/Vol 1/Ch 1/001.jpg");
        mk("Manga/Berserk/Vol 1/Ch 2/001.jpg");
        mk("Manga/Berserk/Vol 2/Ch 1/001.jpg");
        mk("Manga/Vagabond/Vol 1/Ch 1/001.jpg");
        mk("Manga/Vagabond/Vol 1/Ch 2/001.jpg");
        // tante immagini accanto ai capitoli: sono pagine, la cartella resta un volume
        for p in ["01", "02", "03", "04"] {
            mk(&format!("Artbook/{p}.jpg"));
        }
        mk("Artbook/Extra 1/001.jpg");
        let found = scan(std::slice::from_ref(&root));
        let mut got: Vec<(&str, Option<&str>)> =
            found.iter().map(|e| (e.title.as_str(), e.series.as_deref())).collect();
        got.sort();
        assert_eq!(
            got,
            [
                ("Artbook", None),
                ("Chapter 1", Some("Solo Leveling")),
                ("Chapter 2", Some("Solo Leveling")),
                ("Episode 1", Some("Lore Olympus")),
                ("Episode 1", Some("Tower of God")),
                ("Episode 2", Some("Lore Olympus")),
                ("Episode 2", Some("Tower of God")),
                ("Extra 1", Some("Extra")), // dal nome, come prima; da solo resta una copertina singola
                ("One Piece v01", Some("One Piece")),
                ("One Piece v02", Some("One Piece")),
                ("Vol 1 \u{00b7} Ch 1", Some("Berserk")),
                ("Vol 1 \u{00b7} Ch 1", Some("Vagabond")),
                ("Vol 1 \u{00b7} Ch 2", Some("Berserk")),
                ("Vol 1 \u{00b7} Ch 2", Some("Vagabond")),
                ("Vol 2 \u{00b7} Ch 1", Some("Berserk")),
            ]
        );
        // nella serie, i capitoli nei volumi in ordine
        let mut berserk: Vec<&Entry> = found.iter().filter(|e| e.series.as_deref() == Some("Berserk")).collect();
        berserk.sort_by(|a, b| by_number(a, b));
        let order: Vec<&str> = berserk.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(order, ["Vol 1 \u{00b7} Ch 1", "Vol 1 \u{00b7} Ch 2", "Vol 2 \u{00b7} Ch 1"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Un CBZ con una pagina finta e, se data, la sua ComicInfo.xml.
    fn cbz(path: &Path, info: Option<&str>) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        if let Some(xml) = info {
            zip.start_file("ComicInfo.xml", stored).unwrap();
            std::io::Write::write_all(&mut zip, xml.as_bytes()).unwrap();
        }
        zip.start_file("001.jpg", stored).unwrap();
        std::io::Write::write_all(&mut zip, b"x").unwrap();
        zip.finish().unwrap();
    }

    fn scheda(series: &str, number: &str, extra: &str) -> String {
        format!("<ComicInfo><Series>{series}</Series><Number>{number}</Number>{extra}</ComicInfo>")
    }

    #[test]
    fn la_comicinfo_vince_sul_nome() {
        let root = std::env::temp_dir().join(format!("fumetto-libreria-scheda-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // nomi che da soli non direbbero niente, o direbbero altro
        cbz(
            &root.join("scaricati/op_c001_hq [scan].cbz"),
            Some(&scheda("One Piece", "1", "<Title>Romance Dawn</Title><Writer>Eiichiro Oda</Writer>")),
        );
        cbz(&root.join("scaricati/zzz.cbz"), Some(&scheda("One Piece", "2", "")));
        cbz(&root.join("Nebbia sul Porto v01.cbz"), None);
        cbz(&root.join("Nebbia sul Porto v02.cbz"), Some("<ComicInfo><Summary>niente</Summary></ComicInfo>"));
        // un capitolo scaricato da Mihon: una cartella con la sua scheda
        let chapter = root.join("Mihon/Solo Leveling/Capitolo dodici");
        std::fs::create_dir_all(&chapter).unwrap();
        std::fs::write(chapter.join("001.jpg"), b"x").unwrap();
        std::fs::write(chapter.join("ComicInfo.xml"), scheda("Solo Leveling", "12", "<Title>Chapter 12</Title>"))
            .unwrap();

        let found = scan(std::slice::from_ref(&root));
        let got: Vec<_> = found.iter().map(|e| (e.title.as_str(), e.series.as_deref(), e.number)).collect();
        assert_eq!(
            got,
            [
                ("Nebbia sul Porto v01", Some("Nebbia sul Porto"), Some(1)),
                ("Nebbia sul Porto v02", Some("Nebbia sul Porto"), Some(2)),
                ("One Piece 1: Romance Dawn", Some("One Piece"), Some(1)),
                ("One Piece 2", Some("One Piece"), Some(2)),
                ("Solo Leveling: Chapter 12", Some("Solo Leveling"), Some(12)),
            ]
        );
        assert_eq!(found[2].authors.as_deref(), Some("Eiichiro Oda"));

        // la cache: la seconda volta le schede vengono da li'
        let cache = root.join("dati/comicinfo.json");
        assert_eq!(scan_cached(std::slice::from_ref(&root), &cache), found);
        let json = std::fs::read_to_string(&cache).unwrap();
        assert!(json.contains("Romance Dawn") && !json.contains("Mihon"), "solo gli archivi: {json}");
        std::fs::write(&cache, json.replace("Romance Dawn", "Dalla cache")).unwrap();
        let again = scan_cached(std::slice::from_ref(&root), &cache);
        assert_eq!(again[2].title, "One Piece 1: Dalla cache");

        // un volume cambiato si rilegge, uno sparito esce dalla cache
        cbz(&root.join("scaricati/op_c001_hq [scan].cbz"), Some(&scheda("One Piece", "1", "<Title>Nuova</Title>")));
        std::fs::remove_file(root.join("scaricati/zzz.cbz")).unwrap();
        let after = scan_cached(std::slice::from_ref(&root), &cache);
        assert!(after.iter().any(|e| e.title == "One Piece 1: Nuova"));
        let json = std::fs::read_to_string(&cache).unwrap();
        assert!(!json.contains("zzz") && !json.contains("Dalla cache"), "{json}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
