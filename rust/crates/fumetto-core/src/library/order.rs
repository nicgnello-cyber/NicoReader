//! L'ordine: dei volumi in una serie, delle cartelle come su disco, il volume
//! prima e dopo, cosa c'e' in una cartella.

use super::*;

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
pub(super) fn pieces(p: &Path) -> impl Iterator<Item = (bool, std::path::Component<'_>)> {
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
