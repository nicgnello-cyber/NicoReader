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

/// Copia un PDF di prova in un file suo (la password si ricorda per
/// percorso: ogni prova il suo).
fn copy(tag: &str, bytes: &[u8]) -> PathBuf {
    let path = std::env::temp_dir().join(format!("fumetto-{tag}-{}.pdf", std::process::id()));
    std::fs::write(&path, bytes).unwrap();
    path
}

fn red_at_center(book: &Book) -> bool {
    let fit = Fit::Contain { width: 100, height: 150 };
    let Content::Exact(page, _) = book.content(0, fit).unwrap() else { return false };
    pixel(&page, 50, 75) == [255, 0, 0, 255]
}

/// Fatti con qpdf da un PDF con una pagina rossa: AES-256 (i PDF di
/// oggi) con una password con le lettere accentate, RC4 a 128 bit (i
/// vecchi), e uno protetto solo per la stampa (si apre senza password).
#[test]
fn pdf_con_password() {
    for (tag, bytes, password) in [
        ("aes", &include_bytes!("../../prove/protetto-aes.pdf")[..], "segreto \u{e0}\u{e8}"),
        ("rc4", &include_bytes!("../../prove/protetto-rc4.pdf")[..], "vecchia"),
    ] {
        let path = copy(&format!("password-{tag}"), bytes);
        assert!(matches!(Book::open(&path), Err(Error::Password)), "{tag}: senza password");
        assert!(matches!(Book::open_with_password(&path, "sbagliata"), Err(Error::Password)), "{tag}");
        assert!(matches!(Book::open(&path), Err(Error::Password)), "{tag}: la sbagliata non si ricorda");
        let book = Book::open_with_password(&path, password).unwrap();
        assert!(red_at_center(&book), "{tag}: la pagina si legge");
        // copertine e miniature riaprono il file: la password si ricorda
        assert!(red_at_center(&Book::open(&path).unwrap()), "{tag}: riaperto senza chiederla");
        forget(&path);
        let _ = std::fs::remove_file(path);
    }
    let path = copy("solo-permessi", include_bytes!("../../prove/solo-permessi.pdf"));
    assert!(red_at_center(&Book::open(&path).unwrap()), "protetto solo per la stampa: si apre");
    let _ = std::fs::remove_file(path);
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
        (
            "/Type /Page /Parent 2 0 R /MediaBox [0 0 40 60] \
              /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R",
            None,
        ),
        ("", Some(b"q 40 0 0 60 0 0 cm /Im0 Do Q")),
        ("/Type /XObject /Subtype /Image /Width 4 /Height 6 /ColorSpace /DeviceRGB /BitsPerComponent 8", Some(&rgb)),
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
        (
            "/Type /Page /Parent 2 0 R /MediaBox [0 0 160 240] \
              /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R",
            None,
        ),
        ("", Some(b"q 160 0 0 240 0 0 cm /Im0 Do Q")),
        (
            "/Type /XObject /Subtype /Image /Width 16 /Height 24 /ColorSpace /DeviceRGB \
              /BitsPerComponent 8 /Filter /DCTDecode",
            Some(&data),
        ),
    ]);
    let (book, path) = open("jpeg", &bytes);
    let Content::Encoded(raw) = book.content(0, Fit::Width(100)).unwrap() else {
        panic!("doveva dare i byte del JPEG")
    };
    assert_eq!(raw, data);
    let _ = std::fs::remove_file(path);
}
