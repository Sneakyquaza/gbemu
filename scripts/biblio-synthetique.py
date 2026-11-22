#!/usr/bin/env python3
"""Bibliothèque synthétique de N ROM (aucune ROM réelle) : .gb/.gbc/.gba en vrac et en .zip."""
import os, random, sys, zipfile

dest = sys.argv[1]
n = int(sys.argv[2]) if len(sys.argv) > 2 else 5200
random.seed(5200)
MOTS = ("Donjon Douillet Kobold Kart Cerf Volant Boulanger Carillon Étoile Pixel Forêt Lune Robot "
        "Dragon Île Château Ninja Course Puzzle Golf Tennis Fusée Pirate Zèbre Quête Océan Volcan "
        "Neige Désert Ombre Lumière Mage Chevalier Tortue Serpent Abeille Pingouin Train Bateau "
        "Jardin Cuisine Musique Rêve Cristal Tempête Comète Galaxie Sorcier Hibou Renard Loup").split()
REGIONS = ["(France)", "(Europe)", "(USA)", "(Japan)", "(USA, Europe)", "(Europe) (En,Fr,De)", "(Rev 1)"]
os.makedirs(os.path.join(dest, "zips"), exist_ok=True)
os.makedirs(os.path.join(dest, "GBA"), exist_ok=True)

def rom(ext, titre, i):
    taille = 0x8000 if ext != "gba" else 0x400
    d = bytearray(taille)
    d[0:4] = i.to_bytes(4, "little")  # CRC distinct
    t = titre.upper().encode("ascii", "ignore")[:11 if ext != "gba" else 12]
    if ext == "gba":
        d[0xA0:0xA0 + len(t)] = t
    else:
        d[0x134:0x134 + len(t)] = t
        d[0x143] = 0x80 if ext == "gbc" else 0
    return bytes(d)

zipf = None
for i in range(n):
    ext = random.choices(["gb", "gbc", "gba"], [5, 3, 4])[0]
    nom = " ".join(random.sample(MOTS, random.randint(1, 4)))
    if random.random() < 0.3:
        nom += f" {random.randint(2, 9)}"
    fichier = f"{nom} {random.choice(REGIONS)} [{i:04d}].{ext}"
    donnees = rom(ext, nom, i)
    if i % 5 == 0:
        # Un zip sur cinq, parfois plusieurs ROM par archive.
        if zipf is None or random.random() < 0.6:
            if zipf:
                zipf.close()
            zipf = zipfile.ZipFile(os.path.join(dest, "zips", f"lot{i:04d}.zip"), "w", zipfile.ZIP_DEFLATED)
        zipf.writestr(fichier, donnees)
    else:
        sous = "GBA" if ext == "gba" else ""
        with open(os.path.join(dest, sous, fichier), "wb") as f:
            f.write(donnees)
if zipf:
    zipf.close()
