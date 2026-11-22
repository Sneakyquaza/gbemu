//! Sortie son : cadence des échantillons, valeurs bornées, silence sans clic au démarrage.

use gb_core::{GameBoy, CYCLES_PAR_IMAGE};

/// ROM minimale : 32 Kio de NOP (cartouche sans MBC).
fn rom_nop() -> Vec<u8> {
    vec![0; 0x8000]
}

#[test]
fn cadence_et_bornes() {
    for hz in [48_000u32, 44_100, 22_050] {
        let mut gb = GameBoy::new(&rom_nop()).unwrap();
        gb.set_sample_rate(hz);
        // Première image partielle (démarrage hors VBlank) : non comptée.
        gb.run_frame();
        let mut debut = Vec::new();
        gb.drain_samples(&mut debut);
        // Canal 1 actif à volume nul après la boot ROM : silence complet, sans clic.
        assert!(
            debut.iter().all(|x| x.abs() < 1e-3),
            "{hz} Hz : clic au démarrage"
        );
        let mut son = Vec::new();
        for _ in 0..120 {
            gb.run_frame();
            gb.drain_samples(&mut son);
        }
        let attendu = 120.0 * CYCLES_PAR_IMAGE as f64 * hz as f64 / 1_048_576.0;
        let paires = (son.len() / 2) as f64;
        assert!(
            (paires - attendu).abs() <= 2.0,
            "{hz} Hz : {paires} au lieu de {attendu}"
        );
        assert!(son.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
        // Canal 1 actif à volume nul après la boot ROM : silence complet.
        assert!(
            son.iter().all(|x| x.abs() < 1e-3),
            "{hz} Hz : clic au démarrage"
        );
    }
}

#[test]
fn tampon_borne_sans_lecture() {
    let mut gb = GameBoy::new(&rom_nop()).unwrap();
    for _ in 0..200 {
        gb.run_frame();
    }
    let mut son = Vec::new();
    gb.drain_samples(&mut son);
    assert!(son.len() <= 2 * 48_000);
}
