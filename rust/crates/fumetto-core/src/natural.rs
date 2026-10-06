//! Ordinamento "naturale": `pagina10` viene dopo `pagina9`, non prima.

use std::cmp::Ordering;

/// Confronta due nomi come li ordinerebbe una persona: le sequenze di cifre
/// valgono come numeri, il resto carattere per carattere senza distinguere
/// maiuscole e minuscole.
///
/// Si procede carattere per carattere e non a blocchi: cosi' "pagina.jpg"
/// viene prima di "pagina2.jpg" (il punto precede le cifre), mentre la
/// versione Python confrontava "pagina" con "pagina.jpg" e metteva la
/// copertina senza numero dopo la pagina 2.
///
/// A parita' (`01` e `1`) decide il confronto byte per byte, cosi' l'ordine
/// e' totale e non dipende da quello in cui l'archivio elenca i file.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a, b);
    loop {
        let (cx, cy) = match (x.chars().next(), y.chars().next()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(cx), Some(cy)) => (cx, cy),
        };
        let ord;
        if cx.is_ascii_digit() && cy.is_ascii_digit() {
            let (nx, rx) = digits(x);
            let (ny, ry) = digits(y);
            ord = cmp_number(nx, ny);
            (x, y) = (rx, ry);
        } else {
            ord = cx.to_lowercase().cmp(cy.to_lowercase());
            (x, y) = (&x[cx.len_utf8()..], &y[cy.len_utf8()..]);
        }
        if ord != Ordering::Equal {
            return ord;
        }
    }
}

/// Separa la sequenza di cifre in testa dal resto.
fn digits(s: &str) -> (&str, &str) {
    s.split_at(s.bytes().position(|b| !b.is_ascii_digit()).unwrap_or(s.len()))
}

/// Confronto numerico senza convertire: "000123456789012345678901" non sta in un u64.
fn cmp_number(a: &str, b: &str) -> Ordering {
    let a = a.trim_start_matches('0');
    let b = b.trim_start_matches('0');
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(v: &[&str]) -> Vec<String> {
        let mut v: Vec<String> = v.iter().map(|s| s.to_string()).collect();
        v.sort_by(|a, b| natural_cmp(a, b));
        v
    }

    #[test]
    fn numeri_come_numeri() {
        assert_eq!(sorted(&["p10.jpg", "p9.jpg", "p1.jpg"]), ["p1.jpg", "p9.jpg", "p10.jpg"]);
    }

    #[test]
    fn maiuscole_indifferenti() {
        assert_eq!(sorted(&["b.png", "A.png", "a2.png"]), ["A.png", "a2.png", "b.png"]);
    }

    #[test]
    fn capitoli_e_pagine() {
        assert_eq!(sorted(&["c2/p1", "c10/p1", "c2/p10", "c2/p2"]), ["c2/p1", "c2/p2", "c2/p10", "c10/p1"]);
    }

    #[test]
    fn numeri_enormi_e_zeri() {
        assert_eq!(
            sorted(&["x99999999999999999999999", "x0100", "x1", "x01"]),
            ["x01", "x1", "x0100", "x99999999999999999999999"]
        );
    }

    #[test]
    fn numero_prima_del_testo() {
        assert_eq!(sorted(&["a", "1a", ""]), ["", "1a", "a"]);
    }

    #[test]
    fn copertina_senza_numero_prima() {
        assert_eq!(
            sorted(&["pagina2.jpg", "pagina.jpg", "pagina10.jpg"]),
            ["pagina.jpg", "pagina2.jpg", "pagina10.jpg"]
        );
    }

    #[test]
    fn caratteri_non_ascii() {
        assert_eq!(sorted(&["第2話", "第10話", "2é", "éa"]), ["2é", "éa", "第2話", "第10話"]);
    }
}
