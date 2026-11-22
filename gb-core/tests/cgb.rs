//! Matériel Game Boy Color sur un petit programme : banques WRAM/VRAM, palettes, HDMA général
//! et HBlank (durée comprise), double vitesse, sortie RGB et correction des couleurs,
//! sauvegarde d'état (version, mode, données modifiées sans panique).

use gb_core::{ErreurEtat, GameBoy, LARGEUR};

/// Résultats écrits en HRAM (FF80-FF8B) puis boucle infinie.
fn rom_cgb() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x143] = 0x80;
    #[rustfmt::skip]
    let prog: &[u8] = &[
        0x3E, 0x02, 0xE0, 0x70, // SVBK = 2
        0x3E, 0xAA, 0xEA, 0x00, 0xD0, // (D000) = AA
        0x3E, 0x03, 0xE0, 0x70, // SVBK = 3
        0x3E, 0xBB, 0xEA, 0x00, 0xD0,
        0x3E, 0x00, 0xE0, 0x70, // SVBK = 0 → banque 1
        0x3E, 0x11, 0xEA, 0x00, 0xD0,
        0x3E, 0x02, 0xE0, 0x70,
        0xFA, 0x00, 0xD0, 0xE0, 0x80, // FF80 = (D000) banque 2 = AA
        0x3E, 0x01, 0xE0, 0x4F, // VBK = 1
        0x3E, 0x55, 0xEA, 0x00, 0x80,
        0x3E, 0x00, 0xE0, 0x4F,
        0x3E, 0x66, 0xEA, 0x00, 0x80,
        0x3E, 0x01, 0xE0, 0x4F,
        0xFA, 0x00, 0x80, 0xE0, 0x81, // FF81 = (8000) banque 1 = 55
        0x3E, 0x00, 0xE0, 0x4F,
        0x3E, 0x80, 0xE0, 0x68, // BCPS = 80 (auto-incrément)
        0x3E, 0x1F, 0xE0, 0x69, // couleur 0 de la palette 0 : rouge
        0x3E, 0x00, 0xE0, 0x69,
        0xF0, 0x68, 0xE0, 0x82, // FF82 = BCPS = C2
        0x3E, 0x77, 0xEA, 0x00, 0xC0, // (C000) = 77
        0x3E, 0xC0, 0xE0, 0x51, 0x3E, 0x00, 0xE0, 0x52, // source C000
        0x3E, 0x81, 0xE0, 0x53, 0x3E, 0x00, 0xE0, 0x54, // destination 8100
        0x3E, 0x00, 0xE0, 0x55, // DMA général, 1 bloc
        0xF0, 0x55, 0xE0, 0x83, // FF83 = FF
        0xFA, 0x00, 0x81, 0xE0, 0x84, // FF84 = (8100) = 77
        0x3E, 0x99, 0xEA, 0x10, 0xC0, // (C010) = 99
        0x3E, 0xC0, 0xE0, 0x51, 0x3E, 0x10, 0xE0, 0x52, // source C010
        0x3E, 0x82, 0xE0, 0x53, 0x3E, 0x00, 0xE0, 0x54, // destination 8200
        0x3E, 0x81, 0xE0, 0x55, // DMA HBlank, 2 blocs
        0xF0, 0x55, 0xE0, 0x86, // FF86 = 01 (en cours)
        0xF0, 0x55, 0xFE, 0xFF, 0x20, 0xFA, // attente FF55 = FF
        0xFA, 0x00, 0x82, 0xE0, 0x87, // FF87 = (8200) = 99
        0xF0, 0x04, 0xE0, 0x88, // FF88 = DIV
        0x3E, 0xC0, 0xE0, 0x51, 0x3E, 0x00, 0xE0, 0x52,
        0x3E, 0x80, 0xE0, 0x53, 0x3E, 0x00, 0xE0, 0x54,
        0x3E, 0x7F, 0xE0, 0x55, // DMA général, 128 blocs : 1 024 cycles
        0xF0, 0x04, 0xE0, 0x89, // FF89 = DIV
        0x3E, 0x01, 0xE0, 0x4D, 0x10, 0x00, // KEY1 armé ; STOP → double vitesse
        0xF0, 0x4D, 0xE0, 0x85, // FF85 = KEY1 = FE
        0xF0, 0x04, 0xE0, 0x8A, // FF8A = DIV
        0x3E, 0x7F, 0xE0, 0x55, // DMA général, 128 blocs : 2 048 cycles en double vitesse
        0xF0, 0x04, 0xE0, 0x8B, // FF8B = DIV
        0x18, 0xFE, // boucle
    ];
    rom[0x100..0x103].copy_from_slice(&[0xC3, 0x50, 0x01]); // JP 0150 (après l'en-tête)
    rom[0x150..0x150 + prog.len()].copy_from_slice(prog);
    rom
}

fn lance() -> GameBoy {
    let mut gb = GameBoy::new(&rom_cgb()).unwrap();
    assert!(gb.is_cgb());
    for _ in 0..5 {
        gb.run_frame();
    }
    gb
}

#[test]
fn registres_banques_palettes_hdma_double_vitesse() {
    let gb = lance();
    let r: Vec<u8> = (0xFF80..=0xFF8B).map(|a| gb.peek(a)).collect();
    assert_eq!(r[0], 0xAA, "WRAM banque 2");
    assert_eq!(r[1], 0x55, "VRAM banque 1");
    assert_eq!(r[2], 0xC2, "BCPS auto-incrémenté");
    assert_eq!(r[3], 0xFF, "FF55 après DMA général");
    assert_eq!(r[4], 0x77, "copie DMA général");
    assert_eq!(r[6], 0x01, "FF55 pendant le DMA HBlank");
    assert_eq!(r[7], 0x99, "copie DMA HBlank");
    assert_eq!(r[5], 0xFE, "KEY1 en double vitesse");
    // DIV (+1 tous les 64 cycles) : 1 024 cycles de DMA = 16, le double en double vitesse.
    let d1 = r[9].wrapping_sub(r[8]);
    let d2 = r[11].wrapping_sub(r[10]);
    assert!((16..=17).contains(&d1), "durée DMA vitesse normale : {d1}");
    assert!((32..=33).contains(&d2), "durée DMA double vitesse : {d2}");
    assert_eq!(gb.peek(0xD000), 0xAA);
    assert_eq!(gb.peek(0xF000), 0xAA, "écho de la banque WRAM");
}

#[test]
fn image_rgb_et_correction() {
    let mut gb = lance();
    assert_eq!(gb.frame_rgb().len(), LARGEUR * 144 * 3);
    // Ligne 1 de la tuile 0 (tuile vide) : couleur 0 de la palette 0, rouge pur.
    let px = |gb: &GameBoy| gb.frame_rgb()[LARGEUR * 3..LARGEUR * 3 + 3].to_vec();
    assert_eq!(px(&gb), [0xFF, 0, 0]);
    gb.set_color_correction(true);
    gb.run_frame();
    assert_eq!(px(&gb), [201, 0, 46]);
    gb.set_color_correction(false);
    gb.run_frame();
    assert_eq!(px(&gb), [0xFF, 0, 0]);
}

#[test]
fn etat_cgb_aller_retour_version_et_mode() {
    let rom = rom_cgb();
    let mut reference = lance();
    let etat = reference.save_state();
    for _ in 0..30 {
        reference.run_frame();
    }
    let mut gb = GameBoy::new(&rom).unwrap();
    gb.load_state(&etat).unwrap();
    assert!(gb.save_state() == etat);
    for _ in 0..30 {
        gb.run_frame();
    }
    assert!(gb.frame_rgb() == reference.frame_rgb());
    assert!(gb.save_state() == reference.save_state());

    // Ancien format (version 1) refusé.
    let mut v1 = etat.clone();
    v1[4..6].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(gb.load_state(&v1), Err(ErreurEtat::Version(1)));

    // Même ROM en mode DMG imposé : état CGB refusé, console inchangée.
    let mut dmg = GameBoy::new_mode(&rom, false).unwrap();
    let avant = dmg.save_state();
    assert_eq!(dmg.load_state(&etat), Err(ErreurEtat::Corrompu("mode CGB")));
    assert!(dmg.save_state() == avant);
    assert!(gb.load_state(&avant).is_err());
}

#[test]
fn etat_cgb_modifie_sans_panique() {
    let rom = rom_cgb();
    let etat = lance().save_state();
    let mut x = 0x2545_F491_4F6C_DD1Du64;
    let mut alea = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for _ in 0..300 {
        let mut d = etat.clone();
        for _ in 0..1 + alea() % 8 {
            let i = 10 + (alea() as usize) % (d.len() - 10);
            d[i] = alea() as u8;
        }
        let mut g = GameBoy::new(&rom).unwrap();
        if g.load_state(&d).is_ok() {
            for _ in 0..3 {
                g.run_frame();
            }
        }
    }
}
