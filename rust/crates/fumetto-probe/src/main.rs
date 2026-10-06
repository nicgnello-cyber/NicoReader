//! Misure e prove di Fumetto, senza finestra.
//!
//!   fumetto-probe retino [cartella]      qualita' del rimpicciolimento, in PNG e in numeri
//!   fumetto-probe ridimensiona           quanto costa rimpicciolire una pagina sul processore
//!   fumetto-probe disegna                quanto costa un fotogramma sulla scheda video
//!   fumetto-probe scansione <cartella>   decodifica ogni pagina di ogni volume (come prova_scan.py della versione Python)
//!   fumetto-probe scatto <volume> [cartella] [larghezza altezza scala]
//!                                        l'interfaccia vera sulle pagine vere, in PNG
//!   fumetto-probe libreria <cartella> [uscita]
//!                                        la libreria vera di una cartella, in PNG
//!   fumetto-probe migliora <volume> [pagina] [larghezza altezza]
//!                                        l'ingranditore AI su una pagina: prima e dopo, in PNG
//!   fumetto-probe webtoon <volume>       le misure lette dalle intestazioni, e se e' un webtoon
//!
//! Opzione --integrata: usa la scheda video a basso consumo invece della dedicata.

mod chart;
mod scan;
mod scatto;

use std::path::{Path, PathBuf};
use std::time::Instant;

use fumetto_core::{Page, resize};
use fumetto_render::{Gpu, GpuImage, Placement, Renderer, wgpu};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let power = if args.iter().any(|a| a == "--integrata") {
        wgpu::PowerPreference::LowPower
    } else {
        wgpu::PowerPreference::HighPerformance
    };
    let rest: Vec<&str> = args.iter().map(String::as_str).filter(|a| !a.starts_with("--")).collect();
    let result = match rest.as_slice() {
        ["retino", out @ ..] => retino(out.first().map_or("retino".into(), PathBuf::from), power),
        ["ridimensiona"] => ridimensiona(),
        ["disegna"] => disegna(power),
        ["avvio"] => avvio(power),
        ["scansione", dir] => scan::run(&PathBuf::from(dir)),
        ["scatto", volume, rest @ ..] => {
            let num = |i: usize, d: f32| rest.get(i).and_then(|v| v.parse().ok()).unwrap_or(d);
            let out = rest.first().map_or("scatti".into(), PathBuf::from);
            scatto::run(Path::new(volume), &out, (num(1, 1920.0) as u32, num(2, 1080.0) as u32), num(3, 1.0), power)
        }
        ["libreria", root, rest @ ..] => {
            let out = rest.first().map_or("scatti".into(), PathBuf::from);
            scatto::libreria(Path::new(root), &out, (1920, 1080), 1.0, power)
        }
        ["prepara", volume, n @ ..] => prepara(Path::new(volume), n.first().and_then(|n| n.parse().ok()).unwrap_or(8)),
        ["migliora", volume, rest @ ..] => {
            let num = |i: usize, d: u32| rest.get(i).and_then(|v| v.parse().ok()).unwrap_or(d);
            migliora(Path::new(volume), num(0, 1) as usize - 1, (num(1, 1920), num(2, 1080)))
        }
        ["webtoon", volume] => webtoon(Path::new(volume)),
        _ => Err("uso: fumetto-probe retino [cartella] | ridimensiona | disegna | scansione <cartella> \
                  [--integrata]"
            .into()),
    };
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

/// L'ingranditore vero (Real-ESRGAN sulla scheda video) su una pagina vera, mostrata
/// a pagina intera in una finestra `view`: la pagina normale e quella
/// migliorata, alla misura dello schermo, in `migliora-prima.png` e `-dopo.png`.
fn migliora(volume: &Path, page: usize, view: (u32, u32)) -> Result<(), String> {
    use fumetto_core::upscale::{self, Job, Upscaler};
    use fumetto_core::{Fit, Target, decode_page, to_screen};
    let book = std::sync::Arc::new(fumetto_core::Book::open(volume).map_err(|e| e.to_string())?);
    let target = Target::plain(Fit::Contain { width: view.0, height: view.1 });
    let before = decode_page(&book, page, target)?;
    println!(
        "pagina {}: {} x {}, mostrata a {:?}; vale la pena: {}",
        page + 1,
        before.native.0,
        before.native.1,
        target.fit.size(before.native.0, before.native.1),
        upscale::worth(before.native, target)
    );
    let normal = to_screen(before, target);
    save(Path::new("migliora-prima.png"), &normal.rgba, normal.width, normal.height)?;
    let (tx, rx) = std::sync::mpsc::channel();
    // anche su una scheda software: qui si prova, la lentezza si sopporta
    let net = pollster::block_on(fumetto_render::esrgan::Esrgan::new(true))?;
    println!("ingrandimento su {}", net.describe());
    let engine: upscale::Engine = Box::new(move |page, scale, go_on| Ok(net.upscale(page, scale, go_on)));
    let up = Upscaler::new(engine, move |u| {
        let _ = tx.send(u);
    });
    let t = Instant::now();
    up.request(vec![Job { book, generation: 1, index: page, target }]);
    let got = rx.recv_timeout(std::time::Duration::from_secs(200)).map_err(|_| "nessuna risposta".to_string())?;
    let page = got.page.ok_or("l'ingranditore non ce l'ha fatta")?;
    println!("migliorata in {:.1} s: {} x {}", t.elapsed().as_secs_f32(), page.width, page.height);
    // di nuovo, dalla memoria: cambiando zoom non si rifa'
    let t = Instant::now();
    up.request(vec![Job {
        book: std::sync::Arc::new(fumetto_core::Book::open(volume).map_err(|e| e.to_string())?),
        generation: 1,
        index: got.index,
        target: Target::plain(Fit::Width(view.0)),
    }]);
    let again = rx.recv_timeout(std::time::Duration::from_secs(200)).map_err(|_| "nessuna risposta".to_string())?;
    println!(
        "a un'altra misura, dalla memoria, in {:.0} ms: {:?}",
        t.elapsed().as_secs_f32() * 1000.0,
        again.page.map(|p| (p.width, p.height))
    );
    save(Path::new("migliora-dopo.png"), &page.rgba, page.width, page.height)
}

/// Le misure che l'app legge all'apertura per riconoscere i webtoon.
fn webtoon(volume: &Path) -> Result<(), String> {
    let book = fumetto_core::Book::open(volume).map_err(|e| e.to_string())?;
    let t = Instant::now();
    let sizes = book.sample_sizes(7);
    println!("{} pagine misurate in {:.1} ms: {sizes:?}", sizes.len(), t.elapsed().as_secs_f64() * 1000.0);
    let dims: Vec<(u32, u32)> = sizes.iter().map(|s| s.1).collect();
    match fumetto::reader::webtoon_width(&dims) {
        Some(w) => println!("webtoon, strisce larghe {w}"),
        None => println!("non e' un webtoon"),
    }
    Ok(())
}

fn gpu(power: wgpu::PowerPreference) -> Result<(Gpu, Renderer), String> {
    // come l'app: su Windows DX12 (WGPU_BACKEND puo' scegliere altro). Con
    // Vulkan il driver Intel va ogni tanto in errore di memoria mentre il
    // processo si chiude, dentro igxelpicd64.dll (visto con gdb)
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    if cfg!(windows) {
        desc.backends = wgpu::Backends::DX12;
    }
    let instance = wgpu::Instance::new(desc.with_env());
    let gpu = pollster::block_on(Gpu::new(&instance, None, power))?;
    println!("scheda video: {}", gpu.describe());
    let renderer = Renderer::new(&gpu);
    Ok((gpu, renderer))
}

fn at(image: &GpuImage, x: f32, y: f32, w: f32, h: f32) -> Placement<'_> {
    Placement { image, x, y, w, h }
}

fn exact(image: &GpuImage) -> Placement<'_> {
    at(image, 0.0, 0.0, image.width as f32, image.height as f32)
}

/// Come rimpiccioliscono quasi tutti i lettori: bilineare a nucleo fisso,
/// senza togliere prima le frequenze che lo schermo non puo' mostrare.
fn naive(page: &Page, w: u32, h: u32) -> Vec<u8> {
    use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer, images};
    let src = images::ImageRef::new(page.width, page.height, &page.rgba, PixelType::U8x4).unwrap();
    let mut dst = images::Image::new(w, h, PixelType::U8x4);
    let opt = ResizeOptions::new().resize_alg(ResizeAlg::Interpolation(FilterType::Bilinear)).use_alpha(false);
    Resizer::new().resize(&src, &mut dst, &opt).unwrap();
    dst.into_vec()
}

fn retino(out: PathBuf, power: wgpu::PowerPreference) -> Result<(), String> {
    const K: u32 = 3;
    let (w, h) = (2400, 3600);
    let (ow, oh) = (w / K, h / K);
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let (_gpu, r) = gpu(power)?;

    let page = chart::tavola(w, h);
    let reference = chart::reference(&page, K);
    let shots = [
        ("lineare", resize(&page, ow, oh, true).rgba),
        ("gamma", resize(&page, ow, oh, false).rgba),
        ("ingenuo", naive(&page, ow, oh)),
    ];

    // la copia esatta sulla scheda video non deve cambiare nemmeno un pixel
    let small = Page { width: ow, height: oh, rgba: shots[0].1.clone(), opaque: true };
    let gpu_image = r.upload(&small)?;
    let drawn = r.render_to_rgba((ow, oh), &[exact(&gpu_image)], |_, _| {});
    let differ = drawn.iter().zip(&small.rgba).filter(|(a, b)| a != b).count();
    println!(
        "copia esatta sulla scheda video: {}",
        if differ == 0 { "identica, pixel per pixel".to_string() } else { format!("{differ} VALORI DIVERSI") }
    );

    for (name, rgba) in &shots {
        save(&out.join(format!("retino-{name}.png")), rgba, ow, oh)?;
    }
    save(&out.join("retino-riferimento.png"), &reference, ow, oh)?;

    println!("\nScarto dal riferimento (luce reale media), in livelli sRGB su 255: piu' basso = piu' fedele");
    print!("{:<12}", "");
    for b in chart::BANDS {
        print!("{b:>20}");
    }
    println!();
    for (name, rgba) in &shots {
        print!("{name:<12}");
        for v in chart::rms_by_band(rgba, &reference, ow, oh) {
            print!("{v:>20.1}");
        }
        println!();
    }

    // da vicino: un pezzo di ogni fascia, riferimento e poi i tre modi
    let mut all: Vec<&[u8]> = vec![&reference];
    all.extend(shots.iter().map(|(_, s)| s.as_slice()));
    for (n, band) in chart::BANDS.iter().enumerate() {
        let crop = (ow / 2 - 100, n as u32 * oh / 4 + 20, 200, 180);
        let file = out.join(format!("vicino-{}-{}.png", n + 1, band.replace(' ', "-")));
        chart::montage(&all, ow, crop, 3).save(&file).map_err(|e| e.to_string())?;
    }

    // ingrandimento bicubico sulla scheda video: un pezzo di tavola a 2,5 volte
    let piece = crop(&page, 1000, 2750, 240, 160);
    let piece_gpu = r.upload(&piece)?;
    let (zw, zh) = (600, 400);
    let zoomed = r.render_to_rgba((zw, zh), &[at(&piece_gpu, 0.0, 0.0, zw as f32, zh as f32)], |_, _| {});
    save(&out.join("ingrandito-bicubico.png"), &zoomed, zw, zh)?;

    println!("\nImmagini in {} (i ritagli: riferimento, lineare, gamma, ingenuo)", out.display());
    Ok(())
}

fn crop(page: &Page, x: u32, y: u32, w: u32, h: u32) -> Page {
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for row in y..y + h {
        let start = ((row * page.width + x) * 4) as usize;
        rgba.extend_from_slice(&page.rgba[start..start + (w * 4) as usize]);
    }
    Page { width: w, height: h, rgba, opaque: page.opaque }
}

fn save(path: &Path, rgba: &[u8], w: u32, h: u32) -> Result<(), String> {
    image::save_buffer(path, rgba, w, h, image::ColorType::Rgba8).map_err(|e| e.to_string())
}

/// Le prime `n` pagine vere di un volume, passo per passo, su un solo thread,
/// portate alla misura di uno schermo 1920x1080 come fa l'app.
fn prepara(volume: &Path, n: usize) -> Result<(), String> {
    let book = fumetto_core::Book::open(volume).map_err(|e| e.to_string())?;
    let fit = fumetto_core::Fit::Contain { width: 1920, height: 1080 };
    println!(
        "{:>5} {:>11} {:>8} {:>6} {:>9} {:>9} {:>11}",
        "pag.", "misura", "KB", "grigio", "lettura", "decodif.", "rimpicciol."
    );
    for i in 0..n.min(book.len()) {
        let t = Instant::now();
        let content = book.content(i, fit).map_err(|e| e.to_string())?;
        let read = t.elapsed().as_secs_f64() * 1000.0;
        let t = Instant::now();
        let (page, kb) = match content {
            fumetto_core::Content::Encoded(bytes) => {
                (fumetto_core::decode(&bytes).map_err(|e| e.to_string())?, bytes.len() / 1024)
            }
            fumetto_core::Content::Pixels(p) | fumetto_core::Content::Exact(p, _) => (p, 0),
        };
        let decode = t.elapsed().as_secs_f64() * 1000.0;
        // quanto e' lontana dal bianco e nero: la differenza massima fra i canali
        let chroma = page
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| p[0].abs_diff(p[1]).max(p[1].abs_diff(p[2])))
            .max()
            .unwrap_or(0);
        let (w, h) = fit.size(page.width, page.height);
        let t = Instant::now();
        let _ = resize(&page, w, h, true);
        let small = t.elapsed().as_secs_f64() * 1000.0;
        println!(
            "{:>5} {:>11} {:>8} {:>6} {read:>6.1} ms {decode:>6.1} ms {small:>8.1} ms",
            i + 1,
            format!("{}x{}", page.width, page.height),
            kb,
            if chroma == 0 { "si'".to_string() } else { format!("~{chroma}") }
        );
    }
    Ok(())
}

/// Rimpicciolire una pagina sul processore, su un solo thread.
fn ridimensiona() -> Result<(), String> {
    for (what, w, h, ow, oh) in [
        ("tavola 3840x5400 -> 768x1080", 3840, 5400, 768, 1080),
        ("tavola 1600x2400 -> 720x1080", 1600, 2400, 720, 1080),
        ("striscia 800x20000 -> 540x13500", 800, 20_000, 540, 13_500),
    ] {
        let page = chart::tavola(w, h);
        for linear in [true, false] {
            let _ = resize(&page, ow, oh, linear);
            let n = 5;
            let t = Instant::now();
            for _ in 0..n {
                let _ = resize(&page, ow, oh, linear);
            }
            let each = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
            println!("{what:<36} {:<8} {each:7.1} ms", if linear { "lineare" } else { "gamma" });
        }
    }
    Ok(())
}

/// Dove va il tempo per preparare la scheda video, tappa per tappa.
fn avvio(power: wgpu::PowerPreference) -> Result<(), String> {
    let t0 = Instant::now();
    let lap = |what: &str, t: &mut Instant| {
        println!("{what:<40} {:7.1} ms", t.elapsed().as_secs_f64() * 1000.0);
        *t = Instant::now();
    };
    let mut t = Instant::now();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    lap("istanza", &mut t);
    let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()));
    lap(&format!("elenco schede ({})", adapters.len()), &mut t);
    for a in &adapters {
        let i = a.get_info();
        println!("    {} ({:?})", i.name, i.backend);
    }
    let gpu = pollster::block_on(Gpu::new(&instance, None, power))?;
    lap(&format!("scheda e dispositivo: {}", gpu.describe()), &mut t);
    let r = Renderer::new(&gpu);
    lap("programmi della scheda (moduli)", &mut t);
    let img = r.upload(&Page { width: 4, height: 4, rgba: vec![255; 64], opaque: true })?;
    let _ = r.render_to_rgba((16, 16), &[exact(&img)], |_, _| {});
    lap("primo disegno (compila la pipeline)", &mut t);
    println!("{:<40} {:7.1} ms", "totale", t0.elapsed().as_secs_f64() * 1000.0);
    Ok(())
}

/// Il costo di un fotogramma a schermo intero 1920x1080, nei due casi tipici.
fn disegna(power: wgpu::PowerPreference) -> Result<(), String> {
    let (g, r) = gpu(power)?;
    let format = wgpu::TextureFormat::Bgra8Unorm;
    let target = g.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: 1920, height: 1080, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let frame = |items: &[Placement]| {
        let mut encoder = g.device.create_command_encoder(&Default::default());
        r.draw(&mut encoder, &view, format, (1920, 1080), items, fumetto_render::Pass::PLAIN);
        g.queue.submit([encoder.finish()]);
    };
    let page = r.upload(&resize(&chart::tavola(2400, 3600), 720, 1080, true))?;
    let strip = r.upload(&chart::tavola(800, 12_000))?;
    let scenes: [(&str, Vec<Placement>); 2] = [
        ("pagina gia' alla misura (copia)", vec![at(&page, 600.0, 0.0, 720.0, 1080.0)]),
        ("nastro webtoon ingrandito 1,44x (bicubico)", vec![at(&strip, 384.0, -5000.3, 1152.0, 17_280.0)]),
    ];
    for (what, items) in &scenes {
        frame(items);
        r.wait();
        let n = 500;
        let t = Instant::now();
        for _ in 0..n {
            frame(items);
        }
        r.wait();
        let each = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
        println!("{what:<44} {each:6.3} ms a fotogramma");
    }
    Ok(())
}
