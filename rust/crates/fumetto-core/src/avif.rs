//! AVIF: il contenitore lo apre avif-parse (Mozilla, lo usa Firefox), la
//! pagina AV1 la decodifica rav1d (dav1d riscritto in Rust, con le sue
//! istruzioni SIMD). Tutto dentro l'eseguibile: niente librerie da installare.
//!
//! Da AV1 escono tre piani Y, Cb, Cr (i due di colore spesso a meta'
//! risoluzione): qui si riportano a RGB con la matrice che dichiara il file,
//! con tabelle a punto fisso fatte una volta per pagina (`Tables`).
//! L'alfa, che nelle pagine a fumetti non c'e', si ignora: la pagina e' opaca.

use std::mem::MaybeUninit;
use std::ptr::NonNull;

use rav1d::include::dav1d::data::Dav1dData;
use rav1d::include::dav1d::dav1d::{Dav1dContext, Dav1dSettings};
use rav1d::include::dav1d::headers::{
    DAV1D_MC_BT709, DAV1D_MC_BT2020_CL, DAV1D_MC_BT2020_NCL, DAV1D_MC_IDENTITY, DAV1D_PIXEL_LAYOUT_I400,
    DAV1D_PIXEL_LAYOUT_I420, DAV1D_PIXEL_LAYOUT_I422,
};
use rav1d::include::dav1d::picture::Dav1dPicture;
use rav1d::src::lib::{
    dav1d_close, dav1d_data_create, dav1d_data_unref, dav1d_default_settings, dav1d_get_picture, dav1d_open,
    dav1d_picture_unref, dav1d_send_data,
};

use crate::decode::{DecodeError, Page};

/// dav1d risponde "riprova" con -EAGAIN.
const EAGAIN: i32 = 11;

pub fn is_avif(data: &[u8]) -> bool {
    data.len() >= 12 && &data[4..8] == b"ftyp" && matches!(&data[8..12], b"avif" | b"avis")
}

pub fn decode(data: &[u8]) -> Result<Page, DecodeError> {
    let invalid = |what: &str| DecodeError::Invalid(format!("AVIF: {what}"));
    let avif = avif_parse::read_avif(&mut std::io::Cursor::new(data)).map_err(|e| invalid(&e.to_string()))?;
    let meta = avif.primary_item_metadata().map_err(|e| invalid(&e.to_string()))?;
    super::decode::check_size(meta.max_frame_width.get(), meta.max_frame_height.get())?;
    let picture = Decoder::new()
        .ok_or_else(|| invalid("decodificatore"))?
        .picture(&avif.primary_item)
        .ok_or_else(|| invalid("dati AV1"))?;
    picture.to_rgba().ok_or_else(|| invalid("formato dei pixel"))
}

/// Un decodificatore dav1d, chiuso quando esce di scena.
struct Decoder(Option<Dav1dContext>);

impl Decoder {
    fn new() -> Option<Decoder> {
        let mut settings = MaybeUninit::<Dav1dSettings>::uninit();
        // SAFETY: dav1d_default_settings scrive tutta la struttura; poi la si
        // legge. Un thread solo: la lettura anticipata ne usa gia' uno per pagina.
        unsafe {
            dav1d_default_settings(NonNull::new(settings.as_mut_ptr())?);
            let mut settings = settings.assume_init();
            settings.n_threads = 1;
            settings.max_frame_delay = 1;
            let mut ctx: Option<Dav1dContext> = None;
            let r = dav1d_open(NonNull::new(&mut ctx), NonNull::new(&mut settings));
            (r.0 == 0 && ctx.is_some()).then_some(Decoder(ctx))
        }
    }

    /// La prima immagine dei dati AV1.
    fn picture(&mut self, obu: &[u8]) -> Option<Picture> {
        // SAFETY: `data` la riempie dav1d_data_create (tutti i campi); i byte
        // si copiano nel buffer lungo `obu.len()` che restituisce. `pic` e'
        // una struttura nostra, che dav1d scrive e Picture libera.
        unsafe {
            let mut data: Dav1dData = MaybeUninit::zeroed().assume_init();
            let buf = dav1d_data_create(NonNull::new(&mut data), obu.len());
            if buf.is_null() {
                return None;
            }
            std::ptr::copy_nonoverlapping(obu.as_ptr(), buf, obu.len());
            let mut pic = Dav1dPicture::default();
            let mut got = false;
            // una sola immagine: si mandano i dati e la si chiede finche' non
            // arriva (o finche' dav1d non ha piu' niente da dire)
            for _ in 0..64 {
                if data.sz > 0 {
                    let r = dav1d_send_data(self.0, NonNull::new(&mut data)).0;
                    if r < 0 && r != -EAGAIN {
                        break;
                    }
                }
                match dav1d_get_picture(self.0, NonNull::new(&mut pic)).0 {
                    0 => {
                        got = true;
                        break;
                    }
                    r if r == -EAGAIN => {}
                    _ => break,
                }
            }
            if data.sz > 0 {
                dav1d_data_unref(NonNull::new(&mut data));
            }
            got.then_some(Picture(pic))
        }
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        // SAFETY: il contesto e' nostro, aperto da dav1d_open.
        unsafe { dav1d_close(NonNull::new(&mut self.0)) };
    }
}

struct Picture(Dav1dPicture);

impl Drop for Picture {
    fn drop(&mut self) {
        // SAFETY: l'immagine l'ha data dav1d_get_picture e si libera una volta.
        unsafe { dav1d_picture_unref(NonNull::new(&mut self.0)) };
    }
}

impl Picture {
    fn to_rgba(&self) -> Option<Page> {
        let p = &self.0;
        let (w, h) = (p.p.w as usize, p.p.h as usize);
        let bits = p.p.bpc as u32;
        // SAFETY: la sequenza vive quanto l'immagine
        let seq = unsafe { p.seq_hdr?.as_ref() };
        let sub = match p.p.layout {
            DAV1D_PIXEL_LAYOUT_I420 => (1, 1),
            DAV1D_PIXEL_LAYOUT_I422 => (1, 0),
            _ => (0, 0),
        };
        let matrix = match seq.mtrx {
            _ if p.p.layout == DAV1D_PIXEL_LAYOUT_I400 => Matrix::Gray { full: seq.color_range != 0 },
            DAV1D_MC_IDENTITY => Matrix::Identity,
            m => {
                let (kr, kb) = match m {
                    DAV1D_MC_BT709 => (0.2126, 0.0722),
                    DAV1D_MC_BT2020_NCL | DAV1D_MC_BT2020_CL => (0.2627, 0.0593),
                    _ => (0.299, 0.114),
                };
                Matrix::Ycc { kr, kb, full: seq.color_range != 0 }
            }
        };
        let rgba = if bits > 8 {
            self.convert::<u16>(w, h, sub, bits, matrix)?
        } else {
            self.convert::<u8>(w, h, sub, bits, matrix)?
        };
        Some(Page { width: w as u32, height: h as u32, rgba, opaque: true })
    }

    /// I piani di dav1d come fette di campioni (`T`: u8 fino a 8 bit, u16
    /// oltre), poi la conversione.
    fn convert<T: Copy + Into<u32>>(
        &self, w: usize, h: usize, (sx, sy): (usize, usize), bits: u32, matrix: Matrix,
    ) -> Option<Vec<u8>> {
        let p = &self.0;
        let (cw, ch) = ((w + sx) >> sx, (h + sy) >> sy);
        let plane = |i: usize, width: usize, height: usize| -> Option<Plane<'_, T>> {
            let stride = usize::try_from(p.stride[i.min(1)]).ok()? / size_of::<T>();
            let base = p.data[i]?.as_ptr() as *const T;
            if stride < width || height == 0 {
                return None;
            }
            // SAFETY: dav1d da' un piano di `height` righe distanti `stride`
            // campioni, ognuna con almeno `width` campioni; la fetta si ferma
            // alla fine dell'ultima riga, e vive quanto l'immagine (&self)
            let data = unsafe { std::slice::from_raw_parts(base, stride * (height - 1) + width) };
            Some(Plane { data, stride })
        };
        let y = plane(0, w, h)?;
        let chroma = match matrix {
            Matrix::Gray { .. } => None,
            _ => Some((plane(1, cw, ch)?, plane(2, cw, ch)?)),
        };
        Some(planes_to_rgba(w, h, (sx, sy), &y, chroma.as_ref(), bits, matrix))
    }
}

/// Come da Y, Cb, Cr si torna a RGB.
#[derive(Clone, Copy, Debug)]
enum Matrix {
    /// Solo Y: grigio.
    Gray { full: bool },
    /// GBR: Y e' il verde, Cb il blu, Cr il rosso.
    Identity,
    /// Luminanza e differenze di colore, con i pesi del rosso e del blu.
    Ycc { kr: f32, kb: f32, full: bool },
}

/// Un piano: i campioni, e quanti ce ne sono da una riga all'altra.
struct Plane<'a, T> {
    data: &'a [T],
    stride: usize,
}

impl<T: Copy> Plane<'_, T> {
    fn row(&self, y: usize, len: usize) -> &[T] {
        &self.data[y * self.stride..][..len]
    }
}

/// Le cifre dopo la virgola dei punti fissi delle tabelle.
const FRAC: u32 = 16;

/// Per ogni valore di un campione, quanto da' a R, G e B (gia' per 255, in
/// punti fissi): cosi' un pixel costa cinque letture e qualche somma, invece
/// di una dozzina di moltiplicazioni in virgola mobile.
struct Tables {
    y: Vec<i32>,
    r_cr: Vec<i32>,
    g_cb: Vec<i32>,
    g_cr: Vec<i32>,
    b_cb: Vec<i32>,
}

impl Tables {
    fn new(bits: u32, matrix: Matrix) -> Tables {
        let max = ((1u32 << bits) - 1) as f32;
        let fixed = |v: f32| (v * 255.0 * (1 << FRAC) as f32).round() as i32;
        let each = |f: &dyn Fn(f32) -> f32| (0..=max as u32).map(|v| fixed(f(v as f32 / max))).collect::<Vec<i32>>();
        // il campo ridotto ("TV"): il nero a 16/255, il bianco a 235/255
        let luma = |full: bool| move |v: f32| if full { v } else { (v - 16.0 / 255.0) * 255.0 / 219.0 };
        let chroma = |full: bool| move |v: f32| if full { v - 0.5 } else { (v - 0.5) * 255.0 / 224.0 };
        let zero = || vec![0; max as usize + 1];
        match matrix {
            Matrix::Gray { full } => {
                Tables { y: each(&luma(full)), r_cr: zero(), g_cb: zero(), g_cr: zero(), b_cb: zero() }
            }
            // GBR: Y e' il verde, Cb il blu, Cr il rosso: le tabelle di Cb e Cr
            // portano il loro canale intero, quella di Y solo il verde (vedi
            // `planes_to_rgba`)
            Matrix::Identity => {
                let plain = each(&|v| v);
                Tables { y: plain.clone(), r_cr: plain.clone(), g_cb: zero(), g_cr: zero(), b_cb: plain }
            }
            Matrix::Ycc { kr, kb, full } => {
                let kg = 1.0 - kr - kb;
                let c = chroma(full);
                Tables {
                    y: each(&luma(full)),
                    r_cr: each(&|v| 2.0 * (1.0 - kr) * c(v)),
                    b_cb: each(&|v| 2.0 * (1.0 - kb) * c(v)),
                    // G = (Y - kr R - kb B) / kg, con R e B scritti come sopra
                    g_cb: each(&|v| -kb * 2.0 * (1.0 - kb) * c(v) / kg),
                    g_cr: each(&|v| -kr * 2.0 * (1.0 - kr) * c(v) / kg),
                }
            }
        }
    }
}

/// Da punto fisso a 0..255, arrotondando.
fn byte(v: i32) -> u8 {
    ((v + (1 << (FRAC - 1))) >> FRAC).clamp(0, 255) as u8
}

/// I piani Y, Cb, Cr (questi due ridotti di `sx`, `sy`) in pixel RGBA opachi.
fn planes_to_rgba<T: Copy + Into<u32>>(
    w: usize, h: usize, (sx, sy): (usize, usize), y: &Plane<T>, chroma: Option<&(Plane<T>, Plane<T>)>, bits: u32,
    matrix: Matrix,
) -> Vec<u8> {
    let t = Tables::new(bits, matrix);
    let cw = (w + sx) >> sx;
    let index = |v: T| v.into() as usize;
    let mut rgba = vec![255u8; w * h * 4];
    for (row, out) in rgba.chunks_exact_mut(w * 4).enumerate() {
        let ys = y.row(row, w);
        match (chroma, matrix) {
            (Some((cb, cr)), Matrix::Identity) => {
                let (cbs, crs) = (cb.row(row >> sy, cw), cr.row(row >> sy, cw));
                for (x, px) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                    let (b, r) = (index(cbs[x >> sx]), index(crs[x >> sx]));
                    px[..3].copy_from_slice(&[byte(t.r_cr[r]), byte(t.y[index(ys[x])]), byte(t.b_cb[b])]);
                }
            }
            (Some((cb, cr)), _) => {
                let (cbs, crs) = (cb.row(row >> sy, cw), cr.row(row >> sy, cw));
                for (x, px) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                    let (l, b, r) = (t.y[index(ys[x])], index(cbs[x >> sx]), index(crs[x >> sx]));
                    px[..3].copy_from_slice(&[
                        byte(l + t.r_cr[r]),
                        byte(l + t.g_cb[b] + t.g_cr[r]),
                        byte(l + t.b_cb[b]),
                    ]);
                }
            }
            (None, _) => {
                for (x, px) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                    let l = byte(t.y[index(ys[x])]);
                    px[..3].copy_from_slice(&[l, l, l]);
                }
            }
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Due campiture, rossa a sinistra e blu a destra (fatte con libavif).
    #[test]
    fn pagina_a_colori() {
        let data = include_bytes!("../prove/pagina.avif");
        assert!(is_avif(data));
        let p = decode(data).unwrap();
        assert_eq!((p.width, p.height), (96, 64));
        let px = |x: u32, y: u32| &p.rgba[((y * p.width + x) * 4) as usize..][..3];
        let near = |a: &[u8], b: [u8; 3]| a.iter().zip(b).all(|(&a, b)| a.abs_diff(b) <= 4);
        assert!(near(px(10, 10), [200, 40, 30]), "rosso: {:?}", px(10, 10));
        assert!(near(px(80, 50), [30, 60, 190]), "blu: {:?}", px(80, 50));
    }

    /// La formula di prima, pixel per pixel in virgola mobile: le tabelle
    /// devono dare gli stessi numeri (al piu' uno di differenza, per
    /// l'arrotondamento), con ogni matrice, campo e profondita'.
    #[test]
    fn le_tabelle_danno_la_formula() {
        fn reference(yv: f32, cb: f32, cr: f32, m: Matrix) -> [u8; 3] {
            let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            match m {
                Matrix::Gray { full } => {
                    let l = if full { yv } else { (yv - 16.0 / 255.0) * 255.0 / 219.0 };
                    [q(l); 3]
                }
                Matrix::Identity => [q(cr), q(yv), q(cb)],
                Matrix::Ycc { kr, kb, full } => {
                    let (yv, cb, cr) = if full {
                        (yv, cb - 0.5, cr - 0.5)
                    } else {
                        ((yv - 16.0 / 255.0) * 255.0 / 219.0, (cb - 0.5) * 255.0 / 224.0, (cr - 0.5) * 255.0 / 224.0)
                    };
                    let r = yv + 2.0 * (1.0 - kr) * cr;
                    let b = yv + 2.0 * (1.0 - kb) * cb;
                    let g = (yv - kr * r - kb * b) / (1.0 - kr - kb);
                    [q(r), q(g), q(b)]
                }
            }
        }
        fn check<T: Copy + Into<u32> + TryFrom<u32>>(bits: u32, m: Matrix) {
            let max = (1u32 << bits) - 1;
            // 4 x 4 pixel, colore a meta' risoluzione (4:2:0), valori sparsi
            let (w, h) = (4, 4);
            let v = |i: u32| T::try_from(i.wrapping_mul(2_654_435_761) % (max + 1)).ok().unwrap();
            let ys: Vec<T> = (0..16).map(|i| v(i + 1)).collect();
            let cbs: Vec<T> = (0..4).map(|i| v(i + 40)).collect();
            let crs: Vec<T> = (0..4).map(|i| v(i + 90)).collect();
            let y = Plane { data: &ys, stride: 4 };
            let chroma = (Plane { data: &cbs, stride: 2 }, Plane { data: &crs, stride: 2 });
            let with = !matches!(m, Matrix::Gray { .. });
            let out = planes_to_rgba(w, h, (1, 1), &y, with.then_some(&chroma), bits, m);
            for row in 0..h {
                for x in 0..w {
                    let f = |s: T| s.into() as f32 / max as f32;
                    let c = (row / 2) * 2 + x / 2;
                    let want = reference(f(ys[row * 4 + x]), f(cbs[c]), f(crs[c]), m);
                    let got = &out[(row * w + x) * 4..][..3];
                    assert!(
                        got.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 1),
                        "{bits} bit, {m:?}, ({x}, {row}): {got:?} invece di {want:?}"
                    );
                }
            }
        }
        let matrices = [
            Matrix::Gray { full: true },
            Matrix::Gray { full: false },
            Matrix::Identity,
            Matrix::Ycc { kr: 0.2126, kb: 0.0722, full: true },
            Matrix::Ycc { kr: 0.299, kb: 0.114, full: false },
            Matrix::Ycc { kr: 0.2627, kb: 0.0593, full: false },
        ];
        for m in matrices {
            check::<u8>(8, m);
            check::<u16>(10, m);
            check::<u16>(12, m);
        }
    }

    #[test]
    fn pagina_in_bianco_e_nero() {
        let p = decode(include_bytes!("../prove/grigia.avif")).unwrap();
        assert_eq!((p.width, p.height), (40, 30));
        assert!(p.rgba[..3].iter().all(|&v| v.abs_diff(128) <= 3), "{:?}", &p.rgba[..4]);
    }
}
