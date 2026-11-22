//! Cœur d'émulation Game Boy Advance : CPU ARM7TDMI, bus et carte mémoire, interruptions, timers,
//! DMA, touches, PPU (modes 0-5, sprites, fenêtres, effets, mosaïque), son (canaux Game Boy de
//! gb-core + son direct).
//! Aucune E/S : l'appelant fournit la ROM en octets et lit l'écran en mémoire.
//!
//! BIOS : jamais celui de Nintendo. Le BIOS libre Cult-of-GBA (MIT, DenSinH et fleroviux,
//! github.com/Cult-of-GBA/BIOS, commit 61158c3, licence dans `bios/LICENSE`) est embarqué ; il
//! sert les SWI et le vecteur d'IRQ. Démarrage direct par défaut (registres post-BIOS, saut en
//! 0x08000000).

mod apu;
mod bus;
mod cartouche;
mod cpu;
mod dma;
mod etat;
mod ppu;
mod timer;

use etat::Ecrit;

pub use cartouche::{ErreurChargement, TypeSauvegarde};
pub use etat::ErreurEtat;
pub use ppu::{HAUTEUR, LARGEUR};

/// BIOS de remplacement libre (16 Kio).
pub const BIOS_LIBRE: &[u8; 0x4000] = include_bytes!("../bios/cult-of-gba-bios.bin");

/// Cycles par image : 228 lignes × 1 232 cycles (16 777 216 Hz / 280 896 ≈ 59,73 Hz).
pub const CYCLES_PAR_IMAGE: u64 = 280_896;

/// Touches pour [`Gba::set_keys`] (bit à 1 = enfoncée), dans l'ordre de KEYINPUT.
pub mod touche {
    pub const A: u16 = 0x001;
    pub const B: u16 = 0x002;
    pub const SELECT: u16 = 0x004;
    pub const START: u16 = 0x008;
    pub const DROITE: u16 = 0x010;
    pub const GAUCHE: u16 = 0x020;
    pub const HAUT: u16 = 0x040;
    pub const BAS: u16 = 0x080;
    pub const R: u16 = 0x100;
    pub const L: u16 = 0x200;
}

pub struct Gba {
    cpu: cpu::Cpu,
    bus: bus::Bus,
    /// CRC32 de la ROM (sauvegardes d'état).
    crc: u32,
}

impl Gba {
    /// Charge une ROM. `bios` : `None` = BIOS libre embarqué et démarrage direct ; `Some` = BIOS
    /// fourni par l'utilisateur (16 Kio), exécuté depuis 0 (écran de démarrage compris).
    pub fn new(rom: &[u8], bios: Option<&[u8]>) -> Result<Self, ErreurChargement> {
        let cart = cartouche::Cartouche::new(rom)?;
        if let Some(b) = bios {
            if b.len() != 0x4000 {
                return Err(ErreurChargement::BiosTaille(b.len()));
            }
        }
        let mut bus = bus::Bus::new(cart, bios.unwrap_or(BIOS_LIBRE));
        let cpu = if bios.is_some() {
            cpu::Cpu::new_bios(&mut bus)
        } else {
            bus.apres_bios();
            cpu::Cpu::new_direct(&mut bus)
        };
        Ok(Gba {
            cpu,
            bus,
            crc: etat::crc32(rom),
        })
    }

    /// Exécute jusqu'au prochain début de VBlank (ligne 160).
    pub fn run_frame(&mut self) {
        let bus = &mut self.bus;
        let cpu = &mut self.cpu;
        bus.ppu.frame_ready = false;
        // Garde-fou : jamais plus de deux images de cycles par appel.
        let limite = bus.cycles + 2 * CYCLES_PAR_IMAGE;
        while !bus.ppu.frame_ready && bus.cycles < limite {
            if bus.halt {
                if bus.reveil() {
                    bus.halt = false;
                } else {
                    bus.cycles = bus.cycles.max(bus.prochain);
                    bus.evenements();
                    continue;
                }
            }
            if bus.irq_en_attente() && cpu.cpsr & cpu::I == 0 {
                cpu.irq(bus);
            }
            cpu.step(bus);
            if bus.cycles >= bus.prochain {
                bus.evenements();
            }
        }
        bus.apu.sync(bus.cycles);
    }

    /// Écran RGB8 (240×160×3).
    pub fn frame(&self) -> &[u8] {
        self.bus.ppu.frame()
    }

    /// Touches enfoncées (bits de [`touche`]).
    pub fn set_keys(&mut self, touches: u16) {
        self.bus.set_touches(touches);
    }

    /// Registres r0-r15 du mode courant ; r15 = adresse de la prochaine instruction.
    pub fn registres(&self) -> [u32; 16] {
        let mut r = self.cpu.r;
        r[15] = self.cpu.pc();
        r
    }

    pub fn cpsr(&self) -> u32 {
        self.cpu.cpsr
    }

    /// Vrai si le CPU tourne sur un saut vers lui-même (fin des ROM de test).
    pub fn en_boucle_infinie(&self) -> bool {
        self.cpu.boucle_infinie()
    }

    /// Lecture mémoire sans effet ni temps (tests, débogage).
    pub fn peek(&self, adr: u32) -> u8 {
        self.bus.lit8_brut(adr)
    }

    /// Ajoute à `out` les échantillons stéréo entrelacés (G, D) produits depuis le dernier appel
    /// (passe-haut façon condensateur ; au plus 1 s gardée sans lecture).
    pub fn drain_samples(&mut self, out: &mut Vec<f32>) {
        self.bus.apu.sync(self.bus.cycles);
        self.bus.apu.drain_samples(out);
    }

    /// Fréquence de sortie du son (Hz, 48 000 par défaut, bornée à 8 000-192 000).
    pub fn set_sample_rate(&mut self, hz: u32) {
        self.bus.apu.set_sample_rate(hz);
    }

    /// Cycles écoulés depuis le démarrage.
    pub fn cycles(&self) -> u64 {
        self.bus.cycles
    }

    /// Type de sauvegarde de la cartouche (détecté par les chaînes de la ROM).
    pub fn type_sauvegarde(&self) -> TypeSauvegarde {
        self.bus.cart.type_sauvegarde()
    }

    /// Sauvegarde de la cartouche (.sav) ; `None` tant qu'il n'y a rien à écrire.
    pub fn sauvegarde(&self) -> Option<Vec<u8>> {
        self.bus.cart.sauvegarde()
    }

    /// Recharge un .sav (taille quelconque, jamais de panique ; voir `Cartouche`).
    pub fn charger_sauvegarde(&mut self, data: &[u8]) {
        self.bus.cart.charger_sauvegarde(data);
    }

    /// Sauvegarde instantanée : en-tête `GBAE`, version, CRC32 de la ROM, puis CPU et bus.
    pub fn save_state(&self) -> Vec<u8> {
        let mut e = Vec::with_capacity(640 * 1024);
        e.octets(&etat::MAGIE);
        e.u16(etat::VERSION);
        e.u32(self.crc);
        self.cpu.sauve(&mut e);
        self.bus.sauve(&mut e);
        e
    }

    /// Recharge un état de [`Gba::save_state`]. En cas d'erreur (autre ROM, version, données
    /// tronquées ou corrompues), la console reste exactement dans son état précédent.
    pub fn load_state(&mut self, data: &[u8]) -> Result<(), ErreurEtat> {
        let mut l = etat::Lecteur::new(data);
        let mut magie = [0u8; 4];
        l.octets(&mut magie).map_err(|_| ErreurEtat::Format)?;
        if magie != etat::MAGIE {
            return Err(ErreurEtat::Format);
        }
        let v = l.u16()?;
        if v != etat::VERSION {
            return Err(ErreurEtat::Version(v));
        }
        if l.u32()? != self.crc {
            return Err(ErreurEtat::AutreRom);
        }
        let avant = self.save_state();
        let r = self
            .cpu
            .charge(&mut l)
            .and_then(|_| self.bus.charge(&mut l))
            .and_then(|_| {
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
        }
        r
    }
}
