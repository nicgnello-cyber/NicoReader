// L'ultimo passo di Real-ESRGAN: i 48 canali dell'ultima convoluzione
// diventano 4x4 pixel per ogni pixel d'entrata (pixel shuffle), piu' il pixel
// d'entrata stesso (la rete impara solo la differenza). Per il 2x si fa la
// media di ogni 2x2, come il modello x2 che rimpicciolisce l'uscita del 4x.
// Si scrive solo il tassello, senza il bordo calcolato in piu'.

struct Params {
    // la regione calcolata
    w: u32,
    h: u32,
    // il tassello dentro la regione, in pixel d'entrata
    x0: u32,
    y0: u32,
    tw: u32,
    th: u32,
    // 2 o 4
    scale: u32,
    _pad: u32,
}

@group(0) @binding(0) var<uniform> p: Params;
// l'uscita dell'ultima convoluzione: 12 gruppi, il canale 16c + 4i + j e'
// il colore c del sottopixel (riga i, colonna j)
@group(0) @binding(1) var<storage, read> feat: array<vec4<f32>>;
// la regione d'entrata, rgba fra 0 e 1
@group(0) @binding(2) var<storage, read> image: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let ow = p.tw * p.scale;
    if (id.x >= ow || id.y >= p.th * p.scale) {
        return;
    }
    let plane = p.w * p.h;
    let k = 4u / p.scale;
    var rgb = vec3<f32>(0.0);
    var alpha = 0.0;
    for (var di = 0u; di < k; di++) {
        for (var dj = 0u; dj < k; dj++) {
            let sy = id.y * k + di;
            let sx = id.x * k + dj;
            let x = p.x0 + sx / 4u;
            let y = p.y0 + sy / 4u;
            let i = sy % 4u;
            let j = sx % 4u;
            let at = y * p.w + x;
            let base = image[at];
            rgb += vec3<f32>(
                feat[(0u + i) * plane + at][j],
                feat[(4u + i) * plane + at][j],
                feat[(8u + i) * plane + at][j],
            ) + base.rgb;
            alpha += base.a;
        }
    }
    let n = f32(k * k);
    out[id.y * ow + id.x] = pack4x8unorm(clamp(vec4<f32>(rgb / n, alpha / n), vec4<f32>(0.0), vec4<f32>(1.0)));
}
