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

mod order;
mod scan;
mod series;
#[cfg(test)]
mod tests;

pub use order::{
    FolderContents, Order, by_number, first_in_folder_order, folder_cmp, folder_contents, neighbour, origin, root_of,
};
use scan::is_volume_file;
pub use scan::{scan, scan_cached};
pub use series::parse_series;
use series::{folder_collections, group_by_folder, split_same_name_series};

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
