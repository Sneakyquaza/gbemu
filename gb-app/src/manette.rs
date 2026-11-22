//! Manettes (gilrs) : branchement à chaud, plusieurs manettes, zone morte des sticks.
//! Chaque manette donne un instantané [`Pad`] (boutons et directions de stick tenus, fronts)
//! lu par les profils de touches ; l'[`Etat`] positionnel sert à la navigation des menus.

use gilrs::{Axis, Button, EventType, Gilrs};

/// Boutons logiques (bits) pour la navigation des menus.
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

/// Boutons reconnus : nom stable (fichier `touches.json`) et bit de navigation.
pub const BOUTONS: [(Button, &str, u16); 17] = [
    (Button::DPadUp, "CroixHaut", b::HAUT),
    (Button::DPadDown, "CroixBas", b::BAS),
    (Button::DPadLeft, "CroixGauche", b::GAUCHE),
    (Button::DPadRight, "CroixDroite", b::DROITE),
    (Button::South, "Sud", b::SUD),
    (Button::East, "Est", b::EST),
    (Button::West, "Ouest", b::OUEST),
    (Button::North, "Nord", b::NORD),
    (Button::Start, "Start", b::START),
    (Button::Select, "Select", b::SELECT),
    (Button::LeftTrigger, "L1", b::L1),
    (Button::RightTrigger, "R1", b::R1),
    (Button::LeftTrigger2, "L2", b::L2),
    (Button::RightTrigger2, "R2", b::R2),
    (Button::LeftThumb, "L3", 0),
    (Button::RightThumb, "R3", 0),
    (Button::Mode, "Guide", 0),
];

/// Axes de stick reconnus (deux sens chacun).
pub const AXES: [(Axis, &str); 4] = [
    (Axis::LeftStickX, "StickGaucheX"),
    (Axis::LeftStickY, "StickGaucheY"),
    (Axis::RightStickX, "StickDroitX"),
    (Axis::RightStickY, "StickDroitY"),
];

/// Zone morte radiale des sticks (sous ce rayon : stick au repos).
pub const ZONE_MORTE: f32 = 0.25;
/// Seuil à partir duquel un sens de stick compte comme appuyé.
pub const SEUIL_AXE: f32 = 0.5;

/// Instantané d'une manette branchée.
#[derive(Clone, Default, Debug)]
pub struct Pad {
    /// Identifiant gilrs de la session (change au rebranchement).
    pub id: usize,
    /// GUID SDL du modèle, en hexadécimal : clé du profil.
    pub uuid: String,
    pub nom: String,
    /// Bits d'index dans [`BOUTONS`].
    pub tenus: u32,
    pub appuis: u32,
    /// Bit `2 * index dans AXES + (sens positif)`.
    pub axes: u8,
    pub axes_appuis: u8,
}

fn bit_bouton(btn: Button) -> u32 {
    BOUTONS
        .iter()
        .position(|e| e.0 == btn)
        .map_or(0, |i| 1 << i)
}

fn bit_axe(axe: Axis, positif: bool) -> u8 {
    AXES.iter()
        .position(|e| e.0 == axe)
        .map_or(0, |i| 1 << (2 * i + positif as usize))
}

impl Pad {
    pub fn tenu(&self, btn: Button) -> bool {
        self.tenus & bit_bouton(btn) != 0
    }

    pub fn appui(&self, btn: Button) -> bool {
        self.appuis & bit_bouton(btn) != 0
    }

    pub fn axe(&self, axe: Axis, positif: bool) -> bool {
        self.axes & bit_axe(axe, positif) != 0
    }

    pub fn axe_appui(&self, axe: Axis, positif: bool) -> bool {
        self.axes_appuis & bit_axe(axe, positif) != 0
    }

    /// Premier bouton nouvellement appuyé.
    pub fn premier_appui(&self) -> Option<Button> {
        BOUTONS
            .iter()
            .enumerate()
            .find(|(i, _)| self.appuis & (1 << i) != 0)
            .map(|(_, e)| e.0)
    }

    /// Premier sens de stick nouvellement poussé.
    pub fn premier_axe(&self) -> Option<(Axis, bool)> {
        (0..AXES.len() * 2)
            .find(|i| self.axes_appuis & (1 << i) != 0)
            .map(|i| (AXES[i / 2].0, i % 2 == 1))
    }

    /// Boutons tenus autres que `sauf`.
    pub fn autres_tenus(&self, sauf: Button) -> impl Iterator<Item = Button> + '_ {
        BOUTONS
            .iter()
            .enumerate()
            .filter(move |(i, e)| self.tenus & (1 << i) != 0 && e.0 != sauf)
            .map(|(_, e)| e.0)
    }
}

/// Sens de stick tenus après zone morte radiale (`x`, `y` dans [-1, 1], y vers le haut).
pub fn sens_stick(x: f32, y: f32) -> (Option<bool>, Option<bool>) {
    if x * x + y * y < ZONE_MORTE * ZONE_MORTE {
        return (None, None);
    }
    let s = |v: f32| (v.abs() > SEUIL_AXE).then_some(v > 0.0);
    (s(x), s(y))
}

/// Famille de manette, pour nommer les boutons comme ils sont imprimés dessus.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Famille {
    Xbox,
    PlayStation,
    Nintendo,
    Generique,
}

pub fn famille(nom: &str) -> Famille {
    let n = nom.to_ascii_lowercase();
    let a = |mots: &[&str]| mots.iter().any(|m| n.contains(m));
    if a(&["xbox", "xinput", "x-box", "microsoft"]) {
        Famille::Xbox
    } else if a(&[
        "playstation",
        "dualshock",
        "dualsense",
        "ps3",
        "ps4",
        "ps5",
        "sony",
    ]) {
        Famille::PlayStation
    } else if a(&["nintendo", "switch", "pro controller", "joy-con"]) {
        Famille::Nintendo
    } else {
        Famille::Generique
    }
}

/// Nom affiché d'un bouton selon la famille de la manette.
pub fn nom_bouton(btn: Button, f: Famille) -> &'static str {
    use Famille::*;
    match (btn, f) {
        (Button::South, Xbox) | (Button::East, Nintendo) => "A",
        (Button::East, Xbox) | (Button::South, Nintendo) => "B",
        (Button::West, Xbox) | (Button::North, Nintendo) => "X",
        (Button::North, Xbox) | (Button::West, Nintendo) => "Y",
        (Button::South, PlayStation) => "Croix",
        (Button::East, PlayStation) => "Rond",
        (Button::West, PlayStation) => "Carré",
        (Button::North, PlayStation) => "Triangle",
        (Button::South, Generique) => "Bas (Sud)",
        (Button::East, Generique) => "Droite (Est)",
        (Button::West, Generique) => "Gauche (Ouest)",
        (Button::North, Generique) => "Haut (Nord)",
        (Button::Start, PlayStation) => "Options",
        (Button::Select, PlayStation) => "Share",
        (Button::Start, Xbox) => "Menu",
        (Button::Select, Xbox) => "Vue",
        (Button::Start, Nintendo) => "+",
        (Button::Select, Nintendo) => "−",
        (Button::Start, _) => "Start",
        (Button::Select, _) => "Select",
        (Button::DPadUp, _) => "Croix haut",
        (Button::DPadDown, _) => "Croix bas",
        (Button::DPadLeft, _) => "Croix gauche",
        (Button::DPadRight, _) => "Croix droite",
        (Button::LeftTrigger, Xbox) => "LB",
        (Button::RightTrigger, Xbox) => "RB",
        (Button::LeftTrigger2, Xbox) => "LT",
        (Button::RightTrigger2, Xbox) => "RT",
        (Button::LeftTrigger, Nintendo) => "L",
        (Button::RightTrigger, Nintendo) => "R",
        (Button::LeftTrigger2, Nintendo) => "ZL",
        (Button::RightTrigger2, Nintendo) => "ZR",
        (Button::LeftTrigger, _) => "L1",
        (Button::RightTrigger, _) => "R1",
        (Button::LeftTrigger2, _) => "L2",
        (Button::RightTrigger2, _) => "R2",
        (Button::LeftThumb, _) => "L3",
        (Button::RightThumb, _) => "R3",
        (Button::Mode, _) => "Guide",
        _ => "?",
    }
}

pub fn nom_axe(axe: Axis, positif: bool) -> &'static str {
    match (axe, positif) {
        (Axis::LeftStickX, false) => "Stick G gauche",
        (Axis::LeftStickX, true) => "Stick G droite",
        (Axis::LeftStickY, true) => "Stick G haut",
        (Axis::LeftStickY, false) => "Stick G bas",
        (Axis::RightStickX, false) => "Stick D gauche",
        (Axis::RightStickX, true) => "Stick D droite",
        (Axis::RightStickY, true) => "Stick D haut",
        (Axis::RightStickY, false) => "Stick D bas",
        _ => "Axe ?",
    }
}

pub struct Manettes {
    gilrs: Option<Gilrs>,
    pads: Vec<Pad>,
    tenus: u16,
    /// Directions : instant de la prochaine répétition.
    repetition: f64,
    /// Manettes branchées depuis la dernière lecture (GUID, nom).
    branchees: Vec<(String, String)>,
}

#[derive(Default, Clone, Copy)]
pub struct Etat {
    /// Nouveaux appuis (et répétitions des directions tenues).
    pub appuis: u16,
    pub connectee: bool,
}

fn hex(o: [u8; 16]) -> String {
    o.iter().map(|b| format!("{b:02x}")).collect()
}

impl Manettes {
    pub fn ouvrir() -> Manettes {
        Manettes {
            gilrs: Gilrs::new().ok(),
            pads: Vec::new(),
            tenus: 0,
            repetition: 0.0,
            branchees: Vec::new(),
        }
    }

    /// Manettes branchées, dans l'ordre de gilrs (la dernière branchée en dernier).
    pub fn pads(&self) -> &[Pad] {
        &self.pads
    }

    /// Manettes branchées depuis le dernier appel (GUID, nom).
    pub fn branchees(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.branchees)
    }

    pub fn lire(&mut self, maintenant: f64) -> Etat {
        let Some(g) = self.gilrs.as_mut() else {
            return Etat::default();
        };
        while let Some(ev) = g.next_event() {
            if ev.event == EventType::Connected {
                let p = g.gamepad(ev.id);
                self.branchees.push((hex(p.uuid()), p.name().to_string()));
            }
        }
        let precedents = std::mem::take(&mut self.pads);
        let mut tenus = 0;
        for (id, pad) in g.gamepads() {
            let id: usize = id.into();
            let mut p = Pad {
                id,
                uuid: hex(pad.uuid()),
                nom: pad.name().to_string(),
                ..Pad::default()
            };
            for (i, (btn, _, bit)) in BOUTONS.iter().enumerate() {
                if pad.is_pressed(*btn) {
                    p.tenus |= 1 << i;
                    tenus |= bit;
                }
            }
            for paire in [0, 2] {
                let x = pad.value(AXES[paire].0);
                let y = pad.value(AXES[paire + 1].0);
                let (sx, sy) = sens_stick(x, y);
                for (k, s) in [(paire, sx), (paire + 1, sy)] {
                    if let Some(positif) = s {
                        p.axes |= 1 << (2 * k + positif as usize);
                    }
                }
            }
            // Le stick gauche navigue dans les menus comme la croix.
            if p.axe(Axis::LeftStickX, false) {
                tenus |= b::GAUCHE;
            }
            if p.axe(Axis::LeftStickX, true) {
                tenus |= b::DROITE;
            }
            if p.axe(Axis::LeftStickY, true) {
                tenus |= b::HAUT;
            }
            if p.axe(Axis::LeftStickY, false) {
                tenus |= b::BAS;
            }
            let avant = precedents.iter().find(|q| q.id == id);
            p.appuis = p.tenus & !avant.map_or(0, |q| q.tenus);
            p.axes_appuis = p.axes & !avant.map_or(0, |q| q.axes);
            self.pads.push(p);
        }
        let connectee = !self.pads.is_empty();
        let mut appuis = tenus & !self.tenus;
        let dirs = tenus & b::DIRECTIONS;
        if appuis & b::DIRECTIONS != 0 {
            self.repetition = maintenant + 0.38;
        } else if dirs != 0 && maintenant >= self.repetition {
            appuis |= dirs;
            self.repetition = maintenant + 0.075;
        }
        self.tenus = tenus;
        Etat { appuis, connectee }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_morte_radiale() {
        assert_eq!(sens_stick(0.1, 0.15), (None, None));
        assert_eq!(sens_stick(0.8, 0.1), (Some(true), None));
        assert_eq!(sens_stick(-0.6, -0.7), (Some(false), Some(false)));
        // Hors zone morte mais sous le seuil : rien d'appuyé.
        assert_eq!(sens_stick(0.3, 0.3), (None, None));
    }

    #[test]
    fn familles_et_noms() {
        assert_eq!(famille("Xbox Wireless Controller"), Famille::Xbox);
        assert_eq!(famille("Sony DualSense"), Famille::PlayStation);
        assert_eq!(famille("Nintendo Switch Pro Controller"), Famille::Nintendo);
        assert_eq!(nom_bouton(Button::South, Famille::Xbox), "A");
        assert_eq!(nom_bouton(Button::South, Famille::Nintendo), "B");
    }
}
