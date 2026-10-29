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
    /// Le raccolte, le liste di lettura, i generi e l'editore di cui fa parte.
    pub groups: Vec<Group>,
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
    Entry { series: number.map(|_| name), number, title, path, authors: None, groups: Vec::new() }
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
pub fn folder_cmp(a: &Path, b: &Path) -> Ordering {
    let (ca, cb): (Vec<_>, Vec<_>) = (a.components().collect(), b.components().collect());
    for (k, (x, y)) in ca.iter().zip(&cb).enumerate() {
        if x != y {
            // una cartella (c'e' dell'altro dopo) prima di un volume
            let (dir_a, dir_b) = (k + 1 < ca.len(), k + 1 < cb.len());
            return dir_b
                .cmp(&dir_a)
                .then_with(|| natural_cmp(&x.as_os_str().to_string_lossy(), &y.as_os_str().to_string_lossy()))
                .then_with(|| x.cmp(y));
        }
    }
    ca.len().cmp(&cb.len())
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
    fn il_volume_dopo_e_prima() {
        let vol = |path: &str, series: Option<(&str, u32)>, list: Option<u32>| Entry {
            path: PathBuf::from(path),
            title: title_of(Path::new(path)),
            series: series.map(|s| s.0.to_owned()),
            number: series.map(|s| s.1),
            authors: None,
            groups: list.map(|n| Group::new(GroupKind::ReadingList, "Nebbia", Some(n))).into_iter().collect(),
        };
        let server = "http://casa:25600/opds/v1.2/books/";
        let entries = vec![
            vol("/l/Porto v02.cbz", Some(("Porto", 2)), Some(3)),
            vol("/l/Porto v10.cbz", Some(("Porto", 10)), None),
            vol("/l/Porto v01.cbz", Some(("Porto", 1)), Some(1)),
            vol("/l/Orbita v01.cbz", Some(("Orbita", 1)), Some(2)),
            vol(&format!("{server}a/pages/{{n}}#10:Porto v01"), Some(("Porto", 1)), None),
            vol(&format!("{server}b/pages/{{n}}#10:Porto v02"), Some(("Porto", 2)), None),
        ];
        let roots = [PathBuf::from("/l")];
        let next = |p: &str, order| neighbour(&entries, &roots, Path::new(p), order, true);
        let prev = |p: &str, order| neighbour(&entries, &roots, Path::new(p), order, false);
        let series = Order::Series;
        assert_eq!(next("/l/Porto v01.cbz", series), Some("/l/Porto v02.cbz".into()), "per numero, non per titolo");
        assert_eq!(next("/l/Porto v02.cbz", series), Some("/l/Porto v10.cbz".into()));
        assert_eq!(next("/l/Porto v10.cbz", series), None, "l'ultimo; quelli del server non contano");
        assert_eq!(prev("/l/Porto v10.cbz", series), Some("/l/Porto v02.cbz".into()));
        assert_eq!(prev("/l/Porto v01.cbz", series), None, "il primo");
        assert_eq!(
            next(&entries[4].path.to_string_lossy(), series),
            Some(entries[5].path.clone()),
            "dal server, il server"
        );
        assert_eq!(
            next(&entries[4].path.to_string_lossy(), Order::Folders),
            Some(entries[5].path.clone()),
            "il server non ha cartelle: la serie"
        );
        let list = Order::List("Nebbia");
        assert_eq!(next("/l/Porto v01.cbz", list), Some("/l/Orbita v01.cbz".into()), "la lista di lettura");
        assert_eq!(next("/l/Orbita v01.cbz", list), Some("/l/Porto v02.cbz".into()));
        assert_eq!(prev("/l/Porto v02.cbz", list), Some("/l/Orbita v01.cbz".into()));
        assert_eq!(next("/l/Porto v10.cbz", list), None, "fuori dalla lista: la serie");
        // le cartelle: per nome, non per numero
        assert_eq!(next("/l/Orbita v01.cbz", Order::Folders), Some("/l/Porto v01.cbz".into()));
        assert_eq!(next("/l/Porto v02.cbz", Order::Folders), Some("/l/Porto v10.cbz".into()));

        // un volume fuori dalla libreria: la cartella
        let dir = std::env::temp_dir().join(format!("fumetto-dopo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["cap 2.cbz", "cap 10.cbz", "cap 1.cbz", "note.txt"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        let alone = |p: &str, forward| neighbour(&[], &[], &dir.join(p), Order::Series, forward);
        assert_eq!(alone("cap 2.cbz", true), Some(dir.join("cap 10.cbz")));
        assert_eq!(alone("cap 10.cbz", true), None);
        assert_eq!(alone("cap 2.cbz", false), Some(dir.join("cap 1.cbz")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn le_cartelle_in_ordine() {
        let p = |s: &str| PathBuf::from(s);
        let mut paths = vec![
            p("/f/Moebius/Arzach.cbz"),
            p("/f/Manga/Berserk/Vol 10/Ch 2.cbz"),
            p("/f/Manga/Berserk/Vol 2/Ch 10.cbz"),
            p("/f/Manga/Berserk/Vol 2/Ch 9.cbz"),
            p("/f/Sciolto.cbz"),
            p("/f/Moebius/Incal/Incal 1.cbz"),
            p("/f/Manga/Akira.cbz"),
        ];
        paths.sort_by(|a, b| folder_cmp(a, b));
        assert_eq!(
            paths,
            [
                p("/f/Manga/Berserk/Vol 2/Ch 9.cbz"),
                p("/f/Manga/Berserk/Vol 2/Ch 10.cbz"),
                p("/f/Manga/Berserk/Vol 10/Ch 2.cbz"),
                p("/f/Manga/Akira.cbz"),
                p("/f/Moebius/Incal/Incal 1.cbz"),
                p("/f/Moebius/Arzach.cbz"),
                p("/f/Sciolto.cbz"),
            ],
            "prima le cartelle, poi i volumi; 2 prima di 10"
        );

        let entries: Vec<Entry> = paths.iter().map(|path| entry(path.clone())).collect();
        let all: Vec<usize> = (0..entries.len()).collect();
        let top = folder_contents(&entries, &all, Path::new("/f"));
        let names: Vec<_> = top.folders.iter().map(|(f, m)| (f.clone(), m.len())).collect();
        assert_eq!(names, [(p("/f/Manga"), 4), (p("/f/Moebius"), 2)]);
        assert_eq!(top.folders[0].1[0], 0, "la copertina: il primo da leggere");
        assert_eq!(top.volumes, [6]);
        let manga = folder_contents(&entries, &all, Path::new("/f/Manga"));
        assert_eq!(manga.folders.len(), 1);
        assert_eq!(manga.volumes, [3]);
        assert_eq!(folder_contents(&entries, &[6], Path::new("/f/Manga")), FolderContents::default());
    }

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

    #[test]
    fn le_cartelle_degli_autori_sono_raccolte() {
        let root = std::env::temp_dir().join(format!("fumetto-libreria-autori-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mk = |p: &str| {
            let p = root.join(p);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"x").unwrap();
        };
        // un autore: due opere sciolte e una serie in una sua cartella
        mk("Moebius/Arzach.cbz");
        mk("Moebius/Il garage ermetico.cbz");
        mk("Moebius/Incal/Incal v01.cbz");
        mk("Moebius/Incal/Incal v02.cbz");
        // una serie senza numeri: i titoli cominciano con il nome della cartella
        mk("Akira/Akira parte uno.cbz");
        mk("Akira/Akira parte due.cbz");
        // una cartella che ordina: due serie
        mk("Manga/One Piece/One Piece v01.cbz");
        mk("Manga/Berserk/Berserk v01.cbz");
        // un volume da solo nella sua cartella: niente raccolta
        mk("Sciolti/Unico.cbz");
        let found = scan(std::slice::from_ref(&root));
        let get = |title: &str| found.iter().find(|e| e.title == title).unwrap();
        let collections = |title: &str| -> Vec<String> {
            get(title).groups.iter().filter(|g| g.kind == GroupKind::Collection).map(|g| g.name.clone()).collect()
        };
        assert_eq!(get("Arzach").series, None, "opere diverse non sono una serie");
        assert_eq!(get("Il garage ermetico").series, None);
        assert_eq!(collections("Arzach"), ["Moebius"]);
        assert_eq!(collections("Incal v02"), ["Moebius"], "anche la serie dentro la cartella dell'autore");
        assert_eq!(get("Akira parte uno").series.as_deref(), Some("Akira"));
        assert!(collections("Akira parte uno").is_empty(), "la cartella della serie non e' una raccolta");
        assert_eq!(collections("One Piece v01"), ["Manga"]);
        assert_eq!(collections("Berserk v01"), ["Manga"]);
        assert!(collections("Unico").is_empty() && get("Unico").series.is_none());
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
