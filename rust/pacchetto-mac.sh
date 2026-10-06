#!/usr/bin/env bash
# Il pacchetto da distribuire di NicoReader (versione Rust), per macOS.
#
# Uso (nella cartella rust, su un Mac):
#   ./pacchetto-mac.sh [--senza-prove]
#
# Serve: Rust (rustup) con i due sistemi,
#   rustup target add aarch64-apple-darwin x86_64-apple-darwin
# gli strumenti da riga di comando di Xcode (xcode-select --install), nasm
# (brew install nasm: per la parte Intel), curl e python3.
#
# Fa, in ordine:
#   1. pdfium per macOS (Intel e Apple Silicon insieme) in
#      vendor/pdfium/mac-univ, se manca (dal pacchetto di pdfium-binaries
#      della stessa versione della DLL di Windows, controllato con SHA-256)
#   2. le prove (cargo test --workspace) su questo Mac, se non si dice
#      --senza-prove
#   3. l'eseguibile per Apple Silicon e quello per Intel, uniti in uno
#      (lipo): un solo pacchetto per ogni Mac da macOS 10.15 in poi
#   4. dist/NicoReader.app: l'eseguibile, pdfium in Contents/Frameworks,
#      l'icona e le licenze in Contents/Resources/licenze, firmato "ad hoc"
#      (senza un account sviluppatore Apple: vedi il README)
#   5. dist/NicoReader-<versione>-macos.dmg, da mettere nelle Release
#
# L'ingranditore AI non c'e': lo scarica chi lo vuole, al primo uso.
set -euo pipefail
cd "$(dirname "$0")"

prove=1
[[ "${1:-}" == "--senza-prove" ]] && prove=0

# il Mac piu' vecchio che si accetta (Apple Silicon parte comunque da 11)
export MACOSX_DEPLOYMENT_TARGET=10.15
sistemi=(aarch64-apple-darwin x86_64-apple-darwin)

# --- pdfium: la stessa versione di vendor/pdfium/win-x64 (VERSION) ---
pdfium=vendor/pdfium/mac-univ
if [[ ! -f "$pdfium/bin/libpdfium.dylib" ]]; then
    build=$(sed -n 's/^BUILD=//p' vendor/pdfium/win-x64/VERSION)
    tgz=$(mktemp)
    trap 'rm -f "$tgz"' EXIT
    curl --location --fail --silent --show-error --output "$tgz" \
        "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F$build/pdfium-mac-univ.tgz"
    echo "a09650f24f0ed792fffa42324865ff25974cf16a287b0a2a17dd973d03a9eb77  $tgz" | shasum -a 256 --check --quiet
    rm -rf "$pdfium"
    mkdir -p "$pdfium/bin"
    tar -xzf "$tgz" -C "$pdfium" LICENSE VERSION licenses lib/libpdfium.dylib
    # come per Windows: la libreria in bin/, dove la cerca pdf.rs
    mv "$pdfium/lib/libpdfium.dylib" "$pdfium/bin/"
    rmdir "$pdfium/lib"
fi

if (( prove )); then
    cargo test --workspace --quiet
fi
for s in "${sistemi[@]}"; do
    cargo build --release -p fumetto --target "$s"
done

versione=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
app=dist/NicoReader.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Frameworks" "$app/Contents/Resources/licenze/pdfium"

lipo -create -output "$app/Contents/MacOS/nicoreader" \
    "target/aarch64-apple-darwin/release/fumetto" "target/x86_64-apple-darwin/release/fumetto"
cp "$pdfium/bin/libpdfium.dylib" "$app/Contents/Frameworks/"
sed "s/@VERSIONE@/$versione/g" crates/fumetto/risorse/macos/Info.plist > "$app/Contents/Info.plist"
cp crates/fumetto/risorse/macos/nicoreader.icns "$app/Contents/Resources/"
cp "$pdfium/LICENSE" "$app/Contents/Resources/licenze/pdfium/LICENSE.txt"
cp "$pdfium"/licenses/* "$app/Contents/Resources/licenze/pdfium/"
cp crates/fumetto-render/caratteri/OFL-*.txt "$app/Contents/Resources/licenze/"
# i pesi di Real-ESRGAN, dentro l'eseguibile
cp crates/fumetto-render/modelli/LICENSE-Real-ESRGAN.txt "$app/Contents/Resources/licenze/"
# la licenza di NicoReader stesso
cp ../LICENSE-MIT "$app/Contents/Resources/licenze/NicoReader-LICENSE-MIT.txt"
cp ../LICENSE-APACHE "$app/Contents/Resources/licenze/NicoReader-LICENSE-APACHE.txt"
librerie=$(python3 terze_parti.py "$versione" "$app/Contents/Resources/licenze/terze-parti.txt" "${sistemi[@]}")
plutil -lint "$app/Contents/Info.plist" > /dev/null

# Firma "ad hoc": senza, su Apple Silicon il programma non parte proprio.
# Prima la libreria, poi il pacchetto che la contiene.
codesign --force --sign - "$app/Contents/Frameworks/libpdfium.dylib"
codesign --force --sign - "$app"
codesign --verify --strict "$app"

mb=$(du -sm "$app" | cut -f1)
echo
echo "Fatto -> $app  (~$mb MB, $librerie librerie nelle licenze)"

# --- il dmg: NicoReader.app e il collegamento ad Applicazioni, da trascinare ---
dmg="dist/NicoReader-$versione-macos.dmg"
volume=$(mktemp -d)
cp -R "$app" "$volume/"
ln -s /Applications "$volume/Applications"
rm -f "$dmg"
hdiutil create -quiet -volname NicoReader -srcfolder "$volume" -fs HFS+ -format UDZO "$dmg"
rm -rf "$volume"
echo "Pacchetto -> $dmg  ($(du -h "$dmg" | cut -f1) da scaricare)"
