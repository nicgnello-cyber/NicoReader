//! Cio' che si cerca nella libreria: una o piu' condizioni, separate da spazi,
//! che devono valere tutte.
//!
//! - Una parola cerca in titolo, serie, autori e tag: "batman".
//! - Una categoria con i due punti cerca solo fra i suoi tag:
//!   "personaggio:wolverine", "autore: moebius", "genere:giallo"; "tag:"
//!   fra tutti i tag. Le categorie si scrivono in italiano o in inglese, al
//!   singolare o al plurale.
//! - Fra virgolette, piu' parole insieme: "autore:\"chris claremont\"",
//!   "\"anno uno\""; il nome di un tag fra virgolette dev'essere quello,
//!   non solo contenerlo.
//! - Un meno davanti esclude: "-tag:letto", "-manga".
//!
//! "personaggio:wolverine autore:claremont -tag:letto" trova i volumi con
//! Wolverine, di Claremont, che non hanno il tag "letto".

use fumetto_core::library::{Group, same_name};

use super::sections::{found_in, matches_text};
use super::*;

/// Dove cercare il testo di una condizione.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Scope {
    /// Titolo, serie, autori e tutti i tag.
    Everywhere,
    /// Solo fra i tag, di qualsiasi categoria.
    Tags,
    /// Solo fra i tag di una categoria.
    Kind(GroupKind),
}

/// Una condizione.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Term {
    pub scope: Scope,
    /// In minuscolo, senza virgolette.
    pub text: String,
    /// Fra virgolette: il nome del tag dev'essere proprio questo.
    pub exact: bool,
    /// Con il meno davanti: il volume non deve rispondere.
    pub negated: bool,
    /// Dove comincia nella ricerca (in byte): per sostituirla.
    pub start: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Query {
    pub terms: Vec<Term>,
    /// La ricerca finisce con uno spazio: l'ultima condizione e' finita
    /// (la tendina non la completa piu').
    pub closed: bool,
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
        let words = split(search);
        let mut terms: Vec<Term> = Vec::new();
        let mut i = 0;
        while i < words.len() {
            let (word, quoted, start) = &words[i];
            let start = *start;
            i += 1;
            let (negated, word) = match word.strip_prefix('-') {
                Some(rest) if !rest.is_empty() => (true, rest),
                _ => (false, word.as_str()),
            };
            let lower = word.to_lowercase();
            let scope = (!quoted).then(|| lower.split_once(':')).flatten().and_then(|(head, rest)| {
                let scope =
                    if head == "tag" || head == "tags" { Some(Scope::Tags) } else { kind_of(head).map(Scope::Kind) };
                scope.map(|s| (s, rest.to_owned()))
            });
            let term = match scope {
                Some((scope, rest)) => {
                    // "autore:" e poi uno spazio: il nome e' la parola dopo
                    // (se non e' a sua volta una condizione)
                    let (text, exact) = match words.get(i) {
                        Some((next, q, _)) if rest.is_empty() && (*q || !is_condition(next)) => {
                            i += 1;
                            (next.to_lowercase(), *q)
                        }
                        _ => split_value(&rest),
                    };
                    Term { scope, text, exact, negated, start }
                }
                None => Term { scope: Scope::Everywhere, text: lower, exact: *quoted, negated, start },
            };
            terms.push(term);
        }
        let closed = search.ends_with(char::is_whitespace);
        Query { terms, closed }
    }

    /// Non si cerca niente.
    pub(super) fn is_empty(&self) -> bool {
        self.terms.iter().all(|t| t.scope == Scope::Everywhere && t.text.is_empty())
    }

    /// Il volume risponde a tutte le condizioni.
    pub(super) fn matches(&self, e: &Entry) -> bool {
        self.terms.iter().all(|t| t.matches(e) != t.negated)
    }

    /// Fra i tag (la vista dei tag), quelli da mostrare di un volume che
    /// risponde: senza condizioni sui tag tutti; se no quelli che una
    /// condizione sceglie (o tutti, se il volume risponde per titolo).
    pub(super) fn shows_group(&self, e: &Entry, g: &Group) -> bool {
        let mut positive = self.terms.iter().filter(|t| !t.negated && !t.text.is_empty()).peekable();
        positive.peek().is_none()
            || positive.any(|t| t.takes(g) || (t.scope == Scope::Everywhere && matches_text(e, &t.text)))
    }

    /// La condizione che si sta scrivendo, se non e' finita ne' esclude:
    /// quella che la tendina completa.
    pub(super) fn typing(&self) -> Option<&Term> {
        self.terms.last().filter(|t| !self.closed && !t.negated)
    }

    /// Cerca solo per parole, senza tag ne' esclusioni: quello che i server
    /// sanno fare.
    pub(super) fn words_only(&self) -> bool {
        self.terms.iter().all(|t| t.scope == Scope::Everywhere && !t.negated)
    }
}

impl Term {
    /// Il volume risponde (senza contare il meno).
    fn matches(&self, e: &Entry) -> bool {
        match self.scope {
            Scope::Everywhere => found_in(e, &self.text),
            Scope::Tags | Scope::Kind(_) => e.groups.iter().any(|g| self.takes(g)),
        }
    }

    /// Il tag risponde: della categoria giusta, con il testo nel nome (o
    /// proprio quel nome, fra virgolette).
    pub(super) fn takes(&self, g: &Group) -> bool {
        let kind = match self.scope {
            Scope::Kind(k) => g.kind == k,
            Scope::Everywhere | Scope::Tags => true,
        };
        let name = if self.exact { same_name(&g.name, &self.text) } else { g.name.to_lowercase().contains(&self.text) };
        kind && name
    }

    /// La condizione scritta, per rimetterla nella ricerca: con il nome di
    /// un tag scelto al posto di cio' che si era scritto.
    pub(super) fn with_tag(&self, kind: GroupKind, name: &str) -> String {
        let minus = if self.negated { "-" } else { "" };
        let head = kind_names(kind)[if italian() { 0 } else { 2 }].to_lowercase();
        format!("{minus}{head}:{}", quote(name))
    }
}

/// Le parole della ricerca, con quelle fra virgolette tenute insieme (e
/// segnate). Dopo "categoria:" le virgolette restano nella parola, che resta
/// una sola (autore:"chris claremont"): le toglie `split_value`.
fn split(search: &str) -> Vec<(String, bool, usize)> {
    let mut words = Vec::new();
    let (mut word, mut quoted, mut inside, mut keep, mut start) = (String::new(), false, false, false, 0);
    for (at, c) in search.char_indices() {
        if word.is_empty() && !quoted {
            start = at;
        }
        if c == '"' {
            if !inside {
                keep = word.contains(':');
            }
            inside = !inside;
            if keep {
                word.push(c);
            } else {
                quoted = true;
            }
        } else if c.is_whitespace() && !inside {
            if !word.is_empty() || quoted {
                words.push((std::mem::take(&mut word), quoted, start));
            }
            quoted = false;
        } else {
            word.push(c);
        }
    }
    if !word.is_empty() || quoted {
        words.push((word, quoted, start));
    }
    words
}

/// Il valore dopo i due punti: fra virgolette e' esatto.
fn split_value(rest: &str) -> (String, bool) {
    match rest.strip_prefix('"') {
        Some(inner) => (inner.strip_suffix('"').unwrap_or(inner).to_owned(), true),
        None => (rest.to_owned(), false),
    }
}

/// La parola e' una condizione con la categoria ("autore:...", "-tag:...").
fn is_condition(word: &str) -> bool {
    let word = word.strip_prefix('-').unwrap_or(word).to_lowercase();
    word.split_once(':').is_some_and(|(head, _)| head == "tag" || head == "tags" || kind_of(head).is_some())
}

/// Un nome da scrivere nella ricerca: fra virgolette se ha spazi.
fn quote(name: &str) -> String {
    if name.contains(char::is_whitespace) { format!("\"{name}\"") } else { name.to_owned() }
}

/// La categoria detta da una parola ("personaggio", "characters", "genere").
fn kind_of(word: &str) -> Option<GroupKind> {
    if let Some((_, kind)) = SYNONYMS.iter().find(|(w, _)| *w == word) {
        return Some(*kind);
    }
    KINDS.into_iter().find(|&kind| kind_names(kind).iter().any(|n| n.to_lowercase() == word))
}

impl Shelf {
    /// Cosa chiedere ai server: la ricerca com'e' scritta, se cerca solo per
    /// parole; niente se ha condizioni sui tag o esclusioni (i server cercano
    /// per titolo, non per tag).
    pub fn server_search(&self) -> String {
        if Query::parse(&self.search).words_only() { self.search.trim().to_owned() } else { String::new() }
    }
}
