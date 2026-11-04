//! La scansione delle cartelle: i volumi, le loro schede (con la cache su
//! disco), la data in cui sono arrivati.

use super::*;

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

pub(super) fn scan_with(roots: &[PathBuf], info: &mut dyn FnMut(&Path) -> Option<ComicInfo>) -> Vec<Entry> {
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

pub(super) fn walk(root: &Path, dir: &Path, found: &mut Vec<Entry>) {
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

pub(super) fn is_volume_file(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| VOLUME_EXT.iter().any(|e| ext.eq_ignore_ascii_case(e)))
}

pub(super) fn entry(path: PathBuf) -> Entry {
    let title = title_of(&path);
    let (name, number) = parse_series(&title);
    Entry { series: number.map(|_| name), number, title, added: added_at(&path), path, ..Entry::default() }
}

/// Quando il volume e' arrivato: la data di creazione del file (quella della
/// copia sul disco), dove il sistema la tiene; se no l'ultima modifica.
pub(super) fn added_at(path: &Path) -> u64 {
    let Ok(meta) = std::fs::metadata(path) else { return 0 };
    let time = meta.created().or_else(|_| meta.modified());
    time.ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs())
}

/// Cio' che dice la ComicInfo.xml vince su cio' che si indovina dal nome. Una
/// scheda senza serie lascia la serie al nome o alla cartella.
pub(super) fn apply_info(e: &mut Entry, info: &ComicInfo) {
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
pub(super) struct InfoCache(HashMap<String, Option<ComicInfo>>);

impl InfoCache {
    /// Una cache che manca o non si legge e' una cache vuota.
    pub(super) fn load(file: &Path) -> InfoCache {
        let read = std::fs::read(file).ok().and_then(|b| serde_json::from_slice(&b).ok());
        InfoCache(read.unwrap_or_default())
    }

    /// La cache e' un di piu': se non si scrive, la prossima volta si rilegge.
    pub(super) fn save(&self, file: &Path) {
        let Ok(json) = serde_json::to_vec(&self.0) else { return };
        let tmp = file.with_extension("tmp");
        let _ = file
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|_| std::fs::write(&tmp, json))
            .and_then(|_| std::fs::rename(&tmp, file));
    }
}

pub(super) fn cache_key(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
    Some(format!("{}|{}|{modified}", abs.to_string_lossy(), meta.len()))
}
