//! Bus mémoire : carte mémoire DMG/CGB, registres d'E/S, interruptions, DMA OAM, HDMA, série, joypad.
//! Chaque accès du CPU coûte un cycle machine et fait avancer timer, PPU, DMA et série.
//! Double vitesse CGB : le CPU, le timer, le DMA OAM et la série vont 2× plus vite,
//! le PPU, l'APU et le HDMA gardent la vitesse normale (2 points par cycle machine au lieu de 4).

use crate::apu::Apu;
use crate::cartridge::Cartridge;
use crate::etat::{verifie, Ecrit, ErreurEtat, Lecteur};
use crate::ppu::{Ppu, DEBUT_HBLANK};
use crate::timer::Timer;

const IRQ_TIMER: u8 = 0x04;
const IRQ_SERIAL: u8 = 0x08;
const IRQ_JOYPAD: u8 = 0x10;

pub struct Bus {
    pub cart: Cartridge,
    pub cgb: bool,
    /// 8 banques de 4 Kio (DMG : banques 0 et 1 seulement).
    wram: Box<[u8; 0x8000]>,
    /// Décalage de la banque WRAM vue en D000-DFFF (SVBK, 0 → 1).
    wram_banque: usize,
    svbk: u8,
    /// KEY1 : vitesse double en cours, et changement armé (fait au prochain STOP).
    double: bool,
    key1_arme: bool,
    /// Points par cycle machine : 4, ou 2 en double vitesse.
    pas: u32,
    /// HDMA : source, destination (décalage VRAM), blocs de 16 octets restants - 1 (FF55),
    /// transfert HBlank en cours.
    hdma_src: u16,
    hdma_dst: u16,
    hdma_len: u8,
    hdma_hblank: bool,
    /// Infrarouge RP (FF56) : registre seul, jamais de lumière reçue.
    rp: u8,
    /// Registres CGB sans fonction FF72-FF75 (lus/écrits seulement).
    inutilises: [u8; 4],
    hram: [u8; 0x7F],
    pub ppu: Ppu,
    timer: Timer,
    pub apu: Apu,
    /// Valeur de `points` jusqu'où l'APU a été exécuté (il avance par lots, voir `sync_apu`).
    apu_points: u64,
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
    /// Temps écoulé depuis la mise sous tension en points (4 par cycle machine à vitesse normale).
    pub points: u64,
}

impl Bus {
    pub fn new(cart: Cartridge, cgb: bool) -> Self {
        let mut bus = Bus {
            cart,
            cgb,
            wram: Box::new([0; 0x8000]),
            wram_banque: 0x1000,
            svbk: 0,
            double: false,
            key1_arme: false,
            pas: 4,
            hdma_src: 0,
            hdma_dst: 0,
            hdma_len: 0x7F,
            hdma_hblank: false,
            rp: 0,
            inutilises: [0; 4],
            hram: [0; 0x7F],
            ppu: Ppu::new(cgb),
            timer: Timer::new(cgb),
            apu: Apu::new(cgb),
            apu_points: 0,
            if_: 0xE1,
            ie: 0,
            // Boot CGB : aucune ligne du joypad sélectionnée.
            p1: if cgb { 0x30 } else { 0x00 },
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
            points: 0,
        };
        bus.apu.recale_div(bus.timer.div, false);
        bus
    }

    /// WRAM sauvée : 8 Kio en DMG, 32 Kio en CGB.
    fn taille_wram(&self) -> usize {
        if self.cgb {
            0x8000
        } else {
            0x2000
        }
    }

    /// Sauvegarde d'état du bus et de tout le matériel (la sortie série, journal de l'hôte, n'en fait pas partie).
    pub fn sauve(&self, e: &mut Vec<u8>) {
        e.bool(self.cgb);
        e.octets(&self.wram[..self.taille_wram()]);
        e.octets(&self.hram);
        e.u64(self.points);
        e.u64(self.apu_points);
        e.octets(&[self.svbk, self.hdma_len, self.rp]);
        e.octets(&self.inutilises);
        e.bool(self.double);
        e.bool(self.key1_arme);
        e.bool(self.hdma_hblank);
        e.u16(self.hdma_src);
        e.u16(self.hdma_dst);
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
        verifie(l.bool()? == self.cgb, "mode CGB")?;
        let n = self.taille_wram();
        l.octets(&mut self.wram[..n])?;
        l.octets(&mut self.hram)?;
        self.points = l.u64()?;
        self.apu_points = l.u64()?;
        verifie(self.apu_points <= self.points, "cycles")?;
        self.svbk = l.u8_max(7, "SVBK")?;
        self.hdma_len = l.u8_max(0x7F, "HDMA")?;
        self.rp = l.u8()? & 0xC1;
        l.octets(&mut self.inutilises)?;
        self.inutilises[3] &= 0x70;
        self.double = l.bool()?;
        self.key1_arme = l.bool()?;
        self.hdma_hblank = l.bool()?;
        self.hdma_src = l.u16()? & 0xFFF0;
        self.hdma_dst = l.u16()? & 0x1FF0;
        verifie(
            self.cgb || (self.svbk == 0 && !self.double && !self.key1_arme && !self.hdma_hblank),
            "registres CGB",
        )?;
        self.wram_banque = self.svbk.max(1) as usize * 0x1000;
        self.pas = if self.double { 2 } else { 4 };
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
        self.apu.avance(self.points / 4 - self.apu_points / 4);
        self.apu_points = self.points;
    }

    /// STOP : bascule de vitesse si KEY1 est armé (CGB), puis DIV remis à zéro.
    // raccourci : ni veille (réveil par touche) ni pause de ~2050 cycles au changement de vitesse.
    pub fn stop(&mut self) {
        if self.key1_arme {
            self.sync_apu();
            self.key1_arme = false;
            self.double = !self.double;
            self.pas = if self.double { 2 } else { 4 };
        }
        self.io_write(0xFF04, 0);
    }

    #[inline]
    pub fn pending_irq(&self) -> u8 {
        self.ie & self.if_ & 0x1F
    }

    /// Avance tout le matériel d'un cycle machine (4 points, 2 en double vitesse).
    #[inline]
    pub fn tick(&mut self) {
        self.points += self.pas as u64;
        if self.timer.tick() {
            self.if_ |= IRQ_TIMER;
        }
        let r = self.ppu.tick(self.pas);
        if r != 0 {
            self.if_ |= r & 0x1F;
            if r & DEBUT_HBLANK != 0 && self.hdma_hblank {
                self.hdma_bloc();
            }
        }
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

    /// Copie un bloc HDMA de 16 octets vers la banque VRAM courante ; le CPU attend
    /// 8 cycles machine à vitesse normale (16 en double vitesse).
    #[cold]
    #[inline(never)]
    fn hdma_bloc(&mut self) {
        for _ in 0..16 {
            let v = self.read_raw(self.hdma_src);
            self.ppu.vram[self.ppu.vram_banque + self.hdma_dst as usize] = v;
            self.hdma_src = self.hdma_src.wrapping_add(1);
            self.hdma_dst = (self.hdma_dst + 1) & 0x1FFF;
        }
        self.hdma_len = self.hdma_len.wrapping_sub(1) & 0x7F;
        if self.hdma_len == 0x7F {
            self.hdma_hblank = false;
        }
        for _ in 0..32 / self.pas {
            self.tick();
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
            0x8000..=0x9FFF => self.ppu.vram[self.ppu.vram_banque + (addr - 0x8000) as usize],
            0xA000..=0xBFFF => self.cart.read_ram(addr),
            0xC000..=0xCFFF => self.wram[(addr - 0xC000) as usize],
            0xD000..=0xDFFF => self.wram[self.wram_banque + (addr - 0xD000) as usize],
            0xE000..=0xEFFF => self.wram[(addr - 0xE000) as usize],
            0xF000..=0xFDFF => self.wram[self.wram_banque + (addr - 0xF000) as usize],
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
            0x8000..=0x9FFF => {
                self.ppu.vram[self.ppu.vram_banque + (addr - 0x8000) as usize] = v;
            }
            0xA000..=0xBFFF => self.cart.write_ram(addr, v),
            0xC000..=0xCFFF => self.wram[(addr - 0xC000) as usize] = v,
            0xD000..=0xDFFF => self.wram[self.wram_banque + (addr - 0xD000) as usize] = v,
            0xE000..=0xEFFF => self.wram[(addr - 0xE000) as usize] = v,
            0xF000..=0xFDFF => self.wram[self.wram_banque + (addr - 0xF000) as usize] = v,
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
            0xFF4F | 0xFF68..=0xFF6C if self.cgb => self.ppu.read(addr),
            0xFF4D if self.cgb => 0x7E | (self.double as u8) << 7 | self.key1_arme as u8,
            0xFF55 if self.cgb => (!self.hdma_hblank as u8) << 7 | self.hdma_len,
            0xFF56 if self.cgb => 0x3E | self.rp,
            0xFF70 if self.cgb => 0xF8 | self.svbk,
            0xFF72..=0xFF74 if self.cgb => self.inutilises[(addr - 0xFF72) as usize],
            0xFF75 if self.cgb => 0x8F | self.inutilises[3],
            0xFF76 | 0xFF77 if self.cgb => 0x00,
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
                    // (bit 5 de DIV en double vitesse : le séquenceur garde 512 Hz).
                    let bit = if self.double { 0x2000 } else { 0x1000 };
                    if self.timer.div & bit != 0 {
                        self.apu.frame_seq();
                    }
                }
                if self.timer.write(addr, v) {
                    self.if_ |= IRQ_TIMER;
                }
                if addr == 0xFF04 {
                    self.apu.recale_div(self.timer.div, self.double);
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
            0xFF4F | 0xFF68..=0xFF6C if self.cgb => {
                self.ppu.write(addr, v);
            }
            0xFF4D if self.cgb => self.key1_arme = v & 1 != 0,
            0xFF51 if self.cgb => self.hdma_src = (self.hdma_src & 0x00FF) | (v as u16) << 8,
            0xFF52 if self.cgb => self.hdma_src = (self.hdma_src & 0xFF00) | (v & 0xF0) as u16,
            0xFF53 if self.cgb => {
                self.hdma_dst = (self.hdma_dst & 0x00FF) | ((v & 0x1F) as u16) << 8;
            }
            0xFF54 if self.cgb => self.hdma_dst = (self.hdma_dst & 0x1F00) | (v & 0xF0) as u16,
            0xFF55 if self.cgb => {
                if self.hdma_hblank && v & 0x80 == 0 {
                    // Arrêt du transfert HBlank : FF55 lit alors 0x80 | blocs restants - 1.
                    self.hdma_hblank = false;
                } else {
                    self.hdma_len = v & 0x7F;
                    if v & 0x80 != 0 {
                        // raccourci : pas de premier bloc immédiat si le LCD est éteint ou déjà en HBlank.
                        self.hdma_hblank = true;
                    } else {
                        // DMA général : tout de suite, CPU arrêté pendant la copie.
                        while {
                            self.hdma_bloc();
                            self.hdma_len != 0x7F
                        } {}
                    }
                }
            }
            0xFF56 if self.cgb => self.rp = v & 0xC1,
            0xFF72..=0xFF74 if self.cgb => self.inutilises[(addr - 0xFF72) as usize] = v,
            0xFF75 if self.cgb => self.inutilises[3] = v & 0x70,
            0xFF70 if self.cgb => {
                self.svbk = v & 7;
                self.wram_banque = self.svbk.max(1) as usize * 0x1000;
            }
            _ => {}
        }
    }
}
