//! Mise à jour de l'appli : annonce `version.json` du site, téléchargement vérifié (sha256) à
//! côté de l'exécutable, échange des fichiers puis relance. Tout le réseau tourne sur d'autres
//! fils ; l'échange et la relance se font sur le fil de l'interface, hors partie.

use serde::Deserialize;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const URL_DEFAUT: &str = "https://gbemu.oryx3.fr/telecharger/version.json";
const SITE: &str = "gbemu.oryx3.fr";
/// Exécutable plus gros : refusé (annonce fausse ou serveur détourné).
const TAILLE_MAX: u64 = 256 << 20;

/// Contenu de `version.json`.
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Annonce {
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub taille: u64,
    #[serde(default)]
    pub notes: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Etat {
    Rien,
    Recherche,
    AJour,
    /// Recherche échouée (message lisible).
    Erreur(String),
    Dispo(Annonce),
    /// Version, octets reçus, total.
    Telechargement(String, u64, u64),
    /// Nouvel exécutable vérifié, prêt à remplacer l'actuel.
    Pret(PathBuf),
    Installation,
    /// Téléchargement ou installation échoués : rien n'a été changé.
    Echec(String),
}

pub struct Maj {
    etat: Arc<Mutex<Etat>>,
    url: String,
    /// URL remplacée (`--maj-url`, essais en local) : liens http acceptés.
    perso: bool,
    /// Exécutable lu au lancement (après un renommage, `current_exe` suivrait le `.ancien`).
    exe: Option<PathBuf>,
    reveil: Arc<dyn Fn() + Send + Sync>,
}

impl Maj {
    pub fn new(url: Option<String>, reveil: impl Fn() + Send + Sync + 'static) -> Maj {
        Maj {
            etat: Arc::new(Mutex::new(Etat::Rien)),
            perso: url.is_some(),
            url: url.unwrap_or_else(|| URL_DEFAUT.to_string()),
            exe: std::env::current_exe().ok(),
            reveil: Arc::new(reveil),
        }
    }

    pub fn etat(&self) -> Etat {
        self.verrou().clone()
    }

    fn verrou(&self) -> std::sync::MutexGuard<'_, Etat> {
        self.etat.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn mettre(&self, e: Etat) {
        *self.verrou() = e;
    }

    /// Lit l'annonce sur un autre fil. `silencieux` (lancement) : un échec ne laisse aucune trace.
    pub fn chercher(&self, silencieux: bool) {
        {
            let mut e = self.verrou();
            if matches!(
                *e,
                Etat::Recherche | Etat::Telechargement(..) | Etat::Pret(_) | Etat::Installation
            ) {
                return;
            }
            *e = Etat::Recherche;
        }
        let (etat, url, r) = (
            Arc::clone(&self.etat),
            self.url.clone(),
            Arc::clone(&self.reveil),
        );
        std::thread::spawn(move || {
            let res = match lire_annonce(&url) {
                Ok(a) if plus_recente(&a.version, VERSION) => Etat::Dispo(a),
                Ok(_) => Etat::AJour,
                Err(_) if silencieux => Etat::Rien,
                Err(m) => Etat::Erreur(m),
            };
            *etat.lock().unwrap_or_else(|e| e.into_inner()) = res;
            r();
        });
    }

    /// Télécharge la version annoncée dans `<exe>.nouveau` (vérifiée), puis état `Pret`.
    pub fn telecharger(&self) {
        let Etat::Dispo(a) = self.etat() else { return };
        let Some(exe) = self.exe.clone() else {
            return self.mettre(Etat::Echec("Emplacement de l'appli introuvable".into()));
        };
        if !self.perso && !a.url.starts_with("https://") {
            return self.mettre(Etat::Echec("Lien de téléchargement refusé".into()));
        }
        self.mettre(Etat::Telechargement(a.version.clone(), 0, a.taille));
        let (etat, r) = (Arc::clone(&self.etat), Arc::clone(&self.reveil));
        std::thread::spawn(move || {
            let nouveau = suffixe(&exe, ".nouveau");
            let mut dernier = 0;
            let res = telecharger_vers(&agent(15), &a, &nouveau, |faits| {
                // Interface réveillée à chaque pour cent reçu, pas à chaque bloc.
                if faits - dernier > a.taille / 100 {
                    dernier = faits;
                    *etat.lock().unwrap_or_else(|e| e.into_inner()) =
                        Etat::Telechargement(a.version.clone(), faits, a.taille);
                    r();
                }
            })
            .and_then(|()| {
                // Mêmes droits que l'actuel (exécutable sous Linux).
                let droits = std::fs::metadata(&exe).map(|m| m.permissions());
                droits
                    .and_then(|d| std::fs::set_permissions(&nouveau, d))
                    .map_err(|_| "Nouvelle version illisible : rien n'a été changé".to_string())
            });
            let fin = match res {
                Ok(()) => Etat::Pret(nouveau),
                Err(m) => {
                    let _ = std::fs::remove_file(&nouveau);
                    Etat::Echec(m)
                }
            };
            *etat.lock().unwrap_or_else(|e| e.into_inner()) = fin;
            r();
        });
    }

    /// Nouvel exécutable prêt : passe en `Installation` et le rend (une seule fois).
    pub fn prendre_pret(&self) -> Option<PathBuf> {
        let mut e = self.verrou();
        let Etat::Pret(p) = &*e else { return None };
        let p = p.clone();
        *e = Etat::Installation;
        Some(p)
    }

    /// Remplace l'exécutable par `nouveau` et lance la nouvelle version avec les mêmes
    /// arguments. Échec : l'exécutable d'origine est remis, message lisible.
    pub fn installer(&self, nouveau: &Path) -> Result<(), String> {
        let res = self.installer_sans_etat(nouveau);
        if let Err(m) = &res {
            let _ = std::fs::remove_file(nouveau);
            self.mettre(Etat::Echec(m.clone()));
        }
        res
    }

    fn installer_sans_etat(&self, nouveau: &Path) -> Result<(), String> {
        let exe = self
            .exe
            .as_deref()
            .ok_or("Emplacement de l'appli introuvable")?;
        remplacer(exe, nouveau)
            .map_err(|_| "Remplacement de l'appli refusé : rien n'a été changé".to_string())?;
        let args = std::env::args_os()
            .skip(1)
            .filter(|a| a != "--maj-auto" && a != "--apres-maj");
        match std::process::Command::new(exe)
            .args(args)
            .arg("--apres-maj")
            .spawn()
        {
            Ok(_) => Ok(()),
            Err(_) => {
                restaurer(exe, nouveau);
                Err("Nouvelle version impossible à lancer : l'ancienne est gardée".into())
            }
        }
    }
}

/// `chemin` + `fin` (`gbemu.exe` → `gbemu.exe.ancien`).
fn suffixe(chemin: &Path, fin: &str) -> PathBuf {
    let mut s = chemin.as_os_str().to_owned();
    s.push(fin);
    PathBuf::from(s)
}

/// Agent HTTP : `lecture` = attente maximale entre deux blocs reçus (s).
fn agent(lecture: u64) -> ureq::Agent {
    let mut a = ureq::AgentBuilder::new();
    // Robot de test : aucune requête ne sort (même règle que `dat::agent`).
    if crate::dat::RESEAU_COUPE.load(std::sync::atomic::Ordering::Relaxed) {
        if let Ok(p) = ureq::Proxy::new("http://127.0.0.1:9") {
            a = a.proxy(p);
        }
    }
    a.timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(lecture))
        .user_agent(&format!("gbemu/{VERSION}"))
        .build()
}

fn lire_annonce(url: &str) -> Result<Annonce, String> {
    let rep = agent(5)
        .get(url)
        .timeout(Duration::from_secs(5))
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(code, _) => format!("Réponse du serveur : erreur {code}"),
            ureq::Error::Transport(_) => "Serveur injoignable : vérifie la connexion".into(),
        })?;
    let mut d = Vec::new();
    rep.into_reader()
        .take(64 << 10)
        .read_to_end(&mut d)
        .map_err(|_| "Réponse du serveur interrompue".to_string())?;
    serde_json::from_slice(&d).map_err(|_| "Réponse du serveur illisible".into())
}

/// Écrit le fichier annoncé dans `dest` en vérifiant taille et empreinte sha256 ; `progres`
/// reçoit les octets déjà reçus. Dossier non inscriptible : message vers le site.
fn telecharger_vers(
    agent: &ureq::Agent,
    a: &Annonce,
    dest: &Path,
    mut progres: impl FnMut(u64),
) -> Result<(), String> {
    if a.taille == 0 || a.taille > TAILLE_MAX || a.sha256.len() != 64 {
        return Err("Annonce de version invalide".into());
    }
    let mut f = std::fs::File::create(dest).map_err(|_| {
        format!("Dossier de l'appli protégé : télécharge la nouvelle version sur {SITE}")
    })?;
    let rep = agent
        .get(&a.url)
        .call()
        .map_err(|_| "Téléchargement impossible : vérifie la connexion".to_string())?;
    let mut lecteur = rep.into_reader().take(a.taille + 1);
    let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
    let mut tampon = vec![0u8; 64 << 10];
    let mut faits = 0u64;
    loop {
        let n = lecteur
            .read(&mut tampon)
            .map_err(|_| "Téléchargement interrompu : réessaie".to_string())?;
        if n == 0 {
            break;
        }
        ctx.update(&tampon[..n]);
        f.write_all(&tampon[..n]).map_err(|_| {
            "Écriture impossible (disque plein ?) : rien n'a été changé".to_string()
        })?;
        faits += n as u64;
        progres(faits);
    }
    let empreinte: String = ctx
        .finish()
        .as_ref()
        .iter()
        .map(|o| format!("{o:02x}"))
        .collect();
    if faits != a.taille || !empreinte.eq_ignore_ascii_case(&a.sha256) {
        return Err("Fichier reçu abîmé (empreinte différente) : rien n'a été changé".into());
    }
    f.sync_all()
        .map_err(|_| "Écriture impossible : rien n'a été changé".to_string())
}

/// `exe` → `exe.ancien`, `nouveau` → `exe` (Windows permet de renommer l'exe en cours).
/// Échec du second renommage : l'exe d'origine est remis.
fn remplacer(exe: &Path, nouveau: &Path) -> std::io::Result<()> {
    let ancien = suffixe(exe, ".ancien");
    let _ = std::fs::remove_file(&ancien);
    std::fs::rename(exe, &ancien)?;
    if let Err(e) = std::fs::rename(nouveau, exe) {
        let _ = std::fs::rename(&ancien, exe);
        return Err(e);
    }
    Ok(())
}

/// Annule [`remplacer`] : la nouvelle version retourne en `nouveau`, l'ancienne reprend sa place.
fn restaurer(exe: &Path, nouveau: &Path) {
    if std::fs::rename(exe, nouveau).is_ok() {
        let _ = std::fs::rename(suffixe(exe, ".ancien"), exe);
    }
}

/// Lancement : supprime en fond l'ancienne version (et un téléchargement interrompu). Juste
/// après une mise à jour, l'ancien processus peut encore tenir son fichier : quelques essais.
pub fn nettoyer() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    std::thread::spawn(move || {
        let _ = std::fs::remove_file(suffixe(&exe, ".nouveau"));
        let ancien = suffixe(&exe, ".ancien");
        for _ in 0..60 {
            if !ancien.exists() || std::fs::remove_file(&ancien).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}

/// `a` plus récente que `b` (« 0.10.0 » > « 0.9.3 », préfixe « v » et suffixe « -… » ignorés).
pub fn plus_recente(a: &str, b: &str) -> bool {
    let nums = |s: &str| -> Vec<u64> {
        let s = s.trim().trim_start_matches('v');
        let s = s.split(['-', '+']).next().unwrap_or("");
        s.split('.').map(|p| p.parse().unwrap_or(0)).collect()
    };
    let (mut x, mut y) = (nums(a), nums(b));
    let n = x.len().max(y.len());
    x.resize(n, 0);
    y.resize(n, 0);
    x > y
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sauve::DossierTest;

    #[test]
    fn comparaison_des_versions() {
        assert!(plus_recente("0.10.0", "0.9.0"));
        assert!(plus_recente("v1.0", "0.99.9"));
        assert!(plus_recente("0.10.1", "0.10"));
        assert!(!plus_recente("0.10.0", "0.10.0"));
        assert!(!plus_recente("0.9.9", "0.10.0"));
        assert!(!plus_recente("0.10.0-beta", "0.10.0"));
        assert!(!plus_recente("n'importe quoi", "0.1.0"));
    }

    #[test]
    fn remplacement_et_retour() {
        let d = DossierTest::new("maj-remplacer");
        let exe = d.0.join("gbemu.exe");
        let nouveau = suffixe(&exe, ".nouveau");
        std::fs::write(&exe, b"ancien").unwrap();
        std::fs::write(&nouveau, b"nouveau").unwrap();
        remplacer(&exe, &nouveau).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"nouveau");
        assert_eq!(std::fs::read(suffixe(&exe, ".ancien")).unwrap(), b"ancien");
        restaurer(&exe, &nouveau);
        assert_eq!(std::fs::read(&exe).unwrap(), b"ancien");
        // Nouveau fichier absent : l'exe d'origine reste en place.
        assert!(remplacer(&exe, &d.0.join("absent")).is_err());
        assert_eq!(std::fs::read(&exe).unwrap(), b"ancien");
    }

    /// Serveur HTTP local d'une seule réponse.
    fn servir(corps: Vec<u8>) -> String {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let adr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut t = [0u8; 4096];
                let _ = s.read(&mut t);
                let tete = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    corps.len()
                );
                let _ = s.write_all(tete.as_bytes());
                let _ = s.write_all(&corps);
            }
        });
        format!("http://{adr}/gbemu.exe")
    }

    #[test]
    fn telechargement_verifie() {
        let d = DossierTest::new("maj-telecharger");
        let corps = b"nouvelle version".to_vec();
        let sha = "a3fa7e3dd1ae1bd86c2b1d6b46ee8ec5ea27b3fb6dd5a6dc5bfe1f7cf6b5e0a8";
        let vrai: String = ring::digest::digest(&ring::digest::SHA256, &corps)
            .as_ref()
            .iter()
            .map(|o| format!("{o:02x}"))
            .collect();
        let annonce = |url: String, sha256: &str| Annonce {
            version: "9.0.0".into(),
            url,
            sha256: sha256.into(),
            taille: corps.len() as u64,
            notes: String::new(),
        };
        let dest = d.0.join("gbemu.exe.nouveau");
        // Sans `agent()` : le robot de test peut couper le réseau pendant ce test.
        let local = ureq::AgentBuilder::new().build();
        let mut vus = 0;
        let a = annonce(servir(corps.clone()), &vrai);
        telecharger_vers(&local, &a, &dest, |n| vus = n).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), corps);
        assert_eq!(vus, corps.len() as u64);
        // Empreinte fausse : refusé.
        let a = annonce(servir(corps.clone()), sha);
        let e = telecharger_vers(&local, &a, &dest, |_| {}).unwrap_err();
        assert!(e.contains("empreinte"), "{e}");
        // Dossier absent (non inscriptible) : renvoi vers le site.
        let a = annonce(servir(corps.clone()), &vrai);
        let e = telecharger_vers(&local, &a, &d.0.join("non/inscriptible"), |_| {}).unwrap_err();
        assert!(e.contains(SITE), "{e}");
    }
}
