# État des ROM de test GBA (2026-11-22, étape GBA 1 : fondations ; PPU complet)

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
| save/flash64, flash128 | réussis (ID Panasonic 32/1B et Sanyo 62/13, effacement puce/secteur, banques) |
| unsafe | raté au test 1 (miroir SRAM 32 Kio) : comportement que le README dit faux sur console, non visé |
| ppu/hello, shades, stripes | vérifiés à l'œil (pas d'image de référence) : texte « Hello world! » en mode 0, dégradé de bleus, rayures (captures `out/captures/gba/ppu/jsmolka-*.png`) |

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

## PPU complet (`gba-core/src/ppu.rs`, `ppu/fonds.rs`, `ppu/objets.rs`)
Modes 0-5 (texte 4/8 bpp 256-512, affines 128-1024 avec débordement, bitmaps 3/4/5 avec page et
matrice de BG2), points de référence affines rechargés au VBlank et à l'écriture, sprites (128,
toutes tailles, 1D/2D, affines + double taille, semi-transparents, fenêtre OBJ, limite de cycles
1210/954), priorités, WIN0/WIN1/OBJ/extérieur (bascule verticale à Y1/Y2, X1 > X2 boucle), BLDCNT
alpha/éclaircir/assombrir, mosaïque fonds et sprites, écran blanc forcé, GREENSWAP, délai de
3 lignes à l'activation d'un calque / à la fin de l'écran blanc. Rendu par ligne au début du HBlank.
- Tests unitaires (`ppu/tests.rs`, 17) : une scène construite à la main par fonctionnalité, ligne
  générée comparée à la ligne attendue.
- `gba-headless/tests/ppu.rs` :
  - destoer `window_midframe` : 0 point d'écart avec expected.png (5 bits par composante).
  - nba-emu hw-test `sprite-hmosaic` : 0 point d'écart (capture NanoBoyAdvance ×8 échantillonnée).
  - tonc (34 démos, binaires de J. Vijn, exemples CC0) : 120 images sans panique, écran non uni ;
    planche vérifiée à l'œil `out/captures/gba/ppu/tonc_planche.png` (mode 7, bigmap, obj_aff,
    win_demo, bld_demo, mos_demo, oacombo, bm_modes, prio_demo… conformes).
  - `etat_rejoue_meme_ecran` : m7_ex, dispcnt-latch, sprite-hmosaic, 30 images identiques après
    rechargement d'un état.
- nba-emu `dispcnt-latch` : conforme à l'image attendue à l'œil (délais d'activation compris ;
  écarts restants = couleurs corrigées de la capture). Pas de test auto (capture ×2,5).
- Ratés connus (rendu par ligne entière) : nba-emu `bgx`, `bgpd`, `greenswap` (registres changés par
  DMA au milieu de la ligne, photos de console), destoer `lyc_midline` (synchro au cycle près).

## Sauvegardes cartouche (`gba-core/src/cartouche.rs`)
Type par chaînes alignées (`SRAM_V`/`SRAM_F_V`, `FLASH_V`/`FLASH512_V`, `FLASH1M_V`, `EEPROM_V`) ;
sans chaîne : SRAM 64 Kio, type repris de la taille du .sav importé (512 o / 8 Kio = EEPROM,
128 Kio = Flash 128). EEPROM : taille fixée par la longueur du 1er DMA 3 (9/73 bits = 512 o,
17/81 = 8 Kio), fenêtre 0x0DFFFF00 si ROM > 16 Mio. `Gba::sauvegarde()` = `None` tant que rien n'est
à garder (type inconnu jamais écrit, EEPROM de taille inconnue). Tests unitaires : détection,
EEPROM 512 o et 8 Kio (écriture/lecture série, aller-retour .sav, commande invalide), fenêtre des
grandes ROM, repli par taille, tailles de .sav quelconques sans panique, Flash ID/banque/effacement.

## Sauvegardes d'état (`gba-core/src/etat.rs`)
En-tête `GBAE` + version 2 (v2 : champs du PPU complet) + CRC32 ROM, puis CPU et bus (PPU, timers, DMA, son : moteur gb-core +
FIFO, cartouche). ~565 Kio (+ mémoire de sauvegarde) ; meilleur temps ~15-20 µs à la sauvegarde,
~40 µs au chargement (release et debug, opt-level 3). Tests : `gba-headless/tests/roms.rs`
`etats_aller_retour` (arm, flash128, stripes, FuzzARM : rejouer 60 images depuis l'état redonne
le même écran et le même état), `gba-core/tests/etat.rs` (refus Format/Version/AutreRom/Taille sans
changement, 300 états abîmés : refus propre ou exécution sans panique).

## Son (`gba-core/src/apu.rs`)
Aucune ROM de test son GBA publique trouvée. Vérifié par :
- tests unitaires (`apu.rs`) : FIFO pleine / demande de DMA à 16 octets / remise à zéro, volume
  50/100 % et panoramique du son direct, biais 0 qui coupe la moitié négative, carré du canal 1
  (fréquence 512 Hz, relecture des registres en écriture seule), RAM d'onde en 2 banques ;
  bout en bout sur le bus : timer 0 à 32 768 Hz + DMA1 spécial répété → créneau de 2 048 Hz en sortie.
- tonc `snd1_demo` (`gba-headless --wav`) : notes PSG audibles (D 293 Hz, F 352 Hz, D 587 Hz).
- Les canaux Game Boy sont ceux de gb-core (blargg dmg_sound/cgb_sound toujours verts).

## Performances (release, i7-7700, `gba-headless --bench`, 3 000 images)
FuzzARM ~2 150 i/s, armwrestler (menu) ~1 570 i/s, ppu/stripes ~1 950 i/s, arm.gba ~2 870 i/s
(objectif > 300 i/s).
PPU complet (machine chargée, charge 4-7 sur 8 cœurs) : ppu/stripes ~1 900-1 990 (inchangé), hello
~2 240, FuzzARM ~1 900, tonc m3_demo ~1 660, bigmap ~1 120, obj_aff ~970, win_demo ~930,
prio_demo ~810, bld_demo ~760, m7_ex ~590 i/s (m7_ex ~930 sans rendu : le CPU domine). Chemin
rapide sans fenêtre ni effet (calques peints de l'arrière vers l'avant), sinon composition par point.
Son (mesures alternées avant/après, 3 000 images) : FuzzARM et armwrestler sans son inchangés
(±3 %), tonc snd1_demo (2 carrés actifs) ~2 990 → ~2 610 i/s (−13 %).

## Non émulé / raccourcis (commentaires `raccourci :`)
PPU : ligne rendue d'un bloc au début du HBlank (changements en cours de ligne ignorés), limite de
cycles des sprites décomptée par sprite entier, mosaïque verticale des fonds affines approchée
(point de référence reculé de PB/PD), pas d'accès VRAM/OAM bloqués pendant le rendu. Prélecture
cartouche approchée (lecture séquentielle ROM sans attente). Pas de délai de démarrage des timers
ni des DMA, pas d'interruption d'un DMA par un autre. STOP traité comme HALT. C inchangé après MUL.
Son : résolution PWM de SOUNDBIAS (bits 14-15) ignorée (sortie plus propre que la console),
SOUNDCNT_X relu sans rattraper les canaux, rotation des banques d'onde pendant la lecture non
émulée. Série. Sauvegardes : EEPROM toujours prête (pas de délai d'écriture), Flash écrite
sans délai ni « bits seulement à 0 », recherche des chaînes alignée sur 4.
