//! Robot de test (`gbemu --robot GRAINE`) : un utilisateur simulé, sans fenêtre, pilote
//! l'appli entière (bibliothèque, menus, parties GB/GBC/GBA, états, rembobinage, glisser-déposer,
//! fermeture et réouverture sur le même dossier de données) avec des gestes aléatoires tirés
//! d'une graine. Invariants vérifiés à chaque image : aucune panique, aucune image > 200 ms,
//! mémoire bornée, interface cohérente, son sans trou ni saturation, et à chaque fermeture :
//! réglages, touches, sauvegardes et états relus à l'identique.
//!
//! Réseau : toutes les requêtes passent par un mandataire local fermé (`dat::RESEAU_COUPE`) ;
//! `scripts/robot.py` lance en plus chaque session dans un espace réseau vide (`unshare -rn`).
//! Sortie : une ligne JSON (bilan) ; en cas d'échec, la trace des gestes est écrite pour être
//! rejouée (`--rejouer`) et réduite.

use crate::app::{App, Options, Sonde};
use crate::biblio;
use crate::manette::{Pad, BOUTONS};
use eframe::egui::{
    self, Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2, ViewportCommand,
    ViewportId, ViewportInfo,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Durée maximale d'une image d'interface (émulation, entrées et dessin compris).
const IMAGE_MAX_MS: f64 = 200.0;
/// Image bloquée : le chien de garde arrête la session.
const BLOCAGE_S: u64 = 10;
/// Mémoire résidente maximale du processus.
const MEMOIRE_MAX_MO: u64 = 1500;
/// Analyse de la bibliothèque : durée maximale (temps réel).
const ANALYSE_MAX_S: f64 = 30.0;
/// Son : délai avant de juger le tampon (secondes simulées de jeu continu).
const SON_STABLE_S: f64 = 3.0;
/// Échantillons par seconde dans le tampon (stéréo).
const ECH_S: f64 = crate::audio::FREQUENCE as f64 * 2.0;
/// Période de lecture de la carte son simulée (10 ms, stéréo).
const PERIODE_SON: f64 = ECH_S / 100.0;

// ----- Aléa reproductible -----

#[derive(Clone)]
struct Alea(u64);

impl Alea {
    fn new(graine: u64, flux: u64) -> Alea {
        let mut a = Alea(graine ^ flux.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        a.u64();
        a
    }

    /// splitmix64.
    fn u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn n(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.u64() % n as u64) as usize
        }
    }

    fn f(&mut self) -> f32 {
        (self.u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn entre(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }

    fn proba(&mut self, p: f32) -> bool {
        self.f() < p
    }

    fn choix<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.n(v.len())]
    }

    fn octets(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.u64() as u8).collect()
    }
}

// ----- Profils d'utilisateur -----

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum Profil {
    /// Machine rapide (144 Hz), gestes fréquents de tout genre.
    Rapide,
    /// Machine lente (30 images/s irrégulières), gestes espacés.
    Lent,
    /// Clavier seul.
    Clavier,
    /// Manette seule.
    Manette,
    /// Souris seule.
    Souris,
    /// Joue longtemps : touches du jeu tenues, états, avance rapide, rembobinage.
    Joueur,
    /// Rafales, fichiers abîmés, réouvertures, collages, redimensionnements.
    Casseur,
}

const PROFILS: [Profil; 7] = [
    Profil::Rapide,
    Profil::Lent,
    Profil::Clavier,
    Profil::Manette,
    Profil::Souris,
    Profil::Joueur,
    Profil::Casseur,
];

impl Profil {
    fn lire(s: &str) -> Option<Profil> {
        PROFILS
            .iter()
            .copied()
            .find(|p| format!("{p:?}").eq_ignore_ascii_case(s))
    }

    /// Durée d'image (s) : base et écart.
    fn cadence(self) -> (f64, f64) {
        match self {
            Profil::Rapide => (1.0 / 144.0, 0.0005),
            Profil::Lent => (1.0 / 30.0, 0.012),
            Profil::Casseur => (1.0 / 60.0, 0.008),
            _ => (1.0 / 60.0, 0.001),
        }
    }

    /// Images entre deux gestes (min, max).
    fn rythme(self) -> (usize, usize) {
        match self {
            Profil::Rapide => (2, 20),
            Profil::Lent => (20, 120),
            Profil::Casseur => (1, 12),
            Profil::Joueur => (10, 90),
            _ => (6, 50),
        }
    }

    /// Poids des genres de gestes (voir `Genre`).
    fn poids(self) -> [u32; GENRES] {
        //        nav jeu let  F  cmb wid clc gli mol ptr pad bra tai foc dep rou abi col
        match self {
            Profil::Rapide => [12, 8, 5, 6, 4, 14, 3, 3, 4, 2, 6, 1, 2, 1, 2, 1, 0, 1],
            Profil::Lent => [12, 10, 4, 5, 3, 12, 2, 2, 3, 2, 4, 1, 1, 1, 1, 1, 0, 0],
            Profil::Clavier => [25, 15, 10, 10, 8, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0],
            Profil::Manette => [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 40, 2, 0, 0, 0, 1, 0, 0],
            Profil::Souris => [0, 0, 0, 0, 0, 30, 6, 6, 8, 4, 0, 0, 2, 0, 3, 1, 0, 0],
            Profil::Joueur => [6, 30, 1, 8, 4, 3, 0, 0, 0, 0, 6, 0, 0, 0, 0, 1, 0, 0],
            Profil::Casseur => [10, 6, 6, 8, 8, 12, 6, 4, 4, 3, 6, 3, 5, 3, 5, 3, 3, 3],
        }
    }
}

const GENRES: usize = 18;

// ----- Gestes (enregistrés dans la trace) -----

#[derive(Clone, Debug, Serialize, Deserialize)]
enum Geste {
    Touche {
        cle: String,
        ctrl: bool,
        maj: bool,
        alt: bool,
        tenir: u32,
    },
    Texte(String),
    Coller(String),
    Clic {
        x: f32,
        y: f32,
        bouton: u8,
        double: bool,
    },
    Glisser {
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        images: u32,
    },
    Molette {
        x: f32,
        y: f32,
        dy: f32,
    },
    Pointeur {
        x: f32,
        y: f32,
    },
    PointeurParti,
    Manette {
        boutons: u32,
        axes: u8,
        tenir: u32,
    },
    Brancher(bool),
    Taille {
        l: f32,
        h: f32,
        ppp: f32,
    },
    Focus(bool),
    /// Chemins relatifs au dossier de la session ; `survol` images de survol avant le dépôt
    /// (`lacher` faux : glisser annulé).
    Deposer {
        chemins: Vec<String>,
        survol: u32,
        lacher: bool,
    },
    /// Fermeture puis réouverture ; fichiers des données abîmés entre les deux (chemin relatif,
    /// manière).
    Rouvrir {
        abimer: Vec<(String, u8)>,
    },
}

/// Évènement bas niveau à une image donnée.
enum Evt {
    Egui(Event),
    Mods(Modifiers),
    Pad(u32, u8),
    Brancher(bool),
    Taille(f32, f32, f32),
    Focus(bool),
    Survol(Vec<PathBuf>),
    Depot(Vec<PathBuf>),
    Rouvrir(Vec<(String, u8)>),
}

#[derive(Serialize, Deserialize)]
struct Trace {
    graine: u64,
    profil: Profil,
    duree: f64,
    gestes: Vec<(u64, Geste)>,
}

#[derive(Default, Serialize)]
struct Bilan {
    graine: u64,
    profil: String,
    ok: bool,
    genre: String,
    message: String,
    image: u64,
    images: u64,
    secondes_simulees: f64,
    secondes_reelles: f64,
    pire_image_ms: f64,
    pire_attente_disque_ms: f64,
    /// Images lentes dues aux autres programmes du serveur (voir `fil_cpu_et_attentes`).
    images_ralenties_serveur: u32,
    rss_max_mo: u64,
    fils_max: u64,
    rouvertures: u32,
    parties: u32,
    gestes: usize,
}

#[derive(Debug)]
struct Fichier(PathBuf);

impl egui::DroppedFile for Fichier {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

// ----- Fichiers de la session -----

/// ROM Game Boy synthétique : en-tête valide, programme qui écrit sans fin dans la RAM de la
/// cartouche (`sram`), s'arrête (`halt`) ou exécute des octets au hasard.
fn rom_gb(a: &mut Alea, titre: &str, genre: u8) -> Vec<u8> {
    let banques = [2usize, 4, 8][a.n(3)];
    let mut o = vec![0u8; 0x4000 * banques];
    if genre == 2 {
        let n = o.len();
        o[0x150..n].copy_from_slice(&a.octets(n - 0x150));
    }
    o[0x100..0x104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]);
    for (i, b) in titre.bytes().take(11).enumerate() {
        o[0x134 + i] = b;
    }
    o[0x143] = *a.choix(&[0x00, 0x00, 0x80, 0xC0]);
    let (mbc, ram) = *a.choix(&[
        (0x00u8, 0u8),
        (0x03, 0x02),
        (0x03, 0x03),
        (0x13, 0x03),
        (0x10, 0x02),
        (0x1B, 0x02),
        (0x06, 0x00),
        (0x01, 0x00),
    ]);
    o[0x147] = mbc;
    o[0x148] = banques.trailing_zeros() as u8 - 1;
    o[0x149] = ram;
    let code: &[u8] = match genre {
        // ld a,0x0A ; ld (0x0000),a ; ld hl,0xA000 ; boucle : inc (hl) ; inc l ; jr boucle
        0 => &[
            0x3E, 0x0A, 0xEA, 0x00, 0x00, 0x21, 0x00, 0xA0, 0x34, 0x2C, 0x18, 0xFC,
        ],
        // di ; halt ; jr -3
        1 => &[0xF3, 0x76, 0x18, 0xFD],
        _ => &[],
    };
    o[0x150..0x150 + code.len()].copy_from_slice(code);
    let mut x: u8 = 0;
    for &b in &o[0x134..0x14D] {
        x = x.wrapping_sub(b).wrapping_sub(1);
    }
    o[0x14D] = x;
    o
}

/// ROM GBA synthétique : boucle qui écrit en SRAM, ou octets au hasard.
fn rom_gba(a: &mut Alea, titre: &str, hasard: bool) -> Vec<u8> {
    let mut o = if hasard {
        a.octets(0x8000)
    } else {
        vec![0u8; 0x8000]
    };
    let mot = |o: &mut Vec<u8>, i: usize, m: u32| o[i..i + 4].copy_from_slice(&m.to_le_bytes());
    mot(&mut o, 0, 0xEA00_002E); // b 0xC0
    for (i, b) in titre.bytes().take(12).enumerate() {
        o[0xA0 + i] = b;
    }
    if !hasard {
        // mov r0,#0x0E000000 ; boucle : strb r1,[r0] ; add r1,r1,#1 ; b boucle
        for (i, m) in [0xE3A0_040Eu32, 0xE5C0_1000, 0xE281_1001, 0xEAFF_FFFC]
            .iter()
            .enumerate()
        {
            mot(&mut o, 0xC0 + 4 * i, *m);
        }
        o[0x200..0x209].copy_from_slice(b"SRAM_V113");
    }
    o
}

fn zip(chemin: &Path, entrees: &[(&str, &[u8])]) {
    let Ok(f) = std::fs::File::create(chemin) else {
        return;
    };
    let mut z = zip::ZipWriter::new(f);
    let opt = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);
    for (nom, d) in entrees {
        if nom.ends_with('/') {
            let _ = z.add_directory(*nom, opt);
        } else if z.start_file(*nom, opt).is_ok() {
            let _ = z.write_all(d);
        }
    }
    let _ = z.finish();
}

/// ROM de test publiques (blargg, mooneye, acid2, jsmolka…) trouvées sous `dossier`.
fn roms_publiques(dossier: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    let mut pile = vec![dossier.to_path_buf()];
    while let Some(d) = pile.pop() {
        let Ok(l) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in l.flatten() {
            let p = e.path();
            if p.is_dir() {
                pile.push(p);
            } else if matches!(
                p.extension().and_then(|x| x.to_str()),
                Some("gb" | "gbc" | "gba")
            ) && e.metadata().is_ok_and(|m| m.len() <= 4 << 20)
            {
                v.push(p);
            }
        }
    }
    v.sort();
    v
}

/// Prépare `jeux/` (bibliothèque de départ) et `depots/` (fichiers à glisser-déposer).
fn preparer(session: &Path, a: &mut Alea, publiques: &[PathBuf]) -> Vec<String> {
    let jeux = session.join("jeux");
    let dep = session.join("depots");
    let _ = std::fs::create_dir_all(&jeux);
    let _ = std::fs::create_dir_all(dep.join("dossier/sous/sous"));
    let mut depots = Vec::new();
    let mut noter = |p: &Path| {
        if let Ok(r) = p.strip_prefix(session) {
            depots.push(r.to_string_lossy().into_owned());
        }
    };
    // ROM publiques : quelques-unes dans la bibliothèque, d'autres à déposer.
    for i in 0..(2 + a.n(5)).min(publiques.len()) {
        let src = a.choix(publiques).clone();
        let nom = src.file_name().unwrap_or_default().to_os_string();
        let cible = if i % 2 == 0 {
            jeux.join(&nom)
        } else {
            dep.join(&nom)
        };
        if std::fs::copy(&src, &cible).is_ok() && i % 2 == 1 {
            noter(&cible);
        }
    }
    // ROM synthétiques GB/GBC/GBA.
    for i in 0..(3 + a.n(4)) {
        let t = format!("ROBOT{i:02}");
        let genre = *a.choix(&[0u8, 0, 0, 1, 2]);
        let (o, ext) = if a.proba(0.3) {
            (rom_gba(a, &t, genre == 2), "gba")
        } else {
            (rom_gb(a, &t, genre), *a.choix(&["gb", "gbc"]))
        };
        let dans = if a.proba(0.6) { &jeux } else { &dep };
        let c = dans.join(format!("Robot {i} [{genre}].{ext}"));
        let _ = std::fs::write(&c, &o);
        if dans == &dep {
            noter(&c);
        }
    }
    let gb = rom_gb(a, "ZIPROBOT", 0);
    let gba = rom_gba(a, "ZIPGBA", false);
    // Archives : saine, chemins piégés, bombe, taille annoncée fausse, tronquée, au hasard.
    zip(
        &jeux.join("archive saine.zip"),
        &[("a.gb", &gb), ("b.gba", &gba), ("lisezmoi.txt", b"x")],
    );
    let piege = dep.join("piege.zip");
    zip(
        &piege,
        &[
            ("../../evade.gb", &gb),
            ("/absolu.gbc", &gb),
            ("dossier/", b""),
            ("vide.gb", b""),
            ("C:\\fenetres\\x.gba", &gba),
        ],
    );
    noter(&piege);
    if a.proba(0.3) {
        // Bombe : 80 Mio de zéros (au-delà de la limite de 64 Mio) et 60 Mio (en deçà).
        let bombe = dep.join("bombe.zip");
        zip(
            &bombe,
            &[
                ("gros.gb", &vec![0u8; 80 << 20]),
                ("moyen.gba", &vec![0u8; 60 << 20]),
            ],
        );
        noter(&bombe);
    }
    let mut menteur = Vec::new();
    {
        let c = dep.join("menteur.zip");
        zip(&c, &[("menteur.gb", &vec![0x76u8; 8 << 20])]);
        if let Ok(mut d) = std::fs::read(&c) {
            // Répertoire central : taille décompressée annoncée ramenée à 32 Kio.
            if let Some(i) = d.windows(4).rposition(|w| w == [0x50, 0x4B, 0x01, 0x02]) {
                if i + 28 <= d.len() {
                    d[i + 24..i + 28].copy_from_slice(&0x8000u32.to_le_bytes());
                }
            }
            menteur = d;
        }
        let _ = std::fs::write(&c, &menteur);
        noter(&c);
    }
    if let Ok(d) = std::fs::read(jeux.join("archive saine.zip")) {
        let t = dep.join("tronquee.zip");
        let _ = std::fs::write(&t, &d[..d.len() / 2]);
        noter(&t);
    }
    let bruit = [
        ("hasard.zip", a.octets(5000)),
        ("hasard.7z", a.octets(3000)),
        (
            "entete.7z",
            [
                &[0x37u8, 0x7A, 0xBC, 0xAF, 0x27, 0x1C, 0, 4][..],
                &a.octets(200),
            ]
            .concat(),
        ),
        ("vide.gb", Vec::new()),
        ("court.gbc", a.octets(100)),
        ("hasard.gba", {
            let n = 1 + a.n(70_000);
            a.octets(n)
        }),
        ("un octet.gb", vec![0xFF]),
        ("dossier/sous/sous/profond.gb", rom_gb(a, "PROFOND", 0)),
        ("dossier/sous/Accentué ééé ✓.gbc", rom_gb(a, "ACCENT", 0)),
    ];
    for (nom, d) in bruit {
        let c = dep.join(nom);
        let _ = std::fs::write(&c, d);
        if !nom.starts_with("dossier") {
            noter(&c);
        }
    }
    noter(&dep.join("dossier"));
    // Fichier creux géant (aucune place sur le disque), lien en boucle, lien cassé, fichier
    // illisible, nom non UTF-8.
    if let Ok(f) = std::fs::File::create(dep.join("geant.gb")) {
        let _ = f.set_len(300 << 20);
        noter(&dep.join("geant.gb"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::PermissionsExt;
        let _ = std::os::unix::fs::symlink("..", dep.join("dossier/sous/boucle"));
        let _ = std::os::unix::fs::symlink("absent.gb", dep.join("casse.gb"));
        noter(&dep.join("casse.gb"));
        let ferme = dep.join("ferme.gb");
        let _ = std::fs::write(&ferme, rom_gb(a, "FERME", 0));
        let _ = std::fs::set_permissions(&ferme, std::fs::Permissions::from_mode(0o000));
        noter(&ferme);
        let nom = std::ffi::OsStr::from_bytes(b"non utf8 \xff\xfe.gb");
        let _ = std::fs::write(dep.join(nom), rom_gb(a, "NONUTF8", 0));
        // Le dossier contenant ce fichier se dépose ; le nom lui-même n'est pas représentable
        // dans la trace (chaîne UTF-8).
        let _ = std::fs::create_dir_all(dep.join("bizarre"));
        let _ = std::fs::write(dep.join("bizarre").join(nom), rom_gb(a, "NONUTF8", 0));
        noter(&dep.join("bizarre"));
    }
    noter(&jeux);
    depots.push("depots/absent.gb".into());
    depots
}

// ----- Session -----

struct Robot {
    profil: Profil,
    session: PathBuf,
    a: Alea,
    temps: Alea,
    depots: Vec<String>,
    rejouer: Option<Vec<(u64, Geste)>>,
    trace: Arc<Mutex<Trace>>,
    file: BTreeMap<u64, Vec<Evt>>,
    image: u64,
    t: f64,
    taille: Vec2,
    ppp: f32,
    focus: bool,
    mods: Modifiers,
    pad: Option<(u32, u8)>,
    branchees: Vec<(String, String)>,
    prochain: u64,
    /// Fichiers abîmés exprès (chemin, contenu écrit) : leur relecture n'est pas exigée.
    abimes: Vec<(PathBuf, Vec<u8>)>,
    /// Sauvegardes batterie vues : elles ne doivent jamais disparaître.
    savs: HashSet<PathBuf>,
    bilan: Bilan,
    son_depuis: Option<f64>,
    son_reste: f64,
    focus_inconnu: u32,
    analyse_depuis: Option<Instant>,
    journal: bool,
    /// [`IMAGE_MAX_MS`] (relâché dans les tests, compilés sans optimisation).
    image_max_ms: f64,
}

/// Échec d'invariant : genre (clé de dédoublonnage) et message.
type Echec = (String, String);

fn echec<T>(genre: &str, message: impl Into<String>) -> Result<T, Echec> {
    Err((genre.to_string(), message.into()))
}

/// Ce fil : temps processeur (ms, au centième de seconde) et mises en attente volontaires
/// (disque, verrou, condition). Une image lente sans attente volontaire et avec peu de calcul a
/// été ralentie par les autres programmes du serveur, pas par l'appli.
fn fil_cpu_et_attentes() -> (f64, u64) {
    let cpu = std::fs::read_to_string("/proc/thread-self/stat")
        .ok()
        .and_then(|s| {
            let apres = &s[s.rfind(')')? + 2..];
            let c: Vec<&str> = apres.split_whitespace().collect();
            Some((c.get(11)?.parse::<f64>().ok()? + c.get(12)?.parse::<f64>().ok()?) * 10.0)
        })
        .unwrap_or(0.0);
    let vol = std::fs::read_to_string("/proc/thread-self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("voluntary_ctxt_switches:"))
                .and_then(|v| v.trim().parse().ok())
        })
        .unwrap_or(0);
    (cpu, vol)
}

/// Fils d'exécution du processus.
fn fils() -> u64 {
    std::fs::read_dir("/proc/self/task").map_or(0, |l| l.count() as u64)
}

fn rss_mo() -> u64 {
    std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| s.split_whitespace().nth(1)?.parse::<u64>().ok())
        .map_or(0, |p| p * 4096 / (1 << 20))
}

const TOUCHES_NAV: [Key; 14] = [
    Key::ArrowUp,
    Key::ArrowDown,
    Key::ArrowLeft,
    Key::ArrowRight,
    Key::Enter,
    Key::Escape,
    Key::Tab,
    Key::Backspace,
    Key::Space,
    Key::PageUp,
    Key::PageDown,
    Key::Home,
    Key::End,
    Key::Delete,
];
const TOUCHES_JEU: [Key; 13] = [
    Key::ArrowUp,
    Key::ArrowDown,
    Key::ArrowLeft,
    Key::ArrowRight,
    Key::X,
    Key::Z,
    Key::Enter,
    Key::Backspace,
    Key::A,
    Key::S,
    Key::Tab,
    Key::R,
    Key::M,
];
const TOUCHES_F: [Key; 12] = [
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
    Key::F11,
    Key::F12,
];
const LETTRES: &str = "abcdefghijklmnopqrstuvwxyz0123456789";
const FICHIERS_DONNEES: [&str; 6] = [
    "reglages.json",
    "touches.json",
    "index.json",
    "stats.json",
    "telechargements.json",
    "crash.log",
];

impl Robot {
    /// Session prête : fichiers préparés, gestes rejoués (`rejeu`) ou tirés de la graine.
    fn nouveau(
        graine: u64,
        profil: Profil,
        duree: f64,
        session: &Path,
        publiques: &[PathBuf],
        rejeu: Option<Trace>,
        journal: bool,
    ) -> Robot {
        let session = session.to_path_buf();
        let mut fixtures = Alea::new(graine, 1);
        let depots = preparer(&session, &mut fixtures, publiques);
        let trace = Arc::new(Mutex::new(Trace {
            graine,
            profil,
            duree,
            gestes: Vec::new(),
        }));
        Robot {
            profil,
            session,
            a: Alea::new(graine, 2),
            temps: Alea::new(graine, 3),
            depots,
            rejouer: rejeu.map(|t| t.gestes),
            trace,
            file: BTreeMap::new(),
            image: 0,
            t: 0.0,
            taille: Vec2::new(1280.0, 800.0),
            ppp: 1.0,
            focus: true,
            mods: Modifiers::NONE,
            pad: None,
            branchees: Vec::new(),
            prochain: 30,
            abimes: Vec::new(),
            savs: HashSet::new(),
            bilan: Bilan {
                graine,
                profil: format!("{profil:?}"),
                ..Default::default()
            },
            son_depuis: None,
            son_reste: 0.0,
            focus_inconnu: 0,
            analyse_depuis: None,
            journal,
            image_max_ms: IMAGE_MAX_MS,
        }
    }

    fn opts(&self) -> Options {
        Options {
            debut: Instant::now(),
            jeux: Some(self.session.join("jeux")),
            donnees: Some(self.session.join("donnees")),
            demo: false,
            rom: None,
            capture: None,
            images: 30,
            menu: None,
            recherche: None,
            selection: 0,
            grille: false,
            robot: true,
            maj_url: None,
            maj_auto: false,
        }
    }

    fn planifier(&mut self, dans: u64, e: Evt) {
        self.file.entry(self.image + dans).or_default().push(e);
    }

    fn mods_de(ctrl: bool, maj: bool, alt: bool) -> Modifiers {
        Modifiers {
            alt,
            ctrl,
            shift: maj,
            mac_cmd: false,
            command: ctrl,
        }
    }

    /// Transforme un geste en évènements datés.
    fn executer(&mut self, g: &Geste) {
        match g {
            Geste::Touche {
                cle,
                ctrl,
                maj,
                alt,
                tenir,
            } => {
                let Some(k) = Key::from_name(cle) else { return };
                let m = Robot::mods_de(*ctrl, *maj, *alt);
                let ev = |pressed, repeat| {
                    Evt::Egui(Event::Key {
                        key: k,
                        physical_key: Some(k),
                        pressed,
                        repeat,
                        modifiers: m,
                    })
                };
                self.planifier(0, Evt::Mods(m));
                self.planifier(0, ev(true, false));
                if !ctrl && !alt && cle.len() == 1 {
                    let c = if *maj {
                        cle.clone()
                    } else {
                        cle.to_lowercase()
                    };
                    self.planifier(0, Evt::Egui(Event::Text(c)));
                } else if k == Key::Space {
                    self.planifier(0, Evt::Egui(Event::Text(" ".into())));
                }
                // Répétition automatique du clavier après 30 images tenues.
                let mut i = 30;
                while i < *tenir {
                    self.planifier(i as u64, ev(true, true));
                    i += 3;
                }
                self.planifier(*tenir as u64, ev(false, false));
                self.planifier(*tenir as u64, Evt::Mods(Modifiers::NONE));
            }
            Geste::Texte(t) => self.planifier(0, Evt::Egui(Event::Text(t.clone()))),
            Geste::Coller(t) => self.planifier(0, Evt::Egui(Event::Paste(t.clone()))),
            Geste::Clic {
                x,
                y,
                bouton,
                double,
            } => {
                let pos = Pos2::new(*x, *y);
                let b = match bouton {
                    1 => PointerButton::Secondary,
                    2 => PointerButton::Middle,
                    _ => PointerButton::Primary,
                };
                let clic = |pressed| {
                    Evt::Egui(Event::PointerButton {
                        pos,
                        button: b,
                        pressed,
                        modifiers: Modifiers::NONE,
                    })
                };
                self.planifier(0, Evt::Egui(Event::PointerMoved(pos)));
                self.planifier(0, clic(true));
                self.planifier(1, clic(false));
                if *double {
                    self.planifier(2, clic(true));
                    self.planifier(3, clic(false));
                }
            }
            Geste::Glisser {
                x0,
                y0,
                x1,
                y1,
                images,
            } => {
                let n = (*images).max(1);
                let p0 = Pos2::new(*x0, *y0);
                let bouton = |pos, pressed| {
                    Evt::Egui(Event::PointerButton {
                        pos,
                        button: PointerButton::Primary,
                        pressed,
                        modifiers: Modifiers::NONE,
                    })
                };
                self.planifier(0, Evt::Egui(Event::PointerMoved(p0)));
                self.planifier(0, bouton(p0, true));
                for i in 1..=n {
                    let f = i as f32 / n as f32;
                    let p = Pos2::new(x0 + (x1 - x0) * f, y0 + (y1 - y0) * f);
                    self.planifier(i as u64, Evt::Egui(Event::PointerMoved(p)));
                }
                self.planifier(n as u64 + 1, bouton(Pos2::new(*x1, *y1), false));
            }
            Geste::Molette { x, y, dy } => {
                self.planifier(0, Evt::Egui(Event::PointerMoved(Pos2::new(*x, *y))));
                self.planifier(
                    0,
                    Evt::Egui(Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Line,
                        delta: Vec2::new(0.0, *dy),
                        phase: egui::TouchPhase::Move,
                        source: egui::MouseWheelSource::Wheel,
                        modifiers: Modifiers::NONE,
                    }),
                );
            }
            Geste::Pointeur { x, y } => {
                self.planifier(0, Evt::Egui(Event::PointerMoved(Pos2::new(*x, *y))))
            }
            Geste::PointeurParti => self.planifier(0, Evt::Egui(Event::PointerGone)),
            Geste::Manette {
                boutons,
                axes,
                tenir,
            } => {
                self.planifier(0, Evt::Pad(*boutons, *axes));
                self.planifier(*tenir as u64, Evt::Pad(0, 0));
            }
            Geste::Brancher(b) => self.planifier(0, Evt::Brancher(*b)),
            Geste::Taille { l, h, ppp } => self.planifier(0, Evt::Taille(*l, *h, *ppp)),
            Geste::Focus(f) => self.planifier(0, Evt::Focus(*f)),
            Geste::Deposer {
                chemins,
                survol,
                lacher,
            } => {
                let c: Vec<PathBuf> = chemins.iter().map(|c| self.session.join(c)).collect();
                for i in 0..*survol {
                    self.planifier(i as u64, Evt::Survol(c.clone()));
                }
                if *lacher {
                    self.planifier(*survol as u64, Evt::Depot(c));
                }
            }
            Geste::Rouvrir { abimer } => self.planifier(0, Evt::Rouvrir(abimer.clone())),
        }
    }

    fn touche(&mut self, k: Key, ctrl: bool, maj: bool, tenir: u32) -> Geste {
        Geste::Touche {
            cle: k.name().to_string(),
            ctrl,
            maj,
            alt: false,
            tenir,
        }
    }

    fn point(&mut self) -> (f32, f32) {
        (
            self.a.entre(0.0, self.taille.x),
            self.a.entre(0.0, self.taille.y),
        )
    }

    /// Tire le prochain geste selon le profil ; `cibles` : rectangles cliquables de l'image.
    fn tirer(&mut self, cibles: &[Rect], sonde: &Sonde) -> Geste {
        let poids = self.profil.poids();
        let total: u32 = poids.iter().sum();
        let mut r = self.a.n(total as usize) as u32;
        let mut genre = 0;
        for (i, p) in poids.iter().enumerate() {
            if r < *p {
                genre = i;
                break;
            }
            r -= p;
        }
        let en_jeu = sonde.partie.is_some();
        match genre {
            0 => {
                let k = *self.a.choix(&TOUCHES_NAV);
                let tenir = if self.a.proba(0.15) {
                    40 + self.a.n(60) as u32
                } else {
                    1 + self.a.n(4) as u32
                };
                self.touche(k, false, false, tenir)
            }
            1 => {
                let k = *self.a.choix(&TOUCHES_JEU);
                let tenir = if en_jeu {
                    5 + self.a.n(240) as u32
                } else {
                    1 + self.a.n(3) as u32
                };
                let maj = self.a.proba(0.05);
                self.touche(k, false, maj, tenir)
            }
            2 => {
                if self.a.proba(0.5) {
                    let n = 1 + self.a.n(6);
                    let t: String = (0..n)
                        .map(|_| {
                            let i = self.a.n(LETTRES.len());
                            LETTRES.as_bytes()[i] as char
                        })
                        .collect();
                    Geste::Texte(t)
                } else {
                    let i = self.a.n(26);
                    let k =
                        Key::from_name(&((b'A' + i as u8) as char).to_string()).unwrap_or(Key::A);
                    let maj = self.a.proba(0.2);
                    let tenir = 1 + self.a.n(3) as u32;
                    self.touche(k, false, maj, tenir)
                }
            }
            3 => {
                let k = *self.a.choix(&TOUCHES_F);
                let (ctrl, maj) = (self.a.proba(0.25), self.a.proba(0.25));
                let tenir = 1 + self.a.n(3) as u32;
                self.touche(k, ctrl, maj, tenir)
            }
            4 => {
                let k = *self.a.choix(&[
                    Key::Z,
                    Key::Backspace,
                    Key::Tab,
                    Key::V,
                    Key::C,
                    Key::A,
                    Key::Enter,
                    Key::Escape,
                    Key::Plus,
                    Key::Minus,
                    Key::Equals,
                    Key::Q,
                    Key::W,
                    Key::F4,
                ]);
                Geste::Touche {
                    cle: k.name().to_string(),
                    ctrl: self.a.proba(0.7),
                    maj: self.a.proba(0.4),
                    alt: self.a.proba(0.1),
                    tenir: 1 + self.a.n(3) as u32,
                }
            }
            5 if !cibles.is_empty() => {
                let c = *self.a.choix(cibles);
                let x = self.a.entre(c.min.x, c.max.x);
                let y = self.a.entre(c.min.y, c.max.y);
                Geste::Clic {
                    x,
                    y,
                    bouton: if self.a.proba(0.1) { 1 } else { 0 },
                    double: self.a.proba(0.2),
                }
            }
            5 | 6 => {
                let (x, y) = self.point();
                Geste::Clic {
                    x,
                    y,
                    bouton: self.a.n(3) as u8,
                    double: self.a.proba(0.2),
                }
            }
            7 => {
                let (x0, y0) = match cibles.is_empty() {
                    true => self.point(),
                    false => {
                        let c = self.a.choix(cibles).center();
                        (c.x, c.y)
                    }
                };
                let (x1, y1) = self.point();
                Geste::Glisser {
                    x0,
                    y0,
                    x1,
                    y1,
                    images: 1 + self.a.n(40) as u32,
                }
            }
            8 => {
                let (x, y) = self.point();
                let dy = *self.a.choix(&[-30.0, -5.0, -1.0, 1.0, 3.0, 12.0, 200.0]);
                Geste::Molette { x, y, dy }
            }
            9 => {
                if self.a.proba(0.2) {
                    Geste::PointeurParti
                } else {
                    let (x, y) = self.point();
                    Geste::Pointeur { x, y }
                }
            }
            10 => {
                // Un bouton (ou un accord de deux) ; sticks parfois.
                let mut boutons = 1u32 << self.a.n(BOUTONS.len());
                if self.a.proba(0.15) {
                    boutons |= 1 << self.a.n(BOUTONS.len());
                }
                let axes = if self.a.proba(0.15) {
                    1 << self.a.n(8)
                } else {
                    0
                };
                let tenir = if en_jeu && self.a.proba(0.5) {
                    5 + self.a.n(200) as u32
                } else {
                    1 + self.a.n(4) as u32
                };
                Geste::Manette {
                    boutons,
                    axes,
                    tenir,
                }
            }
            11 => Geste::Brancher(self.a.proba(0.6)),
            12 => {
                let (l, h) = if self.a.proba(0.2) {
                    (720.0, 480.0)
                } else {
                    (self.a.entre(720.0, 2560.0), self.a.entre(480.0, 1440.0))
                };
                let ppp = *self.a.choix(&[1.0, 1.0, 1.25, 1.5, 2.0]);
                Geste::Taille {
                    l: l.round(),
                    h: h.round(),
                    ppp,
                }
            }
            13 => Geste::Focus(self.a.proba(0.5)),
            14 if !self.depots.is_empty() => {
                let n = 1 + self.a.n(3);
                let chemins = (0..n)
                    .map(|_| self.a.choix(&self.depots.clone()).clone())
                    .collect();
                Geste::Deposer {
                    chemins,
                    survol: 1 + self.a.n(20) as u32,
                    lacher: self.a.proba(0.85),
                }
            }
            15 => Geste::Rouvrir { abimer: Vec::new() },
            16 => Geste::Rouvrir {
                abimer: self.choisir_abimes(),
            },
            17 => {
                let t = match self.a.n(4) {
                    0 => "a".repeat(1 + self.a.n(5000)),
                    1 => "/".into(),
                    2 => "\u{0}\u{7f}éà😀\n\t".into(),
                    _ => self.a.choix(&self.depots.clone()).clone(),
                };
                Geste::Coller(t)
            }
            _ => self.touche(Key::Escape, false, false, 1),
        }
    }

    /// Fichiers des données à abîmer (casseur) : réglages, index, états…
    fn choisir_abimes(&mut self) -> Vec<(String, u8)> {
        let donnees = self.session.join("donnees");
        let mut candidats: Vec<String> = FICHIERS_DONNEES
            .iter()
            .filter(|f| donnees.join(f).exists())
            .map(|f| format!("donnees/{f}"))
            .collect();
        if let Ok(l) = std::fs::read_dir(donnees.join("etats")) {
            for jeu in l.flatten() {
                if let Ok(l2) = std::fs::read_dir(jeu.path()) {
                    for e in l2.flatten() {
                        if let Ok(r) = e.path().strip_prefix(&self.session) {
                            candidats.push(r.to_string_lossy().into_owned());
                        }
                    }
                }
            }
        }
        candidats.sort();
        let mut v = Vec::new();
        for _ in 0..(1 + self.a.n(3)) {
            if candidats.is_empty() {
                break;
            }
            v.push((self.a.choix(&candidats).clone(), self.a.n(4) as u8));
        }
        v
    }

    fn abimer(&mut self, liste: &[(String, u8)]) {
        for (rel, maniere) in liste {
            let c = self.session.join(rel);
            let Ok(d) = std::fs::read(&c) else { continue };
            let nouveau = match maniere {
                0 => d[..d.len() / 2].to_vec(),
                1 => Vec::new(),
                2 => {
                    let mut d = d;
                    let n = d.len();
                    for i in 0..n.min(64) {
                        let j = (i * 7919) % n;
                        d[j] ^= 0x5A;
                    }
                    d
                }
                _ => b"{\"pas\": \"du json valide".to_vec(),
            };
            if std::fs::write(&c, &nouveau).is_ok() {
                self.abimes.push((c, nouveau));
            }
        }
    }

    /// Fichier abîmé exprès, y compris déplacé depuis (dossier d'états qui suit le nom de la ROM,
    /// `sauve::migrer`) : même nom de fichier et même contenu.
    fn est_abime(&self, c: &Path) -> bool {
        self.abimes.iter().any(|(p, d)| {
            p == c || (p.file_name() == c.file_name() && std::fs::read(c).is_ok_and(|lu| lu == *d))
        })
    }

    /// Après fermeture : tout ce que l'appli avait en mémoire est relu à l'identique.
    fn verifier_persistance(&mut self, avant: &Sonde) -> Result<(), Echec> {
        let donnees = self.session.join("donnees");
        let cfg = crate::sauve::Chemins::trouver(Some(donnees.clone())).config();
        let defaut = serde_json::to_string(&crate::sauve::Config::default()).unwrap_or_default();
        if !self.est_abime(&cfg) && (cfg.exists() || avant.config != defaut) {
            let relu =
                serde_json::to_string(&crate::sauve::Config::charger(&cfg)).unwrap_or_default();
            if relu != avant.config {
                return echec(
                    "reglages-perdus",
                    format!(
                        "reglages.json relu différent :\navant {}\naprès {relu}",
                        avant.config
                    ),
                );
            }
        }
        let tch = crate::touches::Touches::chemin(&donnees);
        if !self.est_abime(&tch) {
            let relu = crate::touches::Touches::charger(&tch).vers_json();
            if relu != avant.touches {
                return echec(
                    "touches-perdues",
                    format!(
                        "touches.json relu différent :\navant {}\naprès {relu}",
                        avant.touches
                    ),
                );
            }
        }
        if let Some(crate::app::PartieSondee {
            export,
            sav,
            etats,
            rom,
        }) = &avant.partie
        {
            if let Some(e) = export {
                let relu = std::fs::read(sav).ok();
                if relu.as_ref() != Some(e) {
                    return echec(
                        "sauvegarde-perdue",
                        format!(
                            "{} : {} octets sur le disque, {} en mémoire",
                            sav.display(),
                            relu.map_or(0, |r| r.len()),
                            e.len()
                        ),
                    );
                }
            }
            let octets = biblio::lire_rom(rom).map_err(|e| ("rom".into(), e.to_string()))?;
            if let Ok(l) = std::fs::read_dir(etats) {
                for f in l.flatten() {
                    let p = f.path();
                    if p.extension().and_then(|e| e.to_str()) != Some("state") || self.est_abime(&p)
                    {
                        continue;
                    }
                    let d = std::fs::read(&p).unwrap_or_default();
                    let mut m = crate::machine::Machine::new(
                        &octets,
                        rom.plateforme == biblio::Plateforme::Gba,
                    )
                    .map_err(|e| ("rom".to_string(), e))?;
                    if let Err(e) = m.load_state(&d) {
                        return echec(
                            "etat-illisible",
                            format!("{} : {e:?} ({} octets)", p.display(), d.len()),
                        );
                    }
                }
            }
        }
        // Sauvegardes batterie : aucune ne disparaît ni ne se vide.
        let dossier = donnees.join("sauvegardes");
        // Deux copies d'une même ROM (même CRC) partagent une sauvegarde qui suit le dernier nom
        // lancé (`sauve::migrer`) : il suffit qu'une sauvegarde de ce CRC reste.
        let non_vide = |p: &Path| std::fs::metadata(p).is_ok_and(|m| m.len() > 0);
        let suffixe = |p: &Path| {
            let n = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            n.rfind(" [").map(|i| n[i..].to_string()).unwrap_or(n)
        };
        let presents: Vec<PathBuf> = std::fs::read_dir(&dossier)
            .map(|l| {
                l.flatten()
                    .map(|e| e.path())
                    .filter(|p| non_vide(p))
                    .collect()
            })
            .unwrap_or_default();
        for s in &self.savs {
            if !non_vide(s) && !presents.iter().any(|p| suffixe(p) == suffixe(s)) {
                return echec("sauvegarde-disparue", s.display().to_string());
            }
        }
        if let Ok(l) = std::fs::read_dir(&dossier) {
            for f in l.flatten() {
                let p = f.path();
                if p.extension().and_then(|e| e.to_str()) == Some("sav")
                    && f.metadata().is_ok_and(|m| m.len() > 0)
                {
                    self.savs.insert(p);
                }
            }
        }
        Ok(())
    }

    /// Invariants après une image.
    fn verifier(&mut self, ctx: &egui::Context, app: &App, duree_ms: f64) -> Result<(), Echec> {
        self.bilan.pire_image_ms = self.bilan.pire_image_ms.max(duree_ms);
        if duree_ms > self.image_max_ms {
            let t = self.trace.lock().unwrap_or_else(|e| e.into_inner());
            let n = t.gestes.len();
            let derniers: Vec<String> = t.gestes[n.saturating_sub(3)..]
                .iter()
                .map(|(i, g)| format!("{i}: {g:?}"))
                .collect();
            return echec(
                "image-lente",
                format!("image de {duree_ms:.0} ms\n{}", derniers.join("\n")),
            );
        }
        if self.image.is_multiple_of(30) {
            let m = rss_mo();
            self.bilan.rss_max_mo = self.bilan.rss_max_mo.max(m);
            self.bilan.fils_max = self.bilan.fils_max.max(fils());
            if self.journal && self.image.is_multiple_of(600) {
                eprintln!("robot {} : {m} Mo, {} fils", self.image, fils());
            }
            if m > MEMOIRE_MAX_MO {
                return echec("memoire", format!("{m} Mo résidents"));
            }
        }
        let s = app.sonde();
        for (a, b) in [("touches", "menu"), ("partie", "fiche")] {
            if s.couches.contains(&a) && s.couches.contains(&b) {
                return echec(
                    "ui-incoherente",
                    format!("{a} et {b} ouverts : {:?}", s.couches),
                );
            }
        }
        if !s.menu_coherent {
            return echec("ui-incoherente", "sélection du menu hors de la liste");
        }
        if !s.sel_coherente {
            return echec(
                "ui-incoherente",
                "sélection de la bibliothèque hors de la vue",
            );
        }
        // Focus clavier sur un élément qui n'existe plus (deux images de suite).
        let focus = ctx.memory(|m| m.focused());
        let connu = focus.is_none_or(|id| ctx.viewport(|v| v.prev_pass.widgets.get(id).is_some()));
        self.focus_inconnu = if connu { 0 } else { self.focus_inconnu + 1 };
        if self.focus_inconnu > 2 {
            return echec("focus", format!("focus sur {focus:?}, élément absent"));
        }
        match (s.analyse_finie, self.analyse_depuis) {
            (true, _) => self.analyse_depuis = None,
            (false, None) => self.analyse_depuis = Some(Instant::now()),
            (false, Some(t)) if t.elapsed().as_secs_f64() > ANALYSE_MAX_S => {
                return echec("analyse-bloquee", format!("analyse > {ANALYSE_MAX_S} s"));
            }
            _ => {}
        }
        Ok(())
    }

    /// Carte son simulée : consomme le temps écoulé, juge le tampon en jeu continu.
    fn son(&mut self, app: &App, dt: f64, au_rythme: bool) -> Result<(), Echec> {
        let Some(sortie) = app.robot_peripheriques().1 else {
            return Ok(());
        };
        // La carte son lit par périodes de 10 ms, à son propre rythme.
        self.son_reste += dt * ECH_S;
        let mut manque = 0;
        while self.son_reste >= PERIODE_SON {
            self.son_reste -= PERIODE_SON;
            manque += sortie.consommer(PERIODE_SON as usize);
        }
        let ms = sortie.remplissage() as f64 * 1000.0 / ECH_S;
        if !au_rythme || dt > 0.05 {
            self.son_depuis = None;
            return Ok(());
        }
        let depuis = *self.son_depuis.get_or_insert(self.t);
        if self.t - depuis < SON_STABLE_S {
            return Ok(());
        }
        if manque > 0 {
            return echec(
                "son-trou",
                format!(
                    "tampon vide après {:.1} s de jeu ({manque} échantillons manquants)",
                    self.t - depuis
                ),
            );
        }
        if ms > 110.0 {
            return echec("son-sature", format!("tampon à {ms:.0} ms"));
        }
        Ok(())
    }

    fn entree(&mut self, dt: f64, evts: Vec<Evt>) -> (RawInput, Vec<(String, u8)>, bool) {
        let mut raw = RawInput {
            time: Some(self.t),
            predicted_dt: dt as f32,
            max_texture_side: Some(8192),
            ..Default::default()
        };
        let mut rouvrir = None;
        for e in evts {
            match e {
                Evt::Egui(ev) => raw.events.push(ev),
                Evt::Mods(m) => {
                    if m != self.mods {
                        raw.events.push(Event::ModifiersChanged(m));
                    }
                    self.mods = m;
                }
                Evt::Pad(b, a) => self.pad = Some((b, a)),
                Evt::Brancher(b) => {
                    if b && self.pad.is_none() {
                        self.branchees
                            .push(("726f626f74".into(), "Manette robot".into()));
                        self.pad = Some((0, 0));
                    } else if !b {
                        self.pad = None;
                    }
                }
                Evt::Taille(l, h, p) => {
                    self.taille = Vec2::new(l, h);
                    self.ppp = p;
                }
                Evt::Focus(f) => {
                    self.focus = f;
                    raw.events.push(Event::WindowFocused(f));
                }
                Evt::Survol(c) => {
                    raw.hovered_files = c
                        .into_iter()
                        .map(|p| egui::HoveredFile {
                            path: Some(p),
                            mime: String::new(),
                        })
                        .collect()
                }
                Evt::Depot(c) => {
                    raw.dropped_files = c
                        .into_iter()
                        .map(|p| Arc::new(Fichier(p)) as egui::DroppedFileHandle)
                        .collect()
                }
                Evt::Rouvrir(a) => rouvrir = Some(a),
            }
        }
        raw.focused = self.focus;
        raw.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, self.taille));
        raw.viewports.insert(
            ViewportId::ROOT,
            ViewportInfo {
                native_pixels_per_point: Some(self.ppp),
                inner_rect: Some(Rect::from_min_size(Pos2::ZERO, self.taille * self.ppp)),
                focused: Some(self.focus),
                fullscreen: Some(false),
                ..Default::default()
            },
        );
        let r = rouvrir.is_some();
        (raw, rouvrir.unwrap_or_default(), r)
    }

    fn lancer(&mut self, duree: f64, garde: &AtomicU64) -> Result<(), Echec> {
        let (base, ecart) = self.profil.cadence();
        let mut ctx = egui::Context::default();
        let mut app = App::creer(&ctx, self.opts());
        let mut cibles: Vec<Rect> = Vec::new();
        let mut etait_en_jeu = false;
        let mut couches_prec = Vec::new();
        let debut = Instant::now();
        while self.t < duree {
            // Machine lente : à-coups occasionnels (jamais au-delà de 0,1 s, borne de l'appli).
            let mut dt = base + (self.temps.f() as f64 - 0.5) * 2.0 * ecart;
            if self.profil == Profil::Lent && self.temps.proba(0.01) {
                dt = 0.09;
            }
            self.t += dt;
            // Gestes : rejoués depuis la trace ou tirés au hasard.
            if let Some(r) = &self.rejouer {
                let gestes: Vec<Geste> = r
                    .iter()
                    .filter(|(i, _)| *i == self.image)
                    .map(|(_, g)| g.clone())
                    .collect();
                for g in gestes {
                    if self.journal {
                        eprintln!("robot {} : geste {g:?}", self.image);
                    }
                    self.executer(&g);
                }
            } else if self.image >= self.prochain {
                let s = app.sonde();
                let g = self.tirer(&cibles, &s);
                self.executer(&g);
                self.trace
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .gestes
                    .push((self.image, g));
                let (a, b) = self.profil.rythme();
                self.prochain = self.image + (a + self.a.n(b - a + 1)) as u64;
            }
            let evts = self.file.remove(&self.image).unwrap_or_default();
            let (raw, abimer, rouvrir) = self.entree(dt, evts);
            if let Some(m) = app.robot_peripheriques().0 {
                let pads = self
                    .pad
                    .map(|(b, a)| Pad {
                        id: 0,
                        uuid: "726f626f74".into(),
                        nom: "Manette robot".into(),
                        tenus: b,
                        axes: a,
                        ..Pad::default()
                    })
                    .into_iter()
                    .collect();
                m.simuler(pads, std::mem::take(&mut self.branchees));
            }
            let attente0 = crate::sauve::ATTENTE_US.load(Ordering::Relaxed);
            let (cpu0, vol0) = fil_cpu_et_attentes();
            let t0 = Instant::now();
            garde.store(debut.elapsed().as_millis() as u64 + 1, Ordering::Relaxed);
            let mut sortie = ctx.run_ui(raw, |ui| app.image(ui));
            let ppp = sortie.pixels_per_point;
            let _ = ctx.tessellate(std::mem::take(&mut sortie.shapes), ppp);
            // Pas de carte graphique : les textures envoyées sont jetées.
            sortie.textures_delta.clear();
            garde.store(0, Ordering::Relaxed);
            let attente =
                (crate::sauve::ATTENTE_US.load(Ordering::Relaxed) - attente0) as f64 / 1000.0;
            self.bilan.pire_attente_disque_ms = self.bilan.pire_attente_disque_ms.max(attente);
            // Attente du disque d'une action explicite sur un emplacement : comptée à part.
            let mut duree_ms = t0.elapsed().as_secs_f64() * 1000.0 - attente;
            if duree_ms > self.image_max_ms {
                let (cpu1, vol1) = fil_cpu_et_attentes();
                // Ni attente volontaire ni calcul à la hauteur : processeur pris par le serveur.
                if vol1 == vol0 && cpu1 - cpu0 + 20.0 < self.image_max_ms {
                    self.bilan.images_ralenties_serveur += 1;
                    duree_ms = cpu1 - cpu0;
                }
            }
            let mut fermer = rouvrir;
            for v in sortie.viewport_output.values() {
                for c in &v.commands {
                    match c {
                        ViewportCommand::Close => fermer = true,
                        ViewportCommand::Fullscreen(true) => {
                            self.taille = Vec2::new(1920.0, 1080.0)
                        }
                        _ => {}
                    }
                }
            }
            let s = app.sonde();
            // `--journal` : changements de couches et images lentes, sur la sortie d'erreur.
            if self.journal && (s.couches != couches_prec || duree_ms > 50.0) {
                eprintln!("robot {} : {:?} {duree_ms:.0} ms", self.image, s.couches);
                couches_prec = s.couches.clone();
            }
            self.verifier(&ctx, &app, duree_ms)?;
            self.son(&app, dt, s.jeu_au_rythme)?;
            if s.partie.is_some() && !etait_en_jeu {
                self.bilan.parties += 1;
            }
            etait_en_jeu = s.partie.is_some();
            if let Some(p) = &s.partie {
                if std::fs::metadata(&p.sav).is_ok_and(|m| m.len() > 0) {
                    self.savs.insert(p.sav.clone());
                }
            }
            cibles = ctx.viewport(|v| {
                let w = &v.prev_pass.widgets;
                w.layer_ids()
                    .flat_map(|l| w.get_layer(l))
                    .filter(|r| {
                        r.enabled
                            && r.visible
                            && (r.sense.senses_click() || r.sense.senses_drag())
                            && r.interact_rect.is_positive()
                            && r.interact_rect.width() < self.taille.x
                    })
                    .map(|r| r.interact_rect)
                    .collect()
            });
            self.image += 1;
            if fermer {
                app.fermer();
                // Après la fermeture : palette du jeu rendue, réglages définitifs.
                let s = app.sonde();
                drop(app);
                self.verifier_persistance(&s)?;
                self.abimer(&abimer);
                self.bilan.rouvertures += 1;
                ctx = egui::Context::default();
                app = App::creer(&ctx, self.opts());
                cibles.clear();
                self.mods = Modifiers::NONE;
                self.son_depuis = None;
                // Réglages relus : identiques à ceux d'avant la fermeture (sauf fichier abîmé).
                let cfg =
                    crate::sauve::Chemins::trouver(Some(self.session.join("donnees"))).config();
                let apres = app.sonde();
                if !self.est_abime(&cfg) && apres.config != s.config {
                    return echec(
                        "reglages-perdus",
                        format!(
                            "réglages rouverts :\navant {}\naprès {}",
                            s.config, apres.config
                        ),
                    );
                }
            }
        }
        app.fermer();
        let s = app.sonde();
        drop(app);
        self.verifier_persistance(&s)?;
        self.bilan.images = self.image;
        Ok(())
    }
}

static PANIQUE: Mutex<Option<String>> = Mutex::new(None);

fn bilan_json(b: &Bilan) -> String {
    serde_json::to_string(b).unwrap_or_default()
}

fn ecrire_trace(chemin: Option<&PathBuf>, trace: &Mutex<Trace>) {
    if let Some(c) = chemin {
        let t = trace.lock().unwrap_or_else(|e| e.into_inner());
        if let Ok(j) = serde_json::to_vec(&*t) {
            let _ = std::fs::write(c, j);
        }
    }
}

/// `gbemu --robot GRAINE [--duree S] [--profil P] [--dossier D] [--roms-test D] [--trace F]
/// [--rejouer F] [--garder]`. Code de sortie : 0 réussite, 1 invariant, 2 panique, 3 blocage.
pub fn principal(args: &[String]) -> i32 {
    let mut graine = 0u64;
    let mut duree = 20.0;
    let mut profil = None;
    let mut dossier = None;
    let mut roms_test = None;
    let mut trace_f = None;
    let mut rejouer = None;
    let mut garder = false;
    let mut journal = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned().unwrap_or_default();
        match a.as_str() {
            "--robot" => graine = val().parse().unwrap_or(0),
            "--duree" => duree = val().parse().unwrap_or(20.0),
            "--profil" => profil = Profil::lire(&val()),
            "--dossier" => dossier = Some(PathBuf::from(val())),
            "--roms-test" => roms_test = Some(PathBuf::from(val())),
            "--trace" => trace_f = Some(PathBuf::from(val())),
            "--rejouer" => rejouer = Some(PathBuf::from(val())),
            "--garder" => garder = true,
            "--journal" => journal = true,
            _ => {}
        }
    }
    let rejeu: Option<Trace> = rejouer
        .and_then(|f| std::fs::read(f).ok())
        .and_then(|d| serde_json::from_slice(&d).ok());
    if let Some(t) = &rejeu {
        graine = t.graine;
        duree = t.duree;
        profil = Some(t.profil);
    }
    let profil = profil.unwrap_or(PROFILS[(graine % PROFILS.len() as u64) as usize]);
    let session =
        dossier.unwrap_or_else(|| std::env::temp_dir().join(format!("gbemu-robot-{graine}")));
    let session = std::path::absolute(&session).unwrap_or(session);
    // Rejeu dans le même dossier : seuls les sous-dossiers du robot sont recréés.
    for d in ["donnees", "jeux", "depots"] {
        let _ = std::fs::remove_dir_all(session.join(d));
    }
    let _ = std::fs::create_dir_all(&session);
    crate::dat::RESEAU_COUPE.store(true, Ordering::Relaxed);

    let publiques = roms_test.map(|d| roms_publiques(&d)).unwrap_or_default();
    let mut r = Robot::nouveau(graine, profil, duree, &session, &publiques, rejeu, journal);
    let trace = Arc::clone(&r.trace);

    // Panique : message et emplacement gardés pour le bilan.
    std::panic::set_hook(Box::new(|info| {
        let pile = std::backtrace::Backtrace::force_capture().to_string();
        // Premières lignes de la pile dans le code de gbemu (dédoublonnage).
        let gbemu: Vec<&str> = pile
            .lines()
            .filter(|l| {
                l.contains("gb_app::") || l.contains("gb_core::") || l.contains("gba_core::")
            })
            .take(6)
            .map(str::trim)
            .collect();
        *PANIQUE.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(format!("{info}\n{}", gbemu.join("\n")));
    }));
    // Chien de garde : image bloquée plus de BLOCAGE_S secondes.
    let garde = Arc::new(AtomicU64::new(0));
    {
        let garde = Arc::clone(&garde);
        let trace = Arc::clone(&trace);
        let trace_f = trace_f.clone();
        let debut = Instant::now();
        let profil = format!("{profil:?}");
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(250));
            let g = garde.load(Ordering::Relaxed);
            let maintenant = debut.elapsed().as_millis() as u64;
            if g != 0 && maintenant.saturating_sub(g) > BLOCAGE_S * 1000 {
                ecrire_trace(trace_f.as_ref(), &trace);
                let b = Bilan {
                    graine,
                    profil,
                    genre: "blocage".into(),
                    message: format!("image bloquée plus de {BLOCAGE_S} s"),
                    ..Default::default()
                };
                println!("{}", bilan_json(&b));
                std::process::exit(3);
            }
        });
    }

    let debut = Instant::now();
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| r.lancer(duree, &garde)));
    let code = match res {
        Ok(Ok(())) => {
            r.bilan.ok = true;
            0
        }
        Ok(Err((genre, message))) => {
            r.bilan.genre = genre;
            r.bilan.message = message;
            1
        }
        Err(_) => {
            r.bilan.genre = "panique".into();
            r.bilan.message = PANIQUE
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
                .unwrap_or_default();
            2
        }
    };
    r.bilan.image = r.image;
    r.bilan.images = r.image;
    r.bilan.secondes_simulees = r.t;
    r.bilan.secondes_reelles = debut.elapsed().as_secs_f64();
    r.bilan.gestes = trace.lock().map_or(0, |t| t.gestes.len());
    if code != 0 {
        ecrire_trace(trace_f.as_ref(), &trace);
    }
    println!("{}", bilan_json(&r.bilan));
    if code == 0 && !garder {
        for d in ["donnees", "jeux", "depots"] {
            let _ = std::fs::remove_dir_all(session.join(d));
        }
        let _ = std::fs::remove_dir(&session);
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rejoue une trace réduite (`scripts/robot.py reduire`) : aucun invariant ne doit casser.
    fn rejouer(json: &str) {
        let t: Trace = serde_json::from_str(json).unwrap();
        let d = crate::sauve::DossierTest::new("robot");
        crate::dat::RESEAU_COUPE.store(true, Ordering::Relaxed);
        let (graine, profil, duree) = (t.graine, t.profil, t.duree);
        let mut r = Robot::nouveau(graine, profil, duree, &d.0, &[], Some(t), false);
        r.image_max_ms = 30_000.0;
        if let Err((genre, message)) = r.lancer(duree, &AtomicU64::new(0)) {
            panic!("graine {graine} : {genre} : {message}");
        }
    }

    /// Clic sur le bouton pause de la barre de jeu, puis Entrée tenue : le bouton gardait le
    /// focus d'egui, Entrée (Start) le « recliquait » et le menu s'ouvrait par-dessus l'écran des
    /// touches (deux menus ouverts).
    #[test]
    fn graine_322_focus_de_la_barre() {
        rejouer(include_str!("../robot-traces/322-focus-barre.json"));
    }

    /// Clic droit sur le curseur de volume : volume changé sans être enregistré, perdu à la
    /// fermeture.
    #[test]
    fn graine_63_volume_perdu() {
        rejouer(include_str!("../robot-traces/63-volume-perdu.json"));
    }

    /// Chargement d'un état : tampon son reparti de zéro, rempli en plusieurs secondes par
    /// l'asservissement, trous à la lecture.
    #[test]
    fn graine_245_trou_dans_le_son() {
        rejouer(include_str!("../robot-traces/245-son-trou.json"));
    }
}
