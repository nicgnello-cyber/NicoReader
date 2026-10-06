//! La tavola sulla scheda video.
//!
//! Le pagine arrivano gia' alla misura dello schermo (le ha rimpicciolite il
//! processore, in luce lineare) oppure alla misura originale quando vanno
//! ingrandite. [`Renderer::upload`] le carica, anche da un altro thread;
//! [`Renderer::draw`] le mette sul nero: una copia pixel per pixel se la
//! misura coincide, un'interpolazione bicubica se va ingrandita.

pub mod esrgan;
mod overlay;
mod tiling;

use std::collections::HashMap;
use std::sync::Mutex;

use fumetto_core::Page;

pub use overlay::{Align, Estimate, Face, Layer, Measure, Overlay, Rect, Scene, Text};
pub use tiling::{MARGIN, TILE_ROWS};
/// La versione di wgpu con cui e' fatto tutto: chi disegna usa questa.
pub use wgpu;

/// Valori sRGB a 8 bit, copiati cosi' come sono: nessuna conversione
/// automatica, ne' qui ne' sul bersaglio.
const PAGE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Il formato dei disegni fuori schermo di [`Renderer::render_to_rgba`].
pub const OFFSCREEN_FORMAT: wgpu::TextureFormat = PAGE_FORMAT;

/// Scheda video, dispositivo e coda.
pub struct Gpu {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    pub async fn new(
        instance: &wgpu::Instance, surface: Option<&wgpu::Surface<'_>>, power: wgpu::PowerPreference,
    ) -> Result<Gpu, String> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: power,
                compatible_surface: surface,
                ..Default::default()
            })
            .await
            .map_err(|e| format!("nessuna scheda video adatta: {e}"))?;
        // le doppie pagine scansionate grandi superano gli 8192 pixel garantiti:
        // si chiede quello che la scheda offre davvero
        let limits = wgpu::Limits {
            max_texture_dimension_2d: adapter.limits().max_texture_dimension_2d,
            ..wgpu::Limits::default()
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("fumetto"),
                required_limits: limits,
                ..Default::default()
            })
            .await
            .map_err(|e| format!("scheda video non utilizzabile: {e}"))?;
        Ok(Gpu { adapter, device, queue })
    }

    pub fn describe(&self) -> String {
        let i = self.adapter.get_info();
        format!("{} ({:?})", i.name, i.backend)
    }
}

/// Una pagina sulla scheda video, in tasselli orizzontali.
pub struct GpuImage {
    pub width: u32,
    pub height: u32,
    tiles: Vec<Tile>,
}

struct Tile {
    bind: wgpu::BindGroup,
    /// Righe dell'immagine contenute nella texture.
    stored: std::ops::Range<u32>,
    /// Righe che appartengono a questo tassello (le altre sono margine).
    core: std::ops::Range<u32>,
}

impl GpuImage {
    /// Memoria occupata sulla scheda video.
    pub fn bytes(&self) -> usize {
        self.tiles.iter().map(|t| self.width as usize * t.stored.len() * 4).sum()
    }
}

/// Luminosita', contrasto e gamma delle pagine. Si applicano ai valori sRGB
/// gia' alla misura dello schermo, nell'ordine: gamma, contrasto attorno al
/// grigio medio, luminosita'. Sono funzioni di un canale alla volta: la
/// copia dal processore usa la stessa formula in una tabella.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Adjust {
    /// Da aggiungere, -0,5..0,5.
    pub brightness: f32,
    /// Quanto allargare attorno al grigio medio, 0,5..2.
    pub contrast: f32,
    /// Gamma, 0,5..2 (sopra 1 schiarisce i mezzitoni).
    pub gamma: f32,
}

impl Adjust {
    pub const NONE: Adjust = Adjust { brightness: 0.0, contrast: 1.0, gamma: 1.0 };

    /// Dai passi delle impostazioni, -100..100 ciascuno (0: come sono).
    pub fn from_steps(brightness: i32, contrast: i32, gamma: i32) -> Adjust {
        let two = |v: i32| 2f32.powf(v.clamp(-100, 100) as f32 / 100.0);
        Adjust { brightness: brightness.clamp(-100, 100) as f32 / 200.0, contrast: two(contrast), gamma: two(gamma) }
    }

    pub fn is_none(&self) -> bool {
        *self == Adjust::NONE
    }

    /// Un valore 0..1 regolato.
    pub fn apply(&self, v: f32) -> f32 {
        let v = v.clamp(0.0, 1.0).powf(1.0 / self.gamma);
        ((v - 0.5) * self.contrast + 0.5 + self.brightness).clamp(0.0, 1.0)
    }

    /// La stessa regolazione per ogni valore a 8 bit.
    pub fn table(&self) -> [u8; 256] {
        std::array::from_fn(|i| (self.apply(i as f32 / 255.0) * 255.0).round() as u8)
    }
}

/// Come disegnare un gruppo di immagini.
#[derive(Clone, Copy, Debug)]
pub struct Pass {
    /// Prima si cancella tutto sul nero (il primo gruppo di un fotogramma).
    pub clear: bool,
    pub adjust: Adjust,
    /// Solo dentro questo cerchio: centro e raggio, in pixel (la lente).
    pub clip: Option<(f32, f32, f32)>,
}

impl Pass {
    pub const PLAIN: Pass = Pass { clear: true, adjust: Adjust::NONE, clip: None };
}

/// Dove disegnare un'immagine, in pixel del bersaglio.
pub struct Placement<'a> {
    pub image: &'a GpuImage,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct QuadUniform {
    rect: [f32; 4],
    src: [f32; 4],
    viewport: [f32; 2],
    mode: u32,
    _pad: u32,
    /// luminosita', contrasto, 1/gamma, 1 = da applicare
    adjust: [f32; 4],
    /// centro x, y e raggio del cerchio, 1 = da ritagliare
    clip: [f32; 4],
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    max_dim: u32,
    tile_layout: wgpu::BindGroupLayout,
    quad_layout: wgpu::BindGroupLayout,
    module: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    pipelines: Mutex<HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>>,
    quads: Mutex<QuadBuffer>,
    quad_stride: u32,
}

struct QuadBuffer {
    buffer: wgpu::Buffer,
    bind: wgpu::BindGroup,
    capacity: u32,
    /// Quanti rettangoli ha gia' scritto il fotogramma: un secondo gruppo
    /// (la lente) scrive dopo di loro. Tutte le scritture arrivano alla
    /// scheda video prima dei disegni, quindi nello stesso tratto la seconda
    /// cancellerebbe la prima.
    used: u32,
}

impl Renderer {
    pub fn new(gpu: &Gpu) -> Renderer {
        let device = gpu.device.clone();
        let tile_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tassello"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let quad_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rettangolo"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let module = device.create_shader_module(wgpu::include_wgsl!("shaders/present.wgsl"));
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("schermo"),
            bind_group_layouts: &[Some(&tile_layout), Some(&quad_layout)],
            immediate_size: 0,
        });
        let quad_stride =
            (size_of::<QuadUniform>() as u32).next_multiple_of(device.limits().min_uniform_buffer_offset_alignment);
        let quads = Mutex::new(QuadBuffer::new(&device, &quad_layout, quad_stride, 16));
        Renderer {
            max_dim: device.limits().max_texture_dimension_2d,
            device,
            queue: gpu.queue.clone(),
            tile_layout,
            quad_layout,
            module,
            pipeline_layout,
            pipelines: Mutex::default(),
            quads,
            quad_stride,
        }
    }

    /// Carica una pagina. Si puo' chiamare da un thread di lavoro: la copia
    /// parte con il prossimo invio alla coda, senza fermare chi disegna.
    pub fn upload(&self, page: &Page) -> Result<GpuImage, String> {
        if page.width > self.max_dim {
            return Err(format!("pagina larga {} pixel: la scheda video arriva a {}", page.width, self.max_dim));
        }
        let row_bytes = page.width as usize * 4;
        let tiles = tiling::tile_rows(page.height)
            .into_iter()
            .map(|t| {
                let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("tassello"),
                    size: wgpu::Extent3d { width: page.width, height: t.stored.len() as u32, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: PAGE_FORMAT,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                let rows = &page.rgba[t.stored.start as usize * row_bytes..t.stored.end as usize * row_bytes];
                self.queue.write_texture(
                    texture.as_image_copy(),
                    rows,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row_bytes as u32),
                        rows_per_image: None,
                    },
                    texture.size(),
                );
                let view = texture.create_view(&Default::default());
                let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &self.tile_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    }],
                });
                Tile { bind, stored: t.stored, core: t.core }
            })
            .collect();
        Ok(GpuImage { width: page.width, height: page.height, tiles })
    }

    /// Compila in anticipo il disegno per un formato di schermo: su DX12 la
    /// prima compilazione costa quasi 100 ms, meglio pagarli mentre si apre la finestra.
    pub fn prepare(&self, format: wgpu::TextureFormat) {
        let _ = self.pipeline(format);
    }

    fn pipeline(&self, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
        let mut cache = self.pipelines.lock().unwrap_or_else(|e| e.into_inner());
        cache
            .entry(format)
            .or_insert_with(|| {
                self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("schermo"),
                    layout: Some(&self.pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &self.module,
                        entry_point: Some("vs"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleStrip,
                        ..Default::default()
                    },
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &self.module,
                        entry_point: Some("fs"),
                        compilation_options: Default::default(),
                        targets: &[Some(format.into())],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            })
            .clone()
    }

    /// Disegna le immagini (sul nero, se `pass.clear`). Il bersaglio deve
    /// essere in un formato *non* sRGB: i valori delle pagine sono gia'
    /// codificati e vanno copiati.
    pub fn draw(
        &self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, format: wgpu::TextureFormat,
        size: (u32, u32), items: &[Placement], pass: Pass,
    ) {
        let (tw, th) = (size.0 as f32, size.1 as f32);
        let a = pass.adjust;
        let adjust = [a.brightness, a.contrast, 1.0 / a.gamma, (!a.is_none()) as u32 as f32];
        let clip = pass.clip.map_or([0.0; 4], |(cx, cy, r)| [cx, cy, r, 1.0]);
        // il riquadro che puo' contenere qualcosa: lo schermo, o attorno al cerchio
        let (bx0, by0, bx1, by1) = match pass.clip {
            Some((cx, cy, r)) => ((cx - r).max(0.0), (cy - r).max(0.0), (cx + r).min(tw), (cy + r).min(th)),
            None => (0.0, 0.0, tw, th),
        };
        let mut quads: Vec<(QuadUniform, &wgpu::BindGroup)> = Vec::new();
        for it in items {
            // misura esatta: posizione sui pixel interi e copia texel per texel
            let exact = (it.w - it.image.width as f32).abs() < 0.5 && (it.h - it.image.height as f32).abs() < 0.5;
            let (x, y) = if exact { (it.x.round(), it.y.round()) } else { (it.x, it.y) };
            let k = it.h / it.image.height as f32;
            for t in &it.image.tiles {
                let y0 = y + t.core.start as f32 * k;
                let y1 = y + t.core.end as f32 * k;
                let mut rect = [x, y0, it.w, y1 - y0];
                let mut src = [0.0, (t.core.start - t.stored.start) as f32, it.image.width as f32, t.core.len() as f32];
                // solo il pezzo che cade nel riquadro utile: per la lente, una
                // pagina ingrandita cinque volte si disegna solo nel suo cerchio
                let (cx0, cy0) = (rect[0].max(bx0), rect[1].max(by0));
                let (cx1, cy1) = ((rect[0] + rect[2]).min(bx1), (rect[1] + rect[3]).min(by1));
                if cx1 <= cx0 || cy1 <= cy0 {
                    continue; // fuori schermo: nel nastro sono quasi tutti
                }
                if pass.clip.is_some() {
                    let (sx, sy) = (src[2] / rect[2], src[3] / rect[3]);
                    src = [
                        src[0] + (cx0 - rect[0]) * sx,
                        src[1] + (cy0 - rect[1]) * sy,
                        (cx1 - cx0) * sx,
                        (cy1 - cy0) * sy,
                    ];
                    rect = [cx0, cy0, cx1 - cx0, cy1 - cy0];
                }
                quads.push((
                    QuadUniform { rect, src, viewport: [tw, th], mode: (!exact) as u32, _pad: 0, adjust, clip },
                    &t.bind,
                ));
            }
        }

        let pipeline = self.pipeline(format);
        let mut qb = self.quads.lock().unwrap_or_else(|e| e.into_inner());
        let first = if pass.clear { 0 } else { qb.used };
        let need = first + quads.len() as u32;
        if need > qb.capacity {
            // il gruppo di prima resta nel buffer vecchio, che vive finche' serve
            *qb = QuadBuffer::new(&self.device, &self.quad_layout, self.quad_stride, need.next_power_of_two());
        }
        qb.used = need;
        let stride = self.quad_stride as usize;
        let mut bytes = vec![0u8; quads.len() * stride];
        for (i, (q, _)) in quads.iter().enumerate() {
            bytes[i * stride..][..size_of::<QuadUniform>()].copy_from_slice(bytemuck::bytes_of(q));
        }
        if !bytes.is_empty() {
            self.queue.write_buffer(&qb.buffer, first as u64 * stride as u64, &bytes);
        }

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("schermo"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: if pass.clear { wgpu::LoadOp::Clear(wgpu::Color::BLACK) } else { wgpu::LoadOp::Load },
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline);
        for (i, (_, tile)) in quads.iter().enumerate() {
            pass.set_bind_group(0, *tile, &[]);
            pass.set_bind_group(1, &qb.bind, &[((first as usize + i) * stride) as u32]);
            pass.draw(0..4, 0..1);
        }
    }

    /// Disegna fuori schermo e restituisce i pixel RGBA: per le prove e per
    /// guardare davvero il risultato, senza finestra. `then` disegna altro
    /// sopra le pagine (l'interfaccia), in [`OFFSCREEN_FORMAT`].
    pub fn render_to_rgba(
        &self, size: (u32, u32), items: &[Placement], then: impl FnOnce(&mut wgpu::CommandEncoder, &wgpu::TextureView),
    ) -> Vec<u8> {
        self.render_to_rgba_with(size, items, Pass::PLAIN, then)
    }

    /// Come `render_to_rgba`, con le regolazioni di `pass`.
    pub fn render_to_rgba_with(
        &self, size: (u32, u32), items: &[Placement], pass: Pass,
        then: impl FnOnce(&mut wgpu::CommandEncoder, &wgpu::TextureView),
    ) -> Vec<u8> {
        let (w, h) = size;
        let target = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fuori schermo"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: PAGE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let view = target.create_view(&Default::default());
        self.draw(&mut encoder, &view, PAGE_FORMAT, size, items, Pass { clear: true, ..pass });
        then(&mut encoder, &view);
        let stride = (w * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (stride * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(stride), rows_per_image: None },
            },
            target.size(),
        );
        self.queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        self.wait();
        let data = buffer.slice(..).get_mapped_range().expect("buffer appena mappato");
        data.chunks(stride as usize).flat_map(|row| &row[..(w * 4) as usize]).copied().collect()
    }

    /// Aspetta che la scheda video abbia finito tutto il lavoro accodato.
    pub fn wait(&self) {
        let _ = self.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
    }
}

impl QuadBuffer {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, stride: u32, capacity: u32) -> QuadBuffer {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rettangoli"),
            size: (stride * capacity) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(size_of::<QuadUniform>() as u64),
                }),
            }],
        });
        QuadBuffer { buffer, bind, capacity, used: 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regolazioni() {
        let none = Adjust::from_steps(0, 0, 0);
        assert!(none.is_none());
        assert!(none.table().iter().enumerate().all(|(i, &v)| v as usize == i), "a zero non cambia niente");
        let brighter = Adjust::from_steps(40, 0, 0).table();
        assert!(brighter[128] > 128 && brighter[255] == 255);
        // il contrasto tiene fermo il grigio medio e allontana il resto
        let c = Adjust::from_steps(0, 100, 0);
        assert!((c.apply(0.5) - 0.5).abs() < 1e-6 && c.apply(0.25) < 0.25 && c.apply(0.75) > 0.75);
        let g = Adjust::from_steps(0, 0, 100).table();
        assert!(g[64] > 64 && g[0] == 0 && g[255] == 255, "la gamma schiarisce i mezzitoni, non gli estremi");
    }
}
