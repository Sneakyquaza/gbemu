//! Rembobinage : tampon circulaire d'états de la console, stockés en différences.
//! Seul le dernier état est gardé en entier ; chaque entrée plus ancienne est le XOR avec
//! l'état suivant, compressé par plages de zéros (la plupart des octets ne changent pas
//! entre deux images). Reculer d'un cran = défaire le dernier XOR, sans rien recopier.

use std::collections::VecDeque;

/// Un état est capturé toutes les `PAS` images émulées.
pub const PAS: u64 = 2;
/// Durée gardée, en secondes de jeu à vitesse normale.
pub const DUREE: f64 = 30.0;
/// Plafond mémoire des différences (le dernier état entier en plus).
pub const OCTETS_MAX: usize = 48 << 20;

pub struct Rembobinage {
    courant: Vec<u8>,
    deltas: VecDeque<Vec<u8>>,
    octets: usize,
    max_entrees: usize,
    max_octets: usize,
}

impl Rembobinage {
    pub fn new(max_entrees: usize, max_octets: usize) -> Rembobinage {
        Rembobinage {
            courant: Vec::new(),
            deltas: VecDeque::new(),
            octets: 0,
            max_entrees,
            max_octets,
        }
    }

    /// Tampon réglé pour [`DUREE`] secondes à 59,73 images/s, un état toutes les [`PAS`].
    pub fn standard() -> Rembobinage {
        Rembobinage::new((DUREE * 59.7275 / PAS as f64) as usize, OCTETS_MAX)
    }

    /// Ajoute l'état le plus récent.
    pub fn pousser(&mut self, etat: Vec<u8>) {
        if self.courant.len() != etat.len() {
            // Taille différente (ne devrait pas arriver pour une même ROM) : on repart de zéro.
            self.vider();
        } else {
            let d = compresser_xor(&self.courant, &etat);
            self.octets += d.capacity();
            self.deltas.push_back(d);
            while self.deltas.len() > self.max_entrees || self.octets > self.max_octets {
                match self.deltas.pop_front() {
                    Some(v) => self.octets -= v.capacity(),
                    None => break,
                }
            }
        }
        self.courant = etat;
    }

    /// Recule d'un cran et rend l'état correspondant (`None` : plus rien en arrière).
    pub fn reculer(&mut self) -> Option<&[u8]> {
        let d = self.deltas.pop_back()?;
        self.octets -= d.capacity();
        appliquer_xor(&mut self.courant, &d);
        Some(&self.courant)
    }

    pub fn vider(&mut self) {
        self.courant.clear();
        self.deltas.clear();
        self.octets = 0;
    }

    /// Nombre de crans disponibles en arrière.
    #[cfg(test)]
    pub fn crans(&self) -> usize {
        self.deltas.len()
    }

    /// Part du tampon remplie (0 à 1).
    pub fn remplissage(&self) -> f32 {
        self.deltas.len() as f32 / self.max_entrees.max(1) as f32
    }

    /// Mémoire occupée (différences + dernier état entier).
    pub fn octets(&self) -> usize {
        self.octets + self.courant.capacity()
    }
}

fn ecrire_varint(sortie: &mut Vec<u8>, mut n: usize) {
    while n >= 0x80 {
        sortie.push(n as u8 | 0x80);
        n >>= 7;
    }
    sortie.push(n as u8);
}

fn lire_varint(d: &[u8], i: &mut usize) -> Option<usize> {
    let mut n = 0usize;
    let mut dec = 0;
    loop {
        let o = *d.get(*i)?;
        *i += 1;
        n |= ((o & 0x7F) as usize) << dec;
        if o & 0x80 == 0 {
            return Some(n);
        }
        dec += 7;
        if dec > 56 {
            return None;
        }
    }
}

/// `a XOR b` codé en suites (plage de zéros, longueur littérale, octets littéraux).
fn compresser_xor(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut s = Vec::new();
    let n = a.len();
    let mut i = 0;
    while i < n {
        let debut = i;
        while i < n && a[i] == b[i] {
            i += 1;
        }
        let zeros = i - debut;
        let lit = i;
        // Un littéral s'arrête à la première suite d'au moins 4 zéros (moins cher à coder ainsi).
        let mut z = 0;
        while i < n {
            if a[i] == b[i] {
                z += 1;
                if z >= 4 {
                    i -= z - 1;
                    z = 0;
                    break;
                }
            } else {
                z = 0;
            }
            i += 1;
        }
        let fin = i - z;
        i = fin;
        ecrire_varint(&mut s, zeros);
        ecrire_varint(&mut s, fin - lit);
        s.extend(a[lit..fin].iter().zip(&b[lit..fin]).map(|(x, y)| x ^ y));
    }
    s.shrink_to_fit();
    s
}

/// Applique sur `etat` un XOR produit par [`compresser_xor`] (données incohérentes : ignorées).
fn appliquer_xor(etat: &mut [u8], d: &[u8]) {
    let mut i = 0;
    let mut pos = 0usize;
    while i < d.len() {
        let (Some(zeros), Some(lit)) = (lire_varint(d, &mut i), lire_varint(d, &mut i)) else {
            return;
        };
        pos += zeros;
        let (Some(src), Some(dst)) = (d.get(i..i + lit), etat.get_mut(pos..pos + lit)) else {
            return;
        };
        dst.iter_mut().zip(src).for_each(|(x, y)| *x ^= y);
        i += lit;
        pos += lit;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn etat(graine: u32, n: usize) -> Vec<u8> {
        // Pseudo-aléatoire, quelques octets modifiés par graine.
        let mut v = vec![0u8; n];
        let mut x = graine.wrapping_mul(2_654_435_761).max(1);
        for _ in 0..50 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            v[x as usize % n] = x as u8;
        }
        v
    }

    #[test]
    fn xor_aller_retour() {
        for (a, b) in [(1, 2), (3, 3), (7, 900)] {
            let (ea, eb) = (etat(a, 4096), etat(b, 4096));
            let d = compresser_xor(&ea, &eb);
            let mut x = eb.clone();
            appliquer_xor(&mut x, &d);
            assert_eq!(x, ea);
        }
        // Tout différent, rien de commun.
        let ea: Vec<u8> = (0..1000).map(|i| i as u8).collect();
        let eb: Vec<u8> = ea.iter().map(|v| !v).collect();
        let mut x = eb.clone();
        appliquer_xor(&mut x, &compresser_xor(&ea, &eb));
        assert_eq!(x, ea);
    }

    #[test]
    fn recule_dans_l_ordre() {
        let mut r = Rembobinage::new(100, usize::MAX);
        for g in 0..10 {
            r.pousser(etat(g, 2048));
        }
        assert_eq!(r.crans(), 9);
        for g in (0..9).rev() {
            assert_eq!(r.reculer().unwrap(), &etat(g, 2048)[..]);
        }
        assert!(r.reculer().is_none());
        // Reprise après un recul : la chaîne reste juste.
        r.pousser(etat(42, 2048));
        assert_eq!(r.reculer().unwrap(), &etat(0, 2048)[..]);
    }

    #[test]
    fn tampon_borne() {
        let mut r = Rembobinage::new(20, usize::MAX);
        for g in 0..100 {
            r.pousser(etat(g, 2048));
        }
        assert_eq!(r.crans(), 20);
        let mut r = Rembobinage::new(10_000, 4000);
        for g in 0..1000 {
            r.pousser(etat(g, 2048));
        }
        assert!(r.octets() <= 4000 + 2048, "{}", r.octets());
        assert!(r.crans() > 0);
        for g in (980..999).rev() {
            if r.crans() == 0 {
                break;
            }
            assert_eq!(r.reculer().unwrap(), &etat(g, 2048)[..]);
        }
    }

    #[test]
    fn delta_corrompu_sans_panique() {
        let mut e = vec![0u8; 16];
        appliquer_xor(
            &mut e,
            &[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01],
        );
        appliquer_xor(&mut e, &[2, 200, 1, 2]);
        appliquer_xor(&mut e, &[20, 1, 1]);
    }

    /// Mémoire réelle sur une ROM de test qui écrit beaucoup (sautée sans `roms/`).
    #[test]
    fn memoire_sur_rom() {
        let chemin = concat!(env!("CARGO_MANIFEST_DIR"), "/../roms");
        let rom = std::fs::read_dir(chemin).ok().and_then(|_| {
            ["blargg/cpu_instrs/cpu_instrs.gb", "dmg-acid2/dmg-acid2.gb"]
                .iter()
                .find_map(|f| std::fs::read(format!("{chemin}/{f}")).ok())
        });
        let Some(rom) = rom else {
            eprintln!("SAUTÉ : roms/ absent");
            return;
        };
        let mut gb = gb_core::GameBoy::new(&rom).unwrap();
        let mut r = Rembobinage::standard();
        let n = (DUREE * 59.7275) as u64;
        for i in 0..n {
            gb.run_frame();
            if i % PAS == 0 {
                r.pousser(gb.save_state());
            }
        }
        eprintln!(
            "rembobinage : {} crans, {} Kio pour {DUREE} s",
            r.crans(),
            r.octets() / 1024
        );
        assert!(r.octets() <= OCTETS_MAX + 128 * 1024);
        let fin = gb.frame().to_vec();
        let ancien = r.reculer().unwrap().to_vec();
        gb.load_state(&ancien).unwrap();
        gb.run_frame();
        gb.run_frame();
        assert_eq!(gb.frame(), &fin[..]);
    }
}
