//! Scrivere un file dei dati (progressi, impostazioni, correzioni, cache)
//! senza mai lasciarlo a meta': prima accanto, con un altro nome, poi al suo
//! posto in un colpo solo. Se il programma si chiude (o il disco si riempie)
//! mentre scrive, resta il file di prima.

use std::io;
use std::path::Path;

/// Scrive `bytes` in `path`, creando la cartella se manca.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write(path, bytes, false)
}

/// Come `write_atomic`, per un file che solo chi lo possiede deve leggere
/// (le impostazioni, con le password che il portachiavi non ha preso).
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write(path, bytes, true)
}

fn write(path: &Path, bytes: &[u8], private: bool) -> io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = private;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn si_scrive_tutto_o_niente() {
        let dir = std::env::temp_dir().join(format!("fumetto-file-{}", std::process::id()));
        let file = dir.join("dentro").join("dati.json");
        write_atomic(&file, b"uno").unwrap();
        write_atomic(&file, b"due").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"due");
        assert!(!file.with_extension("tmp").exists(), "il temporaneo non resta");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            write_private(&file, b"segreto").unwrap();
            assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
