//! Bibliothèque : analyse des dossiers (fichiers en vrac, .zip, .7z), index en cache, lecture des ROM.
//! Les archives sont lues en mémoire, jamais décompressées sur disque.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

/// Taille maximale acceptée pour une ROM (GBA : 32 Mo).
const TAILLE_MAX: u64 = 64 << 20;
/// Version du format d'index ; une autre valeur force une nouvelle analyse.
const VERSION_INDEX: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Plateforme {
    Gb,
    Gbc,
    Gba,
}

impl Plateforme {
    pub fn court(self) -> &'static str {
        match self {
            Plateforme::Gb => "GB",
            Plateforme::Gbc => "GBC",
            Plateforme::Gba => "GBA",
        }
    }

    pub fn nom(self) -> &'static str {
        match self {
            Plateforme::Gb => "Game Boy",
            Plateforme::Gbc => "Game Boy Color",
            Plateforme::Gba => "Game Boy Advance",
        }
    }

    pub fn jouable(self) -> bool {
        self != Plateforme::Gba
    }
}

/// Une ROM trouvée : fichier en vrac ou entrée d'archive.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Rom {
    pub fichier: PathBuf,
    /// Nom de l'entrée dans l'archive (`None` pour un fichier en vrac).
    pub interne: Option<String>,
    pub taille: u64,
    pub crc: u32,
    pub plateforme: Plateforme,
    pub titre_entete: String,
}

impl Rom {
    /// Nom de fichier sans extension (entrée d'archive ou fichier).
    pub fn nom_fichier(&self) -> String {
        let nom = match &self.interne {
            Some(i) => Path::new(i)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned()),
            None => self
                .fichier
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned()),
        };
        nom.unwrap_or_default()
    }
}

#[derive(Serialize, Deserialize)]
struct Conteneur {
    fichier: PathBuf,
    taille: u64,
    mtime: u64,
    roms: Vec<Rom>,
}

#[derive(Serialize, Deserialize, Default)]
struct Index {
    version: u32,
    conteneurs: Vec<Conteneur>,
}

pub enum MsgScan {
    /// Contenu de l'index en cache, envoyé avant toute lecture de disque.
    Cache(Vec<Rom>),
    Progres {
        faits: usize,
        total: usize,
    },
    Fini(Vec<Rom>),
}

#[derive(PartialEq, Clone, Copy)]
enum Genre {
    Rom,
    Zip,
    SeptZ,
}

fn genre(chemin: &Path) -> Option<Genre> {
    let ext = chemin.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "gb" | "gbc" | "gba" => Some(Genre::Rom),
        "zip" => Some(Genre::Zip),
        "7z" => Some(Genre::SeptZ),
        _ => None,
    }
}

fn ext_rom(nom: &str) -> Option<&'static str> {
    let ext = Path::new(nom).extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "gb" => Some("gb"),
        "gbc" => Some("gbc"),
        "gba" => Some("gba"),
        _ => None,
    }
}

/// Plateforme d'après l'extension et l'en-tête (un .gb « CGB seulement » est classé GBC).
fn plateforme(ext: &str, entete: &[u8]) -> Plateforme {
    match ext {
        "gba" => Plateforme::Gba,
        "gbc" => Plateforme::Gbc,
        _ if entete.get(0x143) == Some(&0xC0) => Plateforme::Gbc,
        _ => Plateforme::Gb,
    }
}

/// Titre lu dans l'en-tête de la cartouche (ASCII imprimable seulement).
fn titre_entete(p: Plateforme, entete: &[u8]) -> String {
    let plage = match p {
        Plateforme::Gba => 0xA0..0xAC,
        _ if entete.get(0x143).is_some_and(|f| f & 0x80 != 0) => 0x134..0x13F,
        _ => 0x134..0x144,
    };
    let octets = entete.get(plage).unwrap_or(&[]);
    octets
        .iter()
        .take_while(|&&b| b != 0)
        .filter(|b| b.is_ascii_graphic() || **b == b' ')
        .map(|&b| b as char)
        .collect::<String>()
        .trim()
        .to_string()
}

fn mtime(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

/// Parcourt les sources (dossiers ou fichiers) et liste les fichiers candidats.
fn parcourir(sources: &[PathBuf], sortie: &mut Vec<(PathBuf, u64, u64)>) {
    let mut pile: Vec<(PathBuf, u32)> = sources.iter().map(|s| (s.clone(), 0)).collect();
    while let Some((chemin, prof)) = pile.pop() {
        let Ok(meta) = std::fs::metadata(&chemin) else {
            continue;
        };
        if meta.is_dir() {
            // Garde-fou contre les liens circulaires et les arborescences démesurées.
            if prof > 12 {
                continue;
            }
            let Ok(lecture) = std::fs::read_dir(&chemin) else {
                continue;
            };
            for e in lecture.flatten() {
                let nom = e.file_name();
                if nom.to_string_lossy().starts_with('.') {
                    continue;
                }
                pile.push((e.path(), prof + 1));
            }
        } else if genre(&chemin).is_some() && meta.len() <= TAILLE_MAX * 4 {
            sortie.push((chemin, meta.len(), mtime(&meta)));
        }
    }
}

fn rom_de(fichier: &Path, interne: Option<String>, ext: &str, donnees: &[u8], crc: u32) -> Rom {
    let p = plateforme(ext, donnees);
    Rom {
        fichier: fichier.to_path_buf(),
        interne,
        taille: donnees.len() as u64,
        crc,
        plateforme: p,
        titre_entete: titre_entete(p, donnees),
    }
}

/// Analyse un fichier : une ROM en vrac ou toutes les ROM d'une archive. Jamais de panique.
fn analyser(fichier: &Path) -> Vec<Rom> {
    match genre(fichier) {
        Some(Genre::Rom) => {
            let Some(ext) = ext_rom(&fichier.to_string_lossy()) else {
                return Vec::new();
            };
            match std::fs::read(fichier) {
                Ok(d) if d.len() as u64 <= TAILLE_MAX && !d.is_empty() => {
                    vec![rom_de(fichier, None, ext, &d, crc32fast::hash(&d))]
                }
                _ => Vec::new(),
            }
        }
        Some(Genre::Zip) => analyser_zip(fichier).unwrap_or_default(),
        Some(Genre::SeptZ) => analyser_7z(fichier).unwrap_or_default(),
        None => Vec::new(),
    }
}

fn analyser_zip(fichier: &Path) -> Option<Vec<Rom>> {
    let mut zip = zip::ZipArchive::new(File::open(fichier).ok()?).ok()?;
    let mut roms = Vec::new();
    for i in 0..zip.len() {
        let Ok(mut e) = zip.by_index(i) else {
            continue;
        };
        let Ok(nom) = e.name().map(|n| n.to_string()) else {
            continue;
        };
        let Some(ext) = ext_rom(&nom) else {
            continue;
        };
        if e.is_dir() || e.size() > TAILLE_MAX || e.size() == 0 {
            continue;
        }
        // Le CRC est dans le répertoire central : seul l'en-tête est décompressé.
        let mut entete = Vec::with_capacity(0x150);
        if (&mut e).take(0x150).read_to_end(&mut entete).is_err() {
            continue;
        }
        let p = plateforme(ext, &entete);
        roms.push(Rom {
            fichier: fichier.to_path_buf(),
            interne: Some(nom),
            taille: e.size(),
            crc: e.crc32(),
            plateforme: p,
            titre_entete: titre_entete(p, &entete),
        });
    }
    Some(roms)
}

fn analyser_7z(fichier: &Path) -> Option<Vec<Rom>> {
    use sevenz_rust2::{ArchiveReader, Password};
    let mut lecteur = ArchiveReader::new(File::open(fichier).ok()?, Password::empty()).ok()?;
    let mut roms = Vec::new();
    // raccourci : décompresse chaque ROM entière pour lire l'en-tête (archives solides), suffisant
    // car l'index en cache évite de recommencer ; à affiner si de grosses collections .7z rament.
    let _ = lecteur.for_each_entries(|e, r| {
        let Some(ext) = ext_rom(&e.name) else {
            return Ok(true);
        };
        if e.is_directory || e.size > TAILLE_MAX || e.size == 0 {
            return Ok(true);
        }
        let mut d = Vec::with_capacity(e.size as usize);
        if r.read_to_end(&mut d).is_err() {
            return Ok(true);
        }
        let crc = if e.has_crc {
            e.crc as u32
        } else {
            crc32fast::hash(&d)
        };
        roms.push(rom_de(fichier, Some(e.name.clone()), ext, &d, crc));
        Ok(true)
    });
    Some(roms)
}

/// Lit les octets d'une ROM (fichier, entrée .zip ou .7z), en mémoire.
pub fn lire_rom(rom: &Rom) -> io::Result<Vec<u8>> {
    let invalide = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_string());
    match &rom.interne {
        None => {
            let d = std::fs::read(&rom.fichier)?;
            if d.len() as u64 > TAILLE_MAX {
                return Err(invalide("fichier trop gros"));
            }
            Ok(d)
        }
        Some(nom) => match genre(&rom.fichier) {
            Some(Genre::Zip) => {
                let mut zip = zip::ZipArchive::new(File::open(&rom.fichier)?)
                    .map_err(|_| invalide("archive zip illisible"))?;
                let e = zip
                    .by_name(nom)
                    .map_err(|_| invalide("entrée absente de l'archive"))?;
                let mut d = Vec::new();
                e.take(TAILLE_MAX).read_to_end(&mut d)?;
                Ok(d)
            }
            Some(Genre::SeptZ) => {
                use sevenz_rust2::{ArchiveReader, Password};
                let mut l = ArchiveReader::new(File::open(&rom.fichier)?, Password::empty())
                    .map_err(|_| invalide("archive 7z illisible"))?;
                l.read_file(nom)
                    .map_err(|_| invalide("entrée 7z illisible"))
            }
            _ => Err(invalide("format inconnu")),
        },
    }
}

fn charger_index(chemin: &Path) -> Index {
    std::fs::read(chemin)
        .ok()
        .and_then(|d| serde_json::from_slice::<Index>(&d).ok())
        .filter(|i| i.version == VERSION_INDEX)
        .unwrap_or_default()
}

/// Analyse en arrière-plan : envoie d'abord le cache, puis le résultat complet.
/// Seuls les fichiers nouveaux ou modifiés (taille, date) sont relus.
pub fn scanner(
    sources: Vec<PathBuf>,
    chemin_index: PathBuf,
    envoyer_cache: bool,
    tx: Sender<MsgScan>,
    reveil: impl Fn() + Send + Sync,
) {
    let index = charger_index(&chemin_index);
    if envoyer_cache {
        let roms: Vec<Rom> = index
            .conteneurs
            .iter()
            .flat_map(|c| c.roms.iter().cloned())
            .collect();
        let _ = tx.send(MsgScan::Cache(roms));
        reveil();
    }
    let mut anciens: HashMap<PathBuf, Conteneur> = index
        .conteneurs
        .into_iter()
        .map(|c| (c.fichier.clone(), c))
        .collect();

    let mut fichiers = Vec::new();
    parcourir(&sources, &mut fichiers);
    fichiers.sort();
    fichiers.dedup_by(|a, b| a.0 == b.0);

    let mut resultat: Vec<Option<Conteneur>> = Vec::with_capacity(fichiers.len());
    let mut a_faire = Vec::new();
    for (i, (f, taille, mt)) in fichiers.iter().enumerate() {
        match anciens.remove(f) {
            Some(c) if c.taille == *taille && c.mtime == *mt => resultat.push(Some(c)),
            _ => {
                resultat.push(None);
                a_faire.push(i);
            }
        }
    }

    let total = a_faire.len();
    if total > 0 {
        let suivant = AtomicUsize::new(0);
        let faits = AtomicUsize::new(0);
        let sortie = Mutex::new(Vec::with_capacity(total));
        let ouvriers = std::thread::available_parallelism()
            .map_or(2, |n| n.get())
            .clamp(1, 6);
        std::thread::scope(|s| {
            for _ in 0..ouvriers {
                s.spawn(|| loop {
                    let k = suivant.fetch_add(1, Ordering::Relaxed);
                    let Some(&i) = a_faire.get(k) else {
                        break;
                    };
                    let (f, taille, mt) = &fichiers[i];
                    let c = Conteneur {
                        fichier: f.clone(),
                        taille: *taille,
                        mtime: *mt,
                        roms: analyser(f),
                    };
                    if let Ok(mut v) = sortie.lock() {
                        v.push((i, c));
                    }
                    let n = faits.fetch_add(1, Ordering::Relaxed) + 1;
                    if n.is_multiple_of(64) {
                        let _ = tx.send(MsgScan::Progres { faits: n, total });
                        reveil();
                    }
                });
            }
        });
        for (i, c) in sortie.into_inner().unwrap_or_default() {
            resultat[i] = Some(c);
        }
    }

    let conteneurs: Vec<Conteneur> = resultat.into_iter().flatten().collect();
    let roms: Vec<Rom> = conteneurs
        .iter()
        .flat_map(|c| c.roms.iter().cloned())
        .collect();
    let index = Index {
        version: VERSION_INDEX,
        conteneurs,
    };
    if let Ok(json) = serde_json::to_vec(&index) {
        let _ = crate::sauve::ecrire_atomique(&chemin_index, &json);
    }
    let _ = tx.send(MsgScan::Fini(roms));
    reveil();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entete_et_plateforme() {
        let mut d = vec![0u8; 0x150];
        d[0x134..0x13A].copy_from_slice(b"TETRIX");
        assert_eq!(plateforme("gb", &d), Plateforme::Gb);
        assert_eq!(titre_entete(Plateforme::Gb, &d), "TETRIX");
        d[0x143] = 0xC0;
        assert_eq!(plateforme("gb", &d), Plateforme::Gbc);
        // En-tête tronqué : pas de panique.
        assert_eq!(titre_entete(Plateforme::Gb, &d[..0x20]), "");
        assert_eq!(plateforme("gba", &[]), Plateforme::Gba);
        assert!(Plateforme::Gb.jouable() && Plateforme::Gbc.jouable());
        assert!(!Plateforme::Gba.jouable());
    }

    #[test]
    fn zip_en_memoire() {
        let dossier = std::env::temp_dir().join(format!("gbemu-test-zip-{}", std::process::id()));
        std::fs::create_dir_all(&dossier).unwrap();
        let chemin = dossier.join("a.zip");
        let mut rom = vec![0u8; 0x8000];
        rom[0x134..0x138].copy_from_slice(b"DEMO");
        {
            let f = File::create(&chemin).unwrap();
            let mut w = zip::ZipWriter::new(f);
            let o = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            w.start_file("jeu.gb", o).unwrap();
            std::io::Write::write_all(&mut w, &rom).unwrap();
            w.start_file("lisezmoi.txt", o).unwrap();
            w.finish().unwrap();
        }
        let roms = analyser(&chemin);
        assert_eq!(roms.len(), 1);
        assert_eq!(roms[0].crc, crc32fast::hash(&rom));
        assert_eq!(roms[0].titre_entete, "DEMO");
        assert_eq!(lire_rom(&roms[0]).unwrap(), rom);
        let _ = std::fs::remove_file(&chemin);
        let _ = std::fs::remove_dir(&dossier);
    }
}
