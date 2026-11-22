//! Les 4 timers (prescaler 1/64/256/1024, cascade, IRQ), calculés paresseusement : la valeur se
//! déduit du cycle de départ, et seul le prochain débordement est un évènement.
//! raccourci : pas le retard de 2 cycles au démarrage d'un timer, à ajouter si un test l'exige.

use crate::etat::{verifie, Ecrit, ErreurEtat, Lecteur};

const DECALAGE: [u32; 4] = [0, 6, 8, 10];

#[derive(Clone, Copy, Default)]
struct Timer {
    reload: u16,
    /// Valeur au cycle `debut`.
    compteur: u16,
    ctl: u16,
    debut: u64,
}

pub struct Timers {
    t: [Timer; 4],
    /// Cycle du prochain débordement d'un timer libre (`u64::MAX` si aucun).
    pub prochain: u64,
}

impl Timers {
    pub fn new() -> Self {
        Timers {
            t: [Timer::default(); 4],
            prochain: u64::MAX,
        }
    }

    /// Timer actif compté par l'horloge (pas en cascade).
    fn libre(&self, i: usize) -> bool {
        self.t[i].ctl & 0x80 != 0 && (i == 0 || self.t[i].ctl & 4 == 0)
    }

    fn periode(&self, i: usize) -> u64 {
        (0x10000 - self.t[i].reload as u64) << DECALAGE[(self.t[i].ctl & 3) as usize]
    }

    fn valeur(&self, i: usize, now: u64) -> u16 {
        let t = &self.t[i];
        if !self.libre(i) {
            return t.compteur;
        }
        let ticks = now.saturating_sub(t.debut) >> DECALAGE[(t.ctl & 3) as usize];
        let reste = 0x10000 - t.compteur as u64;
        if ticks < reste {
            t.compteur.wrapping_add(ticks as u16)
        } else {
            let p = 0x10000 - t.reload as u64;
            (t.reload as u64 + (ticks - reste) % p) as u16
        }
    }

    /// Lecture d'un registre (décalage 0x100-0x10E).
    pub fn lit(&self, o: u32, now: u64) -> u16 {
        let i = ((o >> 2) & 3) as usize;
        if o & 2 == 0 {
            self.valeur(i, now)
        } else {
            self.t[i].ctl
        }
    }

    /// Valeur de rechargement (base des écritures 8 bits).
    pub fn reload(&self, o: u32) -> u16 {
        self.t[((o >> 2) & 3) as usize].reload
    }

    /// Écriture d'un registre ; renvoie le masque IRQ des débordements rattrapés.
    pub fn ecrit(&mut self, o: u32, v: u16, now: u64) -> u16 {
        let irq = self.avance(now).0;
        let i = ((o >> 2) & 3) as usize;
        if o & 2 == 0 {
            self.t[i].reload = v;
        } else {
            let ancien = self.t[i].ctl;
            let nv = v & 0xC7;
            if (ancien ^ nv) & 0x87 != 0 {
                // Démarrage, arrêt ou changement de prescaler/cascade : on fige puis on repart.
                let val = self.valeur(i, now);
                self.t[i].compteur = if ancien & 0x80 == 0 && nv & 0x80 != 0 {
                    self.t[i].reload
                } else {
                    val
                };
                self.t[i].debut = now;
            }
            self.t[i].ctl = nv;
        }
        self.calcule_prochain();
        irq
    }

    /// Traite les débordements jusqu'à `now`. Renvoie (masque IRQ, débordements des timers 0 et 1
    /// pour le son direct du lot APU).
    pub fn avance(&mut self, now: u64) -> (u16, [u32; 2]) {
        let mut irq = 0;
        let mut debs = [0u32; 2];
        if now >= self.prochain {
            for i in 0..4 {
                if !self.libre(i) {
                    continue;
                }
                let t = self.t[i];
                let sh = DECALAGE[(t.ctl & 3) as usize];
                let ovf = t.debut + ((0x10000 - t.compteur as u64) << sh);
                if now >= ovf {
                    let p = self.periode(i);
                    let n = 1 + (now - ovf) / p;
                    self.t[i].compteur = t.reload;
                    self.t[i].debut = ovf + (n - 1) * p;
                    self.deborde(i, n, &mut irq, &mut debs);
                }
            }
            self.calcule_prochain();
        }
        (irq, debs)
    }

    /// `n` débordements du timer `i` : IRQ et cascade vers le suivant.
    fn deborde(&mut self, i: usize, n: u64, irq: &mut u16, debs: &mut [u32; 2]) {
        if self.t[i].ctl & 0x40 != 0 {
            *irq |= 1 << (3 + i);
        }
        if i < 2 {
            debs[i] = debs[i].saturating_add(n.min(u32::MAX as u64) as u32);
        }
        let j = i + 1;
        if j < 4 && self.t[j].ctl & 0x84 == 0x84 {
            let total = self.t[j].compteur as u64 + n;
            if total >= 0x10000 {
                let p = 0x10000 - self.t[j].reload as u64;
                let x = total - 0x10000;
                self.t[j].compteur = (self.t[j].reload as u64 + x % p) as u16;
                self.deborde(j, 1 + x / p, irq, debs);
            } else {
                self.t[j].compteur = total as u16;
            }
        }
    }

    fn calcule_prochain(&mut self) {
        self.prochain = (0..4)
            .filter(|&i| self.libre(i))
            .map(|i| {
                let t = &self.t[i];
                t.debut + ((0x10000 - t.compteur as u64) << DECALAGE[(t.ctl & 3) as usize])
            })
            .min()
            .unwrap_or(u64::MAX);
    }

    pub(crate) fn sauve(&self, e: &mut Vec<u8>) {
        for t in &self.t {
            e.u16(t.reload);
            e.u16(t.compteur);
            e.u16(t.ctl);
            e.u64(t.debut);
        }
    }

    pub(crate) fn charge(&mut self, l: &mut Lecteur) -> Result<(), ErreurEtat> {
        let mut t = [Timer::default(); 4];
        for x in &mut t {
            x.reload = l.u16()?;
            x.compteur = l.u16()?;
            x.ctl = l.u16()?;
            x.debut = l.u64()?;
            verifie(x.ctl & !0xC7 == 0 && x.debut < 1 << 60, "timer")?;
        }
        self.t = t;
        self.calcule_prochain();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Timers;

    #[test]
    fn prescaler_et_debordement() {
        let mut t = Timers::new();
        t.ecrit(0x100, 0xFF00, 0); // reload
        t.ecrit(0x102, 0xC1, 0); // actif, IRQ, prescaler 64
        assert_eq!(t.lit(0x100, 64 * 10 + 63), 0xFF0A);
        // Débordement après 256 × 64 cycles.
        assert_eq!(t.prochain, 256 * 64);
        assert_eq!(t.avance(256 * 64 - 1).0, 0);
        assert_eq!(t.avance(256 * 64).0, 1 << 3);
        assert_eq!(t.lit(0x100, 256 * 64), 0xFF00);
        // Plusieurs débordements rattrapés d'un coup.
        let (irq, debs) = t.avance(256 * 64 * 4);
        assert_eq!((irq, debs[0]), (1 << 3, 3));
    }

    #[test]
    fn cascade() {
        let mut t = Timers::new();
        t.ecrit(0x100, 0xFFFF, 0);
        t.ecrit(0x104, 0xFFFE, 0);
        t.ecrit(0x106, 0xC4, 0); // timer 1 : cascade, IRQ
        t.ecrit(0x102, 0x80, 0); // timer 0 : 1 débordement par cycle
        assert_eq!(t.avance(1).0, 0);
        assert_eq!(t.lit(0x104, 1), 0xFFFF);
        assert_eq!(t.avance(2).0, 1 << 4);
        assert_eq!(t.lit(0x104, 2), 0xFFFE);
    }

    #[test]
    fn arret_fige_la_valeur() {
        let mut t = Timers::new();
        t.ecrit(0x102, 0x80, 0);
        t.ecrit(0x102, 0x00, 100);
        assert_eq!(t.lit(0x100, 5000), 100);
        assert_eq!(t.prochain, u64::MAX);
    }
}
