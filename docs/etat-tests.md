# État des ROM de test (2026-11-22, étape 1 : cœur DMG)

ROM téléchargées par `scripts/roms-test.sh` dans `roms/` (ignoré par git), vérifiées par `cargo test`.

## Blargg et dmg-acid2 : tout vert
| Test | Résultat |
|---|---|
| cpu_instrs (11/11) | réussi (sortie série « Passed ») |
| instr_timing | réussi |
| mem_timing (3/3) | réussi |
| mem_timing-2 (3/3) | réussi (statut en $A000) |
| dmg-acid2 | réussi (0 pixel différent de `reference-dmg.png`) |

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

## Performance
Release, i7-7700, cpu_instrs sans limite : ~3 300 images/s (acid2 : ~4 400 images/s).
Mesure : `target/release/gb-headless roms/blargg/cpu_instrs/cpu_instrs.gb --frames 3000 --bench`.
