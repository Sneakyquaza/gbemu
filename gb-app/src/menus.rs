//! Menus (pause, réglages) : éléments, actions, entrées et dessin.

use super::*;
use crate::sons_ui::Son;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum GenreMenu {
    Pause,
    Reglages,
}

#[derive(Clone, Copy, PartialEq)]
enum Act {
    Reprendre,
    Grille,
    Palette,
    PleinEcran,
    Quitter,
    DossierJeux,
    OuvrirDossier,
    OublierAjouts,
    Reanalyser,
    Etats,
    Theme,
    Animations,
    SonsUi,
    Intro,
    Touches,
    Fermer,
}

pub(super) struct Menu {
    genre: GenreMenu,
    sel: usize,
    /// Saisie en cours du chemin du dossier des jeux.
    edition: Option<String>,
    /// Instant d'ouverture (animations d'entrée).
    ouvert_a: f64,
}

impl App {
    // ----- Menus -----

    pub(super) fn ouvrir_menu(&mut self, genre: GenreMenu) {
        self.menu = Some(Menu {
            genre,
            sel: 0,
            edition: None,
            ouvert_a: self.t_prec,
        });
        if genre == GenreMenu::Pause {
            self.figer_image_floue();
        }
        if let Some(a) = &self.audio {
            a.vider();
        }
    }

    fn elements(&self, genre: GenreMenu) -> Vec<(String, Option<String>, Act)> {
        let ecran = if self.config.grille {
            "Grille LCD"
        } else {
            "Net"
        };
        let plein = self.ctx.input(|i| i.viewport().fullscreen).unwrap_or(false);
        let mut v = Vec::new();
        match genre {
            GenreMenu::Pause => {
                v.push(("Reprendre".into(), None, Act::Reprendre));
                v.push(("États".into(), Some("Sauver, charger".into()), Act::Etats));
                v.push(("Écran".into(), Some(ecran.into()), Act::Grille));
                v.push((
                    "Palette".into(),
                    Some(self.config.palette.nom().into()),
                    Act::Palette,
                ));
                v.push((
                    "Plein écran".into(),
                    Some(if plein { "Oui" } else { "Non" }.into()),
                    Act::PleinEcran,
                ));
                v.push(("Retour à la bibliothèque".into(), None, Act::Quitter));
            }
            GenreMenu::Reglages => {
                v.push((
                    "Dossier des jeux".into(),
                    Some(self.dossier_jeux().display().to_string()),
                    Act::DossierJeux,
                ));
                v.push((
                    "Ouvrir le dossier des jeux".into(),
                    None,
                    Act::OuvrirDossier,
                ));
                if !self.config.ajouts.is_empty() {
                    v.push((
                        "Oublier les éléments déposés".into(),
                        Some(self.config.ajouts.len().to_string()),
                        Act::OublierAjouts,
                    ));
                }
                v.push(("Analyser à nouveau".into(), None, Act::Reanalyser));
                v.push(("Écran".into(), Some(ecran.into()), Act::Grille));
                v.push((
                    "Palette".into(),
                    Some(self.config.palette.nom().into()),
                    Act::Palette,
                ));
                v.push(("Touches".into(), None, Act::Touches));
                let oui_non = |b: bool| Some(if b { "Oui" } else { "Non" }.to_string());
                v.push((
                    "Thème".into(),
                    Some(if self.config.jour { "Jour" } else { "Nuit" }.into()),
                    Act::Theme,
                ));
                v.push((
                    "Réduire les animations".into(),
                    oui_non(self.config.animations_reduites),
                    Act::Animations,
                ));
                v.push((
                    "Sons d'interface".into(),
                    oui_non(!self.config.sons_coupes),
                    Act::SonsUi,
                ));
                v.push((
                    "Écran de démarrage".into(),
                    oui_non(!self.config.sans_intro),
                    Act::Intro,
                ));
                v.push(("Fermer".into(), None, Act::Fermer));
            }
        }
        v
    }

    fn agir(&mut self, act: Act) {
        match act {
            Act::Reprendre | Act::Fermer => {
                self.menu = None;
                if let Some(p) = &mut self.partie {
                    p.reprendre();
                }
            }
            Act::Grille => {
                self.config.grille = !self.config.grille;
                self.config.enregistrer(&self.chemins.config());
            }
            Act::Palette => {
                self.config.palette = self.config.palette.suivante();
                self.config.enregistrer(&self.chemins.config());
            }
            Act::PleinEcran => self.basculer_plein_ecran(),
            Act::Quitter => self.quitter_partie(),
            Act::DossierJeux => {
                #[cfg(windows)]
                {
                    if let Some(d) = rfd::FileDialog::new()
                        .set_directory(self.dossier_jeux())
                        .pick_folder()
                    {
                        self.choisir_dossier(d);
                    }
                }
                #[cfg(not(windows))]
                {
                    let texte = self.dossier_jeux().display().to_string();
                    if let Some(m) = &mut self.menu {
                        m.edition = Some(texte);
                    }
                }
            }
            Act::OuvrirDossier => {
                let d = self.dossier_jeux();
                let _ = std::fs::create_dir_all(&d);
                #[cfg(windows)]
                let prog = "explorer";
                #[cfg(not(windows))]
                let prog = "xdg-open";
                let _ = std::process::Command::new(prog).arg(&d).spawn();
            }
            Act::OublierAjouts => {
                self.config.ajouts.clear();
                self.config.enregistrer(&self.chemins.config());
                self.lancer_analyse(false);
                if let Some(m) = &mut self.menu {
                    m.sel = 0;
                }
            }
            Act::Reanalyser => {
                self.lancer_analyse(false);
                self.dire("Analyse de la bibliothèque…");
            }
            Act::Etats => {
                self.menu = None;
                self.ouvrir_etats();
            }
            Act::Theme => {
                self.config.jour = !self.config.jour;
                d::appliquer_theme(&self.ctx, self.config.jour);
                self.config.enregistrer(&self.chemins.config());
            }
            Act::Animations => {
                self.config.animations_reduites = !self.config.animations_reduites;
                d::reduire_mouvement(self.config.animations_reduites);
                self.config.enregistrer(&self.chemins.config());
            }
            Act::SonsUi => {
                self.config.sons_coupes = !self.config.sons_coupes;
                self.config.enregistrer(&self.chemins.config());
            }
            Act::Intro => {
                self.config.sans_intro = !self.config.sans_intro;
                self.config.enregistrer(&self.chemins.config());
            }
            Act::Touches => {
                let genre = self.menu.take().map(|m| m.genre);
                self.ouvrir_touches(genre);
            }
        }
    }

    fn choisir_dossier(&mut self, d: PathBuf) {
        self.config.dossier_jeux = Some(d);
        self.config.enregistrer(&self.chemins.config());
        self.lancer_analyse(false);
    }

    pub(super) fn basculer_plein_ecran(&self) {
        let plein = self.ctx.input(|i| i.viewport().fullscreen).unwrap_or(false);
        self.ctx
            .send_viewport_cmd(ViewportCommand::Fullscreen(!plein));
    }

    pub(super) fn entrees_menu(&mut self, evts: &[Event], pad: Etat) {
        let Some(m) = &self.menu else { return };
        let genre = m.genre;
        let n = self.elements(genre).len();
        let mut valider = pad.appuis & (b::SUD | b::OUEST) != 0;
        let mut retour = pad.appuis & (b::EST | b::START) != 0;
        let mut dy: i32 = 0;
        let mut dx: i32 = 0;
        if pad.appuis & b::GAUCHE != 0 {
            dx -= 1;
        }
        if pad.appuis & b::DROITE != 0 {
            dx += 1;
        }
        let mut basculer = dx != 0;
        if pad.appuis & b::HAUT != 0 {
            dy -= 1;
        }
        if pad.appuis & b::BAS != 0 {
            dy += 1;
        }

        // Saisie d'un chemin : le texte va dans le champ.
        if self.menu.as_ref().is_some_and(|m| m.edition.is_some()) {
            let mut fini: Option<bool> = None;
            if let Some(texte) = self.menu.as_mut().and_then(|m| m.edition.as_mut()) {
                for e in evts {
                    match e {
                        Event::Text(t) | Event::Paste(t) => {
                            texte.extend(t.chars().filter(|c| !c.is_control()))
                        }
                        Event::Key {
                            key, pressed: true, ..
                        } => match key {
                            Key::Backspace => {
                                texte.pop();
                            }
                            Key::Enter => fini = Some(true),
                            Key::Escape => fini = Some(false),
                            _ => {}
                        },
                        _ => {}
                    }
                }
            }
            if let Some(ok) = fini {
                let texte = self.menu.as_mut().and_then(|m| m.edition.take());
                if let (true, Some(t)) = (ok, texte) {
                    let t = t.trim();
                    if !t.is_empty() {
                        self.choisir_dossier(PathBuf::from(t));
                    }
                }
            }
            return;
        }

        for e in evts {
            if let Event::Key {
                key, pressed: true, ..
            } = e
            {
                match key {
                    Key::ArrowUp => dy -= 1,
                    Key::ArrowDown => dy += 1,
                    Key::ArrowLeft => {
                        dx -= 1;
                        basculer = true;
                    }
                    Key::ArrowRight => {
                        dx += 1;
                        basculer = true;
                    }
                    Key::Enter | Key::Space => valider = true,
                    Key::Escape | Key::F2 => retour = true,
                    _ => {}
                }
            }
        }
        if retour {
            self.son(Son::Retour);
            self.agir(Act::Fermer);
            return;
        }
        let Some(m) = &mut self.menu else { return };
        let avant = m.sel;
        if n > 0 {
            if genre == GenreMenu::Pause {
                // Tuiles : la première sur toute la largeur, puis deux colonnes.
                m.sel = deplacer_tuile(m.sel, n, dx, dy);
                basculer = false;
            } else {
                m.sel = (m.sel as i32 + dy).rem_euclid(n as i32) as usize;
            }
        }
        let sel = m.sel.min(n.saturating_sub(1));
        if sel != avant {
            self.son(Son::Deplacement);
        }
        let act = self.elements(genre).get(sel).map(|e| e.2);
        if let Some(act) = act {
            let bascule = act.bascule();
            if valider || (basculer && bascule) {
                self.son(if valider {
                    Son::Valider
                } else {
                    Son::Deplacement
                });
                self.agir(act);
            }
        }
    }

    // ----- Dessin : menus -----

    pub(super) fn dessiner_menu(&mut self, ui: &mut egui::Ui) {
        let Some(m) = &self.menu else { return };
        if m.genre == GenreMenu::Pause {
            self.dessiner_pause(ui);
            return;
        }
        let jt = j();
        let genre = m.genre;
        let sel = m.sel;
        let ouvert_a = m.ouvert_a;
        let edition = m.edition.clone();
        let elements = self.elements(genre);
        let ecran = ui.max_rect();
        let p = ui.painter().clone();
        let age = (self.t_prec - ouvert_a) as f32;
        let t = d::sortie(age / d::duree_fondu(DUREE_MENU).max(0.001));
        let tm = d::ressort(age / d::duree(DUREE_MENU).max(0.001));
        p.rect_filled(
            ecran,
            CornerRadius::ZERO,
            alpha(
                Color32::from_rgb(10, 11, 18),
                if jt.jour { 0.45 } else { 0.72 } * t,
            ),
        );

        let l = 560.0f32.min(ecran.width() - 48.0);
        let n = elements.len().max(1) as f32;
        let h_el = ((ecran.height() - 48.0 - 116.0) / n).clamp(34.0, 48.0);
        let h = 96.0 + n * h_el + 20.0;
        let mut r = Rect::from_center_size(ecran.center(), Vec2::new(l, h));
        r = r.translate(Vec2::new(0.0, (1.0 - tm) * 14.0));
        let mut p = p;
        p.multiply_opacity(t);
        d::ombre(&p, r, 18.0, 18.0, 48.0, 1.0);
        panneau(&p, r, 18.0, jt.surface, Some(jt.trait_));

        let (titre, sous) = (
            "Réglages".to_string(),
            "Bibliothèque, affichage et confort".to_string(),
        );
        let g = ligne(&p, &titre, gras(22.0), jt.texte);
        p.galley(Pos2::new(r.left() + 28.0, r.top() + 24.0), g, jt.texte);
        let g = bloc(&p, &sous, regulier(14.0), jt.doux, l - 56.0, 1);
        p.galley(Pos2::new(r.left() + 28.0, r.top() + 56.0), g, jt.doux);

        // Une seule surbrillance : la souris qui bouge déplace la sélection.
        let bouge = ui.input(|i| i.pointer.is_moving());
        let mut y = r.top() + 96.0;
        let mut clic = None;
        let mut survol = None;
        for (k, (lib, val, act)) in elements.iter().enumerate() {
            let re = Rect::from_min_size(
                Pos2::new(r.left() + 12.0, y),
                Vec2::new(l - 24.0, h_el - 4.0),
            );
            let rep = ui.interact(re, Id::unique(("menu", k)), Sense::click());
            if rep.hovered() && bouge && k != sel {
                survol = Some(k);
            }
            let choisi = k == sel;
            if choisi {
                p.rect_filled(re, CornerRadius::same(11), jt.surface_haute);
                p.rect_filled(
                    Rect::from_min_size(
                        Pos2::new(re.left(), re.top() + 12.0),
                        Vec2::new(3.0, re.height() - 24.0),
                    ),
                    CornerRadius::same(2),
                    jt.accent,
                );
            }
            let g = ligne(
                &p,
                lib,
                moyen(16.0),
                if choisi { jt.texte } else { jt.doux },
            );
            let gl = g.size().x;
            p.galley(
                Pos2::new(re.left() + 18.0, re.center().y - g.size().y * 0.5),
                g,
                jt.texte,
            );
            let edite = choisi && *act == Act::DossierJeux && edition.is_some();
            let valeur = if edite { edition.clone() } else { val.clone() };
            if let Some(v) = valeur {
                let max = re.width() - gl - 54.0;
                let col = if edite { jt.texte } else { jt.doux };
                let g = bloc(&p, &v, regulier(14.5), col, max, 1);
                let gs = g.size();
                let x = re.right() - 18.0 - gs.x;
                if edite {
                    let champ = Rect::from_min_max(
                        Pos2::new(re.right() - 18.0 - max - 10.0, re.top() + 7.0),
                        Pos2::new(re.right() - 8.0, re.bottom() - 7.0),
                    );
                    p.rect_filled(champ, CornerRadius::same(8), jt.fond);
                    p.rect_stroke(
                        champ,
                        CornerRadius::same(8),
                        Stroke::new(1.0, alpha(jt.accent, 0.8)),
                        StrokeKind::Inside,
                    );
                    p.line_segment(
                        [
                            Pos2::new(re.right() - 16.0, re.center().y - 9.0),
                            Pos2::new(re.right() - 16.0, re.center().y + 9.0),
                        ],
                        Stroke::new(1.5, jt.accent),
                    );
                }
                p.galley(Pos2::new(x, re.center().y - gs.y * 0.5), g, col);
                if act.bascule() && choisi {
                    // Chevrons : la valeur se change avec gauche/droite.
                    let cx = x - 14.0;
                    let cyy = re.center().y;
                    p.line_segment(
                        [Pos2::new(cx + 3.0, cyy - 4.0), Pos2::new(cx - 1.0, cyy)],
                        Stroke::new(1.5, jt.doux),
                    );
                    p.line_segment(
                        [Pos2::new(cx - 1.0, cyy), Pos2::new(cx + 3.0, cyy + 4.0)],
                        Stroke::new(1.5, jt.doux),
                    );
                }
            }
            if rep.clicked() {
                clic = Some((k, *act));
            }
            y += h_el;
        }
        if let Some(m) = &mut self.menu {
            if let Some(k) = survol.or(clic.map(|c| c.0)) {
                m.sel = k;
            }
        }
        if let Some((_, act)) = clic {
            self.son(Son::Valider);
            self.agir(act);
        }
        if age < DUREE_MENU {
            ui.ctx().request_repaint();
        }
    }
}

impl Act {
    /// Réglage à deux valeurs ou plus, qui se change aussi avec gauche/droite.
    fn bascule(self) -> bool {
        matches!(
            self,
            Act::Grille
                | Act::Palette
                | Act::PleinEcran
                | Act::Theme
                | Act::Animations
                | Act::SonsUi
                | Act::Intro
        )
    }

    /// Icône et libellé court d'une tuile du menu pause.
    fn tuile(self) -> (Option<Icone>, Option<&'static str>) {
        match self {
            Act::Reprendre => (Some(Icone::Jouer), None),
            Act::Etats => (Some(Icone::Etats), None),
            Act::Grille => (Some(Icone::Ecran), None),
            Act::Palette => (Some(Icone::Palette), None),
            Act::PleinEcran => (Some(Icone::PleinEcran), None),
            Act::Quitter => (Some(Icone::Maison), Some("Bibliothèque")),
            _ => (None, None),
        }
    }
}

/// Déplacement dans les tuiles du menu pause : la tuile 0 occupe toute la première ligne,
/// les suivantes vont par deux. Haut/bas bouclent.
fn deplacer_tuile(sel: usize, n: usize, dx: i32, dy: i32) -> usize {
    if n == 0 {
        return 0;
    }
    let pos = |i: usize| {
        if i == 0 {
            (0, 0)
        } else {
            (((i - 1) / 2 + 1) as i32, ((i - 1) % 2) as i32)
        }
    };
    let (mut l, mut c) = pos(sel.min(n - 1));
    if l > 0 {
        c = (c + dx).clamp(0, 1);
    }
    let lignes = pos(n - 1).0;
    l += dy.signum();
    if l < 0 {
        l = lignes;
    } else if l > lignes {
        l = 0;
    }
    let i = if l == 0 {
        0
    } else {
        (1 + (l - 1) * 2 + c) as usize
    };
    i.min(n - 1)
}

impl App {
    /// Verre dépoli bon marché : la dernière image du jeu réduite (÷4), adoucie, puis
    /// agrandie avec lissage derrière le menu pause.
    pub(super) fn figer_image_floue(&mut self) {
        let Some(partie) = &self.partie else { return };
        const L: usize = gb_core::LARGEUR / 4;
        const H: usize = gb_core::HAUTEUR / 4;
        let teintes = self.config.palette.teintes();
        let mut acc = vec![[0.0f32; 3]; L * H];
        for (i, &v) in partie.gb.frame().iter().enumerate() {
            let (x, y) = (i % gb_core::LARGEUR, i / gb_core::LARGEUR);
            let Some(a) = acc.get_mut((y / 4) * L + x / 4) else {
                continue;
            };
            let c = teintes[(v & 3) as usize];
            for k in 0..3 {
                a[k] += c[k] as f32 / 16.0;
            }
        }
        for _ in 0..2 {
            let src = acc.clone();
            for y in 0..H {
                for x in 0..L {
                    let mut s = [0.0f32; 3];
                    for (dx, dy) in [
                        (-1, -1),
                        (0, -1),
                        (1, -1),
                        (-1, 0),
                        (0, 0),
                        (1, 0),
                        (-1, 1),
                        (0, 1),
                        (1, 1),
                    ] {
                        let xx = (x as i32 + dx).clamp(0, L as i32 - 1) as usize;
                        let yy = (y as i32 + dy).clamp(0, H as i32 - 1) as usize;
                        for k in 0..3 {
                            s[k] += src[yy * L + xx][k] / 9.0;
                        }
                    }
                    acc[y * L + x] = s;
                }
            }
        }
        let px = acc
            .iter()
            .map(|c| {
                // À moitié désaturé : le verre reste neutre quelle que soit la palette.
                let l = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
                let m = |x: f32| ((x + l) * 0.5) as u8;
                Color32::from_rgb(m(c[0]), m(c[1]), m(c[2]))
            })
            .collect();
        let img = ColorImage::new([L, H], px);
        match &mut self.hab.flou {
            Some(t) => t.set(img, TextureOptions::LINEAR),
            None => {
                self.hab.flou = Some(self.ctx.load_texture(
                    "pause-flou",
                    img,
                    TextureOptions::LINEAR,
                ))
            }
        }
    }

    /// Menu pause : le jeu figé derrière un verre dépoli sombre, grandes tuiles à gauche
    /// (la première, Reprendre, sur toute la largeur), image figée nette à droite.
    fn dessiner_pause(&mut self, ui: &mut egui::Ui) {
        let Some(m) = &self.menu else { return };
        let sel = m.sel;
        let ouvert_a = m.ouvert_a;
        let jt = j();
        let elements = self.elements(GenreMenu::Pause);
        let n = elements.len();
        let ecran = ui.max_rect();
        let ctx = ui.ctx().clone();
        let age = (self.t_prec - ouvert_a) as f32;
        let t = d::sortie(age / d::duree_fondu(0.16));
        let base = ui.painter().clone();

        // Verre dépoli : image floutée en « couvrir », assombrie à 55 %, bords vignettés.
        let mut p = base.clone();
        p.multiply_opacity(t);
        if let Some(tex) = &self.hab.flou {
            let ratio = gb_core::LARGEUR as f32 / gb_core::HAUTEUR as f32;
            let taille = if ecran.width() / ecran.height() > ratio {
                Vec2::new(ecran.width(), ecran.width() / ratio)
            } else {
                Vec2::new(ecran.height() * ratio, ecran.height())
            };
            p.image(
                tex.id(),
                Rect::from_center_size(ecran.center(), taille * 1.08),
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        p.rect_filled(ecran, CornerRadius::ZERO, Color32::from_black_alpha(155));
        d::vignette(&p, ecran);
        d::grain(&ctx, &p, ecran, 0.03);

        // Colonne de gauche : titre du jeu, tampon « PAUSE », temps de jeu, tuiles ; le bloc
        // est centré verticalement et l'image figée à droite s'aligne sur lui.
        let marge = (ecran.width() * 0.06).clamp(28.0, 72.0);
        let cw = (ecran.width() * 0.42)
            .clamp(300.0, 480.0)
            .min(ecran.width() - 2.0 * marge);
        let x0 = ecran.left() + marge;
        let titre = self
            .partie
            .as_ref()
            .map(|p| p.titre.clone())
            .unwrap_or_default();
        let gp = ligne(&p, "PAUSE", pixel(10.0), NUIT.accent_clair);
        let g = bloc(
            &p,
            &titre,
            gras(28.0),
            NUIT.texte,
            cw - gp.size().x - 30.0,
            1,
        );
        let temps = self
            .partie
            .as_ref()
            .and(self.hab.stats.en_cours())
            .map(|crc| crate::stats::duree_jeu(self.hab.stats.jeu(crc).temps))
            .unwrap_or_default();
        let sous = if temps.is_empty() {
            "Moins d'une minute de jeu".to_string()
        } else {
            format!("{temps} de jeu")
        };
        let gs = ligne(&p, &sous, regulier(14.5), NUIT.doux);

        let lignes = if n == 0 { 0 } else { 1 + (n - 1).div_ceil(2) };
        let ecart = 12.0;
        let h_entete = g.size().y + 6.0 + gs.size().y + 26.0;
        let dispo = ecran.height() - 2.0 * marge - 48.0 - h_entete;
        let h0 = 68.0;
        let ht = if lignes > 1 {
            ((dispo - h0 - ecart * (lignes - 1) as f32) / (lignes - 1) as f32).clamp(56.0, 100.0)
        } else {
            0.0
        };
        let h_bloc = h_entete + h0 + (lignes.max(1) - 1) as f32 * (ht + ecart);
        let y_titre = (ecran.center().y - 24.0 - h_bloc * 0.5).max(ecran.top() + marge * 0.6);
        p.galley(Pos2::new(x0, y_titre), g.clone(), NUIT.texte);
        // Tampon rétro à côté du titre, en Silkscreen.
        let tampon = Rect::from_min_size(
            Pos2::new(
                x0 + g.size().x + 14.0,
                y_titre + g.size().y * 0.5 - (gp.size().y + 8.0) * 0.5,
            ),
            gp.size() + Vec2::new(14.0, 8.0),
        );
        p.rect_stroke(
            tampon,
            CornerRadius::same(4),
            Stroke::new(1.0, alpha(NUIT.accent_clair, 0.7)),
            StrokeKind::Inside,
        );
        p.galley(tampon.min + Vec2::new(7.0, 4.0), gp, NUIT.accent_clair);
        p.galley(Pos2::new(x0, y_titre + g.size().y + 6.0), gs, NUIT.doux);

        let haut = y_titre + h_entete;
        let bas = haut + h_bloc - h_entete;
        let lt = (cw - ecart) * 0.5;
        let bouge = ui.input(|i| i.pointer.is_moving());
        let mut clic = None;
        let mut survol = None;
        for (k, (lib, val, act)) in elements.iter().enumerate() {
            let (l, c) = if k == 0 {
                (0, 0)
            } else {
                ((k - 1) / 2 + 1, (k - 1) % 2)
            };
            let (large, haute) = if k == 0 { (cw, h0) } else { (lt, ht) };
            let y = if l == 0 {
                haut
            } else {
                haut + h0 + ecart + (l - 1) as f32 * (ht + ecart)
            };
            let mut r = Rect::from_min_size(
                Pos2::new(x0 + c as f32 * (lt + ecart), y),
                Vec2::new(large, haute),
            );
            let rep = ui.interact(r, Id::unique(("tuile", k)), Sense::click());
            if rep.hovered() && bouge && k != sel {
                survol = Some(k);
            }
            if rep.clicked() {
                clic = Some((k, *act));
            }
            // Entrée en cascade, puis soulèvement de la tuile choisie.
            let tk = d::sortie((age - k as f32 * CASCADE) / d::duree_fondu(DUREE_MENU));
            let entree = d::ressort((age - k as f32 * CASCADE) / d::duree(DUREE_MENU).max(0.001));
            let choisi = k == sel;
            let lev = d::anim(&ctx, Id::unique(("tuile-sel", k)), choisi, DUREE_SELECTION);
            r = r.translate(Vec2::new(0.0, (1.0 - entree) * 12.0 - 3.0 * lev));
            let mut pt = base.clone();
            pt.multiply_opacity(tk);
            let lc = lev.clamp(0.0, 1.0);
            d::ombre(
                &pt,
                r.shrink(6.0),
                16.0,
                8.0 + 8.0 * lc,
                24.0,
                0.8 + 0.4 * lc,
            );
            if lc > 0.01 {
                pt.add(
                    egui::epaint::Shadow {
                        offset: [0, 0],
                        blur: 18,
                        spread: 1,
                        color: alpha(jt.accent_clair, 0.18 * lc),
                    }
                    .as_shape(r.expand(3.0), CornerRadius::same(19)),
                );
            }
            let fond = alpha(jt.surface, 0.94).lerp_to_gamma(jt.surface_haute, lc);
            panneau(&pt, r, 16.0, fond, Some(jt.trait_));
            if lc > 0.0 {
                pt.rect_stroke(
                    r.expand(3.0),
                    CornerRadius::same(19),
                    Stroke::new(2.0, alpha(jt.accent, 0.85 * lc)),
                    StrokeKind::Outside,
                );
            }
            let (ic, court) = act.tuile();
            let libelle = court.unwrap_or(lib.as_str());
            let val = val
                .clone()
                .or_else(|| (*act == Act::Quitter).then(|| "Quitter la partie".to_string()));
            let col_ic = if choisi { jt.accent } else { jt.doux };
            if k == 0 {
                // Tuile principale : icône dans une pastille d'accent, libellé à côté.
                let rp = r.height() * 0.3;
                let c = Pos2::new(r.left() + 18.0 + rp, r.center().y);
                pt.circle_filled(c, rp, if choisi { jt.accent } else { jt.surface_haute });
                if let Some(ic) = ic {
                    let coul = if choisi { jt.sur_accent } else { jt.texte };
                    icone(&pt, ic, c + Vec2::new(1.5, 0.0), rp * 1.1, coul);
                }
                let g = ligne(&pt, libelle, gras(19.0), jt.texte);
                pt.galley(
                    Pos2::new(c.x + rp + 16.0, r.center().y - g.size().y * 0.5),
                    g,
                    jt.texte,
                );
            } else {
                let compact = ht < 84.0;
                let t_ic = if compact { 24.0 } else { 32.0 };
                if let Some(ic) = ic {
                    let c = if compact {
                        Pos2::new(r.left() + 16.0 + t_ic * 0.5, r.center().y)
                    } else {
                        Pos2::new(r.left() + 16.0 + t_ic * 0.5, r.top() + 16.0 + t_ic * 0.5)
                    };
                    icone(&pt, ic, c, t_ic, col_ic);
                }
                let x_t = if compact {
                    r.left() + 24.0 + t_ic
                } else {
                    r.left() + 16.0
                };
                let largeur = r.right() - x_t - 12.0;
                let g = bloc(&pt, libelle, moyen(15.5), jt.texte, largeur, 1);
                let gv = val
                    .as_ref()
                    .map(|v| bloc(&pt, v, regulier(13.0), jt.doux, largeur, 1));
                let h_txt = g.size().y + gv.as_ref().map_or(0.0, |g| g.size().y + 2.0);
                let y_t = if compact {
                    r.center().y - h_txt * 0.5
                } else {
                    r.bottom() - 14.0 - h_txt
                };
                pt.galley(Pos2::new(x_t, y_t), g.clone(), jt.texte);
                if let Some(gv) = gv {
                    pt.galley(Pos2::new(x_t, y_t + g.size().y + 2.0), gv, jt.doux);
                }
            }
        }

        // À droite : l'image du jeu, nette et figée, qui luit doucement sous la lampe.
        let xd = x0 + cw + marge;
        let zone = Rect::from_min_max(
            Pos2::new(xd, haut - 10.0),
            Pos2::new(ecran.right() - marge, bas),
        );
        if zone.width() > 240.0 {
            if let Some(tex) = &self.tex_ecran {
                let ratio = gb_core::LARGEUR as f32 / gb_core::HAUTEUR as f32;
                let l = (zone.width() - 20.0)
                    .min((zone.height() - 20.0) * ratio)
                    .min(520.0);
                let r = Rect::from_center_size(zone.center(), Vec2::new(l, l / ratio));
                let clair = self.config.palette.teintes()[0];
                d::lueur(
                    &p,
                    r.center(),
                    r.size() * 0.9,
                    alpha(Color32::from_rgb(clair[0], clair[1], clair[2]), 0.18),
                );
                d::ombre(&p, r, 10.0, 14.0, 40.0, 1.2);
                p.rect_filled(
                    r.expand(10.0),
                    CornerRadius::same(14),
                    Color32::from_rgb(0x2A, 0x2B, 0x38),
                );
                p.add(
                    egui::epaint::RectShape::filled(r, CornerRadius::same(3), Color32::WHITE)
                        .with_texture(
                            tex.id(),
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        ),
                );
            }
        }

        // Pied : rappel des touches.
        let manette = self.manettes.as_ref().is_some() && self.dernier_pad.connectee;
        let puces: &[(&str, &str)] = if manette {
            &[("A", "Choisir"), ("B", "Reprendre"), ("Croix", "Naviguer")]
        } else {
            &[
                ("Entrée", "Choisir"),
                ("Échap", "Reprendre"),
                ("Flèches", "Naviguer"),
            ]
        };
        let mut xp = x0;
        for (tch, a) in puces {
            xp += puce(&p, Pos2::new(xp, ecran.bottom() - 30.0), tch, a) + 22.0;
        }

        if let Some(m) = &mut self.menu {
            if let Some(k) = survol.or(clic.map(|c| c.0)) {
                m.sel = k;
            }
        }
        if let Some((_, act)) = clic {
            self.son(Son::Valider);
            self.agir(act);
        }
        if age < DUREE_MENU + n as f32 * CASCADE {
            ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::deplacer_tuile;

    #[test]
    fn navigation_des_tuiles() {
        // 5 tuiles : [0] / [1 2] / [3 4]
        assert_eq!(deplacer_tuile(0, 5, 0, 1), 1);
        assert_eq!(deplacer_tuile(1, 5, 1, 0), 2);
        assert_eq!(deplacer_tuile(2, 5, 0, 1), 4);
        assert_eq!(deplacer_tuile(4, 5, 0, 1), 0);
        assert_eq!(deplacer_tuile(0, 5, 0, -1), 3);
        assert_eq!(deplacer_tuile(2, 5, 0, -1), 0);
        // Ligne incomplète : 4 tuiles, [3] seule en bas à gauche.
        assert_eq!(deplacer_tuile(2, 4, 0, 1), 3);
    }
}
