//! Écran de jeu : image de la console (filtres, palettes), barre de contrôle flottante,
//! indicateurs, emplacements d'état, carte de reprise, captures d'écran et entrées de jeu.

use super::*;
use crate::etats::{self, Cible, NB};
use eframe::egui::Painter;

/// Durée d'affichage de la barre de contrôle après un mouvement de souris (secondes).
const DUREE_BARRE: f64 = 2.0;
/// Durée des indicateurs « ÉTAT 3 », « ×4 »…
const DUREE_INDIC: f64 = 1.2;
const COLONNES_ETATS: usize = 5;

/// État de l'interface de jeu (barre, panneaux, filtres), propre à l'écran de jeu.
#[derive(Default)]
pub(super) struct Hud {
    t_souris: f64,
    souris: Option<Pos2>,
    /// Popover des filtres d'écran ouvert.
    ecran_ouvert: bool,
    avance_basculee: bool,
    /// Bouton « rembobiner » de la barre tenu.
    recul_souris: bool,
    rembobine: bool,
    indic: Option<(String, f64)>,
    /// Image affichée en virgule flottante (rémanence).
    prec: Vec<[f32; 3]>,
    vues: u64,
    panneau: Option<Panneau>,
    minis: Vec<Option<TextureHandle>>,
    tex_reprise: Option<TextureHandle>,
    reprise_sel: usize,
    palette_globale: Option<Palette>,
    /// Capture de vérification : barre affichée, popover ouvert.
    barre_forcee: bool,
}

struct Panneau {
    sel: usize,
    suppr: bool,
}

/// Raccourcis de l'écran de jeu.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum ActionJeu {
    SauverRapide,
    ChargerRapide,
    EmplacementPrec,
    EmplacementSuiv,
    Sauver(usize),
    Charger(usize),
    AnnulerChargement,
    AnnulerSauvegarde,
    PanneauEtats,
    Capture,
    AvanceBascule,
    VitesseSuivante,
    Muet,
}

/// Raccourcis de l'écran de jeu, lus dans la table d'actions de `touches.rs` (modifiables).
fn raccourcis_jeu(app: &App) -> Vec<ActionJeu> {
    use crate::touches::Action as T;
    use ActionJeu as A;
    let mut v = Vec::new();
    for n in 1..=NB {
        if app.action_appuyee(T::SauverEmplacement(n as u8)) {
            v.push(A::Sauver(n));
        }
        if app.action_appuyee(T::ChargerEmplacement(n as u8)) {
            v.push(A::Charger(n));
        }
    }
    let table = [
        (T::PanneauEtats, A::PanneauEtats),
        (T::SauverEtat, A::SauverRapide),
        (T::EmplacementPrecedent, A::EmplacementPrec),
        (T::EmplacementSuivant, A::EmplacementSuiv),
        (T::ChargerEtat, A::ChargerRapide),
        (T::AnnulerChargement, A::AnnulerChargement),
        (T::AnnulerSauvegarde, A::AnnulerSauvegarde),
        (T::Capture, A::Capture),
        (T::AvanceBascule, A::AvanceBascule),
        (T::VitesseSuivante, A::VitesseSuivante),
        (T::Muet, A::Muet),
    ];
    for (t, a) in table {
        if app.action_appuyee(t) {
            v.push(a);
        }
    }
    v
}

/// Texture à partir d'un PNG (miniature d'état).
fn texture_png(ctx: &egui::Context, nom: &str, png: &[u8]) -> Option<TextureHandle> {
    let img = crate::jaquettes::decoder(png)?;
    let ci = ColorImage::from_rgba_unmultiplied([img.largeur, img.hauteur], &img.rgba);
    Some(ctx.load_texture(nom, ci, TextureOptions::NEAREST))
}

fn uv() -> Rect {
    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0))
}

/// Nom de fichier sûr (captures).
fn nom_sur(s: &str) -> String {
    s.chars()
        .map(|c| {
            if "<>:\"/\\|?*".contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .take(80)
        .collect::<String>()
        .trim()
        .to_string()
}

// ----- Icônes de la barre (primitives, traits arrondis) -----

#[derive(Clone, Copy, PartialEq)]
enum Bouton {
    Pause,
    Recul,
    Vitesse,
    Etats,
    Capture,
    Ecran,
    Volume,
    PleinEcran,
}

fn icone_barre(p: &Painter, b: Bouton, c: Pos2, col: Color32, muet: bool) {
    let tr = Stroke::new(2.0, col);
    let tri = |pts: Vec<Pos2>| {
        p.add(egui::Shape::convex_polygon(pts, col, Stroke::NONE));
    };
    match b {
        Bouton::Pause => {
            for dx in [-4.0, 4.0] {
                p.rect_filled(
                    Rect::from_center_size(c + Vec2::new(dx, 0.0), Vec2::new(4.0, 14.0)),
                    CornerRadius::same(1),
                    col,
                );
            }
        }
        Bouton::Recul | Bouton::Vitesse => {
            let s = if b == Bouton::Recul { -1.0 } else { 1.0 };
            for dx in [-5.0, 3.0] {
                let x = c.x + dx * s;
                tri(if s > 0.0 {
                    vec![
                        Pos2::new(x - 3.0, c.y - 6.0),
                        Pos2::new(x + 5.0, c.y),
                        Pos2::new(x - 3.0, c.y + 6.0),
                    ]
                } else {
                    vec![
                        Pos2::new(x + 3.0, c.y - 6.0),
                        Pos2::new(x + 3.0, c.y + 6.0),
                        Pos2::new(x - 5.0, c.y),
                    ]
                });
            }
        }
        Bouton::Etats => {
            let r = Rect::from_center_size(c, Vec2::splat(15.0));
            p.rect_stroke(r, CornerRadius::same(3), tr, StrokeKind::Middle);
            p.rect_filled(
                Rect::from_min_size(r.min + Vec2::new(4.0, 0.0), Vec2::new(7.0, 5.0)),
                CornerRadius::same(1),
                col,
            );
            p.rect_filled(
                Rect::from_min_size(
                    Pos2::new(r.left() + 3.5, r.bottom() - 6.0),
                    Vec2::new(8.0, 4.0),
                ),
                CornerRadius::same(1),
                col,
            );
        }
        Bouton::Capture => {
            let r = Rect::from_center_size(c + Vec2::new(0.0, 1.0), Vec2::new(18.0, 13.0));
            p.rect_stroke(r, CornerRadius::same(3), tr, StrokeKind::Middle);
            p.circle_stroke(r.center(), 3.5, tr);
            p.rect_filled(
                Rect::from_min_size(r.min + Vec2::new(4.0, -3.0), Vec2::new(5.0, 3.0)),
                CornerRadius::same(1),
                col,
            );
        }
        Bouton::Ecran => {
            p.circle_stroke(c, 7.5, tr);
            let mut pts = vec![Pos2::new(c.x, c.y - 7.5)];
            for k in 0..=8 {
                let a = -std::f32::consts::FRAC_PI_2 + k as f32 * std::f32::consts::PI / 8.0;
                pts.push(c + Vec2::new(a.cos(), a.sin()) * 7.5);
            }
            tri(pts);
        }
        Bouton::Volume => {
            tri(vec![
                Pos2::new(c.x - 8.0, c.y - 3.5),
                Pos2::new(c.x - 4.0, c.y - 3.5),
                Pos2::new(c.x + 1.0, c.y - 8.0),
                Pos2::new(c.x + 1.0, c.y + 8.0),
                Pos2::new(c.x - 4.0, c.y + 3.5),
                Pos2::new(c.x - 8.0, c.y + 3.5),
            ]);
            if muet {
                let t = Stroke::new(2.0, col);
                p.line_segment([c + Vec2::new(4.0, -4.0), c + Vec2::new(10.0, 4.0)], t);
                p.line_segment([c + Vec2::new(4.0, 4.0), c + Vec2::new(10.0, -4.0)], t);
            } else {
                for (r, a) in [(5.0f32, 0.9f32), (9.0, 0.75)] {
                    let pts: Vec<Pos2> = (0..=6)
                        .map(|k| {
                            let ang = -a + k as f32 * a / 3.0;
                            Pos2::new(c.x + 1.0 + ang.cos() * r, c.y + ang.sin() * r)
                        })
                        .collect();
                    p.add(egui::Shape::line(pts, Stroke::new(1.8, col)));
                }
            }
        }
        Bouton::PleinEcran => {
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                let coin = c + Vec2::new(7.0 * sx, 6.0 * sy);
                p.line_segment([coin, coin - Vec2::new(5.0 * sx, 0.0)], tr);
                p.line_segment([coin, coin - Vec2::new(0.0, 4.5 * sy)], tr);
            }
        }
    }
}

fn coche(p: &Painter, c: Pos2, col: Color32) {
    p.add(egui::Shape::line(
        vec![
            c + Vec2::new(-5.0, 0.0),
            c + Vec2::new(-1.5, 3.5),
            c + Vec2::new(5.0, -4.0),
        ],
        Stroke::new(2.0, col),
    ));
}

fn cadenas(p: &Painter, c: Pos2, col: Color32) {
    p.rect_filled(
        Rect::from_center_size(c + Vec2::new(0.0, 2.0), Vec2::new(10.0, 8.0)),
        CornerRadius::same(2),
        col,
    );
    let pts: Vec<Pos2> = (0..=8)
        .map(|k| {
            let a = std::f32::consts::PI + k as f32 * std::f32::consts::PI / 8.0;
            c + Vec2::new(a.cos() * 3.2, -2.0 + a.sin() * 3.6)
        })
        .collect();
    p.add(egui::Shape::line(pts, Stroke::new(1.6, col)));
}

impl App {
    // ----- Début et fin de partie -----

    /// Juste après le lancement d'une partie : palette du jeu, miniature de reprise.
    pub(super) fn debut_jeu(&mut self) {
        let force = self.hud.barre_forcee;
        let cgb = self.partie.as_ref().is_some_and(|p| p.gb.couleur());
        self.hud = Hud {
            barre_forcee: force,
            // GBC, GBA : palette DMG sans objet, rien de mémorisé pour ce jeu.
            palette_globale: (!cgb).then_some(self.config.palette),
            ..Hud::default()
        };
        let Some(p) = &self.partie else { return };
        if let Some(&pal) = self.config.palettes_jeux.get(&format!("{:08X}", p.crc)) {
            if !cgb {
                self.config.palette = pal;
            }
        }
        if p.reprise.is_some() {
            if let Some(png) = p.etats.lire_mini(Cible::Auto) {
                self.hud.tex_reprise = texture_png(&self.ctx, "reprise", &png);
            }
        }
    }

    /// Avant de lâcher la partie : palette mémorisée pour ce jeu, état « auto » écrit.
    pub(super) fn fin_jeu(&mut self) {
        let Some(p) = &mut self.partie else { return };
        p.teintes = self.config.palette.teintes();
        p.fermer();
        let cle = format!("{:08X}", p.crc);
        if let Some(g) = self.hud.palette_globale.take() {
            if g != self.config.palette || self.config.palettes_jeux.contains_key(&cle) {
                self.config.palettes_jeux.insert(cle, self.config.palette);
            }
            self.config.palette = g;
            self.config.enregistrer(&self.chemins.config());
        }
    }

    // ----- Actions -----

    fn indiquer(&mut self, t: impl Into<String>) {
        self.hud.indic = Some((t.into(), self.t_prec + DUREE_INDIC));
    }

    /// Ouvre le panneau des emplacements (la partie est suspendue).
    pub(super) fn ouvrir_etats(&mut self) {
        let Some(p) = &self.partie else { return };
        self.hud.minis = (1..=NB)
            .map(|n| {
                p.etats
                    .lire_mini(Cible::Num(n))
                    .and_then(|d| texture_png(&self.ctx, &format!("etat{n}"), &d))
            })
            .collect();
        self.hud.panneau = Some(Panneau {
            sel: p.courant - 1,
            suppr: false,
        });
        self.hud.ecran_ouvert = false;
        if let Some(a) = &self.audio {
            a.vider();
        }
    }

    fn fermer_etats(&mut self) {
        self.hud.panneau = None;
        if let Some(p) = &mut self.partie {
            p.reprendre();
        }
    }

    fn rafraichir_mini(&mut self, n: usize) {
        let Some(p) = &self.partie else { return };
        if self.hud.minis.len() == NB {
            self.hud.minis[n - 1] = p
                .etats
                .lire_mini(Cible::Num(n))
                .and_then(|d| texture_png(&self.ctx, &format!("etat{n}"), &d));
        }
    }

    fn sauver_volume(&mut self) {
        self.config.enregistrer(&self.chemins.config());
    }

    fn executer(&mut self, a: ActionJeu) {
        use ActionJeu as A;
        let teintes = self.config.palette.teintes();
        let Some(p) = &mut self.partie else { return };
        p.teintes = teintes;
        match a {
            A::SauverRapide | A::Sauver(_) => {
                let n = if let A::Sauver(n) = a { n } else { p.courant };
                let r = p.sauver_etat(n);
                if r.is_ok() {
                    p.courant = n;
                }
                match r {
                    Ok(()) => {
                        self.indiquer(format!("ÉTAT {n} SAUVÉ"));
                        self.rafraichir_mini(n);
                    }
                    Err(m) => self.dire(m),
                }
            }
            A::ChargerRapide | A::Charger(_) => {
                let n = if let A::Charger(n) = a { n } else { p.courant };
                let r = p.charger_etat(Cible::Num(n));
                if r.is_ok() {
                    p.courant = n;
                }
                match r {
                    Ok(()) => {
                        self.indiquer(format!("ÉTAT {n} CHARGÉ"));
                        if let Some(au) = &self.audio {
                            au.vider();
                        }
                    }
                    Err(m) => self.dire(m),
                }
            }
            A::EmplacementPrec | A::EmplacementSuiv => {
                let d = if a == A::EmplacementSuiv { 1 } else { NB - 1 };
                p.courant = (p.courant - 1 + d) % NB + 1;
                let n = p.courant;
                let quand = p.etats.info(Cible::Num(n)).map_or("vide".to_string(), |i| {
                    etats::date_relative(i.date, etats::maintenant())
                });
                self.indiquer(format!("ÉTAT {n} · {quand}"));
            }
            A::AnnulerChargement => match p.annuler_chargement() {
                Ok(()) => self.indiquer("CHARGEMENT ANNULÉ"),
                Err(m) => self.dire(m),
            },
            A::AnnulerSauvegarde => match p.annuler_sauvegarde() {
                Ok(n) => {
                    self.dire(format!("Sauvegarde de l'emplacement {n} annulée"));
                    self.rafraichir_mini(n);
                }
                Err(m) => self.dire(m),
            },
            A::PanneauEtats => self.ouvrir_etats(),
            A::Capture => self.capture_ecran(),
            A::AvanceBascule => {
                self.hud.avance_basculee = !self.hud.avance_basculee;
                if !self.hud.avance_basculee {
                    p.reprendre();
                }
            }
            A::VitesseSuivante => {
                self.config.vitesse = self.config.vitesse.suivante();
                self.sauver_volume();
                self.indiquer(format!("VITESSE {}", self.config.vitesse.nom()));
            }
            A::Muet => {
                self.config.muet = !self.config.muet;
                self.sauver_volume();
                self.indiquer(if self.config.muet { "MUET" } else { "SON" });
            }
        }
    }

    /// Capture PNG ×4 de l'écran du jeu dans `captures/`.
    fn capture_ecran(&mut self) {
        let Some(p) = &self.partie else { return };
        const F: usize = 4;
        let (l, h) = (p.gb.largeur(), p.gb.hauteur());
        let ecran = p.gb.ecran_rgb(&self.config.palette.teintes());
        let mut rgb = Vec::with_capacity(l * h * F * F * 3);
        for y in 0..h * F {
            for x in 0..l * F {
                let i = ((y / F) * l + x / F) * 3;
                rgb.extend_from_slice(&ecran[i..i + 3]);
            }
        }
        let nom = format!("{} {}.png", nom_sur(&p.titre), etats::maintenant());
        let chemin = self.chemins.captures().join(&nom);
        // Écriture en fond : la synchronisation disque ne fige pas l'image.
        match etats::encoder_png(&rgb, (l * F) as u32, (h * F) as u32) {
            Some(png) => {
                crate::sauve::ecrire_en_fond(&chemin, png);
                self.dire(format!("Capture enregistrée : captures/{nom}"));
            }
            None => self.dire("Capture impossible : encodage PNG"),
        }
    }

    /// Vérification visuelle (`--capture`) : `--menu etats|barre|ecran|sauver:N`.
    pub(super) fn preparer_capture(&mut self, jeu_pret: bool) {
        let Some(m) = self.opts.menu.clone() else {
            return;
        };
        if !jeu_pret || self.partie.is_none() {
            return;
        }
        match m.as_str() {
            "etats" => {
                self.ouvrir_etats();
                self.images_menu = self.images_ui;
            }
            "barre" | "ecran" => {
                self.hud.barre_forcee = true;
                self.hud.ecran_ouvert = m == "ecran";
                self.images_menu = self.images_ui;
            }
            _ => {
                let Some(n) = m.strip_prefix("sauver:").and_then(|n| n.parse().ok()) else {
                    // Autres valeurs (pause, reprise…) : traitées ailleurs.
                    return;
                };
                if (1..=NB).contains(&n) {
                    self.executer(ActionJeu::Sauver(n));
                }
            }
        }
        self.opts.menu = None;
    }

    // ----- Dessin : jeu -----

    pub(super) fn dessiner_jeu(&mut self, ui: &mut egui::Ui, rapide: bool) {
        let ecran = ui.max_rect();
        let p = ui.painter().clone();
        p.rect_filled(ecran, CornerRadius::ZERO, FOND_JEU);
        let Some(partie) = &self.partie else { return };

        let retro = self.config.retroeclaire;
        let teintes = self.config.palette.teintes();
        let mut rgb = partie.gb.ecran_rgb(&teintes);
        let (lx, hx) = (partie.gb.largeur(), partie.gb.hauteur());
        // Teinte claire de référence (lueur, grille) : blanc en GBC et GBA.
        let mut c0 = if partie.gb.couleur() {
            [255; 3]
        } else {
            teintes[0]
        };
        if retro {
            // Rétroéclairé : noirs relevés, teintes éclaircies.
            let eclaircir = |v: u8| (v as f32 * 0.8 + 48.0) as u8;
            rgb.iter_mut().for_each(|v| *v = eclaircir(*v));
            c0 = c0.map(eclaircir);
        }
        let images = partie.images;
        let reman = self.config.remanence;
        let n = rgb.len() / 3;
        let px: Vec<Color32> = if reman {
            let neuf = images != self.hud.vues || self.hud.prec.len() != n;
            if self.hud.prec.len() != n {
                self.hud.prec = rgb
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|c| [c[0] as f32, c[1] as f32, c[2] as f32])
                    .collect();
            } else if neuf {
                for (d, c) in self.hud.prec.iter_mut().zip(rgb.as_chunks::<3>().0.iter()) {
                    for k in 0..3 {
                        d[k] = d[k] * 0.5 + c[k] as f32 * 0.5;
                    }
                }
            }
            self.hud
                .prec
                .iter()
                .map(|c| Color32::from_rgb(c[0] as u8, c[1] as u8, c[2] as u8))
                .collect()
        } else {
            self.hud.prec.clear();
            rgb.as_chunks::<3>()
                .0
                .iter()
                .map(|c| Color32::from_rgb(c[0], c[1], c[2]))
                .collect()
        };
        self.hud.vues = images;
        let img = ColorImage::new([lx, hx], px);
        match &mut self.tex_ecran {
            Some(t) => t.set(img, TextureOptions::NEAREST),
            None => {
                self.tex_ecran = Some(ui.ctx().load_texture("ecran", img, TextureOptions::NEAREST))
            }
        }

        // Échelle entière en pixels physiques : chaque pixel de la console reste net.
        let ppp = ui.ctx().pixels_per_point();
        let dispo = ecran.size() * ppp;
        let echelle = ((dispo.x / lx as f32).min(dispo.y / hx as f32).floor()).max(1.0);
        let taille = Vec2::new(lx as f32, hx as f32) * echelle / ppp;
        let coin = ((ecran.center() - taille * 0.5) * ppp).round() / ppp;
        let r = Rect::from_min_size(coin, taille);
        // Lueur douce de la teinte claire autour de l'écran (plus forte en rétroéclairé).
        let (force, flou) = if retro { (0.34, 72.0) } else { (0.16, 44.0) };
        p.add(
            egui::epaint::Shadow {
                offset: [0, 0],
                blur: flou as u8,
                spread: if retro { 6 } else { 0 },
                color: alpha(Color32::from_rgb(c0[0], c0[1], c0[2]), force),
            }
            .as_shape(r, CornerRadius::ZERO),
        );
        if let Some(t) = &self.tex_ecran {
            p.image(t.id(), r, uv(), Color32::WHITE);
            if retro {
                // Léger flou lumineux : copies décalées très transparentes.
                let ton = Color32::from_white_alpha(18);
                let d = (echelle * 0.35).max(1.0) / ppp;
                for v in [
                    Vec2::new(d, 0.0),
                    Vec2::new(-d, 0.0),
                    Vec2::new(0.0, d),
                    Vec2::new(0.0, -d),
                ] {
                    p.image(t.id(), r.translate(v), uv(), ton);
                }
            }
        }
        if self.config.grille && echelle >= 3.0 {
            let pas = echelle / ppp;
            let c = c0;
            let trait_lcd = Stroke::new(
                (echelle / 6.0).floor().max(1.0) / ppp,
                Color32::from_rgba_unmultiplied(c[0] / 2, c[1] / 2, c[2] / 2, 46),
            );
            for i in 1..lx {
                let x = r.left() + i as f32 * pas;
                p.line_segment([Pos2::new(x, r.top()), Pos2::new(x, r.bottom())], trait_lcd);
            }
            for j in 1..hx {
                let y = r.top() + j as f32 * pas;
                p.line_segment([Pos2::new(r.left(), y), Pos2::new(r.right(), y)], trait_lcd);
            }
        }
        let rembob = self.hud.rembobine;
        let remplissage = partie.rembob.remplissage();
        let erreur_sav = partie.erreur_sav;
        let reprise = partie.reprise;
        if rembob {
            // Rendu discret du recul : image un peu voilée, liseré d'accent.
            p.rect_filled(r, CornerRadius::ZERO, Color32::from_black_alpha(26));
            p.rect_stroke(
                r.expand(3.0),
                CornerRadius::same(4),
                Stroke::new(1.5, alpha(j().accent, 0.55)),
                StrokeKind::Outside,
            );
        }

        self.dessiner_indicateurs(&p, ecran, rapide, rembob, remplissage);
        if erreur_sav {
            let g = ligne(
                &p,
                "Écriture de la sauvegarde impossible : vérifiez le dossier gbemu-donnees",
                regulier(13.5),
                TEXTE,
            );
            let gs = g.size();
            let b = Rect::from_min_size(
                Pos2::new(ecran.left() + 24.0, ecran.top() + 20.0),
                Vec2::new(gs.x + 24.0, 30.0),
            );
            p.rect_filled(b, CornerRadius::same(15), alpha(ACCENT, 0.9));
            p.galley(
                Pos2::new(b.left() + 12.0, b.center().y - gs.y * 0.5),
                g,
                TEXTE,
            );
        }
        if reprise.is_some() {
            self.dessiner_reprise(ui, ecran, reprise.unwrap_or(0));
            return;
        }
        if self.hud.panneau.is_some() {
            p.rect_filled(ecran, CornerRadius::ZERO, Color32::from_black_alpha(170));
            self.dessiner_grille_etats(ui, ecran);
            return;
        }
        if self.menu.is_none() {
            self.dessiner_barre(ui, ecran);
        }
        // Rappel des touches au lancement, qui s'efface.
        let age = (self.t_prec - self.debut_partie) as f32;
        let a = (1.0 - (age - 3.5) / 0.8).clamp(0.0, 1.0);
        if a > 0.0 && self.menu.is_none() && !self.hud.barre_forcee {
            let pa = p.clone().with_clip_rect(ecran);
            let y = ecran.bottom() - 30.0;
            let puces = [
                ("Échap", "Menu"),
                ("Tab", "Avance rapide"),
                ("R", "Rembobiner"),
                ("F5  F8", "État"),
                ("X  Z", "A  B"),
                ("Entrée", "Start"),
            ];
            let mut l = 0.0;
            for (t, ac) in puces {
                l += ligne(&p, t, moyen(12.5), TEXTE).size().x
                    + 14.0
                    + 8.0
                    + ligne(&p, ac, regulier(13.5), DOUX).size().x
                    + 22.0;
            }
            let mut x = ecran.center().x - (l - 22.0) * 0.5;
            let fond =
                Rect::from_center_size(Pos2::new(ecran.center().x, y), Vec2::new(l + 18.0, 38.0));
            pa.rect_filled(fond, CornerRadius::same(19), alpha(FOND, 0.9 * a));
            let mut pa = pa;
            pa.multiply_opacity(a);
            for (t, ac) in puces {
                x += puce_avec(&pa, Pos2::new(x, y), t, ac, &NUIT) + 22.0;
            }
            ui.ctx().request_repaint();
        }
    }

    /// Pastilles en haut à droite : vitesse, rembobinage (durée restante), état sauvé…
    fn dessiner_indicateurs(
        &mut self,
        p: &Painter,
        ecran: Rect,
        rapide: bool,
        rembob: bool,
        remplissage: f32,
    ) {
        let t = j();
        let mut y = ecran.top() + 20.0;
        let droite = ecran.right() - 24.0;
        let pastille = |y: f32, texte: &str, a: f32, avec_barre: Option<f32>| {
            let mut pa = p.clone();
            pa.multiply_opacity(a);
            let g = ligne(&pa, texte, pixel(13.0), t.texte);
            let gs = g.size();
            let icone = if avec_barre.is_some() { 22.0 } else { 0.0 };
            let l = gs.x + 24.0 + icone + avec_barre.map_or(0.0, |_| 52.0);
            let b = Rect::from_min_size(Pos2::new(droite - l, y), Vec2::new(l, 30.0));
            pa.rect_filled(b, CornerRadius::same(15), alpha(t.surface_haute, 0.92));
            let mut x = b.left() + 12.0;
            if let Some(f) = avec_barre {
                icone_barre(
                    &pa,
                    Bouton::Recul,
                    Pos2::new(x + 8.0, b.center().y),
                    t.accent,
                    false,
                );
                x += icone;
                let barre = Rect::from_min_size(
                    Pos2::new(b.right() - 12.0 - 44.0, b.center().y - 2.0),
                    Vec2::new(44.0, 4.0),
                );
                pa.rect_filled(barre, CornerRadius::same(2), alpha(t.texte, 0.15));
                pa.rect_filled(
                    Rect::from_min_size(barre.min, Vec2::new(44.0 * f, 4.0)),
                    CornerRadius::same(2),
                    t.accent,
                );
            }
            pa.galley(Pos2::new(x, b.center().y - gs.y * 0.5), g, t.texte);
        };
        if rembob {
            pastille(y, "RECUL", 1.0, Some(remplissage.clamp(0.0, 1.0)));
            y += 38.0;
        } else if rapide {
            pastille(y, self.config.vitesse.nom(), 1.0, None);
            y += 38.0;
        }
        if let Some((texte, fin)) = &self.hud.indic {
            let reste = (*fin - self.t_prec) as f32;
            if reste > 0.0 {
                pastille(y, texte, (reste / 0.3).min(1.0), None);
                self.ctx.request_repaint();
            } else {
                self.hud.indic = None;
            }
        }
    }

    /// Barre de contrôle flottante : apparaît quand la souris bouge, disparaît après 2 s.
    fn dessiner_barre(&mut self, ui: &mut egui::Ui, ecran: Rect) {
        let t = j();
        let souris = ui.input(|i| i.pointer.hover_pos());
        if souris.is_some() && souris != self.hud.souris {
            self.hud.t_souris = self.t_prec;
        }
        self.hud.souris = souris;
        const BOUTONS: [Bouton; 8] = [
            Bouton::Pause,
            Bouton::Recul,
            Bouton::Vitesse,
            Bouton::Etats,
            Bouton::Capture,
            Bouton::Ecran,
            Bouton::Volume,
            Bouton::PleinEcran,
        ];
        let (cote, ecart, curseur) = (40.0, 4.0, 76.0);
        let l_vitesse = 30.0;
        let l = BOUTONS.len() as f32 * (cote + ecart) - ecart + l_vitesse + curseur + 24.0;
        let barre = Rect::from_center_size(
            Pos2::new(ecran.center().x, ecran.bottom() - 46.0),
            Vec2::new(l, cote + 14.0),
        );
        let survol = souris.is_some_and(|s| barre.expand(12.0).contains(s));
        let age = self.t_prec - self.hud.t_souris;
        let visible = self.hud.barre_forcee
            || self.hud.ecran_ouvert
            || survol
            || (self.hud.t_souris > 0.0 && age < DUREE_BARRE);
        let a = ui.ctx().animate_bool_with_time(
            Id::unique("barre-jeu"),
            visible,
            duree_fondu(if visible { 0.16 } else { 0.3 }),
        );
        // Capture de vérification : barre pleinement affichée, sans attendre le fondu.
        let a = if self.hud.barre_forcee { 1.0 } else { a };
        self.hud.recul_souris = false;
        if a <= 0.01 {
            self.hud.ecran_ouvert = false;
            return;
        }
        if visible {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        let mut p = ui.painter().clone();
        p.multiply_opacity(a);
        let barre = barre.translate(Vec2::new(0.0, (1.0 - a) * 10.0));
        ombre(&p, barre, 27.0, 10.0, 28.0, 1.0);
        p.rect_filled(barre, CornerRadius::same(27), alpha(t.surface, 0.94));
        p.rect_stroke(
            barre,
            CornerRadius::same(27),
            Stroke::new(1.0, t.trait_),
            StrokeKind::Inside,
        );
        let muet = self.config.muet;
        let mut x = barre.left() + 12.0;
        let mut clic = None;
        let mut bulle = None;
        let mut zone_ecran = Rect::NOTHING;
        for (k, &b) in BOUTONS.iter().enumerate() {
            let w = if b == Bouton::Vitesse {
                cote + l_vitesse
            } else {
                cote
            };
            let rb = Rect::from_min_size(
                Pos2::new(x, barre.center().y - cote * 0.5),
                Vec2::new(w, cote),
            );
            x += w + ecart;
            let rep = ui.interact(rb, Id::unique(("barre", k)), Sense::CLICK | Sense::DRAG);
            let actif = (b == Bouton::Ecran && self.hud.ecran_ouvert)
                || (b == Bouton::Recul && self.hud.rembobine)
                || (b == Bouton::Vitesse && self.hud.avance_basculee);
            if rep.hovered() || actif {
                p.rect_filled(rb, CornerRadius::same(20), alpha(t.surface_haute, 0.9));
            }
            let col = if actif { t.accent_clair } else { t.texte };
            let c = if b == Bouton::Vitesse {
                Pos2::new(rb.left() + cote * 0.5, rb.center().y)
            } else {
                rb.center()
            };
            icone_barre(&p, b, c, col, muet);
            if b == Bouton::Vitesse {
                let g = ligne(&p, self.config.vitesse.nom(), pixel(12.0), col);
                p.galley(
                    Pos2::new(rb.left() + cote - 4.0, rb.center().y - g.size().y * 0.5),
                    g,
                    col,
                );
            }
            if b == Bouton::Ecran {
                zone_ecran = rb;
            }
            if b == Bouton::Recul && rep.is_pointer_button_down_on() {
                self.hud.recul_souris = true;
            }
            if rep.hovered() {
                bulle = Some((
                    rb,
                    match b {
                        Bouton::Pause => "Pause (Échap)",
                        Bouton::Recul => "Rembobiner (R, maintenir)",
                        Bouton::Vitesse => "Avance rapide (Maj+Tab) · clic droit : vitesse",
                        Bouton::Etats => "États (F4)",
                        Bouton::Capture => "Capture d'écran (F12)",
                        Bouton::Ecran => "Filtres et palette",
                        Bouton::Volume => "Muet (M)",
                        Bouton::PleinEcran => "Plein écran (F11)",
                    },
                ));
            }
            if rep.clicked() {
                clic = Some(b);
            }
            if b == Bouton::Vitesse && rep.secondary_clicked() {
                self.executer(ActionJeu::VitesseSuivante);
            }
        }
        // Curseur de volume.
        let rv = Rect::from_min_size(
            Pos2::new(x + 2.0, barre.center().y - 12.0),
            Vec2::new(curseur - 10.0, 24.0),
        );
        let rep = ui.interact(rv, Id::unique("barre-volume"), Sense::CLICK | Sense::DRAG);
        if let Some(pos) = rep
            .interact_pointer_pos()
            .filter(|_| rep.is_pointer_button_down_on())
        {
            let v = ((pos.x - rv.left()) / rv.width()).clamp(0.0, 1.0);
            self.config.volume = Some((v * 20.0).round() / 20.0);
            self.config.muet = false;
        }
        if rep.drag_stopped() || rep.clicked() {
            self.sauver_volume();
        }
        let vol = if self.config.muet {
            0.0
        } else {
            self.config.volume.unwrap_or(1.0)
        };
        let piste = Rect::from_center_size(rv.center(), Vec2::new(rv.width(), 4.0));
        p.rect_filled(piste, CornerRadius::same(2), alpha(t.texte, 0.18));
        p.rect_filled(
            Rect::from_min_size(piste.min, Vec2::new(piste.width() * vol, 4.0)),
            CornerRadius::same(2),
            t.accent,
        );
        p.circle_filled(
            Pos2::new(piste.left() + piste.width() * vol, piste.center().y),
            6.0,
            t.texte,
        );

        if let Some((rb, texte)) = bulle {
            let g = ligne(&p, texte, regulier(13.0), t.texte);
            let gs = g.size();
            let rbul = Rect::from_center_size(
                Pos2::new(rb.center().x, barre.top() - 22.0),
                Vec2::new(gs.x + 20.0, 26.0),
            );
            let rbul = rbul.translate(Vec2::new(
                (ecran.left() + 8.0 - rbul.left()).max(0.0)
                    + (ecran.right() - 8.0 - rbul.right()).min(0.0),
                0.0,
            ));
            if !self.hud.ecran_ouvert {
                p.rect_filled(rbul, CornerRadius::same(8), t.surface_haute);
                p.galley(rbul.center() - gs * 0.5, g, t.texte);
            }
        }
        if self.hud.ecran_ouvert {
            self.dessiner_filtres(ui, &p, zone_ecran, barre.top() - 10.0);
        }
        match clic {
            Some(Bouton::Pause) => self.ouvrir_menu(GenreMenu::Pause),
            Some(Bouton::Recul) => {}
            Some(Bouton::Vitesse) => self.executer(ActionJeu::AvanceBascule),
            Some(Bouton::Etats) => self.ouvrir_etats(),
            Some(Bouton::Capture) => self.capture_ecran(),
            Some(Bouton::Ecran) => self.hud.ecran_ouvert = !self.hud.ecran_ouvert,
            Some(Bouton::Volume) => self.executer(ActionJeu::Muet),
            Some(Bouton::PleinEcran) => self.basculer_plein_ecran(),
            None => {}
        }
    }

    /// Popover des filtres d'écran et de la palette (au-dessus du bouton « Écran »).
    fn dessiner_filtres(&mut self, ui: &egui::Ui, p: &Painter, bouton: Rect, bas: f32) {
        let t = j();
        let cgb = self.partie.as_ref().is_some_and(|p| p.gb.is_cgb());
        let gba = self.partie.as_ref().is_some_and(|p| p.gb.is_gba());
        // GBC : la palette DMG est remplacée par la correction des couleurs ; GBA : ni l'une
        // ni l'autre (raccourci : pas de correction des couleurs GBA, à ajouter dans gba-core).
        let mut lignes = vec![
            ("Grille LCD", Some(self.config.grille)),
            ("Rémanence", Some(self.config.remanence)),
            ("Rétroéclairé", Some(self.config.retroeclaire)),
        ];
        if !gba {
            lignes.push(if cgb {
                (
                    "Correction des couleurs",
                    Some(!self.config.sans_correction),
                )
            } else {
                ("Palette", None)
            });
        }
        let (l, h_l) = (250.0, 40.0);
        let h = lignes.len() as f32 * h_l + 16.0;
        let r = Rect::from_min_size(
            Pos2::new(bouton.center().x - l * 0.5, bas - h),
            Vec2::new(l, h),
        );
        ombre(p, r, 14.0, 8.0, 24.0, 1.0);
        p.rect_filled(r, CornerRadius::same(14), t.surface);
        p.rect_stroke(
            r,
            CornerRadius::same(14),
            Stroke::new(1.0, t.trait_),
            StrokeKind::Inside,
        );
        let mut choix = None;
        for (k, (lib, val)) in lignes.iter().enumerate() {
            let rl = Rect::from_min_size(
                Pos2::new(r.left() + 8.0, r.top() + 8.0 + k as f32 * h_l),
                Vec2::new(l - 16.0, h_l - 2.0),
            );
            let rep = ui.interact(rl, Id::unique(("filtre", k)), Sense::CLICK);
            if rep.hovered() {
                p.rect_filled(rl, CornerRadius::same(9), t.surface_haute);
            }
            let g = ligne(p, lib, moyen(14.5), t.texte);
            p.galley(
                Pos2::new(rl.left() + 12.0, rl.center().y - g.size().y * 0.5),
                g,
                t.texte,
            );
            match val {
                Some(oui) => {
                    let case = Rect::from_center_size(
                        Pos2::new(rl.right() - 20.0, rl.center().y),
                        Vec2::splat(18.0),
                    );
                    if *oui {
                        p.rect_filled(case, CornerRadius::same(5), t.accent);
                        coche(p, case.center(), t.sur_accent);
                    } else {
                        p.rect_stroke(
                            case,
                            CornerRadius::same(5),
                            Stroke::new(1.5, t.doux),
                            StrokeKind::Inside,
                        );
                    }
                }
                None => {
                    let pal = self.config.palette;
                    let te = pal.teintes();
                    let g = ligne(p, pal.nom(), regulier(13.5), t.doux);
                    let gs = g.size();
                    let x0 = rl.right() - 12.0 - 4.0 * 12.0;
                    for (i, c) in te.iter().enumerate() {
                        p.rect_filled(
                            Rect::from_min_size(
                                Pos2::new(x0 + i as f32 * 12.0, rl.center().y - 6.0),
                                Vec2::splat(12.0),
                            ),
                            CornerRadius::ZERO,
                            Color32::from_rgb(c[0], c[1], c[2]),
                        );
                    }
                    p.galley(
                        Pos2::new(x0 - 10.0 - gs.x, rl.center().y - gs.y * 0.5),
                        g,
                        t.doux,
                    );
                }
            }
            if rep.clicked() {
                choix = Some(k);
            }
        }
        if let Some(k) = choix {
            match k {
                0 => self.config.grille = !self.config.grille,
                1 => self.config.remanence = !self.config.remanence,
                2 => self.config.retroeclaire = !self.config.retroeclaire,
                _ if cgb => {
                    self.config.sans_correction = !self.config.sans_correction;
                    if let Some(pa) = &mut self.partie {
                        pa.gb.set_color_correction(!self.config.sans_correction);
                    }
                }
                _ => {
                    self.config.palette = self.config.palette.suivante();
                    if let Some(pa) = &self.partie {
                        let cle = format!("{:08X}", pa.crc);
                        self.config.palettes_jeux.insert(cle, self.config.palette);
                    }
                }
            }
            self.config.enregistrer(&self.chemins.config());
        }
        // Clic ailleurs : fermeture.
        let ailleurs = ui.input(|i| {
            i.pointer.any_pressed()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|q| !r.contains(q) && !bouton.contains(q))
        });
        if ailleurs {
            self.hud.ecran_ouvert = false;
        }
    }

    /// Grille des 10 emplacements (miniature, numéro, date, verrou), plein écran sur le jeu.
    /// Réutilisable par le menu pause.
    pub(super) fn dessiner_grille_etats(&mut self, ui: &mut egui::Ui, ecran: Rect) {
        let t = j();
        let Some(pan) = &self.hud.panneau else { return };
        let (sel, suppr) = (pan.sel, pan.suppr);
        let Some(partie) = &self.partie else { return };
        let infos: Vec<Option<etats::Info>> =
            (1..=NB).map(|n| partie.etats.info(Cible::Num(n))).collect();
        let courant = partie.courant;
        let titre = partie.titre.clone();
        let maintenant = etats::maintenant();
        let p = ui.painter().clone();

        let ecart = 14.0;
        let dispo = (ecran.width() - 48.0 - 56.0).min(5.0 * 168.0 + 4.0 * ecart);
        let lt = ((dispo - 4.0 * ecart) / COLONNES_ETATS as f32).max(80.0);
        let hm = lt * partie.gb.hauteur() as f32 / partie.gb.largeur() as f32;
        let ht = hm + 46.0;
        let lc = lt * COLONNES_ETATS as f32 + 4.0 * ecart + 56.0;
        let hc = 88.0 + 2.0 * ht + ecart + 78.0;
        let carte = Rect::from_center_size(ecran.center(), Vec2::new(lc, hc));
        ombre(&p, carte, 20.0, 18.0, 48.0, 1.2);
        panneau(&p, carte, 20.0, t.surface, Some(t.trait_));
        let g = ligne(&p, "États", gras(22.0), t.texte);
        p.galley(carte.min + Vec2::new(28.0, 24.0), g, t.texte);
        let g = bloc(&p, &titre, regulier(14.0), t.doux, lc - 56.0, 1);
        p.galley(carte.min + Vec2::new(28.0, 56.0), g, t.doux);

        let mut clic = None;
        for (k, info) in infos.iter().enumerate() {
            let (col, lig) = (k % COLONNES_ETATS, k / COLONNES_ETATS);
            let rt = Rect::from_min_size(
                Pos2::new(
                    carte.left() + 28.0 + col as f32 * (lt + ecart),
                    carte.top() + 88.0 + lig as f32 * (ht + ecart),
                ),
                Vec2::new(lt, ht),
            );
            let rep = ui.interact(rt, Id::unique(("etat", k)), Sense::CLICK);
            let choisi = k == sel;
            let rm = Rect::from_min_size(rt.min, Vec2::new(lt, hm));
            // La case garde le fond sombre du jeu : encre de nuit dans les deux thèmes.
            let n = &NUIT;
            p.rect_filled(rt, CornerRadius::same(10), t.fond_jeu);
            match (info, self.hud.minis.get(k).and_then(|m| m.as_ref())) {
                (Some(_), Some(tex)) => {
                    p.image(tex.id(), rm, uv(), Color32::WHITE);
                }
                (Some(_), None) => {
                    let g = ligne(&p, "?", pixel(20.0), n.discret);
                    p.galley(rm.center() - g.size() * 0.5, g, n.discret);
                }
                (None, _) => {
                    let c = rm.center();
                    let tr = Stroke::new(2.0, n.discret);
                    p.line_segment([c - Vec2::new(9.0, 0.0), c + Vec2::new(9.0, 0.0)], tr);
                    p.line_segment([c - Vec2::new(0.0, 9.0), c + Vec2::new(0.0, 9.0)], tr);
                }
            }
            let yb = rm.bottom() + 23.0;
            let g = ligne(&p, &(k + 1).to_string(), pixel(15.0), n.texte);
            let gl = g.size().x;
            p.galley(
                Pos2::new(rt.left() + 10.0, yb - g.size().y * 0.5),
                g,
                n.texte,
            );
            let date = match info {
                Some(i) => etats::date_relative(i.date, maintenant),
                None => "vide".into(),
            };
            let g = bloc(&p, &date, regulier(12.5), n.doux, lt - gl - 44.0, 1);
            p.galley(
                Pos2::new(rt.left() + 18.0 + gl, yb - g.size().y * 0.5),
                g,
                n.doux,
            );
            if info.is_some_and(|i| i.verrou) {
                cadenas(&p, Pos2::new(rt.right() - 14.0, yb), n.doux);
            }
            if k + 1 == courant {
                p.circle_filled(Pos2::new(rm.right() - 10.0, rm.top() + 10.0), 4.0, t.accent);
            }
            if choisi && suppr {
                p.rect_filled(rm, CornerRadius::ZERO, Color32::from_black_alpha(170));
                let g = bloc(
                    &p,
                    "Supprimer ? Suppr pour confirmer",
                    moyen(13.0),
                    n.texte,
                    lt - 16.0,
                    3,
                );
                p.galley(rm.center() - g.size() * 0.5, g, n.texte);
            }
            if choisi {
                p.rect_stroke(
                    rt.expand(2.0),
                    CornerRadius::same(12),
                    Stroke::new(2.0, t.accent),
                    StrokeKind::Outside,
                );
            } else if rep.hovered() {
                p.rect_stroke(
                    rt.expand(1.0),
                    CornerRadius::same(11),
                    Stroke::new(1.0, t.doux),
                    StrokeKind::Outside,
                );
            }
            if rep.clicked() {
                clic = Some((k, false));
            } else if rep.secondary_clicked() {
                clic = Some((k, true));
            }
        }
        let y = carte.bottom() - 40.0;
        let mut x = carte.left() + 28.0;
        let mut puces = [
            ("Entrée", "Charger"),
            ("S", "Sauver ici"),
            ("V", "Verrouiller"),
            ("Suppr", "Supprimer"),
            ("Ctrl+Z", "Annuler le chargement"),
            ("Échap", "Fermer"),
        ];
        // Fenêtre étroite : libellé court plutôt qu'un pied qui déborde du panneau.
        let largeur: f32 = puces
            .iter()
            .map(|(tc, ac)| largeur_puce(&p, tc, ac) + 20.0)
            .sum();
        if largeur - 20.0 > lc - 56.0 {
            puces[4].1 = "Annuler";
        }
        for (tc, ac) in puces {
            x += puce(&p, Pos2::new(x, y), tc, ac) + 20.0;
        }
        if let Some((k, droit)) = clic {
            if let Some(pan) = &mut self.hud.panneau {
                pan.sel = k;
                pan.suppr = false;
            }
            let plein = infos[k].is_some();
            if droit || !plein {
                self.panneau_agir(PanAct::Sauver);
            } else {
                self.panneau_agir(PanAct::Charger);
            }
        }
    }

    fn panneau_agir(&mut self, act: PanAct) {
        let Some(pan) = &mut self.hud.panneau else {
            return;
        };
        let n = pan.sel + 1;
        let confirme = pan.suppr;
        pan.suppr = act == PanAct::Supprimer && !confirme;
        let Some(p) = &self.partie else { return };
        let info = p.etats.info(Cible::Num(n));
        match act {
            PanAct::Charger => {
                self.executer(ActionJeu::Charger(n));
                if self.partie.as_ref().is_some_and(|p| p.courant == n) && info.is_some() {
                    self.fermer_etats();
                }
            }
            PanAct::Sauver => self.executer(ActionJeu::Sauver(n)),
            PanAct::Verrouiller => {
                let Some(info) = info else {
                    self.dire(format!("Emplacement {n} vide"));
                    return;
                };
                match p.etats.verrouiller(Cible::Num(n), !info.verrou) {
                    Ok(()) => self.indiquer(if info.verrou {
                        format!("ÉTAT {n} DÉVERROUILLÉ")
                    } else {
                        format!("ÉTAT {n} VERROUILLÉ")
                    }),
                    Err(e) => self.dire(format!("Verrou impossible : {e}")),
                }
            }
            PanAct::Supprimer => {
                let Some(info) = info else {
                    if let Some(pan) = &mut self.hud.panneau {
                        pan.suppr = false;
                    }
                    return;
                };
                if info.verrou {
                    if let Some(pan) = &mut self.hud.panneau {
                        pan.suppr = false;
                    }
                    self.dire(format!("Emplacement {n} verrouillé"));
                } else if confirme {
                    match p.etats.supprimer(Cible::Num(n)) {
                        Ok(()) => {
                            self.rafraichir_mini(n);
                            self.indiquer(format!("ÉTAT {n} SUPPRIMÉ"));
                        }
                        Err(e) => self.dire(format!("Suppression impossible : {e}")),
                    }
                }
            }
        }
    }

    /// Carte « Reprendre là où vous étiez ? » au lancement d'un jeu qui a un état auto.
    fn dessiner_reprise(&mut self, ui: &mut egui::Ui, ecran: Rect, date: u64) {
        let t = j();
        let p = ui.painter().clone();
        p.rect_filled(ecran, CornerRadius::ZERO, alpha(t.fond_jeu, 0.94));
        let (lm, hm) = (200.0, 180.0);
        let carte = Rect::from_center_size(ecran.center(), Vec2::new(620.0, hm + 56.0 + 60.0));
        ombre(&p, carte, 20.0, 18.0, 48.0, 1.2);
        panneau(&p, carte, 20.0, t.surface, Some(t.trait_));
        let rm = Rect::from_min_size(carte.min + Vec2::new(28.0, 28.0), Vec2::new(lm, hm));
        p.rect_filled(rm, CornerRadius::same(6), t.fond_jeu);
        if let Some(tex) = &self.hud.tex_reprise {
            p.image(tex.id(), rm, uv(), Color32::WHITE);
        }
        let x = rm.right() + 28.0;
        let g = ligne(&p, "Reprendre là où vous étiez ?", gras(20.0), t.texte);
        p.galley(Pos2::new(x, rm.top() + 2.0), g, t.texte);
        let quand = etats::date_relative(date, etats::maintenant());
        let g = ligne(
            &p,
            &format!("Partie quittée {quand}"),
            regulier(14.0),
            t.doux,
        );
        p.galley(Pos2::new(x, rm.top() + 34.0), g, t.doux);
        let choix = [
            ("Reprendre", "là où vous étiez"),
            ("Recommencer", "depuis la sauvegarde du jeu"),
        ];
        let mut clic = None;
        for (k, (lib, sous)) in choix.iter().enumerate() {
            let rb = Rect::from_min_size(
                Pos2::new(x, rm.top() + 72.0 + k as f32 * 54.0),
                Vec2::new(carte.right() - 28.0 - x, 48.0),
            );
            let rep = ui.interact(rb, Id::unique(("reprise", k)), Sense::CLICK);
            let choisi = self.hud.reprise_sel == k;
            if choisi {
                p.rect_filled(rb, CornerRadius::same(11), t.surface_haute);
                p.rect_filled(
                    Rect::from_min_size(rb.min + Vec2::new(0.0, 12.0), Vec2::new(3.0, 24.0)),
                    CornerRadius::same(2),
                    t.accent,
                );
            } else if rep.hovered() {
                p.rect_filled(rb, CornerRadius::same(11), alpha(t.surface_haute, 0.5));
            }
            let g = ligne(&p, lib, moyen(16.0), if choisi { t.texte } else { t.doux });
            let gl = g.size();
            p.galley(
                Pos2::new(rb.left() + 16.0, rb.center().y - gl.y * 0.5),
                g,
                t.texte,
            );
            let g = ligne(&p, sous, regulier(13.0), t.discret);
            p.galley(
                Pos2::new(rb.left() + 26.0 + gl.x, rb.center().y - g.size().y * 0.5),
                g,
                t.discret,
            );
            if rep.clicked() {
                clic = Some(k);
            }
        }
        let y = carte.bottom() - 32.0;
        let mut xp = carte.left() + 28.0;
        for (tc, ac) in [("Entrée", "Choisir"), ("Échap", "Recommencer")] {
            xp += puce(&p, Pos2::new(xp, y), tc, ac) + 20.0;
        }
        if let Some(k) = clic {
            self.repondre_reprise(k == 0);
        }
    }

    fn repondre_reprise(&mut self, reprendre: bool) {
        let Some(p) = &mut self.partie else { return };
        if let Err(m) = p.repondre_reprise(reprendre) {
            self.dire(m);
        }
        self.hud.tex_reprise = None;
        self.attendre_relache = true;
        if let Some(a) = &self.audio {
            a.vider();
        }
    }

    // ----- Entrées -----

    /// Carte de reprise : gauche/droite, Entrée/A choisit, Échap/B recommence.
    fn entrees_reprise(&mut self, ui: &egui::Ui, pad: Etat) {
        let (mut dx, mut valider, mut non) = (0i32, false, false);
        ui.input(|i| {
            for (k, d) in [
                (Key::ArrowUp, -1),
                (Key::ArrowLeft, -1),
                (Key::ArrowDown, 1),
                (Key::ArrowRight, 1),
            ] {
                if i.key_pressed(k) {
                    dx += d;
                }
            }
            valider = i.key_pressed(Key::Enter) || i.key_pressed(Key::Space);
            non = i.key_pressed(Key::Escape);
        });
        if pad.appuis & (b::HAUT | b::GAUCHE) != 0 {
            dx -= 1;
        }
        if pad.appuis & (b::BAS | b::DROITE) != 0 {
            dx += 1;
        }
        valider |= pad.appuis & (b::EST | b::START) != 0;
        non |= pad.appuis & b::SUD != 0;
        if dx != 0 {
            self.hud.reprise_sel = (self.hud.reprise_sel as i32 + dx).rem_euclid(2) as usize;
        }
        if non {
            self.repondre_reprise(false);
        } else if valider {
            self.repondre_reprise(self.hud.reprise_sel == 0);
        }
    }

    /// Panneau des emplacements : flèches, Entrée/A charge, S/Ouest sauve, V/Nord verrouille,
    /// Suppr/Select supprime (deux fois), Échap/F4/B ferme.
    fn entrees_etats(&mut self, ui: &egui::Ui, pad: Etat) {
        let (mut dx, mut dy) = (0i32, 0i32);
        let mut act = None;
        let mut fermer = false;
        let annuler = ui.input(|i| i.key_pressed(Key::Z) && i.modifiers.command);
        ui.input(|i| {
            dx += i.key_pressed(Key::ArrowRight) as i32 - i.key_pressed(Key::ArrowLeft) as i32;
            dy += i.key_pressed(Key::ArrowDown) as i32 - i.key_pressed(Key::ArrowUp) as i32;
            if i.key_pressed(Key::Enter) || i.key_pressed(Key::Space) {
                act = Some(PanAct::Charger);
            }
            if i.key_pressed(Key::S) {
                act = Some(PanAct::Sauver);
            }
            if i.key_pressed(Key::V) {
                act = Some(PanAct::Verrouiller);
            }
            if i.key_pressed(Key::Delete) {
                act = Some(PanAct::Supprimer);
            }
            fermer = i.key_pressed(Key::Escape) || i.key_pressed(Key::F4);
        });
        dx += (pad.appuis & b::DROITE != 0) as i32 - (pad.appuis & b::GAUCHE != 0) as i32;
        dy += (pad.appuis & b::BAS != 0) as i32 - (pad.appuis & b::HAUT != 0) as i32;
        if pad.appuis & b::EST != 0 {
            act = Some(PanAct::Charger);
        }
        if pad.appuis & b::OUEST != 0 {
            act = Some(PanAct::Sauver);
        }
        if pad.appuis & b::NORD != 0 {
            act = Some(PanAct::Verrouiller);
        }
        if pad.appuis & b::SELECT != 0 {
            act = Some(PanAct::Supprimer);
        }
        fermer |= pad.appuis & b::SUD != 0;
        if fermer {
            self.fermer_etats();
            return;
        }
        if annuler {
            self.executer(ActionJeu::AnnulerChargement);
            return;
        }
        if let Some(pan) = &mut self.hud.panneau {
            if dx != 0 || dy != 0 {
                let (c, l) = (pan.sel % COLONNES_ETATS, pan.sel / COLONNES_ETATS);
                let c = (c as i32 + dx).rem_euclid(COLONNES_ETATS as i32) as usize;
                let l = (l as i32 + dy).rem_euclid((NB / COLONNES_ETATS) as i32) as usize;
                pan.sel = l * COLONNES_ETATS + c;
                pan.suppr = false;
            }
        }
        if let Some(a) = act {
            let charger_vide = a == PanAct::Charger
                && self.hud.panneau.as_ref().is_some_and(|pan| {
                    self.partie
                        .as_ref()
                        .is_some_and(|p| p.etats.info(Cible::Num(pan.sel + 1)).is_none())
                });
            // Entrée sur un emplacement vide : on y sauve.
            self.panneau_agir(if charger_vide { PanAct::Sauver } else { a });
        }
    }

    /// Robot de test : la partie tourne à vitesse normale, son poussé (ni pause, ni carte de
    /// reprise, ni panneau des états, ni avance rapide, ni rembobinage).
    pub(super) fn jeu_au_rythme(&self) -> bool {
        self.partie.as_ref().is_some_and(|p| p.reprise.is_none())
            && self.menu.is_none()
            && self.ecran_touches.is_none()
            && self.hud.panneau.is_none()
            && !self.hud.rembobine
            && !self.hud.avance_basculee
            && !self.action_tenue(Action::AvanceRapide)
    }

    /// Entrées pendant la partie (pause, raccourcis, avance rapide, rembobinage, boutons) ;
    /// renvoie vrai si l'avance rapide (ou le ralenti) est active.
    pub(super) fn entrees_jeu(&mut self, ui: &egui::Ui, pad: Etat, maintenant: f64) -> bool {
        if let Some(p) = &mut self.partie {
            p.teintes = self.config.palette.teintes();
        }
        if self.partie.as_ref().is_some_and(|p| p.reprise.is_some()) {
            if self.opts.capture.is_none() {
                self.entrees_reprise(ui, pad);
            }
            self.hud.rembobine = false;
            return false;
        }
        if self.hud.panneau.is_some() {
            self.entrees_etats(ui, pad);
            self.hud.rembobine = false;
            return false;
        }
        // Pause : Échap, Select+Start à la manette (table d'actions).
        let pause = self.action_appuyee(Action::Pause);
        if pause && self.hud.ecran_ouvert {
            self.hud.ecran_ouvert = false;
            return false;
        }
        if pause {
            self.ouvrir_menu(GenreMenu::Pause);
            return false;
        }
        for a in raccourcis_jeu(self) {
            self.executer(a);
            if self.hud.panneau.is_some() {
                return false;
            }
        }
        let rapide = self.action_tenue(Action::AvanceRapide) || self.hud.avance_basculee;
        let rembob = self.action_tenue(Action::Rembobiner) || self.hud.recul_souris;
        if self.hud.rembobine && !rembob {
            if let Some(p) = &mut self.partie {
                p.reprendre();
            }
        }
        self.hud.rembobine = rembob;
        let boutons = self.boutons_jeu(maintenant);
        // La touche qui a lancé le jeu (Entrée, A) ne doit pas appuyer sur Start.
        if self.attendre_relache {
            self.attendre_relache = boutons != 0;
        }
        let boutons = if self.attendre_relache { 0 } else { boutons };
        let vitesse = if rapide {
            self.config.vitesse.facteur()
        } else {
            1.0
        };
        let volume = if self.config.muet {
            0.0
        } else {
            self.config.volume.unwrap_or(1.0)
        };
        if let Some(p) = &mut self.partie {
            p.gb.set_buttons(boutons);
            p.avancer(maintenant, vitesse, rembob, self.audio.as_ref(), volume);
        }
        rapide
    }
}

#[derive(Clone, Copy, PartialEq)]
enum PanAct {
    Charger,
    Sauver,
    Verrouiller,
    Supprimer,
}
