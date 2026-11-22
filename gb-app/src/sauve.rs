//! Emplacements, réglages et écritures sur disque (atomiques : jamais de fichier à moitié écrit).

use crate::biblio::Rom;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Palette {
    #[default]
    Verte,
    Grise,
    /// Vert franc de la console d'origine.
    Origine,
    /// Gris neutres de la version de poche.
    Poche,
    /// Sarcelle rétroéclairée.
    Lumiere,
    Sepia,
    /// Inversée douce.
    Nuit,
}

impl Palette {
    /// Teintes 0 (clair) à 3 (foncé), en RGB.
    pub fn teintes(self) -> [[u8; 3]; 4] {
        match self {
            Palette::Verte => [
                [0xC4, 0xCF, 0xA1],
                [0x8B, 0x95, 0x6D],
                [0x4D, 0x53, 0x3C],
                [0x1F, 0x1F, 0x1F],
            ],
            Palette::Grise => [
                [0xE8, 0xE8, 0xE2],
                [0xA8, 0xA8, 0xA2],
                [0x60, 0x60, 0x5C],
                [0x1C, 0x1C, 0x1C],
            ],
            Palette::Origine => [
                [0x9B, 0xBC, 0x0F],
                [0x8B, 0xAC, 0x0F],
                [0x30, 0x62, 0x30],
                [0x0F, 0x38, 0x0F],
            ],
            Palette::Poche => [
                [0xC5, 0xC6, 0xB8],
                [0x8C, 0x8D, 0x80],
                [0x51, 0x52, 0x49],
                [0x21, 0x22, 0x1D],
            ],
            Palette::Lumiere => [
                [0x6B, 0xE8, 0xD4],
                [0x3F, 0xB8, 0xA8],
                [0x1E, 0x76, 0x74],
                [0x0B, 0x33, 0x3A],
            ],
            Palette::Sepia => [
                [0xF0, 0xE0, 0xC0],
                [0xC0, 0x9C, 0x70],
                [0x7A, 0x58, 0x3A],
                [0x30, 0x20, 0x14],
            ],
            Palette::Nuit => [
                [0x24, 0x26, 0x38],
                [0x4E, 0x52, 0x74],
                [0x9A, 0x9E, 0xC0],
                [0xE4, 0xE2, 0xEE],
            ],
        }
    }

    pub fn nom(self) -> &'static str {
        match self {
            Palette::Verte => "Verte",
            Palette::Grise => "Grise",
            Palette::Origine => "Origine",
            Palette::Poche => "Poche",
            Palette::Lumiere => "Lumière",
            Palette::Sepia => "Sépia",
            Palette::Nuit => "Nuit",
        }
    }

    pub fn suivante(self) -> Palette {
        match self {
            Palette::Verte => Palette::Grise,
            Palette::Grise => Palette::Origine,
            Palette::Origine => Palette::Poche,
            Palette::Poche => Palette::Lumiere,
            Palette::Lumiere => Palette::Sepia,
            Palette::Sepia => Palette::Nuit,
            Palette::Nuit => Palette::Verte,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Config {
    /// Dossier des jeux choisi par l'utilisateur (`None` : « jeux » à côté de l'exécutable).
    pub dossier_jeux: Option<PathBuf>,
    /// Dossiers et fichiers ajoutés par glisser-déposer.
    pub ajouts: Vec<PathBuf>,
    pub palette: Palette,
    pub grille: bool,
    /// Thème Jour (sinon Nuit).
    pub jour: bool,
    /// « Réduire les animations ».
    pub animations_reduites: bool,
    /// Sons d'interface coupés.
    pub sons_coupes: bool,
    /// Écran de démarrage désactivé.
    pub sans_intro: bool,
    /// Avance rapide (ou ralenti) appliquée par Tab.
    pub vitesse: crate::jeu::Vitesse,
    /// Filtre de rémanence (mélange avec l'image précédente).
    pub remanence: bool,
    /// Effet rétroéclairé (palette éclaircie, halo).
    pub retroeclaire: bool,
    /// Correction des couleurs GBC désactivée (couleurs brutes, plus vives).
    pub sans_correction: bool,
    /// Volume du jeu, 0 à 1 (`None` : 100 %).
    pub volume: Option<f32>,
    pub muet: bool,
    /// Palette choisie par jeu (CRC en hexadécimal).
    pub palettes_jeux: std::collections::BTreeMap<String, Palette>,
}

pub struct Chemins {
    pub jeux_defaut: PathBuf,
    pub donnees: PathBuf,
}

impl Chemins {
    /// Mode portable : tout à côté de l'exécutable ; sinon dossier de données de l'utilisateur.
    pub fn trouver(donnees_force: Option<PathBuf>) -> Chemins {
        let base = std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        let jeux_defaut = base.join("jeux");
        let donnees = donnees_force.unwrap_or_else(|| {
            let portable = base.join("gbemu-donnees");
            if std::fs::create_dir_all(&portable).is_ok() && inscriptible(&portable) {
                portable
            } else {
                dossier_utilisateur().join("gbemu")
            }
        });
        let _ = std::fs::create_dir_all(&donnees);
        Chemins {
            jeux_defaut,
            donnees,
        }
    }

    pub fn config(&self) -> PathBuf {
        self.donnees.join("reglages.json")
    }

    pub fn index(&self) -> PathBuf {
        self.donnees.join("index.json")
    }

    pub fn dat(&self) -> PathBuf {
        self.donnees.join("dat")
    }

    pub fn jaquettes(&self) -> PathBuf {
        self.donnees.join("jaquettes")
    }

    /// Sauvegarde batterie d'une ROM : nom lisible + CRC (stable si le fichier est déplacé).
    pub fn sauvegarde(&self, rom: &Rom) -> PathBuf {
        let nom: String = rom
            .nom_fichier()
            .chars()
            .map(|c| {
                if "<>:\"/\\|?*".contains(c) || c.is_control() {
                    '_'
                } else {
                    c
                }
            })
            .take(80)
            .collect();
        self.donnees
            .join("sauvegardes")
            .join(format!("{} [{:08X}].sav", nom.trim(), rom.crc))
    }

    /// Dossier des états d'une ROM (même nom que la sauvegarde batterie).
    pub fn etats(&self, rom: &Rom) -> PathBuf {
        let sav = self.sauvegarde(rom);
        let nom = sav.file_stem().unwrap_or_default().to_os_string();
        self.donnees.join("etats").join(nom)
    }

    pub fn captures(&self) -> PathBuf {
        self.donnees.join("captures")
    }
}

fn inscriptible(dossier: &Path) -> bool {
    // Fichier vide laissé en place : moins risqué qu'une suppression, inoffensif.
    std::fs::write(dossier.join(".essai-ecriture"), b"").is_ok()
}

fn dossier_utilisateur() -> PathBuf {
    #[cfg(windows)]
    let d = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let d = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    d.unwrap_or_else(std::env::temp_dir)
}

/// Écriture atomique : fichier temporaire synchronisé puis renommé. L'ancienne version est
/// gardée en `.bak` (relue si le fichier principal manque).
pub fn ecrire_atomique(chemin: &Path, donnees: &[u8]) -> io::Result<()> {
    if let Some(p) = chemin.parent() {
        std::fs::create_dir_all(p)?;
    }
    let tmp = chemin.with_extension("tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(donnees)?;
        f.sync_all()?;
    }
    if chemin.exists() {
        let _ = std::fs::rename(chemin, chemin.with_extension("bak"));
    }
    std::fs::rename(&tmp, chemin)
}

/// Lit un fichier écrit par [`ecrire_atomique`], avec repli sur la copie `.bak`.
pub fn lire(chemin: &Path) -> Option<Vec<u8>> {
    std::fs::read(chemin)
        .ok()
        .or_else(|| std::fs::read(chemin.with_extension("bak")).ok())
}

impl Config {
    pub fn charger(chemin: &Path) -> Config {
        lire(chemin)
            .and_then(|d| serde_json::from_slice(&d).ok())
            .unwrap_or_default()
    }

    pub fn enregistrer(&self, chemin: &Path) {
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let _ = ecrire_atomique(chemin, &json);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ecriture_atomique_garde_l_ancienne() {
        let d = std::env::temp_dir().join(format!("gbemu-test-sauve-{}", std::process::id()));
        let f = d.join("x.sav");
        ecrire_atomique(&f, b"un").unwrap();
        ecrire_atomique(&f, b"deux").unwrap();
        assert_eq!(lire(&f).unwrap(), b"deux");
        assert_eq!(std::fs::read(f.with_extension("bak")).unwrap(), b"un");
        assert!(!f.with_extension("tmp").exists());
    }
}
