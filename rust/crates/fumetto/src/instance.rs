//! Una sola finestra: aperto un volume da Esplora file, con "Apri con" o
//! dalla jump list mentre NicoReader e' gia' aperto, il volume va alla
//! finestra che c'e' (che si porta davanti) invece di aprire un secondo
//! NicoReader. Due copie si contenderebbero anche i file dei progressi e
//! delle impostazioni: vince chi chiude per ultimo, e l'altra perde cio' che
//! ha scritto.
//!
//! La prima copia ascolta su un canale locale; quelle dopo le mandano il
//! volume e si chiudono. Su Windows una named pipe (il primo che la crea la
//! tiene: niente corse fra due avvii insieme), altrove un socket Unix in una
//! cartella dell'utente. Il nome viene dalla cartella dei dati: con
//! FUMETTO_DATI diversi (le prove) le copie restano separate.
//!
//! Il dialogo, uguale dappertutto:
//!   la prima copia scrive il suo pid (4 byte), cosi' su Windows la nuova le
//!   permette di venire in primo piano (AllowSetForegroundWindow: un
//!   programma sullo sfondo non puo' prenderselo da solo);
//!   la nuova scrive il volume (4 byte di lunghezza, poi il percorso; vuoto:
//!   solo "fatti vedere");
//!   la prima risponde con un byte, e la nuova si chiude.
//!
//! Se la prima copia non risponde (bloccata), dopo qualche secondo la nuova
//! parte lo stesso: meglio due finestre che nessuna.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

/// Quanto aspettare la copia gia' aperta prima di partire comunque.
const PATIENCE: Duration = Duration::from_secs(3);
/// Un percorso piu' lungo di cosi' non e' un percorso.
const MAX_MESSAGE: usize = 64 * 1024;

/// Com'e' andata all'avvio.
pub enum Start {
    /// NicoReader era gia' aperto e ha preso il volume: questa copia esce.
    Forwarded,
    /// Si e' i primi: i volumi delle copie dopo arrivano a `on_open`.
    First,
    /// Non si e' potuto ne' passare il volume ne' ascoltare (un errore del
    /// sistema): si va avanti da soli, come prima.
    Alone,
}

/// Passa `path` alla copia gia' aperta, se c'e'; se no comincia ad
/// ascoltare, e ogni volume che arriva (o `None`: solo farsi vedere) va a
/// `on_open`, da un thread a parte.
pub fn start(data: &Path, path: Option<&Path>, on_open: impl Fn(Option<PathBuf>) + Send + 'static) -> Start {
    let endpoint = sys::Endpoint::for_data(data);
    // due tentativi: fra il primo e l'ascolto un'altra copia puo' averci preceduti
    for _ in 0..2 {
        match forward(&endpoint, path) {
            Forward::Done => return Start::Forwarded,
            Forward::Stuck => return Start::Alone,
            Forward::Nobody => {}
        }
        match sys::listen(&endpoint) {
            Ok(Some(listener)) => {
                let spawned = std::thread::Builder::new()
                    .name("istanza".into())
                    .spawn(move || listener.serve(&|conn| serve_one(conn, &on_open)));
                return if spawned.is_ok() { Start::First } else { Start::Alone };
            }
            Ok(None) => continue,
            Err(e) => {
                eprintln!("una sola finestra: {e}");
                return Start::Alone;
            }
        }
    }
    Start::Alone
}

enum Forward {
    /// La copia aperta ha il volume.
    Done,
    /// Non c'e' nessuno in ascolto.
    Nobody,
    /// C'e', ma non risponde.
    Stuck,
}

fn forward(endpoint: &sys::Endpoint, path: Option<&Path>) -> Forward {
    let Some(conn) = sys::connect(endpoint) else { return Forward::Nobody };
    let message = path.map(sys::encode).unwrap_or_default();
    // il dialogo in un thread: se l'altra copia e' bloccata, non si aspetta per sempre
    let (tx, rx) = mpsc::channel();
    let talk = std::thread::Builder::new().name("istanza".into()).spawn(move || {
        let _ = tx.send(ask(conn, &message));
    });
    if talk.is_err() {
        return Forward::Stuck;
    }
    match rx.recv_timeout(PATIENCE) {
        Ok(Ok(())) => Forward::Done,
        Ok(Err(e)) => {
            eprintln!("una sola finestra: {e}");
            Forward::Stuck
        }
        Err(_) => Forward::Stuck,
    }
}

/// La copia nuova: legge il pid, permette il primo piano, manda il volume,
/// aspetta la conferma.
fn ask(mut conn: impl Read + Write, message: &[u8]) -> std::io::Result<()> {
    let mut pid = [0u8; 4];
    conn.read_exact(&mut pid)?;
    sys::allow_foreground(u32::from_le_bytes(pid));
    conn.write_all(&(message.len() as u32).to_le_bytes())?;
    conn.write_all(message)?;
    conn.flush()?;
    let mut ack = [0u8; 1];
    conn.read_exact(&mut ack)
}

/// La prima copia: una richiesta, dal pid alla conferma.
fn serve_one(mut conn: impl Read + Write, on_open: &impl Fn(Option<PathBuf>)) -> std::io::Result<()> {
    conn.write_all(&std::process::id().to_le_bytes())?;
    conn.flush()?;
    let mut len = [0u8; 4];
    conn.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_MESSAGE {
        return Err(std::io::Error::other("messaggio troppo lungo"));
    }
    let mut message = vec![0u8; len];
    conn.read_exact(&mut message)?;
    conn.write_all(&[1])?;
    conn.flush()?;
    on_open((!message.is_empty()).then(|| sys::decode(&message)).flatten());
    Ok(())
}

/// FNV-1a: un nome stabile per la cartella dei dati.
fn hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3))
}

#[cfg(unix)]
mod sys {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};

    pub struct Endpoint {
        socket: PathBuf,
        /// Il proprietario della cartella dei dati: un socket di qualcun
        /// altro con lo stesso nome non si usa.
        owner: Option<u32>,
    }

    impl Endpoint {
        /// Nella cartella privata dell'utente (XDG_RUNTIME_DIR, su Linux), o
        /// in quella temporanea (sul Mac e' gia' dell'utente). Il nome e'
        /// corto: un socket ha al massimo 104 caratteri di percorso.
        pub fn for_data(data: &Path) -> Endpoint {
            let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
            let name = format!("nicoreader-{:016x}.sock", super::hash(&data.to_string_lossy()));
            Endpoint { socket: dir.join(name), owner: std::fs::metadata(data).ok().map(|m| m.uid()) }
        }

        #[cfg(test)]
        pub fn at(socket: PathBuf) -> Endpoint {
            Endpoint { socket, owner: None }
        }

        fn ours(&self) -> bool {
            let socket = std::fs::symlink_metadata(&self.socket);
            match (socket, self.owner) {
                (Ok(m), Some(owner)) => m.uid() == owner,
                _ => true,
            }
        }
    }

    pub fn connect(endpoint: &Endpoint) -> Option<UnixStream> {
        if !endpoint.ours() {
            return None;
        }
        UnixStream::connect(&endpoint.socket).ok()
    }

    pub struct Listener(UnixListener);

    /// Comincia ad ascoltare; `None` se qualcuno ci ha appena preceduti. Un
    /// socket rimasto da una copia chiusa male (nessuno risponde) si toglie.
    pub fn listen(endpoint: &Endpoint) -> std::io::Result<Option<Listener>> {
        if endpoint.socket.exists() && endpoint.ours() && UnixStream::connect(&endpoint.socket).is_err() {
            let _ = std::fs::remove_file(&endpoint.socket);
        }
        match UnixListener::bind(&endpoint.socket) {
            Ok(l) => {
                std::fs::set_permissions(&endpoint.socket, std::fs::Permissions::from_mode(0o600))?;
                Ok(Some(Listener(l)))
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => Ok(None),
            Err(e) => Err(e),
        }
    }

    impl Listener {
        pub fn serve(self, handle: &dyn Fn(UnixStream) -> std::io::Result<()>) {
            for conn in self.0.incoming().flatten() {
                let _ = conn.set_read_timeout(Some(super::PATIENCE));
                if let Err(e) = handle(conn) {
                    eprintln!("una sola finestra: {e}");
                }
            }
        }
    }

    pub fn encode(path: &Path) -> Vec<u8> {
        path.as_os_str().as_bytes().to_vec()
    }

    pub fn decode(bytes: &[u8]) -> Option<PathBuf> {
        Some(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
    }

    /// Solo Windows lo vuole.
    pub fn allow_foreground(_pid: u32) {}
}

#[cfg(windows)]
mod sys {
    use std::fs::File;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::os::windows::io::FromRawHandle;
    use std::path::{Path, PathBuf};

    use windows_sys::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GetLastError, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
        PIPE_UNLIMITED_INSTANCES, PIPE_WAIT, WaitNamedPipeW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

    #[derive(Clone)]
    pub struct Endpoint {
        /// `\\.\pipe\NicoReader-<hash>`, in UTF-16 con lo zero in fondo.
        pipe: Vec<u16>,
    }

    impl Endpoint {
        /// La cartella dei dati sta nel profilo dell'utente: ogni utente ha
        /// la sua pipe. Gli altri potrebbero solo leggerla (la protezione di
        /// serie), e da un altro computer non si arriva.
        pub fn for_data(data: &Path) -> Endpoint {
            let name = format!(r"\\.\pipe\NicoReader-{:016x}", super::hash(&data.to_string_lossy()));
            Endpoint { pipe: name.encode_utf16().chain(Some(0)).collect() }
        }

        fn path(&self) -> String {
            String::from_utf16_lossy(&self.pipe[..self.pipe.len() - 1])
        }
    }

    pub fn connect(endpoint: &Endpoint) -> Option<File> {
        // tutte le istanze della pipe occupate (la prima copia ne sta
        // preparando un'altra): si aspetta un momento
        for _ in 0..5 {
            match std::fs::OpenOptions::new().read(true).write(true).open(endpoint.path()) {
                Ok(f) => return Some(f),
                Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => unsafe {
                    WaitNamedPipeW(endpoint.pipe.as_ptr(), 500);
                },
                Err(_) => return None,
            }
        }
        None
    }

    pub struct Listener {
        endpoint: Endpoint,
        /// La prima istanza, gia' creata: dice che la pipe e' nostra.
        first: File,
    }

    /// Un'istanza della pipe; `first`: deve essere la prima in assoluto.
    fn instance(endpoint: &Endpoint, first: bool) -> std::io::Result<Option<File>> {
        let flags = PIPE_ACCESS_DUPLEX | if first { FILE_FLAG_FIRST_PIPE_INSTANCE } else { 0 };
        let mode = PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS;
        // SAFETY: il nome finisce con lo zero; senza attributi di sicurezza
        // vale la protezione di serie
        let handle = unsafe {
            CreateNamedPipeW(
                endpoint.pipe.as_ptr(),
                flags,
                mode,
                PIPE_UNLIMITED_INSTANCES,
                4096,
                4096,
                0,
                std::ptr::null(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            // la prima istanza c'e' gia': un'altra copia ci ha preceduti
            if first && unsafe { GetLastError() } == ERROR_ACCESS_DENIED {
                return Ok(None);
            }
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: la maniglia e' appena nata, ed e' solo di questo File
        Ok(Some(unsafe { File::from_raw_handle(handle as _) }))
    }

    pub fn listen(endpoint: &Endpoint) -> std::io::Result<Option<Listener>> {
        Ok(instance(endpoint, true)?.map(|first| Listener { endpoint: endpoint.clone(), first }))
    }

    impl Listener {
        pub fn serve(self, handle: &dyn Fn(File) -> std::io::Result<()>) {
            let mut next = Some(self.first);
            loop {
                let pipe = match next.take() {
                    Some(p) => p,
                    None => match instance(&self.endpoint, false) {
                        Ok(Some(p)) => p,
                        _ => return eprintln!("una sola finestra: {}", std::io::Error::last_os_error()),
                    },
                };
                use std::os::windows::io::AsRawHandle;
                // SAFETY: la maniglia e' del File, vivo per tutta la chiamata
                let connected = unsafe { ConnectNamedPipe(pipe.as_raw_handle() as _, std::ptr::null_mut()) } != 0
                    || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
                if connected && let Err(e) = handle(pipe) {
                    eprintln!("una sola finestra: {e}");
                }
            }
        }
    }

    pub fn encode(path: &Path) -> Vec<u8> {
        path.as_os_str().encode_wide().flat_map(u16::to_le_bytes).collect()
    }

    pub fn decode(bytes: &[u8]) -> Option<PathBuf> {
        let (pairs, []) = bytes.as_chunks::<2>() else { return None };
        let wide: Vec<u16> = pairs.iter().map(|&p| u16::from_le_bytes(p)).collect();
        Some(PathBuf::from(std::ffi::OsString::from_wide(&wide)))
    }

    /// La copia che ascolta puo' venire in primo piano: da sola non potrebbe.
    pub fn allow_foreground(pid: u32) {
        // SAFETY: un numero, nient'altro
        unsafe { AllowSetForegroundWindow(pid) };
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn la_seconda_copia_passa_il_volume_alla_prima() {
        let dir = std::env::temp_dir().join(format!("fumetto-istanza-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("s.sock");
        let got: Arc<Mutex<Vec<Option<PathBuf>>>> = Arc::default();
        let sink = got.clone();
        let endpoint = sys::Endpoint::at(socket.clone());
        assert!(matches!(forward(&endpoint, None), Forward::Nobody), "all'inizio non c'e' nessuno");
        let listener = sys::listen(&endpoint).unwrap().expect("si ascolta");
        std::thread::spawn(move || {
            listener.serve(&|conn| serve_one(conn, &|p| sink.lock().unwrap().push(p)));
        });
        assert!(sys::listen(&endpoint).unwrap().is_none(), "una seconda non ascolta");
        let volume = Path::new("/fumetti/Nebbia sul Porto v01.cbz");
        assert!(matches!(forward(&endpoint, Some(volume)), Forward::Done));
        assert!(matches!(forward(&endpoint, None), Forward::Done));
        assert_eq!(*got.lock().unwrap(), [Some(volume.to_path_buf()), None]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn un_socket_rimasto_si_toglie() {
        let dir = std::env::temp_dir().join(format!("fumetto-istanza-vecchia-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("s.sock");
        // una copia chiusa male: il socket c'e', nessuno ascolta
        drop(std::os::unix::net::UnixListener::bind(&socket).unwrap());
        assert!(socket.exists());
        let endpoint = sys::Endpoint::at(socket);
        assert!(matches!(forward(&endpoint, None), Forward::Nobody));
        assert!(sys::listen(&endpoint).unwrap().is_some(), "si ascolta al suo posto");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn una_copia_bloccata_non_ferma_la_nuova() {
        let dir = std::env::temp_dir().join(format!("fumetto-istanza-ferma-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("s.sock");
        // ascolta ma non risponde mai
        let _mute = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let endpoint = sys::Endpoint::at(socket);
        let started = std::time::Instant::now();
        assert!(matches!(forward(&endpoint, None), Forward::Stuck));
        assert!(started.elapsed() < PATIENCE + Duration::from_secs(2));
        let _ = std::fs::remove_dir_all(dir);
    }
}
