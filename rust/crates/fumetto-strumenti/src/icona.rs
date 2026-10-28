//! L'icona di NicoReader: icona.png (Linux), fumetto.ico (Windows) e
//! macos/nicoreader.icns (macOS).
//!
//! L'icona e' fatta di forme (un quadrato dagli angoli tondi con la sfumatura,
//! due pagine, la costola fra loro, un riquadro e tre righe per pagina), qui
//! descritte in un riquadro da 256: si disegna a ogni misura direttamente, con
//! i bordi sfumati, invece di rimpicciolire o ingrandire un'immagine. Cosi' ci
//! sono anche le misure del Retina (fino a 1024 px), che da una 256 non si
//! potrebbero fare.
//!
//! I colori si mescolano in luce lineare, come le pagine nel programma, cosi'
//! i bordi non si scuriscono.
//!
//! Per macOS lo stesso disegno, ma piu' piccolo nel suo riquadro: la griglia
//! di Apple vuole il corpo dell'icona a 824 px su 1024 (qui e' a 236 su 256);
//! senza il margine, nel Dock sembrerebbe piu' grande delle altre.

use std::io;
use std::path::Path;

/// Windows: barra del titolo, taskbar, Esplora file, a ogni scala dello schermo.
const MISURE: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];
/// macOS: il tipo di ogni blocco dell'ICNS e il suo lato in pixel (ic10 e' il
/// 512@2x, ic14 il 256@2x).
const MISURE_MAC: [(&[u8; 4], u32); 11] = [
    (b"icp4", 16),
    (b"icp5", 32),
    (b"ic11", 32),
    (b"icp6", 64),
    (b"ic12", 64),
    (b"ic07", 128),
    (b"ic08", 256),
    (b"ic13", 256),
    (b"ic09", 512),
    (b"ic14", 512),
    (b"ic10", 1024),
];
/// Il corpo (236 su 256) diventa 824/1024 del riquadro.
const SCALA_MAC: f64 = 224.0 / 256.0;

// --- il disegno, in un riquadro da 256 ---

/// Un rettangolo dagli angoli tondi: x0, y0, x1, y1, raggio.
type Forma = (f64, f64, f64, f64, f64);
type Rgb = [f64; 3];

/// La sfumatura va in diagonale, da (10, 10) a (246, 246).
const SFUMATURA: (Rgb, Rgb) = ([80.7, 122.4, 255.0], [121.3, 63.6, 255.0]);
const FONDO: Forma = (10.0, 10.0, 246.0, 246.0, 50.0);
/// La costola fra le pagine: il fondo piu' scuro.
const COSTOLA: Forma = (124.0, 62.0, 132.0, 194.0, 0.0);
const OMBRA_COSTOLA: f64 = 210.0 / 255.0;
const PAGINA: Rgb = [246.0, 246.0, 255.0];
const RIQUADRO: Rgb = [79.0, 125.0, 255.0];
const RIGA: Rgb = [187.0, 191.0, 207.0];
/// La pagina sinistra; la destra e' 86 piu' in la'.
const DESTRA: f64 = 86.0;
const FORME: [(Forma, Rgb); 5] = [
    ((46.0, 62.0, 124.0, 194.0, 8.0), PAGINA),
    ((56.0, 74.0, 114.0, 114.0, 5.0), RIQUADRO),
    ((56.0, 124.0, 114.0, 136.0, 4.0), RIGA),
    ((56.0, 146.0, 114.0, 158.0, 4.0), RIGA),
    ((56.0, 168.0, 91.0, 180.0, 4.0), RIGA),
];

pub fn scrivi(risorse: &Path) -> io::Result<()> {
    std::fs::write(risorse.join("icona.png"), png(256, &disegna(256, 1.0))?)?;
    let immagini: Vec<_> =
        MISURE.iter().map(|&lato| Ok((lato, png(lato, &disegna(lato, 1.0))?))).collect::<io::Result<_>>()?;
    std::fs::write(risorse.join("fumetto.ico"), ico(&immagini))?;
    let blocchi: Vec<_> = MISURE_MAC
        .iter()
        .map(|&(tipo, lato)| Ok((tipo, png(lato, &disegna(lato, SCALA_MAC))?)))
        .collect::<io::Result<_>>()?;
    std::fs::write(risorse.join("macos/nicoreader.icns"), icns(&blocchi))
}

fn lineare(c: f64) -> f64 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn srgb(c: f64) -> f64 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

fn colore(rgb: Rgb) -> Rgb {
    rgb.map(|c| lineare(c / 255.0))
}

/// Quanto del pixel in (x, y) copre il rettangolo dagli angoli tondi: dalla
/// distanza dal bordo (negativa dentro), su un pixel di sfumatura. `px`: i
/// pixel per unita' del riquadro da 256.
fn copertura(x: f64, y: f64, (x0, y0, x1, y1, r): Forma, px: f64) -> f64 {
    let qx = (x - (x0 + x1) / 2.0).abs() - ((x1 - x0) / 2.0 - r);
    let qy = (y - (y0 + y1) / 2.0).abs() - ((y1 - y0) / 2.0 - r);
    let fuori = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r;
    (0.5 - fuori * px).clamp(0.0, 1.0)
}

/// L'icona a `lato` pixel, RGBA sRGB a 8 bit; `scala` la rimpicciolisce nel
/// suo riquadro, centrata.
fn disegna(lato: u32, scala: f64) -> Vec<u8> {
    let px = f64::from(lato) / 256.0 * scala;
    // il centro di ogni pixel, nelle coordinate del riquadro da 256
    let centri: Vec<f64> = (0..lato).map(|i| (f64::from(i) + 0.5 - f64::from(lato) / 2.0) / px + 128.0).collect();
    let ombra = lineare(OMBRA_COSTOLA);
    let mut out = Vec::with_capacity((lato * lato * 4) as usize);
    for &y in &centri {
        for &x in &centri {
            let t = (((x + y) - 20.0) / 472.0).clamp(0.0, 1.0);
            // la sfumatura e' fra i due colori come si scrivono (sRGB), non in luce lineare
            let (a, b) = SFUMATURA;
            let fondo = colore([0, 1, 2].map(|i| a[i] * (1.0 - t) + b[i] * t));
            let k = copertura(x, y, COSTOLA, px);
            let mut rgb = fondo.map(|c| c * (1.0 - k) + c * ombra * k);
            for dx in [0.0, DESTRA] {
                for ((x0, y0, x1, y1, r), col) in FORME {
                    let k = copertura(x, y, (x0 + dx, y0, x1 + dx, y1, r), px);
                    let col = colore(col);
                    rgb = [0, 1, 2].map(|i| rgb[i] * (1.0 - k) + col[i] * k);
                }
            }
            let alfa = copertura(x, y, FONDO, px);
            out.extend(rgb.map(srgb).into_iter().chain([alfa]).map(otto_bit));
        }
    }
    out
}

/// Da 0..1 a 0..255; a meta' strada verso il pari, come numpy.
fn otto_bit(c: f64) -> u8 {
    (c * 255.0).round_ties_even() as u8
}

/// Il PNG piu' piccolo fra quelli con i vari filtri: le icone vanno in ogni
/// pacchetto, e sono poche.
fn png(lato: u32, rgba: &[u8]) -> io::Result<Vec<u8>> {
    use png::Filter::*;
    let mut piu_piccolo: Option<Vec<u8>> = None;
    for filtro in [NoFilter, Sub, Up, Avg, Paeth, Adaptive, MinEntropy] {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, lato, lato);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_deflate_compression(png::DeflateCompression::Level(9));
        enc.set_filter(filtro);
        let mut w = enc.write_header().map_err(io::Error::other)?;
        w.write_image_data(rgba).map_err(io::Error::other)?;
        w.finish().map_err(io::Error::other)?;
        if piu_piccolo.as_ref().is_none_or(|p| out.len() < p.len()) {
            piu_piccolo = Some(out);
        }
    }
    piu_piccolo.ok_or_else(|| io::Error::other("nessun filtro"))
}

/// Un ICO con le immagini in PNG (Windows Vista e dopo).
fn ico(immagini: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let n = immagini.len() as u16;
    let mut testa = [0u16.to_le_bytes(), 1u16.to_le_bytes(), n.to_le_bytes()].concat();
    let mut dati: Vec<u8> = Vec::new();
    let mut offset = 6 + 16 * immagini.len() as u32;
    for (lato, png) in immagini {
        // 256 si scrive 0
        let l = (lato % 256) as u8;
        testa.extend([l, l, 0, 0]);
        testa.extend(1u16.to_le_bytes()); // piani
        testa.extend(32u16.to_le_bytes()); // bit per pixel
        testa.extend((png.len() as u32).to_le_bytes());
        testa.extend(offset.to_le_bytes());
        offset += png.len() as u32;
        dati.extend(png);
    }
    testa.extend(dati);
    testa
}

/// Un ICNS: "icns", la lunghezza, e i blocchi (tipo, lunghezza, PNG).
fn icns(blocchi: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut corpo = Vec::new();
    for (tipo, png) in blocchi {
        corpo.extend(*tipo);
        corpo.extend((8 + png.len() as u32).to_be_bytes());
        corpo.extend(png);
    }
    [&b"icns"[..], &(8 + corpo.len() as u32).to_be_bytes(), &corpo].concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn il_disegno() {
        let img = disegna(256, 1.0);
        let pixel = |x: usize, y: usize| &img[(y * 256 + x) * 4..][..4];
        assert_eq!(pixel(0, 0)[3], 0, "fuori dal fondo: trasparente");
        assert_eq!(pixel(128, 30)[3], 255, "dentro: pieno");
        assert_eq!(pixel(60, 190), [246, 246, 255, 255], "la pagina");
        assert_eq!(pixel(80, 90), [79, 125, 255, 255], "il riquadro");
        let costola = pixel(128, 150);
        let fondo = pixel(128, 30);
        assert!(costola[..3].iter().zip(fondo).all(|(c, f)| c <= f), "la costola e' piu' scura");
        // nel Mac c'e' piu' margine
        let mac = disegna(256, SCALA_MAC);
        assert_eq!(mac[(14 * 256 + 128) * 4 + 3], 0);
    }

    #[test]
    fn i_contenitori() {
        let immagini = vec![(16, vec![1, 2, 3]), (256, vec![4, 5])];
        let ico = ico(&immagini);
        assert_eq!(ico[..6], [0, 0, 1, 0, 2, 0]);
        assert_eq!(ico[6..10], [16, 16, 0, 0]);
        assert_eq!(ico[22..26], [0, 0, 0, 0], "256 si scrive 0");
        assert_eq!(u32::from_le_bytes(ico[18..22].try_into().unwrap()), 38, "la prima dopo le voci");
        assert_eq!(u32::from_le_bytes(ico[34..38].try_into().unwrap()), 41);
        assert_eq!(ico[38..], [1, 2, 3, 4, 5]);

        let icns = icns(&[(b"icp4", vec![9; 3])]);
        assert_eq!(icns, [b"icns".as_slice(), &[0, 0, 0, 19], b"icp4", &[0, 0, 0, 11], &[9, 9, 9]].concat());
    }
}
