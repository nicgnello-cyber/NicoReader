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

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use crate::book::{is_image, title_of};
use crate::natural::natural_cmp;
use crate::progress::Progress;

const VOLUME_EXT: &[&str] = &["cbz", "cbr", "cb7", "cbt", "zip", "rar", "7z", "tar", "pdf"];

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub path: PathBuf,
    pub title: String,
    /// Il nome della serie, se il volume ne fa parte.
    pub series: Option<String>,
    /// Il numero nella serie, per ordinarla.
    pub number: Option<u32>,
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
    let mut found = Vec::new();
    for root in roots {
        walk(root, root, &mut found);
    }
    found.sort_by(|a: &Entry, b| natural_cmp(&a.title, &b.title).then_with(|| a.path.cmp(&b.path)));
    found.dedup_by(|a, b| a.path == b.path);
    group_by_folder(&mut found, roots);
    found
}

fn walk(root: &Path, dir: &Path, found: &mut Vec<Entry>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let mut has_images = false;
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
        } else if !has_images && is_image(&name) {
            has_images = true;
        }
    }
    // la cartella della libreria stessa, con immagini sciolte, non e' un volume
    if has_images && dir != root {
        found.push(entry(dir.to_owned()));
    }
}

fn is_volume_file(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| VOLUME_EXT.iter().any(|e| ext.eq_ignore_ascii_case(e)))
}

fn entry(path: PathBuf) -> Entry {
    let title = title_of(&path);
    let (name, number) = parse_series(&title);
    Entry { series: number.map(|_| name), number, title, path }
}

/// I volumi senza numero nel nome, nella stessa cartella (che non sia una
/// della libreria) insieme ad altri: la cartella e' la serie.
fn group_by_folder(found: &mut [Entry], roots: &[PathBuf]) {
    let parent_of = |e: &Entry| e.path.parent().map(Path::to_owned);
    for i in 0..found.len() {
        if found[i].series.is_some() {
            continue;
        }
        let Some(parent) = parent_of(&found[i]).filter(|p| !roots.contains(p)) else { continue };
        let siblings = found.iter().filter(|e| parent_of(e).as_ref() == Some(&parent)).count();
        if siblings >= 2 {
            found[i].series = Some(title_of(&parent));
            found[i].number = first_number(&found[i].title);
        }
    }
}

fn first_number(s: &str) -> Option<u32> {
    let start = s.find(|c: char| c.is_ascii_digit())?;
    let digits: String = s[start..].chars().take_while(char::is_ascii_digit).take(6).collect();
    digits.parse().ok()
}

/// Divide "Nebbia sul Porto v03" in ("Nebbia sul Porto", Some(3)). Senza un
/// numero in fondo, il titolo intero e `None`.
///
/// Il numero sta quasi sempre in fondo, preceduto magari da v, vol, #, t, cap
/// o ch, e seguito a volte da un (anno) o un [gruppo] fra parentesi.
pub fn parse_series(title: &str) -> (String, Option<u32>) {
    let mut s = title.trim();
    // le parentesi in fondo: "(2021)", "[Gruppo]", anche piu' d'una
    while let Some(close) = s.chars().last().filter(|c| *c == ')' || *c == ']') {
        let open = if close == ')' { '(' } else { '[' };
        match s.rfind(open) {
            Some(i) => s = s[..i].trim_end(),
            None => break,
        }
    }
    let whole = (s.to_owned(), None);
    let digits = s.len() - s.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 || digits > 4 {
        return whole;
    }
    let number: u32 = s[s.len() - digits..].parse().unwrap_or(0);
    let before = &s[..s.len() - digits];
    let head = before.trim_end();
    // un segno prima del numero, a sua volta preceduto da un separatore
    const MARKS: &[&str] = &["capitolo", "chapter", "vol.", "cap.", "ch.", "vol", "cap", "ch", "v.", "v", "#", "t"];
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
        assert_eq!(got, [
            ("Cap 1 - Vite", Some("Robo")),
            ("Cap 2 - Dado", Some("Robo")),
            ("Nebbia sul Porto v01", Some("Nebbia sul Porto")),
            ("Nebbia sul Porto v02", Some("Nebbia sul Porto")),
            ("Singolo", None),
        ]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
