//! Téléchargement groupé des infos et images des jeux ajoutés : jaquettes, écrans titre et
//! captures libretro-thumbnails, fiche ScreenScraper si des identifiants sont actifs.
//! Un seul fil ; file d'attente gardée sur disque (reprise au lancement suivant), annulable,
//! suspendue en mode hors ligne ou serveur injoignable. Au plus ~5 requêtes/s vers libretro,
//! cadence ScreenScraper partagée avec les fiches ouvertes.

use crate::biblio::Plateforme;
use crate::jaquettes::{self, Vignette};
use crate::screenscraper::{self, Demande, Identifiants, Limiteur};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// Pause après chaque requête libretro (≤ 5 requêtes/s).
const PAUSE: Duration = Duration::from_millis(200);
/// Attente avant de réessayer quand le serveur est injoignable.
const PAUSE_RESEAU: Duration = Duration::from_secs(30);

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
#[derive(Serialize, Deserialize, Default, Debug)]
#[serde(default)]
pub struct File {
    pub a_faire: VecDeque<Tache>,
    /// Faits depuis le début du lot (barre de progression).
    pub faits: usize,
    pub total: usize,
}

impl File {
    /// Ajoute les jeux absents de la file ; renvoie le nombre ajouté.
    pub fn ajouter(&mut self, taches: Vec<Tache>) -> usize {
        let mut n = 0;
        for t in taches {
            if !self.a_faire.iter().any(|x| x.crc == t.crc) {
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
        match self.a_faire.iter().position(|x| x.crc == t.crc) {
            Some(i) => {
                self.a_faire.remove(i);
            }
            None => self.total += 1,
        }
        self.a_faire.push_front(t);
    }

    /// Retire un jeu traité ; lot fini : compteurs remis à zéro.
    pub fn terminer(&mut self, crc: u32) {
        if let Some(i) = self.a_faire.iter().position(|x| x.crc == crc) {
            self.a_faire.remove(i);
            self.faits += 1;
        }
        if self.a_faire.is_empty() {
            self.faits = 0;
            self.total = 0;
        }
    }

    pub fn annuler(&mut self) {
        *self = File::default();
    }

    /// (faits, total) pendant un lot.
    pub fn progres(&self) -> Option<(usize, usize)> {
        (!self.a_faire.is_empty()).then_some((self.faits, self.total.max(self.faits + 1)))
    }
}

/// Jeu traité (ses vignettes et sa fiche sont à relire depuis le cache).
pub struct Fait {
    pub crc: u32,
    pub nom: String,
}

struct Etat {
    file: File,
    id: Identifiants,
    /// File modifiée depuis le dernier enregistrement.
    sale: bool,
    /// Propriétaire disparu (appli refermée) : le fil enregistre la file et s'arrête.
    fin: bool,
}

pub struct Telechargeur {
    partage: Arc<(Mutex<Etat>, Condvar)>,
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
        let (m, cv) = &*self.partage;
        if let Ok(mut e) = m.lock() {
            e.fin = true;
        }
        cv.notify_all();
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
                file,
                id: Identifiants::default(),
                sale: false,
                fin: false,
            }),
            Condvar::new(),
        ));
        let (tx, rx) = channel();
        let p = Arc::clone(&partage);
        let chemin = d.file.clone();
        std::thread::spawn(move || {
            let agent = crate::dat::agent();
            let mut enregistre = Instant::now();
            loop {
                let (m, cv) = &*p;
                let Ok(mut e) = m.lock() else { return };
                if e.fin {
                    if e.sale {
                        enregistrer(&d.file, &e.file);
                    }
                    return;
                }
                if e.sale && enregistre.elapsed() >= Duration::from_secs(3) {
                    enregistrer(&d.file, &e.file);
                    e.sale = false;
                    enregistre = Instant::now();
                }
                let t = match e.file.a_faire.front() {
                    Some(t) if !hors_ligne() => t.clone(),
                    // File vide ou hors ligne : on attend (le réglage est relu de temps en temps).
                    _ => {
                        if e.sale {
                            enregistrer(&d.file, &e.file);
                            e.sale = false;
                        }
                        let _ = cv.wait_timeout(e, Duration::from_secs(5));
                        continue;
                    }
                };
                let id = e.id.clone();
                drop(e);
                let ok = traiter(&agent, &d, &lim, &t, &id, hors_ligne());
                let Ok(mut e) = m.lock() else { return };
                if !ok {
                    // Réseau coupé : le jeu reste en tête, nouvel essai plus tard (ou annulation).
                    if !e.fin {
                        let _ = cv.wait_timeout(e, PAUSE_RESEAU);
                    }
                    continue;
                }
                e.file.terminer(t.crc);
                e.sale = true;
                drop(e);
                if tx
                    .send(Fait {
                        crc: t.crc,
                        nom: t.nom,
                    })
                    .is_err()
                {
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
        let (m, cv) = &*self.partage;
        let mut e = m.lock().ok()?;
        let r = f(&mut e);
        cv.notify_one();
        Some(r)
    }

    pub fn ajouter(&self, taches: Vec<Tache>) -> usize {
        let chemin = self.chemin.clone();
        self.avec(|e| {
            let n = e.file.ajouter(taches);
            enregistrer(&chemin, &e.file);
            n
        })
        .unwrap_or(0)
    }

    /// Un jeu seul, tout de suite (bouton de la fiche).
    pub fn prioriser(&self, t: Tache) {
        self.avec(|e| {
            e.file.prioriser(t);
            e.sale = true;
        });
    }

    pub fn annuler(&self) {
        let chemin = self.chemin.clone();
        self.avec(|e| {
            e.file.annuler();
            enregistrer(&chemin, &e.file);
        });
    }

    /// Identifiants ScreenScraper en vigueur (jamais écrits dans la file).
    pub fn identifiants(&self, id: Identifiants) {
        self.avec(|e| e.id = id);
    }

    pub fn progres(&self) -> Option<(usize, usize)> {
        let (m, _) = &*self.partage;
        m.lock().ok()?.file.progres()
    }

    /// Vrai si le jeu attend encore son téléchargement.
    pub fn en_attente(&self, crc: u32) -> bool {
        let (m, _) = &*self.partage;
        m.lock()
            .is_ok_and(|e| e.file.a_faire.iter().any(|t| t.crc == crc))
    }
}

fn enregistrer(chemin: &std::path::Path, f: &File) {
    if let Ok(j) = serde_json::to_vec(f) {
        let _ = crate::sauve::ecrire_atomique(chemin, &j);
    }
}

/// Vignettes puis fiche ScreenScraper d'un jeu. Faux si le serveur est injoignable.
fn traiter(
    agent: &ureq::Agent,
    d: &Dossiers,
    lim: &Mutex<Limiteur>,
    t: &Tache,
    id: &Identifiants,
    hors_ligne: bool,
) -> bool {
    if !t.nom.is_empty() {
        for v in [Vignette::Boite, Vignette::Titre, Vignette::Capture] {
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
        let _ = screenscraper::obtenir(agent, &d.screenscraper, &demande, &mut l, hors_ligne);
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
