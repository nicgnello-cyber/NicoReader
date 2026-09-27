"""Le licenze delle librerie Rust (e di quelle in C e C++ che contengono) che
finiscono nell'eseguibile, per pacchetto.sh e pacchetto-mac.sh.

    python3 terze_parti.py <versione> <file da scrivere> <triple> [<triple>...]

Come in pacchetto.ps1: per ogni pacchetto che finisce nell'eseguibile di quei
sistemi nome, versione, licenza dichiarata e i testi di licenza che porta con
se'. Le dipendenze solo per compilare o per le prove restano fuori. Piu' triple
insieme per un eseguibile universale (macOS: Intel e Apple Silicon). Stampa
quante librerie ha scritto.
"""
import json
import os
import re
import subprocess
import sys


def main():
    versione, uscita, triple = sys.argv[1], sys.argv[2], sys.argv[3:]
    comando = ["cargo", "metadata", "--format-version", "1"]
    for t in triple:
        comando += ["--filter-platform", t]
    meta = json.loads(subprocess.run(comando, check=True, capture_output=True).stdout)
    per_id = {p["id"]: p for p in meta["packages"]}
    nodi = {n["id"]: n for n in meta["resolve"]["nodes"]}
    radice = next(p["id"] for p in meta["packages"] if p["name"] == "fumetto")
    visti, coda = set(), [radice]
    while coda:
        i = coda.pop()
        if i in visti:
            continue
        visti.add(i)
        for d in nodi[i]["deps"]:
            # solo le dipendenze normali: niente build.rs ne' prove
            if any(k["kind"] is None for k in d["dep_kinds"]):
                coda.append(d["pkg"])
    pacchetti = sorted((per_id[i] for i in visti if per_id[i]["source"]), key=lambda p: (p["name"], p["version"]))
    nome = re.compile(r"^(LICEN[CS]E|COPYING|NOTICE|UNLICENSE|license)")
    riga = "=" * 78
    with open(uscita, "w", encoding="utf-8", newline="\n") as f:
        f.write("NicoReader %s contiene queste librerie. Qui sotto le loro licenze.\n" % versione)
        f.write("NicoReader contains these libraries. Their licenses follow.\n\n")
        for p in pacchetti:
            f.write("%s\n%s %s   (%s)\n" % (riga, p["name"], p["version"], p["license"]))
            if p["repository"]:
                f.write(p["repository"] + "\n")
            f.write(riga + "\n")
            cartella = os.path.dirname(p["manifest_path"])
            testi = []
            for base, dirs, files in os.walk(cartella):
                if base[len(cartella):].count(os.sep) >= 2:
                    dirs.clear()
                testi += [os.path.join(base, n) for n in files if nome.match(n)]
            for t in sorted(testi):
                f.write("--- %s\n" % os.path.relpath(t, cartella))
                with open(t, encoding="utf-8", errors="replace") as g:
                    f.write(g.read() + "\n")
            if not testi:
                f.write("(testo non incluso nel pacchetto: vale la licenza dichiarata)\n")
            f.write("\n")
    print(len(pacchetti))


if __name__ == "__main__":
    main()
