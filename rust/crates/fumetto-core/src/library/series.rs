//! Le serie: dal nome del volume, dalla cartella che lo contiene, dalle
//! cartelle degli autori; quelle con lo stesso nome restano separate.

use super::*;

/// I volumi senza numero nel nome, nella stessa cartella (che non sia una
/// della libreria) insieme ad altri: la cartella e' la serie, se sembrano i
/// pezzi di una serie (vedi `pieces_of_a_series`). Una cartella che dice solo
/// "Vol 1" o "Ch 3" non e' la serie, e' un pezzo del volume: la serie e' piu'
/// su (Berserk/Vol 1/Ch 1 e' "Berserk", volume "Vol 1 · Ch 1"), altrimenti i
/// "Vol 1" di tutti i manga diventerebbero una serie sola.
pub(super) fn group_by_folder(found: &mut [Entry], roots: &[PathBuf]) {
    let anchors: Vec<_> = found.iter().map(|e| series_folder(&e.path, roots)).collect();
    // per cartella: i volumi ancora senza serie che ci stanno direttamente
    let mut loose: HashMap<&Path, Vec<usize>> = HashMap::new();
    for (i, a) in anchors.iter().enumerate() {
        if let Some((folder, parts)) = a
            && parts.is_empty()
            && found[i].series.is_none()
        {
            loose.entry(folder.as_path()).or_default().push(i);
        }
    }
    let series_folders: HashSet<&Path> = loose
        .iter()
        .filter(|(folder, members)| {
            let titles: Vec<&str> = members.iter().map(|&i| found[i].title.as_str()).collect();
            pieces_of_a_series(&title_of(folder), &titles)
        })
        .map(|(folder, _)| *folder)
        .collect();
    for i in 0..found.len() {
        if found[i].series.is_some() {
            continue;
        }
        let Some((folder, parts)) = &anchors[i] else { continue };
        let siblings = anchors.iter().filter(|a| a.as_ref().is_some_and(|(f, _)| f == folder)).count();
        if siblings < 2 || (parts.is_empty() && !series_folders.contains(folder.as_path())) {
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

/// Dei volumi senza numero nella stessa cartella sono i pezzi di una serie se
/// qualcuno ha un numero nel titolo ("Cap 1 - Vite") o se cominciano tutti
/// con il nome della cartella ("Akira/Akira parte uno"). Titoli diversi
/// senza numeri ("Moebius/Arzach", "Moebius/Il garage ermetico") sono opere
/// diverse: la cartella e' una raccolta, non una serie.
pub(super) fn pieces_of_a_series(folder: &str, titles: &[&str]) -> bool {
    let folder = without_brackets(folder).to_lowercase();
    titles.iter().any(|t| first_number(t).is_some())
        || (!folder.is_empty() && titles.iter().all(|t| t.to_lowercase().starts_with(&folder)))
}

/// Due serie con lo stesso nome in cartelle diverse ("Batman (2011)/Batman
/// v01.cbz" e "Batman (2016)/Batman v01.cbz") sono serie diverse: insieme
/// farebbero una pila sola, con due volumi 1. La cartella di una serie e' la
/// prima sopra il volume che si chiama come lei (parentesi a parte); se non
/// ce n'e' una (volumi sciolti fra altri) la serie resta com'e'. Quando le
/// cartelle di uno stesso nome sono piu' d'una, ogni serie prende il nome
/// della sua ("Batman (2011)"); se anche questi coincidono, con quella sopra
/// ("Batman (DC)").
pub(super) fn split_same_name_series(found: &mut [Entry], roots: &[PathBuf]) {
    let home = |e: &Entry| -> Option<PathBuf> {
        let series = without_brackets(e.series.as_deref()?);
        let root = root_of(&e.path, roots)?;
        e.path
            .ancestors()
            .skip(1)
            .take_while(|a| *a != root && a.starts_with(root))
            .find(|a| without_brackets(&name_of(a)).eq_ignore_ascii_case(series))
            .map(Path::to_path_buf)
    };
    let homes: Vec<Option<PathBuf>> = found.iter().map(home).collect();
    let mut by_name: HashMap<&str, Vec<&Path>> = HashMap::new();
    for (e, h) in found.iter().zip(&homes) {
        if let (Some(series), Some(h)) = (&e.series, h) {
            let seen = by_name.entry(series).or_default();
            if !seen.contains(&h.as_path()) {
                seen.push(h);
            }
        }
    }
    let renamed: Vec<Option<String>> = found
        .iter()
        .zip(&homes)
        .map(|(e, h)| {
            let (series, h) = (e.series.as_deref()?, h.as_deref()?);
            let others = by_name.get(series).filter(|v| v.len() > 1)?;
            let own = name_of(h);
            let clash = others.iter().filter(|o| name_of(o).eq_ignore_ascii_case(&own)).count() > 1;
            Some(match h.parent().filter(|_| clash) {
                Some(up) => format!("{own} ({})", name_of(up)),
                None => own,
            })
        })
        .collect();
    for (e, name) in found.iter_mut().zip(renamed) {
        if name.is_some() {
            e.series = name;
        }
    }
}

/// Il nome di una cartella, cosi' com'e' (`title_of` toglierebbe cio' che
/// segue un punto, come in un file).
pub(super) fn name_of(folder: &Path) -> String {
    folder.file_name().map_or_else(|| folder.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned())
}

/// Le cartelle che riuniscono serie (o volumi) diverse diventano raccolte:
/// quella di un autore, "Manga", "Webtoon". Non la cartella della serie
/// stessa (One Piece/), non quelle che dicono solo "Vol 1", non quelle della
/// libreria. Per ogni volume conta la prima cartella sopra di lui (e sopra
/// la sua serie), se dentro ci sono almeno due serie o volumi diversi.
pub(super) fn folder_collections(found: &mut [Entry], roots: &[PathBuf]) {
    let homes: Vec<Option<PathBuf>> = found.iter().map(|e| collection_folder(e, roots)).collect();
    // per cartella: le serie e i volumi senza serie che contiene
    let mut units: HashMap<&Path, HashSet<String>> = HashMap::new();
    for (e, home) in found.iter().zip(&homes) {
        if let Some(home) = home {
            let unit = e.series.clone().unwrap_or_else(|| e.path.to_string_lossy().into_owned());
            units.entry(home.as_path()).or_default().insert(unit);
        }
    }
    for (e, home) in found.iter_mut().zip(&homes) {
        let Some(home) = home.as_deref().filter(|h| units.get(h).is_some_and(|u| u.len() >= 2)) else { continue };
        let name = title_of(home);
        if !e.groups.iter().any(|g| g.kind == GroupKind::Collection && g.name == name) {
            e.groups.push(Group::new(GroupKind::Collection, name, None));
        }
    }
}

/// La cartella che potrebbe raccogliere il volume: la sua, saltando quelle
/// dei capitoli ("Vol 1") e quella della sua serie; `None` se si arriva a
/// una cartella della libreria.
pub(super) fn collection_folder(e: &Entry, roots: &[PathBuf]) -> Option<PathBuf> {
    let mut folder = e.path.parent()?;
    let is_root = |f: &Path| roots.iter().any(|r| r.as_path() == f);
    while !is_root(folder) && chapter_like(&title_of(folder)) {
        folder = folder.parent()?;
    }
    if let Some(series) = &e.series
        && !is_root(folder)
        && without_brackets(&title_of(folder)).eq_ignore_ascii_case(without_brackets(series))
    {
        folder = folder.parent()?;
    }
    (!is_root(folder) && roots.iter().any(|r| folder.starts_with(r))).then(|| folder.to_owned())
}

/// La cartella che fa da serie per un volume, saltando quelle che dicono solo
/// "Vol 1" (restituite, dall'alto in basso); `None` se si arriva a una
/// cartella della libreria.
pub(super) fn series_folder(volume: &Path, roots: &[PathBuf]) -> Option<(PathBuf, Vec<String>)> {
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
pub(super) fn chapter_like(name: &str) -> bool {
    let s = without_brackets(name);
    let rest = s.trim_end_matches(|c: char| c.is_ascii_digit());
    if rest.len() == s.len() {
        return false;
    }
    let rest = rest.trim_matches(|c: char| c.is_whitespace() || "_-.\u{2013}".contains(c)).to_lowercase();
    rest.is_empty() || MARKS.iter().any(|m| rest == m.trim_end_matches('.'))
}

/// Il nome senza le parentesi in fondo: "(2021)", "[Gruppo]", anche piu' d'una.
pub(super) fn without_brackets(title: &str) -> &str {
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

pub(super) fn first_number(s: &str) -> Option<u32> {
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
