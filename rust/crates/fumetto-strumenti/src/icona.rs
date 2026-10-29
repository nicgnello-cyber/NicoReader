//! L'icona di NicoReader: icona.png (Linux), fumetto.ico (Windows) e
//! macos/nicoreader.icns (macOS), tutte da icona.svg.
//!
//! Il disegno e' un SVG (un riquadro da 256): si disegna a ogni misura
//! direttamente, invece di rimpicciolire o ingrandire un'immagine. Cosi' ogni
//! misura ha i suoi bordi netti, e ci sono anche quelle del Retina (fino a
//! 1024 px).
//!
//! Per macOS lo stesso disegno, ma piu' piccolo nel suo riquadro: la griglia
//! di Apple vuole il corpo dell'icona a 824 px su 1024 (nell'SVG e' a 236 su
//! 256); senza il margine, nel Dock sembrerebbe piu' grande delle altre.

use std::io;
use std::path::Path;

use resvg::{tiny_skia, usvg};

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
const SCALA_MAC: f32 = 224.0 / 256.0;

pub fn scrivi(risorse: &Path) -> io::Result<()> {
    let svg = std::fs::read(risorse.join("icona.svg"))?;
    let tree = usvg::Tree::from_data(&svg, &usvg::Options::default()).map_err(io::Error::other)?;
    std::fs::write(risorse.join("icona.png"), png(256, &disegna(&tree, 256, 1.0)?)?)?;
    let immagini: Vec<_> =
        MISURE.iter().map(|&lato| Ok((lato, png(lato, &disegna(&tree, lato, 1.0)?)?))).collect::<io::Result<_>>()?;
    std::fs::write(risorse.join("fumetto.ico"), ico(&immagini))?;
    let blocchi: Vec<_> = MISURE_MAC
        .iter()
        .map(|&(tipo, lato)| Ok((tipo, png(lato, &disegna(&tree, lato, SCALA_MAC)?)?)))
        .collect::<io::Result<_>>()?;
    std::fs::write(risorse.join("macos/nicoreader.icns"), icns(&blocchi))
}

/// L'icona a `lato` pixel, RGBA a 8 bit (non premoltiplicato); `scala` la
/// rimpicciolisce nel suo riquadro, centrata.
fn disegna(tree: &usvg::Tree, lato: u32, scala: f32) -> io::Result<Vec<u8>> {
    let mut pixmap = tiny_skia::Pixmap::new(lato, lato).ok_or_else(|| io::Error::other("misura impossibile"))?;
    let k = lato as f32 / tree.size().width() * scala;
    let margine = lato as f32 * (1.0 - scala) / 2.0;
    resvg::render(tree, tiny_skia::Transform::from_row(k, 0.0, 0.0, k, margine, margine), &mut pixmap.as_mut());
    Ok(pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect())
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
        let svg = include_bytes!("../../fumetto/risorse/icona.svg");
        let tree = usvg::Tree::from_data(svg, &usvg::Options::default()).unwrap();
        let img = disegna(&tree, 256, 1.0).unwrap();
        let pixel = |x: usize, y: usize| &img[(y * 256 + x) * 4..][..4];
        assert_eq!(pixel(0, 0)[3], 0, "fuori dal fondo: trasparente");
        let fondo = pixel(128, 20);
        assert!(fondo[3] == 255 && fondo[2] > fondo[0], "dentro: pieno, blu-viola");
        assert!(pixel(110, 168).iter().all(|&c| c > 240), "la nuvoletta e' bianca");
        // nel Mac c'e' piu' margine
        let mac = disegna(&tree, 256, SCALA_MAC).unwrap();
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
