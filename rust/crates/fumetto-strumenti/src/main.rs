//! Gli attrezzi degli script dei pacchetti (pacchetto.sh, pacchetto-mac.sh,
//! pacchetto.ps1, prova-finder.sh), uguali su tutti i sistemi:
//!
//!     cargo run -q -p fumetto-strumenti -- licenze <versione> <file> <triple>...
//!     cargo run -q -p fumetto-strumenti -- icona
//!     cargo run -q -p fumetto-strumenti -- volumi-di-prova <cartella> <nome>...
//!
//! `licenze` scrive i testi di licenza delle librerie che finiscono
//! nell'eseguibile e stampa quante sono; `icona` ridisegna le icone in
//! crates/fumetto/risorse; `volumi-di-prova` fa dei CBZ di una pagina.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod icona;
mod licenze;
mod prova;

const USO: &str = "uso:
  fumetto-strumenti licenze <versione> <file da scrivere> <triple> [<triple>...]
  fumetto-strumenti icona
  fumetto-strumenti volumi-di-prova <cartella> <nome> [<nome>...]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let fatto = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["licenze", versione, uscita, ref triple @ ..] if !triple.is_empty() => {
            licenze::scrivi(&workspace().join("Cargo.toml"), versione, Path::new(uscita), triple)
                .map(|n| println!("{n}"))
        }
        ["icona"] => icona::scrivi(&workspace().join("crates/fumetto/risorse")),
        ["volumi-di-prova", cartella, ref nomi @ ..] if !nomi.is_empty() => prova::scrivi(Path::new(cartella), nomi),
        _ => {
            eprintln!("{USO}");
            return ExitCode::from(2);
        }
    };
    match fatto {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("fumetto-strumenti: {e}");
            ExitCode::FAILURE
        }
    }
}

/// La cartella rust/, da qualunque cartella si lanci.
fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
