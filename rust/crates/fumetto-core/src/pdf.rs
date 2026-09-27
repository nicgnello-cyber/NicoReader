//! I PDF, con pdfium: la libreria dei PDF di Chrome, in C++, usata attraverso
//! la sua interfaccia in C. Si carica solo quando si apre un PDF; senza, il
//! resto del lettore funziona lo stesso.
//!
//! Due strade, e la scelta conta per la qualita':
//! - una pagina che e' tutta un'immagine (la scansione di un fumetto, quasi
//!   sempre) si estrae con i suoi pixel originali e passa dal nostro
//!   rimpicciolimento in luce lineare, come un CBZ. Lasciarla disegnare a
//!   pdfium vorrebbe dire il suo filtro, in spazio gamma: il moire' sui retini;
//! - una pagina vera, fatta di testo e forme, la disegna pdfium direttamente
//!   alla misura dello schermo: un vettore non ha risoluzione, non c'e' nulla
//!   da rimpicciolire.
//!
//! pdfium non si puo' usare da piu' thread insieme: ogni chiamata passa da un
//! lucchetto solo.

use std::ffi::{CString, c_char, c_int, c_ulong, c_void};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::book::{Content, Error};
use crate::decode::{MAX_PIXELS, Page};
use crate::lingua::t;
use crate::loader::Fit;

/// La risoluzione di riferimento delle pagine vettoriali: la loro misura
/// "originale". Oltre, lo zoom lo fa la scheda video.
const DPI: f32 = 300.0;

type Handle = *mut c_void;

#[repr(C)]
struct SizeF {
    width: f32,
    height: f32,
}

#[repr(C)]
#[derive(Default)]
struct Matrix {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
}

const FPDF_ANNOT: c_int = 0x01;
const FPDF_REVERSE_BYTE_ORDER: c_int = 0x10;
const FPDF_PAGEOBJ_IMAGE: c_int = 3;

/// Le funzioni di pdfium che servono, lette dalla libreria una volta sola.
struct Api {
    load_document: unsafe extern "system" fn(*const c_char, *const c_char) -> Handle,
    close_document: unsafe extern "system" fn(Handle),
    page_count: unsafe extern "system" fn(Handle) -> c_int,
    page_size: unsafe extern "system" fn(Handle, c_int, *mut SizeF) -> c_int,
    last_error: unsafe extern "system" fn() -> c_ulong,
    load_page: unsafe extern "system" fn(Handle, c_int) -> Handle,
    close_page: unsafe extern "system" fn(Handle),
    page_rotation: unsafe extern "system" fn(Handle) -> c_int,
    count_objects: unsafe extern "system" fn(Handle) -> c_int,
    get_object: unsafe extern "system" fn(Handle, c_int) -> Handle,
    object_type: unsafe extern "system" fn(Handle) -> c_int,
    object_matrix: unsafe extern "system" fn(Handle, *mut Matrix) -> c_int,
    image_bitmap: unsafe extern "system" fn(Handle) -> Handle,
    image_filter_count: unsafe extern "system" fn(Handle) -> c_int,
    image_filter: unsafe extern "system" fn(Handle, c_int, *mut c_void, c_ulong) -> c_ulong,
    image_raw: unsafe extern "system" fn(Handle, *mut c_void, c_ulong) -> c_ulong,
    bitmap_create: unsafe extern "system" fn(c_int, c_int, c_int) -> Handle,
    bitmap_fill: unsafe extern "system" fn(Handle, c_int, c_int, c_int, c_int, c_ulong) -> c_int,
    render: unsafe extern "system" fn(Handle, Handle, c_int, c_int, c_int, c_int, c_int, c_int),
    bitmap_buffer: unsafe extern "system" fn(Handle) -> *mut c_void,
    bitmap_stride: unsafe extern "system" fn(Handle) -> c_int,
    bitmap_width: unsafe extern "system" fn(Handle) -> c_int,
    bitmap_height: unsafe extern "system" fn(Handle) -> c_int,
    bitmap_format: unsafe extern "system" fn(Handle) -> c_int,
    bitmap_destroy: unsafe extern "system" fn(Handle),
    // la libreria resta caricata finche' servono le funzioni sopra
    _lib: libloading::Library,
}

// SAFETY: sono puntatori a funzioni di una libreria che resta caricata; ogni
// chiamata passa dal lucchetto di `api()`.
unsafe impl Send for Api {}

const LIB_NAME: &str = if cfg!(windows) {
    "pdfium.dll"
} else if cfg!(target_os = "macos") {
    "libpdfium.dylib"
} else {
    "libpdfium.so"
};

/// La cartella in `vendor/pdfium`, con i nomi dei pacchetti di pdfium-binaries.
const PLATFORM: &str = if cfg!(target_os = "macos") {
    "mac-univ"
} else if cfg!(all(windows, target_arch = "aarch64")) {
    "win-arm64"
} else if cfg!(windows) {
    "win-x64"
} else if cfg!(target_arch = "aarch64") {
    "linux-arm64"
} else {
    "linux-x64"
};

/// Dove cercare pdfium: accanto al programma (come si distribuisce; su macOS
/// in Fumetto.app/Contents/Frameworks), poi dove dice FUMETTO_PDFIUM, poi
/// nella cartella `vendor` del progetto (chi compila dal sorgente).
fn candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_owned)) {
        if cfg!(target_os = "macos") {
            v.push(dir.join("../Frameworks").join(LIB_NAME));
        }
        v.push(dir.join(LIB_NAME));
    }
    if let Some(p) = std::env::var_os("FUMETTO_PDFIUM") {
        v.push(PathBuf::from(p));
    }
    v.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/pdfium").join(PLATFORM).join("bin").join(LIB_NAME));
    v
}

fn load() -> Result<Api, String> {
    let path = candidates().into_iter().find(|p| p.is_file())
        .ok_or_else(|| format!("{} {LIB_NAME} {}", t("Per i PDF serve", "PDFs need"), t("accanto al programma.", "next to the program.")))?;
    // SAFETY: e' pdfium, dalla nostra cartella; le firme vengono dai suoi
    // header (versione 156.0.8066) e i nomi sono quelli esportati.
    unsafe {
        let lib = libloading::Library::new(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        macro_rules! f {
            ($name:literal) => {
                *lib.get(concat!($name, "\0").as_bytes()).map_err(|e| format!("{}: {e}", $name))?
            };
        }
        let init: unsafe extern "system" fn() = f!("FPDF_InitLibrary");
        init();
        Ok(Api {
            load_document: f!("FPDF_LoadDocument"),
            close_document: f!("FPDF_CloseDocument"),
            page_count: f!("FPDF_GetPageCount"),
            page_size: f!("FPDF_GetPageSizeByIndexF"),
            last_error: f!("FPDF_GetLastError"),
            load_page: f!("FPDF_LoadPage"),
            close_page: f!("FPDF_ClosePage"),
            page_rotation: f!("FPDFPage_GetRotation"),
            count_objects: f!("FPDFPage_CountObjects"),
            get_object: f!("FPDFPage_GetObject"),
            object_type: f!("FPDFPageObj_GetType"),
            object_matrix: f!("FPDFPageObj_GetMatrix"),
            image_bitmap: f!("FPDFImageObj_GetBitmap"),
            image_filter_count: f!("FPDFImageObj_GetImageFilterCount"),
            image_filter: f!("FPDFImageObj_GetImageFilter"),
            image_raw: f!("FPDFImageObj_GetImageDataRaw"),
            bitmap_create: f!("FPDFBitmap_Create"),
            bitmap_fill: f!("FPDFBitmap_FillRect"),
            render: f!("FPDF_RenderPageBitmap"),
            bitmap_buffer: f!("FPDFBitmap_GetBuffer"),
            bitmap_stride: f!("FPDFBitmap_GetStride"),
            bitmap_width: f!("FPDFBitmap_GetWidth"),
            bitmap_height: f!("FPDFBitmap_GetHeight"),
            bitmap_format: f!("FPDFBitmap_GetFormat"),
            bitmap_destroy: f!("FPDFBitmap_Destroy"),
            _lib: lib,
        })
    }
}

/// pdfium, caricato alla prima richiesta, e il lucchetto che lo protegge.
fn api() -> Result<MutexGuard<'static, Api>, Error> {
    static PDFIUM: OnceLock<Result<Mutex<Api>, String>> = OnceLock::new();
    match PDFIUM.get_or_init(|| load().map(Mutex::new)) {
        Ok(m) => Ok(m.lock().unwrap_or_else(|e| e.into_inner())),
        Err(e) => Err(Error::Pdf(e.clone())),
    }
}

/// Un documento aperto e la misura delle sue pagine, in punti tipografici.
pub struct PdfDoc {
    handle: Handle,
    sizes: Vec<(f32, f32)>,
}

// SAFETY: il documento si usa solo tenendo il lucchetto di `api()`.
unsafe impl Send for PdfDoc {}
unsafe impl Sync for PdfDoc {}

impl PdfDoc {
    pub fn open(path: &Path) -> Result<PdfDoc, Error> {
        let api = api()?;
        let c_path = CString::new(path.to_string_lossy().as_bytes())
            .map_err(|_| Error::Pdf(t("percorso con un carattere nullo", "path with a null character").into()))?;
        // SAFETY: stringa UTF-8 terminata da zero, come chiede pdfium; nessuna password.
        let handle = unsafe { (api.load_document)(c_path.as_ptr(), std::ptr::null()) };
        if handle.is_null() {
            // SAFETY: nessun argomento; legge l'ultimo errore di questo thread.
            let code = unsafe { (api.last_error)() };
            let why = match code {
                2 => t("file non trovato o illeggibile", "file not found or unreadable"),
                3 => t("non è un PDF, o è rovinato", "not a PDF, or damaged"),
                4 => t("protetto da password", "password protected"),
                _ => t("errore di pdfium", "pdfium error"),
            };
            return Err(Error::Pdf(format!("{why} ({code})")));
        }
        // SAFETY: documento valido appena aperto.
        let count = unsafe { (api.page_count)(handle) }.max(0);
        let sizes = (0..count)
            .map(|i| {
                let mut s = SizeF { width: 0.0, height: 0.0 };
                // SAFETY: indice nei limiti, puntatore a una struttura nostra.
                unsafe { (api.page_size)(handle, i, &mut s) };
                (s.width.max(1.0), s.height.max(1.0))
            })
            .collect();
        Ok(PdfDoc { handle, sizes })
    }

    pub fn len(&self) -> usize {
        self.sizes.len()
    }

    /// La misura di riferimento di una pagina vettoriale, a 300 DPI.
    fn native(&self, index: usize) -> (u32, u32) {
        let (w, h) = self.sizes[index];
        let k = DPI / 72.0;
        (((w * k).round() as u32).max(1), ((h * k).round() as u32).max(1))
    }

    /// La pagina `index`, per stare in `fit`; `quarter`: dopo sara' girata di
    /// un quarto di giro, e a stare in `fit` deve essere la pagina girata.
    pub fn page(&self, index: usize, fit: Fit, quarter: bool) -> Result<Content, Error> {
        let api = api()?;
        // SAFETY: tutto quello che segue avviene con il lucchetto preso, su
        // oggetti aperti qui e chiusi prima di uscire.
        unsafe {
            let page = (api.load_page)(self.handle, index as c_int);
            if page.is_null() {
                return Err(Error::Pdf(format!("{} {} {}", t("pagina", "page"), index + 1, t("illeggibile", "unreadable"))));
            }
            let content = match scanned_image(&api, page, self.sizes[index]) {
                Some(c) => Ok(c),
                None => render(&api, page, self.native(index), fit, quarter),
            };
            (api.close_page)(page);
            content
        }
    }
}

impl Drop for PdfDoc {
    fn drop(&mut self) {
        if let Ok(api) = api() {
            // SAFETY: il documento e' nostro e non lo usa piu' nessuno.
            unsafe { (api.close_document)(self.handle) };
        }
    }
}

/// Se la pagina e' tutta una sola immagine (una scansione) la restituisce
/// com'e': un JPEG con i suoi byte originali, da decodificare fuori dal
/// lucchetto e in parallelo come la pagina di un CBZ; qualsiasi altro formato
/// con i pixel decodificati da pdfium. Solo il caso sicuro: un oggetto,
/// un'immagine, dritta, che copre almeno il 90% della pagina. Qualsiasi altra
/// cosa sulla pagina (un fumetto vettoriale, un testo) andrebbe persa, quindi
/// in quel caso si disegna.
unsafe fn scanned_image(api: &Api, page: Handle, (pw, ph): (f32, f32)) -> Option<Content> {
    unsafe {
        if (api.page_rotation)(page) != 0 || (api.count_objects)(page) != 1 {
            return None;
        }
        let obj = (api.get_object)(page, 0);
        if obj.is_null() || (api.object_type)(obj) != FPDF_PAGEOBJ_IMAGE {
            return None;
        }
        let mut m = Matrix::default();
        if (api.object_matrix)(obj, &mut m) == 0
            || m.b.abs() > 1e-3
            || m.c.abs() > 1e-3
            || m.a <= 0.0
            || m.d <= 0.0
            || m.a * m.d < 0.9 * pw * ph
        {
            return None;
        }
        if let Some(jpeg) = raw_jpeg(api, obj) {
            return Some(Content::Encoded(jpeg));
        }
        let bmp = (api.image_bitmap)(obj);
        if bmp.is_null() {
            return None;
        }
        let page = bitmap_to_page(api, bmp);
        (api.bitmap_destroy)(bmp);
        page.map(Content::Pixels)
    }
}

/// I byte originali dell'immagine, se e' un JPEG in grigio o RGB: l'unico
/// filtro e' DCTDecode, cioe' il flusso e' un file JPEG completo. CMYK e
/// formati rari restano a pdfium, che sa convertirli.
unsafe fn raw_jpeg(api: &Api, obj: Handle) -> Option<Vec<u8>> {
    unsafe {
        if (api.image_filter_count)(obj) != 1 {
            return None;
        }
        let mut name = [0u8; 32];
        let n = (api.image_filter)(obj, 0, name.as_mut_ptr().cast(), name.len() as c_ulong) as usize;
        if name.get(..n.saturating_sub(1)) != Some(b"DCTDecode".as_slice()) {
            return None;
        }
        let len = (api.image_raw)(obj, std::ptr::null_mut(), 0) as usize;
        if len == 0 {
            return None;
        }
        let mut data = vec![0u8; len];
        (api.image_raw)(obj, data.as_mut_ptr().cast(), len as c_ulong);
        matches!(jpeg_components(&data), Some(1 | 3)).then_some(data)
    }
}

/// Quanti canali ha un JPEG, dall'intestazione del fotogramma (SOF).
fn jpeg_components(data: &[u8]) -> Option<u8> {
    let mut i = 2; // dopo FF D8
    while i + 9 < data.len() {
        if data[i] != 0xFF {
            return None;
        }
        let marker = data[i + 1];
        let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        // SOF0..SOF15, tranne DHT (C4), JPG (C8) e DAC (CC)
        if (0xC0..=0xCF).contains(&marker) && ![0xC4, 0xC8, 0xCC].contains(&marker) {
            return Some(data[i + 9]);
        }
        i += 2 + len;
    }
    None
}

/// Disegna la pagina vettoriale alla misura esatta dello schermo (mai oltre
/// i 300 DPI: piu' in la' ingrandisce la scheda video), sul bianco della carta.
unsafe fn render(api: &Api, page: Handle, native: (u32, u32), fit: Fit, quarter: bool) -> Result<Content, Error> {
    let (mut w, mut h) = if quarter {
        let (a, b) = fit.size(native.1, native.0);
        (b, a)
    } else {
        fit.size(native.0, native.1)
    };
    if w > native.0 {
        (w, h) = native;
    }
    if w as u64 * h as u64 > MAX_PIXELS {
        return Err(Error::Pdf(format!("{}: {w} x {h}", t("pagina troppo grande", "page too large"))));
    }
    unsafe {
        let bmp = (api.bitmap_create)(w as c_int, h as c_int, 0);
        if bmp.is_null() {
            return Err(Error::Pdf(t("memoria insufficiente per la pagina", "not enough memory for the page").into()));
        }
        (api.bitmap_fill)(bmp, 0, 0, w as c_int, h as c_int, 0xFFFF_FFFF);
        // byte in ordine RGB invece di BGR: niente conversione dopo
        (api.render)(bmp, page, 0, 0, w as c_int, h as c_int, 0, FPDF_ANNOT | FPDF_REVERSE_BYTE_ORDER);
        let stride = (api.bitmap_stride)(bmp) as usize;
        let src = std::slice::from_raw_parts((api.bitmap_buffer)(bmp) as *const u8, stride * h as usize);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for row in src.chunks(stride) {
            for p in row[..w as usize * 4].as_chunks::<4>().0 {
                rgba.extend_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
        (api.bitmap_destroy)(bmp);
        Ok(Content::Exact(Page { width: w, height: h, rgba, opaque: true }, native))
    }
}

/// Da un bitmap di pdfium (grigio, BGR, BGRx o BGRA) a RGBA.
unsafe fn bitmap_to_page(api: &Api, bmp: Handle) -> Option<Page> {
    unsafe {
        let (w, h) = ((api.bitmap_width)(bmp), (api.bitmap_height)(bmp));
        if w <= 0 || h <= 0 || w as u64 * h as u64 > MAX_PIXELS {
            return None;
        }
        let (w, h) = (w as usize, h as usize);
        let stride = (api.bitmap_stride)(bmp) as usize;
        let src = std::slice::from_raw_parts((api.bitmap_buffer)(bmp) as *const u8, stride * h);
        let bytes = match (api.bitmap_format)(bmp) {
            1 => 1,     // grigio
            2 => 3,     // BGR
            3 | 4 => 4, // BGRx, BGRA
            _ => return None,
        };
        let alpha = (api.bitmap_format)(bmp) == 4;
        let mut rgba = Vec::with_capacity(w * h * 4);
        for row in src.chunks(stride) {
            for p in row[..w * bytes].chunks_exact(bytes) {
                rgba.extend_from_slice(&match bytes {
                    1 => [p[0], p[0], p[0], 255],
                    _ => [p[2], p[1], p[0], if alpha { p[3] } else { 255 }],
                });
            }
        }
        Some(Page { width: w as u32, height: h as u32, rgba, opaque: !alpha })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::Book;

    /// Un PDF scritto a mano: gli oggetti (dizionario, flusso facoltativo) in
    /// ordine a partire da 1, il catalogo e' il primo. Tabella xref vera.
    fn pdf(objects: &[(&str, Option<&[u8]>)]) -> Vec<u8> {
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (i, (dict, stream)) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            match stream {
                Some(data) => {
                    out.extend_from_slice(format!("<< {dict} /Length {} >>\nstream\n", data.len()).as_bytes());
                    out.extend_from_slice(data);
                    out.extend_from_slice(b"\nendstream");
                }
                None => out.extend_from_slice(format!("<< {dict} >>").as_bytes()),
            }
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
        for o in offsets {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes(),
        );
        out
    }

    fn open(tag: &str, bytes: &[u8]) -> (Book, PathBuf) {
        let path = std::env::temp_dir().join(format!("fumetto-{tag}-{}.pdf", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        (Book::open(&path).unwrap(), path)
    }

    fn pixel(p: &Page, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * p.width + x) * 4) as usize;
        [p.rgba[i], p.rgba[i + 1], p.rgba[i + 2], p.rgba[i + 3]]
    }

    #[test]
    fn pagina_vettoriale_disegnata_alla_misura_esatta() {
        let bytes = pdf(&[
            ("/Type /Catalog /Pages 2 0 R", None),
            ("/Type /Pages /Kids [3 0 R 5 0 R] /Count 2", None),
            ("/Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] /Contents 4 0 R", None),
            ("", Some(b"0 0 0 rg 20 20 160 260 re f")),
            ("/Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] /Contents 6 0 R", None),
            ("", Some(b"1 0 0 rg 0 0 200 300 re f")),
        ]);
        let (book, path) = open("vettoriale", &bytes);
        assert_eq!(book.names, ["1", "2"]);
        let fit = Fit::Contain { width: 100, height: 150 };
        let Content::Exact(page, native) = book.content(0, fit).unwrap() else { panic!("doveva disegnarla") };
        assert_eq!(native, (833, 1250), "200 x 300 punti a 300 DPI");
        assert_eq!((page.width, page.height), fit.size(native.0, native.1));
        assert_eq!(pixel(&page, 50, 75), [0, 0, 0, 255], "il rettangolo nero al centro");
        assert_eq!(pixel(&page, 2, 2), [255, 255, 255, 255], "il margine bianco della carta");
        let Content::Exact(red, _) = book.content(1, fit).unwrap() else { panic!() };
        assert_eq!(pixel(&red, 50, 75), [255, 0, 0, 255], "rosso: l'ordine RGB e' giusto");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn scansione_estratta_con_i_suoi_pixel() {
        let rgb: Vec<u8> = [10u8, 200, 30].repeat(4 * 6);
        let bytes = pdf(&[
            ("/Type /Catalog /Pages 2 0 R", None),
            ("/Type /Pages /Kids [3 0 R] /Count 1", None),
            ("/Type /Page /Parent 2 0 R /MediaBox [0 0 40 60] \
              /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R", None),
            ("", Some(b"q 40 0 0 60 0 0 cm /Im0 Do Q")),
            ("/Type /XObject /Subtype /Image /Width 4 /Height 6 /ColorSpace /DeviceRGB /BitsPerComponent 8",
             Some(&rgb)),
        ]);
        let (book, path) = open("scansione", &bytes);
        let Content::Pixels(page) = book.content(0, Fit::Contain { width: 1000, height: 1000 }).unwrap() else {
            panic!("doveva estrarre l'immagine")
        };
        assert_eq!((page.width, page.height), (4, 6), "i pixel originali, non ridisegnati");
        assert_eq!(pixel(&page, 1, 1), [10, 200, 30, 255]);
        let _ = std::fs::remove_file(path);
    }

    fn jpeg(color: image::ColorType) -> Vec<u8> {
        let mut out = Vec::new();
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 95);
        let pixels = vec![128u8; 16 * 24 * color.bytes_per_pixel() as usize];
        image::ImageEncoder::write_image(enc, &pixels, 16, 24, color.into()).unwrap();
        out
    }

    #[test]
    fn canali_del_jpeg() {
        assert_eq!(jpeg_components(&jpeg(image::ColorType::L8)), Some(1));
        assert_eq!(jpeg_components(&jpeg(image::ColorType::Rgb8)), Some(3));
        assert_eq!(jpeg_components(b"\xFF\xD8non e' un jpeg"), None);
    }

    /// La scansione JPEG dentro un PDF esce con i suoi byte: identici a quelli
    /// messi dentro, da decodificare come la pagina di un CBZ.
    #[test]
    fn scansione_jpeg_con_i_suoi_byte() {
        let data = jpeg(image::ColorType::Rgb8);
        let bytes = pdf(&[
            ("/Type /Catalog /Pages 2 0 R", None),
            ("/Type /Pages /Kids [3 0 R] /Count 1", None),
            ("/Type /Page /Parent 2 0 R /MediaBox [0 0 160 240] \
              /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R", None),
            ("", Some(b"q 160 0 0 240 0 0 cm /Im0 Do Q")),
            ("/Type /XObject /Subtype /Image /Width 16 /Height 24 /ColorSpace /DeviceRGB \
              /BitsPerComponent 8 /Filter /DCTDecode", Some(&data)),
        ]);
        let (book, path) = open("jpeg", &bytes);
        let Content::Encoded(raw) = book.content(0, Fit::Width(100)).unwrap() else {
            panic!("doveva dare i byte del JPEG")
        };
        assert_eq!(raw, data);
        let _ = std::fs::remove_file(path);
    }
}
