#!/usr/bin/env bash
# Version web (PWA) : compile gb-web en WebAssembly et produit out/web/, prêt à servir en
# fichiers statiques (aucune ROM dedans, sauf les 2 jeux libres de jeux-libres/, vérifiés par
# empreinte). Usage : scripts/web.sh
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
SORTIE=out/web
CIBLE=wasm32-unknown-unknown

# Outils : cible wasm et wasm-bindgen-cli à la version exacte du crate (Cargo.lock).
if ! rustup target list --installed | grep -qx "$CIBLE"; then
  rustup target add "$CIBLE"
fi
VERSION_WB=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/version = |"/, ""); print; exit }' Cargo.lock)
if [ "$(wasm-bindgen --version 2>/dev/null | awk '{print $2}')" != "$VERSION_WB" ]; then
  cargo install --locked wasm-bindgen-cli --version "$VERSION_WB"
fi

# Profil release du workspace : opt-level 3, lto, codegen-units 1.
# strip = debuginfo ici seulement (pas symbols) : wasm-bindgen lit la section target_features.
CARGO_PROFILE_RELEASE_STRIP=debuginfo cargo build --release --target "$CIBLE" -p gb-web

rm -rf "$SORTIE"
mkdir -p "$SORTIE/pkg" "$SORTIE/fonts" "$SORTIE/licences"
wasm-bindgen --target web --no-typescript --out-dir "$SORTIE/pkg" "target/$CIBLE/release/gb_web.wasm"
if command -v wasm-opt >/dev/null; then
  wasm-opt -O3 --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext \
    --enable-mutable-globals --enable-reference-types --enable-multivalue \
    "$SORTIE/pkg/gb_web_bg.wasm" -o "$SORTIE/pkg/gb_web_bg.wasm"
else
  echo "wasm-opt absent : .wasm non réoptimisé (facultatif, paquet binaryen)"
fi

cp -r web/index.html web/manifest.webmanifest web/sw.js web/css web/js web/icones "$SORTIE/"
cp gb-app/assets/fonts/Rubik-Regular.ttf gb-app/assets/fonts/Rubik-Medium.ttf \
  gb-app/assets/fonts/Rubik-Bold.ttf "$SORTIE/fonts/"
cp LICENSE "$SORTIE/licences/gbemu-MIT.txt"
cp gb-app/assets/fonts/OFL.txt "$SORTIE/licences/Rubik-OFL.txt"
cp gba-core/bios/LICENSE "$SORTIE/licences/BIOS-Cult-of-GBA-MIT.txt"

# Jeux libres à essayer (homebrew sous licence libre) : copiés depuis roms/homebrew-libres/
# (ignoré par git), téléchargés s'ils manquent, refusés si l'empreinte diffère.
# Champs : dossier, fichier, sha256, URL du jeu, URL de la licence, nom publié de la licence.
JEUX_LIBRES=(
  "libbet libbet.gb 3607412031c8287cf878299ce96e581e85b852dde703806343b95576fa3ff1a9
   https://github.com/pinobatch/libbet/releases/download/v0.08/libbet.gb
   https://raw.githubusercontent.com/pinobatch/libbet/master/LICENSE libbet-zlib.txt"
  "ucity ucity.gbc 9422ee2ca7b7ea1d46b58b2a429fff3f354dfd3e732dee1e7ae6220f148ce6e0
   https://github.com/AntonioND/ucity/releases/download/v1.3/ucity.gbc
   https://raw.githubusercontent.com/AntonioND/ucity/master/gpl-3.0.txt ucity-GPL-3.0.txt"
)
mkdir -p "$SORTIE/jeux-libres"
LISTE_BLANCHE=""
for j in "${JEUX_LIBRES[@]}"; do
  read -r dossier fichier sha url_jeu url_licence licence <<<"$(echo $j)"
  src="roms/homebrew-libres/.dl_$dossier"
  mkdir -p "$src"
  [ -f "$src/$fichier" ] || curl -fsSL -o "$src/$fichier" "$url_jeu"
  [ -f "$src/${url_licence##*/}" ] || curl -fsSL -o "$src/${url_licence##*/}" "$url_licence"
  if [ "$(sha256sum "$src/$fichier" | cut -d' ' -f1)" != "$sha" ]; then
    echo "ERREUR : empreinte inattendue pour $src/$fichier" >&2
    exit 1
  fi
  cp "$src/$fichier" "$SORTIE/jeux-libres/$fichier"
  cp "$src/${url_licence##*/}" "$SORTIE/jeux-libres/$licence"
  LISTE_BLANCHE+="$sha  $SORTIE/jeux-libres/$fichier"$'\n'
done
cat >"$SORTIE/jeux-libres/CREDITS.txt" <<'FIN'
Jeux libres proposés dans gbemu web (homebrew, redistribués sans modification).

Libbet and the Magic Floor v0.08 (Game Boy)
  Auteur : Damian Yerrick. Licence zlib (libbet-zlib.txt).
  Source : https://github.com/pinobatch/libbet

µCity v1.3 (Game Boy Color)
  Auteur : Antonio Niño Díaz. Licence GPL-3.0 (ucity-GPL-3.0.txt).
  Code source complet : https://github.com/AntonioND/ucity (étiquette v1.3)
FIN

# Version du cache hors ligne = empreinte des fichiers servis.
EMPREINTE=$(cd "$SORTIE" && find . -type f ! -name sw.js -print0 | sort -z | xargs -0 sha256sum | sha256sum | cut -c1-12)
sed -i "s/^const VERSION = \"dev\";/const VERSION = \"$EMPREINTE\";/" "$SORTIE/sw.js"

# Appli Windows (scripts/publier-exe.sh), copiée après l'empreinte : une nouvelle version de
# l'exe ne vide pas le cache hors ligne du site (sw.js ne met jamais telecharger/ en cache).
if [ -d out/telecharger ]; then
  cp -r out/telecharger "$SORTIE/telecharger"
fi

# Garde-fou : jamais de ROM ni de sauvegarde dans ce qui sera publié, hors liste blanche
# (chemin et empreinte exacts des jeux libres).
INTRUS=$(find "$SORTIE" -type f \( -iname '*.gb' -o -iname '*.gbc' -o -iname '*.gba' -o -iname '*.zip' \
  -o -iname '*.7z' -o -iname '*.sav' -o -iname '*.state' \) -print0 | sort -z | xargs -0 -r sha256sum |
  grep -vxF -f <(printf '%s' "$LISTE_BLANCHE") || true)
if [ -n "$INTRUS" ]; then
  echo "ERREUR : fichiers de jeu dans $SORTIE :" >&2
  echo "$INTRUS" >&2
  exit 1
fi

W="$SORTIE/pkg/gb_web_bg.wasm"
echo "Prêt : $SORTIE (cache $EMPREINTE)"
echo "gb_web_bg.wasm : $(stat -c %s "$W") octets, $(gzip -9c "$W" | wc -c) compressé gzip"
