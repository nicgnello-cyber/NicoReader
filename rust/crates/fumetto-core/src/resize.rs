//! La pagina alla misura dello schermo, sul processore.
//!
//! Solo per *rimpicciolire*: e' li' che nasce il moire', perche' la pagina ha
//! piu' dettaglio di quanti pixel abbia lo schermo, e quel dettaglio va tolto
//! con un filtro vero invece di lasciarlo trasformare in disegni che non
//! esistono. Ingrandire non perde niente: lo fa la scheda video a ogni fotogramma.
//!
//! Lanczos a 3 lobi in luce lineare: i retini restano del loro tono invece di
//! scurirsi. Si fa nei thread della lettura anticipata, subito dopo la
//! decodifica, e alla scheda video arrivano solo i pixel che si vedranno.
//!
//! Due accorgimenti per la velocita', misurati sulla tavola di prova:
//! - prima di Lanczos si fa la media di blocchi 2x2 (o 4x4...) in luce lineare,
//!   nello stesso passaggio che converte da sRGB, finche' resta almeno un
//!   fattore 2 per il filtro (come il `reducing_gap` di Pillow);
//! - le pagine in bianco e nero (quasi tutti i manga) si filtrano su un canale
//!   invece di quattro.

use std::sync::OnceLock;

use fast_image_resize::images::{Image, ImageRef};
use fast_image_resize::{FilterType, PixelComponentMapper, PixelType, ResizeAlg, ResizeOptions, Resizer};

use crate::decode::Page;

fn srgb_mapper() -> &'static PixelComponentMapper {
    static MAPPER: OnceLock<PixelComponentMapper> = OnceLock::new();
    MAPPER.get_or_init(fast_image_resize::create_srgb_mapper)
}

/// Da sRGB a 8 bit a luce lineare a 16 bit. 8 bit lineari non basterebbero:
/// le ombre si riempirebbero di gradini.
fn to_linear_table() -> &'static [u16; 256] {
    static TABLE: OnceLock<[u16; 256]> = OnceLock::new();
    TABLE.get_or_init(|| {
        std::array::from_fn(|v| {
            let c = v as f64 / 255.0;
            let l = if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
            (l * 65535.0).round() as u16
        })
    })
}

/// Il fattore di scala che si lascia a Lanczos: sotto 2 la riduzione a blocchi
/// si vedrebbe (e' un filtro povero); sopra, Lanczos toglie quello che resta.
const REDUCING_GAP: f64 = 2.0;

/// Porta `page` a `width` x `height`, che devono essere piu' piccoli (o
/// uguali). `linear = false` filtra direttamente i valori sRGB: serve solo per
/// confronto, e' il modo in cui lo fanno quasi tutti.
pub fn resize(page: &Page, width: u32, height: u32, linear: bool) -> Page {
    if (width, height) == (page.width, page.height) {
        return Page { width, height, rgba: page.rgba.clone(), opaque: page.opaque };
    }
    let rgba = if !linear {
        let src = ImageRef::new(page.width, page.height, &page.rgba, PixelType::U8x4).expect("buffer coerente");
        let mut dst = Image::new(width, height, PixelType::U8x4);
        lanczos(&src, &mut dst, page.opaque);
        dst.into_vec()
    } else if !page.opaque {
        resize_with_alpha(page, width, height)
    } else {
        let scale = (page.width as f64 / width as f64).min(page.height as f64 / height as f64);
        let mut block = 1;
        while scale / (block * 2) as f64 >= REDUCING_GAP {
            block *= 2;
        }
        let lin = Linear::from_srgb(page, block);
        let pixel = if lin.gray { PixelType::U16 } else { PixelType::U16x4 };
        let src = ImageRef::new(lin.width, lin.height, bytemuck_u16(&lin.data), pixel).expect("buffer coerente");
        let mut dst16 = Image::new(width, height, pixel);
        lanczos(&src, &mut dst16, true);
        let small = if lin.gray { PixelType::U8 } else { PixelType::U8x4 };
        let mut dst = Image::new(width, height, small);
        srgb_mapper().backward_map(&dst16, &mut dst).expect("U16 -> U8");
        if lin.gray { gray_to_rgba(dst.buffer()) } else { dst.into_vec() }
    };
    Page { width, height, rgba, opaque: page.opaque }
}

/// Ingrandisce con lo stesso filtro della scheda video (Catmull-Rom sui valori
/// sRGB): serve solo nel primo istante, quando la scheda video non e' ancora
/// pronta e la pagina si copia nella finestra dal processore. Filtro uguale
/// vuol dire che il passaggio dall'una all'altra non si vede.
pub fn enlarge(page: &Page, width: u32, height: u32) -> Page {
    let src = ImageRef::new(page.width, page.height, &page.rgba, PixelType::U8x4).expect("buffer coerente");
    let mut dst = Image::new(width, height, PixelType::U8x4);
    let options = ResizeOptions::new()
        .resize_alg(ResizeAlg::Convolution(FilterType::CatmullRom))
        .use_alpha(!page.opaque);
    Resizer::new().resize(&src, &mut dst, &options).expect("stesso tipo di pixel");
    Page { width, height, rgba: dst.into_vec(), opaque: page.opaque }
}

fn lanczos(src: &ImageRef, dst: &mut Image, opaque: bool) {
    let options = ResizeOptions::new()
        .resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3))
        .use_alpha(!opaque);
    Resizer::new().resize(src, dst, &options).expect("stesso tipo di pixel");
}

/// Pagine con trasparenza (rare): la strada semplice, senza scorciatoie.
fn resize_with_alpha(page: &Page, width: u32, height: u32) -> Vec<u8> {
    let src = ImageRef::new(page.width, page.height, &page.rgba, PixelType::U8x4).expect("buffer coerente");
    let mut src16 = Image::new(page.width, page.height, PixelType::U16x4);
    srgb_mapper().forward_map(&src, &mut src16).expect("U8x4 -> U16x4");
    let mut dst16 = Image::new(width, height, PixelType::U16x4);
    let src16 = ImageRef::new(page.width, page.height, src16.buffer(), PixelType::U16x4).expect("buffer coerente");
    lanczos(&src16, &mut dst16, false);
    let mut dst = Image::new(width, height, PixelType::U8x4);
    srgb_mapper().backward_map(&dst16, &mut dst).expect("U16x4 -> U8x4");
    dst.into_vec()
}

/// Una pagina opaca in luce lineare a 16 bit, gia' ridotta a blocchi.
struct Linear {
    width: u32,
    height: u32,
    /// Un canale se `gray`, altrimenti quattro (RGBA, alfa piena).
    data: Vec<u16>,
    gray: bool,
}

impl Linear {
    /// Converte e fa la media di blocchi `block` x `block` in un solo
    /// passaggio. Se tutti i pixel hanno R = G = B tiene un canale solo.
    fn from_srgb(page: &Page, block: u32) -> Linear {
        let lut = to_linear_table();
        // pixel a misura fissa ([u8; 4]): il compilatore toglie i controlli sui limiti
        let pixels = page.rgba.as_chunks::<4>().0;
        let gray = pixels.iter().all(|p| p[0] == p[1] && p[1] == p[2]);
        if block == 1 {
            // nessuna media da fare: solo la tabella, un pixel alla volta
            let data: Vec<u16> = if gray {
                pixels.iter().map(|p| lut[p[0] as usize]).collect()
            } else {
                pixels.iter()
                    .flat_map(|p| [lut[p[0] as usize], lut[p[1] as usize], lut[p[2] as usize], u16::MAX])
                    .collect()
            };
            return Linear { width: page.width, height: page.height, data, gray };
        }
        let (w, h) = (page.width.div_ceil(block), page.height.div_ceil(block));
        let channels = if gray { 1 } else { 4 };
        let mut data = vec![0u16; (w * h) as usize * channels];
        let src_w = page.width as usize;
        let b = block as usize;
        let mut sums = vec![0u32; w as usize * 3];
        for by in 0..h as usize {
            sums.fill(0);
            let rows = (by * b)..((by + 1) * b).min(page.height as usize);
            let n_rows = rows.len() as u32;
            for y in rows {
                let row = &page.rgba[y * src_w * 4..(y + 1) * src_w * 4];
                // un blocco alla volta: niente divisione per ogni pixel
                for (s, px) in sums.as_chunks_mut::<3>().0.iter_mut().zip(row.chunks(b * 4)) {
                    let px = px.as_chunks::<4>().0;
                    if gray {
                        s[0] += px.iter().map(|p| lut[p[0] as usize] as u32).sum::<u32>();
                    } else {
                        for p in px {
                            s[0] += lut[p[0] as usize] as u32;
                            s[1] += lut[p[1] as usize] as u32;
                            s[2] += lut[p[2] as usize] as u32;
                        }
                    }
                }
            }
            for bx in 0..w as usize {
                let cols = b.min(src_w - bx * b) as u32;
                let n = n_rows * cols;
                let s = &sums[bx * 3..][..3];
                let at = (by * w as usize + bx) * channels;
                if gray {
                    data[at] = ((s[0] + n / 2) / n) as u16;
                } else {
                    for c in 0..3 {
                        data[at + c] = ((s[c] + n / 2) / n) as u16;
                    }
                    data[at + 3] = u16::MAX;
                }
            }
        }
        Linear { width: w, height: h, data, gray }
    }
}

fn bytemuck_u16(v: &[u16]) -> &[u8] {
    // SAFETY: u16 non ha byte di riempimento e u8 non ha requisiti di allineamento.
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast(), v.len() * 2) }
}

fn gray_to_rgba(gray: &[u8]) -> Vec<u8> {
    gray.iter().flat_map(|&g| [g, g, g, 255]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checkerboard(w: usize, h: usize, color: bool) -> Page {
        let mut rgba = vec![255u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                if (x + y) % 2 == 0 {
                    rgba[(y * w + x) * 4..][..3].copy_from_slice(if color { &[0, 0, 255] } else { &[0, 0, 0] });
                }
            }
        }
        Page { width: w as u32, height: h as u32, rgba, opaque: true }
    }

    fn center(p: &Page) -> &[u8] {
        &p.rgba[((p.height / 2 * p.width + p.width / 2) * 4) as usize..][..4]
    }

    /// Una scacchiera fine di bianco e nero, rimpicciolita, deve diventare il
    /// grigio che riflette la stessa luce (188), non la media dei valori (128).
    /// Con fattore 2 lavora solo Lanczos, con fattore 8 anche i blocchi.
    #[test]
    fn scacchiera_diventa_grigio_giusto() {
        let page = checkerboard(256, 256, false);
        for size in [128, 32] {
            let lin = resize(&page, size, size, true);
            assert!((center(&lin)[0] as i32 - 188).abs() <= 2, "lineare {size}: {:?}", center(&lin));
        }
        let gam = resize(&page, 128, 128, false);
        assert!((center(&gam)[0] as i32 - 128).abs() <= 2, "gamma: {:?}", center(&gam));
    }

    /// Il canale unico vale solo per il bianco e nero vero: il blu resta blu.
    #[test]
    fn il_colore_resta_colore() {
        let out = resize(&checkerboard(256, 256, true), 32, 32, true);
        let c = center(&out);
        assert!(c[2] > c[0] + 2, "il blu e' sparito: {c:?}");
    }

    #[test]
    fn righe_e_colonne_avanzate() {
        // misure non divisibili per il blocco: l'ultimo blocco e' parziale
        let page = Page { width: 1001, height: 999, rgba: vec![200; 1001 * 999 * 4], opaque: true };
        let out = resize(&page, 100, 100, true);
        assert!(out.rgba.as_chunks::<4>().0.iter().all(|p| (p[0] as i32 - 200).abs() <= 1), "tinta unita cambiata");
    }
}
