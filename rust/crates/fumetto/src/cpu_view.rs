//! Le prime pagine, prima che la scheda video sia pronta.
//!
//! Prepararla costa fino a un secondo e mezzo: su un portatile, per elencare le
//! schede, si sveglia anche quella dedicata. Le pagine invece escono dai thread
//! gia' alla misura dello schermo, quindi basta copiarle nella finestra dal
//! processore (GDI su Windows, CoreGraphics su macOS, X11/Wayland su Linux).
//! Sopra, le copertine (con gli angoli tondi e la dissolvenza) e
//! l'interfaccia, disegnata dal processore con gli stessi caratteri
//! (`SoftOverlay`): anche la libreria e la schermata iniziale si vedono subito.
//! Quando la scheda video arriva prende il posto di questa copia, con gli
//! stessi pixel: il passaggio non si vede.
//!
//! Se la scheda video non arriva mai (driver rotto, macchina virtuale), si
//! resta qui: piu' lento, ma si legge lo stesso.

use std::num::NonZeroU32;
use std::sync::Arc;

use fumetto_core::{Page, enlarge, resize};
use fumetto_render::{Canvas, Scene, SoftOverlay};
use winit::window::Window;

pub struct CpuView {
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
    // tenuto vivo quanto la superficie
    _context: softbuffer::Context<Arc<Window>>,
    /// L'interfaccia, e chi misura i suoi testi finche' si disegna da qui.
    pub soft: SoftOverlay,
}

/// Una pagina (o una copertina) da copiare e il rettangolo dove va, in pixel
/// della finestra.
pub struct Item<'a> {
    pub page: &'a Page,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Raggio degli angoli (le copertine); 0 = angoli vivi.
    pub radius: f32,
    /// Quanto si vede, sopra cio' che c'e' sotto.
    pub opacity: f32,
    /// Con luminosita', contrasto e gamma: le pagine si', le copertine no.
    pub adjusted: bool,
}

impl<'a> Item<'a> {
    /// Una pagina, piena e regolata.
    pub fn page(page: &'a Page, x: f32, y: f32, w: f32, h: f32) -> Item<'a> {
        Item { page, x, y, w, h, radius: 0.0, opacity: 1.0, adjusted: true }
    }
}

impl CpuView {
    pub fn new(window: Arc<Window>) -> Result<CpuView, String> {
        let context = softbuffer::Context::new(window.clone()).map_err(|e| e.to_string())?;
        let surface = softbuffer::Surface::new(&context, window).map_err(|e| e.to_string())?;
        Ok(CpuView { surface, _context: context, soft: SoftOverlay::new() })
    }

    /// Disegna le pagine sul nero, sopra l'interfaccia, e consegna alla
    /// finestra. `table`: luminosita', contrasto e gamma, come valore per
    /// valore (la stessa formula che usera' la scheda video, cosi' il
    /// passaggio non si vede).
    pub fn draw(
        &mut self, size: (u32, u32), items: &[Item], table: Option<&[u8; 256]>, scene: &Scene,
    ) -> Result<(), String> {
        let (vw, vh) = size;
        let (Some(w), Some(h)) = (NonZeroU32::new(vw), NonZeroU32::new(vh)) else { return Ok(()) };
        self.surface.resize(w, h).map_err(|e| e.to_string())?;
        let mut buffer = self.surface.buffer_mut().map_err(|e| e.to_string())?;
        buffer.fill(0);
        for it in items {
            let (w, h) = (it.w.round().max(1.0) as u32, it.h.round().max(1.0) as u32);
            // di solito la pagina e' gia' della misura giusta; se la finestra e'
            // cambiata nel frattempo, la si adatta con i filtri di sempre
            let scaled;
            let page = match (it.page.width, it.page.height) {
                (pw, ph) if (pw, ph) == (w, h) => it.page,
                (pw, _) if pw > w => {
                    scaled = resize(it.page, w, h, true);
                    &scaled
                }
                _ => {
                    scaled = enlarge(it.page, w, h);
                    &scaled
                }
            };
            let look = Look { table: table.filter(|_| it.adjusted), radius: it.radius, opacity: it.opacity };
            blit(&mut buffer, vw, vh, page, it.x.round() as i64, it.y.round() as i64, &look);
        }
        self.soft.draw(&mut Canvas { pixels: &mut buffer, width: vw, height: vh }, scene);
        buffer.present().map_err(|e| e.to_string())
    }
}

/// Come si copia una pagina.
struct Look<'a> {
    table: Option<&'a [u8; 256]>,
    radius: f32,
    opacity: f32,
}

/// Copia `page` in (x, y), tagliando quello che esce dalla finestra.
/// softbuffer vuole 0x00RRGGBB; l'alfa si compone sul nero, e le copertine
/// (angoli tondi, dissolvenza) sopra cio' che c'e' sotto.
fn blit(buffer: &mut [u32], vw: u32, vh: u32, page: &Page, x: i64, y: i64, look: &Look) {
    let identity: [u8; 256] = std::array::from_fn(|i| i as u8);
    let lut = look.table.unwrap_or(&identity);
    let (vw, vh) = (vw as i64, vh as i64);
    let (pw, ph) = (page.width as i64, page.height as i64);
    let (x0, x1) = (x.max(0), (x + pw).min(vw));
    if x0 >= x1 {
        return;
    }
    for sy in y.max(0)..(y + ph).min(vh) {
        let src = &page.rgba[(((sy - y) * pw + (x0 - x)) * 4) as usize..(((sy - y) * pw + (x1 - x)) * 4) as usize];
        let dst = &mut buffer[(sy * vw + x0) as usize..(sy * vw + x1) as usize];
        let plain = look.radius <= 0.0 && look.opacity >= 1.0;
        for (sx, (d, p)) in (x0..x1).zip(dst.iter_mut().zip(src.as_chunks::<4>().0)) {
            let a = p[3] as u32;
            let c = |v: u8| lut[((v as u32 * a + 127) / 255) as usize] as u32;
            let color = (c(p[0]) << 16) | (c(p[1]) << 8) | c(p[2]);
            *d = if plain {
                color
            } else {
                let cover =
                    look.opacity * inside_corners((sx - x) as f32 + 0.5, (sy - y) as f32 + 0.5, pw, ph, look.radius);
                mix(*d, color, cover)
            };
        }
    }
}

/// Quanto del pixel (al suo centro, nella pagina) cade dentro gli angoli
/// arrotondati: come `inside_corners` di present.wgsl.
fn inside_corners(px: f32, py: f32, pw: i64, ph: i64, radius: f32) -> f32 {
    let half = (pw as f32 / 2.0, ph as f32 / 2.0);
    let r = radius.min(half.0.min(half.1));
    if r <= 0.0 {
        return 1.0;
    }
    let q = ((px - half.0).abs() - half.0 + r, (py - half.1).abs() - half.1 + r);
    let d = q.0.max(0.0).hypot(q.1.max(0.0)) + q.0.max(q.1).min(0.0) - r;
    (0.5 - d).clamp(0.0, 1.0)
}

/// `over` sopra `under`, per quanto: 0x00RRGGBB.
fn mix(under: u32, over: u32, amount: f32) -> u32 {
    let a = amount.clamp(0.0, 1.0);
    let channel = |shift: u32| {
        let (u, o) = (((under >> shift) & 0xff) as f32, ((over >> shift) & 0xff) as f32);
        ((o * a + u * (1.0 - a)).round() as u32) << shift
    };
    channel(16) | channel(8) | channel(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copia_e_taglia_ai_bordi() {
        let page = Page {
            width: 2,
            height: 2,
            rgba: vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 10, 20, 30, 0],
            opaque: false,
        };
        let mut buffer = vec![7u32; 9];
        let plain = Look { table: None, radius: 0.0, opacity: 1.0 };
        blit(&mut buffer, 3, 3, &page, 2, -1, &plain); // sporge a destra e in alto
        assert_eq!(buffer, [7, 7, 0x0000ff, 7, 7, 7, 7, 7, 7]);
        let mut buffer = vec![7u32; 9];
        blit(&mut buffer, 3, 3, &page, 0, 0, &plain);
        assert_eq!(&buffer[..5], &[0xff0000, 0x00ff00, 7, 0x0000ff, 0]); // alfa 0 = nero
    }

    #[test]
    fn copertine_con_gli_angoli_tondi_e_la_dissolvenza() {
        let white = Page { width: 20, height: 20, rgba: vec![255; 20 * 20 * 4], opaque: true };
        let mut buffer = vec![0u32; 400];
        blit(&mut buffer, 20, 20, &white, 0, 0, &Look { table: None, radius: 8.0, opacity: 1.0 });
        assert_eq!(buffer[0], 0, "l'angolo resta fuori");
        assert_eq!(buffer[10 * 20 + 10], 0xffffff, "il centro c'e' tutto");
        let mut buffer = vec![0u32; 400];
        blit(&mut buffer, 20, 20, &white, 0, 0, &Look { table: None, radius: 0.0, opacity: 0.5 });
        assert_eq!(buffer[10 * 20 + 10], 0x808080, "a meta' della dissolvenza");
    }
}
