//! Cartouche : ROM (jusqu'à 32 Mio) et mémoire de sauvegarde.
//! raccourci : seule la SRAM 64 Kio est émulée (Flash/EEPROM au lot « sauvegardes », qui détectera
//! le type par les chaînes `SRAM_V`, `FLASH*_V`, `EEPROM_V` de la ROM).

use std::fmt;

/// Taille maximale d'une ROM GBA.
pub const ROM_MAX: usize = 32 * 1024 * 1024;
const SRAM_TAILLE: usize = 0x10000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErreurChargement {
    RomVide,
    RomTropGrande(usize),
    /// Le BIOS fourni ne fait pas 16 Kio.
    BiosTaille(usize),
}

impl fmt::Display for ErreurChargement {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ErreurChargement::RomVide => write!(f, "ROM vide"),
            ErreurChargement::RomTropGrande(n) => write!(f, "ROM trop grande ({n} octets)"),
            ErreurChargement::BiosTaille(n) => {
                write!(f, "BIOS de {n} octets (16 384 attendus)")
            }
        }
    }
}

impl std::error::Error for ErreurChargement {}

pub struct Cartouche {
    /// ROM complétée par des zéros jusqu'à un multiple de 4.
    rom: Vec<u8>,
    sram: Vec<u8>,
}

impl Cartouche {
    pub fn new(rom: &[u8]) -> Result<Self, ErreurChargement> {
        if rom.is_empty() {
            return Err(ErreurChargement::RomVide);
        }
        if rom.len() > ROM_MAX {
            return Err(ErreurChargement::RomTropGrande(rom.len()));
        }
        let mut r = rom.to_vec();
        r.resize(rom.len().div_ceil(4) * 4, 0);
        Ok(Cartouche {
            rom: r,
            sram: vec![0xFF; SRAM_TAILLE],
        })
    }

    /// Lecture 16 bits (adresse paire) ; au-delà de la ROM : bus ouvert de la cartouche (adresse / 2).
    #[inline]
    pub fn lit16(&self, a: u32) -> u16 {
        let o = (a & 0x01FF_FFFE) as usize;
        match self.rom.get(o..o + 2) {
            Some(b) => u16::from_le_bytes([b[0], b[1]]),
            None => (a >> 1) as u16,
        }
    }

    #[inline]
    pub fn lit32(&self, a: u32) -> u32 {
        let o = (a & 0x01FF_FFFC) as usize;
        match self.rom.get(o..o + 4) {
            Some(b) => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            None => {
                let a = a & !3;
                self.lit16(a) as u32 | (self.lit16(a + 2) as u32) << 16
            }
        }
    }

    #[inline]
    pub fn lit8(&self, a: u32) -> u8 {
        (self.lit16(a & !1) >> ((a & 1) * 8)) as u8
    }

    /// SRAM (bus 8 bits, miroir tous les 64 Kio).
    #[inline]
    pub fn lit_sram(&self, a: u32) -> u8 {
        self.sram[a as usize & (SRAM_TAILLE - 1)]
    }

    #[inline]
    pub fn ecrit_sram(&mut self, a: u32, v: u8) {
        self.sram[a as usize & (SRAM_TAILLE - 1)] = v;
    }
}
