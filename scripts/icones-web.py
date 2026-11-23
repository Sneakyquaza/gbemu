#!/usr/bin/env python3
"""Icônes de la version web (web/icons/) : la console de l'app de bureau sur le fond Nuit.
Usage : python3 -I scripts/icones-web.py  (Pillow requis ; à relancer seulement si le dessin change)."""
from pathlib import Path
from PIL import Image, ImageDraw

FOND = (0x16, 0x17, 0x1F)
LAMPE = (0x3A, 0x30, 0x2C)
PLASTIQUE_CLAIR = (0xD8, 0xD4, 0xCC)
PLASTIQUE_OMBRE = (0x9E, 0x99, 0x8F)
CADRE = (0x5E, 0x5F, 0x73)
DMG = [(0xC4, 0xCF, 0xA1), (0x8B, 0x95, 0x6D), (0x4D, 0x53, 0x3C), (0x1F, 0x1F, 0x1F)]
CROIX = (0x2A, 0x2A, 0x30)
BAIE = (0xB0, 0x2E, 0x6A)
PASTILLE = (0x6A, 0x68, 0x70)


def coin_arrondi(d, x1, y1, r, plein, vide, im=None):
    """Arrondit le coin bas droit (x1, y1) d'une forme déjà dessinée avec le rayon r."""
    if im is not None:
        vide = im.getpixel((int(x1 + 2), int(y1 + 2)))
    d.rectangle((x1 - r, y1 - r, x1, y1), fill=vide)
    d.ellipse((x1 - 2 * r, y1 - 2 * r, x1, y1), fill=plein)


def dessiner(taille, echelle_console):
    s = taille * 4  # sur-échantillonnage
    im = Image.new("RGB", (s, s), FOND)
    # Halo de la lampe
    halo = Image.new("L", (s, s), 0)
    ImageDraw.Draw(halo).ellipse((-s * 0.3, -s * 0.45, s * 1.0, s * 0.55), fill=150)
    from PIL import ImageFilter
    halo = halo.filter(ImageFilter.GaussianBlur(s * 0.12))
    im.paste(Image.new("RGB", (s, s), LAMPE), (0, 0), halo)

    d = ImageDraw.Draw(im)
    w = s * 0.40 * echelle_console
    h = s * 0.62 * echelle_console
    x0 = (s - w) / 2
    y0 = (s - h) / 2
    u = w / 100  # unité : 1/100 de la largeur du boîtier
    # Ombre
    ombre = Image.new("L", (s, s), 0)
    ImageDraw.Draw(ombre).rounded_rectangle((x0, y0 + 4 * u, x0 + w, y0 + h + 4 * u), radius=6 * u, fill=170)
    ombre = ombre.filter(ImageFilter.GaussianBlur(5 * u))
    im.paste(Image.new("RGB", (s, s), (0, 0, 0)), (0, 0), ombre)
    # Boîtier : dégradé vertical, coin bas droit très arrondi
    corps = Image.new("L", (s, s), 0)
    cd = ImageDraw.Draw(corps)
    cd.rounded_rectangle((x0, y0, x0 + w, y0 + h), radius=5 * u, fill=255)
    coin_arrondi(cd, x0 + w, y0 + h, 26 * u, 255, 0)
    deg = Image.new("RGB", (s, s))
    dd = ImageDraw.Draw(deg)
    for y in range(s):
        t = min(max((y - y0) / h, 0), 1)
        dd.line((0, y, s, y), fill=tuple(round(a + (b - a) * t) for a, b in zip(PLASTIQUE_CLAIR, PLASTIQUE_OMBRE)))
    im.paste(deg, (0, 0), corps)
    d = ImageDraw.Draw(im)
    # Cadre et écran
    d.rounded_rectangle((x0 + 9 * u, y0 + 9 * u, x0 + w - 9 * u, y0 + 66 * u), radius=4 * u, fill=CADRE)
    coin_arrondi(d, x0 + w - 9 * u, y0 + 66 * u, 14 * u, CADRE, None, im)
    ex0, ey0, ex1, ey1 = x0 + 21 * u, y0 + 17 * u, x0 + w - 21 * u, y0 + 58 * u
    d.rectangle((ex0, ey0, ex1, ey1), fill=DMG[0])
    # Paysage en 4 teintes : soleil, colline, sol
    d.ellipse((ex0 + 36 * u, ey0 + 6 * u, ex0 + 48 * u, ey0 + 18 * u), fill=DMG[1])
    d.polygon([(ex0, ey1 - 12 * u), (ex0 + 18 * u, ey0 + 16 * u), (ex0 + 40 * u, ey1 - 12 * u)], fill=DMG[2])
    d.rectangle((ex0, ey1 - 12 * u, ex1, ey1), fill=DMG[3])
    # Croix
    cx, cy = x0 + 26 * u, y0 + 96 * u
    d.rounded_rectangle((cx - 15 * u, cy - 5 * u, cx + 15 * u, cy + 5 * u), radius=2 * u, fill=CROIX)
    d.rounded_rectangle((cx - 5 * u, cy - 15 * u, cx + 5 * u, cy + 15 * u), radius=2 * u, fill=CROIX)
    # A, B
    for bx, by in ((x0 + 80 * u, y0 + 89 * u), (x0 + 63 * u, y0 + 99 * u)):
        d.ellipse((bx - 8 * u, by - 8 * u, bx + 8 * u, by + 8 * u), fill=BAIE)
    # Select, Start
    for px in (x0 + 36 * u, x0 + 54 * u):
        d.rounded_rectangle((px - 6 * u, y0 + 128 * u, px + 6 * u, y0 + 132 * u), radius=2 * u, fill=PASTILLE)
    return im.resize((taille, taille), Image.LANCZOS)


def main():
    dossier = Path(__file__).resolve().parent.parent / "web" / "icons"
    dossier.mkdir(parents=True, exist_ok=True)
    dessiner(192, 1.15).save(dossier / "icone-192.png", optimize=True)
    dessiner(512, 1.15).save(dossier / "icone-512.png", optimize=True)
    # Masquable : la console tient dans le cercle de sécurité (80 %).
    dessiner(512, 1.0).save(dossier / "icone-masquable-512.png", optimize=True)
    dessiner(180, 1.15).save(dossier / "apple-touch-icon.png", optimize=True)


if __name__ == "__main__":
    main()
