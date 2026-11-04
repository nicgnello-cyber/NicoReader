//! Cio' che serve a leggere l'XML con quick-xml, uguale per i pacchetti
//! EPUB e per i feed dei server.

use quick_xml::events::BytesRef;

/// Aggiunge a `text` il carattere di un riferimento (`&#233;`, `&amp;`): i
/// numerici e i cinque con nome di XML; gli altri non lasciano niente.
/// `None` se il nome non si legge.
pub(crate) fn push_reference(text: &mut String, r: &BytesRef) -> Option<()> {
    if let Ok(Some(c)) = r.resolve_char_ref() {
        text.push(c);
        return Some(());
    }
    text.push_str(match r.decode().ok()?.as_ref() {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        _ => "",
    });
    Some(())
}
