//! Da dove vengono le pagine: una cartella, un CBZ, un CBR.
//!
//! Un [`Book`] conosce i nomi delle sue pagine, gia' in ordine di lettura, e ne
//! legge i byte da qualsiasi thread: la lettura anticipata ne usa diversi.

use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::comicinfo::{self, ComicInfo, is_comic_info};
use crate::decode::Page;
use crate::lingua::t;
use crate::loader::Fit;
use crate::natural::natural_cmp;
use crate::pdf::PdfDoc;

const IMAGE_EXT: &[&str] = &["jpg", "jpeg", "jpe", "jfif", "png", "gif", "bmp", "webp", "tif", "tiff", "avif", "jxl"];

pub fn is_image(name: &str) -> bool {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    !file.starts_with('.')
        && file.rsplit_once('.').is_some_and(|(_, ext)| IMAGE_EXT.iter().any(|e| ext.eq_ignore_ascii_case(e)))
}

#[derive(Debug)]
pub enum Error {
    NotFound,
    NoImages,
    Unsupported,
    Io(io::Error),
    Archive(String),
    Pdf(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::NotFound => f.write_str(t(
                "Non si trova: forse è stato spostato o rinominato.",
                "It can't be found: it may have been moved or renamed.",
            )),
            Error::NoImages => f.write_str(t("Non contiene immagini.", "It contains no images.")),
            Error::Unsupported => f.write_str(t(
                "Formato non riconosciuto. NicoReader legge CBZ, CBR, CB7, CBT, PDF, cartelle di immagini e immagini singole.",
                "Unrecognized format. NicoReader reads CBZ, CBR, CB7, CBT, PDF, image folders and single images.",
            )),
            Error::Io(e) => write!(f, "{e}"),
            Error::Archive(e) => write!(f, "{} ({e})", t("Archivio rovinato o illeggibile", "Damaged or unreadable archive")),
            Error::Pdf(e) => write!(f, "PDF: {e}"),
        }
    }
}

/// Una pagina, nella forma in cui la da' il volume.
pub enum Content {
    /// I byte di un'immagine, da decodificare (tutti i formati tranne il PDF).
    Encoded(Vec<u8>),
    /// Pixel alla loro misura originale: la scansione dentro un PDF. Da
    /// rimpicciolire come qualsiasi altra pagina.
    Pixels(Page),
    /// Pixel gia' alla misura voluta (una pagina PDF vettoriale, disegnata
    /// apposta), con la misura di riferimento della pagina.
    Exact(Page, (u32, u32)),
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

enum Store {
    Folder(PathBuf),
    Zip(Mutex<zip::ZipArchive<File>>),
    /// RAR non solido: ogni lettura apre il suo handle e salta le intestazioni
    /// fino alla pagina, senza decomprimere le altre. Thread-safe senza lock.
    Rar(PathBuf),
    /// RAR solido e 7z (quasi sempre solido): saltare costerebbe la
    /// decompressione di tutto cio' che precede, quindi si legge tutto una
    /// volta sola all'apertura.
    // ponytail: tiene l'archivio intero in RAM; per i solidi molto grandi
    // andrebbe una cartella temporanea
    Memory(HashMap<String, Vec<u8>>),
    /// TAR: non compresso, ogni pagina e' un tratto del file. Si legge da li'.
    Tar(PathBuf, HashMap<String, (u64, u64)>),
    Pdf(PdfDoc),
}

pub struct Book {
    /// Il volume: l'archivio o la cartella (anche se si e' aperta un'immagine sola).
    pub path: PathBuf,
    pub title: String,
    /// Nomi delle pagine in ordine di lettura.
    pub names: Vec<String>,
    /// Pagina da cui partire: se si apre una singola immagine, e' quella.
    pub start_at: usize,
    /// La ComicInfo.xml dentro il volume, se c'e'.
    pub info: Option<ComicInfo>,
    store: Store,
}

/// Il nome di un volume per chi legge: il file senza estensione, la cartella
/// com'e' (in "Vol.01 Ch.001 - Screw" il ".001 - Screw" non e' un'estensione).
pub fn title_of(path: &Path) -> String {
    let name = if path.is_dir() { path.file_name() } else { path.file_stem() };
    name.unwrap_or(path.as_os_str()).to_string_lossy().into_owned()
}

impl Book {
    pub fn open(path: &Path) -> Result<Book, Error> {
        if !path.exists() {
            return Err(Error::NotFound);
        }
        let title = title_of(path);
        let (names, store) = if path.is_dir() {
            (folder_images(path)?, Store::Folder(path.to_owned()))
        } else {
            match sniff(path)? {
                Kind::Zip => open_zip(path)?,
                Kind::Rar => open_rar(path)?,
                Kind::SevenZ => open_7z(path)?,
                Kind::Tar => open_tar(path)?,
                Kind::Pdf => {
                    let doc = PdfDoc::open(path)?;
                    ((1..=doc.len()).map(|n| n.to_string()).collect(), Store::Pdf(doc))
                }
                Kind::Other if is_image(&path.to_string_lossy()) => {
                    // un'immagine sola: si sfoglia tutta la sua cartella, partendo da lei
                    let dir = path.parent().unwrap_or(Path::new("."));
                    let mut book = Book::open(dir)?;
                    let me = path.file_name().map(|n| n.to_string_lossy().into_owned());
                    book.start_at = book.names.iter().position(|n| Some(n) == me.as_ref()).unwrap_or(0);
                    return Ok(book);
                }
                Kind::Other => return Err(Error::Unsupported),
            }
        };
        if names.is_empty() {
            return Err(Error::NoImages);
        }
        let info = match &store {
            // RAR solidi e 7z: la scheda e' gia' in memoria con le pagine
            Store::Memory(files) => files.iter().find(|(n, _)| is_comic_info(n)).and_then(|(_, b)| ComicInfo::parse(b)),
            Store::Pdf(_) => None,
            _ => read_info(path),
        };
        Ok(Book { path: path.to_owned(), title, names, start_at: 0, info, store })
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// La pagina `index`, per stare in `fit`: di solito i byte da decodificare,
    /// per i PDF pixel gia' pronti (vedi `Content`).
    pub fn content(&self, index: usize, fit: Fit) -> Result<Content, Error> {
        self.content_turned(index, fit, false)
    }

    /// Come `content`, per una pagina che poi si girera' di un quarto di giro
    /// (`quarter`): conta solo per le pagine PDF disegnate, che devono stare in
    /// `fit` una volta girate.
    pub fn content_turned(&self, index: usize, fit: Fit, quarter: bool) -> Result<Content, Error> {
        match &self.store {
            Store::Pdf(doc) => doc.page(index, fit, quarter),
            _ => self.read(index).map(Content::Encoded),
        }
    }

    /// I primi `max` byte della pagina (o tutta, se e' piu' corta): quanto
    /// basta per leggerne la misura nell'intestazione, senza leggere il resto.
    /// `None` per i PDF, le cui pagine non sono immagini.
    pub fn head(&self, index: usize, max: usize) -> Option<Vec<u8>> {
        let name = &self.names[index];
        let mut out = Vec::new();
        match &self.store {
            Store::Pdf(_) => return None,
            Store::Folder(dir) => {
                File::open(dir.join(name)).ok()?.take(max as u64).read_to_end(&mut out).ok()?;
            }
            Store::Zip(zip) => {
                let mut zip = zip.lock().unwrap_or_else(|e| e.into_inner());
                zip.by_name(name).ok()?.take(max as u64).read_to_end(&mut out).ok()?;
            }
            Store::Memory(files) => out.extend_from_slice(&files[name][..files[name].len().min(max)]),
            Store::Tar(..) | Store::Rar(_) => {
                // il TAR legge solo il suo tratto; il RAR non sa fermarsi a meta'
                out = self.read(index).ok()?;
                out.truncate(max);
            }
        }
        Some(out)
    }

    /// La misura di qualche pagina sparsa per il volume, dalle intestazioni:
    /// (pagina, larghezza, altezza). Le pagine che non si misurano mancano.
    pub fn sample_sizes(&self, count: usize) -> Vec<(usize, (u32, u32))> {
        let n = self.len();
        let count = count.min(n);
        (0..count)
            .map(|k| if count > 1 { k * (n - 1) / (count - 1) } else { 0 })
            .filter_map(|i| {
                // 256 KB: un JPEG con dentro una miniatura EXIF o un profilo colore
                // tiene la misura anche a qualche decina di KB dall'inizio
                let head = self.head(i, 256 << 10)?;
                crate::decode::dimensions(&head).map(|size| (i, size))
            })
            .collect()
    }

    /// I byte della pagina, cosi' come sono nell'archivio (ancora da decodificare).
    /// Non vale per i PDF, le cui pagine non sono immagini: vedi `content`.
    pub fn read(&self, index: usize) -> Result<Vec<u8>, Error> {
        let name = &self.names[index];
        match &self.store {
            Store::Pdf(_) => Err(Error::Pdf("le pagine di un PDF si leggono con content()".into())),
            Store::Folder(dir) => Ok(std::fs::read(dir.join(name))?),
            Store::Zip(zip) => {
                let mut zip = zip.lock().unwrap_or_else(|e| e.into_inner());
                let mut f = zip.by_name(name).map_err(|e| Error::Archive(e.to_string()))?;
                let mut out = Vec::with_capacity(f.size() as usize);
                f.read_to_end(&mut out)?;
                Ok(out)
            }
            Store::Rar(path) => rar_read(path, name),
            Store::Memory(files) => Ok(files[name].clone()),
            Store::Tar(path, index) => {
                use std::io::{Seek, SeekFrom};
                let (offset, size) = index[name];
                // un file aperto per lettura: i thread non si pestano i piedi
                let mut f = File::open(path)?;
                f.seek(SeekFrom::Start(offset))?;
                let mut out = vec![0; size as usize];
                f.read_exact(&mut out)?;
                Ok(out)
            }
        }
    }
}

enum Kind {
    Zip,
    Rar,
    SevenZ,
    Tar,
    Pdf,
    Other,
}

/// Il tipo vero dai primi byte: molti .cbr sono zip travestiti, e viceversa.
fn sniff(path: &Path) -> Result<Kind, Error> {
    let mut head = [0u8; 262];
    let n = File::open(path)?.read(&mut head)?;
    let head = &head[..n];
    Ok(match head {
        [b'P', b'K', ..] => Kind::Zip,
        [b'R', b'a', b'r', b'!', ..] => Kind::Rar,
        [b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C, ..] => Kind::SevenZ,
        // "%PDF-" di solito in testa, ma lo standard tollera qualche byte prima
        _ if head.windows(5).any(|w| w == b"%PDF-") => Kind::Pdf,
        // la firma del tar sta a 257 byte dall'inizio
        _ if head.get(257..262) == Some(b"ustar") => Kind::Tar,
        _ => Kind::Other,
    })
}

fn sort(names: &mut [String]) {
    names.sort_by(|a, b| natural_cmp(a, b));
}

/// Immagini a qualsiasi profondita' (manga/AUTORE/Titolo/capitolo/pagine),
/// con il percorso relativo alla cartella.
fn folder_images(root: &Path) -> Result<Vec<String>, Error> {
    let mut names = Vec::new();
    let mut stack = vec![root.to_owned()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                stack.push(path);
            } else if let Ok(rel) = path.strip_prefix(root) {
                let rel = rel.to_string_lossy().into_owned();
                if is_image(&rel) {
                    names.push(rel);
                }
            }
        }
    }
    sort(&mut names);
    Ok(names)
}

fn open_zip(path: &Path) -> Result<(Vec<String>, Store), Error> {
    let zip = zip::ZipArchive::new(File::open(path)?).map_err(|e| Error::Archive(e.to_string()))?;
    let mut names: Vec<String> = zip.file_names().filter(|n| is_image(n)).map(str::to_owned).collect();
    sort(&mut names);
    Ok((names, Store::Zip(Mutex::new(zip))))
}

fn open_7z(path: &Path) -> Result<(Vec<String>, Store), Error> {
    let err = |e: sevenz_rust2::Error| Error::Archive(e.to_string());
    let mut archive = sevenz_rust2::ArchiveReader::open(path, sevenz_rust2::Password::empty()).map_err(err)?;
    let mut files = HashMap::new();
    // un archivio troncato si ferma all'errore: le pagine lette fin li' restano
    let _ = archive.for_each_entries(|entry, reader| {
        let wanted = is_image(entry.name()) || (is_comic_info(entry.name()) && entry.size() <= comicinfo::MAX_BYTES);
        if !entry.is_directory() && wanted {
            let mut bytes = Vec::with_capacity(entry.size() as usize);
            reader.read_to_end(&mut bytes)?;
            files.insert(entry.name().to_owned(), bytes);
        }
        Ok(true)
    });
    let mut names: Vec<String> = files.keys().filter(|n| is_image(n)).cloned().collect();
    sort(&mut names);
    Ok((names, Store::Memory(files)))
}

fn open_tar(path: &Path) -> Result<(Vec<String>, Store), Error> {
    let mut archive = tar::Archive::new(File::open(path)?);
    let mut index = HashMap::new();
    for entry in archive.entries()?.flatten() {
        let name = entry.path().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        if entry.header().entry_type().is_file() && is_image(&name) {
            index.insert(name, (entry.raw_file_position(), entry.size()));
        }
    }
    let mut names: Vec<String> = index.keys().cloned().collect();
    sort(&mut names);
    Ok((names, Store::Tar(path.to_owned(), index)))
}

// Il sorgente C++ di UnRAR usa il registro e il generatore casuale di Windows,
// che stanno in advapi32: unrar_sys non lo dichiara, e il linker se ne accorge
// solo quando quelle funzioni finiscono nell'eseguibile.
#[cfg(windows)]
#[link(name = "advapi32")]
unsafe extern "C" {}

fn rar_err(e: unrar::error::UnrarError) -> Error {
    Error::Archive(e.to_string())
}

fn open_rar(path: &Path) -> Result<(Vec<String>, Store), Error> {
    let listing = unrar::Archive::new(path).open_for_listing().map_err(rar_err)?;
    if !listing.is_solid() {
        let mut names = Vec::new();
        // un archivio troncato si ferma alla prima intestazione rotta:
        // le pagine lette fin li' restano sfogliabili
        for entry in listing.flatten() {
            let name = entry.filename.to_string_lossy().into_owned();
            if entry.is_file() && is_image(&name) {
                names.push(name);
            }
        }
        sort(&mut names);
        return Ok((names, Store::Rar(path.to_owned())));
    }
    drop(listing);
    let mut files = HashMap::new();
    let mut cursor = unrar::Archive::new(path).open_for_processing().map_err(rar_err)?;
    while let Ok(Some(header)) = cursor.read_header() {
        let name = header.entry().filename.to_string_lossy().into_owned();
        let info = is_comic_info(&name) && header.entry().unpacked_size <= comicinfo::MAX_BYTES;
        cursor = if header.entry().is_file() && (is_image(&name) || info) {
            match header.read() {
                Ok((bytes, next)) => {
                    files.insert(name, bytes);
                    next
                }
                Err(_) => break,
            }
        } else {
            match header.skip() {
                Ok(next) => next,
                Err(_) => break,
            }
        };
    }
    let mut names: Vec<String> = files.keys().filter(|n| is_image(n)).cloned().collect();
    sort(&mut names);
    Ok((names, Store::Memory(files)))
}

/// La ComicInfo.xml di un volume senza aprirlo tutto: nelle cartelle accanto
/// alle pagine, negli archivi dove si legge da sola (CBZ, CBT, CBR non solidi).
/// Dai CB7 e dai CBR solidi si avrebbe solo decomprimendo cio' che le sta
/// davanti, cioe' quasi tutto: per loro `None`, la si legge con le pagine.
pub fn read_info(path: &Path) -> Option<ComicInfo> {
    let bytes = if path.is_dir() {
        let file = std::fs::read_dir(path).ok()?.flatten().find(|e| is_comic_info(&e.file_name().to_string_lossy()))?;
        if file.metadata().ok()?.len() > comicinfo::MAX_BYTES {
            return None;
        }
        std::fs::read(file.path()).ok()?
    } else {
        match sniff(path).ok()? {
            Kind::Zip => {
                let mut zip = zip::ZipArchive::new(File::open(path).ok()?).ok()?;
                // la piu' in alto, se ce n'e' piu' d'una
                let name = zip.file_names().filter(|n| is_comic_info(n)).min_by_key(|n| n.len())?.to_owned();
                let f = zip.by_name(&name).ok()?;
                read_limited(f)?
            }
            Kind::Tar => {
                let mut archive = tar::Archive::new(File::open(path).ok()?);
                let entry = archive.entries_with_seek().ok()?.flatten().find(|e| {
                    e.header().entry_type().is_file() && e.path().is_ok_and(|p| is_comic_info(&p.to_string_lossy()))
                })?;
                read_limited(entry)?
            }
            Kind::Rar => {
                let listing = unrar::Archive::new(path).open_for_listing().ok()?;
                if listing.is_solid() {
                    return None;
                }
                let name = listing.flatten().find_map(|e| {
                    let name = e.filename.to_string_lossy().into_owned();
                    (e.is_file() && is_comic_info(&name) && e.unpacked_size <= comicinfo::MAX_BYTES).then_some(name)
                })?;
                rar_read(path, &name).ok()?
            }
            Kind::SevenZ | Kind::Pdf | Kind::Other => return None,
        }
    };
    ComicInfo::parse(&bytes)
}

fn read_limited(f: impl Read) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    f.take(comicinfo::MAX_BYTES + 1).read_to_end(&mut out).ok()?;
    (out.len() as u64 <= comicinfo::MAX_BYTES).then_some(out)
}

fn rar_read(path: &Path, want: &str) -> Result<Vec<u8>, Error> {
    let mut cursor = unrar::Archive::new(path).open_for_processing().map_err(rar_err)?;
    while let Some(header) = cursor.read_header().map_err(rar_err)? {
        if header.entry().filename.to_string_lossy() == want {
            return header.read().map(|(bytes, _)| bytes).map_err(rar_err);
        }
        cursor = header.skip().map_err(rar_err)?;
    }
    Err(Error::Archive(format!("{want} {}", t("non è più nell'archivio", "is no longer in the archive"))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn il_titolo_di_una_cartella_tiene_i_punti() {
        let dir = std::env::temp_dir()
            .join(format!("fumetto-test-titolo-{}", std::process::id()))
            .join("Vol.01 Ch.001 - Screw");
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(title_of(&dir), "Vol.01 Ch.001 - Screw");
        assert_eq!(title_of(Path::new("Nebbia sul Porto v03.cbz")), "Nebbia sul Porto v03");
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    /// Tre pagine PNG in una cartella temporanea tutta per il test.
    fn pages_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fumetto-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (i, name) in ["p10.png", "p2.png", "p1.png"].iter().enumerate() {
            image::RgbaImage::from_pixel(4, 6, image::Rgba([i as u8 * 50, 0, 0, 255])).save(dir.join(name)).unwrap();
        }
        dir
    }

    fn check(book: &Book) {
        assert_eq!(book.names, ["p1.png", "p2.png", "p10.png"]);
        let page = crate::decode(&book.read(2).unwrap()).unwrap();
        assert_eq!((page.width, page.height, page.rgba[0]), (4, 6, 0)); // p10 era la prima
    }

    #[test]
    fn cartella_tar_e_7z() {
        let dir = pages_dir("formati");
        check(&Book::open(&dir).unwrap());

        let tar_path = dir.with_extension("cbt");
        let mut builder = tar::Builder::new(File::create(&tar_path).unwrap());
        builder.append_dir_all(".", &dir).unwrap();
        builder.finish().unwrap();
        drop(builder);
        check(&Book::open(&tar_path).unwrap());

        let sevenz_path = dir.with_extension("cb7");
        sevenz_rust2::compress_to_path(&dir, &sevenz_path).unwrap();
        check(&Book::open(&sevenz_path).unwrap());

        for p in [&tar_path, &sevenz_path] {
            let _ = std::fs::remove_file(p);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn la_scheda_in_ogni_formato() {
        let dir = pages_dir("scheda");
        let xml = "<?xml version=\"1.0\"?><ComicInfo><Series>Orbita Bassa</Series><Number>3</Number>\
                   <Manga>YesAndRightToLeft</Manga></ComicInfo>";
        std::fs::write(dir.join("ComicInfo.xml"), xml).unwrap();
        let has_info = |book: &Book| {
            check(book); // la scheda non e' una pagina
            let info = book.info.as_ref().expect("scheda letta");
            assert_eq!((info.series.as_deref(), info.right_to_left), (Some("Orbita Bassa"), Some(true)));
        };
        has_info(&Book::open(&dir).unwrap());

        let tar_path = dir.with_extension("cbt");
        let mut builder = tar::Builder::new(File::create(&tar_path).unwrap());
        builder.append_dir_all(".", &dir).unwrap();
        builder.finish().unwrap();
        drop(builder);
        has_info(&Book::open(&tar_path).unwrap());

        let zip_path = dir.with_extension("cbz");
        let mut zip = zip::ZipWriter::new(File::create(&zip_path).unwrap());
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for name in ["ComicInfo.xml", "p1.png", "p2.png", "p10.png"] {
            zip.start_file(name, stored).unwrap();
            io::Write::write_all(&mut zip, &std::fs::read(dir.join(name)).unwrap()).unwrap();
        }
        zip.finish().unwrap();
        has_info(&Book::open(&zip_path).unwrap());

        let sevenz_path = dir.with_extension("cb7");
        sevenz_rust2::compress_to_path(&dir, &sevenz_path).unwrap();
        has_info(&Book::open(&sevenz_path).unwrap());

        // senza aprirli tutti: il 7z si dovrebbe decomprimere, si salta
        for p in [&dir, &tar_path, &zip_path] {
            assert_eq!(read_info(p).and_then(|i| i.number).as_deref(), Some("3"), "{}", p.display());
        }
        assert_eq!(read_info(&sevenz_path), None);

        for p in [&tar_path, &zip_path, &sevenz_path] {
            let _ = std::fs::remove_file(p);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn riconosce_le_immagini() {
        assert!(is_image("a/b/p01.JPG"));
        assert!(is_image("p.webp"));
        assert!(!is_image("ComicInfo.xml"));
        assert!(!is_image("__MACOSX/._p01.jpg"));
        assert!(!is_image("jpg"));
    }
}
