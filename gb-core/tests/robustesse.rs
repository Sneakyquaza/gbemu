//! Aucune panique sur ROM tronquée ou aléatoire, ni sur sauvegarde aléatoire.

use gb_core::{ErreurEtat, GameBoy};

/// Générateur pseudo-aléatoire xorshift64 (déterministe).
struct Alea(u64);

impl Alea {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn octets(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
}

/// Types de cartouche pris en charge (MBC0/1/3/5, avec RAM, batterie, RTC).
const TYPES: [u8; 15] = [
    0x00, 0x01, 0x02, 0x03, 0x08, 0x09, 0x0F, 0x10, 0x11, 0x12, 0x13, 0x19, 0x1A, 0x1B, 0x1E,
];

#[test]
fn rom_vide_refusee() {
    assert!(GameBoy::new(&[]).is_err());
}

#[test]
fn rom_trop_grande_refusee() {
    assert!(GameBoy::new(&vec![0; 8 * 1024 * 1024 + 1]).is_err());
}

#[test]
fn roms_aleatoires_sans_panique() {
    let mut r = Alea(0x9E37_79B9_7F4A_7C15);
    let tailles = [1usize, 0x100, 0x14F, 0x150, 0x3FFF, 0x8000, 0x9001, 0x40000];
    for (n, &taille) in tailles.iter().enumerate() {
        for &t in &TYPES {
            let mut rom = r.octets(taille);
            if rom.len() > 0x149 {
                rom[0x147] = t;
                rom[0x149] = (r.next() % 6) as u8;
            }
            let Ok(mut gb) = GameBoy::new(&rom) else {
                continue;
            };
            let n_save = (r.next() % 0x20100) as usize;
            gb.import_save(&r.octets(n_save));
            for i in 0..20 {
                gb.set_buttons(if i % 2 == 0 { r.next() as u8 } else { 0 });
                gb.run_frame();
            }
            let _ = gb.export_save();
            assert_eq!(gb.frame().len(), 160 * 144, "taille {n}");
        }
    }
}

#[test]
fn types_inconnus_refuses_sans_panique() {
    let mut r = Alea(42);
    for _ in 0..200 {
        let rom = r.octets(0x8000);
        if let Ok(mut gb) = GameBoy::new(&rom) {
            gb.run_frame();
        }
    }
}

#[test]
fn sauvegarde_aller_retour() {
    let mut rom = vec![0u8; 0x8000];
    rom[0x147] = 0x10; // MBC3 + RTC + RAM + batterie
    rom[0x149] = 3; // 32 Kio
    let mut gb = GameBoy::new(&rom).unwrap();
    let mut save = gb.export_save().expect("batterie");
    assert_eq!(save.len(), 0x8000 + 48);
    save[0] = 0x5A;
    save[0x7FFF] = 0xA5;
    gb.import_save(&save);
    let relue = gb.export_save().unwrap();
    assert_eq!(&relue[..0x8000], &save[..0x8000]);
}

/// ROM de test publique (`roms/`), `None` si absente (lancer `scripts/roms-test.sh`).
fn rom_test(rel: &str) -> Option<Vec<u8>> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
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

/// Petite ROM synthétique (toujours disponible) : son allumé, boucle d'écriture en RAM.
fn rom_synthetique(cgb: bool) -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x143] = if cgb { 0x80 } else { 0 };
    rom[0x147] = 0x10; // MBC3 + RTC + RAM + batterie
    rom[0x149] = 3;
    let prog: &[u8] = &[
        0x3E, 0x0A, 0xEA, 0x00, 0x00, // RAM active
        0x3E, 0x80, 0xE0, 0x26, // son allumé
        0x3E, 0xF0, 0xE0, 0x12, 0x3E, 0x87, 0xE0, 0x14, // canal 1 déclenché
        0x21, 0x00, 0xA0, // LD HL,A000
        0x3C, 0x22, 0xE0, 0x13, // INC A ; LD (HL+),A ; LDH (13),A
        0xCB, 0x74, 0x28, 0xF8, // BIT 6,H ; JR Z
        0x21, 0x00, 0xA0, 0x18, 0xF3, // LD HL,A000 ; JR
    ];
    rom[0x100..0x100 + prog.len()].copy_from_slice(prog);
    rom
}

/// Décalage de `Bus::points` dans un état : en-tête (10), CPU (18), mode CGB (1), WRAM, HRAM.
fn decalage_points(cgb: bool) -> usize {
    10 + 18 + 1 + if cgb { 0x8000 } else { 0x2000 } + 0x7F
}

/// Audit 22/11 (I1) : cpu_instrs (en-tête CGB), octet 32929 (octet 5 de `points`) 0x00 → 0x1E.
/// L'état était accepté, puis `run_frame` rattrapait ~8·10¹² cycles de son (gel).
#[test]
fn compteur_de_cycles_abime_refuse() {
    let Some(rom) = rom_test("blargg/cpu_instrs/cpu_instrs.gb") else {
        return;
    };
    let mut gb = GameBoy::new(&rom).unwrap();
    assert!(gb.is_cgb());
    for _ in 0..10 {
        gb.run_frame();
    }
    let etat = gb.save_state();
    assert_eq!(decalage_points(true) + 5, 32929);
    let mut d = etat.clone();
    d[32929] = 0x1E;
    assert_eq!(gb.load_state(&d), Err(ErreurEtat::Corrompu("cycles")));
    assert!(gb.save_state() == etat, "état modifié par un refus");
}

/// Octets hauts de `points` et `apu_points` (écart ≥ 2²⁴ points) : toujours refusés.
#[test]
fn octets_hauts_des_compteurs_refuses() {
    for cgb in [false, true] {
        let rom = rom_synthetique(cgb);
        let mut gb = GameBoy::new(&rom).unwrap();
        for _ in 0..5 {
            gb.run_frame();
        }
        let etat = gb.save_state();
        let p = decalage_points(cgb);
        for champ in [p, p + 8] {
            for octet in 3..8 {
                let mut d = etat.clone();
                d[champ + octet] ^= 0xFF;
                assert_eq!(
                    gb.load_state(&d),
                    Err(ErreurEtat::Corrompu("cycles")),
                    "cgb {cgb}, champ {champ}, octet {octet}"
                );
            }
        }
        assert!(gb.save_state() == etat);
    }
}

/// États modifiés au hasard (1 à 8 octets) : chargés ou refusés (console intacte), puis
/// `run_frame` rend toujours la main en moins de 50 ms. Un chien de garde signale tout gel.
/// Fuzz plus long : `GBEMU_FUZZ_SECONDES=120 cargo test -p gb-core --test robustesse etats_modifies`.
#[test]
fn etats_modifies_charges_ou_refuses_en_temps_borne() {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    let secondes: Option<u64> = std::env::var("GBEMU_FUZZ_SECONDES")
        .ok()
        .and_then(|s| s.parse().ok());
    let mut roms: Vec<(String, Vec<u8>)> = vec![
        ("synthétique DMG".into(), rom_synthetique(false)),
        ("synthétique CGB".into(), rom_synthetique(true)),
    ];
    for rel in [
        "blargg/cpu_instrs/cpu_instrs.gb",
        "cgb-acid2/cgb-acid2.gbc",
        "dmg-acid2/dmg-acid2.gb",
    ] {
        if let Some(rom) = rom_test(rel) {
            roms.push((rel.into(), rom));
        }
    }
    let (tx, rx) = mpsc::channel::<String>();
    let travail = std::thread::spawn(move || {
        let debut = Instant::now();
        let mut r = Alea(0x927E_7DBB_9121_BFE0);
        let mut total = 0u64;
        let mut acceptes = 0u64;
        let mut pire = (Duration::ZERO, String::new());
        'fin: loop {
            for (nom, rom) in &roms {
                let mut gb = GameBoy::new(rom).unwrap();
                for _ in 0..1 + r.next() % 40 {
                    gb.run_frame();
                }
                let etat = gb.save_state();
                for _ in 0..100 {
                    let graine = r.0;
                    let mut d = etat.clone();
                    for _ in 0..1 + r.next() % 8 {
                        let i = 10 + (r.next() as usize) % (d.len() - 10);
                        d[i] = r.next() as u8;
                    }
                    tx.send(format!("{nom}, graine {graine:#x}")).unwrap();
                    total += 1;
                    if gb.load_state(&d).is_ok() {
                        acceptes += 1;
                        for _ in 0..2 {
                            let t = Instant::now();
                            gb.run_frame();
                            let dt = t.elapsed();
                            if dt > pire.0 {
                                pire = (dt, format!("{nom}, graine {graine:#x}"));
                            }
                            assert!(
                                dt < Duration::from_millis(50),
                                "{nom}, graine {graine:#x} : run_frame {dt:?}"
                            );
                        }
                        let mut son = Vec::new();
                        gb.drain_samples(&mut son);
                        assert!(
                            son.iter().all(|x| x.is_finite()),
                            "{nom}, graine {graine:#x} : son"
                        );
                        gb.load_state(&etat).unwrap();
                    } else {
                        assert!(gb.save_state() == etat, "{nom} : état modifié par un refus");
                    }
                }
                let fini = match secondes {
                    Some(s) => debut.elapsed().as_secs() >= s,
                    None => total >= 10_000,
                };
                if fini {
                    break 'fin;
                }
            }
        }
        eprintln!(
            "{total} états modifiés, {acceptes} acceptés, pire run_frame {:?} ({}), {:?}",
            pire.0,
            pire.1,
            debut.elapsed()
        );
    });
    let mut dernier = String::from("démarrage");
    loop {
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(m) => dernier = m,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => panic!("gel : {dernier}"),
        }
    }
    travail.join().unwrap();
}
