//! Écran de démarrage : 600 ms au plus, superposé à la bibliothèque déjà prête (il n'attend
//! rien et ne retarde rien). « gbemu » descend de 12 px en fondu, les quatre teintes de
//! l'écran s'allument une à une, puis un « ding » à deux notes. Passable par n'importe quelle
//! touche, jamais rejoué dans la session, désactivable dans les réglages.

use crate::dessin::{self as d, j};
use crate::sauve::Palette;
use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Vec2};

/// Durée totale.
const DUREE: f64 = 0.6;
/// Moment du « ding ».
const T_DING: f64 = 0.28;
/// Début du fondu de sortie.
const T_SORTIE: f64 = 0.4;

pub struct Intro {
    debut: Option<f64>,
    /// Moment où une touche l'a écourtée (fondu de sortie rapide).
    passee: Option<f64>,
    ding: bool,
    /// Instant figé (vérification par capture : variable `GBEMU_INTRO_T`).
    fige: Option<f64>,
}

pub enum Etape {
    EnCours,
    /// C'est le moment de jouer le « ding ».
    Ding,
    Finie,
}

impl Intro {
    pub fn new() -> Intro {
        Intro {
            debut: None,
            passee: None,
            ding: false,
            fige: std::env::var("GBEMU_INTRO_T")
                .ok()
                .and_then(|v| v.parse().ok()),
        }
    }

    /// Vrai si l'intro doit tourner dans ce mode (les captures ne la voient que figée).
    pub fn visible_en_capture(&self) -> bool {
        self.fige.is_some()
    }

    pub fn dessiner(&mut self, ui: &egui::Ui, appui: bool) -> Etape {
        let maintenant = ui.input(|i| i.time);
        let debut = *self.debut.get_or_insert(maintenant);
        let mut t = self.fige.unwrap_or(maintenant - debut);
        if appui && self.passee.is_none() && self.fige.is_none() {
            self.passee = Some(t);
        }
        // Écourtée : fondu de 80 ms depuis l'état courant.
        let sortie = match self.passee {
            Some(tp) => {
                let k = ((t - tp) / 0.08).clamp(0.0, 1.0);
                t = tp.max(T_SORTIE) + k * (DUREE - tp.max(T_SORTIE));
                if k >= 1.0 {
                    return Etape::Finie;
                }
                1.0 - k
            }
            None => {
                if t >= DUREE {
                    return Etape::Finie;
                }
                1.0 - ((t - T_SORTIE) / (DUREE - T_SORTIE)).clamp(0.0, 1.0)
            }
        } as f32;
        let sortie = d::sortie(sortie);

        let jt = j();
        let ecran = ui.max_rect();
        let p = ui.painter().clone();
        p.rect_filled(ecran, CornerRadius::ZERO, d::alpha(jt.fond, 0.94 * sortie));
        d::lueur(
            &p,
            ecran.center(),
            Vec2::new(ecran.width() * 0.32, ecran.height() * 0.42),
            d::alpha(jt.lampe, 0.9 * sortie),
        );

        let entree = d::sortie((t / 0.3) as f32);
        let chute = if d::mouvement_reduit() {
            0.0
        } else {
            (1.0 - entree) * 12.0
        };
        let a = entree * sortie;
        let mut pm = p.clone();
        pm.multiply_opacity(a);
        let g = d::ligne(&p, "gbemu", d::gras(46.0), jt.texte);
        let gs = g.size();
        let haut = ecran.center().y - gs.y * 0.5 - 10.0 - chute;
        pm.galley(Pos2::new(ecran.center().x - gs.x * 0.5, haut), g, jt.texte);

        // Les quatre teintes de l'écran s'allument l'une après l'autre.
        let teintes = Palette::Verte.teintes();
        let y = ecran.center().y + gs.y * 0.5 + 6.0;
        for k in 0..4 {
            let tk = d::sortie(((t - 0.08 - k as f64 * 0.05) / 0.08) as f32);
            let c = teintes[3 - k];
            let x = ecran.center().x - 2.0 * 14.0 + k as f32 * 14.0 + 2.0;
            p.rect_filled(
                Rect::from_min_size(Pos2::new(x, y), Vec2::splat(10.0)),
                CornerRadius::same(2),
                d::alpha(Color32::from_rgb(c[0], c[1], c[2]), tk * sortie),
            );
        }

        if self.fige.is_none() {
            ui.ctx().request_repaint();
        }
        if !self.ding && t >= T_DING && self.passee.is_none() {
            self.ding = true;
            return Etape::Ding;
        }
        Etape::EnCours
    }
}
