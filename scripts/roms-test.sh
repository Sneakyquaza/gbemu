#!/usr/bin/env bash
# Retélécharge les ROM de test publiques dans roms/ (ignoré par git).
# Sources officielles : blargg (miroir retrio/gb-test-roms), mooneye-test-suite (gekkio.fi),
# dmg-acid2 et cgb-acid2 (mattcurrie), GBA : jsmolka gba-tests, FuzzARM, armwrestler.
# Chaque archive est extraite dans son propre dossier vide.
set -euo pipefail
cd "$(dirname "$0")/.."
ROMS=roms
mkdir -p "$ROMS"

BLARGG_SHA=c240dd7d700e5c0b00a7bbba52b53e4ee67b5f15
MTS=mts-20260714-0944-31510e1

# blargg (cpu_instrs, instr_timing, mem_timing, mem_timing-2, dmg_sound…)
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

# cgb-acid2 (ROM + image de référence)
if [ ! -d "$ROMS/cgb-acid2" ]; then
  d=$(mktemp -d "$ROMS/.dl_cgbacid2.XXXX")
  curl -fsSL -o "$d/cgb-acid2.gbc" "https://github.com/mattcurrie/cgb-acid2/releases/download/v1.1/cgb-acid2.gbc"
  curl -fsSL -o "$d/reference.png" "https://raw.githubusercontent.com/mattcurrie/cgb-acid2/master/img/reference.png"
  mv "$d" "$ROMS/cgb-acid2"
fi

# GBA : jsmolka gba-tests (MIT), FuzzARM (DenSinH, GPL-3.0, ROM générées), armwrestler corrigé (destoer)
GBA_TESTS_SHA=a7113b67e63f83a9b321696ddd7042ccfad6c881
FUZZARM_SHA=a675329cd57da48e3e406216ba2d79dd7e09ee20
ARMWRESTLER_SHA=802e55ad61b421f4bbbc6f74f96aa4e5df4d630f
gh_archive() { # dépôt sha dossier
  [ -d "$ROMS/$3" ] && return 0
  local d n
  d=$(mktemp -d "$ROMS/.dl_$3.XXXX")
  n=${1#*/}
  curl -fsSL -o "$d/a.tar.gz" "https://github.com/$1/archive/$2.tar.gz"
  mkdir "$d/x" && tar -xzf "$d/a.tar.gz" -C "$d/x"
  mv "$d/x/$n-$2" "$ROMS/$3"
}
gh_archive jsmolka/gba-tests "$GBA_TESTS_SHA" gba-tests
gh_archive DenSinH/FuzzARM "$FUZZARM_SHA" fuzzarm
gh_archive destoer/armwrestler-gba-fixed "$ARMWRESTLER_SHA" armwrestler

# GBA PPU : nba-emu hw-test (BSD-3), destoer gba_tests (MIT), binaires tonc (J. Vijn, exemples
# repris en CC0 par gbadev-org/libtonc-examples)
gh_archive nba-emu/hw-test 3932a0e54dbb5d94b84798d2d5e462e080b95599 nba-hw-test
gh_archive destoer/gba_tests d714cda49375b84183f8437b35848adb80c19562 destoer-gba-tests
if [ ! -d "$ROMS/tonc" ]; then
  d=$(mktemp -d "$ROMS/.dl_tonc.XXXX")
  curl -fsSL -o "$d/tonc-bin.zip" "https://www.coranac.com/files/tonc-bin.zip"
  mkdir "$d/x" && unzip -q "$d/tonc-bin.zip" -d "$d/x"
  mv "$d/x/bin" "$ROMS/tonc"
fi

test -f "$ROMS/blargg/dmg_sound/dmg_sound.gb" || { echo "dmg_sound absent de $ROMS/blargg" >&2; exit 1; }
test -f "$ROMS/blargg/cgb_sound/cgb_sound.gb" || { echo "cgb_sound absent de $ROMS/blargg" >&2; exit 1; }
test -f "$ROMS/gba-tests/arm/arm.gba" || { echo "arm.gba absent de $ROMS/gba-tests" >&2; exit 1; }

echo "ROM de test prêtes dans $ROMS/ (archives téléchargées conservées dans $ROMS/.dl_*)."
