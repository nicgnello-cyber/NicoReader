//! L'interfaccia disegnata dal processore: nei primi istanti, prima che la
//! scheda video sia pronta, la finestra si copia da qui (cpu_view). Le forme
//! sono quelle di `overlay.wgsl`, calcolate pixel per pixel; i testi hanno gli
//! stessi caratteri e la stessa impaginazione di [`Overlay`](crate::Overlay),
//! e si mescolano allo stesso modo (valori sRGB, come nei browser): quando
//! arriva la scheda video il passaggio non si vede.

use glyphon::{SwashCache, SwashContent};

use crate::text::Typesetter;
use crate::{Measure, Rect, Scene, Text};

pub struct SoftOverlay {
    text: Typesetter,
    swash: SwashCache,
}

/// Dove si disegna: pixel 0x00RRGGBB, riga dopo riga.
pub struct Canvas<'a> {
    pub pixels: &'a mut [u32],
    pub width: u32,
    pub height: u32,
}

impl Default for SoftOverlay {
    fn default() -> Self {
        SoftOverlay::new()
    }
}

impl SoftOverlay {
    pub fn new() -> SoftOverlay {
        SoftOverlay { text: Typesetter::new(), swash: SwashCache::new() }
    }

    /// Disegna la scena sopra cio' che la tela contiene gia', strato per
    /// strato: prima i rettangoli, poi i testi.
    pub fn draw(&mut self, canvas: &mut Canvas, scene: &Scene) {
        for layer in &scene.layers {
            for r in &layer.rects {
                fill(canvas, r);
            }
            for t in &layer.texts {
                self.draw_text(canvas, t);
            }
        }
        self.text.end_frame();
    }

    /// Un testo, come lo mette glyphon: al massimo due righe, la riga di
    /// base arrotondata al pixel.
    fn draw_text(&mut self, canvas: &mut Canvas, t: &Text) {
        let (fonts, buffers) = self.text.layer(std::slice::from_ref(t));
        let buffer = buffers[0];
        let (left, top) = (t.x.round(), t.y.round());
        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                let physical = glyph.physical((left, top), 1.0);
                let color = glyph.color_opt.map_or(t.color, |c| [c.r(), c.g(), c.b(), c.a()]);
                let Some(image) = self.swash.get_image(fonts, physical.cache_key) else { continue };
                let x0 = physical.x + image.placement.left;
                let y0 = run.line_y.round() as i32 + physical.y - image.placement.top;
                let (w, h) = (image.placement.width as i32, image.placement.height as i32);
                for gy in 0..h {
                    for gx in 0..w {
                        let i = (gy * w + gx) as usize;
                        let (rgb, alpha) = match image.content {
                            // la copertura del carattere, nel colore del testo
                            SwashContent::Mask | SwashContent::SubpixelMask => {
                                let coverage = match image.content {
                                    SwashContent::Mask => image.data[i],
                                    // tre coperture, una per canale: la media
                                    _ => {
                                        let p = &image.data[i * 4..i * 4 + 3];
                                        ((p[0] as u16 + p[1] as u16 + p[2] as u16) / 3) as u8
                                    }
                                };
                                ([color[0], color[1], color[2]], unit(color[3]) * unit(coverage))
                            }
                            // un carattere a colori (le emoji): i suoi pixel
                            SwashContent::Color => {
                                let p = &image.data[i * 4..i * 4 + 4];
                                ([p[0], p[1], p[2]], unit(p[3]) * unit(color[3]))
                            }
                        };
                        let rgb = rgb.map(unit);
                        canvas.blend(x0 + gx, y0 + gy, rgb, alpha);
                    }
                }
            }
        }
    }
}

impl Measure for SoftOverlay {
    fn width(&mut self, t: &Text) -> f32 {
        self.text.width(t)
    }

    fn lines(&mut self, t: &Text) -> usize {
        self.text.lines(t)
    }
}

impl Canvas<'_> {
    /// Mescola un colore (sRGB in 0..1, alfa non premoltiplicata) sopra un
    /// pixel; fuori dalla tela non fa niente.
    fn blend(&mut self, x: i32, y: i32, rgb: [f32; 3], alpha: f32) {
        if alpha <= 0.0 || x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let px = &mut self.pixels[y as usize * self.width as usize + x as usize];
        let old = [(*px >> 16) & 0xff, (*px >> 8) & 0xff, *px & 0xff].map(|c| c as f32 / 255.0);
        let a = alpha.min(1.0);
        let [r, g, b] = [0, 1, 2].map(|k| ((rgb[k] * a + old[k] * (1.0 - a)) * 255.0).round().clamp(0.0, 255.0) as u32);
        *px = (r << 16) | (g << 8) | b;
    }
}

fn unit(v: u8) -> f32 {
    v as f32 / 255.0
}

/// Un rettangolo o una linea, come `fs` in overlay.wgsl: la distanza dal
/// bordo, sfumata su mezzo pixel (o su quanto dice `blur`).
fn fill(canvas: &mut Canvas, r: &Rect) {
    // il riquadro da coprire, allargato di quanto sfuma piu' un pixel
    let grow = r.blur + 1.0;
    let (lo, hi) = if r.line {
        let g = r.radius + grow;
        ((r.x.min(r.w) - g, r.y.min(r.h) - g), (r.x.max(r.w) + g, r.y.max(r.h) + g))
    } else {
        ((r.x - grow, r.y - grow), (r.x + r.w + grow, r.y + r.h + grow))
    };
    let x0 = (lo.0.floor() as i32).max(0);
    let y0 = (lo.1.floor() as i32).max(0);
    let x1 = (hi.0.ceil() as i32).min(canvas.width as i32);
    let y1 = (hi.1.ceil() as i32).min(canvas.height as i32);
    let soft = r.blur.max(0.5);
    for y in y0..y1 {
        // al centro del pixel, come la scheda video
        let py = y as f32 + 0.5;
        let color = if r.line {
            r.top
        } else {
            let t = ((py - r.y) / r.h.max(1.0)).clamp(0.0, 1.0);
            std::array::from_fn(|k| r.top[k] + (r.bottom[k] - r.top[k]) * t)
        };
        if color[3] <= 0.0 {
            continue;
        }
        for x in x0..x1 {
            let p = (x as f32 + 0.5, py);
            let d = if r.line { line_distance(p, r) } else { box_distance(p, r) };
            let a = 1.0 - smoothstep(-soft, soft, d);
            canvas.blend(x, y, [color[0], color[1], color[2]], color[3] * a);
        }
    }
}

/// Dal segmento (x, y)-(w, h), meno meta' dello spessore.
fn line_distance((px, py): (f32, f32), r: &Rect) -> f32 {
    let (bx, by) = (r.w - r.x, r.h - r.y);
    let (ax, ay) = (px - r.x, py - r.y);
    let h = ((ax * bx + ay * by) / (bx * bx + by * by).max(1e-4)).clamp(0.0, 1.0);
    (ax - bx * h).hypot(ay - by * h) - r.radius
}

/// Dal rettangolo con gli angoli arrotondati; per un contorno, dalla sua
/// striscia interna.
fn box_distance((px, py): (f32, f32), r: &Rect) -> f32 {
    let half = (r.w * 0.5, r.h * 0.5);
    let radius = r.radius.min(half.0.min(half.1));
    let q = ((px - (r.x + half.0)).abs() - half.0 + radius, (py - (r.y + half.1)).abs() - half.1 + radius);
    let d = q.0.max(0.0).hypot(q.1.max(0.0)) + q.0.max(q.1).min(0.0) - radius;
    if r.stroke > 0.0 { (d + r.stroke * 0.5).abs() - r.stroke * 0.5 } else { d }
}

fn smoothstep(lo: f32, hi: f32, x: f32) -> f32 {
    let t = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Face, Layer};

    fn canvas_of(pixels: &mut [u32], width: u32) -> Canvas<'_> {
        let height = pixels.len() as u32 / width;
        Canvas { pixels, width, height }
    }

    #[test]
    fn rettangoli_linee_e_testi() {
        let mut pixels = vec![0u32; 64 * 32];
        let mut soft = SoftOverlay::new();
        let scene = Scene {
            layers: vec![Layer {
                rects: vec![
                    Rect::new(2.0, 2.0, 10.0, 10.0, [1.0, 0.0, 0.0, 1.0]),
                    Rect::new(20.0, 2.0, 10.0, 10.0, [1.0, 1.0, 1.0, 1.0]).stroke(2.0),
                    Rect::line(40.0, 6.0, 60.0, 6.0, 2.0, [0.0, 0.0, 1.0, 0.5]),
                ],
                texts: vec![Text::new("Wol", Face::Sans, 14.0, [255, 255, 255, 255]).at(2.0, 14.0)],
            }],
        };
        soft.draw(&mut canvas_of(&mut pixels, 64), &scene);
        let at = |x: usize, y: usize| pixels[y * 64 + x];
        assert_eq!(at(6, 6), 0xff0000, "dentro il rettangolo pieno");
        assert_eq!(at(0, 0), 0, "fuori resta com'era");
        assert_eq!(at(20, 6), 0xffffff, "il contorno");
        assert_eq!(at(25, 6), 0, "dentro il contorno non si riempie");
        assert_eq!(at(50, 6), 0x000080, "la linea, a meta' trasparente sul nero");
        assert!((14..32).any(|y| (0..40).any(|x| at(x, y) > 0x808080)), "il testo si vede");
    }

    #[test]
    fn misura_come_la_scheda_video() {
        // la stessa impaginazione: chi misura prima della scheda video non
        // vede cambiare niente quando arriva
        let mut soft = SoftOverlay::new();
        let t = Text::new("Tutta la serie", Face::Sans, 14.0, [255; 4]);
        let w = soft.width(&t);
        assert!(w > 50.0 && w < 120.0, "{w}");
        assert_eq!(soft.lines(&t.clone().boxed(w + 1.0, crate::Align::Left)), 1);
        assert!(soft.lines(&t.clone().boxed(w * 0.7, crate::Align::Left)) >= 2, "va a capo");
    }
}
