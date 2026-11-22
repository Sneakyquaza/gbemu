//! Son : bouchon (registres mémorisés, lus avec leurs bits inutilisés à 1). Aucun son produit.
// raccourci : pas de synthèse ni de canaux actifs, à remplacer à l'étape APU.

/// Bits toujours lus à 1 pour FF10..FF2F (DMG).
const MASQUES: [u8; 0x20] = [
    0x80, 0x3F, 0x00, 0xFF, 0xBF, 0xFF, 0x3F, 0x00, 0xFF, 0xBF, 0x7F, 0xFF, 0x9F, 0xFF, 0xBF, 0xFF,
    0xFF, 0x00, 0x00, 0xBF, 0x00, 0x00, 0x70, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
];

pub struct Apu {
    regs: [u8; 0x20],
    wave: [u8; 0x10],
}

impl Apu {
    pub fn new() -> Self {
        let mut regs = [0; 0x20];
        // Valeurs après la boot ROM DMG (NR11, NR12, NR50, NR51, NR52 avec canal 1 actif).
        regs[0x01] = 0x80;
        regs[0x02] = 0xF3;
        regs[0x14] = 0x77;
        regs[0x15] = 0xF3;
        regs[0x16] = 0x81;
        Apu {
            regs,
            wave: [0; 0x10],
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF10..=0xFF2F => {
                let i = (addr - 0xFF10) as usize;
                let v = self.regs[i];
                v | MASQUES[i]
            }
            _ => self.wave[(addr & 0x0F) as usize],
        }
    }

    pub fn write(&mut self, addr: u16, v: u8) {
        match addr {
            0xFF26 => {
                self.regs[0x16] = v & 0x80;
                if v & 0x80 == 0 {
                    // Extinction : tous les registres son sont remis à zéro.
                    self.regs[..0x16].fill(0);
                }
            }
            0xFF10..=0xFF2F => {
                if self.regs[0x16] & 0x80 != 0 {
                    self.regs[(addr - 0xFF10) as usize] = v;
                }
            }
            _ => self.wave[(addr & 0x0F) as usize] = v,
        }
    }
}
