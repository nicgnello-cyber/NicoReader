//! Le licenze delle librerie Rust (e di quelle in C e C++ che contengono)
//! che finiscono nell'eseguibile: per ogni pacchetto nome, versione, licenza
//! dichiarata e i testi di licenza che porta con se'. Le dipendenze solo per
//! compilare o per le prove restano fuori. Piu' triple insieme per un
//! eseguibile universale (macOS: Intel e Apple Silicon).

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

/// Il pacchetto da cui si parte: l'eseguibile.
const RADICE: &str = "fumetto";
/// I nomi dei file di licenza (all'inizio del nome: LICENSE-MIT, COPYING.txt...).
const NOMI: [&str; 6] = ["LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE", "license"];
/// Quante cartelle sotto quella del pacchetto si cercano.
const PROFONDITA: usize = 2;

/// Quello che serve di `cargo metadata --format-version 1`.
#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    resolve: Resolve,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    version: String,
    license: Option<String>,
    repository: Option<String>,
    /// `None`: un pacchetto di questo workspace.
    source: Option<String>,
    manifest_path: PathBuf,
}

#[derive(Deserialize)]
struct Resolve {
    nodes: Vec<Node>,
}

#[derive(Deserialize)]
struct Node {
    id: String,
    deps: Vec<NodeDep>,
}

#[derive(Deserialize)]
struct NodeDep {
    pkg: String,
    dep_kinds: Vec<DepKind>,
}

#[derive(Deserialize)]
struct DepKind {
    /// `None` per le dipendenze normali; "build" o "dev" le altre.
    kind: Option<String>,
}

/// Scrive il file delle licenze; dice quante librerie ci sono.
pub fn scrivi(manifest: &Path, versione: &str, uscita: &Path, triple: &[&str]) -> io::Result<usize> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut comando = Command::new(cargo);
    comando.args(["metadata", "--format-version", "1", "--manifest-path"]).arg(manifest);
    for t in triple {
        comando.args(["--filter-platform", t]);
    }
    let out = comando.output()?;
    if !out.status.success() {
        let errore = String::from_utf8_lossy(&out.stderr);
        return Err(io::Error::other(format!("cargo metadata: {}", errore.trim())));
    }
    let meta: Metadata = serde_json::from_slice(&out.stdout).map_err(io::Error::other)?;
    let pacchetti = nell_eseguibile(&meta)?;
    std::fs::write(uscita, testo(versione, &pacchetti)?)?;
    Ok(pacchetti.len())
}

/// I pacchetti venuti da fuori che finiscono nell'eseguibile, per nome e
/// versione: dalla radice, seguendo solo le dipendenze normali.
fn nell_eseguibile(meta: &Metadata) -> io::Result<Vec<&Package>> {
    let per_id: HashMap<&str, &Package> = meta.packages.iter().map(|p| (p.id.as_str(), p)).collect();
    let nodi: HashMap<&str, &Node> = meta.resolve.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let radice = meta.packages.iter().find(|p| p.name == RADICE).ok_or_else(|| io::Error::other("manca fumetto"))?;
    let mut visti = HashSet::new();
    let mut coda = vec![radice.id.as_str()];
    while let Some(id) = coda.pop() {
        if !visti.insert(id) {
            continue;
        }
        let Some(nodo) = nodi.get(id) else { continue };
        // solo le dipendenze normali: niente build.rs ne' prove
        coda.extend(nodo.deps.iter().filter(|d| d.dep_kinds.iter().any(|k| k.kind.is_none())).map(|d| d.pkg.as_str()));
    }
    let mut pacchetti: Vec<&Package> =
        visti.into_iter().filter_map(|id| per_id.get(id).copied()).filter(|p| p.source.is_some()).collect();
    pacchetti.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
    Ok(pacchetti)
}

fn testo(versione: &str, pacchetti: &[&Package]) -> io::Result<String> {
    let riga = "=".repeat(78);
    let mut out = format!(
        "NicoReader {versione} contiene queste librerie. Qui sotto le loro licenze.\n\
         NicoReader contains these libraries. Their licenses follow.\n\n"
    );
    for p in pacchetti {
        let licenza = p.license.as_deref().unwrap_or("licenza non dichiarata");
        let _ = writeln!(out, "{riga}\n{} {}   ({licenza})", p.name, p.version);
        if let Some(repo) = &p.repository {
            let _ = writeln!(out, "{repo}");
        }
        let _ = writeln!(out, "{riga}");
        let cartella = p.manifest_path.parent().unwrap_or(Path::new("."));
        let mut testi = Vec::new();
        cerca(cartella, 0, &mut testi)?;
        // per nome, come si leggono
        testi.sort_by_key(|t| t.to_string_lossy().into_owned());
        for t in &testi {
            let nome = t.strip_prefix(cartella).unwrap_or(t);
            // gli a capo di Windows diventano quelli di tutti
            let contenuto = String::from_utf8_lossy(&std::fs::read(t)?).replace("\r\n", "\n").replace('\r', "\n");
            let _ = writeln!(out, "--- {}\n{contenuto}", nome.display());
        }
        if testi.is_empty() {
            out.push_str("(testo non incluso nel pacchetto: vale la licenza dichiarata)\n");
        }
        out.push('\n');
    }
    Ok(out)
}

/// I file di licenza nella cartella e fino a `PROFONDITA` cartelle sotto.
fn cerca(cartella: &Path, livello: usize, testi: &mut Vec<PathBuf>) -> io::Result<()> {
    for voce in std::fs::read_dir(cartella)? {
        let voce = voce?;
        if voce.file_type()?.is_dir() {
            if livello < PROFONDITA {
                cerca(&voce.path(), livello + 1, testi)?;
            }
        } else if NOMI.iter().any(|n| voce.file_name().to_string_lossy().starts_with(n)) {
            testi.push(voce.path());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solo_quel_che_finisce_nell_eseguibile() {
        let dir = std::env::temp_dir().join(format!("fumetto-licenze-{}", std::process::id()));
        let a = dir.join("a-1.0");
        std::fs::create_dir_all(a.join("licenze/vecchie/troppo")).unwrap();
        std::fs::write(a.join("LICENSE-MIT"), "MIT\r\ntesto").unwrap();
        std::fs::write(a.join("licenze/COPYING"), "GPL").unwrap();
        std::fs::write(a.join("licenze/vecchie/NOTICE"), "nota").unwrap();
        std::fs::write(a.join("licenze/vecchie/troppo/LICENSE"), "troppo in fondo").unwrap();
        std::fs::write(a.join("README"), "non e' una licenza").unwrap();
        let src = Some("registry+https://github.com/rust-lang/crates.io-index".to_owned());
        let pacchetto = |id: &str, cartella: &Path| Package {
            id: id.into(),
            name: id.into(),
            version: "1.0.0".into(),
            license: Some("MIT".into()),
            repository: None,
            source: src.clone(),
            manifest_path: cartella.join("Cargo.toml"),
        };
        let dep = |pkg: &str, kind: Option<&str>| NodeDep {
            pkg: pkg.into(),
            dep_kinds: vec![DepKind { kind: kind.map(Into::into) }],
        };
        let meta = Metadata {
            packages: vec![
                Package { source: None, ..pacchetto("fumetto", &dir) },
                Package { repository: Some("https://esempio.it/a".into()), ..pacchetto("a", &a) },
                pacchetto("b", &dir.join("b")),
                pacchetto("solo-build", &dir),
                pacchetto("solo-prove", &dir),
                Package { source: None, ..pacchetto("fumetto-core", &dir) },
            ],
            resolve: Resolve {
                nodes: vec![
                    Node {
                        id: "fumetto".into(),
                        deps: vec![
                            dep("fumetto-core", None),
                            dep("solo-build", Some("build")),
                            dep("solo-prove", Some("dev")),
                        ],
                    },
                    Node { id: "fumetto-core".into(), deps: vec![dep("b", None), dep("a", None)] },
                    Node { id: "a".into(), deps: vec![dep("b", None)] },
                    Node { id: "b".into(), deps: vec![] },
                ],
            },
        };
        let pacchetti = nell_eseguibile(&meta).unwrap();
        let nomi: Vec<&str> = pacchetti.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(nomi, ["a", "b"], "ne' il workspace, ne' build.rs, ne' le prove");

        std::fs::create_dir_all(dir.join("b")).unwrap();
        let t = testo("9.9", &pacchetti).unwrap();
        let sep = std::path::MAIN_SEPARATOR;
        assert!(t.starts_with("NicoReader 9.9 contiene"));
        assert!(t.contains("a 1.0.0   (MIT)\nhttps://esempio.it/a\n"));
        assert!(t.contains("--- LICENSE-MIT\nMIT\ntesto\n"), "a capo di Windows: {t}");
        assert!(t.contains(&format!("--- licenze{sep}COPYING\nGPL\n")));
        assert!(t.contains(&format!("--- licenze{sep}vecchie{sep}NOTICE\nnota\n")));
        assert!(!t.contains("troppo in fondo") && !t.contains("README"));
        assert!(t.contains(&format!("b 1.0.0   (MIT)\n{}\n(testo non incluso", "=".repeat(78))));
        let _ = std::fs::remove_dir_all(dir);
    }
}
