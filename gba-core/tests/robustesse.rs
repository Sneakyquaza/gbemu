//! Aucune panique sur ROM vide, tronquée ou aléatoire ; BIOS de mauvaise taille refusé.

use gba_core::{ErreurChargement, Gba};

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

#[test]
fn rom_vide_refusee() {
    assert!(matches!(
        Gba::new(&[], None),
        Err(ErreurChargement::RomVide)
    ));
}

#[test]
fn rom_trop_grande_refusee() {
    assert!(Gba::new(&vec![0; 32 * 1024 * 1024 + 1], None).is_err());
}

#[test]
fn bios_mauvaise_taille_refuse() {
    assert!(matches!(
        Gba::new(&[0; 4], Some(&[0; 100])),
        Err(ErreurChargement::BiosTaille(100))
    ));
}

#[test]
fn roms_aleatoires_sans_panique() {
    let mut r = Alea(0x9E37_79B9_7F4A_7C15);
    let tailles = [1usize, 3, 0xC0, 0x1000, 0x10001, 0x40000];
    for (n, &taille) in tailles.iter().enumerate() {
        for essai in 0..4 {
            let rom = r.octets(taille);
            let mut gba = Gba::new(&rom, None).unwrap();
            for i in 0..20 {
                gba.set_keys((r.next() & 0x3FF) as u16);
                gba.run_frame();
                assert!(
                    gba.frame().len() == 240 * 160 * 3,
                    "rom {n}/{essai} image {i}"
                );
            }
        }
    }
}

/// ROM aléatoire avec en tête un petit programme qui programme DMA, timers et IRQ au hasard
/// (valeurs aléatoires écrites dans toute la zone d'E/S) : aucune panique.
#[test]
fn ecritures_es_aleatoires_sans_panique() {
    let mut r = Alea(0x0123_4567_89AB_CDEF);
    for _ in 0..8 {
        // Boucle : r0 = 0x04000000 + (aléa & 0x3FE) ; r1 = aléa ; strh r1, [r0] ; …
        let mut rom = Vec::new();
        for _ in 0..200 {
            let off = (r.next() & 0x3FE) as u32;
            let v = r.next() as u32 & 0xFF;
            let mots = [
                0xE3A0_0301u32,                                       // mov r0, #0x04000000
                0xE280_0000 | (off & 0xFF),                           // add r0, r0, #off & 0xFF
                0xE280_0C00 | (off >> 8), // add r0, r0, #(off >> 8) << 8
                0xE3A0_1000 | v,          // mov r1, #v
                0xE181_1401 | ((r.next() as u32 & 0xF) << 8) & 0xF00, // orr r1, r1, r1, lsl #n
                0xE1C0_10B0,              // strh r1, [r0]
            ];
            for m in mots {
                rom.extend_from_slice(&m.to_le_bytes());
            }
        }
        rom.extend_from_slice(&0xEAFF_FFFEu32.to_le_bytes()); // b .
        let mut gba = Gba::new(&rom, None).unwrap();
        for _ in 0..30 {
            gba.run_frame();
        }
    }
}
