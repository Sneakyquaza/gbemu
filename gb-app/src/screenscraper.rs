//! Client ScreenScraper (API v2, `jeuInfos.php`) : résumé (français, sinon anglais), note, logo
//! (wheel) et jaquette d'un jeu, cherché par CRC + taille + système. Demandé jeu par jeu (fiche
//! ouverte), un seul fil (≤ `maxthreads`), cadence bornée par les quotas que renvoie le serveur,
//! résultats mis en cache. Coupé en mode hors ligne et sans identifiants développeur.
//!
//! Identifiants : jamais journalisés, jamais écrits ailleurs que dans `screenscraper.json`
//! (données de l'app), envoyés seulement à `*.screenscraper.fr` en HTTPS.

use crate::biblio::Plateforme;
use crate::dat::{agent, encoder_url, perime};
use crate::jaquettes::{decoder, Image};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::VecDeque;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const API: &str = "https://api.screenscraper.fr/api2/jeuInfos.php";
const LOGICIEL: &str = "gbemu";
/// Âge (jours) au-delà duquel un jeu introuvable est redemandé.
const JOURS_ABSENT: u64 = 30;
/// Fichier des identifiants développeur, posé par l'utilisateur (jamais dans le dépôt).
pub const FICHIER_DEV: &str = "screenscraper-dev.json";
/// Fichier des identifiants saisis dans les réglages (dossier des données).
pub const FICHIER: &str = "screenscraper.json";

/// Identifiant du système chez ScreenScraper.
pub fn systeme_id(p: Plateforme) -> u32 {
    match p {
        Plateforme::Gb => 9,
        Plateforme::Gbc => 10,
        Plateforme::Gba => 12,
    }
}

/// Identifiants (compte utilisateur facultatif, identifiants développeur obligatoires).
/// Pas de `Debug` : rien ne doit pouvoir les afficher par mégarde.
#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Identifiants {
    pub utilisateur: String,
    pub mot_de_passe: String,
    pub devid: String,
    pub devpassword: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct FichierDev {
    devid: String,
    devpassword: String,
}

impl Identifiants {
    /// Identifiants saisis (`screenscraper.json`), identifiants développeur de
    /// `screenscraper-dev.json` en priorité (dossier des données, sinon à côté de l'exe).
    pub fn charger(donnees: &Path) -> Identifiants {
        let mut id = Identifiants::saisis(donnees);
        let exe = std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|p| p.join(FICHIER_DEV)));
        let dev = std::iter::once(donnees.join(FICHIER_DEV))
            .chain(exe)
            .find_map(|c| crate::sauve::lire_json::<FichierDev>(&c))
            .filter(|d| !d.devid.trim().is_empty() && !d.devpassword.is_empty());
        if let Some(d) = dev {
            id.devid = d.devid.trim().to_string();
            id.devpassword = d.devpassword;
        }
        id
    }

    /// Identifiants saisis dans les réglages seulement (ceux qu'on réenregistre).
    pub fn saisis(donnees: &Path) -> Identifiants {
        crate::sauve::lire_json(&donnees.join(FICHIER)).unwrap_or_default()
    }

    /// Vrai si les identifiants développeur viennent de `screenscraper-dev.json`.
    pub fn dev_de_fichier(&self, saisis: &Identifiants) -> bool {
        self.actif()
            && (self.devid != saisis.devid.trim() || self.devpassword != saisis.devpassword)
    }

    /// Écrit les identifiants (lisibles par l'utilisateur seul sous Linux).
    pub fn enregistrer(&self, donnees: &Path) -> std::io::Result<()> {
        let chemin = donnees.join(FICHIER);
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        crate::sauve::ecrire_atomique(&chemin, &json)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let _ = std::fs::set_permissions(&chemin, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    /// Sans identifiants développeur, aucune requête n'est faite.
    pub fn actif(&self) -> bool {
        !self.devid.trim().is_empty() && !self.devpassword.is_empty()
    }
}

/// Ce que l'app garde d'un jeu (sans aucune URL : celles de ScreenScraper portent les
/// identifiants).
#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
#[serde(default)]
pub struct FicheSs {
    pub titre: String,
    pub synopsis: String,
    /// Langue du résumé (« fr » ou « en »).
    pub langue: String,
    /// Note sur 20.
    pub note: Option<u8>,
    pub editeur: String,
    pub developpeur: String,
    pub genre: String,
    pub joueurs: String,
    /// Première date de sortie connue (texte du serveur, « 1998-10-21 » ou « 1998 »).
    pub date: String,
}

/// Quotas renvoyés par le serveur (`response.ssuser`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Quota {
    pub par_minute: u32,
    pub fils: u32,
    pub faites_jour: u32,
    pub max_jour: u32,
}

impl Default for Quota {
    /// Avant la première réponse : prudent (ScreenScraper donne au moins 1 fil et ~20/min).
    fn default() -> Quota {
        Quota {
            par_minute: 20,
            fils: 1,
            faites_jour: 0,
            max_jour: u32::MAX,
        }
    }
}

/// Réponse lue : la fiche, les quotas et les adresses des images à télécharger.
#[derive(Debug, PartialEq)]
pub struct Reponse {
    pub fiche: FicheSs,
    pub quota: Option<Quota>,
    pub url_wheel: Option<String>,
    pub url_boite: Option<String>,
}

/// Nombre ou chaîne de chiffres (le serveur renvoie les nombres en texte).
fn entier(v: &Value) -> Option<u32> {
    match v {
        Value::Number(n) => n.as_u64().map(|n| n.min(u32::MAX as u64) as u32),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn texte(v: &Value) -> String {
    match v {
        Value::String(s) => s.trim().to_string(),
        Value::Object(_) => texte(&v["text"]),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// Tableau d'objets ou objet seul (le serveur varie selon le nombre d'entrées).
fn liste(v: &Value) -> Vec<&Value> {
    match v {
        Value::Array(a) => a.iter().collect(),
        Value::Object(_) => vec![v],
        _ => Vec::new(),
    }
}

/// Texte de la première entrée dont `champ` vaut l'une des `preferes` (dans l'ordre), sinon la
/// première entrée non vide.
fn choisir<'a>(v: &'a Value, champ: &str, preferes: &[&str]) -> Option<(&'a Value, String)> {
    let l = liste(v);
    for p in preferes {
        if let Some(e) = l
            .iter()
            .find(|e| e[champ].as_str() == Some(p) && !texte(&e["text"]).is_empty())
        {
            return Some((e, texte(&e["text"])));
        }
    }
    l.into_iter()
        .find(|e| !texte(&e["text"]).is_empty())
        .map(|e| (e, texte(&e["text"])))
}

/// Seules les adresses HTTPS de ScreenScraper reçoivent les identifiants.
pub fn adresse_sure(url: &str) -> bool {
    let Some(reste) = url.strip_prefix("https://") else {
        return false;
    };
    let hote = reste.split(['/', '?', '#']).next().unwrap_or("");
    !hote.contains(['@', ':'])
        && (hote == "screenscraper.fr" || hote.ends_with(".screenscraper.fr"))
}

/// Image d'un genre donné, région mondiale ou européenne de préférence, PNG seulement.
fn media(medias: &Value, genres: &[&str]) -> Option<String> {
    let l = liste(medias);
    for g in genres {
        let du_genre: Vec<&&Value> = l
            .iter()
            .filter(|m| m["type"].as_str() == Some(g))
            .filter(|m| {
                m["format"]
                    .as_str()
                    .is_none_or(|f| f.eq_ignore_ascii_case("png"))
            })
            .filter(|m| m["url"].as_str().is_some_and(adresse_sure))
            .collect();
        for r in ["wor", "eu", "fr", "us", "ss", "jp"] {
            if let Some(m) = du_genre.iter().find(|m| m["region"].as_str() == Some(r)) {
                return m["url"].as_str().map(str::to_string);
            }
        }
        if let Some(m) = du_genre.first() {
            return m["url"].as_str().map(str::to_string);
        }
    }
    None
}

/// Lit une réponse JSON de `jeuInfos.php`. Jamais de panique ; `None` si ce n'est pas une
/// réponse de jeu.
pub fn analyser(json: &str) -> Option<Reponse> {
    let v: Value = serde_json::from_str(json).ok()?;
    let r = &v["response"];
    let jeu = &r["jeu"];
    if !jeu.is_object() {
        return None;
    }
    let u = &r["ssuser"];
    let quota = u.is_object().then(|| {
        let d = Quota::default();
        Quota {
            par_minute: entier(&u["maxrequestspermin"])
                .filter(|&n| n > 0)
                .unwrap_or(d.par_minute),
            fils: entier(&u["maxthreads"])
                .filter(|&n| n > 0)
                .unwrap_or(d.fils),
            faites_jour: entier(&u["requeststoday"]).unwrap_or(0),
            max_jour: entier(&u["maxrequestsperday"])
                .filter(|&n| n > 0)
                .unwrap_or(d.max_jour),
        }
    });
    let (langue, synopsis) = match choisir(&jeu["synopsis"], "langue", &["fr", "en"]) {
        Some((e, t)) => (e["langue"].as_str().unwrap_or("").to_string(), t),
        None => (String::new(), String::new()),
    };
    let genre = liste(&jeu["genres"])
        .into_iter()
        .find_map(|g| choisir(&g["noms"], "langue", &["fr", "en"]).map(|(_, t)| t))
        .unwrap_or_default();
    let fiche = FicheSs {
        titre: choisir(&jeu["noms"], "region", &["fr", "eu", "wor", "ss", "us"])
            .map(|(_, t)| t)
            .unwrap_or_default(),
        synopsis,
        langue,
        note: entier(&jeu["note"]["text"])
            .filter(|&n| n <= 20)
            .map(|n| n as u8),
        editeur: texte(&jeu["editeur"]),
        developpeur: texte(&jeu["developpeur"]),
        genre,
        joueurs: texte(&jeu["joueurs"]),
        date: choisir(&jeu["dates"], "region", &["fr", "eu", "wor", "us", "jp"])
            .map(|(_, t)| t)
            .unwrap_or_default(),
    };
    Some(Reponse {
        fiche,
        quota,
        url_wheel: media(&jeu["medias"], &["wheel", "wheel-hd"]),
        url_boite: media(&jeu["medias"], &["box-2D"]),
    })
}

/// Adresse de la requête (porte les identifiants : jamais affichée ni écrite).
pub fn url_jeu(id: &Identifiants, crc: u32, taille: u64, p: Plateforme, nom: &str) -> String {
    let mut u = format!(
        "{API}?devid={}&devpassword={}&softname={LOGICIEL}&output=json",
        encoder_url(id.devid.trim()),
        encoder_url(&id.devpassword)
    );
    if !id.utilisateur.trim().is_empty() {
        u.push_str(&format!(
            "&ssid={}&sspassword={}",
            encoder_url(id.utilisateur.trim()),
            encoder_url(&id.mot_de_passe)
        ));
    }
    u.push_str(&format!(
        "&systemeid={}&romtype=rom&crc={crc:08X}&romtaille={taille}&romnom={}",
        systeme_id(p),
        encoder_url(nom)
    ));
    u
}

/// Message clair d'un code d'erreur HTTP de ScreenScraper.
pub fn message_statut(code: u16) -> &'static str {
    match code {
        400 => "requête refusée par ScreenScraper",
        401 => "ScreenScraper est fermé aux non-membres pour le moment",
        403 => "identifiants développeur refusés par ScreenScraper",
        404 => "jeu inconnu de ScreenScraper",
        423 => "ScreenScraper est fermé pour le moment",
        426 => "cette version de gbemu est refusée par ScreenScraper",
        429 => "trop de requêtes, nouvel essai dans une minute",
        430 => "quota du jour de ScreenScraper atteint",
        431 => "trop de jeux introuvables aujourd'hui",
        _ => "ScreenScraper ne répond pas correctement",
    }
}

/// Cadence : au plus `par_minute` requêtes sur toute fenêtre de 60 s, et rien une fois le
/// quota du jour épuisé.
#[derive(Default)]
pub struct Limiteur {
    pub quota: Quota,
    faites: VecDeque<Instant>,
    /// Pause imposée par le serveur (429) ou quota du jour (430) jusqu'à cet instant.
    pause: Option<Instant>,
}

impl Limiteur {
    /// Temps à attendre avant la prochaine requête (`None` : plus rien aujourd'hui).
    pub fn attente(&mut self, t: Instant) -> Option<Duration> {
        if self.quota.faites_jour >= self.quota.max_jour {
            return None;
        }
        while self
            .faites
            .front()
            .is_some_and(|&f| t.duration_since(f) >= Duration::from_secs(60))
        {
            self.faites.pop_front();
        }
        let mut d = Duration::ZERO;
        if let Some(p) = self.pause {
            d = p.saturating_duration_since(t);
        }
        if self.faites.len() >= self.quota.par_minute.max(1) as usize {
            if let Some(&f) = self.faites.front() {
                d = d.max((f + Duration::from_secs(60)).saturating_duration_since(t));
            }
        }
        Some(d)
    }

    pub fn noter(&mut self, t: Instant) {
        self.faites.push_back(t);
        self.quota.faites_jour = self.quota.faites_jour.saturating_add(1);
    }

    /// Réponse « trop de requêtes » : une minute de pause.
    pub fn ralentir(&mut self, t: Instant) {
        self.pause = Some(t + Duration::from_secs(60));
    }

    /// Quota du jour épuisé : plus de requête jusqu'au prochain lancement.
    pub fn arreter(&mut self) {
        self.quota.faites_jour = self.quota.max_jour;
    }
}

pub struct Demande {
    pub crc: u32,
    pub taille: u64,
    pub plateforme: Plateforme,
    /// Nom du fichier de la ROM (aide la recherche côté serveur).
    pub nom: String,
    pub id: Identifiants,
}

pub struct Retour {
    pub crc: u32,
    pub fiche: Result<FicheSs, String>,
    pub wheel: Option<Image>,
    pub boite: Option<Image>,
}

pub struct ScreenScraper {
    file: Arc<(Mutex<VecDeque<Demande>>, Condvar)>,
    pub rx: Receiver<Retour>,
    /// Cadence partagée avec le téléchargement groupé (une requête à la fois en tout).
    pub lim: Arc<Mutex<Limiteur>>,
}

impl ScreenScraper {
    pub fn lancer(dossier: PathBuf, reveil: impl Fn() + Send + 'static) -> ScreenScraper {
        let file: Arc<(Mutex<VecDeque<Demande>>, Condvar)> = Arc::default();
        let lim: Arc<Mutex<Limiteur>> = Arc::default();
        let (tx, rx) = channel();
        let (f, l) = (Arc::clone(&file), Arc::clone(&lim));
        std::thread::spawn(move || boucle(dossier, f, l, tx, reveil));
        ScreenScraper { file, rx, lim }
    }

    /// Demande la fiche d'un jeu ; la plus récente passe en premier, les vieilles sont oubliées.
    pub fn demander(&self, d: Demande) {
        let (m, cv) = &*self.file;
        if let Ok(mut q) = m.lock() {
            q.retain(|x| x.crc != d.crc);
            q.push_front(d);
            q.truncate(8);
            cv.notify_one();
        }
    }
}

fn boucle(
    dossier: PathBuf,
    file: Arc<(Mutex<VecDeque<Demande>>, Condvar)>,
    lim: Arc<Mutex<Limiteur>>,
    tx: Sender<Retour>,
    reveil: impl Fn(),
) {
    let agent = agent();
    loop {
        let d = {
            let (m, cv) = &*file;
            let Ok(mut q) = m.lock() else { return };
            loop {
                if let Some(d) = q.pop_front() {
                    break d;
                }
                q = match cv.wait(q) {
                    Ok(q) => q,
                    Err(_) => return,
                };
            }
        };
        let Ok(mut l) = lim.lock() else { return };
        let r = obtenir(&agent, &dossier, &d, &mut l, crate::sauve::hors_ligne());
        drop(l);
        if tx.send(r).is_err() {
            return;
        }
        reveil();
    }
}

fn fichier_cache(dossier: &Path, p: Plateforme, crc: u32, ext: &str) -> PathBuf {
    dossier.join(p.court()).join(format!("{crc:08X}.{ext}"))
}

/// Oublie la réponse « jeu inconnu » d'un jeu (redemandé depuis sa fiche).
pub fn oublier_absent(dossier: &Path, p: Plateforme, crc: u32) {
    let _ = std::fs::remove_file(fichier_cache(dossier, p, crc, "absent"));
}

fn lire_image(chemin: &Path) -> Option<Image> {
    std::fs::read(chemin).ok().and_then(|d| decoder(&d))
}

/// Télécharge une image de ScreenScraper (adresse vérifiée), la garde en cache.
fn image(agent: &ureq::Agent, url: Option<&str>, local: &Path) -> Option<Image> {
    let url = url.filter(|u| adresse_sure(u))?;
    let rep = agent.get(url).call().ok()?;
    let mut d = Vec::new();
    rep.into_reader().take(8 << 20).read_to_end(&mut d).ok()?;
    let img = decoder(&d)?;
    let _ = crate::sauve::ecrire_atomique(local, &d);
    Some(img)
}

/// Fiche en cache, sinon demandée au serveur (identifiants développeur requis, jamais
/// `hors_ligne`, quotas respectés).
pub fn obtenir(
    agent: &ureq::Agent,
    dossier: &Path,
    d: &Demande,
    lim: &mut Limiteur,
    hors_ligne: bool,
) -> Retour {
    let p = d.plateforme;
    let json = fichier_cache(dossier, p, d.crc, "json");
    let wheel = fichier_cache(dossier, p, d.crc, "wheel.png");
    let boite = fichier_cache(dossier, p, d.crc, "boite.png");
    let absent = fichier_cache(dossier, p, d.crc, "absent");
    let retour = |fiche| Retour {
        crc: d.crc,
        fiche,
        wheel: None,
        boite: None,
    };
    if let Some(f) = crate::sauve::lire_json::<FicheSs>(&json) {
        return Retour {
            crc: d.crc,
            fiche: Ok(f),
            wheel: lire_image(&wheel),
            boite: lire_image(&boite),
        };
    }
    if absent.exists() && !perime(&absent, JOURS_ABSENT) {
        return retour(Err(message_statut(404).into()));
    }
    if !d.id.actif() {
        return retour(Err(
            "identifiants développeur ScreenScraper manquants (Réglages, Métadonnées)".into(),
        ));
    }
    if hors_ligne {
        return retour(Err("hors ligne".into()));
    }
    match lim.attente(Instant::now()) {
        None => return retour(Err(message_statut(430).into())),
        // raccourci : attente bloquante du fil (une requête à la fois), suffisant à la cadence
        // d'une fiche ouverte par l'utilisateur.
        Some(a) => std::thread::sleep(a),
    }
    lim.noter(Instant::now());
    let url = url_jeu(&d.id, d.crc, d.taille, p, &d.nom);
    // Erreurs jamais affichées telles quelles : celles de ureq contiennent l'adresse.
    let texte = match agent.get(&url).call() {
        Ok(rep) => {
            let mut t = String::new();
            if rep
                .into_reader()
                .take(4 << 20)
                .read_to_string(&mut t)
                .is_err()
            {
                return retour(Err(message_statut(0).into()));
            }
            t
        }
        Err(ureq::Error::Status(code, _)) => {
            match code {
                404 => {
                    let _ = crate::sauve::ecrire_atomique(&absent, b"");
                }
                429 => lim.ralentir(Instant::now()),
                430 => lim.arreter(),
                _ => {}
            }
            return retour(Err(message_statut(code).into()));
        }
        Err(_) => return retour(Err("ScreenScraper injoignable".into())),
    };
    let Some(rep) = analyser(&texte) else {
        return retour(Err(message_statut(0).into()));
    };
    if let Some(q) = rep.quota {
        lim.quota = q;
    }
    if let Ok(j) = serde_json::to_vec_pretty(&rep.fiche) {
        let _ = crate::sauve::ecrire_atomique(&json, &j);
    }
    Retour {
        crc: d.crc,
        wheel: image(agent, rep.url_wheel.as_deref(), &wheel),
        boite: image(agent, rep.url_boite.as_deref(), &boite),
        fiche: Ok(rep.fiche),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Réponse fabriquée sur le modèle de `jeuInfos.php` (nombres en texte, objets seuls ou
    /// en tableaux).
    const REPONSE: &str = r#"{
      "header": {"APIversion": "2.0", "success": "true", "error": ""},
      "response": {
        "ssuser": {"id": "moi", "maxthreads": "2", "maxrequestspermin": "30",
                   "requeststoday": "41", "maxrequestsperday": "20000"},
        "jeu": {
          "id": "123",
          "noms": [{"region": "us", "text": "Lantern Quest"},
                   {"region": "eu", "text": "Lanterne Quête"}],
          "editeur": {"id": "1", "text": "Studio Lanterne"},
          "developpeur": {"id": "2", "text": "Atelier Brume"},
          "joueurs": {"text": "1-2"},
          "note": {"text": "17"},
          "synopsis": [{"langue": "en", "text": "A lantern."},
                       {"langue": "fr", "text": " Une lanterne. "}],
          "dates": [{"region": "jp", "text": "1998-03-01"}, {"region": "eu", "text": "1999"}],
          "genres": [{"id": "7", "noms": [{"langue": "en", "text": "Puzzle"},
                                          {"langue": "fr", "text": "Réflexion"}]}],
          "medias": [
            {"type": "wheel", "region": "jp", "url": "https://neoclone.screenscraper.fr/api2/mediaJeu.php?media=wheel(jp)", "format": "png"},
            {"type": "wheel", "region": "eu", "url": "https://neoclone.screenscraper.fr/api2/mediaJeu.php?media=wheel(eu)", "format": "png"},
            {"type": "box-2D", "region": "eu", "url": "https://evil.example/box.png", "format": "png"},
            {"type": "box-2D", "region": "us", "url": "https://www.screenscraper.fr/b.jpg", "format": "jpg"}
          ]
        }
      }
    }"#;

    #[test]
    fn analyse_de_reponse() {
        let r = analyser(REPONSE).unwrap();
        let f = &r.fiche;
        assert_eq!(f.titre, "Lanterne Quête");
        assert_eq!(
            (f.synopsis.as_str(), f.langue.as_str()),
            ("Une lanterne.", "fr")
        );
        assert_eq!(f.note, Some(17));
        assert_eq!(f.editeur, "Studio Lanterne");
        assert_eq!(f.developpeur, "Atelier Brume");
        assert_eq!(f.genre, "Réflexion");
        assert_eq!(f.joueurs, "1-2");
        assert_eq!(f.date, "1999");
        assert_eq!(
            r.url_wheel.as_deref(),
            Some("https://neoclone.screenscraper.fr/api2/mediaJeu.php?media=wheel(eu)")
        );
        // Hôte étranger refusé, JPEG non décodable écarté.
        assert_eq!(r.url_boite, None);
        assert_eq!(
            r.quota,
            Some(Quota {
                par_minute: 30,
                fils: 2,
                faites_jour: 41,
                max_jour: 20000
            })
        );
    }

    #[test]
    fn repli_anglais_et_reponses_abimees() {
        let j = r#"{"response":{"jeu":{"synopsis":{"langue":"en","text":"Only english"},
                    "note":{"text":"99"},"medias":"rien"}}}"#;
        let r = analyser(j).unwrap();
        assert_eq!(
            (r.fiche.synopsis.as_str(), r.fiche.langue.as_str()),
            ("Only english", "en")
        );
        assert_eq!(r.fiche.note, None);
        assert_eq!(r.quota, None);
        assert_eq!(r.url_wheel, None);
        for abime in [
            "",
            "{",
            "[]",
            "null",
            r#"{"response":{"jeu":"x"}}"#,
            "Erreur de login",
        ] {
            assert!(analyser(abime).is_none(), "{abime}");
        }
    }

    #[test]
    fn adresses_et_identifiants() {
        assert!(adresse_sure("https://www.screenscraper.fr/x"));
        assert!(adresse_sure("https://screenscraper.fr"));
        for non in [
            "http://www.screenscraper.fr/x",
            "https://screenscraper.fr.evil.com/x",
            "https://evilscreenscraper.fr/x",
            "https://user@evil.com/?a=.screenscraper.fr",
            "https://www.screenscraper.fr:8080/x",
        ] {
            assert!(!adresse_sure(non), "{non}");
        }
        let id = Identifiants {
            utilisateur: "moi".into(),
            mot_de_passe: "p&ss word".into(),
            devid: " dev ".into(),
            devpassword: "d?p".into(),
        };
        let u = url_jeu(&id, 0x0A1B2C3D, 32768, Plateforme::Gbc, "Jeu (Europe).gbc");
        assert!(u.starts_with("https://api.screenscraper.fr/api2/jeuInfos.php?"));
        assert!(u.contains("devid=dev&devpassword=d%3Fp"));
        assert!(u.contains("ssid=moi&sspassword=p%26ss%20word"));
        assert!(u.contains("systemeid=10&romtype=rom&crc=0A1B2C3D&romtaille=32768"));
        assert!(adresse_sure(&u));
        // Sans compte utilisateur : pas de champs ssid/sspassword.
        let sans = Identifiants {
            devid: "d".into(),
            devpassword: "p".into(),
            ..Default::default()
        };
        assert!(!url_jeu(&sans, 1, 1, Plateforme::Gb, "x").contains("ssid"));
        assert_eq!(
            (systeme_id(Plateforme::Gb), systeme_id(Plateforme::Gba)),
            (9, 12)
        );
    }

    #[test]
    fn quotas_respectes() {
        let t = Instant::now();
        let mut l = Limiteur {
            quota: Quota {
                par_minute: 2,
                fils: 1,
                faites_jour: 0,
                max_jour: 3,
            },
            ..Default::default()
        };
        assert_eq!(l.attente(t), Some(Duration::ZERO));
        l.noter(t);
        l.noter(t + Duration::from_secs(10));
        // Fenêtre de 60 s pleine : attendre que la première requête en sorte.
        assert_eq!(
            l.attente(t + Duration::from_secs(20)),
            Some(Duration::from_secs(40))
        );
        assert_eq!(l.attente(t + Duration::from_secs(60)), Some(Duration::ZERO));
        l.noter(t + Duration::from_secs(60));
        // Quota du jour (3) atteint.
        assert_eq!(l.attente(t + Duration::from_secs(600)), None);
        let mut l = Limiteur::default();
        l.ralentir(t);
        assert_eq!(
            l.attente(t + Duration::from_secs(15)),
            Some(Duration::from_secs(45))
        );
        l.arreter();
        assert_eq!(l.attente(t), None);
    }

    #[test]
    fn inactif_sans_identifiants_developpeur() {
        let d = crate::sauve::DossierTest::new("ss");
        let dem = |id: Identifiants| Demande {
            crc: 0x1234,
            taille: 1,
            plateforme: Plateforme::Gb,
            nom: "x.gb".into(),
            id,
        };
        let mut l = Limiteur::default();
        let a = agent();
        // Compte utilisateur seul : aucune requête (sinon le limiteur l'aurait comptée).
        let id = Identifiants {
            utilisateur: "moi".into(),
            mot_de_passe: "secret".into(),
            ..Default::default()
        };
        let r = obtenir(&a, &d.0, &dem(id), &mut l, false);
        assert!(r.fiche.unwrap_err().contains("développeur"));
        assert_eq!(l.quota.faites_jour, 0);
        // Identifiants complets mais hors ligne : rien non plus.
        let id = Identifiants {
            devid: "d".into(),
            devpassword: "p".into(),
            ..Default::default()
        };
        assert_eq!(
            obtenir(&a, &d.0, &dem(id.clone()), &mut l, true).fiche,
            Err("hors ligne".into())
        );
        assert_eq!(l.quota.faites_jour, 0);
        // Fiche en cache : lue sans réseau, même sans identifiants.
        let f = analyser(REPONSE).unwrap().fiche;
        let c = fichier_cache(&d.0, Plateforme::Gb, 0x1234, "json");
        crate::sauve::ecrire_atomique(&c, &serde_json::to_vec(&f).unwrap()).unwrap();
        let r = obtenir(&a, &d.0, &dem(Identifiants::default()), &mut l, true);
        assert_eq!(r.fiche, Ok(f));
        assert_eq!(l.quota.faites_jour, 0);
        // Jeu noté introuvable : pas redemandé.
        let c = fichier_cache(&d.0, Plateforme::Gb, 0x99, "absent");
        crate::sauve::ecrire_atomique(&c, b"").unwrap();
        let mut dm = dem(id);
        dm.crc = 0x99;
        assert!(obtenir(&a, &d.0, &dm, &mut l, false).fiche.is_err());
        assert_eq!(l.quota.faites_jour, 0);
    }

    #[test]
    fn identifiants_enregistres_et_fichier_developpeur() {
        let d = crate::sauve::DossierTest::new("ss-id");
        let id = Identifiants {
            utilisateur: "moi".into(),
            mot_de_passe: "secret".into(),
            ..Default::default()
        };
        id.enregistrer(&d.0).unwrap();
        let lu = Identifiants::charger(&d.0);
        assert!(lu == id && !lu.actif());
        std::fs::write(
            d.0.join(FICHIER_DEV),
            br#"{"devid": "appli", "devpassword": "cle"}"#,
        )
        .unwrap();
        let lu = Identifiants::charger(&d.0);
        assert!(lu.actif());
        assert_eq!(
            (lu.devid.as_str(), lu.utilisateur.as_str()),
            ("appli", "moi")
        );
    }
}
