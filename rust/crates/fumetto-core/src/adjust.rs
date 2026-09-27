//! I ritocchi alla pagina decodificata, prima di portarla alla misura dello
//! schermo: il rifilo dei margini e la rotazione.
//!
//! Si fanno sui pixel originali, nei thread della lettura anticipata: cosi' la
//! pagina rifilata o girata si rimpicciolisce una volta sola, in luce lineare,
//! alla sua misura vera, e alla scheda video arriva come qualsiasi altra.

use crate::decode::Page;

/// Quanto un pixel puo' scostarsi dal fondo ed essere ancora margine: la
/// grana della carta e il rumore dello scanner stanno sotto.
const TOLERANCE: u8 = 20;
/// Se resta meno di questa frazione della pagina il rifilo e' sospetto (una
/// tavola quasi vuota, un disegno chiaro sul bianco): si lascia intera.
const MIN_AREA: f32 = 0.55;
/// Il lato lungo della miniatura su cui si cercano i margini: la media dei
/// blocchi toglie il rumore, e bastano pochi pixel per vedere un bordo.
const PROBE: u32 = 140;

/// Un riquadro della pagina, in pixel: x, y, larghezza, altezza.
pub type Area = (u32, u32, u32, u32);

/// La parte utile della pagina, senza i margini uniformi (bianchi o neri);
/// `None` se non c'e' niente da rifilare, o se il rifilo sarebbe sospetto.
pub fn margins(p: &Page) -> Option<Area> {
    let (w, h) = (p.width, p.height);
    if w < 64 || h < 64 {
        return None;
    }
    let k = w.max(h).div_ceil(PROBE);
    let (tw, th) = (w.div_ceil(k) as usize, h.div_ceil(k) as usize);
    // la miniatura in grigio: media di blocchi k x k, sul nero dove e' trasparente
    let mut sums = vec![0u32; tw * th];
    let mut counts = vec![0u32; tw * th];
    for (y, row) in p.rgba.chunks_exact(w as usize * 4).enumerate() {
        let ty = y / k as usize;
        for (x, px) in row.as_chunks::<4>().0.iter().enumerate() {
            let luma = (px[0] as u32 * 77 + px[1] as u32 * 150 + px[2] as u32 * 29) >> 8;
            let at = ty * tw + x / k as usize;
            sums[at] += luma * px[3] as u32 / 255;
            counts[at] += 1;
        }
    }
    let gray: Vec<u8> = sums.iter().zip(&counts).map(|(s, n)| (s / n.max(&1)) as u8).collect();
    let at = |x: usize, y: usize| gray[y * tw + x];

    // il fondo: la mediana dei quattro angoli, cosi' vale sia per le
    // scansioni sul bianco sia per le tavole sul nero
    let mut corners = [at(0, 0), at(tw - 1, 0), at(0, th - 1), at(tw - 1, th - 1)];
    corners.sort_unstable();
    let bg = ((corners[1] as u16 + corners[2] as u16) / 2) as u8;
    let near = |v: u8| v.abs_diff(bg) <= TOLERANCE;
    let row_blank = |y: usize| (0..tw).all(|x| near(at(x, y)));
    let col_blank = |x: usize, y0: usize, y1: usize| (y0..=y1).all(|y| near(at(x, y)));

    let (mut top, mut bottom) = (0, th - 1);
    while top < bottom && row_blank(top) {
        top += 1;
    }
    while bottom > top && row_blank(bottom) {
        bottom -= 1;
    }
    let (mut left, mut right) = (0, tw - 1);
    while left < right && col_blank(left, top, bottom) {
        left += 1;
    }
    while right > left && col_blank(right, top, bottom) {
        right -= 1;
    }
    let kept = ((right - left + 1) * (bottom - top + 1)) as f32;
    if kept < MIN_AREA * (tw * th) as f32 {
        return None;
    }
    // di nuovo in pixel della pagina, con un filo di respiro attorno al disegno
    let pad = 2;
    let k = k as usize;
    let x0 = (left * k).saturating_sub(pad) as u32;
    let y0 = (top * k).saturating_sub(pad) as u32;
    let x1 = (((right + 1) * k + pad) as u32).min(w);
    let y1 = (((bottom + 1) * k + pad) as u32).min(h);
    let trimmed = x0 > 0 || y0 > 0 || x1 < w || y1 < h;
    (trimmed && x1 - x0 > 32 && y1 - y0 > 32).then_some((x0, y0, x1 - x0, y1 - y0))
}

/// Il riquadro `area` della pagina.
pub fn crop(p: &Page, (x, y, w, h): Area) -> Page {
    let stride = p.width as usize * 4;
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for row in p.rgba.chunks_exact(stride).skip(y as usize).take(h as usize) {
        rgba.extend_from_slice(&row[x as usize * 4..(x + w) as usize * 4]);
    }
    Page { width: w, height: h, rgba, opaque: p.opaque }
}

/// La pagina rifilata, o com'era se non c'e' niente da togliere.
pub fn trim(p: Page) -> Page {
    match margins(&p) {
        Some(area) => crop(&p, area),
        None => p,
    }
}

/// La pagina girata di `degrees` in senso orario (0, 90, 180 o 270).
pub fn rotate(p: Page, degrees: u16) -> Page {
    let (w, h) = (p.width as usize, p.height as usize);
    let src = p.rgba.as_chunks::<4>().0;
    match degrees % 360 {
        0 => p,
        180 => {
            let rgba = src.iter().rev().flatten().copied().collect();
            Page { rgba, ..p }
        }
        quarter => {
            let cw = quarter == 90;
            let mut dst = vec![[0u8; 4]; w * h];
            // a blocchi, perche' una delle due letture salta di riga in riga:
            // un blocco di 64 righe sta tutto nella cache
            const B: usize = 64;
            for by in (0..h).step_by(B) {
                for bx in (0..w).step_by(B) {
                    for y in by..(by + B).min(h) {
                        for x in bx..(bx + B).min(w) {
                            // la pagina nuova e' larga h e alta w
                            let (nx, ny) = if cw { (h - 1 - y, x) } else { (y, w - 1 - x) };
                            dst[ny * h + nx] = src[y * w + x];
                        }
                    }
                }
            }
            Page { width: h as u32, height: w as u32, rgba: dst.into_flattened(), opaque: p.opaque }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un foglio `w` x `h` del colore `paper`, con un riquadro d'inchiostro.
    fn sheet(w: u32, h: u32, paper: u8, ink: Area) -> Page {
        let mut rgba = vec![paper; (w * h * 4) as usize];
        for y in ink.1..ink.1 + ink.3 {
            for x in ink.0..ink.0 + ink.2 {
                let at = ((y * w + x) * 4) as usize;
                rgba[at..at + 3].copy_from_slice(&[40, 40, 40]);
            }
        }
        for a in rgba.iter_mut().skip(3).step_by(4) {
            *a = 255;
        }
        Page { width: w, height: h, rgba, opaque: true }
    }

    #[test]
    fn toglie_il_bianco_attorno_al_disegno() {
        let page = sheet(1000, 1500, 250, (100, 120, 800, 1260));
        let (x, y, w, h) = margins(&page).expect("margini da togliere");
        assert!(x <= 100 && x + 20 >= 100, "sinistra {x}");
        assert!(y <= 120 && y + 20 >= 120, "alto {y}");
        assert!(x + w >= 900 && x + w <= 920, "destra {}", x + w);
        assert!(y + h >= 1380 && y + h <= 1400, "basso {}", y + h);
        let cut = trim(page);
        assert_eq!((cut.width, cut.height), (w, h));
    }

    #[test]
    fn anche_sul_nero() {
        let page = sheet(1000, 1500, 0, (50, 50, 900, 1400));
        let mut page = page;
        // disegno chiaro su fondo nero
        for px in page.rgba.as_chunks_mut::<4>().0 {
            if px[0] == 40 {
                *px = [220, 220, 220, 255];
            }
        }
        assert!(margins(&page).is_some());
    }

    #[test]
    fn pagina_piena_o_quasi_vuota_resta_com_e() {
        assert_eq!(margins(&sheet(1000, 1500, 250, (0, 0, 1000, 1500))), None, "niente margini");
        assert_eq!(margins(&sheet(1000, 1500, 250, (400, 600, 100, 100))), None, "rifilo sospetto: resterebbe poco");
        assert_eq!(margins(&sheet(1000, 1500, 250, (0, 0, 0, 0))), None, "pagina bianca");
    }

    #[test]
    fn rotazioni() {
        // 3 x 2: i pixel numerati 0..6 nel canale rosso
        let rgba: Vec<u8> = (0..6u8).flat_map(|v| [v, 0, 0, 255]).collect();
        let p = || Page { width: 3, height: 2, rgba: rgba.clone(), opaque: true };
        let red = |p: &Page| p.rgba.iter().step_by(4).copied().collect::<Vec<u8>>();
        // 0 1 2      in senso orario:  3 0
        // 3 4 5                        4 1
        //                              5 2
        let cw = rotate(p(), 90);
        assert_eq!((cw.width, cw.height, red(&cw)), (2, 3, vec![3, 0, 4, 1, 5, 2]));
        let ccw = rotate(p(), 270);
        assert_eq!((ccw.width, ccw.height, red(&ccw)), (2, 3, vec![2, 5, 1, 4, 0, 3]));
        assert_eq!(red(&rotate(p(), 180)), [5, 4, 3, 2, 1, 0]);
        assert_eq!(red(&rotate(rotate(p(), 90), 270)), red(&p()), "avanti e indietro: come prima");
    }
}
