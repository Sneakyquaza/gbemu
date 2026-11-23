//! Reconnaissance des jeux : DAT No-Intro et métadonnées `metadat/` de libretro-database
//! (CC BY-SA 4.0 : année et mois, développeur, éditeur, genre, joueurs, classement ESRB), par CRC.
//! Base intégrée à l'exe (`donnees/jeux.bin`, produite par `scripts/donnees-jeux.py`, avec les
//! résumés Wikipédia CC BY-SA) ; les DAT téléchargés (mise à jour facultative, Réglages >
//! Métadonnées) la complètent depuis le cache. Mode hors ligne : jamais de téléchargement.

use crate::biblio::Plateforme;
use std::collections::HashMap;
use std::io::Read as _;
use std::path::Path;

/// Base intégrée (format décrit dans `scripts/donnees-jeux.py`).
const INTEGREE: &[u8] = include_bytes!("../donnees/jeux.bin");
use std::time::Duration;

/// Âge (jours) au-delà duquel un DAT en cache est téléchargé à nouveau.
const JOURS_DAT: u64 = 90;

const DEPOT: &str = "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat";

#[derive(Clone, Debug)]
pub struct InfoJeu {
    /// Nom No-Intro complet (sert aussi au nom des jaquettes).
    pub nom: String,
    pub titre: String,
    pub region: String,
    pub annee: Option<u16>,
    pub plateforme: Plateforme,
    pub mois: Option<u8>,
    pub developpeur: String,
    pub editeur: String,
    pub genre: String,
    /// Nombre maximal de joueurs.
    pub joueurs: Option<u8>,
    pub esrb: String,
    /// Introduction Wikipédia (1 à 3 phrases), vide si aucune.
    pub resume: String,
    /// Article source du résumé : « fr:Titre » ou « en:Titre ».
    pub source_resume: String,
}

impl InfoJeu {
    /// Adresse de l'article Wikipédia du résumé.
    pub fn url_resume(&self) -> Option<String> {
        let (langue, titre) = self.source_resume.split_once(':')?;
        Some(format!(
            "https://{langue}.wikipedia.org/wiki/{}",
            encoder_url(&titre.replace(' ', "_"))
        ))
    }
}

/// Fichiers `metadat/<dossier>` lus après les noms : (dossier, clé dans le bloc `game`).
const METADAT: [(&str, &str); 6] = [
    ("developer", "developer"),
    ("publisher", "publisher"),
    ("genre", "genre"),
    ("releasemonth", "releasemonth"),
    ("maxusers", "users"),
    ("esrb", "esrb_rating"),
];

#[derive(Default, Clone)]
pub struct Base {
    pub jeux: HashMap<u32, InfoJeu>,
}

/// Nom du système chez libretro (DAT et jaquettes).
pub fn systeme(p: Plateforme) -> &'static str {
    match p {
        Plateforme::Gb => "Nintendo - Game Boy",
        Plateforme::Gbc => "Nintendo - Game Boy Color",
        Plateforme::Gba => "Nintendo - Game Boy Advance",
    }
}

/// Encode un segment d'URL (tout sauf les caractères non réservés).
pub fn encoder_url(s: &str) -> String {
    let mut r = String::with_capacity(s.len() * 3 / 2);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            r.push(b as char);
        } else {
            r.push_str(&format!("%{b:02X}"));
        }
    }
    r
}

pub fn agent() -> ureq::Agent {
    let mut a = ureq::AgentBuilder::new();
    // Robot de test : tout passe par un mandataire local fermé (aucune requête ne sort).
    if RESEAU_COUPE.load(std::sync::atomic::Ordering::Relaxed) {
        if let Ok(p) = ureq::Proxy::new("http://127.0.0.1:9") {
            a = a.proxy(p);
        }
    }
    a.timeout_connect(Duration::from_secs(6))
        .timeout(Duration::from_secs(30))
        .user_agent("gbemu/0.1")
        .build()
}

/// Levé par le robot de test avant toute création d'agent.
pub static RESEAU_COUPE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Vrai si le fichier a été écrit il y a plus de `jours` jours.
pub fn perime(chemin: &Path, jours: u64) -> bool {
    std::fs::metadata(chemin)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|d| d > Duration::from_secs(jours * 86_400))
}

/// Lit le fichier en cache, ou le télécharge s'il manque ou est périmé (sauf `hors_ligne`).
/// Téléchargement impossible : l'ancien cache sert. `None` si rien.
fn obtenir(
    agent: &ureq::Agent,
    dossier: &Path,
    sous: &str,
    p: Plateforme,
    hors_ligne: bool,
) -> Option<String> {
    let local = dossier.join(sous).join(format!("{}.dat", systeme(p)));
    let cache = std::fs::read_to_string(&local).ok();
    if cache.is_some() && (hors_ligne || !perime(&local, JOURS_DAT)) {
        return cache;
    }
    if hors_ligne {
        return None;
    }
    telecharger(agent, &local, sous, p).or(cache)
}

fn telecharger(agent: &ureq::Agent, local: &Path, sous: &str, p: Plateforme) -> Option<String> {
    let url = format!("{DEPOT}/{sous}/{}.dat", encoder_url(systeme(p)));
    let rep = agent.get(&url).call().ok()?;
    let mut texte = String::new();
    std::io::Read::read_to_string(&mut rep.into_reader().take(64 << 20), &mut texte).ok()?;
    if !texte.contains("game (") {
        return None;
    }
    let _ = crate::sauve::ecrire_atomique(local, texte.as_bytes());
    Some(texte)
}

/// Base intégrée, complétée par les DAT du cache. `en_ligne` : DAT manquants ou périmés
/// téléchargés d'abord (mise à jour demandée par l'utilisateur, jamais en mode hors ligne).
pub fn charger(dossier: &Path, en_ligne: bool) -> Base {
    let agent = agent();
    let hors_ligne = !en_ligne || crate::sauve::hors_ligne();
    let mut base = integree();
    for p in [Plateforme::Gb, Plateforme::Gbc, Plateforme::Gba] {
        if let Some(t) = obtenir(&agent, dossier, "no-intro", p, hors_ligne) {
            lire_nointro(&t, p, &mut base);
        }
        if let Some(t) = obtenir(&agent, dossier, "releaseyear", p, hors_ligne) {
            lire_annees(&t, &mut base);
        }
        for (sous, cle) in METADAT {
            if let Some(t) = obtenir(&agent, dossier, sous, p, hors_ligne) {
                lire_champ(&t, cle, &mut base);
            }
        }
    }
    base
}

/// Base intégrée à l'exe (vide si illisible, sans panique).
pub fn integree() -> Base {
    lire_integree(INTEGREE).unwrap_or_default()
}

/// Lit le format « GBJX » version 1 : en-tête, puis flux zlib (chaînes, puis jeux).
fn lire_integree(d: &[u8]) -> Option<Base> {
    let flux = d.strip_prefix(b"GBJX\x01")?;
    let mut brut = Vec::new();
    flate2::read::ZlibDecoder::new(flux)
        .take(64 << 20)
        .read_to_end(&mut brut)
        .ok()?;
    let mut o = &brut[..];
    let mut lire = |n: usize| -> Option<&[u8]> {
        let (a, b) = o.split_at_checked(n)?;
        o = b;
        Some(a)
    };
    let u32_ = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let nb = u32_(lire(4)?) as usize;
    let mut chaines = Vec::with_capacity(nb.min(1 << 20));
    for _ in 0..nb {
        let l = lire(2)?;
        let l = u16::from_le_bytes([l[0], l[1]]) as usize;
        chaines.push(std::str::from_utf8(lire(l)?).ok()?);
    }
    let nj = u32_(lire(4)?) as usize;
    let mut base = Base::default();
    base.jeux.reserve(nj.min(1 << 20));
    for _ in 0..nj {
        let e = lire(4 + 1 + 2 + 1 + 1 + 8 * 4)?;
        let s = |k: usize| -> Option<String> {
            Some(chaines.get(u32_(&e[9 + 4 * k..]) as usize)?.to_string())
        };
        let plateforme = match e[4] {
            0 => Plateforme::Gb,
            1 => Plateforme::Gbc,
            2 => Plateforme::Gba,
            _ => return None,
        };
        let nom = s(0)?;
        let annee = u16::from_le_bytes([e[5], e[6]]);
        base.jeux.insert(
            u32_(e),
            InfoJeu {
                titre: titre_lisible(&nom),
                nom,
                region: s(1)?,
                annee: (annee != 0).then_some(annee),
                plateforme,
                mois: (1..=12).contains(&e[7]).then_some(e[7]),
                joueurs: (e[8] != 0).then_some(e[8]),
                developpeur: s(2)?,
                editeur: s(3)?,
                genre: s(4)?,
                esrb: s(5)?,
                resume: s(6)?,
                source_resume: s(7)?,
            },
        );
    }
    Some(base)
}

#[derive(Debug, PartialEq)]
enum Jeton<'a> {
    Ouvre,
    Ferme,
    Mot(&'a str),
}

/// Découpe le format clrmamepro : mots, chaînes entre guillemets, parenthèses.
fn jetons(s: &str) -> Vec<Jeton<'_>> {
    let o = s.as_bytes();
    let mut v = Vec::new();
    let mut i = 0;
    while i < o.len() {
        match o[i] {
            b'(' => {
                v.push(Jeton::Ouvre);
                i += 1;
            }
            b')' => {
                v.push(Jeton::Ferme);
                i += 1;
            }
            b'"' => {
                let d = i + 1;
                let mut f = d;
                while f < o.len() && o[f] != b'"' {
                    f += 1;
                }
                v.push(Jeton::Mot(&s[d..f]));
                i = f + 1;
            }
            c if c.is_ascii_whitespace() => i += 1,
            _ => {
                let d = i;
                while i < o.len() && !o[i].is_ascii_whitespace() && o[i] != b'(' && o[i] != b')' {
                    i += 1;
                }
                v.push(Jeton::Mot(&s[d..i]));
            }
        }
    }
    v
}

/// Champs d'un bloc `game ( … )`, y compris ceux du sous-bloc `rom ( … )`.
fn blocs_jeu<'a>(t: &'a str, mut f: impl FnMut(&HashMap<&'a str, &'a str>)) {
    let j = jetons(t);
    let mut i = 0;
    while i + 1 < j.len() {
        if j[i] == Jeton::Mot("game") && j[i + 1] == Jeton::Ouvre {
            i += 2;
            let mut champs = HashMap::new();
            let mut prof = 1;
            while i < j.len() && prof > 0 {
                match &j[i] {
                    Jeton::Ouvre => prof += 1,
                    Jeton::Ferme => prof -= 1,
                    Jeton::Mot(k) => {
                        if let Some(Jeton::Mot(val)) = j.get(i + 1) {
                            champs.entry(*k).or_insert(*val);
                            i += 1;
                        }
                    }
                }
                i += 1;
            }
            f(&champs);
        } else {
            i += 1;
        }
    }
}

fn lire_nointro(t: &str, p: Plateforme, base: &mut Base) {
    blocs_jeu(t, |c| {
        let (Some(nom), Some(crc)) = (c.get("name"), c.get("crc")) else {
            return;
        };
        let Ok(crc) = u32::from_str_radix(crc, 16) else {
            return;
        };
        let annee = c.get("releaseyear").and_then(|a| a.parse().ok());
        let region = c.get("region").unwrap_or(&"").to_string();
        // Jeu déjà dans la base intégrée : nom et région mis à jour, le reste gardé.
        if let Some(j) = base.jeux.get_mut(&crc) {
            j.nom = nom.to_string();
            j.titre = titre_lisible(nom);
            j.region = region;
            j.plateforme = p;
            if annee.is_some() {
                j.annee = annee;
            }
            return;
        }
        base.jeux.insert(
            crc,
            InfoJeu {
                nom: nom.to_string(),
                titre: titre_lisible(nom),
                region,
                annee,
                plateforme: p,
                mois: None,
                developpeur: String::new(),
                editeur: String::new(),
                genre: String::new(),
                joueurs: None,
                esrb: String::new(),
                resume: String::new(),
                source_resume: String::new(),
            },
        );
    });
}

fn lire_annees(t: &str, base: &mut Base) {
    blocs_jeu(t, |c| {
        let (Some(a), Some(crc)) = (c.get("releaseyear"), c.get("crc")) else {
            return;
        };
        let (Ok(crc), Ok(a)) = (u32::from_str_radix(crc, 16), a.parse()) else {
            return;
        };
        if let Some(j) = base.jeux.get_mut(&crc) {
            j.annee.get_or_insert(a);
        }
    });
}

/// Un champ `metadat/` (clé `cle`) reporté sur les jeux déjà connus (par CRC).
fn lire_champ(t: &str, cle: &str, base: &mut Base) {
    blocs_jeu(t, |c| {
        let (Some(v), Some(crc)) = (c.get(cle), c.get("crc")) else {
            return;
        };
        let Ok(crc) = u32::from_str_radix(crc, 16) else {
            return;
        };
        let Some(j) = base.jeux.get_mut(&crc) else {
            return;
        };
        let v = v.trim();
        match cle {
            // « Nintendo ??? » : studio incertain chez libretro, la société seule reste.
            "developer" => j.developpeur = v.trim_end_matches('?').trim_end().to_string(),
            "publisher" => j.editeur = v.trim_end_matches('?').trim_end().to_string(),
            "genre" => j.genre = v.to_string(),
            "releasemonth" => j.mois = v.parse().ok().filter(|m| (1..=12).contains(m)),
            "users" => j.joueurs = v.parse().ok().filter(|&n| n > 0),
            "esrb_rating" => j.esrb = v.to_string(),
            _ => {}
        }
    });
}

/// « Legend of Zelda, The - Link's Awakening (USA) » → « The Legend of Zelda - Link's Awakening ».
pub fn titre_lisible(nom: &str) -> String {
    let base = nom.split(" (").next().unwrap_or(nom).trim();
    let mut parties: Vec<String> = base.split(" - ").map(str::to_string).collect();
    if let Some(p) = parties.first_mut() {
        for art in [
            ", The", ", A", ", An", ", Les", ", Le", ", La", ", L'", ", Die", ", Der", ", Das",
            ", El", ", Il",
        ] {
            if let Some(debut) = p.strip_suffix(art) {
                let mot = &art[2..];
                let sep = if mot.ends_with('\'') { "" } else { " " };
                *p = format!("{mot}{sep}{debut}");
                break;
            }
        }
    }
    parties.join(" - ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXTRAIT: &str = r#"clrmamepro (
	name "Nintendo - Game Boy"
)
game (
	name "Lanterne, The - Retour (Europe) (Fr)"
	region "Europe"
	rom ( name "Lanterne, The - Retour (Europe) (Fr).gb" size 131072 crc 9A024415 sha1 95 )
)
game (
	name "Autre (Japan)"
	rom ( name "x.gb" size 1 crc ZZ )
)
"#;

    #[test]
    fn lecture_dat() {
        let mut b = Base::default();
        lire_nointro(EXTRAIT, Plateforme::Gb, &mut b);
        lire_annees(
            "game (\n comment \"x\"\n releaseyear \"1999\"\n rom ( crc 9A024415 )\n)",
            &mut b,
        );
        assert_eq!(b.jeux.len(), 1);
        let j = &b.jeux[&0x9A02_4415];
        assert_eq!(j.titre, "The Lanterne - Retour");
        assert_eq!(j.region, "Europe");
        assert_eq!(j.annee, Some(1999));
    }

    #[test]
    fn lecture_metadat() {
        let mut b = Base::default();
        lire_nointro(EXTRAIT, Plateforme::Gb, &mut b);
        let bloc = |k: &str, v: &str| {
            format!("clrmamepro ( name \"x\" )\ngame (\n\tcomment \"c\"\n\t{k} {v}\n\trom ( crc 9A024415 )\n)\ngame (\n\t{k} {v}\n\trom ( crc 11111111 )\n)\n")
        };
        lire_champ(
            &bloc("developer", "\"Morning Star ???\""),
            "developer",
            &mut b,
        );
        lire_champ(&bloc("publisher", "\"Majesco\""), "publisher", &mut b);
        lire_champ(&bloc("genre", "\"Sports\""), "genre", &mut b);
        lire_champ(&bloc("releasemonth", "\"8\""), "releasemonth", &mut b);
        lire_champ(&bloc("users", "2"), "users", &mut b);
        lire_champ(&bloc("esrb_rating", "\"E\""), "esrb_rating", &mut b);
        // CRC inconnu (11111111) : ignoré, rien n'est créé.
        assert_eq!(b.jeux.len(), 1);
        let j = &b.jeux[&0x9A02_4415];
        assert_eq!(j.developpeur, "Morning Star");
        assert_eq!(j.editeur, "Majesco");
        assert_eq!(j.genre, "Sports");
        assert_eq!(j.mois, Some(8));
        assert_eq!(j.joueurs, Some(2));
        assert_eq!(j.esrb, "E");
        // Valeurs absurdes écartées sans panique.
        lire_champ(&bloc("releasemonth", "\"13\""), "releasemonth", &mut b);
        lire_champ(&bloc("users", "beaucoup"), "users", &mut b);
        let j = &b.jeux[&0x9A02_4415];
        assert_eq!((j.mois, j.joueurs), (None, None));
        // Bloc tronqué : aucune panique.
        lire_champ("game ( genre \"Puzzle\" rom ( crc", "genre", &mut b);
    }

    #[test]
    fn base_integree() {
        let b = integree();
        assert!(b.jeux.len() > 7000, "{} jeux", b.jeux.len());
        let avec_resume = b.jeux.values().filter(|j| !j.resume.is_empty()).count();
        assert!(avec_resume > 1000, "{avec_resume} résumés");
        // Tetris (World) (Rev 1), Game Boy.
        let t = &b.jeux[&0x46DF_91AD];
        assert_eq!(t.plateforme, Plateforme::Gb);
        assert!(t.nom.starts_with("Tetris (World)"));
        assert_eq!(t.editeur, "Nintendo");
        assert!(t.source_resume.starts_with("fr:"), "{}", t.source_resume);
        assert!(t
            .url_resume()
            .unwrap()
            .starts_with("https://fr.wikipedia.org/wiki/Tetris"));
    }

    #[test]
    fn integree_abimee_sans_panique() {
        use std::io::Write as _;
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        // 2 chaînes (« », « Jeu (France) »), 1 jeu GBC 1999/12, 2 joueurs, résumé = chaîne 1.
        let mut brut = 2u32.to_le_bytes().to_vec();
        brut.extend(0u16.to_le_bytes());
        brut.extend(12u16.to_le_bytes());
        brut.extend(b"Jeu (France)");
        brut.extend(1u32.to_le_bytes());
        brut.extend(0x1234_5678u32.to_le_bytes());
        brut.push(1);
        brut.extend(1999u16.to_le_bytes());
        brut.extend([12, 2]);
        for k in [1u32, 0, 0, 0, 0, 0, 1, 0] {
            brut.extend(k.to_le_bytes());
        }
        z.write_all(&brut).unwrap();
        let mut d = b"GBJX\x01".to_vec();
        d.extend(z.finish().unwrap());
        let b = lire_integree(&d).unwrap();
        let j = &b.jeux[&0x1234_5678];
        assert_eq!(
            (j.plateforme, j.annee, j.mois, j.joueurs),
            (Plateforme::Gbc, Some(1999), Some(12), Some(2))
        );
        assert_eq!(
            (j.titre.as_str(), j.resume.as_str()),
            ("Jeu", "Jeu (France)")
        );
        assert_eq!(j.url_resume(), None);
        // Tronqué partout, mauvais en-tête, indice de chaîne hors limites : None, sans panique.
        for n in 0..d.len() {
            let _ = lire_integree(&d[..n]);
        }
        assert!(lire_integree(b"GBJX\x02").is_none());
        let mut faux = brut.clone();
        let k = faux.len() - 4;
        faux[k] = 9;
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        z.write_all(&faux).unwrap();
        let mut d = b"GBJX\x01".to_vec();
        d.extend(z.finish().unwrap());
        assert!(lire_integree(&d).is_none());
    }

    #[test]
    fn titres() {
        assert_eq!(titre_lisible("Jeu, L' (France)"), "L'Jeu");
        assert_eq!(titre_lisible("Simple"), "Simple");
        assert_eq!(encoder_url("A & B"), "A%20%26%20B");
    }

    #[test]
    fn hors_ligne_et_peremption() {
        let d = crate::sauve::DossierTest::new("dat");
        let a = agent();
        // Hors ligne sans cache : rien, sans réseau.
        assert!(obtenir(&a, &d.0, "no-intro", Plateforme::Gb, true).is_none());
        let local =
            d.0.join("no-intro")
                .join(format!("{}.dat", systeme(Plateforme::Gb)));
        crate::sauve::ecrire_atomique(&local, b"game ( )").unwrap();
        assert!(!perime(&local, JOURS_DAT));
        let vieux = std::time::SystemTime::now() - Duration::from_secs(91 * 86_400);
        std::fs::File::options()
            .write(true)
            .open(&local)
            .unwrap()
            .set_modified(vieux)
            .unwrap();
        assert!(perime(&local, JOURS_DAT));
        // Périmé mais hors ligne : le cache sert.
        assert_eq!(
            obtenir(&a, &d.0, "no-intro", Plateforme::Gb, true).as_deref(),
            Some("game ( )")
        );
    }
}
