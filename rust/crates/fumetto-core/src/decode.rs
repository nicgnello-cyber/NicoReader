//! Dai byte dell'archivio ai pixel.
//!
//! La pagina si decodifica sempre a piena risoluzione: il rimpicciolimento lo
//! fa `resize`, in luce lineare e con un filtro vero. La decodifica ridotta di
//! libjpeg o di libwebp sarebbe piu' veloce, ma filtra in modo approssimato e
//! in spazio gamma: proprio cio' che scurisce i retini e genera il moire'.

use std::fmt;

use crate::lingua::t;

/// Pixel RGBA a 8 bit, codificati sRGB, alfa non premoltiplicata.
pub struct Page {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// Nessun pixel trasparente: il ricampionamento puo' ignorare l'alfa.
    pub opaque: bool,
}

impl Page {
    pub fn bytes(&self) -> usize {
        self.rgba.len()
    }
}

/// Tetto di sicurezza: un archivio malevolo potrebbe dichiarare una pagina da
/// 100.000 x 100.000 e chiedere 40 GB. 64 megapixel sono 256 MB, come in Python.
pub const MAX_PIXELS: u64 = 64_000_000;

#[derive(Debug)]
pub enum DecodeError {
    TooLarge(u32, u32),
    Invalid(String),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            DecodeError::TooLarge(w, h) => write!(f, "{}: {w} x {h}", t("pagina troppo grande", "page too large")),
            DecodeError::Invalid(e) => write!(f, "{}: {e}", t("immagine illeggibile", "unreadable image")),
        }
    }
}

impl std::error::Error for DecodeError {}

pub fn decode(data: &[u8]) -> Result<Page, DecodeError> {
    if data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        return decode_webp(data);
    }
    if crate::avif::is_avif(data) {
        return crate::avif::decode(data);
    }
    if is_jxl(data) {
        return decode_jxl(data);
    }
    let invalid = |e: image::ImageError| DecodeError::Invalid(e.to_string());
    let reader = || {
        image::ImageReader::new(std::io::Cursor::new(data))
            .with_guessed_format()
            .map_err(|e| DecodeError::Invalid(e.to_string()))
    };
    // le dimensioni dall'intestazione, prima di allocare i pixel
    let (w, h) = reader()?.into_dimensions().map_err(invalid)?;
    check_size(w, h)?;
    let img = reader()?.decode().map_err(invalid)?;
    let opaque = !img.color().has_alpha();
    let img = img.into_rgba8();
    Ok(Page { width: img.width(), height: img.height(), rgba: img.into_raw(), opaque })
}

/// La misura dall'intestazione, senza decodificare: bastano i primi byte.
pub fn dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        let mut info = libwebp_sys::WebPBitstreamFeatures {
            width: 0,
            height: 0,
            has_alpha: 0,
            has_animation: 0,
            format: 0,
            pad: [0; 5],
        };
        // SAFETY: libwebp legge solo `data.len()` byte e scrive nella struttura.
        let status = unsafe { libwebp_sys::WebPGetFeatures(data.as_ptr(), data.len(), &mut info) };
        return (status == libwebp_sys::VP8StatusCode::VP8_STATUS_OK)
            .then_some((info.width as u32, info.height as u32));
    }
    image::ImageReader::new(std::io::Cursor::new(data)).with_guessed_format().ok()?.into_dimensions().ok()
}

pub(crate) fn check_size(w: u32, h: u32) -> Result<(), DecodeError> {
    if w == 0 || h == 0 || w as u64 * h as u64 > MAX_PIXELS {
        return Err(DecodeError::TooLarge(w, h));
    }
    Ok(())
}

/// JPEG XL: il flusso nudo (FF 0A) o il contenitore ISO.
fn is_jxl(data: &[u8]) -> bool {
    const CONTAINER: &[u8] = &[0, 0, 0, 0x0C, b'J', b'X', b'L', b' ', 0x0D, 0x0A, 0x87, 0x0A];
    data.starts_with(&[0xFF, 0x0A]) || data.starts_with(CONTAINER)
}

/// Con jxl-oxide, tutto in Rust. I colori escono in sRGB (le immagini XYB si
/// convertono da sole; le altre sono gia' nel loro spazio, quasi sempre sRGB).
fn decode_jxl(data: &[u8]) -> Result<Page, DecodeError> {
    let invalid = |e: Box<dyn std::error::Error + Send + Sync>| DecodeError::Invalid(format!("JPEG XL: {e}"));
    let image = jxl_oxide::JxlImage::builder().read(std::io::Cursor::new(data)).map_err(invalid)?;
    let (w, h) = (image.width(), image.height());
    check_size(w, h)?;
    let render = image.render_frame(0).map_err(invalid)?;
    let mut stream = render.stream();
    let channels = stream.channels() as usize;
    let mut samples = vec![0u8; w as usize * h as usize * channels];
    stream.write_to_buffer(&mut samples);
    let rgba: Vec<u8> = match channels {
        1 => samples.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        2 => samples.as_chunks::<2>().0.iter().flat_map(|&[g, a]| [g, g, g, a]).collect(),
        3 => samples.as_chunks::<3>().0.iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect(),
        4 => samples,
        n => return Err(DecodeError::Invalid(format!("JPEG XL: {n} canali"))),
    };
    let opaque = channels % 2 == 1 || rgba.iter().skip(3).step_by(4).all(|&a| a == 255);
    Ok(Page { width: w, height: h, rgba, opaque })
}

fn decode_webp(data: &[u8]) -> Result<Page, DecodeError> {
    use libwebp_sys as webp;
    let mut info =
        webp::WebPBitstreamFeatures { width: 0, height: 0, has_alpha: 0, has_animation: 0, format: 0, pad: [0; 5] };
    // SAFETY: libwebp legge solo `data.len()` byte e scrive nella struttura.
    let status = unsafe { webp::WebPGetFeatures(data.as_ptr(), data.len(), &mut info) };
    if status != webp::VP8StatusCode::VP8_STATUS_OK {
        return Err(DecodeError::Invalid("intestazione WebP".into()));
    }
    let (w, h) = (info.width as u32, info.height as u32);
    check_size(w, h)?;
    let stride = w as usize * 4;
    let mut rgba = vec![0u8; stride * h as usize];
    // SAFETY: il buffer e' grande esattamente stride * h, come dichiarato.
    let out =
        unsafe { webp::WebPDecodeRGBAInto(data.as_ptr(), data.len(), rgba.as_mut_ptr(), rgba.len(), stride as i32) };
    if out.is_null() {
        return Err(DecodeError::Invalid("dati WebP".into()));
    }
    Ok(Page { width: w, height: h, rgba, opaque: info.has_alpha == 0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_minimo() {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let p = decode(&png).unwrap();
        assert_eq!((p.width, p.height), (3, 2));
        assert_eq!(&p.rgba[..4], &[10, 20, 30, 255]);
    }

    /// La misura si legge anche da un file tagliato a meta'.
    #[test]
    fn misura_dall_intestazione() {
        let mut jpeg = Vec::new();
        image::RgbImage::from_pixel(640, 2000, image::Rgb([200, 100, 50]))
            .write_to(&mut std::io::Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
            .unwrap();
        assert_eq!(dimensions(&jpeg[..jpeg.len() / 3]), Some((640, 2000)));
        assert_eq!(dimensions(b"niente"), None);
    }

    /// Due campiture, rossa e blu, senza perdita (fatte con libjxl).
    #[test]
    fn jpeg_xl() {
        let p = decode(include_bytes!("../prove/pagina.jxl")).unwrap();
        assert_eq!((p.width, p.height), (96, 64));
        assert_eq!(&p.rgba[..3], &[200, 40, 30]);
        let right = ((10 * 96 + 80) * 4) as usize;
        assert_eq!(&p.rgba[right..right + 3], &[30, 60, 190]);
    }

    #[test]
    fn spazzatura() {
        assert!(decode(b"non sono un'immagine").is_err());
        assert!(decode(b"RIFF\0\0\0\0WEBPxxxx").is_err());
    }
}
