#!/usr/bin/env bash
# Prova, su un Mac, che NicoReader.app apra i volumi che gli manda il Finder.
#
# Uso (nella cartella rust, dopo pacchetto-mac.sh):
#   ./prova-finder.sh
#
# "open -a NicoReader.app volume.cbz" manda lo stesso Apple Event del doppio
# clic nel Finder (application:openURLs:, vedi crates/fumetto/src/finder.rs).
# Due volumi: il primo apre il programma, il secondo arriva a programma gia'
# aperto. Poi lo si chiude come Cmd+Q, e nei progressi salvati devono esserci
# tutti e due. Serve una sessione grafica (i Mac di GitHub Actions ce l'hanno).
set -euo pipefail
cd "$(dirname "$0")"

app="$PWD/dist/NicoReader.app"
prova=$(mktemp -d)
dati="$prova/dati"
trap 'osascript -e "quit app \"NicoReader\"" >/dev/null 2>&1 || true; rm -rf "$prova"' EXIT

# due CBZ con una pagina (un PNG 8x8 grigio), fatti a mano: niente librerie
python3 - "$prova" <<'EOF'
import struct, sys, zipfile, zlib
def png(w, h):
    def chunk(tipo, dati):
        return struct.pack(">I", len(dati)) + tipo + dati + struct.pack(">I", zlib.crc32(tipo + dati))
    righe = b"".join(b"\x00" + b"\x80" * w for _ in range(h))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 0, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(righe)) + chunk(b"IEND", b""))
for nome in ("Primo", "Secondo"):
    with zipfile.ZipFile(f"{sys.argv[1]}/{nome}.cbz", "w") as z:
        z.writestr("001.png", png(8, 8))
EOF

open -n -a "$app" --env FUMETTO_DATI="$dati" --stderr "$prova/errori.txt" "$prova/Primo.cbz"
sleep 15
open -a "$app" "$prova/Secondo.cbz"
sleep 5
# Cmd+Q: AppKit chiude il programma senza tornare da run_app
osascript -e 'quit app "NicoReader"'
for _ in $(seq 20); do pgrep -x nicoreader >/dev/null || break; sleep 1; done

echo "--- quel che ha scritto NicoReader"
cat "$prova/errori.txt" 2>/dev/null || true
echo "--- progressi.json"
cat "$dati/progressi.json" 2>/dev/null || echo "(non c'e')"
ok=1
for nome in Primo Secondo; do
    if grep -q "$nome.cbz" "$dati/progressi.json" 2>/dev/null; then
        echo "$nome.cbz: aperto"
    else
        echo "$nome.cbz: NON aperto"
        ok=0
    fi
done
(( ok )) || { echo "NicoReader non ha aperto i volumi del Finder"; exit 1; }
echo "Fatto: i volumi del Finder si aprono"
