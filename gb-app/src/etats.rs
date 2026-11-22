//! Emplacements d'état d'un jeu : 10 emplacements numérotés, « auto » (reprise) et « annule »
//! (état d'avant le dernier chargement). Dossier `etats/<nom> [CRC]/` : `N.state`, miniature
//! `N.mini.png` (160×144), informations `N.info.json` (date, verrou). Toutes les écritures
//! sont atomiques ; la version précédente reste en `.bak` (annulation de la dernière sauvegarde).

use crate::sauve::ecrire_atomique;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

/// Nombre d'emplacements numérotés (1 à `NB`).
pub const NB: usize = 10;

/// Nom de fichier (sans extension) d'un emplacement.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cible {
    Num(usize),
    Auto,
    Annule,
}

impl Cible {
    fn nom(self) -> String {
        match self {
            Cible::Num(n) => n.to_string(),
            Cible::Auto => "auto".into(),
            Cible::Annule => "annule".into(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Default, Debug, PartialEq)]
#[serde(default)]
pub struct Info {
    /// Secondes depuis 1970 (UTC).
    pub date: u64,
    pub verrou: bool,
}

#[derive(Debug)]
pub enum ErreurEmplacement {
    Verrouille(usize),
    Disque(io::Error),
}

pub struct Etats {
    dossier: PathBuf,
}

pub fn maintenant() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// « à l'instant », « il y a 5 min », « il y a 3 h », « il y a 2 j ».
// raccourci : date relative seulement (pas de fuseau sans dépendance) ; heure locale plus tard.
pub fn date_relative(date: u64, maintenant: u64) -> String {
    let s = maintenant.saturating_sub(date);
    match s {
        0..=59 => "à l'instant".into(),
        60..=3599 => format!("il y a {} min", s / 60),
        3600..=86_399 => format!("il y a {} h", s / 3600),
        _ => format!("il y a {} j", s / 86_400),
    }
}

/// Miniature PNG RGB 160×144 de l'écran (teintes 0-3) avec la palette donnée.
pub fn miniature(ecran: &[u8], teintes: &[[u8; 3]; 4]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(ecran.len() * 3);
    for &v in ecran {
        rgb.extend_from_slice(&teintes[(v & 3) as usize]);
    }
    encoder_png(&rgb, gb_core::LARGEUR as u32, gb_core::HAUTEUR as u32).unwrap_or_default()
}

/// PNG RGB 8 bits en mémoire.
pub fn encoder_png(rgb: &[u8], l: u32, h: u32) -> Option<Vec<u8>> {
    let mut sortie = Vec::new();
    let mut enc = png::Encoder::new(&mut sortie, l, h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().ok()?;
    w.write_image_data(rgb).ok()?;
    w.finish().ok()?;
    Some(sortie)
}

impl Etats {
    pub fn new(dossier: PathBuf) -> Etats {
        Etats { dossier }
    }

    fn fichier(&self, c: Cible, ext: &str) -> PathBuf {
        self.dossier.join(format!("{}.{ext}", c.nom()))
    }

    fn etat(&self, c: Cible) -> PathBuf {
        self.fichier(c, "state")
    }

    fn mini(&self, c: Cible) -> PathBuf {
        self.fichier(c, "mini.png")
    }

    fn infos(&self, c: Cible) -> PathBuf {
        self.fichier(c, "info.json")
    }

    /// Informations d'un emplacement occupé (`None` : vide).
    pub fn info(&self, c: Cible) -> Option<Info> {
        if !self.etat(c).is_file() {
            return None;
        }
        Some(
            std::fs::read(self.infos(c))
                .ok()
                .and_then(|d| serde_json::from_slice(&d).ok())
                .unwrap_or_default(),
        )
    }

    /// Données de l'état (pas de repli sur `.bak` : un emplacement supprimé reste vide).
    pub fn lire(&self, c: Cible) -> Option<Vec<u8>> {
        std::fs::read(self.etat(c)).ok()
    }

    pub fn lire_mini(&self, c: Cible) -> Option<Vec<u8>> {
        std::fs::read(self.mini(c)).ok()
    }

    /// Écrit un état et sa miniature. Un emplacement verrouillé est refusé sauf `forcer`.
    pub fn ecrire(
        &self,
        c: Cible,
        etat: &[u8],
        mini: &[u8],
        forcer: bool,
    ) -> Result<(), ErreurEmplacement> {
        let ancienne = self.info(c);
        if let (Cible::Num(n), Some(Info { verrou: true, .. }), false) = (c, ancienne, forcer) {
            return Err(ErreurEmplacement::Verrouille(n));
        }
        let info = Info {
            date: maintenant(),
            verrou: ancienne.is_some_and(|i| i.verrou),
        };
        let json = serde_json::to_vec(&info).unwrap_or_default();
        // L'état en dernier : s'il manque, l'emplacement est vu vide (jamais à moitié écrit).
        if !mini.is_empty() {
            ecrire_atomique(&self.mini(c), mini).map_err(ErreurEmplacement::Disque)?;
        }
        ecrire_atomique(&self.infos(c), &json).map_err(ErreurEmplacement::Disque)?;
        ecrire_atomique(&self.etat(c), etat).map_err(ErreurEmplacement::Disque)
    }

    pub fn verrouiller(&self, c: Cible, verrou: bool) -> io::Result<()> {
        let mut info = self.info(c).unwrap_or_default();
        info.verrou = verrou;
        let json = serde_json::to_vec(&info).unwrap_or_default();
        // Pas d'`ecrire_atomique` : le `.bak` des informations doit rester celui de la sauvegarde.
        let tmp = self.dossier.join(format!("{}.verrou.tmp", c.nom()));
        std::fs::create_dir_all(&self.dossier)?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(tmp, self.infos(c))
    }

    /// Retour à la version d'avant la dernière sauvegarde de l'emplacement (fichiers `.bak`) ;
    /// relancé une seconde fois, rétablit la sauvegarde annulée. Faux s'il n'y a rien à annuler.
    pub fn annuler_sauvegarde(&self, c: Cible) -> io::Result<bool> {
        let bak = |p: &Path| p.with_extension("bak");
        let e = self.etat(c);
        let Ok(ancien) = std::fs::read(bak(&e)) else {
            return Ok(false);
        };
        let verrou = self.info(c).is_some_and(|i| i.verrou);
        for f in [self.mini(c), self.infos(c)] {
            if let Ok(d) = std::fs::read(bak(&f)) {
                ecrire_atomique(&f, &d)?;
            }
        }
        ecrire_atomique(&e, &ancien)?;
        if verrou {
            self.verrouiller(c, true)?;
        }
        Ok(true)
    }

    /// Vide un emplacement sans rien détruire : les fichiers sont renommés en `.suppr`.
    pub fn supprimer(&self, c: Cible) -> io::Result<()> {
        for f in [self.etat(c), self.mini(c), self.infos(c)] {
            if f.exists() {
                std::fs::rename(&f, f.with_extension("suppr"))?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dossier(nom: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("gbemu-test-etats-{nom}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn emplacements_et_verrou() {
        let e = Etats::new(dossier("verrou"));
        assert!(e.info(Cible::Num(1)).is_none());
        e.ecrire(Cible::Num(1), b"un", b"mini", false).unwrap();
        assert_eq!(e.lire(Cible::Num(1)).unwrap(), b"un");
        assert_eq!(e.lire_mini(Cible::Num(1)).unwrap(), b"mini");
        assert!(e.info(Cible::Num(1)).unwrap().date > 0);
        e.verrouiller(Cible::Num(1), true).unwrap();
        assert!(matches!(
            e.ecrire(Cible::Num(1), b"deux", b"", false),
            Err(ErreurEmplacement::Verrouille(1))
        ));
        assert_eq!(e.lire(Cible::Num(1)).unwrap(), b"un");
        // Les emplacements sont indépendants, auto et annule aussi.
        e.ecrire(Cible::Num(2), b"deux", b"", false).unwrap();
        e.ecrire(Cible::Auto, b"auto", b"", false).unwrap();
        assert_eq!(e.lire(Cible::Auto).unwrap(), b"auto");
        assert!(e.lire(Cible::Annule).is_none());
        e.verrouiller(Cible::Num(1), false).unwrap();
        e.ecrire(Cible::Num(1), b"trois", b"", false).unwrap();
        assert_eq!(e.lire(Cible::Num(1)).unwrap(), b"trois");
    }

    #[test]
    fn ecriture_atomique_et_annulation() {
        let d = dossier("annule");
        let e = Etats::new(d.clone());
        e.ecrire(Cible::Num(3), b"ancien", b"m1", false).unwrap();
        e.ecrire(Cible::Num(3), b"nouveau", b"m2", false).unwrap();
        // Aucun fichier temporaire ne traîne.
        let restes: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .filter_map(|f| f.ok())
            .filter(|f| f.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(restes.is_empty());
        assert!(e.annuler_sauvegarde(Cible::Num(3)).unwrap());
        assert_eq!(e.lire(Cible::Num(3)).unwrap(), b"ancien");
        assert_eq!(e.lire_mini(Cible::Num(3)).unwrap(), b"m1");
        assert!(e.annuler_sauvegarde(Cible::Num(3)).unwrap());
        assert_eq!(e.lire(Cible::Num(3)).unwrap(), b"nouveau");
        assert!(!e.annuler_sauvegarde(Cible::Num(4)).unwrap());
    }

    #[test]
    fn suppression_sans_perte() {
        let d = dossier("suppr");
        let e = Etats::new(d.clone());
        e.ecrire(Cible::Num(5), b"a", b"m", false).unwrap();
        e.ecrire(Cible::Num(5), b"b", b"m", false).unwrap();
        e.supprimer(Cible::Num(5)).unwrap();
        assert!(e.info(Cible::Num(5)).is_none());
        assert!(e.lire(Cible::Num(5)).is_none());
        assert_eq!(std::fs::read(d.join("5.suppr")).unwrap(), b"b");
    }

    #[test]
    fn miniature_decodable() {
        let ecran = vec![2u8; gb_core::LARGEUR * gb_core::HAUTEUR];
        let png = miniature(&ecran, &[[1, 2, 3], [4, 5, 6], [7, 8, 9], [0, 0, 0]]);
        let img = crate::jaquettes::decoder(&png).unwrap();
        assert_eq!((img.largeur, img.hauteur), (160, 144));
        assert_eq!(&img.rgba[..4], &[7, 8, 9, 255]);
    }

    #[test]
    fn dates() {
        assert_eq!(date_relative(100, 130), "à l'instant");
        assert_eq!(date_relative(0, 300), "il y a 5 min");
        assert_eq!(date_relative(0, 7200), "il y a 2 h");
        assert_eq!(date_relative(0, 3 * 86_400), "il y a 3 j");
    }
}
