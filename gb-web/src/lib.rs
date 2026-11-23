//! Version web : gb-core (GB/GBC) et gba-core (GBA) exposés au navigateur par wasm-bindgen,
//! plus la lecture des archives .zip et la reconnaissance des ROM. Aucune E/S : la page fournit
//! les octets (IndexedDB) et lit l'écran directement dans la mémoire du module.
//! Jamais de panique sur une ROM ou une archive malformée : les erreurs remontent en JS.

use gb_core::GameBoy;
use gba_core::{touche, Gba};
use std::io::Read;
use wasm_bindgen::prelude::*;

/// Bits de [`Emu::set_boutons`] : ceux de `gb_core::bouton`, plus L et R pour la GBA.
pub const BIT_L: u16 = 0x100;
pub const BIT_R: u16 = 0x200;

/// Palette DMG « Verte » de l'app de bureau (`gb-app/src/sauve.rs`).
const TEINTES: [[u8; 3]; 4] = [
    [0xC4, 0xCF, 0xA1],
    [0x8B, 0x95, 0x6D],
    [0x4D, 0x53, 0x3C],
    [0x1F, 0x1F, 0x1F],
];

/// Taille maximale d'une cartouche (comme `gb-app/src/biblio.rs`).
const TAILLE_MAX_GB: usize = 8 << 20;
const TAILLE_MAX_GBA: usize = 32 << 20;
/// Nombre maximal d'entrées lues dans une archive.
const ENTREES_MAX: usize = 4096;

fn erreur(s: impl std::fmt::Display) -> JsError {
    JsError::new(&s.to_string())
}

/// Fichier extrait d'une archive.
#[wasm_bindgen]
pub struct Fichier {
    nom: String,
    octets: Vec<u8>,
}

#[wasm_bindgen]
impl Fichier {
    /// Nom sans le chemin dans l'archive.
    #[wasm_bindgen(getter)]
    pub fn nom(&self) -> String {
        self.nom.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn octets(&self) -> Vec<u8> {
        self.octets.clone()
    }
}

fn extension(nom: &str) -> String {
    nom.rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// Lit les ROM (.gb, .gbc, .gba) et sauvegardes (.sav) d'une archive .zip, en mémoire.
/// Les entrées illisibles ou trop grandes sont ignorées ; archive illisible = erreur.
#[wasm_bindgen]
pub fn ouvrir_zip(octets: &[u8]) -> Result<Vec<Fichier>, JsError> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(octets))
        .map_err(|_| erreur("archive .zip illisible"))?;
    let mut sortie = Vec::new();
    for i in 0..zip.len().min(ENTREES_MAX) {
        let Ok(e) = zip.by_index(i) else {
            continue;
        };
        let Ok(chemin) = e.name().map(|n| n.to_string()) else {
            continue;
        };
        let nom = chemin.rsplit('/').next().unwrap_or("").to_string();
        let max = match extension(&nom).as_str() {
            "gb" | "gbc" => TAILLE_MAX_GB,
            "gba" => TAILLE_MAX_GBA,
            "sav" => 1 << 20,
            _ => continue,
        };
        if e.is_dir() || e.size() == 0 || e.size() > max as u64 {
            continue;
        }
        // La taille annoncée peut mentir : lecture bornée.
        let mut donnees = Vec::with_capacity(e.size() as usize);
        if e.take(max as u64 + 1).read_to_end(&mut donnees).is_err() || donnees.len() > max {
            continue;
        }
        sortie.push(Fichier {
            nom,
            octets: donnees,
        });
    }
    Ok(sortie)
}

/// Résultat de [`analyser_rom`].
#[wasm_bindgen]
pub struct InfoRom {
    console: String,
    titre: String,
    crc: u32,
}

#[wasm_bindgen]
impl InfoRom {
    /// "gb", "gbc" ou "gba".
    #[wasm_bindgen(getter)]
    pub fn console(&self) -> String {
        self.console.clone()
    }

    /// Titre de l'en-tête (ASCII imprimable, peut être vide).
    #[wasm_bindgen(getter)]
    pub fn titre(&self) -> String {
        self.titre.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn crc(&self) -> u32 {
        self.crc
    }
}

const LOGO_GB: [u8; 4] = [0xCE, 0xED, 0x66, 0x66];

/// Console d'après l'extension, sinon d'après l'en-tête (logo GB en 0x104, octet fixe GBA 0x96
/// en 0xB2). Un .gb « CGB » (0x143 = 0x80 ou 0xC0) est classé GBC.
fn console_de(nom: &str, rom: &[u8]) -> Option<&'static str> {
    let gb = |r: &[u8]| match r.get(0x143) {
        Some(0x80 | 0xC0) => "gbc",
        _ => "gb",
    };
    match extension(nom).as_str() {
        "gba" => Some("gba"),
        "gb" | "gbc" => Some(gb(rom)),
        _ if rom.get(0x104..0x108) == Some(&LOGO_GB[..]) => Some(gb(rom)),
        _ if rom.get(0xB2) == Some(&0x96) => Some("gba"),
        _ => None,
    }
}

fn titre_entete(console: &str, rom: &[u8]) -> String {
    let plage = match console {
        "gba" => 0xA0..0xAC,
        "gbc" => 0x134..0x13F,
        _ => 0x134..0x144,
    };
    rom.get(plage)
        .unwrap_or(&[])
        .iter()
        .take_while(|&&b| b != 0)
        .filter(|b| b.is_ascii_graphic() || **b == b' ')
        .map(|&b| b as char)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Reconnaît une ROM : console, titre d'en-tête, CRC32. Erreur si le fichier n'est pas une ROM
/// GB/GBC/GBA plausible (taille, en-tête).
#[wasm_bindgen]
pub fn analyser_rom(nom: &str, rom: &[u8]) -> Result<InfoRom, JsError> {
    let console =
        console_de(nom, rom).ok_or_else(|| erreur("ce n'est pas une ROM GB, GBC ou GBA"))?;
    let (min, max) = match console {
        "gba" => (0xC0, TAILLE_MAX_GBA),
        _ => (0x150, TAILLE_MAX_GB),
    };
    if rom.len() < min {
        return Err(erreur("fichier trop court pour une ROM"));
    }
    if rom.len() > max {
        return Err(erreur("fichier trop gros pour une cartouche"));
    }
    Ok(InfoRom {
        console: console.to_string(),
        titre: titre_entete(console, rom),
        crc: crc32fast::hash(rom),
    })
}

enum Machine {
    Gb(Box<GameBoy>),
    Gba(Box<Gba>),
}

/// Une console en marche.
#[wasm_bindgen]
pub struct Emu {
    m: Machine,
    /// Écran RGBA lu par la page directement dans la mémoire du module ([`Emu::ecran`]).
    rgba: Vec<u8>,
}

#[wasm_bindgen]
impl Emu {
    /// Démarre `rom` sur la console donnée par [`analyser_rom`] ("gb", "gbc" ou "gba").
    /// BIOS libre embarqué pour la GBA.
    #[wasm_bindgen(constructor)]
    pub fn new(console: &str, rom: &[u8]) -> Result<Emu, JsError> {
        let m = match console {
            "gba" => Machine::Gba(Box::new(
                Gba::new(rom, None).map_err(|e| erreur(format!("ROM refusée : {e}")))?,
            )),
            "gb" | "gbc" => Machine::Gb(Box::new(
                GameBoy::new(rom).map_err(|e| erreur(format!("ROM refusée : {e}")))?,
            )),
            _ => return Err(erreur("console inconnue")),
        };
        let (l, h) = match m {
            Machine::Gb(_) => (gb_core::LARGEUR, gb_core::HAUTEUR),
            Machine::Gba(_) => (gba_core::LARGEUR, gba_core::HAUTEUR),
        };
        Ok(Emu {
            m,
            rgba: vec![0xFF; l * h * 4],
        })
    }

    #[wasm_bindgen(getter)]
    pub fn largeur(&self) -> u32 {
        match self.m {
            Machine::Gb(_) => gb_core::LARGEUR as u32,
            Machine::Gba(_) => gba_core::LARGEUR as u32,
        }
    }

    #[wasm_bindgen(getter)]
    pub fn hauteur(&self) -> u32 {
        match self.m {
            Machine::Gb(_) => gb_core::HAUTEUR as u32,
            Machine::Gba(_) => gba_core::HAUTEUR as u32,
        }
    }

    /// Fréquence des images en Hz (59,7275 GB, 16 777 216 / 280 896 GBA).
    #[wasm_bindgen(getter)]
    pub fn hz(&self) -> f64 {
        match self.m {
            Machine::Gb(_) => 4_194_304.0 / 70_224.0,
            Machine::Gba(_) => 16_777_216.0 / gba_core::CYCLES_PAR_IMAGE as f64,
        }
    }

    pub fn run_frame(&mut self) {
        match &mut self.m {
            Machine::Gb(g) => g.run_frame(),
            Machine::Gba(g) => g.run_frame(),
        }
    }

    /// Copie l'écran en RGBA (palette verte en DMG) et rend son adresse dans la mémoire du
    /// module (largeur × hauteur × 4 octets, valable jusqu'au prochain appel).
    pub fn ecran(&mut self) -> *const u8 {
        let d = &mut self.rgba;
        match &self.m {
            Machine::Gba(g) => rgb_vers_rgba(g.frame(), d),
            Machine::Gb(g) if g.is_cgb() => rgb_vers_rgba(g.frame_rgb(), d),
            Machine::Gb(g) => {
                for (p, &v) in d.as_chunks_mut::<4>().0.iter_mut().zip(g.frame()) {
                    p[..3].copy_from_slice(&TEINTES[(v & 3) as usize]);
                }
            }
        }
        d.as_ptr()
    }

    /// Boutons tenus : bits de `gb_core::bouton`, plus [`BIT_L`] et [`BIT_R`].
    pub fn set_boutons(&mut self, b: u16) {
        match &mut self.m {
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

    /// Échantillons stéréo entrelacés (G, D) produits depuis le dernier appel.
    pub fn echantillons(&mut self) -> Vec<f32> {
        let mut s = Vec::new();
        match &mut self.m {
            Machine::Gb(g) => g.drain_samples(&mut s),
            Machine::Gba(g) => g.drain_samples(&mut s),
        }
        s
    }

    /// Fréquence de sortie du son (Hz) : celle de l'AudioContext.
    pub fn set_frequence(&mut self, hz: u32) {
        match &mut self.m {
            Machine::Gb(g) => g.set_sample_rate(hz),
            Machine::Gba(g) => g.set_sample_rate(hz),
        }
    }

    /// Sauvegarde batterie à écrire (.sav), `undefined` s'il n'y en a pas (encore).
    pub fn sauvegarde(&self) -> Option<Vec<u8>> {
        match &self.m {
            Machine::Gb(g) => g.export_save(),
            Machine::Gba(g) => g.sauvegarde(),
        }
    }

    /// Recharge un .sav (taille quelconque, jamais de panique).
    pub fn charger_sauvegarde(&mut self, data: &[u8]) {
        match &mut self.m {
            Machine::Gb(g) => g.import_save(data),
            Machine::Gba(g) => g.charger_sauvegarde(data),
        }
    }

    pub fn sauver_etat(&self) -> Vec<u8> {
        match &self.m {
            Machine::Gb(g) => g.save_state(),
            Machine::Gba(g) => g.save_state(),
        }
    }

    /// Recharge un état ; en cas d'erreur la console reste dans son état précédent.
    pub fn charger_etat(&mut self, data: &[u8]) -> Result<(), JsError> {
        match &mut self.m {
            Machine::Gb(g) => g.load_state(data).map_err(erreur),
            Machine::Gba(g) => g.load_state(data).map_err(erreur),
        }
    }
}

fn rgb_vers_rgba(rgb: &[u8], rgba: &mut [u8]) {
    for (p, c) in rgba
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(rgb.as_chunks::<3>().0)
    {
        p[..3].copy_from_slice(c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_par_extension_et_entete() {
        let mut gb = vec![0u8; 0x8000];
        gb[0x104..0x108].copy_from_slice(&LOGO_GB);
        assert_eq!(console_de("a.gb", &gb), Some("gb"));
        assert_eq!(console_de("a.bin", &gb), Some("gb"));
        gb[0x143] = 0xC0;
        assert_eq!(console_de("a.GB", &gb), Some("gbc"));
        let mut gba = vec![0u8; 0x200];
        gba[0xB2] = 0x96;
        assert_eq!(console_de("x", &gba), Some("gba"));
        assert_eq!(console_de("x.gba", &[]), Some("gba"));
        assert_eq!(console_de("x.txt", &[1, 2, 3]), None);
    }

    #[test]
    fn rom_malformee_sans_panique() {
        // Natif : JsError n'est pas constructible hors wasm, on teste donc les fonctions pures.
        assert!(console_de("vide.gb", &[]).is_some());
        assert_eq!(titre_entete("gb", &[]), "");
        assert!(GameBoy::new(&[]).is_err());
        assert!(Gba::new(&[], None).is_err());
    }
}
