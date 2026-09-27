#!/bin/sh
# Mette NicoReader nel menu delle applicazioni, solo per questo utente (niente
# sudo): nicoreader.desktop in ~/.local/share/applications, con il percorso di
# questa cartella, e l'icona. NicoReader resta dov'e': se si sposta la cartella,
# si rilancia lo script.
#
#   ./installa.sh             nel menu, e tra i programmi per aprire CBZ, CBR, CB7, CBT e PDF
#   ./installa.sh --rimuovi   toglie menu e icona (la cartella resta)
set -eu

qui=$(cd "$(dirname "$0")" && pwd)
dati=${XDG_DATA_HOME:-$HOME/.local/share}
voce="$dati/applications/nicoreader.desktop"
icona="$dati/icons/hicolor/256x256/apps/nicoreader.png"

aggiorna() {
    update-desktop-database "$dati/applications" 2>/dev/null || true
    gtk-update-icon-cache --quiet "$dati/icons/hicolor" 2>/dev/null || true
}

if [ "${1:-}" = "--rimuovi" ]; then
    rm -f "$voce" "$icona"
    aggiorna
    echo "NicoReader tolto dal menu."
    exit 0
fi

if [ ! -x "$qui/nicoreader" ]; then
    echo "nicoreader non c'e' in $qui" >&2
    exit 1
fi

# Exec vuole il percorso tra virgolette, con \ " ` $ preceduti da \; e poi,
# come ogni valore del file, ogni \ raddoppiato
percorso=$(printf '%s' "$qui/nicoreader" | sed -e 's/[\\"`$]/\\&/g' -e 's/\\/\\\\/g')

mkdir -p "$dati/applications" "$(dirname "$icona")"
cp "$qui/icona.png" "$icona"
EXEC="\"$percorso\" %f" awk '/^Exec=/ { print "Exec=" ENVIRON["EXEC"]; next } { print }' \
    "$qui/nicoreader.desktop" > "$voce"
aggiorna
echo "NicoReader e' nel menu delle applicazioni."
