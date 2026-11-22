//! Sauvegardes d'état sur ROM de test publiques : état sauvé à l'image N puis rechargé dans une
//! console neuve → exécution identique à la référence (écran, série, son, état complet).
//! Mode imposé (DMG ou CGB) : cpu_instrs a l'octet CGB 0x80 dans son en-tête.
//! Sautent proprement si `roms/` manque : lancer `scripts/roms-test.sh`.

use gb_core::{ErreurEtat, GameBoy};
use std::path::Path;

fn rom(rel: &str) -> Option<Vec<u8>> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../roms")
        .join(rel);
    let d = std::fs::read(&p).ok();
    if d.is_none() {
        eprintln!(
            "SAUTÉ : {} absent (lancer scripts/roms-test.sh)",
            p.display()
        );
    }
    d
}

fn execute(gb: &mut GameBoy, images: u32, son: &mut Vec<f32>) {
    for _ in 0..images {
        gb.run_frame();
        gb.drain_samples(son);
    }
}

/// Référence N + M images d'un trait ; puis N images, état, console neuve, chargement, M images.
fn aller_retour(rel: &str, n: u32, m: u32, cgb: bool) {
    let Some(data) = rom(rel) else { return };
    let mut reference = GameBoy::new_mode(&data, cgb).unwrap();
    let mut son_ref = Vec::new();
    execute(&mut reference, n, &mut son_ref);
    let serie_n = reference.serial_output().len();
    let etat = reference.save_state();
    // CGB : 32 Kio de WRAM, 16 Kio de VRAM et l'image RGB (67,5 Kio) en plus.
    let max = if cgb { 160 * 1024 } else { 100 * 1024 };
    assert!(etat.len() < max, "{rel} : état de {} octets", etat.len());
    son_ref.clear();
    execute(&mut reference, m, &mut son_ref);

    let mut gb = GameBoy::new_mode(&data, cgb).unwrap();
    gb.load_state(&etat).unwrap();
    let mut son = Vec::new();
    execute(&mut gb, m, &mut son);
    assert!(gb.frame() == reference.frame(), "{rel} : écran différent");
    assert!(
        gb.frame_rgb() == reference.frame_rgb(),
        "{rel} : écran RGB différent"
    );
    assert_eq!(
        gb.serial_output(),
        &reference.serial_output()[serie_n..],
        "{rel} : sortie série différente"
    );
    assert!(son == son_ref, "{rel} : son différent");
    assert!(
        gb.save_state() == reference.save_state(),
        "{rel} : état final différent"
    );
}

#[test]
fn etat_cpu_instrs() {
    aller_retour("blargg/cpu_instrs/cpu_instrs.gb", 700, 1500, false);
}

/// Mode CGB, en double vitesse (cpu_instrs passe en double vitesse au démarrage).
#[test]
fn etat_cpu_instrs_cgb() {
    aller_retour("blargg/cpu_instrs/cpu_instrs.gb", 700, 1500, true);
}

#[test]
fn etat_cgb_acid2() {
    aller_retour("cgb-acid2/cgb-acid2.gbc", 3, 60, true);
}

#[test]
fn etat_dmg_acid2() {
    aller_retour("dmg-acid2/dmg-acid2.gb", 3, 60, false);
}

#[test]
fn etat_dmg_sound() {
    aller_retour("blargg/dmg_sound/dmg_sound.gb", 400, 800, false);
}

#[test]
fn etat_cgb_sound() {
    aller_retour("blargg/cgb_sound/cgb_sound.gb", 400, 800, true);
}

#[test]
fn etat_autre_rom_refuse() {
    let (Some(a), Some(b)) = (
        rom("blargg/cpu_instrs/cpu_instrs.gb"),
        rom("dmg-acid2/dmg-acid2.gb"),
    ) else {
        return;
    };
    let etat = GameBoy::new(&a).unwrap().save_state();
    let mut gb = GameBoy::new(&b).unwrap();
    let avant = gb.save_state();
    assert_eq!(gb.load_state(&etat), Err(ErreurEtat::AutreRom));
    assert!(gb.save_state() == avant);
}
