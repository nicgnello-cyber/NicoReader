//! Le pagine di un archivio che si decomprime tutto all'apertura (RAR solido,
//! 7z): saltare a una pagina costerebbe decomprimere tutto cio' che la
//! precede. Stanno in memoria fino a `IN_MEMORY`; quelle dopo vanno in una
//! cartella temporanea, che sparisce quando si chiude il volume. Cosi' un
//! omnibus da qualche GB non riempie la RAM.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::comicinfo::{ComicInfo, is_comic_info};

/// Quanto di un archivio si tiene in memoria: i volumi normali ci stanno
/// interi, e si leggono senza toccare il disco.
const IN_MEMORY: usize = 512 << 20;

/// Le cartelle lasciate da un programma che non si e' chiuso bene si tolgono
/// dopo un giorno: prima potrebbero essere di un'altra copia ancora aperta
/// (le prove, per esempio).
const STALE: Duration = Duration::from_secs(24 * 60 * 60);

pub(super) struct Unpacked {
    memory: HashMap<String, Vec<u8>>,
    in_memory: usize,
    limit: usize,
    spill: Spill,
}

/// La cartella temporanea, creata alla prima pagina che non sta in memoria.
enum Spill {
    None,
    Dir {
        dir: PathBuf,
        files: HashMap<String, PathBuf>,
    },
    /// Non si e' potuta creare (o il disco e' pieno): tutto in memoria.
    Failed,
}

impl Unpacked {
    pub(super) fn new() -> Unpacked {
        Unpacked::with_limit(IN_MEMORY)
    }

    fn with_limit(limit: usize) -> Unpacked {
        Unpacked { memory: HashMap::new(), in_memory: 0, limit, spill: Spill::None }
    }

    /// Una pagina (o la scheda) decompressa.
    pub(super) fn add(&mut self, name: String, bytes: Vec<u8>) {
        if self.in_memory + bytes.len() > self.limit {
            match self.write(&name, &bytes) {
                Ok(true) => return,
                Ok(false) => {}
                Err(e) => {
                    // il disco pieno: da qui si resta in memoria
                    eprintln!("archivio, cartella temporanea: {e}");
                    self.spill = Spill::Failed;
                }
            }
        }
        self.in_memory += bytes.len();
        self.memory.insert(name, bytes);
    }

    /// Scrive la pagina nella cartella temporanea: `false` se non c'e'.
    fn write(&mut self, name: &str, bytes: &[u8]) -> io::Result<bool> {
        if let Spill::None = self.spill {
            self.spill = match spill_dir() {
                Ok(dir) => Spill::Dir { dir, files: HashMap::new() },
                Err(e) => {
                    eprintln!("archivio, cartella temporanea: {e}");
                    Spill::Failed
                }
            };
        }
        let Spill::Dir { dir, files } = &mut self.spill else { return Ok(false) };
        // un nome semplice: quello dentro l'archivio puo' avere cartelle,
        // caratteri strani, o ripetersi in cartelle diverse
        let file = dir.join(format!("{:05}", files.len()));
        std::fs::write(&file, bytes)?;
        files.insert(name.to_owned(), file);
        Ok(true)
    }

    /// I nomi di tutto cio' che si e' decompresso.
    pub(super) fn names(&self) -> impl Iterator<Item = &str> {
        let on_disk = match &self.spill {
            Spill::Dir { files, .. } => Some(files.keys()),
            Spill::None | Spill::Failed => None,
        };
        self.memory.keys().chain(on_disk.into_iter().flatten()).map(String::as_str)
    }

    /// I byte di una pagina.
    pub(super) fn read(&self, name: &str) -> io::Result<Vec<u8>> {
        self.head(name, usize::MAX)
    }

    /// I primi `max` byte di una pagina.
    pub(super) fn head(&self, name: &str, max: usize) -> io::Result<Vec<u8>> {
        if let Some(bytes) = self.memory.get(name) {
            return Ok(bytes[..bytes.len().min(max)].to_vec());
        }
        match &self.spill {
            Spill::Dir { files, .. } if let Some(file) = files.get(name) => {
                let mut out = Vec::new();
                File::open(file)?.take(max as u64).read_to_end(&mut out)?;
                Ok(out)
            }
            _ => Err(io::Error::new(io::ErrorKind::NotFound, format!("{name}: non e' nell'archivio"))),
        }
    }

    /// La ComicInfo.xml, se c'era.
    pub(super) fn comic_info(&self) -> Option<ComicInfo> {
        let name = self.names().find(|n| is_comic_info(n))?;
        ComicInfo::parse(&self.read(name).ok()?)
    }

    /// Quante pagine stanno nella cartella temporanea (per le prove).
    #[cfg(test)]
    fn on_disk(&self) -> usize {
        match &self.spill {
            Spill::Dir { files, .. } => files.len(),
            Spill::None | Spill::Failed => 0,
        }
    }
}

impl Drop for Spill {
    fn drop(&mut self) {
        if let Spill::Dir { dir, .. } = self {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// Una cartella nuova per un archivio, in quella temporanea del sistema;
/// intanto si tolgono quelle vecchie rimaste da un programma chiuso male.
fn spill_dir() -> io::Result<PathBuf> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join("nicoreader-archivi");
    let mine = format!("{}-", std::process::id());
    if let Ok(entries) = std::fs::read_dir(&root) {
        for entry in entries.flatten() {
            let old =
                entry.metadata().and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().unwrap_or_default() > STALE);
            if old && !entry.file_name().to_string_lossy().starts_with(&mine) {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    let dir = root.join(format!("{mine}{}", NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oltre_il_limite_le_pagine_vanno_su_disco() {
        let mut unpacked = Unpacked::with_limit(10);
        unpacked.add("a/p1.jpg".into(), vec![1; 8]);
        unpacked.add("b/p1.jpg".into(), vec![2; 8]); // non ci sta piu'
        unpacked.add("ComicInfo.xml".into(), b"<ComicInfo><Number>7</Number></ComicInfo>".to_vec());
        assert_eq!(unpacked.on_disk(), 2);
        let mut names: Vec<&str> = unpacked.names().collect();
        names.sort_unstable();
        assert_eq!(names, ["ComicInfo.xml", "a/p1.jpg", "b/p1.jpg"]);
        assert_eq!(unpacked.read("a/p1.jpg").unwrap(), [1; 8]);
        assert_eq!(unpacked.read("b/p1.jpg").unwrap(), [2; 8]);
        assert_eq!(unpacked.head("b/p1.jpg", 3).unwrap(), [2; 3]);
        assert!(unpacked.read("c.jpg").is_err());
        assert_eq!(unpacked.comic_info().and_then(|i| i.number).as_deref(), Some("7"), "anche la scheda su disco");
        // chiuso il volume, la cartella sparisce
        let Spill::Dir { dir, .. } = &unpacked.spill else { panic!("cartella temporanea") };
        let dir = dir.clone();
        assert!(dir.is_dir());
        drop(unpacked);
        assert!(!dir.exists());
    }
}
