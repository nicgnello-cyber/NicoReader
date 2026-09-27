// Mette un tassello di pagina a schermo, sul nero.
//
// Due modi:
//   0  copia esatta: il tassello e' gia' alla misura dello schermo (lo ha
//      rimpicciolito il processore, con il filtro giusto). Un texel per pixel.
//   1  bicubico Catmull-Rom: la pagina va ingrandita (webtoon stretti, zoom).
//      Ingrandire non crea moire', basta interpolare bene, e lo si fa qui a
//      ogni fotogramma invece di tenere in memoria una copia ingrandita.

struct Quad {
    rect: vec4f,      // x, y, larghezza, altezza sullo schermo, in pixel
    src: vec4f,       // x, y, larghezza, altezza nel tassello, in texel
    viewport: vec2f,  // misura del bersaglio in pixel
    mode: u32,
    _pad: u32,
    adjust: vec4f,    // luminosita', contrasto, 1/gamma, 1 = da applicare
    clip: vec4f,      // centro x, y e raggio di un cerchio, 1 = solo dentro (la lente)
}

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(0) var<uniform> q: Quad;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    // striscia di due triangoli: (0,0) (1,0) (0,1) (1,1)
    let k = vec2f(f32(i & 1u), f32((i >> 1u) & 1u));
    let px = q.rect.xy + k * q.rect.zw;
    return vec4f(px.x / q.viewport.x * 2.0 - 1.0, 1.0 - px.y / q.viewport.y * 2.0, 0.0, 1.0);
}

fn texel(p: vec2i) -> vec4f {
    let size = vec2i(textureDimensions(tex)) - 1;
    return textureLoad(tex, clamp(p, vec2i(0), size), 0);
}

// pesi di Catmull-Rom per i quattro campioni attorno a t (0 <= t < 1)
fn catmull_rom(t: f32) -> vec4f {
    let t2 = t * t;
    let t3 = t2 * t;
    return vec4f(
        -0.5 * t3 + t2 - 0.5 * t,
        1.5 * t3 - 2.5 * t2 + 1.0,
        -1.5 * t3 + 2.0 * t2 + 0.5 * t,
        0.5 * t3 - 0.5 * t2,
    );
}

@fragment
fn fs(@builtin(position) pos: vec4f) -> @location(0) vec4f {
    if q.clip.w > 0.5 && distance(pos.xy, q.clip.xy) > q.clip.z {
        discard;
    }
    // coordinata continua nel tassello; i centri dei texel stanno a +0,5
    let t = q.src.xy + (pos.xy - q.rect.xy) / q.rect.zw * q.src.zw;
    var c: vec4f;
    if q.mode == 0u {
        c = texel(vec2i(floor(t)));
    } else {
        let base = floor(t - 0.5);
        let f = t - 0.5 - base;
        let wx = catmull_rom(f.x);
        let wy = catmull_rom(f.y);
        c = vec4f(0.0);
        for (var j = 0; j < 4; j++) {
            var row = vec4f(0.0);
            for (var i = 0; i < 4; i++) {
                row += wx[i] * texel(vec2i(base) + vec2i(i - 1, j - 1));
            }
            c += wy[j] * row;
        }
        c = clamp(c, vec4f(0.0), vec4f(1.0));
    }
    var rgb = c.rgb * c.a;   // sul nero
    if q.adjust.w > 0.5 {
        // come Adjust::apply: gamma, contrasto attorno al grigio medio, luminosita'
        rgb = pow(clamp(rgb, vec3f(0.0), vec3f(1.0)), vec3f(q.adjust.z));
        rgb = clamp((rgb - 0.5) * q.adjust.y + 0.5 + q.adjust.x, vec3f(0.0), vec3f(1.0));
    }
    return vec4f(rgb, 1.0);
}
