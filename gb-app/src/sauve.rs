//! Emplacements, réglages et écritures sur disque (atomiques : jamais de fichier à moitié écrit).

use crate::biblio::Rom;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, SystemTime};

/// Mode hors ligne en vigueur (copie de `Config::hors_ligne`, lue par les fils de téléchargement).
static HORS_LIGNE: AtomicBool = AtomicBool::new(false);

/// Vrai si aucun téléchargement (DAT, jaquettes) ne doit être fait.
pub fn hors_ligne() -> bool {
    HORS_LIGNE.load(Ordering::Relaxed)
}

/// Valeur illisible (variante inconnue, mauvais type) : valeur par défaut, le reste est gardé.
pub fn ou_defaut<'de, D: Deserializer<'de>, T: DeserializeOwned + Default>(
    d: D,
) -> Result<T, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(T::deserialize(v).unwrap_or_default())
}

/// Table dont seules les entrées illisibles sont écartées.
pub fn carte_tolerante<'de, D, K, V, C>(d: D) -> Result<C, D::Error>
where
    D: Deserializer<'de>,
    K: DeserializeOwned + Ord,
    V: DeserializeOwned,
    C: FromIterator<(K, V)>,
{
    let mut c = Vec::new();
    if let Ok(serde_json::Value::Object(m)) = serde_json::Value::deserialize(d) {
        for (k, v) in m {
            let une = serde_json::Value::Object([(k, v)].into_iter().collect());
            if let Ok(e) = serde_json::from_value::<BTreeMap<K, V>>(une) {
                c.extend(e);
            }
        }
    }
    Ok(c.into_iter().collect())
}

/// Chemin non UTF-8 (Linux) : impossible en JSON, il est omis plutôt que de tout bloquer.
fn chemin_option<S: Serializer>(c: &Option<PathBuf>, s: S) -> Result<S::Ok, S::Error> {
    c.as_ref().filter(|c| c.to_str().is_some()).serialize(s)
}

fn chemins_utf8<S: Serializer>(c: &[PathBuf], s: S) -> Result<S::Ok, S::Error> {
    s.collect_seq(c.iter().filter(|c| c.to_str().is_some()))
}

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
    #[serde(deserialize_with = "ou_defaut", serialize_with = "chemin_option")]
    pub dossier_jeux: Option<PathBuf>,
    /// Dossiers et fichiers ajoutés par glisser-déposer.
    #[serde(deserialize_with = "ou_defaut", serialize_with = "chemins_utf8")]
    pub ajouts: Vec<PathBuf>,
    #[serde(deserialize_with = "ou_defaut")]
    pub palette: Palette,
    #[serde(deserialize_with = "ou_defaut")]
    pub grille: bool,
    /// Thème Jour (sinon Nuit).
    #[serde(deserialize_with = "ou_defaut")]
    pub jour: bool,
    /// « Réduire les animations ».
    #[serde(deserialize_with = "ou_defaut")]
    pub animations_reduites: bool,
    /// Sons d'interface coupés.
    #[serde(deserialize_with = "ou_defaut")]
    pub sons_coupes: bool,
    /// Écran de démarrage désactivé.
    #[serde(deserialize_with = "ou_defaut")]
    pub sans_intro: bool,
    /// Avance rapide (ou ralenti) appliquée par Tab.
    #[serde(deserialize_with = "ou_defaut")]
    pub vitesse: crate::jeu::Vitesse,
    /// Filtre de rémanence (mélange avec l'image précédente).
    #[serde(deserialize_with = "ou_defaut")]
    pub remanence: bool,
    /// Effet rétroéclairé (palette éclaircie, halo).
    #[serde(deserialize_with = "ou_defaut")]
    pub retroeclaire: bool,
    /// Correction des couleurs GBC désactivée (couleurs brutes, plus vives).
    #[serde(deserialize_with = "ou_defaut")]
    pub sans_correction: bool,
    /// Volume du jeu, 0 à 1 (`None` : 100 %).
    #[serde(deserialize_with = "ou_defaut")]
    pub volume: Option<f32>,
    #[serde(deserialize_with = "ou_defaut")]
    pub muet: bool,
    /// Palette choisie par jeu (CRC en hexadécimal).
    #[serde(deserialize_with = "carte_tolerante")]
    pub palettes_jeux: std::collections::BTreeMap<String, Palette>,
    /// Aucun téléchargement (noms des jeux, jaquettes).
    #[serde(deserialize_with = "ou_defaut")]
    pub hors_ligne: bool,
}

pub struct Chemins {
    pub jeux_defaut: PathBuf,
    pub donnees: PathBuf,
    /// Fichiers recréables (index, DAT, jaquettes). Hors mode portable sous Windows :
    /// `%LOCALAPPDATA%` (non itinérant) ; sinon le dossier des données.
    pub cache: PathBuf,
}

/// Fichiers et dossiers du cache (déplacés une fois si le cache change d'emplacement).
const CACHE: [&str; 3] = ["index.json", "dat", "jaquettes"];

impl Chemins {
    /// Mode portable : tout à côté de l'exécutable ; sinon dossier de données de l'utilisateur.
    pub fn trouver(donnees_force: Option<PathBuf>) -> Chemins {
        let base = std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        let portable = base.join("gbemu-donnees");
        let (donnees, cache) = match donnees_force {
            Some(d) => (d.clone(), d),
            None if std::fs::create_dir_all(&portable).is_ok() && inscriptible(&portable) => {
                (portable.clone(), portable)
            }
            None => (dossier_utilisateur().join("gbemu"), dossier_cache()),
        };
        let _ = std::fs::create_dir_all(&donnees);
        if cache != donnees && std::fs::create_dir_all(&cache).is_ok() {
            for f in CACHE {
                if !cache.join(f).exists() {
                    let _ = std::fs::rename(donnees.join(f), cache.join(f));
                }
            }
        }
        // Dossier de l'exécutable en lecture seule (Program Files) : jeux avec les données.
        let jeux = base.join("jeux");
        let jeux_defaut = if std::fs::create_dir_all(&jeux).is_ok() {
            jeux
        } else {
            donnees.join("jeux")
        };
        Chemins {
            jeux_defaut,
            donnees,
            cache,
        }
    }

    pub fn config(&self) -> PathBuf {
        self.donnees.join("reglages.json")
    }

    pub fn index(&self) -> PathBuf {
        self.cache.join("index.json")
    }

    pub fn dat(&self) -> PathBuf {
        self.cache.join("dat")
    }

    pub fn jaquettes(&self) -> PathBuf {
        self.cache.join("jaquettes")
    }

    /// Sauvegarde batterie d'une ROM : nom lisible + CRC. ROM renommée : la sauvegarde du même
    /// CRC est reprise sous le nouveau nom.
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
        let dossier = self.donnees.join("sauvegardes");
        let chemin = dossier.join(format!("{} [{:08X}].sav", nom.trim(), rom.crc));
        if !chemin.exists() && !chemin.with_extension("bak").exists() {
            migrer(&dossier, &chemin, &format!(" [{:08X}].sav", rom.crc), false);
        }
        chemin
    }

    /// Dossier des états d'une ROM (même nom que la sauvegarde batterie, repris de même).
    pub fn etats(&self, rom: &Rom) -> PathBuf {
        let sav = self.sauvegarde(rom);
        let nom = sav.file_stem().unwrap_or_default().to_os_string();
        let dossier = self.donnees.join("etats");
        let chemin = dossier.join(nom);
        if !chemin.exists() {
            migrer(&dossier, &chemin, &format!(" [{:08X}]", rom.crc), true);
        }
        chemin
    }

    pub fn captures(&self) -> PathBuf {
        self.donnees.join("captures")
    }
}

/// ROM renommée : le fichier (ou dossier) le plus récent qui finit par `suffixe` (CRC) prend le
/// nom `cible`, avec sa copie `.bak`.
fn migrer(dossier: &Path, cible: &Path, suffixe: &str, dossiers: bool) {
    let Ok(lecture) = std::fs::read_dir(dossier) else {
        return;
    };
    let ancien = lecture
        .flatten()
        .filter(|e| e.file_name().to_str().is_some_and(|n| n.ends_with(suffixe)))
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir() == dossiers))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .max();
    if let Some((_, a)) = ancien {
        if std::fs::rename(&a, cible).is_ok() && !dossiers {
            let _ = std::fs::rename(a.with_extension("bak"), cible.with_extension("bak"));
        }
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

fn dossier_cache() -> PathBuf {
    #[cfg(windows)]
    if let Some(d) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(d).join("gbemu");
    }
    dossier_utilisateur().join("gbemu")
}

/// Une autre instance utilise déjà ce dossier de données.
#[derive(Debug)]
pub struct DejaOuvert;

/// Verrou d'instance unique (fichier `verrou` des données), tenu tant que le fichier rendu reste
/// ouvert. Verrou impossible à poser (disque en lecture seule…) : `Ok(None)`, on continue sans.
pub fn verrou_instance(donnees: &Path) -> Result<Option<std::fs::File>, DejaOuvert> {
    let Ok(f) = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(donnees.join("verrou"))
    else {
        return Ok(None);
    };
    match f.try_lock() {
        Ok(()) => Ok(Some(f)),
        Err(std::fs::TryLockError::WouldBlock) => Err(DejaOuvert),
        Err(_) => Ok(None),
    }
}

#[cfg(test)]
thread_local! {
    /// Tests : simule un remplacement refusé (antivirus qui garde le fichier ouvert).
    static ECHEC_REMPLACEMENT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn refuse_en_test() -> io::Result<()> {
    #[cfg(test)]
    if ECHEC_REMPLACEMENT.with(|e| e.get()) {
        return Err(io::Error::other("remplacement refusé (test)"));
    }
    Ok(())
}

/// Écriture atomique : fichier temporaire (nom unique) synchronisé puis renommé par-dessus
/// l'original, qui ne manque donc jamais. La version précédente est copiée en `.bak`.
/// Remplacement refusé (antivirus, indexeur, OneDrive…) : 5 réessais, puis réécriture directe ;
/// en dernier recours le `.tmp` complet reste et [`lire`] le reprend.
pub fn ecrire_atomique(chemin: &Path, donnees: &[u8]) -> io::Result<()> {
    static COMPTEUR: AtomicU32 = AtomicU32::new(0);
    if let Some(p) = chemin.parent() {
        std::fs::create_dir_all(p)?;
    }
    let mut nom = chemin.file_name().unwrap_or_default().to_os_string();
    nom.push(format!(
        ".{}-{}.tmp",
        std::process::id(),
        COMPTEUR.fetch_add(1, Ordering::Relaxed)
    ));
    let tmp = chemin.with_file_name(nom);
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(donnees)?;
        f.sync_all()?;
    }
    if chemin.exists() {
        // Copie et non renommage : le fichier principal reste en place jusqu'au remplacement.
        let _ = std::fs::copy(chemin, chemin.with_extension("bak"));
    }
    let remplacer = || refuse_en_test().and_then(|()| std::fs::rename(&tmp, chemin));
    let mut r = remplacer();
    for _ in 0..5 {
        if r.is_ok() {
            return r;
        }
        std::thread::sleep(Duration::from_millis(50));
        r = remplacer();
    }
    if r.is_err() {
        let direct = refuse_en_test().and_then(|()| {
            let mut f = std::fs::File::create(chemin)?;
            f.write_all(donnees)?;
            f.sync_all()
        });
        if direct.is_ok() {
            let _ = std::fs::remove_file(&tmp);
            return direct;
        }
    }
    r
}

/// `.tmp` restés à côté de `chemin` (remplacement refusé), ancien nom fixe compris.
fn temporaires(chemin: &Path) -> Vec<PathBuf> {
    let mut v = vec![chemin.with_extension("tmp")];
    let mut prefixe = chemin
        .file_name()
        .unwrap_or_default()
        .as_encoded_bytes()
        .to_vec();
    prefixe.push(b'.');
    let dossier = chemin.parent().unwrap_or(Path::new("."));
    if let Ok(lecture) = std::fs::read_dir(dossier) {
        v.extend(
            lecture
                .flatten()
                .filter(|e| {
                    let n = e.file_name();
                    let n = n.as_encoded_bytes();
                    n.starts_with(&prefixe) && n.ends_with(b".tmp")
                })
                .map(|e| e.path()),
        );
    }
    v
}

/// Lit un fichier écrit par [`ecrire_atomique`] : le principal s'il passe `valide`, sauf si un
/// `.tmp` valide plus récent est resté (remplacement refusé) ; principal absent ou invalide :
/// le plus récent valide parmi `.tmp` et `.bak`.
pub fn lire_valide(chemin: &Path, valide: impl Fn(&[u8]) -> bool) -> Option<Vec<u8>> {
    let date = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let lu = |p: &Path| std::fs::read(p).ok().filter(|d| valide(d));
    let principal = lu(chemin);
    let reference = principal.as_ref().and_then(|_| date(chemin));
    let mut candidats = temporaires(chemin);
    if principal.is_none() {
        candidats.push(chemin.with_extension("bak"));
    }
    let mut meilleur: Option<(SystemTime, Vec<u8>)> = None;
    for c in candidats {
        let Some(t) = date(&c) else { continue };
        if reference.is_some_and(|r| t <= r) || meilleur.as_ref().is_some_and(|m| t <= m.0) {
            continue;
        }
        if let Some(d) = lu(&c) {
            meilleur = Some((t, d));
        }
    }
    meilleur.map(|m| m.1).or(principal)
}

/// Lecture brute (sauvegardes batterie) : un `.tmp` n'est repris que s'il a la taille du
/// principal ou du `.bak` (taille fixe par cartouche ; un `.tmp` tronqué est écarté).
pub fn lire(chemin: &Path) -> Option<Vec<u8>> {
    let taille = [chemin.to_path_buf(), chemin.with_extension("bak")]
        .iter()
        .find_map(|p| std::fs::metadata(p).ok().map(|m| m.len()));
    lire_valide(chemin, |d| taille.is_none_or(|t| d.len() as u64 == t))
}

/// Lecture d'un JSON : un fichier illisible est remplacé par sa copie `.bak` (ou `.tmp`).
pub fn lire_json<T: DeserializeOwned>(chemin: &Path) -> Option<T> {
    let d = lire_valide(chemin, |d| serde_json::from_slice::<T>(d).is_ok())?;
    serde_json::from_slice(&d).ok()
}

/// Copie datée de `chemin` (sauvegarde batterie) dans `historique/` à côté, sauf si elle est
/// identique à la dernière ; seules les `garder` plus récentes restent.
pub fn historiser(chemin: &Path, garder: usize) -> io::Result<()> {
    let Some(d) = lire(chemin) else {
        return Ok(());
    };
    let dossier = chemin.with_file_name("historique");
    let prefixe = format!(
        "{} ",
        chemin.file_stem().unwrap_or_default().to_string_lossy()
    );
    let mut t = SystemTime::now();
    let longueur = horodatage(t).len();
    let mut anciens: Vec<PathBuf> = std::fs::read_dir(&dossier)
        .map(|l| {
            l.flatten()
                .filter(|e| {
                    e.file_name().to_str().is_some_and(|n| {
                        n.starts_with(&prefixe)
                            && n.ends_with(".sav")
                            && n.len() == prefixe.len() + longueur + 4
                    })
                })
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default();
    anciens.sort();
    if let Some(dernier) = anciens.last() {
        if std::fs::read(dernier).ok().as_deref() == Some(&d[..]) {
            return Ok(());
        }
    }
    let nom = |t| dossier.join(format!("{prefixe}{}.sav", horodatage(t)));
    let mut nouveau = nom(t);
    while nouveau.exists() {
        t += Duration::from_millis(1);
        nouveau = nom(t);
    }
    ecrire_atomique(&nouveau, &d)?;
    anciens.push(nouveau);
    for vieux in &anciens[..anciens.len().saturating_sub(garder)] {
        let _ = std::fs::remove_file(vieux);
    }
    Ok(())
}

/// « 2026-11-22_14-03-09-250 » (UTC, millisecondes) : triable, valide comme nom de fichier
/// sous Windows.
pub fn horodatage(t: SystemTime) -> String {
    let duree = t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    let s = duree.as_secs();
    let (j, r) = ((s / 86_400) as i64, s % 86_400);
    // Date civile depuis le nombre de jours (algorithme « civil_from_days » de H. Hinnant).
    let z = j + 719_468;
    let ere = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let jour = doy - (153 * mp + 2) / 5 + 1;
    let mois = if mp < 10 { mp + 3 } else { mp - 9 };
    let an = yoe + ere * 400 + i64::from(mois <= 2);
    format!(
        "{an:04}-{mois:02}-{jour:02}_{:02}-{:02}-{:02}-{:03}",
        r / 3600,
        r / 60 % 60,
        r % 60,
        duree.subsec_millis()
    )
}

impl Config {
    /// Une valeur illisible garde sa valeur par défaut ; un fichier illisible, sa copie `.bak`.
    pub fn charger(chemin: &Path) -> Config {
        let c: Config = lire_json(chemin).unwrap_or_default();
        HORS_LIGNE.store(c.hors_ligne, Ordering::Relaxed);
        c
    }

    /// Enregistre et applique le mode hors ligne (lu par les fils de téléchargement).
    pub fn enregistrer(&self, chemin: &Path) {
        HORS_LIGNE.store(self.hors_ligne, Ordering::Relaxed);
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let _ = ecrire_atomique(chemin, &json);
        }
    }
}

/// Tests : dossier temporaire unique, supprimé à la fin du test.
#[cfg(test)]
pub struct DossierTest(pub PathBuf);

#[cfg(test)]
impl DossierTest {
    pub fn new(nom: &str) -> DossierTest {
        static N: AtomicU32 = AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!(
            "gbemu-test-{nom}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        DossierTest(d)
    }
}

#[cfg(test)]
impl Drop for DossierTest {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn restes_tmp(d: &Path) -> usize {
        std::fs::read_dir(d)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .count()
    }

    #[test]
    fn ecriture_atomique_garde_l_ancienne() {
        let d = DossierTest::new("sauve");
        let f = d.0.join("x.sav");
        ecrire_atomique(&f, b"un").unwrap();
        ecrire_atomique(&f, b"deux").unwrap();
        assert_eq!(lire(&f).unwrap(), b"deux");
        assert_eq!(std::fs::read(f.with_extension("bak")).unwrap(), b"un");
        assert_eq!(restes_tmp(&d.0), 0);
    }

    #[test]
    fn remplacement_refuse_relit_la_plus_recente() {
        let d = DossierTest::new("refus");
        let f = d.0.join("x.sav");
        ecrire_atomique(&f, b"un").unwrap();
        ECHEC_REMPLACEMENT.with(|e| e.set(true));
        assert!(ecrire_atomique(&f, b"de").is_err());
        ECHEC_REMPLACEMENT.with(|e| e.set(false));
        // Le principal n'a jamais manqué ; la version récente est reprise du `.tmp`.
        assert_eq!(std::fs::read(&f).unwrap(), b"un");
        assert_eq!(restes_tmp(&d.0), 1);
        assert_eq!(lire(&f).unwrap(), b"de");
        // Écriture suivante réussie : elle gagne, le vieux `.tmp` est ignoré.
        ecrire_atomique(&f, b"tr").unwrap();
        assert_eq!(lire(&f).unwrap(), b"tr");
        // `.tmp` tronqué (taille différente) : écarté.
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(d.0.join("x.sav.1-1.tmp"), b"t").unwrap();
        assert_eq!(lire(&f).unwrap(), b"tr");
    }

    #[test]
    fn json_illisible_relit_le_bak() {
        let d = DossierTest::new("json");
        let f = d.0.join("reglages.json");
        let c = Config {
            jour: true,
            ..Config::default()
        };
        c.enregistrer(&f);
        c.enregistrer(&f);
        std::fs::write(&f, b"{abime").unwrap();
        assert!(Config::charger(&f).jour);
        // Principal manquant : idem.
        let _ = std::fs::remove_file(&f);
        assert!(Config::charger(&f).jour);
    }

    #[test]
    fn valeur_inconnue_garde_le_reste() {
        let d = DossierTest::new("inconnue");
        let f = d.0.join("reglages.json");
        std::fs::write(
            &f,
            br#"{"jour":true,"palette":"Rouge","grille":"oui","vitesse":"X8","dossier_jeux":"/j",
               "palettes_jeux":{"0000000A":"Sepia","0000000B":"Rouge"},"hors_ligne":true}"#,
        )
        .unwrap();
        let c = Config::charger(&f);
        assert!(c.jour && c.hors_ligne && !c.grille);
        assert_eq!(c.palette, Palette::Verte);
        assert_eq!(c.vitesse, crate::jeu::Vitesse::X8);
        assert_eq!(c.dossier_jeux, Some(PathBuf::from("/j")));
        assert_eq!(c.palettes_jeux.len(), 1);
        assert_eq!(c.palettes_jeux["0000000A"], Palette::Sepia);
        assert!(hors_ligne());
        Config::default().enregistrer(&f);
        assert!(!hors_ligne());
    }

    #[cfg(unix)]
    #[test]
    fn chemin_non_utf8_n_empeche_pas_l_enregistrement() {
        use std::os::unix::ffi::OsStrExt;
        let d = DossierTest::new("utf8");
        let f = d.0.join("reglages.json");
        let mauvais = PathBuf::from(std::ffi::OsStr::from_bytes(b"/jeux/caf\xe9"));
        let c = Config {
            jour: true,
            dossier_jeux: Some(mauvais.clone()),
            ajouts: vec![mauvais, PathBuf::from("/ok")],
            ..Config::default()
        };
        c.enregistrer(&f);
        let r = Config::charger(&f);
        assert!(r.jour && r.dossier_jeux.is_none());
        assert_eq!(r.ajouts, vec![PathBuf::from("/ok")]);
    }

    #[test]
    fn historique_garde_les_dernieres() {
        let d = DossierTest::new("historique");
        let f = d.0.join("Jeu [0000000A].sav");
        let h = d.0.join("historique");
        for (i, date) in ["2026-01-0", "2026-01-1"].iter().enumerate() {
            for k in 0..4 {
                let n = h.join(format!("Jeu [0000000A] {date}{k}_00-00-00-000.sav"));
                ecrire_atomique(&n, &[i as u8, k]).unwrap();
            }
        }
        // Autre jeu : jamais touché.
        let autre = h.join("Autre [0000000B] 2020-01-01_00-00-00-000.sav");
        ecrire_atomique(&autre, b"a").unwrap();
        ecrire_atomique(&f, b"courante").unwrap();
        historiser(&f, 5).unwrap();
        historiser(&f, 5).unwrap();
        let mut noms: Vec<String> = std::fs::read_dir(&h)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("Jeu "))
            .collect();
        noms.sort();
        assert_eq!(noms.len(), 5);
        assert!(noms[0].contains("2026-01-10"));
        assert_eq!(std::fs::read(h.join(&noms[4])).unwrap(), b"courante");
        assert!(autre.exists());
    }

    #[test]
    fn sauvegarde_retrouvee_apres_renommage() {
        let d = DossierTest::new("renommage");
        let c = Chemins {
            jeux_defaut: d.0.join("jeux"),
            donnees: d.0.clone(),
            cache: d.0.clone(),
        };
        let rom = |nom: &str| Rom {
            fichier: PathBuf::from(format!("/jeux/{nom}.gb")),
            interne: None,
            taille: 0,
            crc: 0xABCD,
            plateforme: crate::biblio::Plateforme::Gb,
            titre_entete: String::new(),
        };
        let avant = c.sauvegarde(&rom("Ancien"));
        ecrire_atomique(&avant, b"partie").unwrap();
        ecrire_atomique(&avant, b"partie2").unwrap();
        std::fs::create_dir_all(c.etats(&rom("Ancien")).join("x")).unwrap();
        let apres = c.sauvegarde(&rom("Nouveau (France)"));
        assert_eq!(lire(&apres).unwrap(), b"partie2");
        assert_eq!(
            std::fs::read(apres.with_extension("bak")).unwrap(),
            b"partie"
        );
        assert!(!avant.exists());
        assert!(c.etats(&rom("Nouveau (France)")).join("x").is_dir());
        // Un autre CRC n'est pas repris.
        let autre = Rom {
            crc: 1,
            ..rom("Ancien")
        };
        assert!(lire(&c.sauvegarde(&autre)).is_none());
    }

    #[test]
    fn deuxieme_instance_refusee() {
        let d = DossierTest::new("verrou");
        let v = verrou_instance(&d.0).unwrap();
        assert!(v.is_some());
        assert!(verrou_instance(&d.0).is_err());
        drop(v);
        assert!(verrou_instance(&d.0).is_ok_and(|v| v.is_some()));
    }

    #[test]
    fn horodatages() {
        let t = |s| SystemTime::UNIX_EPOCH + Duration::from_secs(s);
        assert_eq!(horodatage(t(0)), "1970-01-01_00-00-00-000");
        assert_eq!(horodatage(t(1_709_210_096)), "2024-02-29_12-34-56-000");
        assert_eq!(
            horodatage(t(1_798_761_599) + Duration::from_millis(7)),
            "2026-12-31_23-59-59-007"
        );
    }
}
