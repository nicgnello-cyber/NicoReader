# NicoReader

*[English](README.md) · Italiano*

Un lettore di fumetti, manga e webtoon: la galleria nera, dove esiste solo la
tavola. Le pagine si rimpiccioliscono in luce lineare (i retini restano del
loro tono, senza moiré), si girano in un millisecondo e si riaprono dove le si
era lasciate.

Legge cartelle, CBZ/ZIP, CBR/RAR (anche solidi), CB7/7Z, CBT/TAR, PDF ed EPUB
(quelli fatti di immagini: manga e fumetti); pagine
JPEG, PNG, WebP, GIF, BMP, AVIF e JPEG XL. Ha la libreria con le serie
(prese dalla ComicInfo.xml quando il volume ce l'ha: i manga si aprono anche
da destra a sinistra da soli), miniature, segnalibri, doppia pagina, nastro per i webtoon (riconosciuti da
soli), zoom, lente, luminosità/contrasto/gamma, tasti personalizzabili, e un
ingrandimento AI facoltativo (Real-ESRGAN, sulla scheda video, senza niente
da scaricare). La libreria può mostrare anche i volumi di un server Komga o
Kavita, letti pagina per pagina senza scaricarli. In
italiano e in inglese.

## Scarica

| Sistema | Installer |
|---|---|
| **Windows** (quasi tutti i PC) | [NicoReader-windows-x64-setup.exe](https://github.com/nicgnello-cyber/NicoReader/releases/latest/download/NicoReader-windows-x64-setup.exe) |
| **Windows su ARM** (Snapdragon X, Surface Pro 11) | [NicoReader-windows-arm64-setup.exe](https://github.com/nicgnello-cyber/NicoReader/releases/latest/download/NicoReader-windows-arm64-setup.exe) |
| **macOS** 10.15 o piu' recente, Intel e Apple Silicon | [NicoReader-macos.dmg](https://github.com/nicgnello-cyber/NicoReader/releases/latest/download/NicoReader-macos.dmg) |
| **Linux**: Ubuntu, Debian, Mint | [NicoReader-linux-x64.deb](https://github.com/nicgnello-cyber/NicoReader/releases/latest/download/NicoReader-linux-x64.deb) |
| **Linux**: qualsiasi distribuzione, un file solo | [NicoReader-linux-x64.AppImage](https://github.com/nicgnello-cyber/NicoReader/releases/latest/download/NicoReader-linux-x64.AppImage) |
| **Linux**: le altre distribuzioni | [NicoReader-linux-x64.tar.gz](https://github.com/nicgnello-cyber/NicoReader/releases/latest/download/NicoReader-linux-x64.tar.gz) |

Senza installare niente, su Windows: [zip x64](https://github.com/nicgnello-cyber/NicoReader/releases/latest/download/NicoReader-windows-x64.zip),
[zip ARM64](https://github.com/nicgnello-cyber/NicoReader/releases/latest/download/NicoReader-windows-arm64.zip). Le versioni precedenti e le
novita' di ognuna: [Releases](https://github.com/nicgnello-cyber/NicoReader/releases).

## Installarlo

**Windows.** Si apre il `-setup.exe`. Non e' firmato, quindi la prima volta
Windows SmartScreen avvisa: "Ulteriori informazioni" -> "Esegui comunque".
Installa per l'utente, senza chiedere di essere amministratore (si puo'
scegliere "per tutti"): menu Start, icona sul desktop se la si vuole, doppio
clic sui CBZ, CBR, CB7 e CBT (solo se nessun altro programma li apre gia'),
NicoReader in "Apri con" anche per i PDF. Si disinstalla da Impostazioni -> App.

**macOS.** Si apre il dmg e si trascina NicoReader in Applicazioni. Non e'
firmato con un account sviluppatore Apple, quindi la prima volta macOS lo
blocca: si apre da Impostazioni di Sistema -> Privacy e sicurezza -> "Apri
comunque" (una volta sola), oppure da Terminale con
`xattr -dr com.apple.quarantine /Applications/NicoReader.app`. I volumi si
aprono dal Finder (doppio clic su CBZ, CBR, CB7 e CBT se nessun'altra app li
apre gia', "Apri con" per questi e per i PDF, o trascinandoli, anche una
cartella, sull'icona nel Dock) e dall'app (Cmd+O, la libreria).

**Linux.** Il `.deb` si installa con un doppio clic o con
`sudo apt install ./NicoReader-linux-x64.deb`: NicoReader finisce nel menu delle
applicazioni e c'e' il comando `nicoreader`. Con il tar.gz: si scompatta dove
si vuole e `./installa.sh` mette NicoReader nel menu e tra i programmi per aprire
CBZ, CBR, CB7, CBT e PDF (solo per l'utente, senza sudo); `./installa.sh
--rimuovi` lo toglie. L'AppImage non s'installa: lo si rende eseguibile
(`chmod +x NicoReader-linux-x64.AppImage`, o Proprieta' → Consenti
l'esecuzione) e lo si apre. Servono i driver Vulkan (Mesa, o quelli NVIDIA) e
libxkbcommon-x11, che ogni desktop ha gia'.

Il codice e' pronto anche per Linux ARM64, ma per lui non si fanno ancora
pacchetti.

## Usarlo

Si apre un volume dal menu (tasto destro), con Ctrl+O (Cmd+O sul Mac), dalla
libreria o con doppio clic sul file. Senza volume riprende l'ultimo letto.
Ctrl+, apre le impostazioni (anche i tasti). Su Windows gli ultimi volumi
letti sono anche nella jump list: tasto destro sull'icona di NicoReader nella
barra delle applicazioni.

All'ultima pagina, andando avanti ancora una volta l'app dice quale volume
viene dopo, e andando avanti di nuovo lo apre; alla prima pagina, andando
indietro due volte si apre il precedente, dalla sua ultima pagina. Quale
dipende da dove lo si è aperto: il seguente della serie (per numero), della
lista di lettura o, nella vista per cartelle, il seguente su disco, passando
alla cartella dopo quando una è finita. Ci sono anche nel menu del tasto
destro, sotto Volume precedente e Volume successivo.

Su uno schermo touch si tocca ai lati per girare pagina, o si striscia; si
trascina per spostare una pagina ingrandita o per scorrere il nastro, la
libreria e le miniature; si allargano due dita per ingrandire (nel nastro
cambia la larghezza); due tocchi al centro ingrandiscono o tornano alla
pagina intera; un dito tenuto giù apre il menu. Sul trackpad del Mac si
ingrandisce con il pizzico.

Se il nome di un file è disordinato e manca la ComicInfo.xml, tasto destro
sul volume nella libreria → Modifica i dati… per dargli titolo, serie,
numero e verso di lettura. Le correzioni stanno in `correzioni.json` nella
cartella dei dati: i file non si toccano (nemmeno quelli dei server); "Torna
ai dati del file", nello stesso menu, le toglie.

L'icona ad albero nella barra della libreria mostra la libreria per
cartelle, come su disco: prima le sottocartelle (con la copertina del primo
volume che contengono), poi i volumi, in ordine naturale (2 prima di 10), e
in alto il percorso per risalire (anche con Backspace o Esc). Le cartelle con
dentro serie diverse, come quelle di un autore, sono anche raccolte.
NicoReader ricorda la vista e l'ultima cartella aperta.

L'icona degli scaffali nella barra della libreria mostra raccolte, liste di
lettura, generi ed editori: dalla ComicInfo.xml (SeriesGroup, StoryArc con il
suo numero, Genre, Publisher), dai dati degli EPUB e da un server Komga o
Kavita (le sue raccolte e liste di lettura). Una lista di lettura si apre nel
suo ordine, anche fra serie diverse.

Per leggere da un server Komga o Kavita: tasto destro → Aggiungi un server…
(o l'icona della cartella nella libreria). Per Komga l'indirizzo del server,
con nome e password; per Kavita il link OPDS che c'è nelle impostazioni
dell'utente (dentro c'è la chiave, la password non serve). Il server entra
nella libreria solo se risponde; serie e volumi compaiono accanto a quelli
dei file, e le pagine si chiedono una alla volta (lo "streaming" delle
pagine di OPDS): il volume si apre subito. Scrivendo nella libreria si cerca
anche sui server, dopo una breve pausa: quello che la libreria non ha (un
server troppo grande per leggerlo tutto, o una parola trovata in un riassunto
o in un'etichetta) compare sotto, in "Sul server".

Il punto di lettura è lo stesso del server: si legge un capitolo sul
telefono con l'app di Komga o Kavita e NicoReader riparte da lì, e
viceversa. Per leggere fuori casa: tasto destro su un volume → Scarica per
leggere senza rete. Nella libreria compare "scaricato", e si apre anche se
il server non si raggiunge (la libreria continua a mostrare i volumi del
server com'erano l'ultima volta). Quello che si legge senza rete arriva al
server appena risponde.

Su Windows e macOS la password va nel portachiavi del sistema (Gestione
credenziali, Portachiavi), non nel file delle impostazioni; su Linux resta
in `impostazioni.json`, leggibile solo da te. In ogni caso curl la riceve
dallo standard input, mai dalla riga di comando.

Un PDF protetto da password la chiede quando lo si apre. Su Windows e
macOS NicoReader la tiene nel portachiavi del sistema e non la richiede; su
Linux la chiede a ogni avvio.

A ogni avvio NicoReader chiede a GitHub qual è l'ultima versione (non manda
nient'altro); se ne è uscita una nuova chiede, una volta per versione:
Aggiorna, Novità (la pagina della Release) o Più tardi. Aggiorna scarica la
versione nuova, la controlla con l'impronta SHA-256 che pubblica GitHub, si
chiude, la installa e si riapre dove eri. Funziona con l'installer di
Windows, su macOS, con l'AppImage e con la cartella del tar.gz di Linux;
con lo zip di Windows o il .deb apre invece la pagina della Release. Il
controllo si spegne in Impostazioni → Lettura.

Da riga di comando, dopo averlo compilato:

    rust\target\release\fumetto.exe [volume]      (Windows)
    rust/target/release/fumetto [volume]            (Linux, macOS)

## Compilarlo

Nella cartella `rust`, con Rust (rustup) e nasm (su ARM64 nasm non serve;
sul Mac servono anche gli strumenti di Xcode, `xcode-select --install`):

    cargo build --release
    cargo test --workspace

Prima di una pull request, `cargo fmt --all` e
`cargo clippy --workspace --all-targets -- -D warnings`: GitHub Actions li
controlla tutti e due.

Su Linux e macOS le prove dei PDF vogliono pdfium in `vendor/pdfium/`: lo
scaricano `pacchetto.sh` e `pacchetto-mac.sh` la prima volta.

I pacchetti da distribuire (eseguibile, pdfium, licenze; lo zip e l'installer
per Windows, l'AppImage, il tar.gz e il .deb per Linux, il dmg per macOS):

    powershell -ExecutionPolicy Bypass -File pacchetto.ps1               (Windows x64)
    powershell -ExecutionPolicy Bypass -File pacchetto.ps1 -Arch arm64   (Windows ARM64)
    ./pacchetto.sh                                                       (Linux)
    ./pacchetto-mac.sh                                                   (macOS)

L'installer per Windows vuole Inno Setup 6 (`winget install
JRSoftware.InnoSetup`); senza, `pacchetto.ps1` lo dice e fa solo lo zip. Il
.deb vuole `dpkg-deb`, che c'e' su Debian e Ubuntu.

Gli stessi li fa GitHub Actions (`.github/workflows/pacchetti.yml`): a mano da
Actions -> Pacchetti -> Run workflow (scegliendo i sistemi), per ogni tag `v*`,
e per le pull request che toccano il codice.

Una nuova versione, con i link di "Scarica" che la seguono da soli: si alza
`version` in `rust/Cargo.toml`, e dopo il merge si crea il tag, dal sito di
GitHub (Releases -> Draft a new release -> tag `v0.4.4`, "create on publish"
-> Publish release) oppure con

    git tag v0.4.4
    git push origin v0.4.4

Actions fa i pacchetti di tutti i sistemi e li pubblica nella Release `v0.4.4`,
con i nomi senza versione dei link (se il tag non e' la versione di
Cargo.toml si ferma), e con le note di `.github/note-release.md` se la
Release non ne ha. Per riscrivere solo le note della versione attuale: Run
workflow con "note" spuntato.

## Licenza

NicoReader e' distribuito con licenza MIT o Apache-2.0, a scelta
([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)). Le librerie
che contiene hanno le loro licenze, raccolte nei pacchetti nella cartella
`licenze`.

Il programma prima si chiamava Fumetto: nel codice le cartelle e i moduli
(`rust/crates/fumetto`, ...) hanno ancora quel nome.

## Dove sta il resto

Lo stato del progetto, le decisioni, le misure e i prossimi passi sono in
`LAVORO_E_PROSSIMI_PASSI.txt`.
