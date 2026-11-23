#!/usr/bin/env bash
# Version web (PWA) : compile gb-web en WebAssembly et produit out/web/, prêt à servir en
# fichiers statiques (aucune ROM dedans). Usage : scripts/web.sh
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

# Version du cache hors ligne = empreinte des fichiers servis.
EMPREINTE=$(cd "$SORTIE" && find . -type f ! -name sw.js -print0 | sort -z | xargs -0 sha256sum | sha256sum | cut -c1-12)
sed -i "s/^const VERSION = \"dev\";/const VERSION = \"$EMPREINTE\";/" "$SORTIE/sw.js"

# Garde-fou : jamais de ROM ni de sauvegarde dans ce qui sera publié.
INTRUS=$(find "$SORTIE" -type f \( -iname '*.gb' -o -iname '*.gbc' -o -iname '*.gba' -o -iname '*.zip' \
  -o -iname '*.7z' -o -iname '*.sav' -o -iname '*.state' \))
if [ -n "$INTRUS" ]; then
  echo "ERREUR : fichiers de jeu dans $SORTIE :" >&2
  echo "$INTRUS" >&2
  exit 1
fi

W="$SORTIE/pkg/gb_web_bg.wasm"
echo "Prêt : $SORTIE (cache $EMPREINTE)"
echo "gb_web_bg.wasm : $(stat -c %s "$W") octets, $(gzip -9c "$W" | wc -c) compressé gzip"
