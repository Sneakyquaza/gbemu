# État des ROM de test (2026-11-22, étapes 1, 2a et GBC : cœur DMG + son + Game Boy Color)

ROM téléchargées par `scripts/roms-test.sh` dans `roms/` (ignoré par git), vérifiées par `cargo test`.
Les tests DMG imposent le mode DMG (`GameBoy::new_mode(rom, false)`) : cpu_instrs, instr_timing et
mem_timing ont l'octet CGB 0x80 dans leur en-tête et démarreraient sinon en mode CGB.

## Blargg et dmg-acid2 : tout vert
| Test | Résultat |
|---|---|
| cpu_instrs (11/11) | réussi (sortie série « Passed ») |
| instr_timing | réussi |
| mem_timing (3/3) | réussi |
| mem_timing-2 (3/3) | réussi (statut en $A000) |
| dmg-acid2 | réussi (0 pixel différent de `reference-dmg.png`) |
| dmg_sound (12/12 + ROM complète) | réussi (statut en $A000) |

### dmg_sound (étape 2a, APU)
Tests `blargg_dmg_sound` (chaque ROM de `rom_singles/`, liste des ratés attendus `DMG_SOUND_KO_CONNUS`, vide)
et `blargg_dmg_sound_complet` (`dmg_sound.gb`).
01-registers, 02-len ctr, 03-trigger, 04-sweep, 05-sweep details, 06-overflow on trigger,
07-len sweep period sync, 08-len ctr during power, 09-wave read while on, 10-wave trigger while on,
11-regs after power, 12-wave write while on : tous réussis.
Écoute : `gb-headless roms/blargg/dmg_sound/dmg_sound.gb --frames 2400 --wav out/dmg_sound.wav` (bips finaux vers 34 s).

## mooneye-test-suite (mts-20260714) : 77 réussis / 103
Périmètre : `acceptance/` + `emulator-only/` (MBC1, MBC2, MBC5). Le test `mooneye_acceptance_et_mbc1`
échoue sur toute régression ; la liste des ratés attendus est `MOONEYE_KO_CONNUS` dans `gb-headless/tests/roms.rs`.

### Réussis (77)
- CPU / interruptions : add_sp_e_timing, call_cc_timing, call_cc_timing2, call_timing, call_timing2,
  di_timing-GS, div_timing, ei_sequence, ei_timing, halt_ime0_ei, halt_ime0_nointr_timing,
  halt_ime1_timing, halt_ime1_timing2-GS, if_ie_registers, instr/daa, interrupts/ie_push, intr_timing,
  jp_cc_timing, jp_timing, ld_hl_sp_e_timing, pop_timing, push_timing, rapid_di_ei, ret_cc_timing,
  ret_timing, reti_intr_timing, reti_timing, rst_timing
- Démarrage DMG : boot_div-dmgABCmgb, boot_hwio-dmgABCmgb, boot_regs-dmgABC
- bits : mem_oam, reg_f, unused_hwio-GS
- Timer (13/13) : div_write, rapid_toggle, tim00, tim00_div_trigger, tim01, tim01_div_trigger, tim10,
  tim10_div_trigger, tim11, tim11_div_trigger, tima_reload, tima_write_reloading, tma_write_reloading
- DMA OAM : oam_dma/basic, oam_dma/reg_read, oam_dma/sources-GS, oam_dma_restart, oam_dma_start, oam_dma_timing
- PPU : hblank_ly_scx_timing-GS, intr_1_2_timing-GS, intr_2_0_timing, stat_irq_blocking
- MBC1 (12/13) : bits_bank1, bits_bank2, bits_mode, bits_ramg, ram_64kb, ram_256kb, rom_512kb, rom_1Mb,
  rom_2Mb, rom_4Mb, rom_8Mb, rom_16Mb
- MBC5 (8/8) : rom_512kb à rom_64Mb

### Ratés (26)
- Autres modèles, hors cible DMG ABC (9) : boot_div-dmg0, boot_div-S, boot_div2-S, boot_hwio-dmg0,
  boot_hwio-S, boot_regs-dmg0, boot_regs-mgb, boot_regs-sgb, boot_regs-sgb2
- PPU au point près (8) : intr_2_mode0_timing, intr_2_mode0_timing_sprites, intr_2_mode3_timing,
  intr_2_oam_ok_timing, lcdon_timing-GS, lcdon_write_timing-GS, stat_lyc_onoff, vblank_stat_intr-GS
  (PPU par ligne : mode 3 sans pénalités sprites/fenêtre, allumage LCD simplifié, pas de quirk LY=153)
- Série (1) : boot_sclk_align-dmgABCmgb (pas d'alignement sur l'horloge série)
- MBC1 multicarte (1) : multicart_rom_8Mb (MBC1M non détecté)
- MBC2 (7) : non pris en charge (hors brief)

## Game Boy Color (mode CGB)
Mode CGB si l'octet 0x143 de l'en-tête vaut 0x80 ou 0xC0 ; tests ci-dessous en mode CGB imposé
(`GameBoy::new_mode(rom, true)`).

| Test | Résultat |
|---|---|
| cgb-acid2 | réussi (0 pixel RGB différent de `reference.png`, conversion `(c << 3) \| (c >> 2)`, sans correction) |
| cpu_instrs (11/11) | réussi (en double vitesse : la ROM bascule via KEY1 + STOP) |
| instr_timing | réussi |
| cgb_sound (12/12 + ROM complète) | réussi (`blargg_cgb_sound`, `blargg_cgb_sound_complet`, `CGB_SOUND_KO_CONNUS` vide) |
| mooneye misc CGB (1/5) | boot_div-cgbABCDE réussi ; 4 ratés connus (`MOONEYE_CGB_KO_CONNUS`) |
| `gb-core/tests/cgb.rs` | banques WRAM/VRAM, palettes, HDMA général et HBlank (durée 8 cycles/bloc, ×2 en double vitesse), KEY1, RGB et correction, états |

Différences CGB de l'APU : RAM d'onde lisible à tout moment pendant la lecture, pas de corruption de la RAM
d'onde au redéclenchement, longueurs remises à zéro hors tension (et non inscriptibles).

### mooneye CGB ratés (4)
Ces ROM sont des ROM DMG (0x143 = 0) écrites pour un CGB en **mode compatibilité DMG**, non émulé
(hors brief) : exécutées en mode CGB, elles voient le matériel d'un jeu CGB.
- boot_regs-cgb : attend les registres du mode compatibilité (D=00, E=08, L=7C) ; on donne ceux d'un jeu CGB
  (A=11 F=80 B=00 C=00 D=FF E=56 H=00 L=0D, Pan Docs).
- boot_hwio-C, unused_hwio-C : attendent KEY1 (FF4D) verrouillé à 0xFF (mode compatibilité) ; en mode CGB il vaut 0x7E.
  FF00 et FF72-FF77 sont conformes.
- vblank_stat_intr-C : PPU au point près (même cause que vblank_stat_intr-GS).

DIV interne au démarrage en CGB = 0x2674, calé sur boot_div-cgbABCDE (seule valeur qui passe, par pas de 4).

## Performance
Release, i7-7700, cpu_instrs sans limite : ~3 300 images/s à l'étape 1 ; avec l'APU (étape 2a) ~3 000 images/s
(machine chargée, base mesurée en alternance à ~3 450), acid2 ~4 300 images/s.
Mesure : `target/release/gb-headless roms/blargg/cpu_instrs/cpu_instrs.gb --frames 3000 --bench`.

GBC (mesures en alternance avec le binaire d'avant GBC, machine chargée) :
- DMG : dmg-acid2 ~4 400 images/s (avant : ~4 600, −4 %), dmg_sound ~3 450 (avant : ~3 550, −3 %).
  Coût : écran RGB gris rempli à chaque ligne pour `frame_rgb` et pas PPU variable (4 ou 2 points).
- CGB : cpu_instrs en double vitesse ~2 350 images/s, cgb-acid2 ~4 150, cgb_sound ~3 300.
  (cpu_instrs démarre maintenant en CGB par son en-tête : sans `new_mode`, la mesure ci-dessus est en double vitesse.)
