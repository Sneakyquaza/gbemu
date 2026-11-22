//! Reconnaissance des jeux : DAT No-Intro et années de sortie de libretro-database (CC BY-SA 4.0).
//! Métadonnées seulement, téléchargées une fois au premier lancement puis lues depuis le cache.

use crate::biblio::Plateforme;
use std::collections::HashMap;
use std::io::Read as _;
use std::path::Path;
use std::time::Duration;

const DEPOT: &str = "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat";

#[derive(Clone, Debug)]
pub struct InfoJeu {
    /// Nom No-Intro complet (sert aussi au nom des jaquettes).
    pub nom: String,
    pub titre: String,
    pub region: String,
    pub annee: Option<u16>,
    pub plateforme: Plateforme,
}

#[derive(Default)]
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
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(6))
        .timeout(Duration::from_secs(30))
        .user_agent("gbemu/0.1")
        .build()
}

/// Lit le fichier en cache ou le télécharge une fois. `None` si hors ligne.
fn obtenir(agent: &ureq::Agent, dossier: &Path, sous: &str, p: Plateforme) -> Option<String> {
    let local = dossier.join(sous).join(format!("{}.dat", systeme(p)));
    if let Ok(t) = std::fs::read_to_string(&local) {
        return Some(t);
    }
    let url = format!("{DEPOT}/{sous}/{}.dat", encoder_url(systeme(p)));
    let rep = agent.get(&url).call().ok()?;
    let mut texte = String::new();
    std::io::Read::read_to_string(&mut rep.into_reader().take(64 << 20), &mut texte).ok()?;
    if !texte.contains("game (") {
        return None;
    }
    let _ = crate::sauve::ecrire_atomique(&local, texte.as_bytes());
    Some(texte)
}

/// Charge (et télécharge si besoin) les trois plateformes.
pub fn charger(dossier: &Path) -> Base {
    let agent = agent();
    let mut base = Base::default();
    for p in [Plateforme::Gb, Plateforme::Gbc, Plateforme::Gba] {
        if let Some(t) = obtenir(&agent, dossier, "no-intro", p) {
            lire_nointro(&t, p, &mut base);
        }
        if let Some(t) = obtenir(&agent, dossier, "releaseyear", p) {
            lire_annees(&t, &mut base);
        }
    }
    base
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
        base.jeux.insert(
            crc,
            InfoJeu {
                nom: nom.to_string(),
                titre: titre_lisible(nom),
                region: c.get("region").unwrap_or(&"").to_string(),
                annee,
                plateforme: p,
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
    fn titres() {
        assert_eq!(titre_lisible("Jeu, L' (France)"), "L'Jeu");
        assert_eq!(titre_lisible("Simple"), "Simple");
        assert_eq!(encoder_url("A & B"), "A%20%26%20B");
    }
}
