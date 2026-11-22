# État des ROM de test GBA (2026-11-22, étape GBA 1 : fondations)

ROM téléchargées par `scripts/roms-test.sh` dans `roms/` (ignoré par git), vérifiées par `cargo test`
(`gba-headless/tests/roms.rs`, sautent avec « SAUTÉ » si absentes). BIOS : Cult-of-GBA (libre, MIT)
embarqué, démarrage direct sauf mention.

## jsmolka gba-tests (commit a7113b6)
Résultat lu dans r12 quand la ROM tourne sur sa boucle finale (`b .`) ; l'écran affiche le même
résultat en mode 4.

| ROM | Résultat |
|---|---|
| arm | réussi (aussi après démarrage complet par le BIOS libre exécuté depuis 0) |
| thumb | réussi |
| memory | réussi (miroirs, écritures 8 bits VRAM/OAM/palette) |
| nes | réussi |
| save/none | réussi |
| save/sram | réussi |
| bios | raté au test 2 : lecture du BIOS après SWI = 0xE3A02004 (octets du BIOS de Nintendo en 0x188, impossible avec un BIOS libre). Test 1 (valeur après démarrage 0xE129F000) réussi. Le test 3 (pendant une IRQ) demande lui aussi le BIOS de Nintendo. |
| save/flash64, flash128 | ratés (flash64 : test 10 ; flash128 ne finit pas) : Flash non émulée, lot « sauvegardes » |
| unsafe | raté au test 1 (miroir SRAM 32 Kio) : comportement que le README dit faux sur console, non visé |
| ppu/hello, shades, stripes | vérifiés à l'œil (pas d'image de référence) : texte « Hello world! » en mode 0, dégradé de bleus, rayures |

## FuzzARM (DenSinH, commit a675329)
ARM_Any, ARM_DataProcessing, THUMB_Any, THUMB_DataProcessing, FuzzARM : tous réussis (« End of
testing », aucun rapport d'erreur au début de l'EWRAM), ~2 500 images.

## armwrestler (version corrigée destoer, commit 802e55a)
Menu piloté par touches simulées dans le test. Les 5 pages ARM (ALU 1 et 2, LDR/STR 1 et 2,
LDM/STM) et les 3 pages Thumb (ALU, LDR/STR, LDM/STM) : tout « OK », aucun « BAD ».

## Tests du cœur sans ROM externe (`gba-core`)
- `tests/irq.rs` : programme ARM assemblé à la main, SWI VBlankIntrWait du BIOS libre → HALT, IRQ
  VBlank, gestionnaire utilisateur en 0x03007FFC : 60 retours en 60 images.
- `src/timer.rs` : prescaler, débordements multiples rattrapés, cascade, arrêt.
- `src/dma.rs` : DMA immédiat 32 bits + IRQ, DMA HBlank répété avec rechargement de destination.
- `tests/robustesse.rs` : ROM vide / trop grande / BIOS de mauvaise taille refusés ; ROM aléatoires
  (1 octet à 256 Kio) et écritures aléatoires dans toutes les E/S : aucune panique.

## Performances (release, i7-7700, `gba-headless --bench`, 3 000 images)
FuzzARM ~2 150 i/s, armwrestler (menu) ~1 570 i/s, ppu/stripes ~1 950 i/s, arm.gba ~2 870 i/s
(objectif > 300 i/s).

## Non émulé / raccourcis (commentaires `raccourci :`)
PPU : sprites, fenêtres, mélange, mosaïque, modes 1/2/5, affine du fond 2 en bitmap. Prélecture
cartouche approchée (lecture séquentielle ROM sans attente). Pas de délai de démarrage des timers
ni des DMA, pas d'interruption d'un DMA par un autre. STOP traité comme HALT. C inchangé après MUL.
Son (registres relus tels qu'écrits), Flash/EEPROM, série.
