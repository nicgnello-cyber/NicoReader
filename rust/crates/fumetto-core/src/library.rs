//! La libreria: i volumi dentro le cartelle scelte, riconosciuti e raggruppati
//! in serie.
//!
//! Un volume e' un archivio (CBZ, CBR, CB7, CBT, ZIP, RAR, 7Z, TAR, PDF, EPUB) o una
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
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::book::{is_image, read_info, title_of};
use crate::comicinfo::ComicInfo;
use crate::natural::natural_cmp;
use crate::progress::Progress;
use crate::{epub, remote};

const VOLUME_EXT: &[&str] = &["cbz", "cbr", "cb7", "cbt", "zip", "rar", "7z", "tar", "pdf", "epub"];

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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    pub path: PathBuf,
    pub title: String,
    /// Il nome della serie, se il volume ne fa parte.
    pub series: Option<String>,
    /// Il numero nella serie, per ordinarla.
    pub number: Option<u32>,
    /// Gli autori, dalla ComicInfo.xml: per cercarli.
    pub authors: Option<String>,
    /// Le raccolte, le liste di lettura, i generi e l'editore di cui fa parte.
    pub groups: Vec<Group>,
    /// Quando e' arrivato nella libreria (secondi dal 1970; 0: non si sa), per
    /// "aggiunti di recente": la data del file.
    pub added: u64,
    /// L'anno di uscita, dalla ComicInfo.xml o dall'EPUB.
    pub year: Option<u32>,
}

/// Che cosa riunisce dei volumi oltre la serie.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum GroupKind {
    /// Una raccolta di serie ("Universo Marvel"; SeriesGroup, le collezioni
    /// di Komga e Kavita).
    Collection,
    /// Una lista di lettura: volumi di serie diverse in un ordine preciso
    /// (StoryArc, le liste di Komga e Kavita).
    ReadingList,
    Genre,
    Publisher,
}

/// Un gruppo di cui fa parte un volume; `order`: il suo posto nella lista.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Group {
    pub kind: GroupKind,
    pub name: String,
    pub order: Option<u32>,
}

impl Group {
    pub fn new(kind: GroupKind, name: impl Into<String>, order: Option<u32>) -> Group {
        Group { kind, name: name.into(), order }
    }
}

/// I gruppi che dice una scheda.
pub fn groups_of(info: &ComicInfo) -> Vec<Group> {
    let mut out: Vec<Group> = Vec::new();
    out.extend(info.series_group.iter().map(|n| Group::new(GroupKind::Collection, n, None)));
    out.extend(info.story_arcs.iter().map(|(n, o)| Group::new(GroupKind::ReadingList, n, *o)));
    out.extend(info.genres.iter().map(|n| Group::new(GroupKind::Genre, n, None)));
    out.extend(info.publisher.iter().map(|n| Group::new(GroupKind::Publisher, n, None)));
    out
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
    split_same_name_series(&mut found, roots);
    folder_collections(&mut found, roots);
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
            // gli EPUB di testo (i romanzi) non sono fumetti
            if !crate::epub::looks_like_comic(&e.path()) {
                continue;
            }
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
    Entry { series: number.map(|_| name), number, title, added: added_at(&path), path, ..Entry::default() }
}

/// Quando il volume e' arrivato: la data di creazione del file (quella della
/// copia sul disco), dove il sistema la tiene; se no l'ultima modifica.
fn added_at(path: &Path) -> u64 {
    let Ok(meta) = std::fs::metadata(path) else { return 0 };
    let time = meta.created().or_else(|_| meta.modified());
    time.ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs())
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
    e.groups = groups_of(info);
    e.year = info.year;
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
/// della libreria) insieme ad altri: la cartella e' la serie, se sembrano i
/// pezzi di una serie (vedi `pieces_of_a_series`). Una cartella che dice solo
/// "Vol 1" o "Ch 3" non e' la serie, e' un pezzo del volume: la serie e' piu'
/// su (Berserk/Vol 1/Ch 1 e' "Berserk", volume "Vol 1 · Ch 1"), altrimenti i
/// "Vol 1" di tutti i manga diventerebbero una serie sola.
fn group_by_folder(found: &mut [Entry], roots: &[PathBuf]) {
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
fn pieces_of_a_series(folder: &str, titles: &[&str]) -> bool {
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
fn split_same_name_series(found: &mut [Entry], roots: &[PathBuf]) {
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
fn name_of(folder: &Path) -> String {
    folder.file_name().map_or_else(|| folder.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned())
}

/// Le cartelle che riuniscono serie (o volumi) diverse diventano raccolte:
/// quella di un autore, "Manga", "Webtoon". Non la cartella della serie
/// stessa (One Piece/), non quelle che dicono solo "Vol 1", non quelle della
/// libreria. Per ogni volume conta la prima cartella sopra di lui (e sopra
/// la sua serie), se dentro ci sono almeno due serie o volumi diversi.
fn folder_collections(found: &mut [Entry], roots: &[PathBuf]) {
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
fn collection_folder(e: &Entry, roots: &[PathBuf]) -> Option<PathBuf> {
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

/// In che ordine si va da un volume all'altro: quello della vista da cui lo
/// si e' aperto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order<'a> {
    /// La lista di lettura con questo nome.
    List(&'a str),
    /// Le serie, per numero; un volume senza serie, come nelle cartelle.
    Series,
    /// Le cartelle come le mostra la libreria ([`folder_cmp`]): finita una
    /// cartella si passa alla seguente, anche scendendo o risalendo.
    Folders,
}

/// Il volume da leggere dopo (o prima, se `forward` e' falso) quello in
/// `path`, nell'ordine `order`. Una lista di lettura vale solo se il volume
/// ne fa parte. Sempre dalla stessa parte: un volume della cartella non
/// continua con quello del server, ne' con quello di un'altra cartella della
/// libreria. Un volume fuori dalla libreria: il vicino nella sua cartella.
/// `None`: non c'e'.
pub fn neighbour(entries: &[Entry], roots: &[PathBuf], path: &Path, order: Order, forward: bool) -> Option<PathBuf> {
    let here = crate::absolute(path);
    let from = origin(&here);
    let same = |e: &Entry| crate::absolute(&e.path) == here;
    let step = |mut members: Vec<&Entry>, cmp: &dyn Fn(&Entry, &Entry) -> Ordering| {
        members.retain(|e| origin(&e.path) == from);
        members.sort_by(|a, b| cmp(a, b));
        let k = members.iter().position(|e| same(e))?;
        let to = if forward { k.checked_add(1) } else { k.checked_sub(1) };
        to.and_then(|k| members.get(k)).map(|e| e.path.clone())
    };
    if let Some(current) = entries.iter().find(|e| same(e)) {
        let place = |e: &Entry, name: &str| {
            e.groups
                .iter()
                .find(|g| g.kind == GroupKind::ReadingList && g.name == name)
                .map(|g| g.order.unwrap_or(u32::MAX))
        };
        if let Order::List(name) = order
            && place(current, name).is_some()
        {
            let members = entries.iter().filter(|e| place(e, name).is_some()).collect();
            return step(members, &|a, b| place(a, name).cmp(&place(b, name)).then_with(|| by_number(a, b)));
        }
        let root = root_of(&current.path, roots);
        // i volumi dei server non hanno cartelle: si va per serie
        if let Some(series) = current.series.as_ref().filter(|_| order != Order::Folders || root.is_none()) {
            let members = entries.iter().filter(|e| e.series.as_ref() == Some(series)).collect();
            return step(members, &by_number);
        }
        if let Some(root) = root {
            let members = entries.iter().filter(|e| root_of(&e.path, roots) == Some(root)).collect();
            return step(members, &|a, b| folder_cmp(&a.path, &b.path));
        }
    }
    if remote::is_remote(&here) {
        return None;
    }
    let dir = here.parent()?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.file_name().is_some_and(|n| is_volume_file(&n.to_string_lossy())))
        .filter(|p| {
            *p == here || !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("epub")) || epub::looks_like_comic(p)
        })
        .collect();
    files.sort_by(|a, b| folder_cmp(a, b));
    let k = files.iter().position(|p| *p == here)?;
    let to = if forward { k.checked_add(1) } else { k.checked_sub(1) };
    to.and_then(|k| files.get(k)).cloned()
}

/// La cartella della libreria che contiene `path` (la piu' interna, se una
/// sta dentro l'altra).
pub fn root_of<'a>(path: &Path, roots: &'a [PathBuf]) -> Option<&'a Path> {
    roots.iter().filter(|r| path.starts_with(r)).max_by_key(|r| r.components().count()).map(PathBuf::as_path)
}

/// L'ordine delle cartelle, come in Esplora file: in ogni cartella prima le
/// sottocartelle, poi i volumi, ciascuno in ordine naturale (2 prima di 10).
/// Letto di fila e' l'ordine in cui si leggono: una cartella intera, poi la
/// seguente.
///
/// Dev'essere un ordine totale: con uno incoerente l'ordinamento della
/// libreria standard si ferma con un panic. Per questo ogni pezzo del
/// percorso conta come (volume?, nome), e i percorsi si confrontano pezzo
/// per pezzo: l'ultimo pezzo e' il volume, gli altri cartelle. Cosi' anche
/// una cartella di immagini che e' un volume e ne contiene altri ("X" e
/// "X/y.cbz") ha un posto solo: "X" fra i volumi, cio' che contiene fra le
/// cartelle.
pub fn folder_cmp(a: &Path, b: &Path) -> Ordering {
    let (mut ka, mut kb) = (pieces(a), pieces(b));
    loop {
        match (ka.next(), kb.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some((va, x)), Some((vb, y))) => {
                // false (una cartella) prima di true (un volume)
                let o = va
                    .cmp(&vb)
                    .then_with(|| natural_cmp(&x.as_os_str().to_string_lossy(), &y.as_os_str().to_string_lossy()))
                    .then_with(|| x.cmp(&y));
                if o != Ordering::Equal {
                    return o;
                }
            }
        }
    }
}

/// I pezzi di un percorso per [`folder_cmp`]: (e' il volume, cioe' l'ultimo; pezzo).
fn pieces(p: &Path) -> impl Iterator<Item = (bool, std::path::Component<'_>)> {
    let last = p.components().count().saturating_sub(1);
    p.components().enumerate().map(move |(k, part)| (k == last, part))
}

/// Cosa sta in una cartella: le sottocartelle, ognuna con i volumi che
/// contiene a ogni profondita', e i volumi che ci stanno direttamente; tutto
/// nell'ordine di [`folder_cmp`]. Indici in `entries`, scelti fra `among`.
#[derive(Debug, Default, PartialEq)]
pub struct FolderContents {
    pub folders: Vec<(PathBuf, Vec<usize>)>,
    pub volumes: Vec<usize>,
}

pub fn folder_contents(entries: &[Entry], among: &[usize], folder: &Path) -> FolderContents {
    let mut out = FolderContents::default();
    for &i in among {
        let Ok(rest) = entries[i].path.strip_prefix(folder) else { continue };
        let mut parts = rest.components();
        let Some(first) = parts.next() else { continue };
        if parts.next().is_none() {
            out.volumes.push(i);
            continue;
        }
        let sub = folder.join(first);
        match out.folders.iter_mut().find(|(p, _)| *p == sub) {
            Some((_, members)) => members.push(i),
            None => out.folders.push((sub, vec![i])),
        }
    }
    let by_path = |a: &usize, b: &usize| folder_cmp(&entries[*a].path, &entries[*b].path);
    out.folders.sort_by(|a, b| folder_cmp(&a.0, &b.0));
    for (_, members) in &mut out.folders {
        members.sort_by(by_path);
    }
    out.volumes.sort_by(by_path);
    out
}

/// Da dove viene un volume: "" dal disco, schema e host dal server.
pub fn origin(path: &Path) -> String {
    if !remote::is_remote(path) {
        return String::new();
    }
    let s = path.to_string_lossy();
    let rest = s.split_once("://").map_or(&*s, |(_, r)| r);
    let host = rest.split('/').next().unwrap_or("");
    format!("{}://{host}", s.split("://").next().unwrap_or("").to_ascii_lowercase())
}

/// Per ordinare i volumi di una serie: prima quelli con il numero, per numero
/// (a parita', per titolo), poi quelli senza, per titolo. Un ordine totale
/// (vedi [`folder_cmp`]): confrontare ora i numeri ora i titoli, secondo la
/// coppia, lo renderebbe incoerente.
pub fn by_number(a: &Entry, b: &Entry) -> Ordering {
    match (a.number, b.number) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
    .then_with(|| natural_cmp(&a.title, &b.title))
    .then_with(|| a.path.cmp(&b.path))
}

#[cfg(test)]
mod tests;
