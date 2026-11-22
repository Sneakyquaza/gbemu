//! Timer DMG : compteur interne 16 bits (DIV = octet haut), TIMA sur front descendant.

use crate::etat::{Ecrit, ErreurEtat, Lecteur};

const DIV_CGB: u16 = 0x2674;

pub struct Timer {
    /// Compteur interne, +4 par cycle machine.
    pub div: u16,
    pub tima: u8,
    pub tma: u8,
    pub tac: u8,
    /// TIMA a débordé au cycle précédent : rechargement au cycle courant.
    overflow: bool,
    /// Le cycle courant est celui du rechargement TMA → TIMA.
    reloading: bool,
}

impl Timer {
    /// DIV interne au démarrage calé sur mooneye boot_div-dmgABCmgb / boot_div-cgbABCDE.
    pub fn new(cgb: bool) -> Self {
        Timer {
            div: if cgb { DIV_CGB } else { 0xABC8 },
            tima: 0,
            tma: 0,
            tac: 0xF8,
            overflow: false,
            reloading: false,
        }
    }

    pub fn sauve(&self, e: &mut Vec<u8>) {
        e.u16(self.div);
        e.octets(&[self.tima, self.tma, self.tac]);
        e.bool(self.overflow);
        e.bool(self.reloading);
    }

    pub fn charge(&mut self, l: &mut Lecteur) -> Result<(), ErreurEtat> {
        self.div = l.u16()?;
        self.tima = l.u8()?;
        self.tma = l.u8()?;
        self.tac = l.u8()?;
        self.overflow = l.bool()?;
        self.reloading = l.bool()?;
        Ok(())
    }

    #[inline]
    fn signal(&self, div: u16) -> bool {
        const BITS: [u16; 4] = [1 << 9, 1 << 3, 1 << 5, 1 << 7];
        self.tac & 0x04 != 0 && div & BITS[(self.tac & 3) as usize] != 0
    }

    /// Incrémente TIMA ; renvoie vrai au débordement.
    // L'interruption est levée dès le débordement (le rechargement TMA suit au cycle suivant) :
    // avec la convention « cycle puis accès » du bus, c'est ce que mesure mooneye rapid_toggle.
    fn inc_tima(&mut self) -> bool {
        let (v, o) = self.tima.overflowing_add(1);
        self.tima = v;
        self.overflow |= o;
        o
    }

    /// Avance d'un cycle machine ; renvoie vrai si l'interruption timer est levée.
    #[inline]
    pub fn tick(&mut self) -> bool {
        self.reloading = false;
        if self.overflow {
            self.overflow = false;
            self.tima = self.tma;
            self.reloading = true;
        }
        let old = self.div;
        self.div = old.wrapping_add(4);
        self.signal(old) && !self.signal(self.div) && self.inc_tima()
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF04 => (self.div >> 8) as u8,
            0xFF05 => self.tima,
            0xFF06 => self.tma,
            _ => self.tac | 0xF8,
        }
    }

    /// Écriture d'un registre ; renvoie vrai si TIMA déborde (incrément parasite de DIV/TAC).
    pub fn write(&mut self, addr: u16, v: u8) -> bool {
        match addr {
            0xFF04 => {
                let irq = self.signal(self.div) && self.inc_tima();
                self.div = 0;
                irq
            }
            0xFF05 => {
                if !self.reloading {
                    self.tima = v;
                    self.overflow = false;
                }
                false
            }
            0xFF06 => {
                self.tma = v;
                if self.reloading {
                    self.tima = v;
                }
                false
            }
            _ => {
                let old = self.signal(self.div);
                self.tac = v & 0x07;
                old && !self.signal(self.div) && self.inc_tima()
            }
        }
    }
}
