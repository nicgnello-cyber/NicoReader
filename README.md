# NicoReader

*English · [Italiano](README.it.md)*

A reader for comics, manga and webtoons: a black gallery where only the page
exists. Pages are downscaled in linear light (screentones keep their tone, no
moiré), turn in a millisecond, and reopen where you left off.

It reads folders, CBZ/ZIP, CBR/RAR (solid archives too), CB7/7Z, CBT/TAR,
PDF and EPUB (the ones made of images: manga and comics), with JPEG, PNG, WebP, GIF, BMP, AVIF and JPEG XL pages. It has a library
organized by series (from ComicInfo.xml when a volume has one, so manga also
open right to left on their own), thumbnails, bookmarks, two-page spreads, a vertical strip
for webtoons (detected automatically), zoom, a magnifier,
brightness/contrast/gamma, customizable keys, and optional AI upscaling
(Real-ESRGAN, on the graphics card, nothing to download). The library can
also show the volumes of a Komga or Kavita server, read page by page
without downloading them. The interface is
in English and
Italian.

## Download

| System | Installer |
|---|---|
| **Windows** (most PCs) | [NicoReader-windows-x64-setup.exe](https://github.com/nicgnello-cyber/NicoReader/releases/latest/download/NicoReader-windows-x64-setup.exe) |
| **Windows on ARM** (Snapdragon X, Surface Pro 11) | [NicoReader-windows-arm64-setup.exe](https://github.com/nicgnello-cyber/NicoReader/releases/latest/download/NicoReader-windows-arm64-setup.exe) |
| **macOS** 10.15 or later, Intel and Apple Silicon | [NicoReader-macos.dmg](https://github.com/nicgnello-cyber/NicoReader/releases/latest/download/NicoReader-macos.dmg) |
| **Linux**: Ubuntu, Debian, Mint | [NicoReader-linux-x64.deb](https://github.com/nicgnello-cyber/NicoReader/releases/latest/download/NicoReader-linux-x64.deb) |
| **Linux**: any distribution, one file | [NicoReader-linux-x64.AppImage](https://github.com/nicgnello-cyber/NicoReader/releases/latest/download/NicoReader-linux-x64.AppImage) |
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
`xattr -dr com.apple.quarantine /Applications/NicoReader.app`. Volumes open
from the Finder (double-click on CBZ, CBR, CB7 and CBT files if no other app
already opens them, "Open With" for those and for PDFs, or drag them, or a
folder, onto the icon in the Dock), and from within the app (Cmd+O, or the
library).

**Linux.** Install the `.deb` with a double click or with
`sudo apt install ./NicoReader-linux-x64.deb`: NicoReader appears in the
application menu, and the `nicoreader` command is available. With the
tar.gz: extract it wherever you like, then `./installa.sh` adds NicoReader to
the menu and to the programs that open CBZ, CBR, CB7, CBT and PDF files (for
the current user only, no sudo needed); `./installa.sh --rimuovi` removes it
again. The AppImage needs no installing: make it executable (`chmod +x
NicoReader-linux-x64.AppImage`, or Properties → Allow executing) and open it.
You need Vulkan drivers (Mesa or NVIDIA's) and libxkbcommon-x11, which
every desktop already has.

The code also builds for Linux ARM64, but there are no packages for it yet.

## Using it

Open a volume from the menu (right click), with Ctrl+O (Cmd+O on a Mac), from
the library, or by double-clicking the file. With no volume, it resumes the
last one you read. Ctrl+, opens the settings (keys included). On Windows the
volumes you read last are also in the jump list: right click the NicoReader
icon on the taskbar.

At the last page, going forward once more tells you which volume comes
next, and going forward again opens it; at the first page, going back twice
opens the previous one, at its last page. Which one depends on where you
opened it from: the next in the series (by number), in the reading list, or,
in the folder view, the next on disk, moving on to the next folder when one
is finished. Both are also in the right-click menu, under Previous volume and
Next volume.

On a touch screen, tap the sides to turn pages or swipe; drag to move a
zoomed page or to scroll the strip, the library and the thumbnails; pinch
with two fingers to zoom (in the strip it changes the width); double-tap the
middle to zoom in or back out; hold a finger down for the menu. On a Mac
trackpad, pinch to zoom.

If a file name is messy and the volume has no ComicInfo.xml, right-click it
in the library → Edit details… to set its title, series, number and reading
direction. The corrections live in `correzioni.json` in the data folder, so
the files stay untouched (server volumes too); "Back to the file's details"
in the same menu undoes them.

The tree icon in the library bar shows the library by folder, as on disk:
subfolders first (with the cover of the first volume inside), then the
volumes, in natural order (2 before 10), with the path at the top to go back
up (Backspace or Esc too). Folders full of different series, like an
author's, are also collections. NicoReader remembers the view and the last
folder you were in.

The shelves icon in the library bar shows the collections, reading lists,
genres and publishers: from ComicInfo.xml (SeriesGroup, StoryArc with its
number, Genre, Publisher), from EPUB metadata, and from a Komga or Kavita
server (its collections and reading lists). A reading list opens in its own
order, even across different series.

To read from a Komga or Kavita server: right click → Add a server… (or the
folder icon in the library). For Komga, the server address with your user
name and password; for Kavita, the OPDS link from your user settings (it
contains the key, so no password is needed). The server enters the library
only if it answers; its series and volumes appear next to the local ones,
and pages are fetched one at a time (OPDS page streaming), so a volume opens
right away. Typing in the library also searches the servers, after a short
pause: what the library doesn't have (a server too big to read whole, or a
match in a summary or a tag) appears below, under "On the server".

The reading position is shared with the server: read a chapter on your
phone with Komga's or Kavita's app and NicoReader picks up from there, and
the other way round. To read away from home, right click a volume →
Download to read offline: the library marks it "downloaded", and it opens
even when the server can't be reached (the library keeps showing the
server's volumes as they were last time). What you read offline reaches the
server the next time it answers.

On Windows and macOS the password goes to the system keychain (Credential
Manager, Keychain), not to the settings file; on Linux it stays in
`impostazioni.json`, readable only by you. Either way it is handed to curl
on its standard input, never on the command line.

A password-protected PDF asks for its password when you open it. On
Windows and macOS NicoReader keeps it in the system keychain and doesn't
ask again; on Linux it asks at each launch.

At each launch NicoReader asks GitHub which version is the latest (nothing
else is sent); when a newer one is out, it asks once per version: Update, What's
new (the Release page) or Later. Update downloads the new version, checks it
against the SHA-256 that GitHub publishes, closes, installs it and reopens
where you were. It works with the Windows installer, on macOS, with the
AppImage and with the Linux tar.gz folder; with the Windows zip or the .deb
it opens the Release page instead. Turn the check off in Settings → Reading.

From the command line, after building it:

    rust\target\release\fumetto.exe [volume]      (Windows)
    rust/target/release/fumetto [volume]            (Linux, macOS)

## Building

In the `rust` folder, with Rust (rustup) and nasm (not needed on ARM64; on a
Mac you also need the Xcode command line tools, `xcode-select --install`):

    cargo build --release
    cargo test --workspace

Before a pull request, `cargo fmt --all` and
`cargo clippy --workspace --all-targets -- -D warnings`: GitHub Actions
checks both.

On Linux and macOS the PDF tests need pdfium in `vendor/pdfium/`:
`pacchetto.sh` and `pacchetto-mac.sh` download it the first time they run.

The packages to distribute (executable, pdfium, licenses; the zip and the
installer for Windows, the AppImage, the tar.gz and the .deb for Linux, the dmg
for macOS):

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
`v0.4.3`, create on publish → Publish release) or with

    git tag v0.4.3
    git push origin v0.4.3

Actions builds the packages for every system and publishes them in the
`v0.4.3` Release, under the version-less names the links use (it stops if the
tag doesn't match the version in Cargo.toml), with the notes from
`.github/note-release.md` if the Release has none. To rewrite only the notes
of the current version: Run workflow with "note" checked.

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
