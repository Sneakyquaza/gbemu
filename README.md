# gbemu

Émulateur **Game Boy, Game Boy Color et Game Boy Advance** écrit en Rust, avec une application de bureau soignée (Windows en priorité, Linux aussi).

*A Game Boy / Game Boy Color / Game Boy Advance emulator written in Rust, with a polished desktop front-end (Windows first, Linux too). Personal, non-commercial project.*

## Fonctionnalités

- **Cœurs d'émulation** maison, sans dépendance :
  - `gb-core` : Game Boy (DMG) et Game Boy Color (CGB) : CPU SM83, PPU, APU 4 canaux, MBC1/3/5, RTC, double vitesse, HDMA ;
  - `gba-core` : Game Boy Advance : CPU ARM7TDMI (ARM + Thumb), PPU complet (modes 0–5, sprites affines, fenêtres, transparence, mosaïque), son (canaux GB + son direct FIFO), DMA, timers, sauvegardes SRAM / Flash / EEPROM. BIOS libre de remplacement embarqué ([Cult-of-GBA BIOS](https://github.com/Cult-of-GBA/BIOS), MIT) : aucun BIOS Nintendo n'est fourni ni requis.
- **Application** (`gb-app`, eframe/egui) :
  - bibliothèque de jeux (dossier ou glisser-déposer, .gb/.gbc/.gba en vrac ou dans des .zip/.7z), reconnaissance par CRC via les DAT No-Intro de [libretro-database](https://github.com/libretro/libretro-database), jaquettes [libretro-thumbnails](https://github.com/libretro-thumbnails/libretro-thumbnails) ;
  - recherche instantanée, favoris, récents, temps de jeu, thèmes Nuit / Jour ;
  - 10 save states par jeu avec miniatures, reprise automatique, rembobinage, avance rapide, filtres d'écran (grille LCD, rémanence…) ;
  - remappage clavier / manette sur une console dessinée, profils par manette ;
  - ouverture en ~0,1 s.
- **Aucun jeu n'est fourni.** L'utilisateur ajoute lui-même ses propres fichiers.

## Métadonnées (ScreenScraper)

Une intégration de l'API ScreenScraper v2 est en cours (résumés, notes, logos, par CRC + taille + système, dans le respect des quotas renvoyés par l'API, avec cache local et mode hors ligne). Les identifiants de l'utilisateur restent sur sa machine.

## Tests

Validé uniquement avec des ROM de test publiques : blargg (cpu_instrs, instr_timing, mem_timing, dmg_sound, cgb_sound), mooneye-test-suite, dmg-acid2 / cgb-acid2, jsmolka gba-tests, FuzzARM, armwrestler, nba-emu hw-test, destoer, démos tonc. Voir `docs/etat-tests.md` et `docs/etat-tests-gba.md`.

```sh
scripts/roms-test.sh      # télécharge les ROM de test publiques dans roms/
cargo test --workspace
cargo build --release -p gb-app
cargo build --release --target x86_64-pc-windows-gnu -p gb-app
```

## Licences tierces

Voir `gb-app/assets/licences/LICENCES.txt` (polices Rubik et Silkscreen : OFL ; BIOS Cult-of-GBA : MIT ; crates Rust et leurs licences).

Game Boy, Game Boy Color et Game Boy Advance sont des marques de Nintendo. Ce projet n'est ni affilié à Nintendo ni approuvé par Nintendo.
