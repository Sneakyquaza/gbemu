//! Cœur d'émulation Game Boy (DMG) et Game Boy Color (CGB) : CPU SM83, bus, timer, PPU,
//! cartouches MBC0/1/3/5, son.
//! Aucune E/S : l'appelant fournit la ROM et les sauvegardes en octets et lit l'écran en mémoire.

mod apu;
mod bus;
mod cartridge;
mod cpu;
mod etat;
mod ppu;
mod timer;

use etat::Ecrit;

pub use cartridge::LoadError;
pub use etat::ErreurEtat;
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

/// Cycles machine par image (70 224 points / 4) à vitesse normale (le double en double vitesse CGB).
pub const CYCLES_PAR_IMAGE: u64 = 17_556;
const POINTS_PAR_IMAGE: u64 = CYCLES_PAR_IMAGE * 4;

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
    /// Charge une ROM ; démarre directement en 0x0100 avec les registres post-boot.
    /// Mode CGB si l'en-tête (0x143) vaut 0x80 ou 0xC0, sinon DMG.
    pub fn new(rom: &[u8]) -> Result<Self, LoadError> {
        let cgb = matches!(rom.get(0x143), Some(0x80 | 0xC0));
        Self::new_mode(rom, cgb)
    }

    /// Comme [`GameBoy::new`] mais avec le mode imposé (tests : ROM DMG exécutées en CGB, ou l'inverse).
    pub fn new_mode(rom: &[u8], cgb: bool) -> Result<Self, LoadError> {
        let cart = cartridge::Cartridge::new(rom)?;
        Ok(GameBoy {
            cpu: cpu::Cpu::new(cgb),
            bus: bus::Bus::new(cart, cgb),
        })
    }

    /// Vrai en mode Game Boy Color.
    pub fn is_cgb(&self) -> bool {
        self.bus.cgb
    }

    /// Exécute jusqu'au prochain début de VBlank (ou une durée d'image si l'écran est éteint).
    pub fn run_frame(&mut self) {
        let start = self.bus.points;
        self.bus.ppu.frame_ready = false;
        while !self.bus.ppu.frame_ready {
            let elapsed = self.bus.points - start;
            if elapsed >= 2 * POINTS_PAR_IMAGE
                || (!self.bus.ppu.lcd_on() && elapsed >= POINTS_PAR_IMAGE)
            {
                break;
            }
            self.cpu.step(&mut self.bus);
        }
        self.bus.cart.advance_rtc(self.bus.points / 4 - start / 4);
        self.bus.sync_apu();
    }

    /// Exécute une instruction.
    pub fn step(&mut self) {
        self.cpu.step(&mut self.bus);
    }

    /// Écran DMG : 160×144 teintes de 0 (blanc) à 3 (noir). En mode CGB, ce sont les index
    /// de couleur 0-3 avant palette (sans signification de couleur) : utiliser [`GameBoy::frame_rgb`].
    pub fn frame(&self) -> &[u8] {
        &self.bus.ppu.frame[..]
    }

    /// Écran RGB8 : 160×144×3 octets. CGB : couleurs BGR555 converties (voir
    /// [`GameBoy::set_color_correction`]) ; DMG : gris neutres 0xFF, 0xAA, 0x55, 0x00.
    pub fn frame_rgb(&self) -> &[u8] {
        &self.bus.ppu.rgb[..]
    }

    /// Correction des couleurs CGB façon écran d'origine (désactivée par défaut : conversion
    /// linéaire 5 → 8 bits). Réglage de sortie, hors sauvegarde d'état ; visible dès la ligne suivante.
    pub fn set_color_correction(&mut self, on: bool) {
        self.bus.ppu.set_correction(on);
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

    /// Sauvegarde d'état complète (CPU, mémoire, PPU avec l'écran, APU, cartouche),
    /// liée à la ROM par son CRC32. Rapide (copie de ~40 Kio + RAM cartouche) : utilisable
    /// pour un rembobinage. Ni la sortie série ni les échantillons son en attente n'y sont.
    pub fn save_state(&self) -> Vec<u8> {
        let mut e = Vec::with_capacity(64 * 1024);
        e.octets(&etat::MAGIE);
        e.u16(etat::VERSION);
        e.u32(self.bus.cart.crc);
        self.cpu.sauve(&mut e);
        self.bus.sauve(&mut e);
        e
    }

    /// Recharge un état de [`GameBoy::save_state`]. En cas d'erreur (autre ROM, version,
    /// données tronquées ou corrompues), la console reste exactement dans son état précédent.
    /// Vide la sortie série ; les échantillons son en attente sont gardés.
    pub fn load_state(&mut self, data: &[u8]) -> Result<(), ErreurEtat> {
        let mut l = etat::Lecteur::new(data);
        let mut magie = [0u8; 4];
        l.octets(&mut magie).map_err(|_| ErreurEtat::Format)?;
        if magie != etat::MAGIE {
            return Err(ErreurEtat::Format);
        }
        let version = l.u16()?;
        if version != etat::VERSION {
            return Err(ErreurEtat::Version(version));
        }
        if l.u32()? != self.bus.cart.crc {
            return Err(ErreurEtat::AutreRom);
        }
        let avant = self.save_state();
        let r = self
            .cpu
            .charge(&mut l)
            .and_then(|_| self.bus.charge(&mut l));
        let r = r.and_then(|_| {
            if l.fini() {
                Ok(())
            } else {
                Err(ErreurEtat::Taille)
            }
        });
        if r.is_err() {
            // Retour à l'état d'avant : il vient d'être produit, sa relecture ne peut pas échouer.
            let mut l = etat::Lecteur::new(&avant[etat::TAILLE_ENTETE..]);
            let ok = self
                .cpu
                .charge(&mut l)
                .and_then(|_| self.bus.charge(&mut l));
            debug_assert!(ok.is_ok());
            return r;
        }
        self.bus.serial_out.clear();
        Ok(())
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
