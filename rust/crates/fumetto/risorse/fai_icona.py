"""Rifa fumetto.ico (Windows) e macos/nicoreader.icns (macOS) da icona.png
(256x256, qui accanto).

Una misura per ogni uso di Windows (barra del titolo, taskbar, Esplora file,
a ogni scala dello schermo), rimpicciolite come le pagine: in luce lineare,
con l'alfa premoltiplicato, cosi' i bordi non si scuriscono. Ogni misura e'
un PNG dentro l'ICO: 30 KB invece dei 270 del vecchio file.

Per macOS lo stesso disegno, ma piu' piccolo nel suo riquadro: la griglia
di Apple vuole il corpo dell'icona a 824 px su 1024, e icona.png lo ha a 236
su 256; senza il margine, nel Dock sembrerebbe piu' grande delle altre. Le
misure arrivano a 256 (128@2x): oltre servirebbe un disegno piu' grande.

    python fai_icona.py
"""
import io
import struct
from pathlib import Path

import numpy as np
from PIL import Image

QUI = Path(__file__).parent
MISURE = [16, 20, 24, 32, 40, 48, 64, 256]
# macOS: il tipo di ogni blocco dell'ICNS e il suo lato in pixel
MISURE_MAC = [("icp4", 16), ("icp5", 32), ("ic11", 32), ("icp6", 64), ("ic12", 64),
              ("ic07", 128), ("ic08", 256), ("ic13", 256)]
# il lato del disegno nel riquadro da 256: il corpo, 236 px in icona.png,
# diventa 824/1024 del riquadro
LATO_MAC = 224


def lineare(c):
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def srgb(c):
    c = np.clip(c, 0.0, 1.0)
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * c ** (1 / 2.4) - 0.055)


def rimpicciolisci(rgba, lato):
    a = rgba[..., 3:4]
    piani = np.concatenate([lineare(rgba[..., :3]) * a, a], axis=2)
    out = np.stack([np.asarray(Image.fromarray(piani[..., k].astype(np.float32), "F")
                               .resize((lato, lato), Image.LANCZOS)) for k in range(4)], axis=2)
    a = np.clip(out[..., 3:4], 0.0, 1.0)
    rgb = np.where(a > 1e-6, out[..., :3] / np.maximum(a, 1e-6), 0.0)
    return np.round(np.concatenate([srgb(rgb), a], axis=2) * 255).astype(np.uint8)


def in_png(rgba, lato):
    px = rgba if lato == rgba.shape[0] else rimpicciolisci(rgba, lato)
    buf = io.BytesIO()
    Image.fromarray((px * 255).round().astype(np.uint8) if px.dtype != np.uint8 else px, "RGBA") \
        .save(buf, "PNG", optimize=True)
    return buf.getvalue()


def icns(rgba):
    lato = rgba.shape[0]
    riquadro = np.zeros_like(rgba)
    bordo = (lato - LATO_MAC) // 2
    riquadro[bordo:bordo + LATO_MAC, bordo:bordo + LATO_MAC] = rimpicciolisci(rgba, LATO_MAC) / 255
    blocchi = b""
    for tipo, lato in MISURE_MAC:
        dati = in_png(riquadro, lato)
        blocchi += tipo.encode("ascii") + struct.pack(">I", 8 + len(dati)) + dati
    (QUI / "macos" / "nicoreader.icns").write_bytes(b"icns" + struct.pack(">I", 8 + len(blocchi)) + blocchi)


def main():
    rgba = np.asarray(Image.open(QUI / "icona.png").convert("RGBA")).astype(np.float64) / 255
    immagini = [(lato, in_png(rgba, lato)) for lato in MISURE]
    testa = struct.pack("<HHH", 0, 1, len(immagini))
    offset = 6 + 16 * len(immagini)
    voci, dati = b"", b""
    for lato, png in immagini:
        voci += struct.pack("<BBBBHHII", lato % 256, lato % 256, 0, 0, 1, 32, len(png), offset + len(dati))
        dati += png
    (QUI / "fumetto.ico").write_bytes(testa + voci + dati)
    icns(rgba)


if __name__ == "__main__":
    main()
