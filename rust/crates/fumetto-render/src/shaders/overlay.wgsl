// Le forme dell'interfaccia, descritte dalla loro distanza dal bordo: niente
// triangoli, nessun bordo seghettato, a qualsiasi scala.
//   - rettangoli con gli angoli arrotondati, pieni o solo il contorno, con i
//     bordi morbidi (le ombre) e una sfumatura dall'alto in basso;
//   - linee con le estremita' tonde: le icone sono fatte di queste.

struct View {
    size: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> view: View;

struct Shape {
    // rettangolo: x, y, larghezza, altezza; linea: x1, y1, x2, y2 (pixel)
    @location(0) rect: vec4<f32>,
    @location(1) top: vec4<f32>,    // colore in alto (sRGB, alfa non premoltiplicata)
    @location(2) bottom: vec4<f32>, // colore in basso
    // raggio (per la linea: meta' spessore), sfocatura, contorno, 1 = linea
    @location(3) style: vec4<f32>,
};

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) p: vec2<f32>,
    @location(1) rect: vec4<f32>,
    @location(2) top: vec4<f32>,
    @location(3) bottom: vec4<f32>,
    @location(4) style: vec4<f32>,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32, s: Shape) -> Out {
    // il riquadro da coprire, allargato di quanto sfuma piu' un pixel
    var lo: vec2<f32>;
    var size: vec2<f32>;
    let grow = s.style.y + 1.0;
    if (s.style.w > 0.5) {
        let r = s.style.x + grow;
        lo = min(s.rect.xy, s.rect.zw) - vec2<f32>(r);
        size = abs(s.rect.zw - s.rect.xy) + vec2<f32>(2.0 * r);
    } else {
        lo = s.rect.xy - vec2<f32>(grow);
        size = s.rect.zw + vec2<f32>(2.0 * grow);
    }
    let corner = vec2<f32>(f32(vi & 1u), f32(vi >> 1u));
    let p = lo + corner * size;
    var o: Out;
    o.pos = vec4<f32>(p / view.size * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    o.p = p;
    o.rect = s.rect;
    o.top = s.top;
    o.bottom = s.bottom;
    o.style = s.style;
    return o;
}

fn rounded_box(p: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn fs(o: Out) -> @location(0) vec4<f32> {
    var d: f32;
    var c = o.top;
    if (o.style.w > 0.5) {
        let a = o.rect.xy;
        let ba = o.rect.zw - a;
        let pa = o.p - a;
        let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-4), 0.0, 1.0);
        d = length(pa - ba * h) - o.style.x;
    } else {
        let half = o.rect.zw * 0.5;
        let r = min(o.style.x, min(half.x, half.y));
        d = rounded_box(o.p - (o.rect.xy + half), half, r);
        if (o.style.z > 0.0) {
            // solo il contorno, spesso style.z, all'interno del bordo
            d = abs(d + o.style.z * 0.5) - o.style.z * 0.5;
        }
        let t = clamp((o.p.y - o.rect.y) / max(o.rect.w, 1.0), 0.0, 1.0);
        c = mix(o.top, o.bottom, t);
    }
    let soft = max(o.style.y, 0.5);
    let a = 1.0 - smoothstep(-soft, soft, d);
    return vec4<f32>(c.rgb * c.a * a, c.a * a);
}
