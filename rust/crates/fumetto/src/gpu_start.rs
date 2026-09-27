//! La scheda video si prepara in un thread tutto suo, dalla prima riga del
//! programma: costa fino a un secondo e mezzo (su un portatile si sveglia anche
//! la scheda dedicata, solo per sapere che c'e'). Nel frattempo la finestra si
//! apre, la prima pagina si decodifica e si mostra copiandola dal processore;
//! quando la scheda video e' pronta arriva un evento e prende il suo posto.

use fumetto_render::{Gpu, Overlay, Renderer, wgpu};
use winit::event_loop::EventLoopProxy;

use crate::app::UserEvent;

pub struct GpuStart {
    pub instance: wgpu::Instance,
    pub gpu: Gpu,
    pub renderer: Renderer,
    pub overlay: Overlay,
    /// Quando e' finita ogni tappa, in ms dall'avvio del thread.
    pub steps: Vec<(&'static str, f32)>,
}

/// Il formato di schermo che quasi sempre si ottiene: il disegno per questo
/// formato si compila in anticipo.
pub const LIKELY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

pub fn spawn(proxy: EventLoopProxy<UserEvent>) {
    std::thread::Builder::new()
        .name("scheda-video".into())
        .spawn(move || {
            let _ = proxy.send_event(UserEvent::Gpu(Box::new(start())));
        })
        .expect("thread della scheda video");
}

fn start() -> Result<GpuStart, String> {
    let t0 = std::time::Instant::now();
    let mut steps = Vec::new();
    let mut lap = |what| steps.push((what, t0.elapsed().as_secs_f32() * 1000.0));
    // la scheda integrata pilota lo schermo: niente copie tra due schede,
    // meno consumo, e disegnare una pagina le costa 0,3 ms
    let power = match std::env::var("FUMETTO_GPU").as_deref() {
        Ok("dedicata") => wgpu::PowerPreference::HighPerformance,
        _ => wgpu::PowerPreference::LowPower,
    };
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    // su Windows solo DX12: presenta con il modello "flip" di DXGI, e chiedere
    // anche Vulkan costava altri 550 ms di avvio per nulla. WGPU_BACKEND puo'
    // sempre scegliere altro.
    if cfg!(windows) {
        desc.backends = wgpu::Backends::DX12;
    }
    let instance = wgpu::Instance::new(desc.with_env());
    lap("istanza");
    let gpu = pollster::block_on(Gpu::new(&instance, None, power))?;
    lap("scheda e dispositivo");
    let renderer = Renderer::new(&gpu);
    lap("programmi di disegno");
    renderer.prepare(LIKELY_FORMAT);
    lap("disegno compilato");
    let overlay = Overlay::new(&gpu, LIKELY_FORMAT);
    lap("caratteri e interfaccia");
    Ok(GpuStart { instance, gpu, renderer, overlay, steps })
}
