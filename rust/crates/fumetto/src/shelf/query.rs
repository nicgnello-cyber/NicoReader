//! Cio' che si cerca nella libreria: il testo scritto e, se davanti c'e' una
//! categoria con i due punti, solo fra i tag di quella categoria:
//! "personaggio:wolverine", "autore: moebius", "genere:giallo". "tag:" cerca
//! fra tutti i tag, di qualsiasi categoria. Le categorie si scrivono in
//! italiano o in inglese, al singolare o al plurale.

use fumetto_core::library::Group;

use super::sections::found_in;
use super::*;

/// Dove cercare il testo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Scope {
    /// Titolo, serie, autori e tutti i tag.
    Everywhere,
    /// Solo fra i tag, di qualsiasi categoria.
    Tags,
    /// Solo fra i tag di una categoria.
    Kind(GroupKind),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Query {
    pub scope: Scope,
    /// In minuscolo, senza spazi ai lati.
    pub text: String,
}

/// Le parole che dicono una categoria, oltre ai suoi nomi (kind_label) nelle
/// due lingue.
const SYNONYMS: &[(&str, GroupKind)] = &[
    ("genere", GroupKind::Genre),
    ("generi", GroupKind::Genre),
    ("tipo", GroupKind::Format),
    ("tipi", GroupKind::Format),
    ("lista", GroupKind::ReadingList),
    ("liste", GroupKind::ReadingList),
    ("list", GroupKind::ReadingList),
    ("lists", GroupKind::ReadingList),
    ("format", GroupKind::Format),
];

impl Query {
    pub(super) fn parse(search: &str) -> Query {
        let lower = search.trim().to_lowercase();
        if let Some((head, rest)) = lower.split_once(':') {
            let head = head.trim();
            let scope =
                if head == "tag" || head == "tags" { Some(Scope::Tags) } else { kind_of(head).map(Scope::Kind) };
            if let Some(scope) = scope {
                return Query { scope, text: rest.trim().to_owned() };
            }
        }
        Query { scope: Scope::Everywhere, text: lower }
    }

    /// Non si cerca niente.
    pub(super) fn is_empty(&self) -> bool {
        self.scope == Scope::Everywhere && self.text.is_empty()
    }

    /// Il volume risponde.
    pub(super) fn matches(&self, e: &Entry) -> bool {
        match self.scope {
            Scope::Everywhere => found_in(e, &self.text),
            _ => e.groups.iter().any(|g| self.takes(g)),
        }
    }

    /// Il tag risponde: della categoria giusta, con il testo nel nome.
    pub(super) fn takes(&self, g: &Group) -> bool {
        let kind = match self.scope {
            Scope::Kind(k) => g.kind == k,
            Scope::Everywhere | Scope::Tags => true,
        };
        kind && g.name.to_lowercase().contains(&self.text)
    }

    /// Il testo cerca anche titoli, serie e autori (senza una categoria davanti).
    pub(super) fn everywhere(&self) -> bool {
        self.scope == Scope::Everywhere
    }
}

/// La categoria detta da una parola ("personaggio", "characters", "genere").
fn kind_of(word: &str) -> Option<GroupKind> {
    if let Some((_, kind)) = SYNONYMS.iter().find(|(w, _)| *w == word) {
        return Some(*kind);
    }
    KINDS.into_iter().find(|&kind| kind_names(kind).iter().any(|n| n.to_lowercase() == word))
}

impl Shelf {
    /// Cosa chiedere ai server: la ricerca com'e' scritta; niente se ha una
    /// categoria davanti (i server cercano per titolo, non per tag).
    pub fn server_search(&self) -> String {
        if Query::parse(&self.search).everywhere() { self.search.trim().to_owned() } else { String::new() }
    }
}
