//! Vie de la bibliothèque : favoris, temps de jeu et dernier lancement par jeu (clé : CRC32),
//! dans `stats.json` (écriture atomique). Fichier absent ou abîmé : on repart de zéro.

use crate::sauve;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Clone, Copy, Default, PartialEq, Debug)]
#[serde(default)]
pub struct Jeu {
    /// Temps de jeu cumulé (secondes, pause exclue).
    pub temps: f64,
    /// Dernier lancement (secondes depuis 1970, 0 = jamais).
    pub dernier: u64,
    pub favori: bool,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Fichier {
    version: u32,
    jeux: HashMap<u32, Jeu>,
}

pub struct Stats {
    /// `None` : rien n'est écrit (mode démonstration).
    chemin: Option<PathBuf>,
    jeux: HashMap<u32, Jeu>,
    /// Jeu en cours et temps non encore écrit.
    en_cours: Option<u32>,
    sale: bool,
    t_ecrit: f64,
}

pub fn maintenant() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Stats {
    pub fn charger(chemin: PathBuf) -> Stats {
        let jeux = sauve::lire(&chemin)
            .and_then(|d| serde_json::from_slice::<Fichier>(&d).ok())
            .map(|f| f.jeux)
            .unwrap_or_default();
        Stats {
            chemin: Some(chemin),
            jeux,
            en_cours: None,
            sale: false,
            t_ecrit: 0.0,
        }
    }

    /// Statistiques fictives pour juger la bibliothèque de démonstration (jamais écrites).
    pub fn demo() -> Stats {
        let t = maintenant();
        let mut jeux = HashMap::new();
        for (crc, temps, age, favori) in [
            (23u32, 11_520.0, 7_200u64, true),
            (1, 2_400.0, 86_400, false),
            (17, 720.0, 3 * 86_400, true),
            (9, 41_000.0, 9 * 86_400, false),
            (19, 300.0, 20 * 86_400, false),
        ] {
            jeux.insert(
                crc,
                Jeu {
                    temps,
                    dernier: t - age,
                    favori,
                },
            );
        }
        Stats {
            chemin: None,
            jeux,
            en_cours: None,
            sale: false,
            t_ecrit: 0.0,
        }
    }

    pub fn jeu(&self, crc: u32) -> Jeu {
        self.jeux.get(&crc).copied().unwrap_or_default()
    }

    pub fn basculer_favori(&mut self, crc: u32) -> bool {
        let j = self.jeux.entry(crc).or_default();
        j.favori = !j.favori;
        let f = j.favori;
        self.ecrire();
        f
    }

    /// Une partie commence : date de lancement notée tout de suite.
    pub fn lancer(&mut self, crc: u32) {
        self.jeux.entry(crc).or_default().dernier = maintenant();
        self.en_cours = Some(crc);
        self.ecrire();
    }

    /// Jeu en cours (CRC), s'il y en a un.
    pub fn en_cours(&self) -> Option<u32> {
        self.en_cours
    }

    /// Ajoute `dt` secondes de jeu effectif ; écrit au plus toutes les 30 s.
    pub fn compter(&mut self, dt: f32, t: f64) {
        let Some(crc) = self.en_cours else { return };
        self.jeux.entry(crc).or_default().temps += dt as f64;
        self.sale = true;
        if t - self.t_ecrit > 30.0 {
            self.t_ecrit = t;
            self.ecrire();
        }
    }

    pub fn arreter(&mut self) {
        self.en_cours = None;
        if self.sale {
            self.ecrire();
        }
    }

    fn ecrire(&mut self) {
        self.sale = false;
        let Some(chemin) = &self.chemin else { return };
        let f = Fichier {
            version: 1,
            jeux: self.jeux.clone(),
        };
        if let Ok(json) = serde_json::to_vec(&f) {
            let _ = sauve::ecrire_atomique(chemin, &json);
        }
    }
}

/// « 3 h 12 », « 12 min », vide sous une minute.
pub fn duree_jeu(s: f64) -> String {
    let m = (s / 60.0) as u64;
    match m {
        0 => String::new(),
        1..=59 => format!("{m} min"),
        _ => format!("{} h {:02}", m / 60, m % 60),
    }
}

/// « à l'instant », « il y a 5 min », « il y a 2 h », « hier », « il y a 3 j ».
pub fn il_y_a(dernier: u64) -> String {
    if dernier == 0 {
        return String::new();
    }
    let s = maintenant().saturating_sub(dernier);
    match s {
        0..=59 => "à l'instant".to_string(),
        60..=3_599 => format!("il y a {} min", s / 60),
        3_600..=86_399 => format!("il y a {} h", s / 3_600),
        86_400..=172_799 => "hier".to_string(),
        _ => format!("il y a {} j", s / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aller_retour_et_fichier_abime() {
        let d = std::env::temp_dir().join(format!("gbemu-stats-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        let c = d.join("stats.json");
        let mut s = Stats::charger(c.clone());
        s.lancer(7);
        s.compter(90.0, 1.0);
        s.arreter();
        assert!(s.basculer_favori(7));
        let s2 = Stats::charger(c.clone());
        assert_eq!(s2.jeu(7).temps, 90.0);
        assert!(s2.jeu(7).favori && s2.jeu(7).dernier > 0);
        std::fs::write(&c, b"{pas du json").unwrap();
        let _ = std::fs::remove_file(c.with_extension("bak"));
        assert_eq!(Stats::charger(c).jeu(7), Jeu::default());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn durees_lisibles() {
        assert_eq!(duree_jeu(30.0), "");
        assert_eq!(duree_jeu(720.0), "12 min");
        assert_eq!(duree_jeu(11_520.0), "3 h 12");
    }
}
