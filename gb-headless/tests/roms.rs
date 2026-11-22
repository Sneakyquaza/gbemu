//! Tests par ROM de test publiques (blargg, mooneye-test-suite, dmg-acid2, cgb-acid2).
//! Ils sautent proprement si `roms/` manque : lancer `scripts/roms-test.sh`.
//! Les tests DMG imposent le mode DMG (cpu_instrs, instr_timing et mem_timing ont l'octet
//! CGB 0x80 dans leur en-tête) ; les tests `_cgb` imposent le mode CGB.

use gb_core::GameBoy;
use gb_headless::{
    blargg_memory_status, blargg_memory_text, mooneye_passed, read_png_rgb, read_png_shades,
    run_until, serial_contains,
};
use std::path::{Path, PathBuf};

fn roms() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../roms")
}

/// Charge une ROM de test dans le mode imposé, ou `None` (avec message) si elle n'est pas téléchargée.
fn charge(rel: &str, cgb: bool) -> Option<GameBoy> {
    let p = roms().join(rel);
    let Ok(data) = std::fs::read(&p) else {
        eprintln!(
            "SAUTÉ : {} absent (lancer scripts/roms-test.sh)",
            p.display()
        );
        return None;
    };
    Some(GameBoy::new_mode(&data, cgb).expect("ROM de test illisible"))
}

fn blargg_serie(rel: &str, max_frames: u32, cgb: bool) {
    let Some(mut gb) = charge(rel, cgb) else {
        return;
    };
    run_until(&mut gb, max_frames, |gb| {
        serial_contains(gb, "Passed") || serial_contains(gb, "Failed")
    });
    let out = String::from_utf8_lossy(gb.serial_output());
    assert!(out.contains("Passed"), "{rel} :\n{out}");
}

#[test]
fn blargg_cpu_instrs() {
    blargg_serie("blargg/cpu_instrs/cpu_instrs.gb", 6000, false);
}

#[test]
fn blargg_cpu_instrs_cgb() {
    blargg_serie("blargg/cpu_instrs/cpu_instrs.gb", 6000, true);
}

#[test]
fn blargg_instr_timing() {
    blargg_serie("blargg/instr_timing/instr_timing.gb", 600, false);
}

#[test]
fn blargg_instr_timing_cgb() {
    blargg_serie("blargg/instr_timing/instr_timing.gb", 600, true);
}

#[test]
fn blargg_mem_timing() {
    blargg_serie("blargg/mem_timing/mem_timing.gb", 600, false);
}

#[test]
fn blargg_mem_timing_2() {
    let Some(mut gb) = charge("blargg/mem_timing-2/mem_timing.gb", false) else {
        return;
    };
    run_until(&mut gb, 600, |gb| blargg_memory_status(gb).is_some());
    assert_eq!(
        blargg_memory_status(&gb),
        Some(0),
        "{}",
        blargg_memory_text(&gb)
    );
}

#[test]
fn dmg_acid2() {
    let Some(mut gb) = charge("dmg-acid2/dmg-acid2.gb", false) else {
        return;
    };
    run_until(&mut gb, 120, |gb| gb.hit_ld_b_b());
    assert!(gb.hit_ld_b_b(), "dmg-acid2 ne s'est pas terminé");
    let reference = read_png_shades(&roms().join("dmg-acid2/reference-dmg.png")).unwrap();
    let diff = gb
        .frame()
        .iter()
        .zip(&reference)
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        diff, 0,
        "dmg-acid2 : {diff} pixels différents de la référence"
    );
}

#[test]
fn cgb_acid2() {
    let Some(mut gb) = charge("cgb-acid2/cgb-acid2.gbc", true) else {
        return;
    };
    run_until(&mut gb, 120, |gb| gb.hit_ld_b_b());
    assert!(gb.hit_ld_b_b(), "cgb-acid2 ne s'est pas terminé");
    // Référence en conversion linéaire (c << 3) | (c >> 2), sans correction des couleurs.
    let reference = read_png_rgb(&roms().join("cgb-acid2/reference.png")).unwrap();
    let diff = gb
        .frame_rgb()
        .chunks(3)
        .zip(reference.chunks(3))
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        diff, 0,
        "cgb-acid2 : {diff} pixels différents de la référence"
    );
}

/// Tests mooneye qui échouent encore (ou visent un autre modèle que le DMG ABC).
/// Voir docs/etat-tests.md.
const MOONEYE_KO_CONNUS: &[&str] = &[
    // Autres modèles (DMG0, MGB, SGB, CGB/AGB « S ») : hors cible.
    "acceptance/boot_div2-S.gb",
    "acceptance/boot_div-dmg0.gb",
    "acceptance/boot_div-S.gb",
    "acceptance/boot_hwio-dmg0.gb",
    "acceptance/boot_hwio-S.gb",
    "acceptance/boot_regs-dmg0.gb",
    "acceptance/boot_regs-mgb.gb",
    "acceptance/boot_regs-sgb2.gb",
    "acceptance/boot_regs-sgb.gb",
    // PPU par ligne : pas de durée de mode 3 exacte ni de démarrage LCD au point près.
    "acceptance/ppu/intr_2_mode0_timing.gb",
    "acceptance/ppu/intr_2_mode0_timing_sprites.gb",
    "acceptance/ppu/intr_2_mode3_timing.gb",
    "acceptance/ppu/intr_2_oam_ok_timing.gb",
    "acceptance/ppu/lcdon_timing-GS.gb",
    "acceptance/ppu/lcdon_write_timing-GS.gb",
    "acceptance/ppu/stat_lyc_onoff.gb",
    "acceptance/ppu/vblank_stat_intr-GS.gb",
    // Série : pas d'alignement sur l'horloge série interne.
    "acceptance/serial/boot_sclk_align-dmgABCmgb.gb",
    // MBC1 multicarte (MBC1M) non détecté ; MBC2 non pris en charge.
    "emulator-only/mbc1/multicart_rom_8Mb.gb",
    "emulator-only/mbc2/bits_ramg.gb",
    "emulator-only/mbc2/bits_romb.gb",
    "emulator-only/mbc2/bits_unused.gb",
    "emulator-only/mbc2/ram.gb",
    "emulator-only/mbc2/rom_1Mb.gb",
    "emulator-only/mbc2/rom_2Mb.gb",
    "emulator-only/mbc2/rom_512kb.gb",
];

fn liste_gb(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            liste_gb(&p, out);
        } else if p.extension().is_some_and(|x| x == "gb") {
            out.push(p);
        }
    }
}

/// Exécute un test mooneye dans le mode imposé : réussi si les registres Fibonacci sont
/// présents à `LD B,B`.
fn mooneye(p: &Path, cgb: bool) -> bool {
    let Ok(data) = std::fs::read(p) else {
        return false;
    };
    let Ok(mut gb) = GameBoy::new_mode(&data, cgb) else {
        return false;
    };
    run_until(&mut gb, 1200, |gb| gb.hit_ld_b_b());
    gb.hit_ld_b_b() && mooneye_passed(&gb)
}

#[test]
fn mooneye_acceptance_et_mbc1() {
    let base = roms().join("mooneye");
    if !base.is_dir() {
        eprintln!(
            "SAUTÉ : {} absent (lancer scripts/roms-test.sh)",
            base.display()
        );
        return;
    }
    let mut fichiers = Vec::new();
    liste_gb(&base.join("acceptance"), &mut fichiers);
    liste_gb(&base.join("emulator-only"), &mut fichiers);
    fichiers.sort();

    let mut regressions = Vec::new();
    let (mut ok, mut ko) = (0, 0);
    for p in &fichiers {
        let rel = p
            .strip_prefix(&base)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let passe = mooneye(p, false);
        let connu_ko = MOONEYE_KO_CONNUS.contains(&rel.as_str());
        if passe {
            ok += 1;
            if connu_ko {
                eprintln!("NOUVEAU SUCCÈS : {rel} (à retirer de MOONEYE_KO_CONNUS)");
            }
        } else {
            ko += 1;
            if !connu_ko {
                regressions.push(rel);
            }
        }
    }
    eprintln!("mooneye : {ok} réussis, {ko} ratés");
    assert!(
        regressions.is_empty(),
        "régressions mooneye : {regressions:#?}"
    );
}

/// Tests mooneye propres au CGB (dossier misc/, ROM DMG exécutées en mode CGB imposé).
const MOONEYE_CGB: &[&str] = &[
    "misc/boot_div-cgbABCDE.gb",
    "misc/boot_hwio-C.gb",
    "misc/boot_regs-cgb.gb",
    "misc/bits/unused_hwio-C.gb",
    "misc/ppu/vblank_stat_intr-C.gb",
];

/// Parmi `MOONEYE_CGB`, ceux qui échouent encore (voir docs/etat-tests.md).
const MOONEYE_CGB_KO_CONNUS: &[&str] = &[
    // Écrits pour un CGB qui exécute une ROM DMG (mode compatibilité, non émulé) : registres
    // post-boot du mode compatibilité (D=00, E=08, L=7C) et KEY1 verrouillé (FF4D lu 0xFF).
    "misc/boot_hwio-C.gb",
    "misc/boot_regs-cgb.gb",
    "misc/bits/unused_hwio-C.gb",
    // PPU par ligne : pas de quirk LY=153 / STAT au point près (comme vblank_stat_intr-GS).
    "misc/ppu/vblank_stat_intr-C.gb",
];

#[test]
fn mooneye_cgb() {
    let base = roms().join("mooneye");
    if !base.is_dir() {
        eprintln!(
            "SAUTÉ : {} absent (lancer scripts/roms-test.sh)",
            base.display()
        );
        return;
    }
    let mut regressions = Vec::new();
    for &rel in MOONEYE_CGB {
        let passe = mooneye(&base.join(rel), true);
        let connu_ko = MOONEYE_CGB_KO_CONNUS.contains(&rel);
        if passe && connu_ko {
            eprintln!("NOUVEAU SUCCÈS : {rel} (à retirer de MOONEYE_CGB_KO_CONNUS)");
        } else if !passe && !connu_ko {
            regressions.push(rel);
        }
    }
    assert!(
        regressions.is_empty(),
        "régressions mooneye CGB : {regressions:#?}"
    );
}

/// Sous-tests blargg dmg_sound qui échouent encore (voir docs/etat-tests.md).
const DMG_SOUND_KO_CONNUS: &[&str] = &[];

/// Résultat d'un test blargg à sortie mémoire : `None` si réussi, sinon le texte affiché.
fn blargg_memoire(p: &Path, max_frames: u32, cgb: bool) -> Option<String> {
    let data = std::fs::read(p).ok()?;
    let mut gb = GameBoy::new_mode(&data, cgb).expect("ROM de test illisible");
    run_until(&mut gb, max_frames, |gb| blargg_memory_status(gb).is_some());
    (blargg_memory_status(&gb) != Some(0)).then(|| blargg_memory_text(&gb))
}

#[test]
fn blargg_dmg_sound_complet() {
    let p = roms().join("blargg/dmg_sound/dmg_sound.gb");
    if !p.is_file() {
        eprintln!(
            "SAUTÉ : {} absent (lancer scripts/roms-test.sh)",
            p.display()
        );
        return;
    }
    if let Some(t) = blargg_memoire(&p, 3600, false) {
        panic!("dmg_sound :\n{t}");
    }
}

#[test]
fn blargg_dmg_sound() {
    let base = roms().join("blargg/dmg_sound/rom_singles");
    let Ok(rd) = std::fs::read_dir(&base) else {
        eprintln!(
            "SAUTÉ : {} absent (lancer scripts/roms-test.sh)",
            base.display()
        );
        return;
    };
    let mut fichiers: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    fichiers.sort();
    let mut regressions = Vec::new();
    for p in &fichiers {
        let nom = p.file_name().unwrap().to_string_lossy().into_owned();
        let echec = blargg_memoire(p, 1200, false);
        let connu_ko = DMG_SOUND_KO_CONNUS.contains(&nom.as_str());
        match echec {
            None if connu_ko => {
                eprintln!("NOUVEAU SUCCÈS : {nom} (à retirer de DMG_SOUND_KO_CONNUS)")
            }
            Some(t) if !connu_ko => regressions.push(format!("{nom} : {t}")),
            _ => {}
        }
    }
    assert!(
        regressions.is_empty(),
        "régressions dmg_sound : {regressions:#?}"
    );
}

/// Sous-tests blargg cgb_sound qui échouent encore (voir docs/etat-tests.md).
const CGB_SOUND_KO_CONNUS: &[&str] = &[];

#[test]
fn blargg_cgb_sound_complet() {
    let p = roms().join("blargg/cgb_sound/cgb_sound.gb");
    if !p.is_file() {
        eprintln!(
            "SAUTÉ : {} absent (lancer scripts/roms-test.sh)",
            p.display()
        );
        return;
    }
    if let Some(t) = blargg_memoire(&p, 3600, true) {
        panic!("cgb_sound :\n{t}");
    }
}

#[test]
fn blargg_cgb_sound() {
    let base = roms().join("blargg/cgb_sound/rom_singles");
    let Ok(rd) = std::fs::read_dir(&base) else {
        eprintln!(
            "SAUTÉ : {} absent (lancer scripts/roms-test.sh)",
            base.display()
        );
        return;
    };
    let mut fichiers: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    fichiers.sort();
    let mut regressions = Vec::new();
    for p in &fichiers {
        let nom = p.file_name().unwrap().to_string_lossy().into_owned();
        let echec = blargg_memoire(p, 1200, true);
        let connu_ko = CGB_SOUND_KO_CONNUS.contains(&nom.as_str());
        match echec {
            None if connu_ko => {
                eprintln!("NOUVEAU SUCCÈS : {nom} (à retirer de CGB_SOUND_KO_CONNUS)")
            }
            Some(t) if !connu_ko => regressions.push(format!("{nom} : {t}")),
            _ => {}
        }
    }
    assert!(
        regressions.is_empty(),
        "régressions cgb_sound : {regressions:#?}"
    );
}
