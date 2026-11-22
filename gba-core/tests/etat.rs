//! Sauvegardes d'état sans ROM externe : refus propres, aucune panique sur état abîmé.

use gba_core::{ErreurEtat, Gba};

struct Alea(u64);

impl Alea {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// ROM de test : boucle infinie ARM (`b .`) précédée de quelques octets.
fn rom() -> Vec<u8> {
    let mut r = vec![0u8; 0x400];
    r[0..4].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes());
    r
}

#[test]
fn refus_propres() {
    let mut g = Gba::new(&rom(), None).unwrap();
    g.run_frame();
    let e = g.save_state();
    assert_eq!(g.load_state(b"xx"), Err(ErreurEtat::Format));
    let mut v = e.clone();
    v[4] = 99;
    assert_eq!(g.load_state(&v), Err(ErreurEtat::Version(99)));
    let mut autre = rom();
    autre[0x100] = 1;
    let mut h = Gba::new(&autre, None).unwrap();
    assert_eq!(h.load_state(&e), Err(ErreurEtat::AutreRom));
    assert_eq!(g.load_state(&e[..e.len() - 1]), Err(ErreurEtat::Taille));
    let mut long = e.clone();
    long.push(0);
    assert_eq!(g.load_state(&long), Err(ErreurEtat::Taille));
    assert_eq!(g.save_state(), e, "console inchangée après les refus");
    assert_eq!(g.load_state(&e), Ok(()));
}

#[test]
fn etats_abimes_sans_panique() {
    let mut g = Gba::new(&rom(), None).unwrap();
    for _ in 0..3 {
        g.run_frame();
    }
    let e = g.save_state();
    let mut r = Alea(0x1234_5678_9ABC_DEF1);
    for _ in 0..300 {
        let mut v = e.clone();
        for _ in 0..1 + r.next() % 8 {
            let i = 10 + (r.next() as usize % (v.len() - 10));
            v[i] = r.next() as u8;
        }
        if r.next().is_multiple_of(4) {
            v.truncate(10 + r.next() as usize % (v.len() - 10));
        }
        if g.load_state(&v).is_ok() {
            g.run_frame();
            g.run_frame();
        } else {
            assert_eq!(g.save_state(), e, "refus : console inchangée");
        }
        g.load_state(&e).unwrap();
    }
}
