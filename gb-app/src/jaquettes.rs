//! Jaquettes (libretro-thumbnails, Named_Boxarts) : téléchargées à la demande pour les jeux
//! reconnus, mises en cache sur disque, décodées et réduites hors du fil de l'interface.

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

type Demande = (Plateforme, String);

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

    /// Demande la jaquette d'un jeu (nom No-Intro) ; les dernières demandes passent en premier.
    pub fn demander(&self, p: Plateforme, nom: &str) {
        let (m, cv) = &*self.file;
        if let Ok(mut q) = m.lock() {
            q.push_front((p, nom.to_string()));
            q.truncate(256);
            cv.notify_one();
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
        let (p, nom) = {
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
        let img = obtenir(&agent, &dossier, p, &nom, crate::sauve::hors_ligne());
        if tx.send((nom, img)).is_err() {
            return;
        }
        reveil();
    }
}

/// Jaquette en cache, sinon téléchargée (jamais `hors_ligne`).
fn obtenir(
    agent: &ureq::Agent,
    dossier: &Path,
    p: Plateforme,
    nom: &str,
    hors_ligne: bool,
) -> Option<Image> {
    let fichier = nom_fichier(nom);
    let local = dossier.join(p.court()).join(format!("{fichier}.png"));
    let absent = local.with_extension("absent");
    if let Ok(d) = std::fs::read(&local) {
        return decoder(&d);
    }
    if hors_ligne || (absent.exists() && !perime(&absent, JOURS_ABSENT)) {
        return None;
    }
    let url = format!(
        "https://thumbnails.libretro.com/{}/Named_Boxarts/{}.png",
        encoder_url(systeme(p)),
        encoder_url(&fichier)
    );
    match agent.get(&url).call() {
        Ok(rep) => {
            let mut d = Vec::new();
            rep.into_reader().take(8 << 20).read_to_end(&mut d).ok()?;
            let img = decoder(&d)?;
            let _ = crate::sauve::ecrire_atomique(&local, &d);
            Some(img)
        }
        // Jaquette inexistante : on le note pour ne plus la redemander.
        Err(ureq::Error::Status(404, _)) => {
            let _ = crate::sauve::ecrire_atomique(&absent, b"");
            None
        }
        Err(_) => None,
    }
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
        assert!(obtenir(&agent(), &d.0, Plateforme::Gb, "Jeu (France)", true).is_none());
        // Aucun marqueur « absent » : la jaquette sera demandée une fois en ligne.
        assert!(std::fs::read_dir(&d.0).unwrap().next().is_none());
    }
}
