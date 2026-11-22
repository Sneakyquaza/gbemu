//! Partie en cours : émulation cadencée sur l'horloge, son, sauvegarde batterie automatique,
//! états (emplacements, reprise, annulation) et rembobinage.

use crate::audio::Sortie;
use crate::biblio::Rom;
use crate::etats::{self, Cible, ErreurEmplacement, Etats};
use crate::machine::Machine;
use crate::rembobinage::{self, Rembobinage};
use crate::sauve;
use gb_core::ErreurEtat;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Images par seconde de la Game Boy (4 194 304 / 70 224).
const IPS: f64 = 59.7275;
/// Intervalle de vérification de la sauvegarde batterie (secondes).
const INTERVALLE_SAUVEGARDE: f64 = 2.0;
/// Vitesse du rembobinage (×2 par rapport au jeu).
const VITESSE_RECUL: f64 = 2.0;

/// Vitesse appliquée par l'avance rapide (ou le ralenti).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Vitesse {
    Ralenti,
    X2,
    #[default]
    X4,
    X8,
}

impl Vitesse {
    pub fn facteur(self) -> f64 {
        match self {
            Vitesse::Ralenti => 0.5,
            Vitesse::X2 => 2.0,
            Vitesse::X4 => 4.0,
            Vitesse::X8 => 8.0,
        }
    }

    pub fn nom(self) -> &'static str {
        match self {
            Vitesse::Ralenti => "×0,5",
            Vitesse::X2 => "×2",
            Vitesse::X4 => "×4",
            Vitesse::X8 => "×8",
        }
    }

    pub fn suivante(self) -> Vitesse {
        match self {
            Vitesse::X2 => Vitesse::X4,
            Vitesse::X4 => Vitesse::X8,
            Vitesse::X8 => Vitesse::Ralenti,
            Vitesse::Ralenti => Vitesse::X2,
        }
    }
}

pub struct Partie {
    pub gb: Machine,
    pub titre: String,
    pub crc: u32,
    chemin_sav: PathBuf,
    derniere_sav: Option<Vec<u8>>,
    t_sav: f64,
    t_prec: Option<f64>,
    accumul: f64,
    pub images: u64,
    ech: Vec<f32>,
    pub erreur_sav: bool,
    pub etats: Etats,
    /// Emplacement visé par la sauvegarde/le chargement rapides (1 à 10).
    pub courant: usize,
    /// Date de l'état « auto » proposé à la reprise ; la partie attend la réponse.
    pub reprise: Option<u64>,
    /// Dernier emplacement sauvegardé (pour l'annulation).
    pub derniere_sauvegarde: Option<usize>,
    pub rembob: Rembobinage,
    /// Palette des miniatures (tenue à jour par l'écran de jeu).
    pub teintes: [[u8; 3]; 4],
    ferme: bool,
}

/// Message clair pour un état refusé par le cœur.
fn message(e: ErreurEtat) -> String {
    match e {
        ErreurEtat::AutreRom => "Cet état appartient à un autre jeu : rien n'a été chargé".into(),
        ErreurEtat::Version(v) => {
            format!("État d'une version inconnue de gbemu (format {v}) : rien n'a été chargé")
        }
        _ => "État illisible ou abîmé : rien n'a été chargé".into(),
    }
}

impl Partie {
    pub fn lancer(
        rom: Rom,
        titre: String,
        chemin_sav: PathBuf,
        dossier_etats: PathBuf,
        proposer_reprise: bool,
    ) -> Result<Partie, String> {
        let octets =
            crate::biblio::lire_rom(&rom).map_err(|e| format!("Lecture impossible : {e}"))?;
        let mut gb = Machine::new(&octets, rom.plateforme == crate::biblio::Plateforme::Gba)?;
        gb.set_sample_rate(crate::audio::FREQUENCE);
        let derniere_sav = sauve::lire(&chemin_sav);
        if let Some(s) = &derniere_sav {
            gb.import_save(s);
        }
        let etats = Etats::new(dossier_etats);
        let reprise = if proposer_reprise {
            etats.info(Cible::Auto).map(|i| i.date)
        } else {
            None
        };
        Ok(Partie {
            gb,
            titre,
            crc: rom.crc,
            chemin_sav,
            derniere_sav,
            t_sav: 0.0,
            t_prec: None,
            accumul: 0.0,
            images: 0,
            ech: Vec::new(),
            erreur_sav: false,
            etats,
            courant: 1,
            reprise,
            derniere_sauvegarde: None,
            rembob: Rembobinage::standard(),
            teintes: crate::sauve::Palette::default().teintes(),
            ferme: false,
        })
    }

    /// Avance l'émulation jusqu'à `maintenant` (secondes) à `vitesse` (1 = normale), ou recule
    /// si `rembobiner`. Rend vrai si l'image a changé.
    pub fn avancer(
        &mut self,
        maintenant: f64,
        vitesse: f64,
        rembobiner: bool,
        audio: Option<&Sortie>,
        volume: f32,
    ) -> bool {
        let dt = self
            .t_prec
            .map_or(1.0 / 60.0, |t| (maintenant - t).clamp(0.0, 0.1));
        self.t_prec = Some(maintenant);
        if rembobiner {
            return self.reculer(dt);
        }
        // Asservissement léger sur le tampon son (±0,5 %, inaudible) : ni trou ni dérive.
        let normale = vitesse == 1.0;
        let mut v = vitesse;
        if let (true, Some(a)) = (normale, audio) {
            let ms = a.remplissage() as f64 * 1000.0 / (2.0 * crate::audio::FREQUENCE as f64);
            if ms < 40.0 {
                v = 1.005;
            } else if ms > 80.0 {
                v = 0.995;
            }
        }
        self.accumul += dt * IPS * v;
        let mut n = 0;
        while self.accumul >= 1.0 && n < 12 {
            self.gb.run_frame();
            self.accumul -= 1.0;
            self.images += 1;
            n += 1;
            if self.images.is_multiple_of(rembobinage::PAS) {
                self.rembob.pousser(self.gb.save_state());
            }
            self.son(normale, audio, volume);
        }
        if n == 12 {
            self.accumul = 0.0;
        }
        if maintenant - self.t_sav >= INTERVALLE_SAUVEGARDE {
            self.t_sav = maintenant;
            self.sauver();
        }
        n > 0
    }

    /// Recule dans le tampon de rembobinage (son coupé).
    fn reculer(&mut self, dt: f64) -> bool {
        self.accumul += dt * IPS * VITESSE_RECUL / rembobinage::PAS as f64;
        let mut change = false;
        while self.accumul >= 1.0 {
            self.accumul -= 1.0;
            let Some(e) = self.rembob.reculer() else {
                self.accumul = 0.0;
                break;
            };
            // Un état du tampon vient de cette console : il se recharge toujours.
            change |= self.gb.load_state(e).is_ok();
        }
        self.ech.clear();
        self.gb.drain_samples(&mut self.ech);
        change
    }

    fn son(&mut self, jouer: bool, audio: Option<&Sortie>, volume: f32) {
        self.ech.clear();
        self.gb.drain_samples(&mut self.ech);
        if let (true, Some(a)) = (jouer, audio) {
            if volume < 1.0 {
                self.ech.iter_mut().for_each(|s| *s *= volume);
            }
            a.pousser(&self.ech);
        }
    }

    /// Reprend le cadencement sans rattraper le temps passé en pause.
    pub fn reprendre(&mut self) {
        self.t_prec = None;
        self.accumul = 0.0;
    }

    /// Écrit la sauvegarde batterie si elle a changé (écriture atomique).
    pub fn sauver(&mut self) {
        let Some(s) = self.gb.export_save() else {
            return;
        };
        if self.derniere_sav.as_deref() == Some(&s[..]) {
            return;
        }
        match sauve::ecrire_atomique(&self.chemin_sav, &s) {
            Ok(()) => {
                self.derniere_sav = Some(s);
                self.erreur_sav = false;
            }
            Err(_) => self.erreur_sav = true,
        }
    }

    // ----- États -----

    /// Miniature PNG de l'écran courant.
    fn mini(&self) -> Vec<u8> {
        let rgb = self.gb.ecran_rgb(&self.teintes);
        etats::miniature(&rgb, self.gb.largeur(), self.gb.hauteur())
    }

    /// Sauvegarde l'état dans l'emplacement `n` (1 à 10) ; message d'erreur sinon.
    pub fn sauver_etat(&mut self, n: usize) -> Result<(), String> {
        let mini = self.mini();
        match self
            .etats
            .ecrire(Cible::Num(n), &self.gb.save_state(), &mini, false)
        {
            Ok(()) => {
                self.derniere_sauvegarde = Some(n);
                Ok(())
            }
            Err(ErreurEmplacement::Verrouille(n)) => Err(format!("Emplacement {n} verrouillé")),
            Err(ErreurEmplacement::Disque(e)) => Err(format!("État non écrit : {e}")),
        }
    }

    /// Charge un emplacement. L'état d'avant est gardé dans « annule » (Ctrl+Z).
    /// Un état refusé laisse la partie intacte.
    pub fn charger_etat(&mut self, c: Cible) -> Result<(), String> {
        let Some(donnees) = self.etats.lire(c) else {
            return Err(match c {
                Cible::Num(n) => format!("Emplacement {n} vide"),
                _ => "Aucun état à charger".into(),
            });
        };
        let avant = self.gb.save_state();
        self.gb.load_state(&donnees).map_err(message)?;
        let mini = self.mini();
        // Après le chargement : si l'écriture échoue, l'état d'avant n'est perdu que du disque.
        let _ = self.etats.ecrire(Cible::Annule, &avant, &mini, true);
        self.reprendre();
        Ok(())
    }

    /// Ctrl+Z : revient à l'état d'avant le dernier chargement (et inversement si répété).
    pub fn annuler_chargement(&mut self) -> Result<(), String> {
        self.charger_etat(Cible::Annule)
            .map_err(|_| "Aucun chargement à annuler".to_string())
    }

    /// Ctrl+Maj+Z : rétablit la version d'avant la dernière sauvegarde d'état.
    pub fn annuler_sauvegarde(&mut self) -> Result<usize, String> {
        let Some(n) = self.derniere_sauvegarde else {
            return Err("Aucune sauvegarde d'état à annuler".into());
        };
        match self.etats.annuler_sauvegarde(Cible::Num(n)) {
            Ok(true) => Ok(n),
            Ok(false) => {
                // Emplacement vide avant cette sauvegarde : on le vide (fichiers gardés en .suppr).
                self.etats
                    .supprimer(Cible::Num(n))
                    .map_err(|e| format!("Annulation impossible : {e}"))?;
                Ok(n)
            }
            Err(e) => Err(format!("Annulation impossible : {e}")),
        }
    }

    /// Réponse à la carte de reprise : charge l'état « auto » ou repart de la sauvegarde du jeu.
    pub fn repondre_reprise(&mut self, reprendre: bool) -> Result<(), String> {
        self.reprise = None;
        self.reprendre();
        if reprendre {
            let d = self
                .etats
                .lire(Cible::Auto)
                .ok_or("État de reprise introuvable")?;
            self.gb.load_state(&d).map_err(message)?;
        }
        Ok(())
    }

    /// Fermeture de la partie : sauvegarde batterie et état « auto » (reprise au lancement).
    pub fn fermer(&mut self) {
        if self.ferme {
            return;
        }
        self.ferme = true;
        self.sauver();
        eprintln!(
            "rembobinage : {} Kio en mémoire",
            self.rembob.octets() / 1024
        );
        // Carte de reprise sans réponse : l'ancien état auto est gardé tel quel.
        if self.reprise.is_none() && self.images > 0 {
            let mini = self.mini();
            let _ = self
                .etats
                .ecrire(Cible::Auto, &self.gb.save_state(), &mini, true);
        }
    }
}

impl Drop for Partie {
    fn drop(&mut self) {
        self.fermer();
    }
}
