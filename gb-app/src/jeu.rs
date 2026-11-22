//! Partie en cours : émulation cadencée sur l'horloge, son, sauvegarde batterie automatique.

use crate::audio::Sortie;
use crate::biblio::Rom;
use crate::sauve;
use gb_core::GameBoy;
use std::path::PathBuf;

/// Images par seconde de la Game Boy (4 194 304 / 70 224).
const IPS: f64 = 59.7275;
/// Facteur d'avance rapide.
pub const AVANCE: f64 = 4.0;
/// Intervalle de vérification de la sauvegarde batterie (secondes).
const INTERVALLE_SAUVEGARDE: f64 = 2.0;

pub struct Partie {
    pub gb: GameBoy,
    pub titre: String,
    chemin_sav: PathBuf,
    derniere_sav: Option<Vec<u8>>,
    t_sav: f64,
    t_prec: Option<f64>,
    accumul: f64,
    pub images: u64,
    ech: Vec<f32>,
    pub erreur_sav: bool,
}

impl Partie {
    pub fn lancer(rom: Rom, titre: String, chemin_sav: PathBuf) -> Result<Partie, String> {
        let octets =
            crate::biblio::lire_rom(&rom).map_err(|e| format!("Lecture impossible : {e}"))?;
        let mut gb = GameBoy::new(&octets).map_err(|e| format!("ROM refusée : {e:?}"))?;
        gb.set_sample_rate(crate::audio::FREQUENCE);
        let derniere_sav = sauve::lire(&chemin_sav);
        if let Some(s) = &derniere_sav {
            gb.import_save(s);
        }
        Ok(Partie {
            gb,
            titre,
            chemin_sav,
            derniere_sav,
            t_sav: 0.0,
            t_prec: None,
            accumul: 0.0,
            images: 0,
            ech: Vec::new(),
            erreur_sav: false,
        })
    }

    /// Avance l'émulation jusqu'à `maintenant` (secondes). Rend vrai si une image a été produite.
    pub fn avancer(&mut self, maintenant: f64, rapide: bool, audio: Option<&Sortie>) -> bool {
        let dt = self
            .t_prec
            .map_or(1.0 / 60.0, |t| (maintenant - t).clamp(0.0, 0.1));
        self.t_prec = Some(maintenant);
        // Asservissement léger sur le tampon son (±0,5 %, inaudible) : ni trou ni dérive.
        let mut vitesse = if rapide { AVANCE } else { 1.0 };
        if let (false, Some(a)) = (rapide, audio) {
            let ms = a.remplissage() as f64 * 1000.0 / (2.0 * crate::audio::FREQUENCE as f64);
            if ms < 40.0 {
                vitesse = 1.005;
            } else if ms > 80.0 {
                vitesse = 0.995;
            }
        }
        self.accumul += dt * IPS * vitesse;
        let mut n = 0;
        while self.accumul >= 1.0 && n < 12 {
            self.gb.run_frame();
            self.accumul -= 1.0;
            self.images += 1;
            n += 1;
            self.son(rapide, audio);
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

    fn son(&mut self, rapide: bool, audio: Option<&Sortie>) {
        self.ech.clear();
        self.gb.drain_samples(&mut self.ech);
        if let (false, Some(a)) = (rapide, audio) {
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
}

impl Drop for Partie {
    fn drop(&mut self) {
        self.sauver();
    }
}
