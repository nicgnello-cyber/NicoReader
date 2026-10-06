"""Disegna l'icona di NicoReader e ne fa icona.png (Linux), fumetto.ico
(Windows) e macos/nicoreader.icns (macOS).

L'icona e' fatta di forme (un quadrato dagli angoli tondi con la sfumatura,
due pagine, la costola fra loro, un riquadro e tre righe per pagina), qui
descritte in un riquadro da 256: si disegna a ogni misura direttamente, con
i bordi sfumati, invece di rimpicciolire o ingrandire un'immagine. Cosi' ci
sono anche le misure del Retina (fino a 1024 px), che da una 256 non si
potevano fare.

I colori si mescolano in luce lineare, come le pagine nel programma, cosi'
i bordi non si scuriscono.

Per macOS lo stesso disegno, ma piu' piccolo nel suo riquadro: la griglia
di Apple vuole il corpo dell'icona a 824 px su 1024 (qui e' a 236 su 256);
senza il margine, nel Dock sembrerebbe piu' grande delle altre.

    python fai_icona.py
"""
import io
import struct
from pathlib import Path

import numpy as np
from PIL import Image

QUI = Path(__file__).parent
# Windows: barra del titolo, taskbar, Esplora file, a ogni scala dello schermo
MISURE = [16, 20, 24, 32, 40, 48, 64, 256]
# macOS: il tipo di ogni blocco dell'ICNS e il suo lato in pixel (ic10 e' il
# 512@2x, ic14 il 256@2x)
MISURE_MAC = [("icp4", 16), ("icp5", 32), ("ic11", 32), ("icp6", 64), ("ic12", 64), ("ic07", 128),
              ("ic08", 256), ("ic13", 256), ("ic09", 512), ("ic14", 512), ("ic10", 1024)]
# il corpo (236 su 256) diventa 824/1024 del riquadro
SCALA_MAC = 224 / 256

# --- il disegno, in un riquadro da 256 ---
# la sfumatura va in diagonale, da (10, 10) a (246, 246)
SFUMATURA = ((80.7, 122.4, 255), (121.3, 63.6, 255))
FONDO = (10, 10, 246, 246, 50)
# la costola fra le pagine: il fondo piu' scuro
COSTOLA = (124, 62, 132, 194, 0)
OMBRA_COSTOLA = 210 / 255
PAGINA = (246, 246, 255)
RIQUADRO = (79, 125, 255)
RIGA = (187, 191, 207)
# la pagina sinistra; la destra e' 86 piu' in la'
DESTRA = 86
FORME = [
    # (x0, y0, x1, y1, raggio), colore
    ((46, 62, 124, 194, 8), PAGINA),
    ((56, 74, 114, 114, 5), RIQUADRO),
    ((56, 124, 114, 136, 4), RIGA),
    ((56, 146, 114, 158, 4), RIGA),
    ((56, 168, 91, 180, 4), RIGA),
]


def lineare(c):
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def srgb(c):
    c = np.clip(c, 0.0, 1.0)
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * c ** (1 / 2.4) - 0.055)


def colore(rgb):
    return lineare(np.asarray(rgb, dtype=np.float64) / 255)


def copertura(x, y, forma, px):
    """Quanto di ogni pixel copre il rettangolo dagli angoli tondi: dalla
    distanza dal bordo (negativa dentro), su un pixel di sfumatura."""
    x0, y0, x1, y1, r = forma
    qx = np.abs(x - (x0 + x1) / 2) - ((x1 - x0) / 2 - r)
    qy = np.abs(y - (y0 + y1) / 2) - ((y1 - y0) / 2 - r)
    fuori = np.hypot(np.maximum(qx, 0), np.maximum(qy, 0)) + np.minimum(np.maximum(qx, qy), 0) - r
    return np.clip(0.5 - fuori * px, 0.0, 1.0)[..., None]


def disegna(lato, scala=1.0):
    """L'icona a `lato` pixel (sRGB a 8 bit con l'alfa); `scala` la
    rimpicciolisce nel suo riquadro, centrata."""
    px = lato / 256 * scala
    # il centro di ogni pixel, nelle coordinate del riquadro da 256
    c = (np.arange(lato) + 0.5 - lato / 2) / px + 128
    x, y = np.meshgrid(c, c)
    t = np.clip(((x + y) - 20) / 472, 0, 1)[..., None]
    # la sfumatura e' fra i due colori come si scrivono (sRGB), non in luce lineare
    fondo = colore(np.asarray(SFUMATURA[0]) * (1 - t) + np.asarray(SFUMATURA[1]) * t)
    a = copertura(x, y, COSTOLA, px)
    rgb = fondo * (1 - a) + fondo * lineare(np.array(OMBRA_COSTOLA)) * a
    for dx in (0, DESTRA):
        for (x0, y0, x1, y1, r), col in FORME:
            a = copertura(x, y, (x0 + dx, y0, x1 + dx, y1, r), px)
            rgb = rgb * (1 - a) + colore(col) * a
    alfa = copertura(x, y, FONDO, px)
    out = np.concatenate([srgb(rgb), alfa], axis=2)
    return Image.fromarray(np.round(out * 255).astype(np.uint8), "RGBA")


def in_png(img):
    buf = io.BytesIO()
    img.save(buf, "PNG", optimize=True)
    return buf.getvalue()


def main():
    disegna(256).save(QUI / "icona.png", optimize=True)

    immagini = [(lato, in_png(disegna(lato))) for lato in MISURE]
    testa = struct.pack("<HHH", 0, 1, len(immagini))
    offset = 6 + 16 * len(immagini)
    voci, dati = b"", b""
    for lato, png in immagini:
        voci += struct.pack("<BBBBHHII", lato % 256, lato % 256, 0, 0, 1, 32, len(png), offset + len(dati))
        dati += png
    (QUI / "fumetto.ico").write_bytes(testa + voci + dati)

    blocchi = b""
    for tipo, lato in MISURE_MAC:
        png = in_png(disegna(lato, SCALA_MAC))
        blocchi += tipo.encode("ascii") + struct.pack(">I", 8 + len(png)) + png
    (QUI / "macos" / "nicoreader.icns").write_bytes(b"icns" + struct.pack(">I", 8 + len(blocchi)) + blocchi)


if __name__ == "__main__":
    main()
