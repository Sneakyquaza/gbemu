//! Aucune panique sur ROM tronquée ou aléatoire, ni sur sauvegarde aléatoire.

use gb_core::GameBoy;

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
