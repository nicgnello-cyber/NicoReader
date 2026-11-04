//! Le richieste, con curl (nome e password dallo stdin, non sulla riga di
//! comando), e gli indirizzi.

use super::*;

/// Chiede un indirizzo con curl. `max_time`: i secondi oltre i quali si
/// rinuncia.
pub fn fetch(url: &str, login: Option<(&str, &str)>, max_time: u32) -> Result<Vec<u8>, String> {
    curl(&Request { url, login, ..Request::default() }, Some(max_time))
}

/// Una richiesta per curl, oltre al semplice GET.
#[derive(Default)]
pub(super) struct Request<'a> {
    pub(super) url: &'a str,
    pub(super) login: Option<(&'a str, &'a str)>,
    /// "PATCH", "DELETE"...
    pub(super) method: Option<&'a str>,
    /// Un corpo in JSON.
    pub(super) json: Option<&'a str>,
    /// Dove scrivere la risposta, invece di restituirla.
    pub(super) output: Option<&'a Path>,
}

/// Fa la richiesta con curl. `max_time`: i secondi oltre i quali si rinuncia;
/// `None` (per scaricare un volume intero) solo se il trasferimento si ferma.
pub(super) fn curl(r: &Request, max_time: Option<u32>) -> Result<Vec<u8>, String> {
    let mut config = format!("url = {}\n", quoted(r.url));
    if let Some((user, password)) = r.login {
        config += &format!("user = {}\n", quoted(&format!("{user}:{password}")));
    }
    if let Some(method) = r.method {
        config += &format!("request = {}\n", quoted(method));
    }
    if let Some(json) = r.json {
        config += &format!("header = {}\ndata = {}\n", quoted("Content-Type: application/json"), quoted(json));
    }
    if let Some(output) = r.output {
        config += &format!("output = {}\n", quoted(&output.to_string_lossy()));
    }
    let mut curl = Command::new("curl");
    // solo http e https, anche dopo un rinvio: un catalogo non puo' far
    // leggere a curl un file del computer o parlare altri protocolli
    curl.args(["--config", "-", "--globoff", "--location", "--fail", "--silent", "--show-error"])
        .args(["--proto", "=http,https", "--proto-redir", "=http,https"])
        .args(["--connect-timeout", "10"])
        .args(["--user-agent", concat!("NicoReader/", env!("CARGO_PKG_VERSION"))]);
    match max_time {
        Some(t) => curl.args(["--max-time", &t.to_string()]),
        // meno di 1 KB al secondo per un minuto: il trasferimento e' fermo
        None => curl.args(["--speed-limit", "1024", "--speed-time", "60"]),
    };
    curl.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    crate::upscale::hide_window(&mut curl);
    let mut child = curl.spawn().map_err(|e| format!("curl: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        // curl legge tutta la configurazione prima di cominciare; chiuso lo
        // stdin (qui, alla fine del blocco) sa che non c'e' altro
        let _ = stdin.write_all(config.as_bytes());
    }
    let out = child.wait_with_output().map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(explain(&String::from_utf8_lossy(&out.stderr)));
    }
    Ok(out.stdout)
}

/// Un valore fra virgolette per la configurazione di curl.
pub(super) fn quoted(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '\n' | '\r' => {}
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// L'errore di curl detto per chi legge.
pub(super) fn explain(stderr: &str) -> String {
    let raw = stderr.trim().trim_start_matches("curl: ").to_owned();
    let code = |c: &str| raw.contains(&format!("error: {c}"));
    let text = if code("401") || code("403") {
        t("nome o password sbagliati", "wrong user name or password")
    } else if code("404") {
        t("non trovato sul server", "not found on the server")
    } else if raw.starts_with("(6)") {
        t("indirizzo sconosciuto", "unknown address")
    } else if raw.starts_with("(7)") {
        t("il server non risponde: \u{e8} acceso?", "the server doesn't answer: is it on?")
    } else if raw.starts_with("(28)") {
        t("il server ci mette troppo a rispondere", "the server takes too long to answer")
    } else {
        return raw;
    };
    // "(7) Failed to connect...": il numero di curl non dice niente a chi legge
    let detail = raw.split_once(") ").filter(|(code, _)| code.starts_with('(')).map_or(raw.as_str(), |(_, d)| d);
    format!("{text} ({detail})")
}

/// "http://casa:8080/opds?x" -> "http://casa:8080".
pub(super) fn origin(url: &str) -> &str {
    let after = url.find("://").map_or(0, |i| i + 3);
    let end = url[after..].find(['/', '?', '#']).map_or(url.len(), |i| after + i);
    &url[..end]
}

/// Un indirizzo del feed (anche relativo, come li da' Kavita) reso completo.
pub(super) fn join(base: &str, href: &str) -> String {
    let href = href.trim();
    if is_remote(Path::new(href)) {
        return href.to_owned();
    }
    if let Some(rest) = href.strip_prefix("//") {
        let scheme = base.split_once("://").map_or("http", |(s, _)| s);
        return format!("{scheme}://{rest}");
    }
    if href.starts_with('/') {
        return format!("{}{href}", origin(base));
    }
    let base = base.split(['#']).next().unwrap_or(base);
    if href.starts_with('?') {
        return format!("{}{href}", base.split('?').next().unwrap_or(base));
    }
    let path = base.split('?').next().unwrap_or(base);
    let dir =
        if path.len() > origin(path).len() { &path[..path.rfind('/').map_or(path.len(), |i| i + 1)] } else { path };
    let sep = if dir.ends_with('/') { "" } else { "/" };
    format!("{dir}{sep}{}", href.trim_start_matches("./"))
}
