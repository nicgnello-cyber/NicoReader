//! L'ingrandimento AI sulla scheda video: Real-ESRGAN (realesr-animevideov3,
//! BSD-3, di Xintao Wang) scritto in WGSL, senza programmi esterni.
//!
//! La rete e' piccola: 18 convoluzioni 3x3 a 64 canali, ciascuna con la sua
//! PReLU tranne l'ultima, che da' 48 canali; il pixel shuffle li fa diventare
//! 4x4 pixel per ogni pixel d'entrata, a cui si somma l'entrata stessa
//! ingrandita. I pesi (1,2 MB, in mezza precisione) sono quelli del modello
//! ncnn ufficiale, dentro l'eseguibile: modelli/realesr-animevideov3.bin,
//! SHA-256 548a36f9c3f4ab8da56cd3b13badf23968bee207b396dad14d04b830e5f2ab2d.
//! Il "2x" e' la stessa rete, con la media di ogni 2x2 dell'uscita 4x.
//!
//! La pagina si lavora a tasselli, ciascuno con 18 pixel di bordo in piu'
//! (quanto la rete "vede" intorno a un pixel): il risultato e' lo stesso di
//! tutta la pagina in una volta, senza giunture. Uno strato alla volta, ogni
//! invio alla scheda video dura poco: sulla stessa scheda il disegno delle
//! pagine non si ferma. Il dispositivo e' suo, e preferisce la scheda piu'
//! potente (su un portatile con due, la dedicata).

use fumetto_core::Page;
use fumetto_core::lingua::t;

const WEIGHTS: &[u8] = include_bytes!("../modelli/realesr-animevideov3.bin");

/// Il lato di un tassello, in pixel d'entrata.
pub const TILE: u32 = 192;
/// Il bordo calcolato in piu' intorno al tassello: 18 strati 3x3.
const HALO: u32 = 18;
/// Le convoluzioni: 17 con PReLU, poi l'ultima.
const LAYERS: usize = 18;

/// Quanti pixel in fila e quanti gruppi d'uscita fa ogni invocazione (vedi
/// esrgan_conv.wgsl). Si puo' cambiare per le misure con FUMETTO_ESRGAN=px,per.
const PX: u32 = 4;
const PER: u32 = 4;

/// Un programma di convoluzione, con come va lanciato.
struct Conv {
    pipe: wgpu::ComputePipeline,
    px: u32,
    /// In quante parti si dividono i gruppi d'uscita (la z del lancio).
    split: u32,
}

/// Uno strato: dove cominciano i suoi pesi e i suoi vec4 (bias, poi pendenze).
#[derive(Clone, Copy, Debug)]
struct Layer {
    in_groups: u32,
    out_groups: u32,
    weights: u32,
    vecs: u32,
}

/// La rete pronta per la scheda video: i pesi come mat4x4 e i vec4.
struct Net {
    layers: Vec<Layer>,
    mats: Vec<f32>,
    vecs: Vec<f32>,
}

pub struct Esrgan {
    name: String,
    device: wgpu::Device,
    queue: wgpu::Queue,
    first: Conv,
    middle: Conv,
    last: Conv,
    shuffle: wgpu::ComputePipeline,
    _weights: wgpu::Buffer,
    _vecs: wgpu::Buffer,
    input: wgpu::Buffer,
    params: Vec<wgpu::Buffer>,
    out: wgpu::Buffer,
    staging: wgpu::Buffer,
    groups: Vec<wgpu::BindGroup>,
    tile: u32,
}

impl Esrgan {
    /// Prepara la rete sulla scheda piu' potente. `allow_cpu`: accetta anche
    /// una scheda software (per le prove: per leggere servirebbero minuti).
    pub async fn new(allow_cpu: bool) -> Result<Esrgan, String> {
        Esrgan::with_tile(allow_cpu, TILE).await
    }

    async fn with_tile(allow_cpu: bool, tile: u32) -> Result<Esrgan, String> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        if cfg!(windows) {
            desc.backends = wgpu::Backends::DX12;
        }
        let instance = wgpu::Instance::new(desc.with_env());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                ..Default::default()
            })
            .await
            .map_err(|e| format!("{}: {e}", t("Nessuna scheda video adatta", "No suitable graphics card")))?;
        let info = adapter.get_info();
        if info.device_type == wgpu::DeviceType::Cpu && !allow_cpu {
            let what = t(
                "è una scheda video software: sarebbe troppo lenta",
                "is a software graphics card: it would be too slow",
            );
            return Err(format!("{} {what}", info.name));
        }
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("ingrandimento"),
                required_limits: wgpu::Limits::downlevel_defaults(),
                ..Default::default()
            })
            .await
            .map_err(|e| format!("{}: {e}", t("Scheda video non utilizzabile", "Graphics card not usable")))?;
        let net = parse(WEIGHTS)?;
        Ok(Esrgan::build(format!("{} ({:?})", info.name, info.backend), device, queue, &net, tile))
    }

    fn build(name: String, device: wgpu::Device, queue: wgpu::Queue, net: &Net, tile: u32) -> Esrgan {
        use wgpu::util::DeviceExt;
        let storage = |label, contents: &[u8]| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents,
                usage: wgpu::BufferUsages::STORAGE,
            })
        };
        let weights = storage("pesi", bytemuck::cast_slice(&net.mats));
        let vecs = storage("bias e pendenze", bytemuck::cast_slice(&net.vecs));
        let side = (tile + 2 * HALO) as u64;
        let buffer = |label, size: u64, usage| {
            device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage, mapped_at_creation: false })
        };
        let rw = wgpu::BufferUsages::STORAGE;
        let input = buffer("entrata", side * side * 16, rw | wgpu::BufferUsages::COPY_DST);
        let feats = [buffer("mappa a", side * side * 16 * 16, rw), buffer("mappa b", side * side * 16 * 16, rw)];
        let out_size = (tile as u64 * 4).pow(2) * 4;
        let out = buffer("uscita", out_size, rw | wgpu::BufferUsages::COPY_SRC);
        let staging = buffer("lettura", out_size, wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST);
        let params: Vec<wgpu::Buffer> = (0..=LAYERS)
            .map(|_| buffer("parametri", 32, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST))
            .collect();

        let (px, per) = std::env::var("FUMETTO_ESRGAN")
            .ok()
            .and_then(|v| v.split_once(',').and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?))))
            .unwrap_or((PX, PER));
        let conv = |in_groups: u32, out_groups: u32, prelu: bool| {
            // il piu' grande divisore dei gruppi d'uscita che non supera `per`
            let per = (1..=per.min(out_groups)).rev().find(|d| out_groups.is_multiple_of(*d)).unwrap_or(1);
            let source = include_str!("shaders/esrgan_conv.wgsl")
                .replace("{IN}", &in_groups.to_string())
                .replace("{OUT}", &out_groups.to_string())
                .replace("{PER}", &per.to_string())
                .replace("{PX}", &px.to_string())
                .replace("{PRELU}", if prelu { "true" } else { "false" });
            Conv { pipe: pipeline(&device, "convoluzione", &source), px, split: out_groups / per }
        };
        let first = conv(net.layers[0].in_groups, net.layers[0].out_groups, true);
        let middle = conv(net.layers[1].in_groups, net.layers[1].out_groups, true);
        let last = conv(net.layers[LAYERS - 1].in_groups, net.layers[LAYERS - 1].out_groups, false);
        let shuffle = pipeline(&device, "pixel shuffle", include_str!("shaders/esrgan_shuffle.wgsl"));

        let mut groups = Vec::new();
        for k in 0..LAYERS {
            let pipe = if k == 0 {
                &first.pipe
            } else if k == LAYERS - 1 {
                &last.pipe
            } else {
                &middle.pipe
            };
            // le mappe si alternano: lo strato k scrive in feats[k % 2]
            let src = if k == 0 { &input } else { &feats[(k - 1) % 2] };
            groups.push(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("strato"),
                layout: &pipe.get_bind_group_layout(0),
                entries: &entries(&[&params[k], src, &feats[k % 2], &weights, &vecs]),
            }));
        }
        groups.push(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pixel shuffle"),
            layout: &shuffle.get_bind_group_layout(0),
            entries: &entries(&[&params[LAYERS], &feats[(LAYERS - 1) % 2], &input, &out]),
        }));
        // i parametri dei pesi non cambiano: solo la misura della regione
        for (k, l) in net.layers.iter().enumerate() {
            queue.write_buffer(&params[k], 8, bytemuck::cast_slice(&[l.weights, l.vecs]));
        }
        Esrgan {
            name,
            device,
            queue,
            first,
            middle,
            last,
            shuffle,
            _weights: weights,
            _vecs: vecs,
            input,
            params,
            out,
            staging,
            groups,
            tile,
        }
    }

    /// La scheda video su cui lavora.
    pub fn describe(&self) -> &str {
        &self.name
    }

    /// La pagina ingrandita `scale` volte (2 o 4). `None` se `go_on` dice di
    /// smettere (la si controlla fra uno strato e l'altro) o la scheda video
    /// non risponde.
    pub fn upscale(&self, page: &Page, scale: u32, go_on: &dyn Fn() -> bool) -> Option<Page> {
        assert!(scale == 2 || scale == 4, "solo 2x o 4x");
        let (w, h) = (page.width, page.height);
        let mut rgba = vec![0u8; (w * scale) as usize * (h * scale) as usize * 4];
        for ty in (0..h).step_by(self.tile as usize) {
            for tx in (0..w).step_by(self.tile as usize) {
                let (tw, th) = (self.tile.min(w - tx), self.tile.min(h - ty));
                let tile = self.tile_of(page, (tx, ty, tw, th), scale, go_on)?;
                // le righe del tassello al loro posto
                let row = (tw * scale * 4) as usize;
                let stride = (w * scale * 4) as usize;
                for r in 0..(th * scale) as usize {
                    let at = ((ty * scale) as usize + r) * stride + (tx * scale * 4) as usize;
                    rgba[at..at + row].copy_from_slice(&tile[r * row..(r + 1) * row]);
                }
            }
        }
        Some(Page { width: w * scale, height: h * scale, rgba, opaque: page.opaque })
    }

    /// Un tassello: la regione con il suo bordo (dentro la pagina), la rete
    /// strato per strato, il pixel shuffle sul solo tassello; i suoi pixel
    /// RGBA, riga per riga.
    fn tile_of(
        &self, page: &Page, (tx, ty, tw, th): (u32, u32, u32, u32), scale: u32, go_on: &dyn Fn() -> bool,
    ) -> Option<Vec<u8>> {
        let (x0, y0) = (tx.saturating_sub(HALO), ty.saturating_sub(HALO));
        let (x1, y1) = ((tx + tw + HALO).min(page.width), (ty + th + HALO).min(page.height));
        let (rw, rh) = (x1 - x0, y1 - y0);
        let mut input = Vec::with_capacity((rw * rh * 4) as usize);
        for y in y0..y1 {
            let at = ((y * page.width + x0) * 4) as usize;
            input.extend(page.rgba[at..at + (rw * 4) as usize].iter().map(|&b| b as f32 / 255.0));
        }
        self.queue.write_buffer(&self.input, 0, bytemuck::cast_slice(&input));
        for k in 0..LAYERS {
            self.queue.write_buffer(&self.params[k], 0, bytemuck::cast_slice(&[rw, rh]));
        }
        let shuffle = [rw, rh, tx - x0, ty - y0, tw, th, scale, 0];
        self.queue.write_buffer(&self.params[LAYERS], 0, bytemuck::cast_slice(&shuffle));

        let groups = |n: u32| n.div_ceil(8);
        for k in 0..LAYERS {
            if !go_on() {
                return None;
            }
            let conv = match k {
                0 => &self.first,
                _ if k == LAYERS - 1 => &self.last,
                _ => &self.middle,
            };
            let mut enc = self.device.create_command_encoder(&Default::default());
            {
                let mut pass = enc.begin_compute_pass(&Default::default());
                pass.set_pipeline(&conv.pipe);
                pass.set_bind_group(0, &self.groups[k], &[]);
                pass.dispatch_workgroups(groups(rw.div_ceil(conv.px)), groups(rh), conv.split);
            }
            // uno strato per invio, e si aspetta che finisca: fra uno e
            // l'altro la scheda disegna le pagine
            self.queue.submit([enc.finish()]);
            self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        }
        let bytes = (tw * scale * th * scale * 4) as u64;
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.shuffle);
            pass.set_bind_group(0, &self.groups[LAYERS], &[]);
            pass.dispatch_workgroups(groups(tw * scale), groups(th * scale), 1);
        }
        enc.copy_buffer_to_buffer(&self.out, 0, &self.staging, 0, bytes);
        self.queue.submit([enc.finish()]);
        let slice = self.staging.slice(..bytes);
        let (sent, got) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = sent.send(r);
        });
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        got.recv().ok()?.ok()?;
        let pixels = slice.get_mapped_range().ok()?.to_vec();
        self.staging.unmap();
        Some(pixels)
    }
}

/// I buffer di un gruppo, nell'ordine dei loro binding.
fn entries<'a>(list: &[&'a wgpu::Buffer]) -> Vec<wgpu::BindGroupEntry<'a>> {
    list.iter()
        .enumerate()
        .map(|(i, b)| wgpu::BindGroupEntry { binding: i as u32, resource: b.as_entire_binding() })
        .collect()
}

fn pipeline(device: &wgpu::Device, label: &str, source: &str) -> wgpu::ComputePipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    })
}

/// I pesi dal file ncnn: per ogni convoluzione un'etichetta (0x01306B47,
/// mezza precisione), i pesi [uscita][entrata][3][3] e i bias in f32; per
/// ogni PReLU le 64 pendenze in f32.
fn parse(bin: &[u8]) -> Result<Net, String> {
    let mut at = 0;
    let mut take = |n: usize| -> Result<&[u8], String> {
        let s = bin.get(at..at + n).ok_or("pesi troncati")?;
        at += n;
        Ok(s)
    };
    let f32s = |b: &[u8]| -> Vec<f32> { b.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect() };
    let mut net = Net { layers: Vec::new(), mats: Vec::new(), vecs: Vec::new() };
    let mut cin = 3usize;
    for k in 0..LAYERS {
        let cout = if k == LAYERS - 1 { 48 } else { 64 };
        let tag = u32::from_le_bytes(take(4)?.try_into().unwrap_or_default());
        if tag != 0x0130_6B47 {
            return Err(format!("pesi in un formato inatteso ({tag:#x})"));
        }
        let w: Vec<f32> =
            take(cout * cin * 9 * 2)?.as_chunks::<2>().0.iter().map(|c| f16_to_f32(u16::from_le_bytes(*c))).collect();
        let bias = f32s(take(cout * 4)?);
        let slope = if k < LAYERS - 1 { Some(f32s(take(cout * 4)?)) } else { None };
        let (in_groups, out_groups) = (cin.div_ceil(4), cout / 4);
        let layer = Layer {
            in_groups: in_groups as u32,
            out_groups: out_groups as u32,
            weights: (net.mats.len() / 16) as u32,
            vecs: (net.vecs.len() / 4) as u32,
        };
        for og in 0..out_groups {
            for ig in 0..in_groups {
                for tap in 0..9 {
                    // per colonne: la colonna c moltiplica il canale d'entrata 4*ig + c
                    for c in 0..4 {
                        for r in 0..4 {
                            let (o, i) = (og * 4 + r, ig * 4 + c);
                            net.mats.push(if i < cin { w[(o * cin + i) * 9 + tap] } else { 0.0 });
                        }
                    }
                }
            }
        }
        net.vecs.extend(&bias);
        if let Some(s) = slope {
            net.vecs.extend(&s);
        }
        net.layers.push(layer);
        cin = cout;
    }
    if at != bin.len() {
        return Err("pesi: avanzano dei byte".into());
    }
    Ok(net)
}

fn f16_to_f32(h: u16) -> f32 {
    let sign = (h as u32 & 0x8000) << 16;
    let exp = (h >> 10) & 0x1f;
    let frac = (h & 0x3ff) as u32;
    let bits = match exp {
        0 if frac == 0 => sign,
        0 => {
            // subnormale: si normalizza
            let shift = frac.leading_zeros() - 21;
            sign | ((113 - shift) << 23) | ((frac << shift) & 0x3ff) << 13
        }
        31 => sign | 0x7f80_0000 | (frac << 13),
        _ => sign | ((exp as u32 + 112) << 23) | (frac << 13),
    };
    f32::from_bits(bits)
}

#[cfg(test)]
mod tests;
