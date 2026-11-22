//! Cœur d'émulation Game Boy (DMG) : CPU SM83, bus, timer, PPU, cartouches MBC0/1/3/5, son.
//! Aucune E/S : l'appelant fournit la ROM et les sauvegardes en octets et lit l'écran en mémoire.

mod apu;
mod bus;
mod cartridge;
mod cpu;
mod ppu;
mod timer;

pub use cartridge::LoadError;
pub use ppu::{HAUTEUR, LARGEUR};

/// Boutons pour [`GameBoy::set_buttons`] (bit à 1 = enfoncé).
pub mod bouton {
    pub const DROITE: u8 = 0x01;
    pub const GAUCHE: u8 = 0x02;
    pub const HAUT: u8 = 0x04;
    pub const BAS: u8 = 0x08;
    pub const A: u8 = 0x10;
    pub const B: u8 = 0x20;
    pub const SELECT: u8 = 0x40;
    pub const START: u8 = 0x80;
}

/// Cycles machine par image (70 224 points / 4).
pub const CYCLES_PAR_IMAGE: u64 = 17_556;

/// Registres CPU visibles (tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Registres {
    pub a: u8,
    pub f: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub sp: u16,
    pub pc: u16,
}

pub struct GameBoy {
    cpu: cpu::Cpu,
    bus: bus::Bus,
}

impl GameBoy {
    /// Charge une ROM ; démarre directement en 0x0100 avec les registres post-boot DMG.
    pub fn new(rom: &[u8]) -> Result<Self, LoadError> {
        let cart = cartridge::Cartridge::new(rom)?;
        Ok(GameBoy {
            cpu: cpu::Cpu::new(),
            bus: bus::Bus::new(cart),
        })
    }

    /// Exécute jusqu'au prochain début de VBlank (ou une durée d'image si l'écran est éteint).
    pub fn run_frame(&mut self) {
        let start = self.bus.cycles;
        self.bus.ppu.frame_ready = false;
        while !self.bus.ppu.frame_ready {
            let elapsed = self.bus.cycles - start;
            if elapsed >= 2 * CYCLES_PAR_IMAGE
                || (!self.bus.ppu.lcd_on() && elapsed >= CYCLES_PAR_IMAGE)
            {
                break;
            }
            self.cpu.step(&mut self.bus);
        }
        self.bus.cart.advance_rtc(self.bus.cycles - start);
        self.bus.sync_apu();
    }

    /// Exécute une instruction.
    pub fn step(&mut self) {
        self.cpu.step(&mut self.bus);
    }

    /// Écran : 160×144 teintes de 0 (blanc) à 3 (noir).
    pub fn frame(&self) -> &[u8] {
        &self.bus.ppu.frame[..]
    }

    /// Ajoute à `out` les échantillons son stéréo entrelacés (G, D) produits depuis le dernier appel.
    /// Sans appel, au plus 1 s est gardée.
    pub fn drain_samples(&mut self, out: &mut Vec<f32>) {
        self.bus.sync_apu();
        self.bus.apu.drain_samples(out);
    }

    /// Fréquence d'échantillonnage du son en Hz (48 000 par défaut, bornée à 8 000-192 000).
    pub fn set_sample_rate(&mut self, hz: u32) {
        self.bus.apu.set_sample_rate(hz);
    }

    /// Octets sortis par le port série depuis le démarrage.
    pub fn serial_output(&self) -> &[u8] {
        &self.bus.serial_out
    }

    pub fn set_buttons(&mut self, pressed: u8) {
        self.bus.set_buttons(pressed);
    }

    /// Sauvegarde batterie (RAM + RTC), `None` si la cartouche n'a pas de batterie.
    pub fn export_save(&self) -> Option<Vec<u8>> {
        self.bus.cart.export_save()
    }

    pub fn import_save(&mut self, data: &[u8]) {
        self.bus.cart.import_save(data);
    }

    pub fn title(&self) -> String {
        self.bus.cart.title()
    }

    /// Vrai si `LD B,B` a été exécuté (fin des tests mooneye).
    pub fn hit_ld_b_b(&self) -> bool {
        self.cpu.ld_b_b
    }

    pub fn registres(&self) -> Registres {
        let c = &self.cpu;
        Registres {
            a: c.a,
            f: c.f,
            b: c.b,
            c: c.c,
            d: c.d,
            e: c.e,
            h: c.h,
            l: c.l,
            sp: c.sp,
            pc: c.pc,
        }
    }

    /// Lecture mémoire sans effet de bord (tests, débogage).
    pub fn peek(&self, addr: u16) -> u8 {
        self.bus.read_raw(addr)
    }
}
