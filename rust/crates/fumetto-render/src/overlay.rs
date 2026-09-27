//! Le scritte e le forme sopra la tavola: didascalie, avvisi, "vai a pagina",
//! il menu. Le disegna la scheda video nello stesso fotogramma delle pagine,
//! subito dopo: niente finestre in piu', niente copie.
//!
//! Chi disegna l'interfaccia descrive una [`Scene`] (rettangoli e testi, a
//! strati: ogni strato copre i precedenti) ogni volta che la vuole a schermo;
//! qui la si impagina e la si disegna.
//!
//! I caratteri sono dentro l'eseguibile, cosi' l'aspetto e' lo stesso su ogni
//! sistema: Instrument Serif per numeri e titoli, Instrument Sans (variabile)
//! per le etichette. Licenza SIL OFL 1.1: i testi sono
//! accanto ai file, in `caratteri/`. Nessun carattere di sistema viene
//! caricato: costerebbe decine di millisecondi all'avvio e cambierebbe da un
//! computer all'altro.

use std::sync::Arc;

use glyphon::cosmic_text::{Align as TextAlign, FeatureTag, FontFeatures};
use glyphon::{
    Attrs, Buffer, Cache, Color, ColorMode, Family, FontSystem, Metrics, Resolution, Shaping, Style, SwashCache,
    TextArea, TextAtlas, TextBounds, TextRenderer, Viewport, Weight, fontdb,
};

use crate::{Gpu, wgpu};

const SERIF: &[u8] = include_bytes!("../caratteri/InstrumentSerif-Regular.ttf");
const SANS: &[u8] = include_bytes!("../caratteri/InstrumentSans.ttf");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    Serif,
    Sans,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Un testo su una riga, dentro una scatola che parte da (x, y).
#[derive(Clone, Debug)]
pub struct Text {
    pub text: String,
    pub face: Face,
    /// Corpo in pixel.
    pub size: f32,
    pub weight: u16,
    /// Spaziatura tra le lettere, in frazioni del corpo.
    pub tracking: f32,
    /// Cifre tutte larghe uguali (dove il carattere le ha).
    pub tabular: bool,
    /// sRGB, alfa non premoltiplicata.
    pub color: [u8; 4],
    pub x: f32,
    pub y: f32,
    /// Larghezza della scatola, per allineare a destra o al centro.
    pub width: Option<f32>,
    pub align: Align,
}

impl Text {
    pub fn new(text: impl Into<String>, face: Face, size: f32, color: [u8; 4]) -> Text {
        Text {
            text: text.into(),
            face,
            size,
            weight: 400,
            tracking: 0.0,
            tabular: false,
            color,
            x: 0.0,
            y: 0.0,
            width: None,
            align: Align::Left,
        }
    }

    pub fn at(mut self, x: f32, y: f32) -> Text {
        (self.x, self.y) = (x, y);
        self
    }

    /// Allineato dentro una scatola larga `width` che parte da x.
    pub fn boxed(mut self, width: f32, align: Align) -> Text {
        (self.width, self.align) = (Some(width), align);
        self
    }

    pub fn weight(mut self, weight: u16) -> Text {
        self.weight = weight;
        self
    }

    pub fn tracking(mut self, em: f32) -> Text {
        self.tracking = em;
        self
    }

    pub fn tabular(mut self) -> Text {
        self.tabular = true;
        self
    }

    /// Distanza fra la cima della scatola e la linea di base, come la calcola
    /// cosmic-text: la riga (1,2 corpi) centrata su ascendenti + discendenti.
    pub fn baseline(&self) -> f32 {
        // dalle tabelle hhea dei due caratteri, in millesimi del corpo
        let (ascent, descent) = match self.face {
            Face::Serif => (0.99, 0.31),
            Face::Sans => (0.97, 0.25),
        };
        let line = (self.size * 1.2).ceil();
        (line - (ascent + descent) * self.size) / 2.0 + ascent * self.size
    }

    /// Per mettere la linea di base a quota y.
    pub fn on_baseline(self, x: f32, y: f32) -> Text {
        let top = y - self.baseline();
        self.at(x, top)
    }
}

/// Un rettangolo, con angoli arrotondati, bordi sfumati e una sfumatura
/// verticale di colore; oppure solo il suo contorno; oppure una linea dalle
/// estremita' tonde (i tratti delle icone). I colori sono sRGB in 0..1, alfa
/// non premoltiplicata.
#[derive(Clone, Copy, Debug)]
pub struct Rect {
    /// Per una linea: x1, y1, x2, y2.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Per una linea: meta' dello spessore.
    pub radius: f32,
    pub blur: f32,
    /// Spessore del contorno; 0 = pieno.
    pub stroke: f32,
    pub line: bool,
    pub top: [f32; 4],
    pub bottom: [f32; 4],
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) -> Rect {
        Rect { x, y, w, h, radius: 0.0, blur: 0.0, stroke: 0.0, line: false, top: color, bottom: color }
    }

    /// Una linea da (x1, y1) a (x2, y2), spessa `width` pixel.
    pub fn line(x1: f32, y1: f32, x2: f32, y2: f32, width: f32, color: [f32; 4]) -> Rect {
        Rect { line: true, radius: width / 2.0, ..Rect::new(x1, y1, x2, y2, color) }
    }

    /// Solo il contorno, spesso `width` pixel.
    pub fn stroke(mut self, width: f32) -> Rect {
        self.stroke = width;
        self
    }

    pub fn radius(mut self, r: f32) -> Rect {
        self.radius = r;
        self
    }

    pub fn blur(mut self, b: f32) -> Rect {
        self.blur = b;
        self
    }

    /// Dal colore dato in alto a questo in basso.
    pub fn to(mut self, bottom: [f32; 4]) -> Rect {
        self.bottom = bottom;
        self
    }
}

/// Quanto spazio prende un testo.
pub trait Measure {
    /// Larghezza della riga, in pixel, senza andare a capo.
    fn width(&mut self, text: &Text) -> f32;
    /// Quante righe, andando a capo nella larghezza della sua scatola.
    fn lines(&mut self, text: &Text) -> usize;
}

/// Una stima senza i caratteri veri: per le prove, e per chi impagina prima
/// che la scheda video sia pronta.
pub struct Estimate;

impl Measure for Estimate {
    fn width(&mut self, t: &Text) -> f32 {
        let em = match t.face {
            Face::Sans => 0.52,
            Face::Serif => 0.44,
        };
        t.text.chars().count() as f32 * t.size * (em + t.tracking)
    }

    fn lines(&mut self, t: &Text) -> usize {
        t.width.map_or(1, |w| (self.width(t) / w.max(1.0)).ceil().max(1.0) as usize)
    }
}

/// Uno strato: prima i rettangoli, poi i testi.
#[derive(Default, Debug)]
pub struct Layer {
    pub rects: Vec<Rect>,
    pub texts: Vec<Text>,
}

#[derive(Default, Debug)]
pub struct Scene {
    pub layers: Vec<Layer>,
}

impl Scene {
    pub fn is_empty(&self) -> bool {
        self.layers.iter().all(|l| l.rects.is_empty() && l.texts.is_empty())
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Instance {
    rect: [f32; 4],
    top: [f32; 4],
    bottom: [f32; 4],
    style: [f32; 4],
}

pub struct Overlay {
    device: wgpu::Device,
    queue: wgpu::Queue,
    fonts: FontSystem,
    swash: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    /// Uno per strato: glyphon disegna tutto cio' che ha preparato in un colpo.
    texts: Vec<TextRenderer>,
    pipeline: wgpu::RenderPipeline,
    view: wgpu::Buffer,
    view_bind: wgpu::BindGroup,
    instances: wgpu::Buffer,
    capacity: usize,
}

impl Overlay {
    /// `format`: quello del bersaglio, *non* sRGB come per le pagine.
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Overlay {
        let device = gpu.device.clone();
        let mut db = fontdb::Database::new();
        for bytes in [SERIF, SANS] {
            db.load_font_source(fontdb::Source::Binary(Arc::new(bytes)));
        }
        let fonts = FontSystem::new_with_locale_and_db("it-IT".into(), db);
        let cache = Cache::new(&device);
        let viewport = Viewport::new(&device, &cache);
        // "Web": i colori restano valori sRGB e si mescolano cosi', come nei
        // browser e come le nostre pagine (il bersaglio non converte niente)
        let mut atlas = TextAtlas::with_color_mode(&device, &gpu.queue, &cache, format, ColorMode::Web);
        let texts = vec![TextRenderer::new(&mut atlas, &device, wgpu::MultisampleState::default(), None)];

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("interfaccia"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let view = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("interfaccia: misura"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let view_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: view.as_entire_binding() }],
        });
        let module = device.create_shader_module(wgpu::include_wgsl!("shaders/overlay.wgsl"));
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("interfaccia"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("interfaccia"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<Instance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4],
                })],
            },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let capacity = 64;
        let instances = Self::instance_buffer(&device, capacity);
        Overlay {
            device,
            queue: gpu.queue.clone(),
            fonts,
            swash: SwashCache::new(),
            viewport,
            atlas,
            texts,
            pipeline,
            view,
            view_bind,
            instances,
            capacity,
        }
    }

    fn instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("interfaccia: forme"),
            size: (capacity * size_of::<Instance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Impagina un testo; al massimo `max_lines` righe (`None`: tutte).
    fn shape(&mut self, t: &Text, max_lines: Option<usize>) -> Buffer {
        let line = (t.size * 1.2).ceil();
        let mut b = Buffer::new(&mut self.fonts, Metrics::new(t.size, line));
        b.set_size(t.width, max_lines.map(|n| line * n as f32));
        let family = match t.face {
            Face::Serif => "Instrument Serif",
            Face::Sans => "Instrument Sans",
        };
        let mut attrs = Attrs::new()
            .family(Family::Name(family))
            .style(Style::Normal)
            .weight(Weight(t.weight));
        if t.tracking != 0.0 {
            attrs = attrs.letter_spacing(t.tracking);
        }
        if t.tabular {
            let mut f = FontFeatures::new();
            f.enable(FeatureTag::new(b"tnum"));
            attrs = attrs.font_features(f);
        }
        let align = match t.align {
            Align::Left => TextAlign::Left,
            Align::Center => TextAlign::Center,
            Align::Right => TextAlign::Right,
        };
        b.set_text(&t.text, &attrs, Shaping::Advanced, Some(align));
        b.shape_until_scroll(&mut self.fonts, false);
        b
    }

    /// Disegna la scena sopra cio' che il bersaglio contiene gia'.
    // ponytail: ogni testo si impagina a ogni fotogramma (pochi microsecondi
    // l'uno, e sono una decina); una cache per testo se diventano centinaia
    pub fn draw(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, size: (u32, u32),
                scene: &Scene) -> Result<(), String> {
        if scene.is_empty() {
            return Ok(());
        }
        let (w, h) = size;
        self.viewport.update(&self.queue, Resolution { width: w, height: h });
        self.queue.write_buffer(&self.view, 0, bytemuck::cast_slice(&[w as f32, h as f32, 0.0, 0.0]));

        let mut instances = Vec::new();
        let mut ranges = Vec::new();
        for layer in &scene.layers {
            let start = instances.len() as u32;
            instances.extend(layer.rects.iter().map(|r| Instance {
                rect: [r.x, r.y, r.w, r.h],
                top: r.top,
                bottom: r.bottom,
                style: [r.radius, r.blur, r.stroke, r.line as u8 as f32],
            }));
            ranges.push(start..instances.len() as u32);
        }
        if instances.len() > self.capacity {
            self.capacity = instances.len().next_power_of_two();
            self.instances = Self::instance_buffer(&self.device, self.capacity);
        }
        if !instances.is_empty() {
            self.queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&instances));
        }

        while self.texts.len() < scene.layers.len() {
            self.texts.push(TextRenderer::new(&mut self.atlas, &self.device, wgpu::MultisampleState::default(), None));
        }
        for (i, layer) in scene.layers.iter().enumerate() {
            let buffers: Vec<Buffer> = layer.texts.iter().map(|t| self.shape(t, Some(2))).collect();
            let areas = buffers.iter().zip(&layer.texts).map(|(b, t)| TextArea {
                buffer: b,
                left: t.x.round(),
                top: t.y.round(),
                scale: 1.0,
                bounds: TextBounds::default(),
                default_color: Color::rgba(t.color[0], t.color[1], t.color[2], t.color[3]),
                custom_glyphs: &[],
            });
            self.texts[i]
                .prepare(&self.device, &self.queue, &mut self.fonts, &mut self.atlas, &self.viewport, areas,
                         &mut self.swash)
                .map_err(|e| format!("testo: {e}"))?;
        }

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("interfaccia"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            ..Default::default()
        });
        for (i, range) in ranges.into_iter().enumerate() {
            if !range.is_empty() {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.view_bind, &[]);
                pass.set_vertex_buffer(0, self.instances.slice(..));
                pass.draw(0..4, range);
            }
            self.texts[i].render(&self.atlas, &self.viewport, &mut pass).map_err(|e| format!("testo: {e}"))?;
        }
        drop(pass);
        self.atlas.trim();
        Ok(())
    }
}

impl Measure for Overlay {
    fn width(&mut self, t: &Text) -> f32 {
        let free = Text { width: None, ..t.clone() };
        self.shape(&free, Some(1)).layout_runs().map(|r| r.line_w).fold(0.0, f32::max)
    }

    fn lines(&mut self, t: &Text) -> usize {
        self.shape(t, None).layout_runs().count()
    }
}
