//! La tavola di prova del moire'.
//!
//! Sintetica, cosi' si sa esattamente quanto inchiostro c'e' in ogni punto e
//! il risultato si misura, non solo si guarda. Quattro fasce:
//!   1. cerchi concentrici sempre piu' fitti (la prova classica dell'aliasing)
//!   2. retino a punti a 45 gradi, passo 8 pixel, dal 10 al 90% di inchiostro
//!   3. tratteggio di linee sottili (1 pixel ogni 3), verticali e diagonali
//!   4. retino al 50% con sopra cerchi di china spessi, come un disegno vero

use fumetto_core::Page;

pub const BANDS: [&str; 4] = ["cerchi concentrici", "retino 45 gradi", "tratteggio", "retino + china"];

pub fn tavola(w: u32, h: u32) -> Page {
    let mut rgba = vec![255u8; (w * h * 4) as usize];
    let band_h = h / 4;
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 + 0.5, (y % band_h) as f32 + 0.5);
            let ink = match y / band_h {
                0 => {
                    // frequenza locale r / w cicli per pixel: arriva a 0,5 (il limite) sul bordo
                    let (dx, dy) = (fx - w as f32 / 2.0, fy - band_h as f32 / 2.0);
                    let r2 = dx * dx + dy * dy;
                    0.5 + 0.5 * (std::f32::consts::PI * r2 / w as f32).cos()
                }
                1 => dot(fx, fy, 8.0, true, 0.1 + 0.8 * x as f32 / w as f32),
                2 => {
                    let line = if x < w / 2 { x % 3 == 0 } else { (x + y) % 3 == 0 };
                    if line { 1.0 } else { 0.0 }
                }
                _ => {
                    let (dx, dy) = ((fx % 600.0) - 300.0, (fy % 600.0) - 300.0);
                    let r = (dx * dx + dy * dy).sqrt();
                    let ring = [(90.0, 12.0), (180.0, 5.0), (260.0, 2.0)]
                        .iter()
                        .any(|&(radius, half)| (r - radius).abs() < half);
                    if ring { 1.0 } else { dot(fx, fy, 6.0, false, 0.5) }
                }
            };
            // l'inchiostro assorbe: 1 = nero. I valori intermedi (solo nella fascia
            // 1) sono luce riflessa, quindi si codificano in sRGB come farebbe uno scanner.
            let v = to_srgb(1.0 - ink);
            let i = ((y * w + x) * 4) as usize;
            rgba[i..i + 3].fill(v);
        }
    }
    Page { width: w, height: h, rgba, opaque: true }
}

/// Un punto di retino: 1 se (x, y) cade dentro il punto della sua cella.
fn dot(x: f32, y: f32, period: f32, rotated: bool, coverage: f32) -> f32 {
    let (u, v) =
        if rotated { ((x + y) / std::f32::consts::SQRT_2, (x - y) / std::f32::consts::SQRT_2) } else { (x, y) };
    let (cu, cv) = (u.rem_euclid(period) - period / 2.0, v.rem_euclid(period) - period / 2.0);
    let radius = period * (coverage / std::f32::consts::PI).sqrt();
    if cu * cu + cv * cv < radius * radius { 1.0 } else { 0.0 }
}

pub fn to_linear(v: u8) -> f32 {
    let c = v as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

pub fn to_srgb(l: f32) -> u8 {
    let l = l.clamp(0.0, 1.0);
    let c = if l <= 0.0031308 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
    (c * 255.0).round() as u8
}

/// Il riferimento: media esatta di ogni blocco k x k in luce lineare, cioe' la
/// quantita' di luce che l'occhio riceverebbe da quel pezzo di carta.
pub fn reference(page: &Page, k: u32) -> Vec<u8> {
    let (w, h) = (page.width / k, page.height / k);
    let mut out = vec![255u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0f32; 3];
            for sy in y * k..(y + 1) * k {
                for sx in x * k..(x + 1) * k {
                    let i = ((sy * page.width + sx) * 4) as usize;
                    for (s, &v) in sum.iter_mut().zip(&page.rgba[i..i + 3]) {
                        *s += to_linear(v);
                    }
                }
            }
            let i = ((y * w + x) * 4) as usize;
            for c in 0..3 {
                out[i + c] = to_srgb(sum[c] / (k * k) as f32);
            }
        }
    }
    out
}

/// Scarto quadratico medio (in livelli sRGB 0-255) fascia per fascia.
pub fn rms_by_band(a: &[u8], b: &[u8], w: u32, h: u32) -> [f32; 4] {
    let band_h = h / 4;
    let mut out = [0f32; 4];
    for (band, slot) in out.iter_mut().enumerate() {
        let (mut sum, mut n) = (0f64, 0u64);
        for y in band as u32 * band_h..(band as u32 + 1) * band_h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                for c in 0..3 {
                    let d = a[i + c] as f64 - b[i + c] as f64;
                    sum += d * d;
                    n += 1;
                }
            }
        }
        *slot = (sum / n as f64).sqrt() as f32;
    }
    out
}

/// Ritagli affiancati e ingranditi 3 volte senza filtri, per guardarli da vicino.
pub fn montage(images: &[&[u8]], w: u32, crop: (u32, u32, u32, u32), zoom: u32) -> image::RgbaImage {
    let (cx, cy, cw, ch) = crop;
    let gap = 8;
    let mut out = image::RgbaImage::from_pixel(
        (cw * zoom + gap) * images.len() as u32 - gap,
        ch * zoom,
        image::Rgba([40, 40, 40, 255]),
    );
    for (n, img) in images.iter().enumerate() {
        for y in 0..ch * zoom {
            for x in 0..cw * zoom {
                let i = (((cy + y / zoom) * w + cx + x / zoom) * 4) as usize;
                let px = image::Rgba([img[i], img[i + 1], img[i + 2], 255]);
                out.put_pixel(n as u32 * (cw * zoom + gap) + x, y, px);
            }
        }
    }
    out
}
