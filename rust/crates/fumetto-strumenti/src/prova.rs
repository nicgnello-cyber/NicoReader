//! Dei volumi per le prove dei pacchetti (prova-finder.sh): CBZ con una
//! pagina sola, un PNG 8x8 grigio.

use std::io::{self, Write};
use std::path::Path;

use zip::write::SimpleFileOptions;

/// Scrive `<cartella>/<nome>.cbz` per ogni nome.
pub fn scrivi(cartella: &Path, nomi: &[&str]) -> io::Result<()> {
    std::fs::create_dir_all(cartella)?;
    let pagina = pagina()?;
    for nome in nomi {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(cartella.join(format!("{nome}.cbz")))?);
        let senza = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("001.png", senza)?;
        zip.write_all(&pagina)?;
        zip.finish()?;
    }
    Ok(())
}

fn pagina() -> io::Result<Vec<u8>> {
    const LATO: u32 = 8;
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, LATO, LATO);
    enc.set_color(png::ColorType::Grayscale);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(io::Error::other)?;
    w.write_image_data(&[0x80; (LATO * LATO) as usize]).map_err(io::Error::other)?;
    w.finish().map_err(io::Error::other)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volumi_di_una_pagina() {
        let dir = std::env::temp_dir().join(format!("fumetto-volumi-di-prova-{}", std::process::id()));
        scrivi(&dir, &["Primo", "Secondo"]).unwrap();
        for nome in ["Primo", "Secondo"] {
            let mut zip = zip::ZipArchive::new(std::fs::File::open(dir.join(format!("{nome}.cbz"))).unwrap()).unwrap();
            assert_eq!(zip.len(), 1);
            let mut pagina = Vec::new();
            io::Read::read_to_end(&mut zip.by_name("001.png").unwrap(), &mut pagina).unwrap();
            let img = png::Decoder::new(io::Cursor::new(pagina)).read_info().unwrap();
            assert_eq!((img.info().width, img.info().height), (8, 8));
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
