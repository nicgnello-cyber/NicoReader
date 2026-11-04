use super::*;

#[test]
fn mezza_precisione() {
    for (h, f) in [
        (0x3c00, 1.0),
        (0xc000, -2.0),
        (0x3555, 0.333_251_95),
        (0x0001, 5.960_464_5e-8),
        (0x0003, 1.788_139_3e-7),
        (0x7bff, 65504.0),
    ] {
        assert_eq!(f16_to_f32(h), f, "{h:#x}");
    }
    assert_eq!(f16_to_f32(0x0000), 0.0);
    assert!(f16_to_f32(0x7c00).is_infinite());
}

#[test]
fn i_pesi_si_leggono_tutti() {
    let net = parse(WEIGHTS).unwrap();
    assert_eq!(net.layers.len(), LAYERS);
    assert_eq!((net.layers[0].in_groups, net.layers[0].out_groups), (1, 16));
    assert_eq!((net.layers[17].in_groups, net.layers[17].out_groups), (16, 12));
    // mat4x4: 16*1*9 + 16 * 16*16*9 + 12*16*9
    assert_eq!(net.mats.len() / 16, 144 + 16 * 2304 + 1728);
    // bias e pendenze: 17 * (16 + 16) + 12
    assert_eq!(net.vecs.len() / 4, 17 * 32 + 12);
}

/// La rete, scritta nel modo piu' diretto: il riferimento per la scheda
/// video. Pixel per pixel, canale per canale, con il padding a zero.
fn reference(page: &Page, scale: u32) -> Vec<u8> {
    let net = parse(WEIGHTS).unwrap();
    let (w, h) = (page.width as usize, page.height as usize);
    let input: Vec<f32> = page.rgba.iter().map(|&b| b as f32 / 255.0).collect();
    // mappa[canale][y][x]
    let mut map: Vec<f32> =
        (0..3).flat_map(|c| (0..w * h).map(move |p| (c, p))).map(|(c, p)| input[p * 4 + c]).collect();
    let mut cin = 3;
    for (k, l) in net.layers.iter().enumerate() {
        let cout = l.out_groups as usize * 4;
        let mut next = vec![0f32; cout * w * h];
        for o in 0..cout {
            let bias = net.vecs[l.vecs as usize * 4 + o];
            for y in 0..h {
                for x in 0..w {
                    let mut acc = bias;
                    for tap in 0..9 {
                        let (sx, sy) = (x as i32 + tap as i32 % 3 - 1, y as i32 + tap as i32 / 3 - 1);
                        if sx < 0 || sy < 0 || sx >= w as i32 || sy >= h as i32 {
                            continue;
                        }
                        for i in 0..cin {
                            let (og, ig, r, c) = (o / 4, i / 4, o % 4, i % 4);
                            let m = (l.weights as usize + (og * l.in_groups as usize + ig) * 9 + tap) * 16;
                            acc += net.mats[m + c * 4 + r] * map[(i * h + sy as usize) * w + sx as usize];
                        }
                    }
                    if k < LAYERS - 1 && acc < 0.0 {
                        acc *= net.vecs[(l.vecs as usize + l.out_groups as usize) * 4 + o];
                    }
                    next[(o * h + y) * w + x] = acc;
                }
            }
        }
        map = next;
        cin = cout;
    }
    let (ow, oh, k) = (w * scale as usize, h * scale as usize, 4 / scale as usize);
    let mut out = vec![0u8; ow * oh * 4];
    for oy in 0..oh {
        for ox in 0..ow {
            let mut px = [0f32; 4];
            for di in 0..k {
                for dj in 0..k {
                    let (sy, sx) = (oy * k + di, ox * k + dj);
                    let (x, y, i, j) = (sx / 4, sy / 4, sy % 4, sx % 4);
                    for (c, v) in px.iter_mut().enumerate().take(3) {
                        *v += map[((c * 16 + i * 4 + j) * h + y) * w + x] + input[(y * w + x) * 4 + c];
                    }
                    px[3] += input[(y * w + x) * 4 + 3];
                }
            }
            for c in 0..4 {
                out[(oy * ow + ox) * 4 + c] = ((px[c] / (k * k) as f32).clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }
    out
}

/// Un'immagine piccola con un po' di tutto: righe, una diagonale, rumore.
fn sample(w: u32, h: u32) -> Page {
    let mut rgba = Vec::new();
    let mut seed = 7u32;
    for y in 0..h {
        for x in 0..w {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (seed >> 27) as u8;
            let ink = if x == y || y % 5 == 0 || x % 7 == 3 { 20 } else { 220 };
            rgba.extend([ink + noise, ink.saturating_sub(noise), 200 - noise * 3, 255]);
        }
    }
    Page { width: w, height: h, rgba, opaque: true }
}

/// Sulla scheda video (anche software: qui conta il risultato), a tasselli
/// piccoli perche' le giunture si vedano, contro il riferimento. Senza
/// nessuna scheda la prova non si puo' fare e lo dice.
#[test]
fn la_scheda_video_fa_quello_che_fa_la_rete() {
    let Ok(gpu) = pollster::block_on(Esrgan::with_tile(true, 8)) else {
        eprintln!("nessuna scheda video, nemmeno software: prova saltata");
        return;
    };
    let page = sample(20, 13);
    for scale in [4, 2] {
        let got = gpu.upscale(&page, scale, &|| true).unwrap();
        assert_eq!((got.width, got.height), (20 * scale, 13 * scale));
        let want = reference(&page, scale);
        let worst = got.rgba.iter().zip(&want).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
        assert!(worst <= 1, "{scale}x: differenza massima {worst} su 255 ({})", gpu.describe());
    }
    assert!(gpu.upscale(&page, 4, &|| false).is_none(), "fermata a meta'");
}
