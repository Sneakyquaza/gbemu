//! Console émulée par la partie : Game Boy / Game Boy Color (`gb-core`) ou Game Boy Advance
//! (`gba-core`), derrière une seule interface (écran, boutons, son, sauvegardes, états).

use crate::touches::{BIT_L, BIT_R};
use gb_core::{ErreurEtat, GameBoy};
use gba_core::{touche, Gba};

pub enum Machine {
    Gb(Box<GameBoy>),
    Gba(Box<Gba>),
}

/// Erreur d'état GBA rendue avec le type de `gb-core` (mêmes cas).
fn erreur_gba(e: gba_core::ErreurEtat) -> ErreurEtat {
    match e {
        gba_core::ErreurEtat::Format => ErreurEtat::Format,
        gba_core::ErreurEtat::Version(v) => ErreurEtat::Version(v),
        gba_core::ErreurEtat::AutreRom => ErreurEtat::AutreRom,
        gba_core::ErreurEtat::Taille => ErreurEtat::Taille,
        gba_core::ErreurEtat::Corrompu(c) => ErreurEtat::Corrompu(c),
    }
}

impl Machine {
    /// Démarre la ROM sur la bonne console (`gba` : fichier .gba). BIOS libre embarqué pour la GBA.
    pub fn new(rom: &[u8], gba: bool) -> Result<Machine, String> {
        if gba {
            let g = Gba::new(rom, None).map_err(|e| format!("ROM refusée : {e}"))?;
            Ok(Machine::Gba(Box::new(g)))
        } else {
            let g = GameBoy::new(rom).map_err(|e| format!("ROM refusée : {e:?}"))?;
            Ok(Machine::Gb(Box::new(g)))
        }
    }

    pub fn largeur(&self) -> usize {
        match self {
            Machine::Gb(_) => gb_core::LARGEUR,
            Machine::Gba(_) => gba_core::LARGEUR,
        }
    }

    pub fn hauteur(&self) -> usize {
        match self {
            Machine::Gb(_) => gb_core::HAUTEUR,
            Machine::Gba(_) => gba_core::HAUTEUR,
        }
    }

    pub fn is_cgb(&self) -> bool {
        matches!(self, Machine::Gb(g) if g.is_cgb())
    }

    pub fn is_gba(&self) -> bool {
        matches!(self, Machine::Gba(_))
    }

    /// Écran en couleurs natives (GBC, GBA) : la palette DMG ne s'applique pas.
    pub fn couleur(&self) -> bool {
        self.is_gba() || self.is_cgb()
    }

    pub fn set_color_correction(&mut self, oui: bool) {
        if let Machine::Gb(g) = self {
            g.set_color_correction(oui);
        }
    }

    pub fn set_sample_rate(&mut self, hz: u32) {
        match self {
            Machine::Gb(g) => g.set_sample_rate(hz),
            Machine::Gba(g) => g.set_sample_rate(hz),
        }
    }

    pub fn run_frame(&mut self) {
        match self {
            Machine::Gb(g) => g.run_frame(),
            Machine::Gba(g) => g.run_frame(),
        }
    }

    /// Boutons tenus : bits `gb_core::bouton` et `touches::BIT_L` / `BIT_R`.
    pub fn set_buttons(&mut self, b: u16) {
        match self {
            Machine::Gb(g) => g.set_buttons(b as u8),
            Machine::Gba(g) => {
                use gb_core::bouton as gb;
                let mut t = 0;
                for (de, vers) in [
                    (gb::A as u16, touche::A),
                    (gb::B as u16, touche::B),
                    (gb::SELECT as u16, touche::SELECT),
                    (gb::START as u16, touche::START),
                    (gb::DROITE as u16, touche::DROITE),
                    (gb::GAUCHE as u16, touche::GAUCHE),
                    (gb::HAUT as u16, touche::HAUT),
                    (gb::BAS as u16, touche::BAS),
                    (BIT_R, touche::R),
                    (BIT_L, touche::L),
                ] {
                    if b & de != 0 {
                        t |= vers;
                    }
                }
                g.set_keys(t);
            }
        }
    }

    /// Échantillons son (stéréo entrelacé G/D).
    pub fn drain_samples(&mut self, sortie: &mut Vec<f32>) {
        match self {
            Machine::Gb(g) => g.drain_samples(sortie),
            Machine::Gba(g) => g.drain_samples(sortie),
        }
    }

    pub fn save_state(&self) -> Vec<u8> {
        match self {
            Machine::Gb(g) => g.save_state(),
            Machine::Gba(g) => g.save_state(),
        }
    }

    pub fn load_state(&mut self, data: &[u8]) -> Result<(), ErreurEtat> {
        match self {
            Machine::Gb(g) => g.load_state(data),
            Machine::Gba(g) => g.load_state(data).map_err(erreur_gba),
        }
    }

    /// Sauvegarde de la cartouche à écrire, `None` s'il n'y en a pas (encore).
    pub fn export_save(&self) -> Option<Vec<u8>> {
        match self {
            Machine::Gb(g) => g.export_save(),
            Machine::Gba(g) => g.sauvegarde(),
        }
    }

    pub fn import_save(&mut self, data: &[u8]) {
        match self {
            Machine::Gb(g) => g.import_save(data),
            Machine::Gba(g) => g.charger_sauvegarde(data),
        }
    }

    /// Pixels RGB (largeur × hauteur × 3) : couleurs du cœur en GBC et GBA, palette donnée en DMG.
    pub fn ecran_rgb(&self, teintes: &[[u8; 3]; 4]) -> Vec<u8> {
        match self {
            Machine::Gba(g) => g.frame().to_vec(),
            Machine::Gb(g) if g.is_cgb() => g.frame_rgb().to_vec(),
            Machine::Gb(g) => g
                .frame()
                .iter()
                .flat_map(|&v| teintes[(v & 3) as usize])
                .collect(),
        }
    }
}
