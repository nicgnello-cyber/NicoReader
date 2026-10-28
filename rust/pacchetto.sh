#!/usr/bin/env bash
# Il pacchetto da distribuire di NicoReader (versione Rust), per Linux.
#
# Uso (nella cartella rust):
#   ./pacchetto.sh [--senza-prove]
#
# Serve: Rust (rustup), nasm (su x86_64), curl e tar.
#
# Fa, in ordine:
#   1. pdfium per questo sistema in vendor/pdfium/linux-<arch>, se manca
#      (dal pacchetto di pdfium-binaries della stessa versione della DLL di
#      Windows, controllato con SHA-256)
#   2. le prove (cargo test --workspace), se non si dice --senza-prove
#   3. l'eseguibile (cargo build --release)
#   4. dist/NicoReader/: nicoreader, libpdfium.so (per i PDF), icona.png,
#      nicoreader.desktop e installa.sh (per il menu delle applicazioni) e la
#      cartella licenze/ con le licenze di tutto cio' che sta dentro
#   5. dist/NicoReader-<versione>-linux-<arch>.tar.gz, da mettere nelle Release
#   6. dist/NicoReader-<versione>-linux-<arch>.AppImage, un file solo che gira
#      su qualunque distribuzione (con appimagetool, scaricato la prima volta
#      in vendor/appimagetool e controllato con SHA-256)
#   7. dist/nicoreader_<versione>_<arch>.deb, il pacchetto per Debian, Ubuntu,
#      Mint e simili (si installa con un doppio clic o con apt), se c'e'
#      dpkg-deb: NicoReader in /opt/nicoreader, il comando nicoreader, il menu e
#      l'icona per tutti gli utenti
#
# L'ingranditore AI non c'e': lo scarica chi lo vuole, al primo uso.
set -euo pipefail
cd "$(dirname "$0")"

prove=1
[[ "${1:-}" == "--senza-prove" ]] && prove=0

case "$(uname -m)" in
    x86_64) arch=x64; deb=amd64; triple=x86_64-unknown-linux-gnu
            sha=0b43f405477cf2cfc4dbff06905093c3309756c6bca1fb9da99234a2ca97fed2
            appimage_arch=x86_64; appimage_sha=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0 ;;
    aarch64|arm64) arch=arm64; deb=arm64; triple=aarch64-unknown-linux-gnu
            sha=0e6f90dccbc6b81fd5d7106abaf164c4222178f024c204d00d526b60fd2ad535
            appimage_arch=aarch64; appimage_sha=f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158 ;;
    *) echo "sistema non previsto: $(uname -m)" >&2; exit 1 ;;
esac

# --- pdfium: la stessa versione di vendor/pdfium/win-x64 (VERSION) ---
pdfium="vendor/pdfium/linux-$arch"
if [[ ! -f "$pdfium/bin/libpdfium.so" ]]; then
    build=$(sed -n 's/^BUILD=//p' vendor/pdfium/win-x64/VERSION)
    tgz=$(mktemp)
    trap 'rm -f "$tgz"' EXIT
    curl --location --fail --silent --show-error --output "$tgz" \
        "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F$build/pdfium-linux-$arch.tgz"
    echo "$sha  $tgz" | sha256sum --check --quiet
    rm -rf "$pdfium"
    mkdir -p "$pdfium/bin"
    tar -xzf "$tgz" -C "$pdfium" LICENSE VERSION licenses lib/libpdfium.so
    # come per Windows: la libreria in bin/, dove la cerca pdf.rs
    mv "$pdfium/lib/libpdfium.so" "$pdfium/bin/"
    rmdir "$pdfium/lib"
    rm -f "$tgz"
    trap - EXIT
fi

if (( prove )); then
    cargo test --workspace --quiet
fi
cargo build --release -p fumetto

versione=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
dist=dist/NicoReader
rm -rf "$dist"
mkdir -p "$dist/licenze/pdfium"

cp target/release/fumetto "$dist/nicoreader"
cp "$pdfium/bin/libpdfium.so" "$dist/"
cp crates/fumetto/risorse/icona.png "$dist/"
# il menu delle applicazioni: ./installa.sh nella cartella scompattata
cp crates/fumetto/risorse/linux/nicoreader.desktop "$dist/"
install -m 755 crates/fumetto/risorse/linux/installa.sh "$dist/"
cp "$pdfium/LICENSE" "$dist/licenze/pdfium/LICENSE.txt"
cp "$pdfium"/licenses/* "$dist/licenze/pdfium/"
cp crates/fumetto-render/caratteri/OFL-*.txt "$dist/licenze/"
# i pesi di Real-ESRGAN, dentro l'eseguibile
cp crates/fumetto-render/modelli/LICENSE-Real-ESRGAN.txt "$dist/licenze/"
# la licenza di NicoReader stesso
cp ../LICENSE-MIT "$dist/licenze/NicoReader-LICENSE-MIT.txt"
cp ../LICENSE-APACHE "$dist/licenze/NicoReader-LICENSE-APACHE.txt"

# --- le licenze delle librerie Rust (e di quelle in C e C++ che contengono) ---
librerie=$(cargo run -q -p fumetto-strumenti -- licenze "$versione" "$dist/licenze/terze-parti.txt" "$triple")

mb=$(du -sm "$dist" | cut -f1)
echo
echo "Fatto -> $dist/nicoreader  (~$mb MB, $librerie librerie nelle licenze)"

pacchetto="dist/NicoReader-$versione-linux-$arch.tar.gz"
rm -f "$pacchetto"
tar -czf "$pacchetto" -C dist NicoReader
echo "Pacchetto -> $pacchetto  ($(du -h "$pacchetto" | cut -f1) da scaricare)"

# --- l'AppImage ---
# appimagetool di una versione fissata; il runtime che apre l'AppImage e'
# quello dentro appimagetool stesso (anche lui e' un AppImage): niente altro da
# scaricare. Senza FUSE (container, CI) gli AppImage si estraggono da se'.
appimagetool="vendor/appimagetool/1.9.1/appimagetool-$appimage_arch.AppImage"
if [[ ! -x "$appimagetool" ]]; then
    mkdir -p "$(dirname "$appimagetool")"
    curl --location --fail --silent --show-error --output "$appimagetool.parziale" \
        "https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-$appimage_arch.AppImage"
    echo "$appimage_sha  $appimagetool.parziale" | sha256sum --check --quiet
    chmod +x "$appimagetool.parziale"
    mv "$appimagetool.parziale" "$appimagetool"
fi
export APPIMAGE_EXTRACT_AND_RUN=1
appdir=$(mktemp -d)
trap 'rm -rf "$appdir" "$appdir.runtime"' EXIT
mkdir -p "$appdir/usr/bin"
# l'eseguibile e pdfium insieme: NicoReader lo cerca accanto a se'
cp "$dist/nicoreader" "$dist/libpdfium.so" "$appdir/usr/bin/"
cp -r "$dist/licenze" "$appdir/"
ln -s usr/bin/nicoreader "$appdir/AppRun"
cp crates/fumetto/risorse/linux/nicoreader.desktop "$appdir/"
cp crates/fumetto/risorse/icona.png "$appdir/nicoreader.png"
ln -s nicoreader.png "$appdir/.DirIcon"
runtime="$appdir.runtime"
offset=$("$appimagetool" --appimage-offset)
head -c "$offset" "$appimagetool" > "$runtime"
pacchetto_appimage="dist/NicoReader-$versione-linux-$arch.AppImage"
rm -f "$pacchetto_appimage"
ARCH=$appimage_arch "$appimagetool" --no-appstream --runtime-file "$runtime" "$appdir" "$pacchetto_appimage" > /dev/null
rm -rf "$appdir" "$runtime"
trap - EXIT
echo "AppImage -> $pacchetto_appimage  ($(du -h "$pacchetto_appimage" | cut -f1) da scaricare)"

# --- il .deb ---
if ! command -v dpkg-deb > /dev/null; then
    echo "Pacchetto .deb: manca dpkg-deb, resta solo il tar.gz"
    exit 0
fi
radice=$(mktemp -d)
trap 'rm -rf "$radice"' EXIT
mkdir -p "$radice/DEBIAN" "$radice/opt" "$radice/usr/bin" "$radice/usr/share/applications" \
    "$radice/usr/share/icons/hicolor/256x256/apps" "$radice/usr/share/doc/nicoreader"
cp -r "$dist" "$radice/opt/nicoreader"
# nel .deb il menu e l'icona li mette il pacchetto: installa.sh non serve
rm "$radice/opt/nicoreader/installa.sh" "$radice/opt/nicoreader/nicoreader.desktop" "$radice/opt/nicoreader/icona.png"
# il comando nel PATH; pdfium NicoReader lo trova lo stesso, accanto al file vero
ln -s /opt/nicoreader/nicoreader "$radice/usr/bin/nicoreader"
cp crates/fumetto/risorse/linux/nicoreader.desktop "$radice/usr/share/applications/"
cp crates/fumetto/risorse/icona.png "$radice/usr/share/icons/hicolor/256x256/apps/nicoreader.png"
cat > "$radice/usr/share/doc/nicoreader/copyright" <<FINE
NicoReader $versione
https://github.com/nicgnello-cyber/NicoReader

NicoReader: MIT o Apache-2.0, a scelta (/opt/nicoreader/licenze/NicoReader-LICENSE-*).
Le licenze delle librerie contenute sono in /opt/nicoreader/licenze.
The licenses of the bundled libraries are in /opt/nicoreader/licenze.
FINE
find "$radice" -type d -exec chmod 755 {} +
find "$radice" -type f -exec chmod 644 {} +
chmod 755 "$radice/opt/nicoreader/nicoreader"
# la glibc che serve davvero: la versione piu' alta chiesta dai due binari
glibc=$(objdump -T "$dist/nicoreader" "$dist/libpdfium.so" | grep -oE 'GLIBC_[0-9.]+' | sed 's/GLIBC_//' | sort -Vu | tail -n 1)
cat > "$radice/DEBIAN/control" <<FINE
Package: nicoreader
Version: $versione
Architecture: $deb
Maintainer: nicgnello-cyber <290337305+nicgnello-cyber@users.noreply.github.com>
Installed-Size: $(du -sk --exclude=DEBIAN "$radice" | cut -f1)
Depends: libc6 (>= $glibc), libgcc-s1, libstdc++6, libxkbcommon0, libxkbcommon-x11-0
Recommends: libvulkan1, mesa-vulkan-drivers, curl, xdg-utils
Section: graphics
Priority: optional
Homepage: https://github.com/nicgnello-cyber/NicoReader
Description: comic, manga and webtoon reader
 NicoReader legge fumetti, manga e webtoon: cartelle, CBZ, CBR, CB7, CBT, PDF e EPUB,
 con pagine JPEG, PNG, WebP, GIF, BMP, AVIF e JPEG XL. Le pagine si
 rimpiccioliscono in luce lineare, si girano subito e si riaprono dove le si
 era lasciate. In italiano e in inglese.
FINE
pacchetto_deb="dist/nicoreader_${versione}_$deb.deb"
rm -f "$pacchetto_deb"
dpkg-deb --root-owner-group -Zxz --build "$radice" "$pacchetto_deb" > /dev/null
echo "Pacchetto .deb -> $pacchetto_deb  ($(du -h "$pacchetto_deb" | cut -f1) da scaricare)"
