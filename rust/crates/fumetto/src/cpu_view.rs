//! Le prime pagine, prima che la scheda video sia pronta.
//!
//! Prepararla costa fino a un secondo e mezzo: su un portatile, per elencare le
//! schede, si sveglia anche quella dedicata. Le pagine invece escono dai thread
//! gia' alla misura dello schermo, quindi basta copiarle nella finestra dal
//! processore (GDI su Windows, CoreGraphics su macOS, X11/Wayland su Linux).
//! Quando la scheda video arriva prende il posto di questa copia, con gli
//! stessi pixel: il passaggio non si vede.
//!
//! Se la scheda video non arriva mai (driver rotto, macchina virtuale), si
//! resta qui: piu' lento, ma si legge lo stesso.

use std::num::NonZeroU32;
use std::sync::Arc;

use fumetto_core::{Page, enlarge, resize};
use winit::window::Window;

pub struct CpuView {
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
    // tenuto vivo quanto la superficie
    _context: softbuffer::Context<Arc<Window>>,
}

/// Una pagina da copiare e il rettangolo dove va, in pixel della finestra.
pub struct Item<'a> {
    pub page: &'a Page,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl CpuView {
    pub fn new(window: Arc<Window>) -> Result<CpuView, String> {
        let context = softbuffer::Context::new(window.clone()).map_err(|e| e.to_string())?;
        let surface = softbuffer::Surface::new(&context, window).map_err(|e| e.to_string())?;
        Ok(CpuView { surface, _context: context })
    }

    /// Disegna le pagine sul nero e consegna alla finestra. `table`:
    /// luminosita', contrasto e gamma, come valore per valore (la stessa
    /// formula che usera' la scheda video, cosi' il passaggio non si vede).
    pub fn draw(&mut self, size: (u32, u32), items: &[Item], table: Option<&[u8; 256]>) -> Result<(), String> {
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
            blit(&mut buffer, vw, vh, page, it.x.round() as i64, it.y.round() as i64, table);
        }
        buffer.present().map_err(|e| e.to_string())
    }
}

/// Copia `page` in (x, y), tagliando quello che esce dalla finestra.
/// softbuffer vuole 0x00RRGGBB; l'alfa si compone sul nero.
fn blit(buffer: &mut [u32], vw: u32, vh: u32, page: &Page, x: i64, y: i64, table: Option<&[u8; 256]>) {
    let identity: [u8; 256] = std::array::from_fn(|i| i as u8);
    let lut = table.unwrap_or(&identity);
    let (vw, vh) = (vw as i64, vh as i64);
    let (pw, ph) = (page.width as i64, page.height as i64);
    let (x0, x1) = (x.max(0), (x + pw).min(vw));
    if x0 >= x1 {
        return;
    }
    for sy in y.max(0)..(y + ph).min(vh) {
        let src = &page.rgba[(((sy - y) * pw + (x0 - x)) * 4) as usize..(((sy - y) * pw + (x1 - x)) * 4) as usize];
        let dst = &mut buffer[(sy * vw + x0) as usize..(sy * vw + x1) as usize];
        for (d, p) in dst.iter_mut().zip(src.as_chunks::<4>().0) {
            let a = p[3] as u32;
            let c = |v: u8| lut[((v as u32 * a + 127) / 255) as usize] as u32;
            *d = (c(p[0]) << 16) | (c(p[1]) << 8) | c(p[2]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copia_e_taglia_ai_bordi() {
        let page = Page { width: 2, height: 2, rgba: vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 10, 20, 30, 0], opaque: false };
        let mut buffer = vec![7u32; 9];
        blit(&mut buffer, 3, 3, &page, 2, -1, None); // sporge a destra e in alto
        assert_eq!(buffer, [7, 7, 0x0000ff, 7, 7, 7, 7, 7, 7]);
        let mut buffer = vec![7u32; 9];
        blit(&mut buffer, 3, 3, &page, 0, 0, None);
        assert_eq!(&buffer[..5], &[0xff0000, 0x00ff00, 7, 0x0000ff, 0]); // alfa 0 = nero
    }
}
