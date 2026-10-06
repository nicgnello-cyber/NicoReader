# NicoReader

*[English](README.md) · Italiano*

Un lettore di fumetti, manga e webtoon: la galleria nera, dove esiste solo la
tavola. Le pagine si rimpiccioliscono in luce lineare (i retini restano del
loro tono, senza moiré), si girano in un millisecondo e si riaprono dove le si
era lasciate.

Legge cartelle, CBZ/ZIP, CBR/RAR (anche solidi), CB7/7Z, CBT/TAR e PDF; pagine
JPEG, PNG, WebP, GIF, BMP, AVIF e JPEG XL. Ha la libreria con le serie
(prese dalla ComicInfo.xml quando il volume ce l'ha: i manga si aprono anche
da destra a sinistra da soli), miniature, segnalibri, doppia pagina, nastro per i webtoon (riconosciuti da
soli), zoom, lente, luminosità/contrasto/gamma, tasti personalizzabili, e un
ingrandimento AI facoltativo (Real-ESRGAN, sulla scheda video, senza niente
da scaricare). In
italiano e in inglese.

## Scarica

| Sistema | Installer |
|---|---|
| **Windows** (quasi tutti i PC) | [NicoReader-windows-x64-setup.exe](https://github.com/nicgnello-cyber/NicoReader/releases/latest/download/NicoReader-windows-x64-setup.exe) |
| **Windows su ARM** (Snapdragon X, Surface Pro 11) | [NicoReader-windows-arm64-setup.exe](https://github.com/nicgnello-cyber/NicoReader/releases/latest/download/NicoReader-windows-arm64-setup.exe) |
| **macOS** 10.15 o piu' recente, Intel e Apple Silicon | [NicoReader-macos.dmg](https://github.com/nicgnello-cyber/NicoReader/releases/latest/download/NicoReader-macos.dmg) |
| **Linux**: Ubuntu, Debian, Mint | [NicoReader-linux-x64.deb](https://github.com/nicgnello-cyber/NicoReader/releases/latest/download/NicoReader-linux-x64.deb) |
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
--rimuovi` lo toglie. Servono i driver Vulkan (Mesa, o quelli NVIDIA) e
libxkbcommon-x11, che ogni desktop ha gia'.

Il codice e' pronto anche per Linux ARM64, ma per lui non si fanno ancora
pacchetti.

## Usarlo

Si apre un volume dal menu (tasto destro), con Ctrl+O (Cmd+O sul Mac), dalla
libreria o con doppio clic sul file. Senza volume riprende l'ultimo letto.
Ctrl+, apre le impostazioni (anche i tasti).

Una volta al giorno NicoReader chiede a GitHub qual è l'ultima versione (non
manda nient'altro); se ne è uscita una nuova, propone di aprirne la pagina,
una volta per versione. Si spegne in Impostazioni → Lettura.

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
per Windows, il tar.gz e il .deb per Linux, il dmg per macOS):

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
GitHub (Releases -> Draft a new release -> tag `v0.2.0`, "create on publish"
-> Publish release) oppure con

    git tag v0.2.0
    git push origin v0.2.0

Actions fa i pacchetti di tutti i sistemi e li pubblica nella Release `v0.2.0`,
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
