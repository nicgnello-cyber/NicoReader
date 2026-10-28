//! La jump list di Windows: con il tasto destro sull'icona nella barra delle
//! applicazioni (o sul pulsante nel menu Start) ci sono gli ultimi volumi
//! letti, come i Recenti del menu, e un clic ne riapre uno.
//!
//! Ogni voce e' un collegamento a NicoReader con il volume come argomento:
//! cosi' ci stanno anche i volumi dei server, che non sono file. Windows la
//! tiene da se' fra un avvio e l'altro; la si riscrive quando cambiano i
//! recenti, in un thread a parte (le chiamate COM vogliono il loro
//! appartamento, e non devono fermare la finestra).
//!
//! Un volume tolto dalla lista a mano ("Rimuovi da questo elenco") non va
//! rimesso: Windows rifiuterebbe tutta la categoria. Si ricorda in un file
//! accanto ai progressi. Fuori da Windows qui non si fa niente.

use std::path::{Path, PathBuf};

/// Quante voci al massimo (Windows ne mostra di solito una decina).
pub const MAX_ITEMS: usize = 10;

/// Una voce: il volume e come si chiama.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub path: PathBuf,
    pub title: String,
}

pub struct JumpList {
    /// L'ultima lista mandata: si riscrive solo se cambia.
    last: Option<Vec<Item>>,
    #[cfg(windows)]
    worker: Option<std::sync::mpsc::Sender<Vec<Item>>>,
}

impl JumpList {
    /// `data`: la cartella dei dati; senza (la prova automatica) la jump list
    /// vera non si tocca.
    pub fn new(data: Option<&Path>) -> JumpList {
        #[cfg(windows)]
        let worker = data.map(|d| win::spawn(d.join("jumplist-tolti.json")));
        #[cfg(not(windows))]
        let _ = data;
        JumpList {
            last: None,
            #[cfg(windows)]
            worker,
        }
    }

    /// I volumi da mostrare, dal piu' recente.
    pub fn update(&mut self, mut items: Vec<Item>) {
        items.truncate(MAX_ITEMS);
        if self.last.as_ref() == Some(&items) {
            return;
        }
        #[cfg(windows)]
        if let Some(w) = &self.worker {
            let _ = w.send(items.clone());
        }
        self.last = Some(items);
    }
}

/// Un argomento della riga di comando di Windows, scritto come lo rilegge
/// `CommandLineToArgvW` (e la libreria standard di Rust): fra virgolette se
/// serve, con le barre prima di una virgoletta raddoppiate.
#[cfg_attr(not(windows), allow(dead_code))]
fn quote_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_owned();
    }
    let mut out = String::from('"');
    let mut slashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => slashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', 2 * slashes + 1));
                slashes = 0;
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', slashes));
                slashes = 0;
            }
        }
        if c != '\\' {
            out.push(c);
        }
    }
    // le barre in fondo, prima della virgoletta che chiude
    out.extend(std::iter::repeat_n('\\', 2 * slashes));
    out.push('"');
    out
}

#[cfg(windows)]
mod win {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::{Receiver, Sender, channel};

    use fumetto_core::lingua::t;
    use windows::Win32::Foundation::E_FAIL;
    use windows::Win32::Storage::EnhancedStorage::PKEY_Title;
    use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PVCHF_DEFAULT, PropVariantChangeType};
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    };
    use windows::Win32::System::Variant::VT_LPWSTR;
    use windows::Win32::UI::Shell::Common::{IObjectArray, IObjectCollection};
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
    use windows::Win32::UI::Shell::{
        DestinationList, EnumerableObjectCollection, ICustomDestinationList, IShellLinkW, ShellLink,
    };
    use windows::core::{HSTRING, Interface, Result};

    use super::{Item, quote_arg};

    /// Il thread che scrive la jump list: riceve le liste, e delle tante
    /// arrivate insieme scrive solo l'ultima.
    pub(super) fn spawn(removed_file: PathBuf) -> Sender<Vec<Item>> {
        let (tx, rx) = channel();
        let started = std::thread::Builder::new().name("jump list".into()).spawn(move || run(&rx, &removed_file));
        if let Err(e) = started {
            eprintln!("jump list: {e}");
        }
        tx
    }

    fn run(rx: &Receiver<Vec<Item>>, removed_file: &Path) {
        if let Err(e) = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok() {
            return eprintln!("jump list: {e}");
        }
        let mut removed = load(removed_file);
        while let Ok(mut items) = rx.recv() {
            while let Ok(newer) = rx.try_recv() {
                items = newer;
            }
            let before = removed.len();
            if let Err(e) = unsafe { commit(&items, &mut removed) } {
                eprintln!("jump list: {e}");
            }
            if removed.len() != before {
                save(removed_file, &removed);
            }
        }
    }

    /// Riscrive la lista. Prima si raccolgono le voci tolte a mano da
    /// quando la si e' scritta l'ultima volta: non si possono rimettere.
    unsafe fn commit(items: &[Item], removed: &mut BTreeSet<String>) -> Result<()> {
        unsafe {
            let list: ICustomDestinationList = CoCreateInstance(&DestinationList, None, CLSCTX_INPROC_SERVER)?;
            let mut slots = 0u32;
            let gone: IObjectArray = list.BeginList(&mut slots)?;
            for i in 0..gone.GetCount()? {
                let link: IShellLinkW = gone.GetAt(i)?;
                let mut buf = vec![0u16; 32 * 1024];
                link.GetArguments(&mut buf)?;
                let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                removed.insert(String::from_utf16_lossy(&buf[..len]));
            }
            let exe = std::env::current_exe().map_err(|e| windows::core::Error::new(E_FAIL, e.to_string()))?;
            let links: IObjectCollection = CoCreateInstance(&EnumerableObjectCollection, None, CLSCTX_INPROC_SERVER)?;
            let mut added = 0;
            for item in items {
                let arg = quote_arg(&item.path.to_string_lossy());
                if added >= slots as usize || removed.contains(&arg) {
                    continue;
                }
                links.AddObject(&shell_link(&exe, &arg, item)?)?;
                added += 1;
            }
            if added > 0 {
                list.AppendCategory(
                    &HSTRING::from(t("Letti di recente", "Recently read")),
                    &links.cast::<IObjectArray>()?,
                )?;
            }
            list.CommitList()
        }
    }

    /// Un collegamento a NicoReader che apre il volume. Il titolo che si
    /// vede e' una proprieta' del collegamento; il percorso fa da suggerimento.
    unsafe fn shell_link(exe: &Path, arg: &str, item: &Item) -> Result<IShellLinkW> {
        unsafe {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            link.SetPath(&HSTRING::from(exe))?;
            link.SetArguments(&HSTRING::from(arg))?;
            link.SetIconLocation(&HSTRING::from(exe), 0)?;
            // il suggerimento ha un limite (INFOTIPSIZE, 1024 caratteri)
            let tip: String = item.path.to_string_lossy().chars().take(1000).collect();
            link.SetDescription(&HSTRING::from(tip))?;
            // il titolo dev'essere una stringa VT_LPWSTR: quella che fa il
            // crate e' un BSTR, la si converte
            let mut title = PROPVARIANT::default();
            PropVariantChangeType(&mut title, &PROPVARIANT::from(item.title.as_str()), PVCHF_DEFAULT, VT_LPWSTR)?;
            let store: IPropertyStore = link.cast()?;
            store.SetValue(&PKEY_Title, &title)?;
            store.Commit()?;
            Ok(link)
        }
    }

    fn load(file: &Path) -> BTreeSet<String> {
        std::fs::read(file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    /// Le voci tolte si ricordano per sempre: sono poche, e chi ne toglie una
    /// non la vuole veder tornare al prossimo avvio.
    fn save(file: &Path, removed: &BTreeSet<String>) {
        let Ok(json) = serde_json::to_vec(removed) else { return };
        if let Err(e) = std::fs::write(file, json) {
            eprintln!("jump list: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argomenti_come_li_rilegge_windows() {
        assert_eq!(quote_arg(r"C:\Fumetti\Nebbia.cbz"), r"C:\Fumetti\Nebbia.cbz");
        assert_eq!(quote_arg(r"C:\Fumetti\Nebbia sul Porto v01.cbz"), r#""C:\Fumetti\Nebbia sul Porto v01.cbz""#);
        assert_eq!(quote_arg(r"C:\Una cartella\"), r#""C:\Una cartella\\""#, "la barra in fondo non chiude");
        assert_eq!(quote_arg(r#"a "b" c"#), r#""a \"b\" c""#);
        assert_eq!(quote_arg(r#"x\"y z"#), r#""x\\\"y z""#);
        assert_eq!(quote_arg(""), r#""""#);
        let remote = "http://casa:25600/opds/v1.2/books/0RV8/pages/{pageNumber}#6:Volume 1";
        assert_eq!(quote_arg(remote), format!("\"{remote}\""));
    }

    #[test]
    fn si_riscrive_solo_se_cambia() {
        let mut list = JumpList::new(None);
        let item = |n: usize| Item { path: PathBuf::from(format!("v{n}.cbz")), title: format!("Volume {n}") };
        list.update((0..20).map(item).collect());
        assert_eq!(list.last.as_ref().map(Vec::len), Some(MAX_ITEMS));
        let before = list.last.clone();
        list.update((0..12).map(item).collect());
        assert_eq!(list.last, before, "oltre le dieci voci non cambia niente");
    }
}
