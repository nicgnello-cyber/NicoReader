//! Tutte le pagine di tutti i volumi di una cartella: si aprono, si decodificano
//! e si contano i problemi. Stessa ricerca dei volumi di prova_scan.py (versione Python), per
//! poter confrontare i numeri: ogni archivio e ogni cartella che contiene
//! direttamente delle immagini.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use fumetto_core::{Book, Content, Fit, Loader, decode, is_image, natural_cmp};

const ARCHIVES: &[&str] = &["cbz", "zip", "cbr", "rar", "cb7", "7z", "cbt", "tar", "pdf"];

pub fn run(root: &Path) -> Result<(), String> {
    let volumes = find_volumes(root);
    if volumes.is_empty() {
        return Err(format!("Nessun fumetto in {}", root.display()));
    }
    let threads = Loader::default_threads();
    // i PDF vettoriali si disegnano gia' alla misura: quella di uno schermo tipico
    let fit = Fit::Contain { width: 1920, height: 1080 };
    println!("{} volumi, {threads} thread di decodifica\n", volumes.len());
    println!("{:<44} {:>6} {:>9} {:>9} {:>6}  esito", "volume", "pag.", "apre", "pagina", "lato");

    let t0 = Instant::now();
    let (mut pages, mut broken_volumes) = (0, 0);
    let mut all_ms = Vec::new();
    for path in &volumes {
        let name: String = path.file_name().unwrap_or_default().to_string_lossy().chars().take(44).collect();
        let t = Instant::now();
        let book = match Book::open(path) {
            Ok(b) => b,
            Err(e) => {
                println!("{name:<44} NON SI APRE: {e}");
                broken_volumes += 1;
                continue;
            }
        };
        let open_ms = t.elapsed().as_secs_f64() * 1000.0;

        let next = AtomicUsize::new(0);
        let results = Mutex::new(Vec::with_capacity(book.len()));
        std::thread::scope(|s| {
            for _ in 0..threads {
                s.spawn(|| {
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= book.len() {
                            break;
                        }
                        let t = Instant::now();
                        let r = book.content(i, fit).map_err(|e| e.to_string()).and_then(|c| match c {
                            Content::Encoded(b) => decode(&b).map_err(|e| e.to_string()),
                            Content::Pixels(p) | Content::Exact(p, _) => Ok(p),
                        });
                        let ms = t.elapsed().as_secs_f64() * 1000.0;
                        let r = r.map(|p| p.width.max(p.height));
                        results.lock().unwrap().push((i, ms, r));
                    }
                });
            }
        });
        let results = results.into_inner().unwrap();
        let mut ms: Vec<f64> = results.iter().filter(|r| r.2.is_ok()).map(|r| r.1).collect();
        ms.sort_by(f64::total_cmp);
        let broken: Vec<_> = results.iter().filter_map(|r| r.2.as_ref().err().map(|e| (r.0, e))).collect();
        let side = results.iter().filter_map(|r| r.2.as_ref().ok()).max().copied().unwrap_or(0);
        let median = ms.get(ms.len() / 2).copied().unwrap_or(0.0);
        println!(
            "{name:<44} {:>6} {:>6.0} ms {:>6.1} ms {side:>6}  {}",
            book.len(),
            open_ms,
            median,
            if broken.is_empty() { "ok".to_string() } else { format!("{} PAGINE ROTTE", broken.len()) }
        );
        for (i, e) in broken.iter().take(3) {
            println!("{:46}! {}: {e}", "", book.names[*i]);
        }
        broken_volumes += !broken.is_empty() as usize;
        pages += book.len();
        all_ms.extend(ms);
    }
    let secs = t0.elapsed().as_secs_f64();
    all_ms.sort_by(f64::total_cmp);
    let pct = |p: f64| {
        all_ms.get(((all_ms.len() as f64 * p) as usize).min(all_ms.len().saturating_sub(1))).copied().unwrap_or(0.0)
    };
    println!(
        "\n{} volumi, {pages} pagine in {secs:.0} s ({:.0} pagine al secondo). Volumi con problemi: {broken_volumes}.",
        volumes.len(),
        pages as f64 / secs
    );
    println!(
        "Lettura + decodifica di una pagina, su un thread: mediana {:.1} ms, 90% sotto {:.1} ms, peggiore {:.1} ms",
        pct(0.5),
        pct(0.9),
        all_ms.last().copied().unwrap_or(0.0)
    );
    Ok(())
}

fn find_volumes(root: &Path) -> Vec<PathBuf> {
    let mut found = BTreeSet::new();
    let mut stack = vec![root.to_owned()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = path.to_string_lossy();
            let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if ARCHIVES.contains(&ext.as_str()) {
                found.insert(path.clone());
            } else if is_image(&name) {
                found.insert(dir.clone());
            }
        }
    }
    let mut v: Vec<PathBuf> = found.into_iter().collect();
    v.sort_by(|a, b| natural_cmp(&a.to_string_lossy(), &b.to_string_lossy()));
    v
}
