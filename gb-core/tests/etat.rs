//! Sauvegardes d'état : aller-retour exact, refus d'une autre ROM, aucune panique sur données
//! aléatoires, tronquées ou modifiées.

use gb_core::{ErreurEtat, GameBoy};
use std::time::Instant;

/// Générateur pseudo-aléatoire xorshift64 (déterministe).
struct Alea(u64);

impl Alea {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Petit programme : boucle qui écrit dans la RAM cartouche, le son et l'écran.
/// MBC3 + RTC + 32 Kio de RAM batterie (type 0x10).
fn rom_mbc3() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x147] = 0x10;
    rom[0x149] = 3;
    let prog: &[u8] = &[
        0x3E, 0x0A, 0xEA, 0x00, 0x00, // LD A,0A ; LD (0000),A : RAM active
        0x3E, 0x80, 0xE0, 0x26, // LD A,80 ; LDH (26),A : son allumé
        0x3E, 0xF0, 0xE0, 0x12, // LD A,F0 ; LDH (12),A : volume max
        0x3E, 0x87, 0xE0, 0x14, // LD A,87 ; LDH (14),A : déclenchement canal 1
        0x21, 0x00, 0xA0, // LD HL,A000
        // boucle (0x0114) :
        0x3C, // INC A
        0x22, // LD (HL+),A
        0xE0, 0x13, // LDH (13),A : fréquence
        0xCB, 0x74, // BIT 6,H
        0x28, 0xF8, // JR Z,boucle
        0x21, 0x00, 0xA0, // LD HL,A000
        0x18, 0xF3, // JR boucle
    ];
    rom[0x100..0x100 + prog.len()].copy_from_slice(prog);
    rom
}

fn execute(gb: &mut GameBoy, images: u32, son: &mut Vec<f32>) {
    for _ in 0..images {
        gb.run_frame();
        gb.drain_samples(son);
    }
}

#[test]
fn aller_retour_exact() {
    let rom = rom_mbc3();
    let mut reference = GameBoy::new(&rom).unwrap();
    let mut son_ref = Vec::new();
    execute(&mut reference, 30, &mut son_ref);
    let etat = reference.save_state();
    son_ref.clear();
    execute(&mut reference, 60, &mut son_ref);

    let mut gb = GameBoy::new(&rom).unwrap();
    gb.load_state(&etat).unwrap();
    assert!(
        gb.save_state() == etat,
        "relecture puis sauvegarde différente"
    );
    let mut son = Vec::new();
    execute(&mut gb, 60, &mut son);
    assert!(gb.frame() == reference.frame());
    assert!(son == son_ref, "son différent");
    assert!(son.iter().any(|x| x.abs() > 0.01), "son inactif");
    assert!(gb.save_state() == reference.save_state());
    assert!(gb.export_save() == reference.export_save());
}

#[test]
fn autre_rom_refusee() {
    let etat = GameBoy::new(&rom_mbc3()).unwrap().save_state();
    let mut autre = rom_mbc3();
    autre[0x7FFF] = 1;
    let mut gb = GameBoy::new(&autre).unwrap();
    assert_eq!(gb.load_state(&etat), Err(ErreurEtat::AutreRom));
}

#[test]
fn donnees_aleatoires_tronquees_ou_modifiees_sans_panique() {
    let rom = rom_mbc3();
    let mut gb = GameBoy::new(&rom).unwrap();
    gb.run_frame();
    let etat = gb.save_state();
    let mut r = Alea(0x9E37_79B9_7F4A_7C15);

    // Octets aléatoires (avec ou sans en-tête valide) : toujours refusés, état intact.
    for i in 0..200 {
        let n = (r.next() % 200_000) as usize;
        let mut d: Vec<u8> = (0..n).map(|_| r.next() as u8).collect();
        if i % 2 == 0 && n >= 10 {
            d[..10].copy_from_slice(&etat[..10]);
        }
        assert!(gb.load_state(&d).is_err());
        assert!(gb.save_state() == etat, "état modifié par un refus");
    }
    // Troncatures et octet en trop.
    for n in (0..etat.len()).step_by(97).chain([etat.len() - 1]) {
        assert!(gb.load_state(&etat[..n]).is_err(), "tronqué à {n}");
    }
    let mut long = etat.clone();
    long.push(0);
    assert_eq!(gb.load_state(&long), Err(ErreurEtat::Taille));
    assert!(gb.save_state() == etat);

    // Octets modifiés après l'en-tête : refusés ou acceptés, puis exécution sans panique.
    for _ in 0..300 {
        let mut d = etat.clone();
        for _ in 0..1 + r.next() % 8 {
            let i = 10 + (r.next() as usize) % (d.len() - 10);
            d[i] = r.next() as u8;
        }
        let mut g = GameBoy::new(&rom).unwrap();
        if g.load_state(&d).is_ok() {
            for _ in 0..3 {
                g.run_frame();
            }
            let mut son = Vec::new();
            g.drain_samples(&mut son);
            assert!(son.iter().all(|x| x.is_finite()));
        }
    }
}

#[test]
fn taille_et_vitesse() {
    let mut gb = GameBoy::new(&vec![0u8; 0x8000]).unwrap();
    gb.run_frame();
    let etat = gb.save_state();
    assert!(etat.len() < 48 * 1024, "{} octets", etat.len());
    let n = 2000;
    let t = Instant::now();
    let mut total = 0;
    for _ in 0..n {
        total += std::hint::black_box(gb.save_state()).len();
    }
    let sauve = t.elapsed().as_secs_f64() / n as f64 * 1e6;
    let t = Instant::now();
    for _ in 0..n {
        gb.load_state(&etat).unwrap();
    }
    let charge = t.elapsed().as_secs_f64() / n as f64 * 1e6;
    eprintln!(
        "état {} octets : sauvegarde {sauve:.1} µs, chargement {charge:.1} µs",
        total / n
    );
    // Large marge (machine chargée, tests en parallèle) ; la cible est < 50 µs.
    assert!(sauve < 500.0, "sauvegarde {sauve:.1} µs");
}
