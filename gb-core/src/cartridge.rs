//! Cartouche : en-tête, contrôleurs MBC0/1/3/5, RAM batterie et RTC du MBC3.

use crate::etat::{crc32, verifie, Ecrit, ErreurEtat, Lecteur};
use std::fmt;

/// Taille maximale acceptée (MBC5 : 512 banques de 16 Kio).
const ROM_MAX: usize = 8 * 1024 * 1024;
/// Cycles machine (M) par seconde : 4 194 304 Hz / 4.
const M_CYCLES_PAR_SECONDE: u64 = 1_048_576;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// Fichier vide.
    Vide,
    /// Plus de 8 Mio.
    TropGrande(usize),
    /// Type de cartouche (octet 0x147) non pris en charge.
    MapperInconnu(u8),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Vide => write!(f, "fichier ROM vide"),
            LoadError::TropGrande(n) => write!(f, "ROM trop grande ({n} octets, max 8 Mio)"),
            LoadError::MapperInconnu(t) => {
                write!(f, "type de cartouche 0x{t:02X} non pris en charge")
            }
        }
    }
}

impl std::error::Error for LoadError {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Mbc0,
    Mbc1,
    Mbc3,
    Mbc5,
}

/// Horloge temps réel du MBC3 (secondes, minutes, heures, jours sur 9 bits, arrêt, retenue).
#[derive(Default, Clone)]
struct Rtc {
    s: u8,
    m: u8,
    h: u8,
    dl: u8,
    dh: u8,
    latched: [u8; 5],
    latch_prev: u8,
    sub: u64,
}

impl Rtc {
    fn regs(&self) -> [u8; 5] {
        [self.s, self.m, self.h, self.dl, self.dh]
    }

    fn advance(&mut self, m_cycles: u64) {
        if self.dh & 0x40 != 0 {
            return;
        }
        self.sub += m_cycles;
        while self.sub >= M_CYCLES_PAR_SECONDE {
            self.sub -= M_CYCLES_PAR_SECONDE;
            self.tick_second();
        }
    }

    fn tick_second(&mut self) {
        self.s = (self.s + 1) & 0x3F;
        if self.s != 60 {
            return;
        }
        self.s = 0;
        self.m = (self.m + 1) & 0x3F;
        if self.m != 60 {
            return;
        }
        self.m = 0;
        self.h = (self.h + 1) & 0x1F;
        if self.h != 24 {
            return;
        }
        self.h = 0;
        let days = (((self.dh as u16) & 1) << 8 | self.dl as u16) + 1;
        self.dl = days as u8;
        self.dh = (self.dh & 0xFE) | ((days >> 8) as u8 & 1);
        if days > 0x1FF {
            self.dh |= 0x80;
        }
    }

    fn write(&mut self, reg: u8, v: u8) {
        match reg {
            0x08 => {
                self.s = v & 0x3F;
                self.sub = 0;
            }
            0x09 => self.m = v & 0x3F,
            0x0A => self.h = v & 0x1F,
            0x0B => self.dl = v,
            _ => self.dh = v & 0xC1,
        }
    }
}

pub struct Cartridge {
    rom: Vec<u8>,
    rom_mask: usize,
    ram: Vec<u8>,
    kind: Kind,
    battery: bool,
    rtc: Option<Rtc>,
    ram_enabled: bool,
    /// MBC1 : 5 bits bas ; MBC3 : 7 bits ; MBC5 : 9 bits.
    rom_bank: u16,
    /// MBC1 : registre 2 bits ; MBC3 : banque RAM ou registre RTC ; MBC5 : 4 bits.
    bank2: u8,
    mbc1_mode: bool,
    rom_off0: usize,
    rom_off1: usize,
    ram_off: usize,
    /// CRC32 du fichier ROM (empreinte des sauvegardes d'état).
    pub crc: u32,
}

impl Cartridge {
    pub fn new(data: &[u8]) -> Result<Self, LoadError> {
        if data.is_empty() {
            return Err(LoadError::Vide);
        }
        if data.len() > ROM_MAX {
            return Err(LoadError::TropGrande(data.len()));
        }
        // Taille en puissance de deux (au moins 2 banques), complétée par 0xFF :
        // une ROM tronquée se lit sans dépassement.
        let size = data.len().next_power_of_two().max(0x8000);
        let mut rom = data.to_vec();
        rom.resize(size, 0xFF);

        let t = rom[0x147];
        let (kind, ram_hdr, battery, timer) = match t {
            0x00 => (Kind::Mbc0, false, false, false),
            0x08 => (Kind::Mbc0, true, false, false),
            0x09 => (Kind::Mbc0, true, true, false),
            0x01 => (Kind::Mbc1, false, false, false),
            0x02 => (Kind::Mbc1, true, false, false),
            0x03 => (Kind::Mbc1, true, true, false),
            0x0F => (Kind::Mbc3, false, true, true),
            0x10 => (Kind::Mbc3, true, true, true),
            0x11 => (Kind::Mbc3, false, false, false),
            0x12 => (Kind::Mbc3, true, false, false),
            0x13 => (Kind::Mbc3, true, true, false),
            0x19 | 0x1C => (Kind::Mbc5, false, false, false),
            0x1A | 0x1D => (Kind::Mbc5, true, false, false),
            0x1B | 0x1E => (Kind::Mbc5, true, true, false),
            _ => return Err(LoadError::MapperInconnu(t)),
        };
        let ram_size = if ram_hdr {
            match rom[0x149] {
                1 => 0x800,
                2 => 0x2000,
                3 => 0x8000,
                4 => 0x20000,
                5 => 0x10000,
                _ => 0,
            }
        } else {
            0
        };
        let mut cart = Cartridge {
            rom_mask: size - 1,
            rom,
            ram: vec![0; ram_size],
            kind,
            battery,
            rtc: timer.then(Rtc::default),
            ram_enabled: false,
            rom_bank: 1,
            bank2: 0,
            mbc1_mode: false,
            rom_off0: 0,
            rom_off1: 0x4000,
            ram_off: 0,
            crc: crc32(data),
        };
        cart.update_offsets();
        Ok(cart)
    }

    /// Sauvegarde d'état : RAM, registres MBC et RTC (la ROM est identifiée par `crc`).
    pub fn sauve(&self, e: &mut Vec<u8>) {
        e.u32(self.ram.len() as u32);
        e.octets(&self.ram);
        e.bool(self.ram_enabled);
        e.u16(self.rom_bank);
        e.u8(self.bank2);
        e.bool(self.mbc1_mode);
        e.bool(self.rtc.is_some());
        if let Some(rtc) = &self.rtc {
            e.octets(&rtc.regs());
            e.octets(&rtc.latched);
            e.u8(rtc.latch_prev);
            e.u64(rtc.sub);
        }
    }

    pub fn charge(&mut self, l: &mut Lecteur) -> Result<(), ErreurEtat> {
        verifie(l.u32()? as usize == self.ram.len(), "taille RAM")?;
        l.octets(&mut self.ram)?;
        self.ram_enabled = l.bool()?;
        self.rom_bank = l.u16_max(0x1FF, "MBC")?;
        self.bank2 = l.u8_max(0x0F, "MBC")?;
        self.mbc1_mode = l.bool()?;
        verifie(l.bool()? == self.rtc.is_some(), "RTC")?;
        if let Some(rtc) = &mut self.rtc {
            let mut r = [0u8; 5];
            l.octets(&mut r)?;
            rtc.s = r[0] & 0x3F;
            rtc.m = r[1] & 0x3F;
            rtc.h = r[2] & 0x1F;
            rtc.dl = r[3];
            rtc.dh = r[4] & 0xC1;
            l.octets(&mut rtc.latched)?;
            rtc.latch_prev = l.u8()?;
            rtc.sub = l.u64()?;
            verifie(rtc.sub < M_CYCLES_PAR_SECONDE, "RTC")?;
        }
        self.update_offsets();
        Ok(())
    }

    fn update_offsets(&mut self) {
        match self.kind {
            Kind::Mbc0 => {
                self.rom_off0 = 0;
                self.rom_off1 = 0x4000;
                self.ram_off = 0;
            }
            Kind::Mbc1 => {
                let hi = (self.bank2 as usize) << 5;
                let lo = (self.rom_bank as usize & 0x1F).max(1);
                self.rom_off0 = if self.mbc1_mode { hi * 0x4000 } else { 0 };
                self.rom_off1 = (hi | lo) * 0x4000;
                self.ram_off = if self.mbc1_mode {
                    self.bank2 as usize * 0x2000
                } else {
                    0
                };
            }
            Kind::Mbc3 => {
                self.rom_off0 = 0;
                self.rom_off1 = (self.rom_bank as usize & 0x7F).max(1) * 0x4000;
                self.ram_off = (self.bank2 as usize & 0x03) * 0x2000;
            }
            Kind::Mbc5 => {
                self.rom_off0 = 0;
                self.rom_off1 = (self.rom_bank as usize & 0x1FF) * 0x4000;
                self.ram_off = (self.bank2 as usize & 0x0F) * 0x2000;
            }
        }
        self.rom_off0 &= self.rom_mask;
        self.rom_off1 &= self.rom_mask;
    }

    #[inline]
    pub fn read_rom(&self, addr: u16) -> u8 {
        let off = if addr < 0x4000 {
            self.rom_off0
        } else {
            self.rom_off1
        };
        self.rom[(off | (addr as usize & 0x3FFF)) & self.rom_mask]
    }

    pub fn write_rom(&mut self, addr: u16, v: u8) {
        match (self.kind, addr >> 13) {
            (Kind::Mbc0, _) => {}
            (_, 0) => self.ram_enabled = v & 0x0F == 0x0A,
            (Kind::Mbc1, 1) => self.rom_bank = (v & 0x1F) as u16,
            (Kind::Mbc1, 2) => self.bank2 = v & 0x03,
            (Kind::Mbc1, _) => self.mbc1_mode = v & 1 != 0,
            (Kind::Mbc3, 1) => self.rom_bank = (v & 0x7F) as u16,
            (Kind::Mbc3, 2) => self.bank2 = v & 0x0F,
            (Kind::Mbc3, _) => {
                if let Some(rtc) = &mut self.rtc {
                    if rtc.latch_prev == 0 && v == 1 {
                        rtc.latched = rtc.regs();
                    }
                    rtc.latch_prev = v;
                }
            }
            (Kind::Mbc5, 1) => {
                if addr < 0x3000 {
                    self.rom_bank = (self.rom_bank & 0x100) | v as u16;
                } else {
                    self.rom_bank = (self.rom_bank & 0xFF) | ((v as u16 & 1) << 8);
                }
            }
            (Kind::Mbc5, 2) => self.bank2 = v & 0x0F,
            (Kind::Mbc5, _) => {}
        }
        self.update_offsets();
    }

    fn rtc_selected(&self) -> bool {
        self.kind == Kind::Mbc3 && self.rtc.is_some() && (0x08..=0x0C).contains(&self.bank2)
    }

    pub fn read_ram(&self, addr: u16) -> u8 {
        if !self.ram_enabled && self.kind != Kind::Mbc0 {
            return 0xFF;
        }
        if self.rtc_selected() {
            if let Some(rtc) = &self.rtc {
                return rtc.latched[(self.bank2 - 0x08) as usize];
            }
        }
        if self.ram.is_empty() {
            return 0xFF;
        }
        let i = (self.ram_off | (addr as usize & 0x1FFF)) & (self.ram.len() - 1);
        self.ram[i]
    }

    pub fn write_ram(&mut self, addr: u16, v: u8) {
        if !self.ram_enabled && self.kind != Kind::Mbc0 {
            return;
        }
        if self.rtc_selected() {
            let reg = self.bank2;
            if let Some(rtc) = &mut self.rtc {
                rtc.write(reg, v);
            }
            return;
        }
        if self.ram.is_empty() {
            return;
        }
        let i = (self.ram_off | (addr as usize & 0x1FFF)) & (self.ram.len() - 1);
        self.ram[i] = v;
    }

    /// Fait avancer la RTC de `m_cycles` cycles machine.
    pub fn advance_rtc(&mut self, m_cycles: u64) {
        if let Some(rtc) = &mut self.rtc {
            rtc.advance(m_cycles);
        }
    }

    /// Sauvegarde batterie : RAM, puis 48 octets RTC (format BGB/VBA, horodatage à 0)
    /// si la cartouche a une horloge. `None` sans batterie.
    pub fn export_save(&self) -> Option<Vec<u8>> {
        if !self.battery {
            return None;
        }
        let mut out = self.ram.clone();
        if let Some(rtc) = &self.rtc {
            for r in rtc.regs().iter().chain(rtc.latched.iter()) {
                out.extend_from_slice(&(*r as u32).to_le_bytes());
            }
            // raccourci : horodatage Unix à 0 (le cœur n'a pas d'horloge), à fournir par l'interface.
            out.extend_from_slice(&0u64.to_le_bytes());
        }
        Some(out)
    }

    /// Recharge une sauvegarde ; les octets manquants ou en trop sont ignorés.
    pub fn import_save(&mut self, data: &[u8]) {
        let n = self.ram.len().min(data.len());
        self.ram[..n].copy_from_slice(&data[..n]);
        if let Some(rtc) = &mut self.rtc {
            let rest = &data[n..];
            if rest.len() >= 40 {
                let r = |i: usize| rest[i * 4];
                rtc.s = r(0) & 0x3F;
                rtc.m = r(1) & 0x3F;
                rtc.h = r(2) & 0x1F;
                rtc.dl = r(3);
                rtc.dh = r(4) & 0xC1;
                rtc.latched = [r(5), r(6), r(7), r(8), r(9)];
            }
        }
    }

    /// Titre de l'en-tête (ASCII imprimable seulement).
    pub fn title(&self) -> String {
        self.rom[0x134..0x144]
            .iter()
            .take_while(|&&b| b != 0)
            .filter(|b| b.is_ascii_graphic() || **b == b' ')
            .map(|&b| b as char)
            .collect()
    }
}
