//! L'ingrandimento AI: le scansioni a bassa risoluzione, mostrate piu' grandi
//! dei loro pixel, rifatte con Real-ESRGAN invece che solo interpolate.
//!
//! Real-ESRGAN (BSD-3, di Xintao Wang) e' un programma esterno, in C++ con
//! ncnn e Vulkan: lavora sulla scheda video, e su un portatile con due schede
//! puo' usare la dedicata mentre la integrata disegna le pagine. Non sta nel
//! pacchetto: chi lo vuole lo scarica al primo uso (44 MB da GitHub, di cui se
//! ne tengono 9), chi non lo vuole non paga niente.
//!
//! Il modello e' realesr-animevideov3, non x4plus-anime: misurato su una tavola
//! degradata contro l'originale, 25,1 dB in 1,4 s contro 17,1 dB in 4,5 s; il
//! modello "grande" ingrandisce 4x quando ne serve 2x e altera lo spessore dei
//! tratti.
//!
//! Il lettore mostra subito la pagina normale; quella migliorata arriva dopo,
//! dall'unico thread di questo modulo, e prende il suo posto senza avvisi.

use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::book::Book;
use crate::decode::{Page, decode};
use crate::lingua::t;
use crate::loader::{Decoded, Target, decode_page, to_screen};

/// Il pacchetto di Real-ESRGAN per questo sistema.
#[cfg(windows)]
const URL: &str =
    "https://github.com/xinntao/Real-ESRGAN/releases/download/v0.2.5.0/realesrgan-ncnn-vulkan-20220424-windows.zip";
#[cfg(target_os = "macos")]
const URL: &str =
    "https://github.com/xinntao/Real-ESRGAN/releases/download/v0.2.5.0/realesrgan-ncnn-vulkan-20220424-macos.zip";
#[cfg(all(unix, not(target_os = "macos")))]
const URL: &str =
    "https://github.com/xinntao/Real-ESRGAN/releases/download/v0.2.5.0/realesrgan-ncnn-vulkan-20220424-ubuntu.zip";

/// Quanto si scarica, in MB: e' cio' che si promette a chi dice di si'.
pub const DOWNLOAD_MB: u32 = 44;

#[cfg(windows)]
const EXE: &str = "realesrgan-ncnn-vulkan.exe";
#[cfg(not(windows))]
const EXE: &str = "realesrgan-ncnn-vulkan";

/// Del pacchetto si tiene solo questo (con l'impronta SHA-256, dove la si
/// conosce: i file per Windows, verificati sul pacchetto ufficiale). Un file
/// diverso da quello atteso non si tiene: e' un programma che si esegue.
const KEEP: &[(&str, Option<&str>)] = &[
    (EXE, if cfg!(windows) { Some("07e49f7cbb4ede01ae4dd4c399d3a7e5846e3d2085c3128eff881e55cb7b1a0c") } else { None }),
    ("vcomp140.dll", Some("8f72ef2e483465444b2059fc6744d6cb22cd8d8a27f6fa56befd2a42dcd0f78b")),
    ("realesr-animevideov3-x2.bin", Some("548a36f9c3f4ab8da56cd3b13badf23968bee207b396dad14d04b830e5f2ab2d")),
    ("realesr-animevideov3-x2.param", Some("b88ff4f00ebf019a7fdac17fdd45a7fd3665d37509efc5baf2e4da2e24420a04")),
    ("realesr-animevideov3-x4.bin", Some("548a36f9c3f4ab8da56cd3b13badf23968bee207b396dad14d04b830e5f2ab2d")),
    ("realesr-animevideov3-x4.param", Some("850a248e7c14c27e5bd8cf7265113a9441036a7db63963bb8aa5169d788a435e")),
];

/// Si migliora solo una pagina mostrata almeno 1,2 volte i suoi pixel: sotto,
/// l'interpolazione della scheda video non si distingue.
pub const MIN_GAIN: f32 = 1.2;

/// Oltre questo tempo il programma si ferma: una pagina che non arriva in tre
/// minuti non arrivera' piu'.
const TIMEOUT: Duration = Duration::from_secs(180);

/// Quante pagine migliorate (ai loro pixel, prima della misura dello schermo)
/// si tengono: cambiando zoom si riusano invece di rifarle. Una tavola 2x pesa
/// 25-40 MB.
const KEEP_DONE: usize = 3;

/// L'eseguibile, se e' gia' stato scaricato in `dir`.
pub fn exe(dir: &Path) -> Option<PathBuf> {
    let exe = dir.join(EXE);
    exe.is_file().then_some(exe)
}

/// Vale la pena di migliorare una pagina `native` mostrata come vuole `target`?
pub fn worth(native: (u32, u32), target: Target) -> bool {
    let (w, _) = target.fit.size(native.0, native.1);
    native.0 > 0 && w as f32 >= native.0 as f32 * MIN_GAIN
}

/// Scarica il pacchetto in `dir` e ne tiene solo il necessario. Ci vuole
/// tempo: va chiamata da un thread a parte. Il download lo fa curl, che c'e'
/// in Windows 10 e 11, in macOS e in ogni Linux: niente librerie TLS
/// nell'eseguibile per un file che si scarica una volta.
pub fn install(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir.join("models")).map_err(|e| e.to_string())?;
    let zip_path = dir.join("pacchetto.zip");
    let result = download(&zip_path).and_then(|_| unpack(&zip_path, dir));
    let _ = std::fs::remove_file(&zip_path);
    if result.is_err() {
        let _ = std::fs::remove_dir_all(dir);
    }
    result
}

fn download(to: &Path) -> Result<(), String> {
    let mut curl = Command::new("curl");
    curl.args(["--location", "--fail", "--silent", "--show-error", "--max-time", "600", "--output"])
        .arg(to)
        .arg(URL)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    hide_window(&mut curl);
    let out = curl.output().map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }
    Ok(())
}

fn unpack(zip_path: &Path, dir: &Path) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().rsplit('/').next().unwrap_or_default().to_owned();
        let Some((_, hash)) = KEEP.iter().find(|(n, _)| *n == name) else { continue };
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        if let Some(hash) = hash {
            let got: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
            if got != *hash {
                return Err(format!("{name}: {}", t("non è il file atteso", "not the expected file")));
            }
        }
        let to = if name.ends_with(".bin") || name.ends_with(".param") { dir.join("models").join(&name) } else { dir.join(&name) };
        std::fs::write(&to, bytes).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        if name == EXE {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&to, std::fs::Permissions::from_mode(0o755));
        }
    }
    exe(dir).map(|_| ()).ok_or_else(|| {
        t("Il pacchetto scaricato non contiene il programma atteso.", "The downloaded package lacks the expected program.")
            .to_owned()
    })
}

fn hide_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

/// Una pagina migliorata, alla misura dello schermo; `None` se non si e'
/// potuto (una pagina PDF disegnata, il programma che non parte): non la si
/// richiede piu'.
pub struct Upscaled {
    pub generation: u64,
    pub index: usize,
    pub target: Target,
    pub page: Option<Page>,
}

/// Una pagina da migliorare.
#[derive(Clone)]
pub struct Job {
    pub book: Arc<Book>,
    pub generation: u64,
    pub index: usize,
    pub target: Target,
}

pub struct Upscaler {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

struct State {
    dir: PathBuf,
    queue: VecDeque<Job>,
    /// Il programma al lavoro: lo si ferma se l'app si chiude.
    child: Option<Child>,
    stop: bool,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Upscaler {
    /// `dir`: dove sta (o stara') il programma; `deliver` riceve le pagine
    /// migliorate, dal thread del modulo.
    pub fn new(dir: PathBuf, deliver: impl Fn(Upscaled) + Send + Sync + 'static) -> Upscaler {
        let shared = Arc::new(Shared {
            state: Mutex::new(State { dir, queue: VecDeque::new(), child: None, stop: false }),
            wake: Condvar::new(),
        });
        let worker = {
            let shared = shared.clone();
            std::thread::Builder::new()
                .name("ingrandimento".into())
                .spawn(move || {
                    let _ = thread_priority::set_current_thread_priority(thread_priority::ThreadPriority::Min);
                    work(&shared, &deliver)
                })
                .expect("thread dell'ingrandimento")
        };
        Upscaler { shared, worker: Some(worker) }
    }

    pub fn dir(&self) -> PathBuf {
        self.shared.lock().dir.clone()
    }

    pub fn installed(&self) -> bool {
        exe(&self.dir()).is_some()
    }

    /// Sostituisce la coda: le pagine a schermo, dalla piu' urgente. Quella in
    /// lavorazione finisce comunque.
    pub fn request(&self, jobs: Vec<Job>) {
        self.shared.lock().queue = jobs.into();
        self.shared.wake.notify_all();
    }
}

impl Drop for Upscaler {
    fn drop(&mut self) {
        let mut s = self.shared.lock();
        s.stop = true;
        // niente programmi orfani che occupano la scheda video
        if let Some(child) = &mut s.child {
            let _ = child.kill();
        }
        drop(s);
        self.shared.wake.notify_all();
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

/// La chiave di una pagina migliorata: stesso volume, pagina e ritocchi.
type Key = (u64, usize, bool, u16);

fn work(shared: &Shared, deliver: &dyn Fn(Upscaled)) {
    let temp = std::env::temp_dir().join(format!("fumetto-ingrandimento-{}", std::process::id()));
    let mut done: VecDeque<(Key, (u32, u32), Arc<Page>)> = VecDeque::new();
    loop {
        let (job, dir) = {
            let mut s = shared.lock();
            loop {
                if s.stop {
                    let _ = std::fs::remove_dir_all(&temp);
                    return;
                }
                if let Some(job) = s.queue.pop_front() {
                    break (job, s.dir.clone());
                }
                s = shared.wake.wait(s).unwrap_or_else(|e| e.into_inner());
            }
        };
        let key = (job.generation, job.index, job.target.trim, job.target.rotation);
        let big = match done.iter().find(|d| d.0 == key) {
            Some((_, native, page)) => Some((*native, page.clone())),
            None => upscale(shared, &job, &dir, &temp).map(|(native, page)| {
                let page = Arc::new(page);
                done.push_back((key, native, page.clone()));
                if done.len() > KEEP_DONE {
                    done.pop_front();
                }
                (native, page)
            }),
        };
        let page = big.map(|(native, big)| {
            let copy = Page { width: big.width, height: big.height, rgba: big.rgba.clone(), opaque: big.opaque };
            to_screen(Decoded { native, page: copy, exact: false }, job.target)
        });
        deliver(Upscaled { generation: job.generation, index: job.index, target: job.target, page });
    }
}

/// La pagina rifatta da Real-ESRGAN, ai suoi pixel (2x, o 4x per le
/// scansioni minuscole); con la misura di riferimento della pagina originale.
fn upscale(shared: &Shared, job: &Job, dir: &Path, temp: &Path) -> Option<((u32, u32), Page)> {
    let exe = exe(dir)?;
    let d = decode_page(&job.book, job.index, job.target).ok()?;
    if d.exact {
        return None; // pagina PDF disegnata: ha gia' tutti i pixel che servono
    }
    std::fs::create_dir_all(temp).ok()?;
    let (input, output) = (temp.join("pagina.png"), temp.join("migliorata.png"));
    let _ = std::fs::remove_file(&output);
    let (w, h) = (d.page.width, d.page.height);
    // senza alfa il PNG pesa un quarto di meno, e si scrive prima
    let saved = if d.page.opaque {
        let rgb: Vec<u8> = d.page.rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
        image::save_buffer(&input, &rgb, w, h, image::ColorType::Rgb8)
    } else {
        image::save_buffer(&input, &d.page.rgba, w, h, image::ColorType::Rgba8)
    };
    saved.ok()?;
    let scale = if w.max(h) < 700 { "4" } else { "2" };
    let mut cmd = Command::new(&exe);
    cmd.arg("-i").arg(&input).arg("-o").arg(&output).args(["-n", "realesr-animevideov3", "-s", scale])
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hide_window(&mut cmd);
    {
        let mut s = shared.lock();
        if s.stop {
            return None;
        }
        s.child = Some(cmd.spawn().ok()?);
    }
    let started = Instant::now();
    let ok = loop {
        std::thread::sleep(Duration::from_millis(40));
        let mut s = shared.lock();
        let go_on = !s.stop && started.elapsed() < TIMEOUT;
        let Some(child) = &mut s.child else { break false };
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if go_on => {}
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    shared.lock().child = None;
    if !ok {
        return None;
    }
    let page = decode(&std::fs::read(&output).ok()?).ok()?;
    Some((d.native, page))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::Fit;

    #[test]
    fn si_migliora_solo_cio_che_si_ingrandisce() {
        let page = Target::plain(Fit::Contain { width: 1920, height: 1080 });
        assert!(worth((500, 700), page), "700 di altezza mostrati a 1080: 1,54x");
        assert!(!worth((1600, 2400), page), "si rimpicciolisce");
        assert!(!worth((800, 1000), page), "1,08x: non si vede la differenza");
        assert!(worth((720, 12_000), Target::plain(Fit::Width(1080))), "striscia da 720 larga 1080");
    }
}
