//! I profili colore delle scansioni. Una pagina salvata da Photoshop porta
//! spesso il suo profilo (Adobe RGB, un "Dot Gain 20%" per il bianco e nero):
//! senza leggerlo i colori escono spenti, o i grigi troppo scuri o chiari.
//! Qui i pixel passano in sRGB, lo spazio dello schermo e di tutto il resto
//! del programma, con moxcms (in Rust, gia' dentro per la libreria `image`).
//!
//! Costruire una conversione costa piu' che usarla, e le pagine di un volume
//! portano quasi sempre lo stesso profilo: le ultime conversioni si tengono,
//! per profilo. Un profilo che non cambia niente (sRGB) si riconosce una
//! volta e poi si salta; uno che non si legge, o di un altro tipo (CMYK), si
//! lascia stare.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, LazyLock, Mutex};

use moxcms::{ColorProfile, DataColorSpace, Layout, Transform8BitExecutor, TransformOptions};

/// Quanti profili diversi si ricordano.
const KEPT: usize = 16;

/// Come si portano in sRGB i pixel di un profilo.
#[derive(Clone)]
enum Conversion {
    /// Colori: RGBA in RGBA.
    Rgb(Arc<Transform8BitExecutor>),
    /// Bianco e nero: il grigio in RGB.
    Gray(Arc<Transform8BitExecutor>),
}

/// Le conversioni gia' fatte, per impronta del profilo; `None`: non serve
/// (o non si puo').
static CONVERSIONS: LazyLock<Mutex<HashMap<u64, Option<Conversion>>>> = LazyLock::new(Mutex::default);

/// Porta in sRGB i pixel RGBA di una pagina che ha il profilo `icc`. Un
/// profilo che non si capisce lascia i pixel come sono.
pub(crate) fn to_srgb(rgba: &mut [u8], icc: &[u8]) {
    if let Some(conversion) = conversion(icc) {
        conversion.apply(rgba);
    }
}

fn conversion(icc: &[u8]) -> Option<Conversion> {
    let mut hasher = DefaultHasher::new();
    icc.hash(&mut hasher);
    let key = hasher.finish();
    let mut cache = CONVERSIONS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(known) = cache.get(&key) {
        return known.clone();
    }
    let made = build(icc).filter(|c| !c.is_identity());
    if cache.len() >= KEPT {
        cache.clear();
    }
    cache.insert(key, made.clone());
    made
}

fn build(icc: &[u8]) -> Option<Conversion> {
    let source = ColorProfile::new_from_slice(icc).ok()?;
    let srgb = ColorProfile::new_srgb();
    let options = TransformOptions::default();
    match source.color_space {
        DataColorSpace::Rgb => {
            source.create_transform_8bit(Layout::Rgba, &srgb, Layout::Rgba, options).ok().map(Conversion::Rgb)
        }
        DataColorSpace::Gray => {
            source.create_transform_8bit(Layout::Gray, &srgb, Layout::Rgb, options).ok().map(Conversion::Gray)
        }
        _ => None,
    }
}

impl Conversion {
    /// Non cambia niente (un profilo sRGB, o quasi): si salta. Si prova su
    /// qualche colore, i primari e qualche mezza tinta.
    fn is_identity(&self) -> bool {
        const SAMPLES: [[u8; 3]; 8] = [
            [0, 0, 0],
            [255, 255, 255],
            [255, 0, 0],
            [0, 255, 0],
            [0, 0, 255],
            [128, 128, 128],
            [200, 120, 60],
            [40, 90, 180],
        ];
        let mut rgba: Vec<u8> = SAMPLES.iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect();
        if let Conversion::Gray(_) = self {
            // il grigio: si guarda il primo canale di ogni campione
            for p in rgba.as_chunks_mut::<4>().0 {
                p[1] = p[0];
                p[2] = p[0];
            }
        }
        let before = rgba.clone();
        if !self.apply(&mut rgba) {
            return true;
        }
        rgba.iter().zip(&before).all(|(a, b)| a.abs_diff(*b) <= 1)
    }

    /// Converte sul posto; `false` se moxcms rifiuta (i pixel restano).
    fn apply(&self, rgba: &mut [u8]) -> bool {
        // a pezzi: una copia di tutta la pagina peserebbe quanto la pagina
        const CHUNK: usize = 64 * 1024;
        match self {
            Conversion::Rgb(t) => {
                let mut src = Vec::with_capacity(CHUNK.min(rgba.len()));
                for part in rgba.chunks_mut(CHUNK) {
                    src.clear();
                    src.extend_from_slice(part);
                    if t.transform(&src, part).is_err() {
                        return false;
                    }
                }
            }
            Conversion::Gray(t) => {
                let (mut gray, mut rgb) = (Vec::new(), Vec::new());
                for part in rgba.chunks_mut(CHUNK) {
                    gray.clear();
                    gray.extend(part.as_chunks::<4>().0.iter().map(|p| p[0]));
                    rgb.resize(gray.len() * 3, 0);
                    if t.transform(&gray, &mut rgb).is_err() {
                        return false;
                    }
                    for (p, c) in part.as_chunks_mut::<4>().0.iter_mut().zip(rgb.as_chunks::<3>().0) {
                        p[..3].copy_from_slice(c);
                    }
                }
            }
        }
        true
    }
}

/// Il profilo di un WebP: il pezzo ICCP del contenitore RIFF, se c'e'.
pub(crate) fn webp_profile(data: &[u8]) -> Option<&[u8]> {
    let mut at = 12;
    while at + 8 <= data.len() {
        let size = u32::from_le_bytes(data[at + 4..at + 8].try_into().ok()?) as usize;
        let body = data.get(at + 8..at + 8 + size)?;
        if &data[at..at + 4] == b"ICCP" {
            return Some(body);
        }
        // i pezzi sono lunghi un numero pari di byte
        at += 8 + size + (size & 1);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(p: ColorProfile) -> Vec<u8> {
        p.encode().unwrap()
    }

    #[test]
    fn adobe_rgb_in_srgb() {
        let icc = profile(ColorProfile::new_adobe_rgb());
        // un verde Adobe RGB e' piu' saturo di quello che l'sRGB sa fare
        let mut rgba = vec![60, 160, 90, 255, 128, 128, 128, 200];
        to_srgb(&mut rgba, &icc);
        assert_ne!(&rgba[..3], &[60, 160, 90], "i colori cambiano");
        assert!(rgba[1] > 160 && rgba[0] < 60, "il verde si allarga: {:?}", &rgba[..3]);
        assert!(rgba[4].abs_diff(128) <= 2 && rgba[4] == rgba[5], "il grigio resta grigio: {:?}", &rgba[4..7]);
        assert_eq!(rgba[7], 200, "l'alfa resta");
    }

    #[test]
    fn srgb_e_profili_strani_non_cambiano_niente() {
        let icc = profile(ColorProfile::new_srgb());
        let mut rgba = vec![60, 160, 90, 255];
        to_srgb(&mut rgba, &icc);
        assert_eq!(rgba, [60, 160, 90, 255]);
        to_srgb(&mut rgba, b"non sono un profilo");
        assert_eq!(rgba, [60, 160, 90, 255]);
    }

    #[test]
    fn il_grigio_con_il_suo_profilo() {
        // gamma 1.8, come i vecchi Mac: 100 e' un grigio piu' chiaro di quanto
        // dica in sRGB (circa 2.2), che per scriverlo ha bisogno di un numero piu'
        // alto: (100/255)^1.8 = 0,186 di luce, in sRGB 119
        let icc = profile(ColorProfile::new_gray_with_gamma(1.8));
        let mut rgba = vec![100, 100, 100, 255];
        to_srgb(&mut rgba, &icc);
        assert!(rgba[0].abs_diff(119) <= 2 && rgba[0] == rgba[1] && rgba[1] == rgba[2], "{rgba:?}");
    }

    #[test]
    fn il_profilo_di_un_webp() {
        let mut data = b"RIFF\0\0\0\0WEBPVP8X".to_vec();
        data.extend_from_slice(&10u32.to_le_bytes());
        data.extend_from_slice(&[0; 10]);
        data.extend_from_slice(b"ICCP");
        data.extend_from_slice(&3u32.to_le_bytes());
        data.extend_from_slice(b"abc\0");
        data.extend_from_slice(b"VP8 ");
        assert_eq!(webp_profile(&data), Some(&b"abc"[..]));
        assert_eq!(webp_profile(b"RIFF\0\0\0\0WEBPVP8 "), None);
    }
}
