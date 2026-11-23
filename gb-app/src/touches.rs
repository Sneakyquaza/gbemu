//! Touches : boutons de la console et raccourcis de l'appli (table [`Action`], point
//! d'accroche unique de tous les raccourcis), profils clavier et manettes, conflits,
//! fichier `touches.json` (versionné, écriture atomique, valeurs inconnues ignorées).

use crate::manette::{self, bit_bouton, Famille, Pad, AXES, BOUTONS};
use crate::sauve;
use eframe::egui::{self, Key, Modifiers};
use gilrs::{Axis, Button};
use serde_json::{json, Map, Value};
use std::path::Path;

pub const VERSION: u64 = 1;
/// Entrées au plus par bouton ou raccourci (les défauts clavier en ont jusqu'à 3).
pub const MAX_ENTREES: usize = 3;

/// Boutons de la console, dans l'ordre de « Tout configurer ».
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bouton {
    Haut,
    Bas,
    Gauche,
    Droite,
    A,
    B,
    Select,
    Start,
    /// Gâchettes de la Game Boy Advance (sans effet sur Game Boy).
    L,
    R,
}

/// Bits des gâchettes au-dessus des 8 bits de `gb_core::bouton`.
pub const BIT_L: u16 = 0x100;
pub const BIT_R: u16 = 0x200;

impl Bouton {
    pub const TOUS: [Bouton; 10] = [
        Bouton::Haut,
        Bouton::Bas,
        Bouton::Gauche,
        Bouton::Droite,
        Bouton::A,
        Bouton::B,
        Bouton::Select,
        Bouton::Start,
        Bouton::L,
        Bouton::R,
    ];

    pub fn nom(self) -> &'static str {
        match self {
            Bouton::Haut => "Haut",
            Bouton::Bas => "Bas",
            Bouton::Gauche => "Gauche",
            Bouton::Droite => "Droite",
            Bouton::A => "A",
            Bouton::B => "B",
            Bouton::Select => "Select",
            Bouton::Start => "Start",
            Bouton::L => "L",
            Bouton::R => "R",
        }
    }

    /// Bit du registre joypad de `gb_core` (L et R : [`BIT_L`], [`BIT_R`]).
    pub fn bit(self) -> u16 {
        use gb_core::bouton as g;
        (match self {
            Bouton::Haut => g::HAUT,
            Bouton::Bas => g::BAS,
            Bouton::Gauche => g::GAUCHE,
            Bouton::Droite => g::DROITE,
            Bouton::A => g::A,
            Bouton::B => g::B,
            Bouton::Select => g::SELECT,
            Bouton::Start => g::START,
            Bouton::L => return BIT_L,
            Bouton::R => return BIT_R,
        }) as u16
    }
}

/// Raccourcis de l'appli. Le comportement de chacun vit chez son appelant ; ici seulement
/// le nom, les touches par défaut et la lecture (tenu / appuyé).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Pause,
    /// Avance rapide tant que la touche est tenue.
    AvanceRapide,
    /// Avance rapide basculée (appui = marche / arrêt).
    AvanceBascule,
    /// Rembobinage tant que la touche est tenue.
    Rembobiner,
    /// Sauvegarde rapide dans l'emplacement courant.
    SauverEtat,
    /// Chargement rapide de l'emplacement courant.
    ChargerEtat,
    EmplacementPrecedent,
    EmplacementSuivant,
    /// Sauver dans l'emplacement 1 à 10.
    SauverEmplacement(u8),
    /// Charger l'emplacement 1 à 10.
    ChargerEmplacement(u8),
    AnnulerChargement,
    AnnulerSauvegarde,
    /// Panneau des emplacements d'état.
    PanneauEtats,
    /// Vitesse d'avance rapide suivante.
    VitesseSuivante,
    Muet,
    Capture,
    PleinEcran,
    TurboA,
    TurboB,
}

/// Nombre d'emplacements adressables directement (F1…F10).
pub const EMPLACEMENTS: u8 = 10;
/// Fréquence du turbo (appuis par seconde).
const TURBO_HZ: f64 = 10.0;

impl Action {
    /// Toutes les actions, dans l'ordre d'affichage.
    pub fn toutes() -> Vec<Action> {
        use Action::*;
        let mut v = vec![
            Pause,
            AvanceRapide,
            AvanceBascule,
            Rembobiner,
            SauverEtat,
            ChargerEtat,
            EmplacementPrecedent,
            EmplacementSuivant,
            AnnulerChargement,
            AnnulerSauvegarde,
            PanneauEtats,
            VitesseSuivante,
            Muet,
            Capture,
            PleinEcran,
            TurboA,
            TurboB,
        ];
        v.extend((1..=EMPLACEMENTS).map(SauverEmplacement));
        v.extend((1..=EMPLACEMENTS).map(ChargerEmplacement));
        v
    }

    pub fn nom(self) -> String {
        use Action::*;
        match self {
            Pause => "Pause".into(),
            AvanceRapide => "Avance rapide (maintenue)".into(),
            AvanceBascule => "Avance rapide (basculée)".into(),
            Rembobiner => "Rembobiner (maintenu)".into(),
            SauverEtat => "Sauver l'état".into(),
            ChargerEtat => "Charger l'état".into(),
            EmplacementPrecedent => "Emplacement précédent".into(),
            EmplacementSuivant => "Emplacement suivant".into(),
            SauverEmplacement(n) => format!("Sauver dans l'emplacement {n}"),
            ChargerEmplacement(n) => format!("Charger l'emplacement {n}"),
            AnnulerChargement => "Annuler le dernier chargement".into(),
            AnnulerSauvegarde => "Annuler la dernière sauvegarde".into(),
            PanneauEtats => "Panneau des états".into(),
            VitesseSuivante => "Vitesse d'avance suivante".into(),
            Muet => "Couper le son".into(),
            Capture => "Capture d'écran".into(),
            PleinEcran => "Plein écran".into(),
            TurboA => "Turbo A".into(),
            TurboB => "Turbo B".into(),
        }
    }

    /// Nom stable dans `touches.json`.
    fn cle(self) -> String {
        match self {
            Action::SauverEmplacement(n) => format!("SauverEmplacement{n}"),
            Action::ChargerEmplacement(n) => format!("ChargerEmplacement{n}"),
            a => format!("{a:?}"),
        }
    }

    fn depuis_cle(s: &str) -> Option<Action> {
        Action::toutes().into_iter().find(|a| a.cle() == s)
    }
}

/// Une touche clavier (avec modificateurs), un bouton de manette (éventuellement tenu
/// avec un autre : accord) ou un sens de stick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Entree {
    Touche {
        touche: Key,
        maj: bool,
        ctrl: bool,
    },
    Bouton {
        bouton: Button,
        avec: Option<Button>,
    },
    Axe {
        axe: Axis,
        positif: bool,
    },
}

const fn t(touche: Key) -> Entree {
    Entree::Touche {
        touche,
        maj: false,
        ctrl: false,
    }
}

const fn m(bouton: Button) -> Entree {
    Entree::Bouton { bouton, avec: None }
}

fn nom_btn(b: Button) -> Option<&'static str> {
    BOUTONS.iter().find(|e| e.0 == b).map(|e| e.1)
}

fn btn_depuis(s: &str) -> Option<Button> {
    BOUTONS.iter().find(|e| e.1 == s).map(|e| e.0)
}

impl Entree {
    /// Texte stable : `Ctrl+Maj+F3`, `Manette:Select+Start`, `Axe:StickGaucheX+`.
    pub fn texte(&self) -> String {
        match *self {
            Entree::Touche { touche, maj, ctrl } => format!(
                "{}{}{}",
                if ctrl { "Ctrl+" } else { "" },
                if maj { "Maj+" } else { "" },
                touche.name()
            ),
            Entree::Bouton { bouton, avec } => {
                let n = nom_btn(bouton).unwrap_or("?");
                match avec.and_then(nom_btn) {
                    Some(a) => format!("Manette:{a}+{n}"),
                    None => format!("Manette:{n}"),
                }
            }
            Entree::Axe { axe, positif } => {
                let n = AXES.iter().find(|e| e.0 == axe).map_or("?", |e| e.1);
                format!("Axe:{n}{}", if positif { '+' } else { '-' })
            }
        }
    }

    pub fn lire(s: &str) -> Option<Entree> {
        if let Some(r) = s.strip_prefix("Manette:") {
            return match r.split_once('+') {
                Some((a, n)) => {
                    let (a, n) = (btn_depuis(a)?, btn_depuis(n)?);
                    (a != n).then(|| accord(n, a))
                }
                None => Some(m(btn_depuis(r)?)),
            };
        }
        if let Some(r) = s.strip_prefix("Axe:") {
            let (n, signe) = r.split_at(r.len().checked_sub(1)?);
            let axe = AXES.iter().find(|e| e.1 == n)?.0;
            return match signe {
                "+" => Some(Entree::Axe { axe, positif: true }),
                "-" => Some(Entree::Axe {
                    axe,
                    positif: false,
                }),
                _ => None,
            };
        }
        let (mut maj, mut ctrl, mut reste) = (false, false, s);
        loop {
            if let Some(r) = reste.strip_prefix("Ctrl+") {
                ctrl = true;
                reste = r;
            } else if let Some(r) = reste.strip_prefix("Maj+") {
                maj = true;
                reste = r;
            } else {
                break;
            }
        }
        Some(Entree::Touche {
            touche: Key::from_name(reste)?,
            maj,
            ctrl,
        })
    }

    /// Nom affiché à l'utilisateur.
    pub fn libelle(&self, f: Famille) -> String {
        match *self {
            Entree::Touche { touche, maj, ctrl } => format!(
                "{}{}{}",
                if ctrl { "Ctrl+" } else { "" },
                if maj { "Maj+" } else { "" },
                nom_touche(touche)
            ),
            Entree::Bouton { bouton, avec } => match avec {
                Some(a) => format!(
                    "{}+{}",
                    manette::nom_bouton(a, f),
                    manette::nom_bouton(bouton, f)
                ),
                None => manette::nom_bouton(bouton, f).into(),
            },
            Entree::Axe { axe, positif } => manette::nom_axe(axe, positif).into(),
        }
    }
}

/// Accord de deux boutons, rangé dans l'ordre de [`BOUTONS`] (Select+Start = Start+Select).
fn accord(x: Button, y: Button) -> Entree {
    let rang = |b: Button| BOUTONS.iter().position(|e| e.0 == b).unwrap_or(usize::MAX);
    let (principal, avec) = if rang(x) >= rang(y) { (x, y) } else { (y, x) };
    Entree::Bouton {
        bouton: principal,
        avec: Some(avec),
    }
}

/// Accord capturé : `appuye` en dernier pendant que `tenu` est tenu.
pub fn entree_accord(tenu: Button, appuye: Button) -> Entree {
    accord(appuye, tenu)
}

pub fn nom_touche(k: Key) -> String {
    match k {
        Key::ArrowUp => "Flèche haut".into(),
        Key::ArrowDown => "Flèche bas".into(),
        Key::ArrowLeft => "Flèche gauche".into(),
        Key::ArrowRight => "Flèche droite".into(),
        Key::Enter => "Entrée".into(),
        Key::Backspace => "Retour arrière".into(),
        Key::Space => "Espace".into(),
        Key::Escape => "Échap".into(),
        Key::Delete => "Suppr".into(),
        Key::Insert => "Inser".into(),
        Key::PageUp => "Page préc.".into(),
        Key::PageDown => "Page suiv.".into(),
        Key::Home => "Début".into(),
        Key::End => "Fin".into(),
        autre => autre.symbol_or_name().into(),
    }
}

/// Ce qu'on attribue : un bouton de la console ou un raccourci.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cible {
    Bouton(Bouton),
    Action(Action),
}

impl Cible {
    pub fn nom(self) -> String {
        match self {
            Cible::Bouton(b) => b.nom().into(),
            Cible::Action(a) => a.nom(),
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Profil {
    pub boutons: [Vec<Entree>; 10],
    /// Une ligne par action de [`Action::toutes`], dans le même ordre.
    pub raccourcis: Vec<(Action, Vec<Entree>)>,
}

impl Profil {
    fn vide() -> Profil {
        Profil {
            boutons: Default::default(),
            raccourcis: Action::toutes().into_iter().map(|a| (a, vec![])).collect(),
        }
    }

    /// Défauts clavier (ceux de l'appli avant la table d'actions, plus les états du cahier).
    pub fn clavier() -> Profil {
        use Action::*;
        let mut p = Profil::vide();
        p.boutons = [
            vec![t(Key::ArrowUp)],
            vec![t(Key::ArrowDown)],
            vec![t(Key::ArrowLeft)],
            vec![t(Key::ArrowRight)],
            vec![t(Key::X), t(Key::K)],
            vec![t(Key::Z), t(Key::W), t(Key::J)],
            vec![t(Key::Backspace), t(Key::Space)],
            vec![t(Key::Enter)],
            vec![t(Key::A)],
            vec![t(Key::S)],
        ];
        let f = [
            Key::F1,
            Key::F2,
            Key::F3,
            Key::F4,
            Key::F5,
            Key::F6,
            Key::F7,
            Key::F8,
            Key::F9,
            Key::F10,
        ];
        let maj = |k| Entree::Touche {
            touche: k,
            maj: true,
            ctrl: false,
        };
        let ctrl = |k| Entree::Touche {
            touche: k,
            maj: false,
            ctrl: true,
        };
        for (a, e) in p.raccourcis.iter_mut() {
            *e = match *a {
                Pause => vec![t(Key::Escape)],
                AvanceRapide => vec![t(Key::Tab)],
                AvanceBascule => vec![maj(Key::Tab)],
                Rembobiner => vec![t(Key::R)],
                SauverEtat => vec![t(Key::F5)],
                ChargerEtat => vec![t(Key::F8)],
                EmplacementPrecedent => vec![t(Key::F6)],
                EmplacementSuivant => vec![t(Key::F7)],
                SauverEmplacement(n) => vec![maj(f[n as usize - 1])],
                ChargerEmplacement(n) => vec![ctrl(f[n as usize - 1])],
                AnnulerChargement => vec![ctrl(Key::Z)],
                AnnulerSauvegarde => vec![Entree::Touche {
                    touche: Key::Z,
                    maj: true,
                    ctrl: true,
                }],
                PanneauEtats => vec![t(Key::F4)],
                VitesseSuivante => vec![t(Key::Plus), t(Key::Equals)],
                Muet => vec![t(Key::M)],
                Capture => vec![t(Key::F12)],
                PleinEcran => vec![t(Key::F11)],
                TurboA | TurboB => vec![],
            };
        }
        p
    }

    /// Défauts manette, positionnels (Est/Nord = A, Sud/Ouest = B).
    pub fn manette() -> Profil {
        let mut p = Profil::vide();
        let axe = |axe, positif| Entree::Axe { axe, positif };
        p.boutons = [
            vec![m(Button::DPadUp), axe(Axis::LeftStickY, true)],
            vec![m(Button::DPadDown), axe(Axis::LeftStickY, false)],
            vec![m(Button::DPadLeft), axe(Axis::LeftStickX, false)],
            vec![m(Button::DPadRight), axe(Axis::LeftStickX, true)],
            vec![m(Button::East), m(Button::North)],
            vec![m(Button::South), m(Button::West)],
            vec![m(Button::Select)],
            vec![m(Button::Start)],
            vec![m(Button::LeftTrigger)],
            vec![m(Button::RightTrigger)],
        ];
        for (a, e) in p.raccourcis.iter_mut() {
            *e = match a {
                Action::Pause => vec![accord(Button::Start, Button::Select)],
                // L1/R1 sont les gâchettes L/R de la GBA : avance et rembobinage sur L2/R2.
                Action::AvanceRapide => vec![m(Button::RightTrigger2)],
                Action::Rembobiner => vec![m(Button::LeftTrigger2)],
                // Accords optionnels du cahier (Select+R1 sauver, Select+L1 charger).
                Action::SauverEtat => vec![accord(Button::RightTrigger, Button::Select)],
                Action::ChargerEtat => vec![accord(Button::LeftTrigger, Button::Select)],
                _ => vec![],
            };
        }
        p
    }

    pub fn entrees(&self, c: Cible) -> &[Entree] {
        match c {
            Cible::Bouton(b) => &self.boutons[b as usize],
            Cible::Action(a) => self
                .raccourcis
                .iter()
                .find(|r| r.0 == a)
                .map_or(&[], |r| &r.1),
        }
    }

    fn entrees_mut(&mut self, c: Cible) -> Option<&mut Vec<Entree>> {
        match c {
            Cible::Bouton(b) => Some(&mut self.boutons[b as usize]),
            Cible::Action(a) => self
                .raccourcis
                .iter_mut()
                .find(|r| r.0 == a)
                .map(|r| &mut r.1),
        }
    }

    fn cibles(&self) -> impl Iterator<Item = (Cible, &Vec<Entree>)> {
        Bouton::TOUS
            .iter()
            .map(|&b| (Cible::Bouton(b), &self.boutons[b as usize]))
            .chain(self.raccourcis.iter().map(|(a, e)| (Cible::Action(*a), e)))
    }

    /// Qui utilise déjà cette entrée dans le profil.
    pub fn cible_de(&self, e: &Entree) -> Option<Cible> {
        self.cibles().find(|(_, v)| v.contains(e)).map(|(c, _)| c)
    }

    /// Attribue `e` à `c` (en tête de liste). Si une autre cible l'avait, elle la perd :
    /// renvoyée pour le signaler. Au-delà de [`MAX_ENTREES`], la plus ancienne tombe.
    pub fn attribuer(&mut self, c: Cible, e: Entree) -> Option<Cible> {
        let ancien = self.cible_de(&e).filter(|&a| a != c);
        if let Some(a) = ancien {
            if let Some(v) = self.entrees_mut(a) {
                v.retain(|x| *x != e);
            }
        }
        if let Some(v) = self.entrees_mut(c) {
            v.retain(|x| *x != e);
            v.insert(0, e);
            v.truncate(MAX_ENTREES);
        }
        ancien
    }

    pub fn effacer(&mut self, c: Cible) {
        if let Some(v) = self.entrees_mut(c) {
            v.clear();
        }
    }

    /// Bits `gb_core` des boutons tenus (directions opposées annulées, turbo compris).
    pub fn boutons_tenus(&self, c: &Clavier, pad: Option<&Pad>, maintenant: f64) -> u16 {
        // Les boutons d'un accord tenu (Select+Start…) vont au raccourci, pas au jeu.
        let masque = pad.map_or(0, |p| {
            self.raccourcis
                .iter()
                .flat_map(|(_, l)| l)
                .fold(0, |m, e| match *e {
                    Entree::Bouton {
                        bouton,
                        avec: Some(a),
                    } if p.tenu(bouton) && p.tenu(a) => m | bit_bouton(bouton) | bit_bouton(a),
                    _ => m,
                })
        });
        let filtre = pad.map(|p| Pad {
            tenus: p.tenus & !masque,
            ..p.clone()
        });
        let pad = filtre.as_ref();
        let mut v = 0u16;
        for b in Bouton::TOUS {
            if self.boutons[b as usize]
                .iter()
                .any(|e| tenue(e, c, pad, false))
            {
                v |= b.bit();
            }
        }
        if ((maintenant * TURBO_HZ * 2.0) as i64) % 2 == 0 {
            if self.tenue(Action::TurboA, c, pad) {
                v |= Bouton::A.bit();
            }
            if self.tenue(Action::TurboB, c, pad) {
                v |= Bouton::B.bit();
            }
        }
        v
    }

    pub fn tenue(&self, a: Action, c: &Clavier, pad: Option<&Pad>) -> bool {
        self.entrees(Cible::Action(a))
            .iter()
            .any(|e| tenue(e, c, pad, true))
    }

    pub fn appuyee(&self, a: Action, c: &Clavier, pad: Option<&Pad>) -> bool {
        self.entrees(Cible::Action(a))
            .iter()
            .any(|e| appuyee(e, c, pad))
    }

    fn vers_json(&self) -> Value {
        let liste = |v: &[Entree]| Value::from(v.iter().map(Entree::texte).collect::<Vec<_>>());
        let mut boutons = Map::new();
        for b in Bouton::TOUS {
            boutons.insert(b.nom().into(), liste(&self.boutons[b as usize]));
        }
        let mut raccourcis = Map::new();
        for (a, e) in &self.raccourcis {
            raccourcis.insert(a.cle(), liste(e));
        }
        json!({ "boutons": boutons, "raccourcis": raccourcis })
    }

    /// Part des défauts `base` et remplace ce que le fichier donne ; le reste est ignoré.
    fn depuis_json(v: &Value, base: Profil) -> Profil {
        let mut p = base;
        let lire_liste = |v: &Value| -> Option<Vec<Entree>> {
            let mut l: Vec<Entree> = Vec::new();
            for e in v.as_array()?.iter().filter_map(Value::as_str) {
                if let Some(e) = Entree::lire(e) {
                    if !l.contains(&e) && l.len() < MAX_ENTREES {
                        l.push(e);
                    }
                }
            }
            Some(l)
        };
        if let Some(o) = v.get("boutons").and_then(Value::as_object) {
            for b in Bouton::TOUS {
                if let Some(l) = o.get(b.nom()).and_then(lire_liste) {
                    p.boutons[b as usize] = l;
                }
            }
        }
        if let Some(o) = v.get("raccourcis").and_then(Value::as_object) {
            for (cle, val) in o {
                if let (Some(a), Some(l)) = (Action::depuis_cle(cle), lire_liste(val)) {
                    if let Some(e) = p.entrees_mut(Cible::Action(a)) {
                        *e = l;
                    }
                }
            }
        }
        // Une même entrée lue deux fois (fichier modifié à la main) : la première garde.
        let mut vus: Vec<Entree> = Vec::new();
        for b in Bouton::TOUS {
            p.boutons[b as usize].retain(|e| {
                let neuf = !vus.contains(e);
                vus.push(*e);
                neuf
            });
        }
        for (_, l) in p.raccourcis.iter_mut() {
            l.retain(|e| {
                let neuf = !vus.contains(e);
                vus.push(*e);
                neuf
            });
        }
        p
    }
}

/// `exact` (raccourcis) : Maj exact aussi, Maj+Tab ne tient pas l'avance rapide de Tab.
fn tenue(e: &Entree, c: &Clavier, pad: Option<&Pad>, exact: bool) -> bool {
    match *e {
        Entree::Touche { touche, maj, ctrl } => {
            // Ctrl exact (Ctrl+Z n'appuie pas sur B), Maj seulement si demandé.
            let maj_ok = if exact {
                maj == c.mods.shift
            } else {
                !maj || c.mods.shift
            };
            c.tenues.contains(&touche) && maj_ok && ctrl == c.mods.command
        }
        Entree::Bouton { bouton, avec } => {
            pad.is_some_and(|p| p.tenu(bouton) && avec.is_none_or(|a| p.tenu(a)))
        }
        Entree::Axe { axe, positif } => pad.is_some_and(|p| p.axe(axe, positif)),
    }
}

/// Front d'appui : modificateurs exacts au clavier (F1 ≠ Maj+F1) ; un accord se déclenche
/// quand ses deux boutons sont tenus et que l'un des deux vient d'être appuyé.
fn appuyee(e: &Entree, c: &Clavier, pad: Option<&Pad>) -> bool {
    match *e {
        Entree::Touche { touche, maj, ctrl } => c
            .appuis
            .iter()
            .any(|(k, md)| *k == touche && md.shift == maj && md.command == ctrl),
        Entree::Bouton { bouton, avec } => pad.is_some_and(|p| match avec {
            None => p.appui(bouton),
            Some(a) => p.tenu(bouton) && p.tenu(a) && (p.appui(bouton) || p.appui(a)),
        }),
        Entree::Axe { axe, positif } => pad.is_some_and(|p| p.axe_appui(axe, positif)),
    }
}

/// Clavier de l'image en cours : touches tenues, fronts d'appui (sans répétition).
#[derive(Default, Clone, Debug)]
pub struct Clavier {
    pub tenues: Vec<Key>,
    pub appuis: Vec<(Key, Modifiers)>,
    pub mods: Modifiers,
}

impl Clavier {
    pub fn lire(i: &egui::InputState) -> Clavier {
        Clavier {
            tenues: i.keys_down.iter().copied().collect(),
            appuis: i
                .events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        repeat: false,
                        modifiers,
                        ..
                    } => Some((*key, *modifiers)),
                    _ => None,
                })
                .collect(),
            mods: i.modifiers,
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct ProfilManette {
    /// GUID SDL du modèle (gilrs `Gamepad::uuid`), en hexadécimal.
    pub uuid: String,
    pub nom: String,
    pub profil: Profil,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Touches {
    pub clavier: Profil,
    /// Appliqué à toute manette sans profil propre.
    pub manette_defaut: Profil,
    pub manettes: Vec<ProfilManette>,
}

impl Default for Touches {
    fn default() -> Touches {
        Touches {
            clavier: Profil::clavier(),
            manette_defaut: Profil::manette(),
            manettes: Vec::new(),
        }
    }
}

impl Touches {
    pub fn chemin(donnees: &Path) -> std::path::PathBuf {
        donnees.join("touches.json")
    }

    pub fn charger(chemin: &Path) -> Touches {
        // JSON illisible : la copie `.bak` (ou `.tmp`) prend le relais, sinon les défauts.
        sauve::lire_json::<Value>(chemin)
            .map(|v| Touches::depuis_valeur(&v))
            .unwrap_or_default()
    }

    /// Écriture en fond (la synchronisation disque ne fige pas l'écran des touches).
    pub fn enregistrer(&self, chemin: &Path) {
        sauve::ecrire_en_fond(chemin, self.vers_json().into_bytes());
    }

    pub fn vers_json(&self) -> String {
        let manettes: Vec<Value> = self
            .manettes
            .iter()
            .map(|p| json!({ "uuid": p.uuid, "nom": p.nom, "profil": p.profil.vers_json() }))
            .collect();
        let v = json!({
            "version": VERSION,
            "clavier": self.clavier.vers_json(),
            "manette_defaut": self.manette_defaut.vers_json(),
            "manettes": manettes,
        });
        serde_json::to_string_pretty(&v).unwrap_or_default()
    }

    /// Lecture tolérante : fichier illisible → défauts ; champ ou valeur inconnus → ignorés.
    #[cfg(test)]
    pub fn depuis_json(texte: &str) -> Touches {
        serde_json::from_str::<Value>(texte)
            .map(|v| Touches::depuis_valeur(&v))
            .unwrap_or_default()
    }

    fn depuis_valeur(v: &Value) -> Touches {
        let mut t = Touches::default();
        if let Some(c) = v.get("clavier") {
            t.clavier = Profil::depuis_json(c, Profil::clavier());
        }
        if let Some(c) = v.get("manette_defaut") {
            t.manette_defaut = Profil::depuis_json(c, Profil::manette());
        }
        for p in v
            .get("manettes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(uuid) = p.get("uuid").and_then(Value::as_str) else {
                continue;
            };
            if uuid.is_empty() || t.manettes.iter().any(|q| q.uuid == uuid) {
                continue;
            }
            t.manettes.push(ProfilManette {
                uuid: uuid.into(),
                nom: p
                    .get("nom")
                    .and_then(Value::as_str)
                    .unwrap_or("Manette")
                    .into(),
                profil: p.get("profil").map_or_else(Profil::manette, |v| {
                    Profil::depuis_json(v, t.manette_defaut.clone())
                }),
            });
        }
        t
    }

    pub fn profil_pad(&self, uuid: &str) -> &Profil {
        self.manettes
            .iter()
            .find(|p| p.uuid == uuid)
            .map_or(&self.manette_defaut, |p| &p.profil)
    }

    /// Profil propre de cette manette, créé depuis le profil par défaut s'il manque.
    pub fn profil_pad_mut(&mut self, uuid: &str, nom: &str) -> &mut Profil {
        let i = match self.manettes.iter().position(|p| p.uuid == uuid) {
            Some(i) => i,
            None => {
                self.manettes.push(ProfilManette {
                    uuid: uuid.into(),
                    nom: nom.into(),
                    profil: self.manette_defaut.clone(),
                });
                self.manettes.len() - 1
            }
        };
        &mut self.manettes[i].profil
    }

    /// Clavier puis chaque manette avec son profil.
    fn sources<'a>(
        &'a self,
        pads: &'a [Pad],
    ) -> impl Iterator<Item = (&'a Profil, Option<&'a Pad>)> {
        std::iter::once((&self.clavier, None))
            .chain(pads.iter().map(|p| (self.profil_pad(&p.uuid), Some(p))))
    }

    /// Boutons Game Boy tenus (clavier + toutes les manettes).
    pub fn boutons(&self, c: &Clavier, pads: &[Pad], maintenant: f64) -> u16 {
        let mut v = self
            .sources(pads)
            .fold(0, |v, (p, pad)| v | p.boutons_tenus(c, pad, maintenant));
        let [gauche, droite, haut, bas] =
            [Bouton::Gauche, Bouton::Droite, Bouton::Haut, Bouton::Bas].map(Bouton::bit);
        // Directions opposées impossibles sur une vraie croix.
        if v & (gauche | droite) == gauche | droite {
            v &= !(gauche | droite);
        }
        if v & (haut | bas) == haut | bas {
            v &= !(haut | bas);
        }
        v
    }

    pub fn tenue(&self, a: Action, c: &Clavier, pads: &[Pad]) -> bool {
        self.sources(pads).any(|(p, pad)| p.tenue(a, c, pad))
    }

    pub fn appuyee(&self, a: Action, c: &Clavier, pads: &[Pad]) -> bool {
        self.sources(pads).any(|(p, pad)| p.appuyee(a, c, pad))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bits de `gb_core::bouton` élargis aux 16 bits de l'appli.
    mod g {
        use gb_core::bouton as b;
        pub const HAUT: u16 = b::HAUT as u16;
        pub const A: u16 = b::A as u16;
        pub const B: u16 = b::B as u16;
        pub const START: u16 = b::START as u16;
        pub const SELECT: u16 = b::SELECT as u16;
    }

    fn clavier(tenues: &[Key]) -> Clavier {
        Clavier {
            tenues: tenues.to_vec(),
            ..Clavier::default()
        }
    }

    fn appui(k: Key, shift: bool) -> Clavier {
        Clavier {
            tenues: vec![k],
            appuis: vec![(
                k,
                Modifiers {
                    shift,
                    ..Modifiers::NONE
                },
            )],
            mods: Modifiers {
                shift,
                ..Modifiers::NONE
            },
        }
    }

    fn pad(tenus: &[Button], appuis: &[Button]) -> Pad {
        let bits = |l: &[Button]| {
            l.iter().fold(0u32, |v, b| {
                v | 1 << BOUTONS.iter().position(|e| e.0 == *b).unwrap()
            })
        };
        Pad {
            uuid: "0300abcd".into(),
            tenus: bits(tenus),
            appuis: bits(appuis),
            ..Pad::default()
        }
    }

    #[test]
    fn defauts_clavier_inchanges() {
        let t = Touches::default();
        let b = |ks: &[Key]| t.boutons(&clavier(ks), &[], 0.5);
        assert_eq!(b(&[Key::ArrowUp]), g::HAUT);
        assert_eq!(b(&[Key::X]), g::A);
        assert_eq!(b(&[Key::K]), g::A);
        assert_eq!(b(&[Key::Z]), g::B);
        assert_eq!(b(&[Key::W]), g::B);
        assert_eq!(b(&[Key::J]), g::B);
        assert_eq!(b(&[Key::Enter]), g::START);
        assert_eq!(b(&[Key::Backspace]), g::SELECT);
        assert_eq!(b(&[Key::Space]), g::SELECT);
        assert_eq!(b(&[Key::A, Key::S]), BIT_L | BIT_R);
        // Directions opposées annulées.
        assert_eq!(b(&[Key::ArrowLeft, Key::ArrowRight, Key::ArrowUp]), g::HAUT);
        assert!(t.tenue(Action::AvanceRapide, &clavier(&[Key::Tab]), &[]));
        assert!(t.appuyee(Action::Pause, &appui(Key::Escape, false), &[]));
        assert!(t.appuyee(Action::PleinEcran, &appui(Key::F11, false), &[]));
    }

    #[test]
    fn modificateurs_exacts_sur_les_fronts() {
        let t = Touches::default();
        let f1 = appui(Key::F1, false);
        let maj_f1 = appui(Key::F1, true);
        let mut ctrl_f1 = appui(Key::F1, false);
        ctrl_f1.appuis[0].1.command = true;
        assert!(!t.appuyee(Action::ChargerEmplacement(1), &f1, &[]));
        assert!(!t.appuyee(Action::SauverEmplacement(1), &f1, &[]));
        assert!(t.appuyee(Action::SauverEmplacement(1), &maj_f1, &[]));
        assert!(!t.appuyee(Action::ChargerEmplacement(1), &maj_f1, &[]));
        assert!(t.appuyee(Action::ChargerEmplacement(1), &ctrl_f1, &[]));
        assert!(t.appuyee(Action::PanneauEtats, &appui(Key::F4, false), &[]));
        assert!(t.appuyee(Action::AvanceBascule, &appui(Key::Tab, true), &[]));
        // Ctrl tenu : Z n'appuie pas sur B.
        let mut ctrl_z = clavier(&[Key::Z]);
        ctrl_z.mods.command = true;
        assert_eq!(t.boutons(&ctrl_z, &[], 0.5), 0);
        assert!(t.appuyee(Action::SauverEtat, &appui(Key::F5, false), &[]));
        assert!(t.appuyee(Action::ChargerEtat, &appui(Key::F8, false), &[]));
    }

    #[test]
    fn defauts_manette_et_accord_pause() {
        let t = Touches::default();
        let c = Clavier::default();
        let p = pad(&[Button::East, Button::South], &[]);
        assert_eq!(t.boutons(&c, &[p], 0.5), g::A | g::B);
        let mut p = pad(&[Button::DPadUp], &[]);
        p.axes = 1 << 3; // stick gauche vers le haut
        assert_eq!(t.boutons(&c, &[p], 0.5), g::HAUT);
        let combo = pad(&[Button::Select, Button::Start], &[Button::Start]);
        assert!(t.appuyee(Action::Pause, &c, &[combo]));
        let combo = pad(&[Button::Select, Button::Start], &[Button::Select]);
        assert!(t.appuyee(Action::Pause, &c, &[combo]));
        let seul = pad(&[Button::Start], &[Button::Start]);
        assert!(!t.appuyee(Action::Pause, &c, &[seul]));
        let sauver = pad(
            &[Button::Select, Button::RightTrigger],
            &[Button::RightTrigger],
        );
        assert!(t.appuyee(Action::SauverEtat, &c, &[sauver]));
        let charger = pad(&[Button::Select, Button::LeftTrigger], &[Button::Select]);
        assert!(t.appuyee(Action::ChargerEtat, &c, &[charger]));
        let r2 = pad(&[Button::RightTrigger2], &[]);
        assert!(t.tenue(Action::AvanceRapide, &c, &[r2]));
        let gachettes = pad(&[Button::LeftTrigger, Button::RightTrigger], &[]);
        assert_eq!(t.boutons(&c, &[gachettes], 0.5), BIT_L | BIT_R);
    }

    #[test]
    fn accords_et_maj_ne_fuient_pas() {
        let t = Touches::default();
        let c = Clavier::default();
        // Select+Start (pause), Select+R1 (sauver) : rien ne part au jeu.
        let combo = pad(&[Button::Select, Button::Start], &[]);
        assert_eq!(t.boutons(&c, &[combo], 0.5), 0);
        let sauver = pad(&[Button::Select, Button::RightTrigger], &[]);
        assert_eq!(t.boutons(&c, &[sauver], 0.5), 0);
        // Hors accord, Select et R restent des boutons du jeu.
        let seul = pad(&[Button::Select], &[]);
        assert_eq!(t.boutons(&c, &[seul], 0.5), g::SELECT);
        // Maj+Tab bascule l'avance rapide sans la tenir.
        let maj_tab = appui(Key::Tab, true);
        assert!(!t.tenue(Action::AvanceRapide, &maj_tab, &[]));
        assert!(t.tenue(Action::AvanceRapide, &clavier(&[Key::Tab]), &[]));
    }

    #[test]
    fn defauts_sans_conflit() {
        for p in [Profil::clavier(), Profil::manette()] {
            let mut vus = Vec::new();
            for (_, l) in p.cibles() {
                assert!(l.len() <= MAX_ENTREES);
                for e in l {
                    assert!(!vus.contains(e), "{e:?} en double");
                    vus.push(*e);
                }
            }
        }
    }

    #[test]
    fn conflit_retire_l_autre_cible() {
        let mut p = Profil::clavier();
        let x = t(Key::X);
        let perdu = p.attribuer(Cible::Bouton(Bouton::B), x);
        assert_eq!(perdu, Some(Cible::Bouton(Bouton::A)));
        assert_eq!(p.entrees(Cible::Bouton(Bouton::A)), &[t(Key::K)]);
        assert_eq!(p.entrees(Cible::Bouton(Bouton::B))[0], x);
        assert_eq!(p.entrees(Cible::Bouton(Bouton::B)).len(), MAX_ENTREES);
        // Raccourci contre bouton : F5 donné à Start, l'état rapide le perd.
        let perdu = p.attribuer(Cible::Bouton(Bouton::Start), t(Key::F5));
        assert_eq!(perdu, Some(Cible::Action(Action::SauverEtat)));
        assert!(p.entrees(Cible::Action(Action::SauverEtat)).is_empty());
        // Réattribuer à la même cible : pas de conflit, pas de doublon.
        assert_eq!(p.attribuer(Cible::Bouton(Bouton::Start), t(Key::F5)), None);
        assert_eq!(
            p.entrees(Cible::Bouton(Bouton::Start)),
            &[t(Key::F5), t(Key::Enter)]
        );
        p.effacer(Cible::Bouton(Bouton::Start));
        assert!(p.entrees(Cible::Bouton(Bouton::Start)).is_empty());
    }

    #[test]
    fn json_aller_retour() {
        let mut t = Touches::default();
        t.clavier.attribuer(Cible::Bouton(Bouton::A), t_maj(Key::Q));
        t.clavier.effacer(Cible::Action(Action::Capture));
        let p = t.profil_pad_mut("0300abcd", "Manette test");
        p.attribuer(
            Cible::Action(Action::SauverEtat),
            entree_accord(Button::Select, Button::RightTrigger),
        );
        p.attribuer(
            Cible::Bouton(Bouton::A),
            Entree::Axe {
                axe: Axis::RightStickX,
                positif: false,
            },
        );
        let relu = Touches::depuis_json(&t.vers_json());
        assert_eq!(relu, t);
    }

    fn t_maj(k: Key) -> Entree {
        Entree::Touche {
            touche: k,
            maj: true,
            ctrl: true,
        }
    }

    #[test]
    fn valeurs_inconnues_ignorees() {
        assert_eq!(Touches::depuis_json("pas du json {"), Touches::default());
        assert_eq!(Touches::depuis_json("[1, 2]"), Touches::default());
        let texte = r#"{
            "version": 99, "inconnu": {"x": 1},
            "clavier": {
                "boutons": {"A": ["ToucheMartienne", "Q", 42, "Q"], "Z": ["X"], "B": "pas une liste"},
                "raccourcis": {"Teleporter": ["F9"], "Pause": ["Manette:Inconnu", "P"]}
            },
            "manettes": [{"nom": "sans uuid"}, {"uuid": "ab", "profil": 7}, {"uuid": "ab"}]
        }"#;
        let t = Touches::depuis_json(texte);
        let c = &t.clavier;
        assert_eq!(c.entrees(Cible::Bouton(Bouton::A)), &[t_simple(Key::Q)]);
        assert_eq!(
            c.entrees(Cible::Bouton(Bouton::B)),
            Profil::clavier().entrees(Cible::Bouton(Bouton::B))
        );
        assert_eq!(c.entrees(Cible::Action(Action::Pause)), &[t_simple(Key::P)]);
        assert_eq!(t.manette_defaut, Profil::manette());
        assert_eq!(t.manettes.len(), 1);
        assert_eq!(t.manettes[0].profil, Profil::manette());
    }

    fn t_simple(k: Key) -> Entree {
        t(k)
    }

    #[test]
    fn textes_d_entree() {
        for e in [
            t(Key::F3),
            t_maj(Key::F3),
            m(Button::South),
            entree_accord(Button::Select, Button::Start),
            Entree::Axe {
                axe: Axis::LeftStickY,
                positif: true,
            },
        ] {
            assert_eq!(Entree::lire(&e.texte()), Some(e), "{}", e.texte());
        }
        assert_eq!(
            entree_accord(Button::Select, Button::Start),
            entree_accord(Button::Start, Button::Select)
        );
        assert_eq!(Entree::lire("Manette:Sud+Sud"), None);
        assert_eq!(Entree::lire("Axe:"), None);
        assert_eq!(Entree::lire(""), None);
    }

    #[test]
    fn persistance_atomique() {
        let d = crate::sauve::DossierTest::new("touches");
        let dossier = &d.0;
        let chemin = Touches::chemin(dossier);
        let mut t = Touches::default();
        t.clavier
            .attribuer(Cible::Bouton(Bouton::Start), t_simple(Key::P));
        t.enregistrer(&chemin);
        assert_eq!(Touches::charger(&chemin), t);
        sauve::attendre_ecritures();
        assert!(!chemin.with_extension("tmp").exists());
        // Fichier abîmé : la copie `.bak` reprend la main.
        std::fs::copy(&chemin, chemin.with_extension("bak")).unwrap();
        std::fs::write(&chemin, b"{\"clavier\": [tronqu").unwrap();
        assert_eq!(Touches::charger(&chemin), t);
        // Fichier manquant : défauts.
        assert_eq!(
            Touches::charger(&dossier.join("absent.json")),
            Touches::default()
        );
    }

    #[test]
    fn turbo_alterne() {
        let mut t = Touches::default();
        t.clavier
            .attribuer(Cible::Action(Action::TurboA), t_simple(Key::Q));
        let c = clavier(&[Key::Q]);
        let a: Vec<bool> = (0..4)
            .map(|i| t.boutons(&c, &[], 0.01 + i as f64 * 0.05) & g::A != 0)
            .collect();
        assert_eq!(a, [true, false, true, false]);
    }
}
