//! Téléchargement groupé des infos et images des jeux ajoutés : jaquettes, écrans titre et
//! captures libretro-thumbnails, fiche ScreenScraper si des identifiants sont actifs.
//! Un seul fil ; file d'attente gardée sur disque (reprise au lancement suivant), annulable,
//! suspendue en mode hors ligne ou serveur injoignable. Au plus ~5 requêtes/s vers libretro,
//! cadence ScreenScraper partagée avec les fiches ouvertes.

use crate::biblio::Plateforme;
use crate::jaquettes::{self, Vignette};
use crate::screenscraper::{self, Demande, Identifiants, Limiteur};
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// Pause après chaque requête libretro (≤ 5 requêtes/s).
const PAUSE: Duration = Duration::from_millis(200);
/// Attente avant de réessayer quand le serveur est injoignable (doublée à chaque échec suivant).
const PAUSE_RESEAU: Duration = Duration::from_secs(30);
/// Échecs d'un même jeu avant qu'il passe en fin de file (pas de blocage en tête).
const ESSAIS: u32 = 3;
/// Intervalle minimal entre deux enregistrements de la file pendant un lot (cache : ~1 Mo pour
/// 5 200 jeux).
const INTERVALLE_ENREGISTREMENT: Duration = Duration::from_secs(30);

/// Un jeu à compléter.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Tache {
    pub crc: u32,
    pub taille: u64,
    pub plateforme: Plateforme,
    /// Nom No-Intro (vide : jeu non reconnu, ScreenScraper seulement).
    pub nom: String,
    /// Nom du fichier de la ROM (aide ScreenScraper).
    pub fichier: String,
    /// Demandé depuis la fiche : images notées absentes redemandées.
    #[serde(default)]
    pub force: bool,
}

/// File d'attente (enregistrée telle quelle, jamais d'identifiant dedans).
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct File {
    pub a_faire: VecDeque<Tache>,
    /// Faits depuis le début du lot (barre de progression).
    pub faits: usize,
    pub total: usize,
    /// Jeu en cours de traitement par le fil.
    #[serde(skip)]
    en_cours: Option<u32>,
    /// Jeu en cours redemandé depuis la fiche entre-temps : gardé en file à la fin du traitement.
    #[serde(skip)]
    relance: bool,
}

impl File {
    /// Ajoute les jeux absents de la file ; renvoie le nombre ajouté.
    pub fn ajouter(&mut self, taches: Vec<Tache>) -> usize {
        let mut vus: HashSet<u32> = self.a_faire.iter().map(|x| x.crc).collect();
        let mut n = 0;
        for t in taches {
            if vus.insert(t.crc) {
                self.a_faire.push_back(t);
                n += 1;
            }
        }
        self.total += n;
        n
    }

    /// Place le jeu en tête (forcé), qu'il soit déjà dans la file ou non.
    pub fn prioriser(&mut self, mut t: Tache) {
        t.force = true;
        if self.en_cours == Some(t.crc) {
            self.relance = true;
        }
        match self.a_faire.iter().position(|x| x.crc == t.crc) {
            Some(i) => {
                self.a_faire.remove(i);
            }
            None => self.total += 1,
        }
        self.a_faire.push_front(t);
    }

    /// Jeu de tête, noté en cours de traitement.
    fn commencer(&mut self) -> Option<Tache> {
        let t = self.a_faire.front()?.clone();
        self.en_cours = Some(t.crc);
        self.relance = false;
        Some(t)
    }

    /// Retire un jeu traité (sauf s'il a été redemandé pendant son traitement) ; lot fini :
    /// compteurs remis à zéro.
    pub fn terminer(&mut self, crc: u32) {
        let relance = self.en_cours == Some(crc) && self.relance;
        self.abandonner();
        if relance {
            return;
        }
        if let Some(i) = self.a_faire.iter().position(|x| x.crc == crc) {
            self.a_faire.remove(i);
            self.faits = self.faits.saturating_add(1);
        }
        if self.a_faire.is_empty() {
            self.faits = 0;
            self.total = 0;
        }
    }

    /// Traitement interrompu (réseau) : le jeu reste en file.
    fn abandonner(&mut self) {
        self.en_cours = None;
        self.relance = false;
    }

    /// Jeu en échec répété : passe en fin de file (les suivants ne restent pas bloqués).
    fn repousser(&mut self, crc: u32) {
        self.abandonner();
        if let Some(t) = self
            .a_faire
            .iter()
            .position(|x| x.crc == crc)
            .and_then(|i| self.a_faire.remove(i))
        {
            self.a_faire.push_back(t);
        }
    }

    /// File vidée ; le jeu en cours reste noté (redemandé entre-temps : il ne sera pas perdu).
    pub fn annuler(&mut self) {
        *self = File {
            en_cours: self.en_cours,
            ..File::default()
        };
    }

    /// (faits, total) pendant un lot.
    pub fn progres(&self) -> Option<(usize, usize)> {
        (!self.a_faire.is_empty())
            .then(|| (self.faits, self.total.max(self.faits.saturating_add(1))))
    }
}

/// Jeu traité (ses vignettes et sa fiche sont à relire depuis le cache).
pub struct Fait {
    pub crc: u32,
    pub nom: String,
}

struct Etat {
    /// Partagée avec l'enregistrement en cours (copie seulement si elle change entre-temps).
    file: Arc<File>,
    id: Identifiants,
    /// Version de la file (+1 à chaque changement) et dernière version enregistrée.
    version: u64,
    ecrite: u64,
    /// Changement à enregistrer sans attendre (ajout, annulation).
    urgent: bool,
    /// Propriétaire disparu (appli refermée) : le fil enregistre la file et s'arrête.
    fin: bool,
}

impl Etat {
    fn changee(&mut self) {
        self.version += 1;
    }

    fn file(&mut self) -> &mut File {
        Arc::make_mut(&mut self.file)
    }
}

/// État partagé avec le fil ; le 3e verrou range les enregistrements (jamais une version plus
/// ancienne par-dessus une plus récente), il est pris avant celui de l'état.
type Partage = (Mutex<Etat>, Condvar, Mutex<()>);

pub struct Telechargeur {
    partage: Arc<Partage>,
    chemin: PathBuf,
    pub rx: Receiver<Fait>,
}

/// Dossiers du cache où vont les images.
pub struct Dossiers {
    pub file: PathBuf,
    pub jaquettes: PathBuf,
    pub screenscraper: PathBuf,
}

impl Drop for Telechargeur {
    fn drop(&mut self) {
        let (m, cv, _) = &*self.partage;
        if let Ok(mut e) = m.lock() {
            e.fin = true;
        }
        cv.notify_all();
        // Fermeture : la file est enregistrée tout de suite (le fil finit peut-être une requête).
        enregistrer(&self.partage, &self.chemin);
    }
}

impl Telechargeur {
    /// Reprend la file enregistrée (s'il en reste) et lance le fil. `hors_ligne` : lu avant
    /// chaque jeu (`sauve::hors_ligne` dans l'appli).
    pub fn lancer(
        d: Dossiers,
        lim: Arc<Mutex<Limiteur>>,
        hors_ligne: fn() -> bool,
        reveil: impl Fn() + Send + 'static,
    ) -> Telechargeur {
        let file: File = crate::sauve::lire_json(&d.file).unwrap_or_default();
        let partage = Arc::new((
            Mutex::new(Etat {
                file: Arc::new(file),
                id: Identifiants::default(),
                version: 0,
                ecrite: 0,
                urgent: false,
                fin: false,
            }),
            Condvar::new(),
            Mutex::new(()),
        ));
        let (tx, rx) = channel();
        let p = Arc::clone(&partage);
        let chemin = d.file.clone();
        std::thread::spawn(move || {
            let agent = crate::dat::agent();
            let mut enregistre = Instant::now();
            // Échecs de suite (attente croissante) et échecs du jeu de tête.
            let mut echecs = 0u32;
            let mut essais = (0u32, 0u32);
            loop {
                let (m, cv, _) = &*p;
                let Ok(mut e) = m.lock() else { return };
                if e.fin {
                    drop(e);
                    enregistrer(&p, &d.file);
                    return;
                }
                let inactif = e.file.a_faire.is_empty() || hors_ligne();
                if e.version != e.ecrite
                    && (e.urgent || inactif || enregistre.elapsed() >= INTERVALLE_ENREGISTREMENT)
                {
                    // Écriture hors du verrou : `progres()` n'attend jamais le disque.
                    drop(e);
                    enregistrer(&p, &d.file);
                    enregistre = Instant::now();
                    continue;
                }
                let Some(t) = (!inactif).then(|| e.file().commencer()).flatten() else {
                    // File vide ou hors ligne : on attend (le réglage est relu de temps en temps).
                    let _ = cv.wait_timeout(e, Duration::from_secs(5));
                    continue;
                };
                let id = e.id.clone();
                drop(e);
                let ok = traiter(&agent, &d, &lim, &t, &id, hors_ligne);
                let Ok(mut e) = m.lock() else { return };
                if !ok {
                    e.file().abandonner();
                    // Passé hors ligne pendant le jeu : pas un échec, il reste en tête.
                    if e.fin || hors_ligne() {
                        continue;
                    }
                    // Réseau coupé ou serveur limité : nouvel essai plus tard (ou annulation),
                    // le jeu passe en fin de file après `ESSAIS` échecs.
                    echecs = echecs.saturating_add(1);
                    essais = if essais.0 == t.crc {
                        (t.crc, essais.1 + 1)
                    } else {
                        (t.crc, 1)
                    };
                    if essais.1 >= ESSAIS {
                        e.file().repousser(t.crc);
                        e.changee();
                        essais = (0, 0);
                    }
                    let _ = cv.wait_timeout(e, attente(echecs));
                    continue;
                }
                echecs = 0;
                e.file().terminer(t.crc);
                e.changee();
                drop(e);
                if tx
                    .send(Fait {
                        crc: t.crc,
                        nom: t.nom,
                    })
                    .is_err()
                {
                    enregistrer(&p, &d.file);
                    return;
                }
                reveil();
            }
        });
        Telechargeur {
            partage,
            chemin,
            rx,
        }
    }

    fn avec<R>(&self, f: impl FnOnce(&mut Etat) -> R) -> Option<R> {
        let (m, cv, _) = &*self.partage;
        let mut e = m.lock().ok()?;
        let r = f(&mut e);
        cv.notify_one();
        Some(r)
    }

    /// Enregistrement fait par le fil, sans attendre le disque ici.
    pub fn ajouter(&self, taches: Vec<Tache>) -> usize {
        self.avec(|e| {
            let n = e.file().ajouter(taches);
            e.changee();
            e.urgent = true;
            n
        })
        .unwrap_or(0)
    }

    /// Un jeu seul, tout de suite (bouton de la fiche).
    pub fn prioriser(&self, t: Tache) {
        self.avec(|e| {
            e.file().prioriser(t);
            e.changee();
        });
    }

    pub fn annuler(&self) {
        self.avec(|e| {
            e.file().annuler();
            e.changee();
            e.urgent = true;
        });
    }

    /// Identifiants ScreenScraper en vigueur (jamais écrits dans la file).
    pub fn identifiants(&self, id: Identifiants) {
        self.avec(|e| e.id = id);
    }

    pub fn progres(&self) -> Option<(usize, usize)> {
        let (m, _, _) = &*self.partage;
        m.lock().ok()?.file.progres()
    }

    /// Vrai si le jeu attend encore son téléchargement.
    pub fn en_attente(&self, crc: u32) -> bool {
        let (m, _, _) = &*self.partage;
        m.lock()
            .is_ok_and(|e| e.file.a_faire.iter().any(|t| t.crc == crc))
    }
}

/// Attente avant le nouvel essai après `echecs` échecs de suite : 30 s, 1 min, 2 min… 8 min.
fn attente(echecs: u32) -> Duration {
    PAUSE_RESEAU * (1 << echecs.saturating_sub(1).min(4))
}

/// Enregistre la file si elle a changé : écriture hors du verrou (cache recréable : sans `.bak`).
fn enregistrer(p: &Partage, chemin: &std::path::Path) {
    let (m, _, ordre) = p;
    let _ordre = ordre.lock().unwrap_or_else(|e| e.into_inner());
    let (file, version) = {
        let Ok(mut e) = m.lock() else { return };
        if e.version == e.ecrite {
            return;
        }
        e.urgent = false;
        (Arc::clone(&e.file), e.version)
    };
    // Copie relâchée avant le disque : un changement pendant l'écriture ne copie pas la file.
    let j = serde_json::to_vec(&*file);
    drop(file);
    if let Ok(j) = j {
        let _ = crate::sauve::ecrire_cache(chemin, &j);
    }
    if let Ok(mut e) = m.lock() {
        e.ecrite = version;
    }
}

/// Vignettes puis fiche ScreenScraper d'un jeu. Faux si le serveur est injoignable ou limité,
/// ou si le mode hors ligne est passé (relu avant chaque vignette).
fn traiter(
    agent: &ureq::Agent,
    d: &Dossiers,
    lim: &Mutex<Limiteur>,
    t: &Tache,
    id: &Identifiants,
    hors_ligne: fn() -> bool,
) -> bool {
    if !t.nom.is_empty() {
        for v in [Vignette::Boite, Vignette::Titre, Vignette::Capture] {
            if hors_ligne() {
                return false;
            }
            match jaquettes::precharger(agent, &d.jaquettes, t.plateforme, &t.nom, v, t.force) {
                Ok(true) => std::thread::sleep(PAUSE),
                Ok(false) => {}
                Err(()) => return false,
            }
        }
    }
    if id.actif() {
        if t.force {
            screenscraper::oublier_absent(&d.screenscraper, t.plateforme, t.crc);
        }
        let Ok(mut l) = lim.lock() else { return true };
        let demande = Demande {
            crc: t.crc,
            taille: t.taille,
            plateforme: t.plateforme,
            nom: t.fichier.clone(),
            id: id.clone(),
        };
        // Quota du jour épuisé, fiche absente… : le jeu est compté comme traité.
        let _ = screenscraper::obtenir(agent, &d.screenscraper, &demande, &mut l, hors_ligne());
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tache(crc: u32) -> Tache {
        Tache {
            crc,
            taille: 32768,
            plateforme: Plateforme::Gb,
            nom: format!("Jeu {crc} (France)"),
            fichier: format!("jeu{crc}.gb"),
            force: false,
        }
    }

    #[test]
    fn file_ajout_priorite_fin() {
        let mut f = File::default();
        assert_eq!(f.progres(), None);
        assert_eq!(f.ajouter(vec![tache(1), tache(2), tache(1)]), 2);
        assert_eq!(f.ajouter(vec![tache(2), tache(3)]), 1);
        assert_eq!(f.progres(), Some((0, 3)));
        // Jeu déjà en file : passe en tête, forcé, sans changer le total.
        f.prioriser(tache(3));
        assert_eq!(f.a_faire.front().map(|t| (t.crc, t.force)), Some((3, true)));
        assert_eq!(f.progres(), Some((0, 3)));
        // Jeu absent de la file : ajouté en tête, total +1.
        f.prioriser(tache(9));
        assert_eq!(f.progres(), Some((0, 4)));
        f.terminer(9);
        f.terminer(9);
        assert_eq!(f.progres(), Some((1, 4)));
        for c in [3, 1, 2] {
            f.terminer(c);
        }
        assert_eq!((f.progres(), f.faits, f.total), (None, 0, 0));
        f.ajouter(vec![tache(5)]);
        f.annuler();
        assert!(f.a_faire.is_empty() && f.progres().is_none());
    }

    #[test]
    fn demande_forcee_pendant_le_traitement_gardee() {
        let mut f = File::default();
        f.ajouter(vec![tache(1), tache(2)]);
        assert_eq!(f.commencer().map(|t| t.crc), Some(1));
        // Clic « Télécharger les infos » sur ce jeu pendant son traitement : il reste, forcé.
        f.prioriser(tache(1));
        f.terminer(1);
        assert_eq!(f.a_faire.front().map(|t| (t.crc, t.force)), Some((1, true)));
        assert_eq!(f.progres(), Some((0, 2)));
        // 2e passage sans nouveau clic : retiré.
        f.commencer();
        f.terminer(1);
        assert_eq!(f.progres(), Some((1, 2)));
        // Annulation puis nouveau clic pendant le traitement : gardé aussi.
        f.commencer();
        f.annuler();
        f.prioriser(tache(2));
        f.terminer(2);
        assert_eq!(f.a_faire.len(), 1);
        // Autre jeu forcé pendant le traitement : le jeu traité est bien retiré.
        f.commencer();
        f.prioriser(tache(3));
        f.terminer(2);
        assert_eq!(f.a_faire.iter().map(|t| t.crc).collect::<Vec<_>>(), [3]);
    }

    #[test]
    fn echecs_repetes_en_fin_de_file_attente_croissante() {
        let mut f = File::default();
        f.ajouter(vec![tache(1), tache(2), tache(3)]);
        f.commencer();
        f.repousser(1);
        let ordre: Vec<u32> = f.a_faire.iter().map(|t| t.crc).collect();
        assert_eq!(ordre, [2, 3, 1]);
        assert_eq!(f.progres(), Some((0, 3)));
        let a: Vec<u64> = (1..=7).map(|n| attente(n).as_secs()).collect();
        assert_eq!(a, [30, 60, 120, 240, 480, 480, 480]);
        assert_eq!(attente(u32::MAX).as_secs(), 480);
    }

    #[test]
    fn compteurs_abimes_sans_panique() {
        let mut f: File =
            serde_json::from_str(&format!("{{\"faits\": {}, \"total\": 0}}", usize::MAX)).unwrap();
        assert_eq!(f.progres(), None);
        f.a_faire.push_back(tache(1));
        assert_eq!(f.progres(), Some((usize::MAX, usize::MAX)));
        f.a_faire.push_back(tache(2));
        f.terminer(1);
        assert_eq!(f.faits, usize::MAX);
    }

    #[test]
    fn grande_file_enregistree_par_le_fil_sans_bak() {
        let d = crate::sauve::DossierTest::new("grande_file");
        let chemin = d.0.join("telechargements.json");
        let dossiers = Dossiers {
            file: chemin.clone(),
            jaquettes: d.0.join("jaquettes"),
            screenscraper: d.0.join("screenscraper"),
        };
        let t = Telechargeur::lancer(dossiers, Arc::default(), || true, || {});
        let taches: Vec<Tache> = (0..5200).map(tache).collect();
        let debut = Instant::now();
        assert_eq!(t.ajouter(taches.clone()), 5200);
        assert_eq!(t.ajouter(taches), 0);
        // Dédoublonnage linéaire, aucune écriture sur ce fil (build debug compris).
        assert!(debut.elapsed() < Duration::from_secs(1));
        assert_eq!(t.progres(), Some((0, 5200)));
        drop(t);
        let relue: File = crate::sauve::lire_json(&chemin).unwrap();
        assert_eq!(relue.a_faire.len(), 5200);
        assert!(!chemin.with_extension("bak").exists());
    }

    #[test]
    fn reprise_apres_redemarrage_et_hors_ligne() {
        let d = crate::sauve::DossierTest::new("telechargement");
        let dossiers = || Dossiers {
            file: d.0.join("telechargements.json"),
            jaquettes: d.0.join("jaquettes"),
            screenscraper: d.0.join("screenscraper"),
        };
        // Hors ligne : rien n'est consommé ni demandé au réseau.
        let t = Telechargeur::lancer(dossiers(), Arc::default(), || true, || {});
        assert_eq!(t.ajouter(vec![tache(1), tache(2)]), 2);
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(t.progres(), Some((0, 2)));
        assert!(t.en_attente(2));
        drop(t);
        // « Redémarrage » : la file est relue depuis le disque.
        let t = Telechargeur::lancer(dossiers(), Arc::default(), || true, || {});
        assert_eq!(t.progres(), Some((0, 2)));
        t.annuler();
        assert_eq!(t.progres(), None);
        // Enregistrée par le fil, au plus tard à la fermeture.
        drop(t);
        let relue: File = crate::sauve::lire_json(&dossiers().file).unwrap();
        assert!(relue.a_faire.is_empty());
        assert!(!d.0.join("jaquettes").exists());
    }

    #[test]
    fn identifiants_jamais_dans_la_file() {
        let mut f = File::default();
        f.ajouter(vec![tache(7)]);
        let j = serde_json::to_string(&f).unwrap();
        assert!(!j.contains("devpassword") && !j.contains("mot_de_passe"));
        let relue: File = serde_json::from_str(&j).unwrap();
        assert_eq!(relue.a_faire[0], tache(7));
        // Fichier abîmé : file vide, sans panique.
        assert!(serde_json::from_str::<File>("{\"a_faire\": 3}").is_err());
    }
}
