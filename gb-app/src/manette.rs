//! Manettes (gilrs) : état tenu des boutons, fronts d'appui et répétition des directions.

use gilrs::{Axis, Button, Gilrs};

/// Boutons logiques (bits).
pub mod b {
    pub const HAUT: u16 = 1 << 0;
    pub const BAS: u16 = 1 << 1;
    pub const GAUCHE: u16 = 1 << 2;
    pub const DROITE: u16 = 1 << 3;
    pub const SUD: u16 = 1 << 4;
    pub const EST: u16 = 1 << 5;
    pub const OUEST: u16 = 1 << 6;
    pub const NORD: u16 = 1 << 7;
    pub const START: u16 = 1 << 8;
    pub const SELECT: u16 = 1 << 9;
    pub const L1: u16 = 1 << 10;
    pub const R1: u16 = 1 << 11;
    pub const L2: u16 = 1 << 12;
    pub const R2: u16 = 1 << 13;
    pub const DIRECTIONS: u16 = HAUT | BAS | GAUCHE | DROITE;
}

pub struct Manettes {
    gilrs: Option<Gilrs>,
    tenus: u16,
    /// Directions : instant de la prochaine répétition.
    repetition: f64,
}

#[derive(Default, Clone, Copy)]
pub struct Etat {
    pub tenus: u16,
    /// Nouveaux appuis (et répétitions des directions tenues).
    pub appuis: u16,
    pub connectee: bool,
}

impl Manettes {
    pub fn ouvrir() -> Manettes {
        Manettes {
            gilrs: Gilrs::new().ok(),
            tenus: 0,
            repetition: 0.0,
        }
    }

    pub fn lire(&mut self, maintenant: f64) -> Etat {
        let Some(g) = self.gilrs.as_mut() else {
            return Etat::default();
        };
        while g.next_event().is_some() {}
        let mut tenus = 0;
        let mut connectee = false;
        for (_, pad) in g.gamepads() {
            connectee = true;
            let boutons = [
                (Button::DPadUp, b::HAUT),
                (Button::DPadDown, b::BAS),
                (Button::DPadLeft, b::GAUCHE),
                (Button::DPadRight, b::DROITE),
                (Button::South, b::SUD),
                (Button::East, b::EST),
                (Button::West, b::OUEST),
                (Button::North, b::NORD),
                (Button::Start, b::START),
                (Button::Select, b::SELECT),
                (Button::LeftTrigger, b::L1),
                (Button::RightTrigger, b::R1),
                (Button::LeftTrigger2, b::L2),
                (Button::RightTrigger2, b::R2),
            ];
            for (btn, bit) in boutons {
                if pad.is_pressed(btn) {
                    tenus |= bit;
                }
            }
            let (x, y) = (pad.value(Axis::LeftStickX), pad.value(Axis::LeftStickY));
            if x < -0.5 {
                tenus |= b::GAUCHE;
            }
            if x > 0.5 {
                tenus |= b::DROITE;
            }
            if y > 0.5 {
                tenus |= b::HAUT;
            }
            if y < -0.5 {
                tenus |= b::BAS;
            }
        }
        let mut appuis = tenus & !self.tenus;
        let dirs = tenus & b::DIRECTIONS;
        if appuis & b::DIRECTIONS != 0 {
            self.repetition = maintenant + 0.38;
        } else if dirs != 0 && maintenant >= self.repetition {
            appuis |= dirs;
            self.repetition = maintenant + 0.075;
        }
        self.tenus = tenus;
        Etat {
            tenus,
            appuis,
            connectee,
        }
    }
}
