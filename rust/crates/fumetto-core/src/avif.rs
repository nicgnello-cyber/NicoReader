//! AVIF: il contenitore lo apre avif-parse (Mozilla, lo usa Firefox), la
//! pagina AV1 la decodifica rav1d (dav1d riscritto in Rust, con le sue
//! istruzioni SIMD). Tutto dentro l'eseguibile: niente librerie da installare.
//!
//! Da AV1 escono tre piani Y, Cb, Cr (i due di colore spesso a meta'
//! risoluzione): qui si riportano a RGB con la matrice che dichiara il file.
//! L'alfa, che nelle pagine a fumetti non c'e', si ignora: la pagina e' opaca.

use std::ffi::c_void;
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
    let picture = Decoder::new().ok_or_else(|| invalid("decodificatore"))?.picture(&avif.primary_item)
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
        let bpc = p.p.bpc as u32;
        let layout = p.p.layout;
        // SAFETY: la sequenza vive quanto l'immagine
        let seq = unsafe { p.seq_hdr?.as_ref() };
        let full = seq.color_range != 0;
        let (sx, sy) = match layout {
            DAV1D_PIXEL_LAYOUT_I420 => (1, 1),
            DAV1D_PIXEL_LAYOUT_I422 => (1, 0),
            _ => (0, 0),
        };
        let gray = layout == DAV1D_PIXEL_LAYOUT_I400;
        let wide = bpc > 8;
        let max = ((1u32 << bpc) - 1) as f32;
        // un campione di un piano, riportato a 0..1
        let sample = |plane: usize, x: usize, y: usize| -> f32 {
            let stride = p.stride[plane.min(1)];
            let base = p.data[plane].map_or(std::ptr::null(), |d| d.as_ptr() as *const c_void);
            // SAFETY: (x, y) sta dentro il piano (w x h, o la sua meta' per il
            // colore), e lo stride e' quello che ha dato dav1d
            unsafe {
                let row = base.cast::<u8>().offset(y as isize * stride);
                let v = if wide { *row.cast::<u16>().add(x) as f32 } else { *row.add(x) as f32 };
                v / max
            }
        };
        let (kr, kb) = match seq.mtrx {
            DAV1D_MC_BT709 => (0.2126, 0.0722),
            DAV1D_MC_BT2020_NCL | DAV1D_MC_BT2020_CL => (0.2627, 0.0593),
            _ => (0.299, 0.114),
        };
        let identity = seq.mtrx == DAV1D_MC_IDENTITY;
        let mut rgba = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                let yv = sample(0, x, y);
                let (cb, cr) = if gray { (0.5, 0.5) } else { (sample(1, x >> sx, y >> sy), sample(2, x >> sx, y >> sy)) };
                let (r, g, b) = if identity {
                    (cr, yv, cb) // GBR: Y e' il verde, U il blu, V il rosso
                } else {
                    let (yv, cb, cr) = if full {
                        (yv, cb - 0.5, cr - 0.5)
                    } else {
                        ((yv - 16.0 / 255.0) * 255.0 / 219.0, (cb - 0.5) * 255.0 / 224.0, (cr - 0.5) * 255.0 / 224.0)
                    };
                    let kg = 1.0 - kr - kb;
                    let r = yv + 2.0 * (1.0 - kr) * cr;
                    let b = yv + 2.0 * (1.0 - kb) * cb;
                    let g = (yv - kr * r - kb * b) / kg;
                    (r, g, b)
                };
                let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                rgba.extend_from_slice(&[q(r), q(g), q(b), 255]);
            }
        }
        Some(Page { width: w as u32, height: h as u32, rgba, opaque: true })
    }
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

    #[test]
    fn pagina_in_bianco_e_nero() {
        let p = decode(include_bytes!("../prove/grigia.avif")).unwrap();
        assert_eq!((p.width, p.height), (40, 30));
        assert!(p.rgba[..3].iter().all(|&v| v.abs_diff(128) <= 3), "{:?}", &p.rgba[..4]);
    }
}
