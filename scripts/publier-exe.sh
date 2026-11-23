#!/usr/bin/env bash
# Appli Windows proposée sur le site : compile gbemu.exe et remplit out/telecharger/ (exe,
# LICENCES.txt, version.json lu par la mise à jour de l'appli et par le site). scripts/web.sh
# le copie ensuite dans out/web/telecharger/. Usage : scripts/publier-exe.sh ["notes de version"]
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
SORTIE=out/telecharger
URL=https://gbemu.oryx3.fr/telecharger/gbemu.exe
NOTES=${1:-}

cargo build --release --target x86_64-pc-windows-gnu -p gb-app
EXE=target/x86_64-pc-windows-gnu/release/gbemu.exe
VERSION=$(awk '/^\[workspace.package\]/ { w = 1 } w && /^version = / { gsub(/version = |"/, ""); print; exit }' Cargo.toml)

mkdir -p "$SORTIE"
cp "$EXE" "$SORTIE/gbemu.exe"
cp gb-app/assets/licences/LICENCES.txt "$SORTIE/LICENCES.txt"
SHA=$(sha256sum "$SORTIE/gbemu.exe" | cut -d' ' -f1)
TAILLE=$(stat -c %s "$SORTIE/gbemu.exe")
python3 -I - "$SORTIE/version.json" "$VERSION" "$URL" "$SHA" "$TAILLE" "$NOTES" <<'FIN'
import json, sys
chemin, version, url, sha, taille, notes = sys.argv[1:]
with open(chemin, "w", encoding="utf-8") as f:
    json.dump({"version": version, "url": url, "sha256": sha, "taille": int(taille), "notes": notes},
              f, ensure_ascii=False, indent=1)
    f.write("\n")
FIN
echo "Prêt : $SORTIE (gbemu $VERSION, $TAILLE octets, sha256 $SHA)"
