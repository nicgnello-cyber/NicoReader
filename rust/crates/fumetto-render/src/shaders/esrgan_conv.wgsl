// Una convoluzione 3x3 di Real-ESRGAN (realesr-animevideov3), con la sua
// PReLU. I canali stanno a gruppi di quattro (vec4), piano per piano:
// mappa[gruppo][y][x]. Fuori dalla regione valgono zero, come il padding
// della rete.
//
// Ogni invocazione fa PX pixel in fila e PER gruppi d'uscita (la terza
// dimensione del lancio sceglie quali): ogni peso letto serve PX pixel, ogni
// pixel letto PER gruppi. IN, OUT, PER, PX e PRELU li sostituisce esrgan.rs:
// con i cicli a numero fisso il compilatore tiene gli accumulatori nei
// registri.

const IN: u32 = {IN}u;
const OUT: u32 = {OUT}u;
const PER: u32 = {PER}u;
const PX: u32 = {PX}u;
const PRELU: bool = {PRELU};

struct Params {
    w: u32,
    h: u32,
    // il primo peso (mat4x4) e il primo vec4 (bias, poi pendenze) dello strato
    weights: u32,
    vecs: u32,
}

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> src: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> dst: array<vec4<f32>>;
// [gruppo d'uscita][gruppo d'entrata][tap]: colonna c, riga r = peso
// (uscita 4*og + r, entrata 4*ig + c)
@group(0) @binding(3) var<storage, read> weights: array<mat4x4<f32>>;
@group(0) @binding(4) var<storage, read> vecs: array<vec4<f32>>;

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let x0 = id.x * PX;
    if (x0 >= p.w || id.y >= p.h) {
        return;
    }
    let y = i32(id.y);
    let og0 = id.z * PER;
    let plane = p.w * p.h;
    var acc: array<vec4<f32>, PER * PX>;
    for (var o = 0u; o < PER; o++) {
        let b = vecs[p.vecs + og0 + o];
        for (var k = 0u; k < PX; k++) {
            acc[o * PX + k] = b;
        }
    }
    for (var t = 0u; t < 9u; t++) {
        let sy = y + i32(t / 3u) - 1;
        if (sy < 0 || sy >= i32(p.h)) {
            continue;
        }
        let dx = i32(t % 3u) - 1;
        for (var ig = 0u; ig < IN; ig++) {
            var v: array<vec4<f32>, PX>;
            for (var k = 0u; k < PX; k++) {
                let sx = i32(x0 + k) + dx;
                if (sx >= 0 && sx < i32(p.w)) {
                    v[k] = src[ig * plane + u32(sy) * p.w + u32(sx)];
                } else {
                    v[k] = vec4<f32>(0.0);
                }
            }
            for (var o = 0u; o < PER; o++) {
                let m = weights[p.weights + ((og0 + o) * IN + ig) * 9u + t];
                for (var k = 0u; k < PX; k++) {
                    acc[o * PX + k] += m * v[k];
                }
            }
        }
    }
    for (var k = 0u; k < PX; k++) {
        let x = x0 + k;
        if (x >= p.w) {
            break;
        }
        for (var o = 0u; o < PER; o++) {
            var a = acc[o * PX + k];
            if (PRELU) {
                a = select(a * vecs[p.vecs + OUT + og0 + o], a, a > vec4<f32>(0.0));
            }
            dst[(og0 + o) * plane + id.y * p.w + x] = a;
        }
    }
}
