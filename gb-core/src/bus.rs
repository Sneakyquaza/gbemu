//! Bus mémoire : carte mémoire DMG, registres d'E/S, interruptions, DMA OAM, série, joypad.
//! Chaque accès du CPU coûte un cycle machine et fait avancer timer, PPU, DMA et série.

use crate::apu::Apu;
use crate::cartridge::Cartridge;
use crate::etat::{verifie, Ecrit, ErreurEtat, Lecteur};
use crate::ppu::Ppu;
use crate::timer::Timer;

const IRQ_TIMER: u8 = 0x04;
const IRQ_SERIAL: u8 = 0x08;
const IRQ_JOYPAD: u8 = 0x10;

pub struct Bus {
    pub cart: Cartridge,
    wram: [u8; 0x2000],
    hram: [u8; 0x7F],
    pub ppu: Ppu,
    timer: Timer,
    pub apu: Apu,
    /// Valeur de `cycles` jusqu'où l'APU a été exécuté (il avance par lots, voir `sync_apu`).
    apu_cycles: u64,
    pub if_: u8,
    pub ie: u8,
    p1: u8,
    /// Boutons enfoncés (bit à 1) : 0-3 croix (droite, gauche, haut, bas), 4-7 A, B, Select, Start.
    buttons: u8,
    sb: u8,
    sc: u8,
    serial_cycles: u32,
    pub serial_out: Vec<u8>,
    dma_reg: u8,
    dma_active: bool,
    dma_src: u16,
    dma_idx: u16,
    dma_start: Option<(u16, u8)>,
    /// Cycles machine écoulés depuis la mise sous tension.
    pub cycles: u64,
}

impl Bus {
    pub fn new(cart: Cartridge) -> Self {
        let mut bus = Bus {
            cart,
            wram: [0; 0x2000],
            hram: [0; 0x7F],
            ppu: Ppu::new(),
            timer: Timer::new(),
            apu: Apu::new(),
            apu_cycles: 0,
            if_: 0xE1,
            ie: 0,
            p1: 0x00,
            buttons: 0,
            sb: 0,
            sc: 0x7E,
            serial_cycles: 0,
            serial_out: Vec::new(),
            dma_reg: 0xFF,
            dma_active: false,
            dma_src: 0,
            dma_idx: 0,
            dma_start: None,
            cycles: 0,
        };
        bus.apu.recale_div(bus.timer.div);
        bus
    }

    /// Sauvegarde d'état du bus et de tout le matériel (la sortie série, journal de l'hôte, n'en fait pas partie).
    pub fn sauve(&self, e: &mut Vec<u8>) {
        e.octets(&self.wram);
        e.octets(&self.hram);
        e.u64(self.cycles);
        e.u64(self.apu_cycles);
        e.octets(&[self.if_, self.ie, self.p1, self.buttons, self.sb, self.sc]);
        e.u32(self.serial_cycles);
        e.u8(self.dma_reg);
        e.bool(self.dma_active);
        e.u16(self.dma_src);
        e.u16(self.dma_idx);
        let (src, delai) = self.dma_start.unwrap_or_default();
        e.bool(self.dma_start.is_some());
        e.u16(src);
        e.u8(delai);
        self.timer.sauve(e);
        self.ppu.sauve(e);
        self.apu.sauve(e);
        self.cart.sauve(e);
    }

    pub fn charge(&mut self, l: &mut Lecteur) -> Result<(), ErreurEtat> {
        l.octets(&mut self.wram)?;
        l.octets(&mut self.hram)?;
        self.cycles = l.u64()?;
        self.apu_cycles = l.u64()?;
        verifie(self.apu_cycles <= self.cycles, "cycles")?;
        let mut r = [0u8; 6];
        l.octets(&mut r)?;
        [self.if_, self.ie, self.p1, self.buttons, self.sb, self.sc] = r;
        self.p1 &= 0x30;
        self.serial_cycles = l.u32()?;
        verifie(self.serial_cycles <= 1024, "série")?;
        self.dma_reg = l.u8()?;
        self.dma_active = l.bool()?;
        self.dma_src = l.u16()?;
        self.dma_idx = l.u16_max(0xA0, "DMA")?;
        let attente = l.bool()?;
        let (src, delai) = (l.u16()?, l.u8()?);
        self.dma_start = attente.then_some((src, delai));
        self.timer.charge(l)?;
        self.ppu.charge(l)?;
        self.apu.charge(l)?;
        self.cart.charge(l)
    }

    /// Rattrape les cycles son en attente (avant tout accès à l'APU).
    pub fn sync_apu(&mut self) {
        self.apu.avance(self.cycles - self.apu_cycles);
        self.apu_cycles = self.cycles;
    }

    /// STOP et écriture de DIV : compteur interne remis à zéro.
    pub fn reset_div(&mut self) {
        self.io_write(0xFF04, 0);
    }

    #[inline]
    pub fn pending_irq(&self) -> u8 {
        self.ie & self.if_ & 0x1F
    }

    /// Avance tout le matériel d'un cycle machine (4 points).
    #[inline]
    pub fn tick(&mut self) {
        self.cycles += 1;
        if self.timer.tick() {
            self.if_ |= IRQ_TIMER;
        }
        self.if_ |= self.ppu.tick();
        if self.dma_start.is_some() || self.dma_active {
            self.tick_dma();
        }
        if self.serial_cycles > 0 {
            self.serial_cycles -= 1;
            if self.serial_cycles == 0 {
                self.sc &= 0x7F;
                self.if_ |= IRQ_SERIAL;
            }
        }
    }

    fn tick_dma(&mut self) {
        if let Some((src, delay)) = self.dma_start {
            if delay <= 1 {
                self.dma_start = None;
                self.dma_active = true;
                self.dma_src = src;
                self.dma_idx = 0;
            } else {
                self.dma_start = Some((src, delay - 1));
            }
        }
        if self.dma_active {
            // L'OAM reste bloquée un cycle après le dernier octet copié.
            if self.dma_idx == 0xA0 {
                self.dma_active = false;
            } else {
                let v = self.read_raw(self.dma_src.wrapping_add(self.dma_idx));
                self.ppu.oam[self.dma_idx as usize] = v;
                self.dma_idx += 1;
            }
        }
    }

    /// Lecture CPU : un cycle, puis l'accès.
    #[inline]
    pub fn read(&mut self, addr: u16) -> u8 {
        self.tick();
        // raccourci : seule l'OAM est bloquée pendant le DMA (pas de conflit de bus externe/VRAM).
        if self.dma_active && (0xFE00..0xFF00).contains(&addr) {
            return 0xFF;
        }
        if (0xFF10..0xFF40).contains(&addr) {
            self.sync_apu();
        }
        self.read_raw(addr)
    }

    /// Écriture CPU : un cycle, puis l'accès.
    #[inline]
    pub fn write(&mut self, addr: u16, v: u8) {
        self.tick();
        if self.dma_active && (0xFE00..0xFF00).contains(&addr) {
            return;
        }
        self.write_raw(addr, v);
    }

    /// Lecture sans cycle ni blocage DMA (source DMA, débogage).
    pub fn read_raw(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.cart.read_rom(addr),
            0x8000..=0x9FFF => self.ppu.vram[(addr - 0x8000) as usize],
            0xA000..=0xBFFF => self.cart.read_ram(addr),
            0xC000..=0xDFFF => self.wram[(addr - 0xC000) as usize],
            0xE000..=0xFDFF => self.wram[(addr - 0xE000) as usize],
            0xFE00..=0xFE9F => self.ppu.oam[(addr - 0xFE00) as usize],
            0xFEA0..=0xFEFF => 0x00,
            0xFF00..=0xFF7F => self.io_read(addr),
            0xFF80..=0xFFFE => self.hram[(addr - 0xFF80) as usize],
            0xFFFF => self.ie,
        }
    }

    fn write_raw(&mut self, addr: u16, v: u8) {
        match addr {
            0x0000..=0x7FFF => self.cart.write_rom(addr, v),
            0x8000..=0x9FFF => self.ppu.vram[(addr - 0x8000) as usize] = v,
            0xA000..=0xBFFF => self.cart.write_ram(addr, v),
            0xC000..=0xDFFF => self.wram[(addr - 0xC000) as usize] = v,
            0xE000..=0xFDFF => self.wram[(addr - 0xE000) as usize] = v,
            0xFE00..=0xFE9F => self.ppu.oam[(addr - 0xFE00) as usize] = v,
            0xFEA0..=0xFEFF => {}
            0xFF00..=0xFF7F => self.io_write(addr, v),
            0xFF80..=0xFFFE => self.hram[(addr - 0xFF80) as usize] = v,
            0xFFFF => self.ie = v,
        }
    }

    fn joypad_read(&self) -> u8 {
        let mut low = 0x0F;
        if self.p1 & 0x10 == 0 {
            low &= !(self.buttons & 0x0F);
        }
        if self.p1 & 0x20 == 0 {
            low &= !(self.buttons >> 4);
        }
        0xC0 | self.p1 | low
    }

    pub fn set_buttons(&mut self, pressed: u8) {
        let before = self.joypad_read();
        self.buttons = pressed;
        // Interruption sur passage d'une ligne de 1 à 0.
        if before & !self.joypad_read() & 0x0F != 0 {
            self.if_ |= IRQ_JOYPAD;
        }
    }

    fn io_read(&self, addr: u16) -> u8 {
        match addr {
            0xFF00 => self.joypad_read(),
            0xFF01 => self.sb,
            0xFF02 => self.sc | 0x7E,
            0xFF04..=0xFF07 => self.timer.read(addr),
            0xFF0F => self.if_ | 0xE0,
            0xFF10..=0xFF3F => self.apu.read(addr),
            0xFF46 => self.dma_reg,
            0xFF40..=0xFF4B => self.ppu.read(addr),
            _ => 0xFF,
        }
    }

    fn io_write(&mut self, addr: u16, v: u8) {
        match addr {
            0xFF00 => self.p1 = v & 0x30,
            0xFF01 => self.sb = v,
            0xFF02 => {
                self.sc = v & 0x81;
                if v & 0x81 == 0x81 {
                    // Horloge interne, pas de partenaire : l'octet sort, 0xFF entre.
                    // raccourci : transfert en 1024 cycles fixes, sans alignement sur l'horloge série.
                    self.serial_out.push(self.sb);
                    self.sb = 0xFF;
                    self.serial_cycles = 1024;
                }
            }
            0xFF04..=0xFF07 => {
                if addr == 0xFF04 {
                    // Remise à zéro de DIV : front descendant si son bit 4 était à 1.
                    self.sync_apu();
                    if self.timer.div & 0x1000 != 0 {
                        self.apu.frame_seq();
                    }
                }
                if self.timer.write(addr, v) {
                    self.if_ |= IRQ_TIMER;
                }
                if addr == 0xFF04 {
                    self.apu.recale_div(self.timer.div);
                }
            }
            0xFF0F => self.if_ = v & 0x1F,
            0xFF10..=0xFF3F => {
                self.sync_apu();
                self.apu.write(addr, v);
            }
            0xFF46 => {
                self.dma_reg = v;
                // Les sources E000-FFFF lisent la WRAM (C000-DFFF) sur DMG.
                let src = if v >= 0xE0 { v - 0x20 } else { v };
                self.dma_start = Some(((src as u16) << 8, 2));
            }
            0xFF40..=0xFF4B => self.if_ |= self.ppu.write(addr, v),
            _ => {}
        }
    }
}
