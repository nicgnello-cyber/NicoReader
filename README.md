# NicoReader

*English · [Italiano](README.it.md)*

A reader for comics, manga and webtoons: a black gallery where only the page
exists. Pages are downscaled in linear light (screentones keep their tone, no
moiré), turn in a millisecond, and reopen where you left off.

It reads folders, CBZ/ZIP, CBR/RAR (solid archives too), CB7/7Z, CBT/TAR and
PDF, with JPEG, PNG, WebP, GIF, BMP, AVIF and JPEG XL pages. It has a library
organized by series, thumbnails, bookmarks, two-page spreads, a vertical strip
for webtoons (detected automatically), zoom, a magnifier,
brightness/contrast/gamma, customizable keys, and optional AI upscaling
(Real-ESRGAN, downloaded on first use). The interface is in English and
Italian.

## Download

| System | Installer |
|---|---|
| **Windows** (most PCs) | [NicoReader-windows-x64-setup.exe](https://github.com/nicgnello-cyber/NicoReader/releases/latest/download/NicoReader-windows-x64-setup.exe) |
| **Windows on ARM** (Snapdragon X, Surface Pro 11) | [NicoReader-windows-arm64-setup.exe](https://github.com/nicgnello-cyber/NicoReader/releases/latest/download/NicoReader-windows-arm64-setup.exe) |
| **macOS** 10.15 or later, Intel and Apple Silicon | [NicoReader-macos.dmg](https://github.com/nicgnello-cyber/NicoReader/releases/latest/download/NicoReader-macos.dmg) |
| **Linux**: Ubuntu, Debian, Mint | [NicoReader-linux-x64.deb](https://github.com/nicgnello-cyber/NicoReader/releases/latest/download/NicoReader-linux-x64.deb) |
| **Linux**: other distributions | [NicoReader-linux-x64.tar.gz](https://github.com/nicgnello-cyber/NicoReader/releases/latest/download/NicoReader-linux-x64.tar.gz) |

To use it on Windows without installing anything:
[zip x64](https://github.com/nicgnello-cyber/NicoReader/releases/latest/download/NicoReader-windows-x64.zip),
[zip ARM64](https://github.com/nicgnello-cyber/NicoReader/releases/latest/download/NicoReader-windows-arm64.zip).
Earlier versions and what changed in each: [Releases](https://github.com/nicgnello-cyber/NicoReader/releases).

## Installing

**Windows.** Run the `-setup.exe`. It isn't signed, so the first time Windows
SmartScreen shows a warning: click "More info", then "Run anyway". It
installs for the current user without asking for administrator rights (you
can choose "for all users" instead). It adds a Start menu entry, a desktop
icon if you want one, double-click opening for CBZ, CBR, CB7 and CBT files
(only if no other program opens them already), and NicoReader under "Open
with" for PDFs too. Uninstall it from Settings → Apps.

**macOS.** Open the dmg and drag NicoReader into Applications. It isn't
signed with an Apple developer account, so macOS blocks it the first time:
open it from System Settings → Privacy & Security → "Open Anyway" (only
once), or from Terminal with
`xattr -dr com.apple.quarantine /Applications/NicoReader.app`. Open volumes
from within the app (Cmd+O, or the library); double-clicking files in the
Finder doesn't work yet.

**Linux.** Install the `.deb` with a double click or with
`sudo apt install ./NicoReader-linux-x64.deb`: NicoReader appears in the
application menu, and the `nicoreader` command is available. With the
tar.gz: extract it wherever you like, then `./installa.sh` adds NicoReader to
the menu and to the programs that open CBZ, CBR, CB7, CBT and PDF files (for
the current user only, no sudo needed); `./installa.sh --rimuovi` removes it
again. You need Vulkan drivers (Mesa or NVIDIA's) and libxkbcommon-x11, which
every desktop already has.

The code also builds for Linux ARM64, but there are no packages for it yet.

## Using it

Open a volume from the menu (right click), with Ctrl+O (Cmd+O on a Mac), from
the library, or by double-clicking the file. With no volume, it resumes the
last one you read. Ctrl+, opens the settings (keys included).

From the command line, after building it:

    rust\target\release\fumetto.exe [volume]      (Windows)
    rust/target/release/fumetto [volume]            (Linux, macOS)

## Building

In the `rust` folder, with Rust (rustup) and nasm (not needed on ARM64; on a
Mac you also need the Xcode command line tools, `xcode-select --install`):

    cargo build --release
    cargo test --workspace

On Linux and macOS the PDF tests need pdfium in `vendor/pdfium/`:
`pacchetto.sh` and `pacchetto-mac.sh` download it the first time they run.

The packages to distribute (executable, pdfium, licenses; the zip and the
installer for Windows, the tar.gz and the .deb for Linux, the dmg for macOS):

    powershell -ExecutionPolicy Bypass -File pacchetto.ps1               (Windows x64)
    powershell -ExecutionPolicy Bypass -File pacchetto.ps1 -Arch arm64   (Windows ARM64)
    ./pacchetto.sh                                                       (Linux)
    ./pacchetto-mac.sh                                                   (macOS)

The Windows installer needs Inno Setup 6 (`winget install
JRSoftware.InnoSetup`); without it, `pacchetto.ps1` says so and builds only
the zip. The .deb needs `dpkg-deb`, which Debian and Ubuntu have.

GitHub Actions builds the same packages (`.github/workflows/pacchetti.yml`):
manually from Actions → Pacchetti → Run workflow (choosing the systems), for
every `v*` tag, and for pull requests that touch the code.

To publish a new version, with the Download links following it
automatically: raise `version` in `rust/Cargo.toml`, merge, then create the
tag, either from the GitHub website (Releases → Draft a new release → tag
`v0.2.0`, create on publish → Publish release) or with

    git tag v0.2.0
    git push origin v0.2.0

Actions builds the packages for every system and publishes them in the
`v0.2.0` Release, under the version-less names the links use (it stops if the
tag doesn't match the version in Cargo.toml).

## License

NicoReader is licensed under either MIT or Apache-2.0, at your option
([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)). The libraries
it includes have their own licenses, collected in the `licenze` folder of
each package.

The program used to be called Fumetto: folders and modules in the code
(`rust/crates/fumetto`, ...) still carry that name, and the code comments and
the project notes are in Italian.

## Where the rest is

The project status, decisions, measurements and next steps are in
`LAVORO_E_PROSSIMI_PASSI.txt` (in Italian).
