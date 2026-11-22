#!/usr/bin/env bash
# Retélécharge les ROM de test publiques dans roms/ (ignoré par git).
# Sources officielles : blargg (miroir retrio/gb-test-roms), mooneye-test-suite (gekkio.fi),
# dmg-acid2 (mattcurrie/dmg-acid2). Chaque archive est extraite dans son propre dossier vide.
set -euo pipefail
cd "$(dirname "$0")/.."
ROMS=roms
mkdir -p "$ROMS"

BLARGG_SHA=c240dd7d700e5c0b00a7bbba52b53e4ee67b5f15
MTS=mts-20260714-0944-31510e1

# blargg
if [ ! -d "$ROMS/blargg" ]; then
  d=$(mktemp -d "$ROMS/.dl_blargg.XXXX")
  curl -fsSL -o "$d/blargg.tar.gz" "https://github.com/retrio/gb-test-roms/archive/$BLARGG_SHA.tar.gz"
  mkdir "$d/x" && tar -xzf "$d/blargg.tar.gz" -C "$d/x"
  mv "$d/x/gb-test-roms-$BLARGG_SHA" "$ROMS/blargg"
fi

# mooneye-test-suite
if [ ! -d "$ROMS/mooneye" ]; then
  d=$(mktemp -d "$ROMS/.dl_mooneye.XXXX")
  curl -fsSL -o "$d/mts.tar.gz" "https://gekkio.fi/files/mooneye-test-suite/$MTS/$MTS.tar.gz"
  mkdir "$d/x" && tar -xzf "$d/mts.tar.gz" -C "$d/x"
  mv "$d/x/$MTS" "$ROMS/mooneye"
fi

# dmg-acid2 (ROM + image de référence)
if [ ! -d "$ROMS/dmg-acid2" ]; then
  d=$(mktemp -d "$ROMS/.dl_acid2.XXXX")
  curl -fsSL -o "$d/dmg-acid2.gb" "https://github.com/mattcurrie/dmg-acid2/releases/download/v1.0/dmg-acid2.gb"
  curl -fsSL -o "$d/reference-dmg.png" "https://raw.githubusercontent.com/mattcurrie/dmg-acid2/master/img/reference-dmg.png"
  mv "$d" "$ROMS/dmg-acid2"
fi

echo "ROM de test prêtes dans $ROMS/ (archives téléchargées conservées dans $ROMS/.dl_*)."
