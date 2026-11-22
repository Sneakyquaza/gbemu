//! Les 4 canaux DMA : immédiat, VBlank, HBlank et spécial (FIFO son pour DMA1/2, capture vidéo
//! pour DMA3). Un transfert s'exécute d'un bloc, le CPU étant arrêté pendant ce temps.
//! raccourci : pas le délai de 2 cycles au démarrage ni l'interruption d'un DMA par un autre plus
//! prioritaire ; la valeur lue du BIOS ou d'une zone vide est le bus ouvert du CPU.

use crate::bus::Bus;
use crate::etat::{verifie, Ecrit, ErreurEtat, Lecteur};

/// Masques d'adresse source / destination par canal.
const MASQUE_SRC: [u32; 4] = [0x07FF_FFFF, 0x0FFF_FFFF, 0x0FFF_FFFF, 0x0FFF_FFFF];
const MASQUE_DST: [u32; 4] = [0x07FF_FFFF, 0x07FF_FFFF, 0x07FF_FFFF, 0x0FFF_FFFF];

#[derive(Clone, Copy, Default)]
pub struct Dma {
    pub sad: u32,
    pub dad: u32,
    pub cnt: u16,
    pub ctl: u16,
    src: u32,
    dst: u32,
    compte: u32,
}

/// Moment de déclenchement (bits 12-13 de CNT_H).
pub const IMMEDIAT: u16 = 0;
pub const VBLANK: u16 = 1;
pub const HBLANK: u16 = 2;
pub const SPECIAL: u16 = 3;

impl Dma {
    fn moment(&self) -> u16 {
        (self.ctl >> 12) & 3
    }

    fn actif(&self) -> bool {
        self.ctl & 0x8000 != 0
    }

    fn compte_initial(&self, n: usize) -> u32 {
        match (self.cnt, n) {
            (0, 3) => 0x10000,
            (0, _) => 0x4000,
            (c, 3) => c as u32,
            (c, _) => (c & 0x3FFF) as u32,
        }
    }
}

impl Bus {
    /// Écriture d'un registre DMA (décalage 0x0B0-0x0DE).
    pub(crate) fn dma_ecrit(&mut self, o: u32, v: u16) {
        let n = ((o - 0xB0) / 12) as usize;
        let d = &mut self.dma[n];
        match (o - 0xB0) % 12 {
            0 => d.sad = (d.sad & 0xFFFF_0000) | v as u32,
            2 => d.sad = (d.sad & 0xFFFF) | (v as u32) << 16,
            4 => d.dad = (d.dad & 0xFFFF_0000) | v as u32,
            6 => d.dad = (d.dad & 0xFFFF) | (v as u32) << 16,
            8 => d.cnt = v,
            _ => {
                let avant = d.actif();
                // DMA3 seul a le bit DRQ cartouche (11).
                d.ctl = v & if n == 3 { 0xFFE0 } else { 0xF7E0 };
                if !avant && d.actif() {
                    d.src = d.sad & MASQUE_SRC[n];
                    d.dst = d.dad & MASQUE_DST[n];
                    d.compte = d.compte_initial(n);
                    if d.moment() == IMMEDIAT {
                        self.dma_transfert(n);
                    }
                }
            }
        }
    }

    pub(crate) fn dma_lit_ctl(&self, o: u32) -> u16 {
        self.dma[((o - 0xB0) / 12) as usize].ctl
    }

    /// Déclenche les canaux actifs programmés sur `moment` (ordre de priorité 0 → 3).
    pub(crate) fn dma_declenche(&mut self, moment: u16) {
        for n in 0..4 {
            let d = &self.dma[n];
            if d.actif() && d.moment() == moment && (moment != SPECIAL || n == 3) {
                self.dma_transfert(n);
            }
        }
    }

    /// Fin de la capture vidéo (DMA3 spécial) : le canal s'arrête.
    pub(crate) fn dma_fin_capture(&mut self) {
        let d = &mut self.dma[3];
        if d.actif() && d.moment() == SPECIAL {
            d.ctl &= !0x8000;
        }
    }

    /// Requête FIFO son (canal 1 ou 2, mode spécial) : 4 mots, destination fixe.
    /// Appelée par le son direct (FIFO A/B à moitié vide sur débordement de timer).
    pub(crate) fn dma_fifo(&mut self, n: usize) {
        let d = self.dma[n];
        if !(n == 1 || n == 2) || !d.actif() || d.moment() != SPECIAL {
            return;
        }
        let mut src = d.src;
        for i in 0..4 {
            let v = self.lit32(src & !3, i > 0);
            self.ecrit32(d.dst & !3, v, i > 0);
            src = match (d.ctl >> 7) & 3 {
                1 => src.wrapping_sub(4),
                2 => src,
                _ => src.wrapping_add(4),
            };
        }
        self.cycles += 2;
        self.dma[n].src = src;
        if d.ctl & 0x4000 != 0 {
            self.if_ |= 1 << (8 + n);
        }
        if d.ctl & 0x200 == 0 {
            self.dma[n].ctl &= !0x8000;
        }
    }

    fn dma_transfert(&mut self, n: usize) {
        let d = self.dma[n];
        let mot = d.ctl & 0x400 != 0;
        let pas: u32 = if mot { 4 } else { 2 };
        let pas_src = match (d.ctl >> 7) & 3 {
            1 => pas.wrapping_neg(),
            2 => 0,
            _ => pas,
        };
        let pas_dst = match (d.ctl >> 5) & 3 {
            1 => pas.wrapping_neg(),
            2 => 0,
            _ => pas,
        };
        let (mut src, mut dst) = (d.src, d.dst);
        if n == 3 {
            self.cart.dma_eeprom(src, dst, d.compte);
        }
        for i in 0..d.compte {
            let seq = i > 0;
            if mot {
                let v = self.lit32(src & !3, seq);
                self.ecrit32(dst & !3, v, seq);
            } else {
                let v = self.lit16(src & !1, seq);
                self.ecrit16(dst & !1, v, seq);
            }
            src = src.wrapping_add(pas_src);
            dst = dst.wrapping_add(pas_dst);
        }
        self.cycles += 2;
        let d = &mut self.dma[n];
        d.src = src;
        d.dst = dst;
        if d.ctl & 0x4000 != 0 {
            self.if_ |= 1 << (8 + n);
        }
        if d.ctl & 0x200 != 0 && d.moment() != IMMEDIAT {
            d.compte = d.compte_initial(n);
            if (d.ctl >> 5) & 3 == 3 {
                d.dst = d.dad & MASQUE_DST[n];
            }
        } else {
            d.ctl &= !0x8000;
        }
    }
}

impl Dma {
    pub(crate) fn sauve(&self, e: &mut Vec<u8>) {
        e.u32(self.sad);
        e.u32(self.dad);
        e.u16(self.cnt);
        e.u16(self.ctl);
        e.u32(self.src);
        e.u32(self.dst);
        e.u32(self.compte);
    }

    pub(crate) fn charge(l: &mut Lecteur) -> Result<Self, ErreurEtat> {
        let d = Dma {
            sad: l.u32()?,
            dad: l.u32()?,
            cnt: l.u16()?,
            ctl: l.u16()?,
            src: l.u32()?,
            dst: l.u32()?,
            compte: l.u32()?,
        };
        verifie(d.compte <= 0x10000, "dma")?;
        Ok(d)
    }
}

#[cfg(test)]
mod tests {
    use crate::bus::Bus;
    use crate::cartouche::Cartouche;

    fn bus() -> Bus {
        Bus::new(Cartouche::new(&[0; 4]).unwrap(), crate::BIOS_LIBRE)
    }

    #[test]
    fn immediat_32_bits_et_irq() {
        let mut b = bus();
        for i in 0..4 {
            b.ecrit32(0x0200_0000 + i * 4, 0x1111_1111 * (i + 1), false);
        }
        b.ecrit32(0x0400_00D4, 0x0200_0000, false); // DMA3SAD
        b.ecrit32(0x0400_00D8, 0x0300_0100, false); // DMA3DAD
        b.ecrit16(0x0400_00DC, 4, false);
        b.ecrit16(0x0400_00DE, 0xC400, false); // actif, IRQ, 32 bits, immédiat
        for i in 0..4 {
            assert_eq!(b.lit32_brut(0x0300_0100 + i * 4), 0x1111_1111 * (i + 1));
        }
        assert_eq!(b.lit16_brut(0x0400_00DE) & 0x8000, 0, "canal arrêté");
        assert_ne!(b.if_ & (1 << 11), 0);
    }

    #[test]
    fn hblank_repete_destination_rechargee() {
        let mut b = bus();
        b.ecrit16(0x0200_0000, 0xABCD, false);
        b.ecrit32(0x0400_00B0, 0x0200_0000, false); // DMA0SAD
        b.ecrit32(0x0400_00B4, 0x0300_0000, false);
        b.ecrit16(0x0400_00B8, 1, false);
        // Répétition, HBlank, source fixe, destination incrément + rechargement.
        b.ecrit16(
            0x0400_00BA,
            0x8000 | 0x2000 | 0x200 | (2 << 7) | (3 << 5),
            false,
        );
        assert_eq!(b.lit16_brut(0x0300_0000), 0);
        b.cycles = 960;
        b.evenements();
        assert_eq!(b.lit16_brut(0x0300_0000), 0xABCD);
        b.ecrit16(0x0300_0000, 0, false);
        b.cycles = 1232 + 960;
        b.evenements();
        assert_eq!(b.lit16_brut(0x0300_0000), 0xABCD, "destination rechargée");
        assert_ne!(
            b.lit16_brut(0x0400_00BA) & 0x8000,
            0,
            "canal toujours actif"
        );
    }
}
