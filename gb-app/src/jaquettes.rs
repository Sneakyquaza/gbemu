//! Vignettes libretro-thumbnails (jaquettes Named_Boxarts, écrans titre Named_Titles, captures
//! Named_Snaps ; dépôt GitHub, sinon thumbnails.libretro.com) : téléchargées à la demande pour les jeux reconnus, mises en cache sur disque,
//! décodées et réduites hors du fil de l'interface.

use crate::biblio::Plateforme;
use crate::dat::{agent, encoder_url, perime, systeme};
use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};

/// Âge (jours) au-delà duquel une jaquette notée absente est redemandée.
const JOURS_ABSENT: u64 = 30;

/// Côté maximal d'une jaquette décodée (en pixels).
const COTE_MAX: usize = 384;

pub struct Image {
    pub largeur: usize,
    pub hauteur: usize,
    pub rgba: Vec<u8>,
}

/// Genre de vignette libretro.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Vignette {
    Boite,
    Titre,
    Capture,
}

impl Vignette {
    fn dossier_distant(self) -> &'static str {
        match self {
            Vignette::Boite => "Named_Boxarts",
            Vignette::Titre => "Named_Titles",
            Vignette::Capture => "Named_Snaps",
        }
    }

    /// Sous-dossier du cache (les jaquettes restent à la racine de la plateforme).
    fn sous_dossier(self) -> Option<&'static str> {
        match self {
            Vignette::Boite => None,
            Vignette::Titre => Some("titres"),
            Vignette::Capture => Some("captures"),
        }
    }
}

/// Clé d'une vignette (nom de texture, réponse du fil) : le nom No-Intro seul pour la jaquette.
pub fn cle(v: Vignette, nom: &str) -> String {
    match v {
        Vignette::Boite => nom.to_string(),
        Vignette::Titre => format!("titre:{nom}"),
        Vignette::Capture => format!("capture:{nom}"),
    }
}

type Demande = (Plateforme, String, Vignette);

pub struct Jaquettes {
    file: Arc<(Mutex<VecDeque<Demande>>, Condvar)>,
    pub rx: Receiver<(String, Option<Image>)>,
}

impl Jaquettes {
    pub fn lancer(dossier: PathBuf, reveil: impl Fn() + Send + 'static) -> Jaquettes {
        let file: Arc<(Mutex<VecDeque<Demande>>, Condvar)> = Arc::default();
        let (tx, rx) = channel();
        let f = Arc::clone(&file);
        std::thread::spawn(move || boucle(dossier, f, tx, reveil));
        Jaquettes { file, rx }
    }

    /// Demande une vignette d'un jeu (nom No-Intro) ; les dernières demandes passent en premier.
    /// La réponse porte la clé [`cle`]. Renvoie les clés des demandes trop anciennes abandonnées
    /// (à redemander si elles reviennent à l'écran).
    pub fn demander(&self, p: Plateforme, nom: &str, v: Vignette) -> Vec<String> {
        let (m, cv) = &*self.file;
        let Ok(mut q) = m.lock() else {
            return Vec::new();
        };
        q.push_front((p, nom.to_string(), v));
        cv.notify_one();
        if q.len() > 256 {
            q.split_off(256)
                .into_iter()
                .map(|(_, n, v)| cle(v, &n))
                .collect()
        } else {
            Vec::new()
        }
    }
}

/// Nom de fichier libretro : `&*/:`<>?\|"` remplacés par `_`.
fn nom_fichier(nom: &str) -> String {
    nom.chars()
        .map(|c| if "&*/:`<>?\\|\"".contains(c) { '_' } else { c })
        .collect()
}

fn boucle(
    dossier: PathBuf,
    file: Arc<(Mutex<VecDeque<Demande>>, Condvar)>,
    tx: Sender<(String, Option<Image>)>,
    reveil: impl Fn(),
) {
    let agent = agent();
    loop {
        let (p, nom, v) = {
            let (m, cv) = &*file;
            let Ok(mut q) = m.lock() else { return };
            loop {
                if let Some(d) = q.pop_front() {
                    break d;
                }
                // Propriétaire disparu (appli refermée) : le fil s'arrête.
                if Arc::strong_count(&file) == 1 {
                    return;
                }
                q = match cv.wait_timeout(q, std::time::Duration::from_secs(1)) {
                    Ok((q, _)) => q,
                    Err(_) => return,
                };
            }
        };
        let img = obtenir(&agent, &dossier, p, &nom, v, crate::sauve::hors_ligne());
        if tx.send((cle(v, &nom), img)).is_err() {
            return;
        }
        reveil();
    }
}

/// Fichier du cache d'une vignette, et son marqueur « absent ».
fn chemins_cache(dossier: &Path, p: Plateforme, nom: &str, v: Vignette) -> (PathBuf, PathBuf) {
    let mut local = dossier.join(p.court());
    if let Some(s) = v.sous_dossier() {
        local.push(s);
    }
    let local = local.join(format!("{}.png", nom_fichier(nom)));
    let absent = local.with_extension("absent");
    (local, absent)
}

/// Marqueur « absent » récent. Marqueurs vides (v0.7, écrits pendant une panne de
/// thumbnails.libretro.com) ignorés.
fn absent_recent(absent: &Path) -> bool {
    std::fs::read(absent).is_ok_and(|d| d == MARQUE_ABSENT) && !perime(absent, JOURS_ABSENT)
}

const MARQUE_ABSENT: &[u8] = b"2";

/// Réponse d'un serveur de vignettes.
enum Reponse {
    Png(Vec<u8>),
    /// Lien symbolique du dépôt (GitHub renvoie le nom du fichier visé).
    Lien(String),
    Introuvable,
    Refus,
}

fn demander(agent: &ureq::Agent, url: &str) -> Result<Reponse, ()> {
    match agent.get(url).call() {
        Ok(rep) => {
            let mut d = Vec::new();
            if rep.into_reader().take(8 << 20).read_to_end(&mut d).is_err() {
                return Ok(Reponse::Refus);
            }
            if d.starts_with(b"\x89PNG") {
                return Ok(Reponse::Png(d));
            }
            Ok(match std::str::from_utf8(&d) {
                Ok(t) if d.len() < 512 && t.trim().ends_with(".png") && !t.contains("..") => {
                    Reponse::Lien(t.trim().to_string())
                }
                _ => Reponse::Refus,
            })
        }
        Err(ureq::Error::Status(404, _)) => Ok(Reponse::Introuvable),
        Err(ureq::Error::Status(..)) => Ok(Reponse::Refus),
        Err(_) => Err(()),
    }
}

/// Télécharge la vignette dans le cache : `Ok(Some)` PNG reçu et gardé, `Ok(None)` inexistante
/// (marquée absente) ou refusée, `Err` serveurs injoignables (réseau coupé).
/// Dépôt libretro-thumbnails sur GitHub d'abord (thumbnails.libretro.com vide le 22/11/2026),
/// le serveur libretro ensuite.
fn telecharger(
    agent: &ureq::Agent,
    p: Plateforme,
    nom: &str,
    v: Vignette,
    local: &Path,
    absent: &Path,
) -> Result<Option<Vec<u8>>, ()> {
    let fichier = encoder_url(&nom_fichier(nom));
    let depot = format!(
        "https://raw.githubusercontent.com/libretro-thumbnails/{}/master/{}/",
        systeme(p).replace(' ', "_"),
        v.dossier_distant()
    );
    let serveur = format!(
        "https://thumbnails.libretro.com/{}/{}/",
        encoder_url(systeme(p)),
        v.dossier_distant()
    );
    let mut injoignable = true;
    let mut introuvable = true;
    for base in [&depot, &serveur] {
        let mut rep = demander(agent, &format!("{base}{fichier}.png"));
        if let Ok(Reponse::Lien(cible)) = &rep {
            let cible = cible.rsplit('/').next().unwrap_or(cible);
            rep = demander(agent, &format!("{base}{}", encoder_url(cible)));
        }
        match rep {
            Ok(Reponse::Png(d)) => {
                let _ = crate::sauve::ecrire_atomique(local, &d);
                return Ok(Some(d));
            }
            Ok(Reponse::Introuvable) => injoignable = false,
            Ok(_) => {
                injoignable = false;
                introuvable = false;
            }
            Err(()) => introuvable = false,
        }
    }
    if injoignable {
        return Err(());
    }
    // Introuvable partout : on le note pour ne plus la redemander.
    if introuvable {
        let _ = crate::sauve::ecrire_atomique(absent, MARQUE_ABSENT);
    }
    Ok(None)
}

/// Vignette en cache, sinon téléchargée (jamais `hors_ligne`).
fn obtenir(
    agent: &ureq::Agent,
    dossier: &Path,
    p: Plateforme,
    nom: &str,
    v: Vignette,
    hors_ligne: bool,
) -> Option<Image> {
    let (local, absent) = chemins_cache(dossier, p, nom, v);
    if let Ok(d) = std::fs::read(&local) {
        return decoder(&d);
    }
    if hors_ligne || absent_recent(&absent) {
        return None;
    }
    decoder(&telecharger(agent, p, nom, v, &local, &absent).ok()??)
}

/// Vrai si la vignette reste à demander (ni en cache, ni notée absente récemment).
pub fn manquante(dossier: &Path, p: Plateforme, nom: &str, v: Vignette) -> bool {
    let (local, absent) = chemins_cache(dossier, p, nom, v);
    !local.exists() && !absent_recent(&absent)
}

/// Met la vignette en cache sans la décoder (téléchargement groupé). `force` : redemandée même
/// notée absente. `Ok(true)` si une requête a été faite, `Err` serveur injoignable.
pub fn precharger(
    agent: &ureq::Agent,
    dossier: &Path,
    p: Plateforme,
    nom: &str,
    v: Vignette,
    force: bool,
) -> Result<bool, ()> {
    let (local, absent) = chemins_cache(dossier, p, nom, v);
    if local.exists() || (!force && !manquante(dossier, p, nom, v)) {
        return Ok(false);
    }
    telecharger(agent, p, nom, v, &local, &absent).map(|_| true)
}

/// Décode un PNG en RGBA et le réduit (moyenne par zones) à [`COTE_MAX`]. Jamais de panique.
pub fn decoder(d: &[u8]) -> Option<Image> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(d));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut lecteur = dec.read_info().ok()?;
    let mut tampon = vec![0; lecteur.output_buffer_size()];
    let info = lecteur.next_frame(&mut tampon).ok()?;
    let (l, h) = (info.width as usize, info.height as usize);
    if l == 0 || h == 0 {
        return None;
    }
    let canaux = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        png::ColorType::Indexed => return None,
    };
    let px = |x: usize, y: usize| -> [u32; 4] {
        let i = (y * l + x) * canaux;
        let s = &tampon[i..i + canaux];
        match canaux {
            4 => [s[0] as u32, s[1] as u32, s[2] as u32, s[3] as u32],
            3 => [s[0] as u32, s[1] as u32, s[2] as u32, 255],
            2 => [s[0] as u32, s[0] as u32, s[0] as u32, s[1] as u32],
            _ => [s[0] as u32, s[0] as u32, s[0] as u32, 255],
        }
    };
    let f = (l.max(h) as f32 / COTE_MAX as f32).max(1.0);
    let (nl, nh) = (
        ((l as f32 / f) as usize).max(1),
        ((h as f32 / f) as usize).max(1),
    );
    let mut rgba = Vec::with_capacity(nl * nh * 4);
    for y in 0..nh {
        let (y0, y1) = (y * h / nh, ((y + 1) * h / nh).max(y * h / nh + 1));
        for x in 0..nl {
            let (x0, x1) = (x * l / nl, ((x + 1) * l / nl).max(x * l / nl + 1));
            let mut somme = [0u32; 4];
            for yy in y0..y1.min(h) {
                for xx in x0..x1.min(l) {
                    let p = px(xx, yy);
                    for k in 0..4 {
                        somme[k] += p[k];
                    }
                }
            }
            let n = ((y1.min(h) - y0) * (x1.min(l) - x0)).max(1) as u32;
            rgba.extend(somme.iter().map(|s| (s / n) as u8));
        }
    }
    Some(Image {
        largeur: nl,
        hauteur: nh,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hors_ligne_rien_n_est_demande() {
        let d = crate::sauve::DossierTest::new("jaquettes");
        for v in [Vignette::Boite, Vignette::Titre, Vignette::Capture] {
            assert!(obtenir(&agent(), &d.0, Plateforme::Gb, "Jeu (France)", v, true).is_none());
        }
        // Aucun marqueur « absent » : la jaquette sera demandée une fois en ligne.
        assert!(std::fs::read_dir(&d.0).unwrap().next().is_none());
    }

    #[test]
    fn marqueurs_absents() {
        let d = crate::sauve::DossierTest::new("absents");
        let (_, absent) = chemins_cache(&d.0, Plateforme::Gba, "Jeu (Europe)", Vignette::Titre);
        assert!(manquante(
            &d.0,
            Plateforme::Gba,
            "Jeu (Europe)",
            Vignette::Titre
        ));
        // Marqueur vide de la v0.7 (panne du serveur libretro) : ignoré.
        crate::sauve::ecrire_atomique(&absent, b"").unwrap();
        assert!(manquante(
            &d.0,
            Plateforme::Gba,
            "Jeu (Europe)",
            Vignette::Titre
        ));
        crate::sauve::ecrire_atomique(&absent, MARQUE_ABSENT).unwrap();
        assert!(!manquante(
            &d.0,
            Plateforme::Gba,
            "Jeu (Europe)",
            Vignette::Titre
        ));
    }

    #[test]
    fn vignettes_en_cache_par_genre() {
        let d = crate::sauve::DossierTest::new("vignettes");
        // PNG 1×1 gris posé dans le cache des écrans titre seulement.
        let png = crate::etats::encoder_png(&[90, 90, 90], 1, 1).unwrap();
        let titre = d.0.join("GB").join("titres").join("A_B (Europe).png");
        crate::sauve::ecrire_atomique(&titre, &png).unwrap();
        let a = agent();
        let lire = |v| obtenir(&a, &d.0, Plateforme::Gb, "A&B (Europe)", v, true);
        assert!(lire(Vignette::Titre).is_some());
        assert!(lire(Vignette::Boite).is_none());
        assert!(lire(Vignette::Capture).is_none());
        assert_eq!(cle(Vignette::Boite, "X"), "X");
        assert_ne!(cle(Vignette::Titre, "X"), cle(Vignette::Capture, "X"));
    }
}
