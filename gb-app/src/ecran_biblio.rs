//! Écran bibliothèque : entrées, grille de cartes, écran vide.

use super::*;

impl App {
    pub(super) fn entrees_biblio(&mut self, evts: &[Event], pad: Etat) {
        let mut dx: i32 = 0;
        let mut dy: i32 = 0;
        let mut lancer = pad.appuis & b::SUD != 0;
        let mut recherche_changee = false;
        for e in evts {
            match e {
                Event::Text(t) | Event::Paste(t) => {
                    let avant = self.recherche.len();
                    self.recherche.extend(t.chars().filter(|c| !c.is_control()));
                    recherche_changee |= self.recherche.len() != avant;
                }
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => match key {
                    Key::ArrowLeft => dx -= 1,
                    Key::ArrowRight => dx += 1,
                    Key::ArrowUp => dy -= 1,
                    Key::ArrowDown => dy += 1,
                    Key::PageUp => dy -= 3,
                    Key::PageDown => dy += 3,
                    Key::Enter => lancer = true,
                    Key::Backspace => {
                        recherche_changee |= self.recherche.pop().is_some();
                    }
                    Key::Escape => {
                        if !self.recherche.is_empty() {
                            self.recherche.clear();
                            recherche_changee = true;
                        }
                    }
                    Key::Tab => self.changer_filtre(if modifiers.shift { -1 } else { 1 }),
                    Key::F2 => self.ouvrir_menu(GenreMenu::Reglages),
                    Key::F5 => {
                        self.lancer_analyse(false);
                        self.dire("Analyse de la bibliothèque…");
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        if pad.appuis & b::EST != 0 && !self.recherche.is_empty() {
            self.recherche.clear();
            recherche_changee = true;
        }
        if pad.appuis & b::L1 != 0 {
            self.changer_filtre(-1);
        }
        if pad.appuis & b::R1 != 0 {
            self.changer_filtre(1);
        }
        if pad.appuis & b::START != 0 {
            self.ouvrir_menu(GenreMenu::Reglages);
        }
        if pad.appuis & b::GAUCHE != 0 {
            dx -= 1;
        }
        if pad.appuis & b::DROITE != 0 {
            dx += 1;
        }
        if pad.appuis & b::HAUT != 0 {
            dy -= 1;
        }
        if pad.appuis & b::BAS != 0 {
            dy += 1;
        }
        if recherche_changee {
            self.changer_recherche();
        }
        let n = self.vue.len();
        if n > 0 && (dx != 0 || dy != 0) {
            let c = self.colonnes.max(1) as i32;
            let mut s = self.sel as i32 + dx;
            for _ in 0..dy.abs() {
                let suiv = s + c * dy.signum();
                if suiv >= 0 && suiv < n as i32 {
                    s = suiv;
                } else if dy > 0 && (s / c) < (n as i32 - 1) / c {
                    // Ligne incomplète en bas : on va au dernier jeu.
                    s = n as i32 - 1;
                }
            }
            self.sel = s.clamp(0, n as i32 - 1) as usize;
            self.rendre_visible();
        }
        if lancer {
            self.lancer_selection();
        }
    }

    fn rendre_visible(&mut self) {
        let ligne = (self.sel / self.colonnes.max(1)) as f32;
        let y = ligne * self.ligne_h;
        let marge = 12.0;
        if y - marge < self.defil_cible {
            self.defil_cible = (y - marge).max(0.0);
        } else if y + self.ligne_h + marge > self.defil_cible + self.vue_h {
            self.defil_cible = y + self.ligne_h + marge - self.vue_h;
        }
    }

    // ----- Dessin : bibliothèque -----

    pub(super) fn dessiner_biblio(&mut self, ui: &mut egui::Ui, dt: f32) {
        let ecran = ui.max_rect();
        let p = ui.painter().clone();
        p.rect_filled(ecran, CornerRadius::ZERO, FOND);

        // En-tête : marque, recherche, filtres.
        let cy = ecran.top() + HAUT_ENTETE * 0.5 + 4.0;
        let marque = ligne(&p, "gbemu", gras(25.0), TEXTE);
        let lm = marque.size().x;
        p.galley(
            Pos2::new(ecran.left() + MARGE, cy - marque.size().y * 0.52),
            marque,
            TEXTE,
        );
        // Les quatre teintes de l'écran DMG, en signature.
        for (k, c) in Palette::Verte.teintes().iter().rev().enumerate() {
            let x = ecran.left() + MARGE + lm + 10.0 + k as f32 * 7.0;
            p.rect_filled(
                Rect::from_min_size(Pos2::new(x, cy + 2.0), Vec2::splat(5.0)),
                CornerRadius::same(1),
                Color32::from_rgb(c[0], c[1], c[2]),
            );
        }

        let x_rech = ecran.left() + MARGE + 150.0;
        let l_rech = (ecran.width() * 0.34).clamp(220.0, 440.0);
        let rech = Rect::from_min_size(Pos2::new(x_rech, cy - 20.0), Vec2::new(l_rech, 40.0));
        p.rect_filled(rech, CornerRadius::same(20), SURFACE);
        let actif = !self.recherche.is_empty();
        p.rect_stroke(
            rech,
            CornerRadius::same(20),
            Stroke::new(1.0, if actif { alpha(ACCENT, 0.7) } else { TRAIT }),
            StrokeKind::Inside,
        );
        // Loupe.
        let lc = Pos2::new(rech.left() + 21.0, rech.center().y - 1.0);
        p.circle_stroke(lc, 6.0, Stroke::new(1.8, DOUX));
        p.line_segment(
            [lc + Vec2::new(4.3, 4.3), lc + Vec2::new(8.5, 8.5)],
            Stroke::new(1.8, DOUX),
        );
        let xt = rech.left() + 38.0;
        if actif {
            let g = ligne(&p, &self.recherche, regulier(15.5), TEXTE);
            let w = g.size().x.min(rech.width() - 56.0);
            let pt = Pos2::new(xt, rech.center().y - g.size().y * 0.5);
            p.with_clip_rect(rech.shrink(4.0)).galley(pt, g, TEXTE);
            let t = ui.input(|i| i.time);
            if (t * 1.6).fract() < 0.6 {
                p.line_segment(
                    [
                        Pos2::new(xt + w + 2.0, rech.center().y - 9.0),
                        Pos2::new(xt + w + 2.0, rech.center().y + 9.0),
                    ],
                    Stroke::new(1.5, ACCENT),
                );
            }
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
        } else {
            let g = ligne(&p, "Tapez pour chercher un jeu", regulier(15.5), DISCRET);
            p.galley(
                Pos2::new(xt, rech.center().y - g.size().y * 0.5),
                g,
                DISCRET,
            );
        }

        // Filtres de plateforme (onglets) et compteur, alignés à droite.
        let compte = self.vue.len();
        let texte_compte = match compte {
            0 => "Aucun jeu".to_string(),
            1 => "1 jeu".to_string(),
            n => format!("{n} jeux"),
        };
        let gc = ligne(&p, &texte_compte, regulier(14.0), DOUX);
        let mut x = ecran.right() - MARGE - gc.size().x;
        let gcl = gc.size();
        p.galley(Pos2::new(x, cy - gcl.y * 0.5), gc, DOUX);
        if let Some((f, t)) = self.progres {
            let g = ligne(&p, &format!("Analyse {f} / {t}"), regulier(12.0), DISCRET);
            let gs = g.size();
            p.galley(
                Pos2::new(ecran.right() - MARGE - gs.x, cy + 12.0),
                g,
                DISCRET,
            );
        } else if !self.analyse_finie {
            let g = ligne(&p, "Analyse…", regulier(12.0), DISCRET);
            let gs = g.size();
            p.galley(
                Pos2::new(ecran.right() - MARGE - gs.x, cy + 12.0),
                g,
                DISCRET,
            );
        }
        x -= 28.0;
        let onglets = [
            (Some(Plateforme::Gba), "GBA"),
            (Some(Plateforme::Gbc), "GBC"),
            (Some(Plateforme::Gb), "GB"),
            (None, "Tous"),
        ];
        for (k, (f, nom)) in onglets.iter().enumerate() {
            let choisi = self.filtre == *f;
            let g = ligne(&p, nom, moyen(14.5), if choisi { TEXTE } else { DOUX });
            let pastille = if f.is_some() { 14.0 } else { 0.0 };
            let l = g.size().x + 26.0 + pastille;
            let r = Rect::from_min_size(Pos2::new(x - l, cy - 17.0), Vec2::new(l, 34.0));
            let rep = ui.interact(r, Id::unique(("onglet", k)), Sense::click());
            let t = ui
                .ctx()
                .animate_bool_with_time(Id::unique(("onglet-t", k)), choisi, 0.15);
            if t > 0.0 {
                p.rect_filled(r, CornerRadius::same(17), alpha(SURFACE_HAUTE, t));
            } else if rep.hovered() {
                p.rect_filled(r, CornerRadius::same(17), alpha(SURFACE, 0.8));
            }
            let mut xg = r.left() + 13.0;
            if let Some(pf) = f {
                p.circle_filled(Pos2::new(xg + 4.0, r.center().y), 4.0, couleur(*pf));
                xg += pastille;
            }
            let col = if choisi { TEXTE } else { DOUX };
            p.galley(Pos2::new(xg, r.center().y - g.size().y * 0.5), g, col);
            if rep.clicked() {
                self.filtre = *f;
                self.changer_recherche();
            }
            x -= l + 4.0;
        }

        // Grille.
        let grille = Rect::from_min_max(
            Pos2::new(ecran.left() + MARGE, ecran.top() + HAUT_ENTETE),
            Pos2::new(ecran.right() - MARGE, ecran.bottom() - HAUT_PIED),
        );
        let colonnes =
            (((grille.width() + ECART_X) / (LARGEUR_MIN_CARTE + ECART_X)).floor() as usize).max(1);
        let lc = (grille.width() - ECART_X * (colonnes - 1) as f32) / colonnes as f32;
        let hc = lc + TEXTE_CARTE;
        self.colonnes = colonnes;
        self.ligne_h = hc + ECART_Y;
        self.vue_h = grille.height();
        let lignes = self.vue.len().div_ceil(colonnes);
        let max_defil = (lignes as f32 * self.ligne_h - ECART_Y + 24.0 - grille.height()).max(0.0);

        let molette = ui.input(|i| i.smooth_scroll_delta.y);
        if molette != 0.0 && ui.rect_contains_pointer(grille) {
            self.defil_cible -= molette;
        }
        self.defil_cible = self.defil_cible.clamp(0.0, max_defil);
        let k = 1.0 - (-dt * 16.0).exp();
        self.defil += (self.defil_cible - self.defil) * k;
        if (self.defil_cible - self.defil).abs() < 0.3 {
            self.defil = self.defil_cible;
        } else {
            ui.ctx().request_repaint();
        }

        // Ombres de défilement : la grille glisse sous l'en-tête et le pied.
        let pg = p.with_clip_rect(grille.expand2(Vec2::new(12.0, 0.0)));
        let haut = grille.top() + 12.0;
        let premiere = ((self.defil - 12.0) / self.ligne_h).floor().max(0.0) as usize;
        let derniere = ((self.defil + grille.height()) / self.ligne_h).ceil() as usize + 1;
        let mut a_demander = Vec::new();
        for ligne_i in premiere..derniere.min(lignes) {
            for col in 0..colonnes {
                let k = ligne_i * colonnes + col;
                let Some(&fi) = self.vue.get(k) else { break };
                let r = Rect::from_min_size(
                    Pos2::new(
                        grille.left() + col as f32 * (lc + ECART_X),
                        haut + ligne_i as f32 * self.ligne_h - self.defil,
                    ),
                    Vec2::new(lc, hc),
                );
                let f = &self.fiches[fi];
                let rom = &self.roms[f.rom];
                let id = Id::unique(("carte", f.rom));
                let rep = ui.interact(r.intersect(grille), id, Sense::click());
                if rep.clicked() {
                    self.sel = k;
                }
                let double = rep.double_clicked();
                let choisi = k == self.sel;
                let t = ui.ctx().animate_bool_with_time(id, choisi, 0.14);
                let tex = f.dat.as_ref().and_then(|(_, nom)| self.textures.get(nom));
                if tex.is_none() {
                    if let Some((pl, nom)) = &f.dat {
                        if !self.demandees.contains(nom) {
                            a_demander.push((*pl, nom.clone()));
                        }
                    }
                }
                carte(&pg, r, lc, f, rom.plateforme, tex, t, rep.hovered());
                if double {
                    self.sel = k;
                    self.lancer_selection();
                }
            }
        }
        if let Some(j) = &self.jaquettes {
            for (pl, nom) in a_demander {
                j.demander(pl, &nom);
                self.demandees.insert(nom);
            }
        }
        // Fondus haut et bas de la zone de défilement.
        degrade(
            &p,
            Rect::from_min_size(
                grille.left_top() - Vec2::new(12.0, 0.0),
                Vec2::new(grille.width() + 24.0, 14.0),
            ),
            true,
        );
        degrade(
            &p,
            Rect::from_min_max(
                Pos2::new(grille.left() - 12.0, grille.bottom() - 18.0),
                Pos2::new(grille.right() + 12.0, grille.bottom()),
            ),
            false,
        );

        if self.vue.is_empty() {
            self.dessiner_vide(&p, grille);
        }

        // Pied : raccourcis.
        let yp = ecran.bottom() - HAUT_PIED * 0.5;
        p.line_segment(
            [
                Pos2::new(ecran.left() + MARGE, ecran.bottom() - HAUT_PIED),
                Pos2::new(ecran.right() - MARGE, ecran.bottom() - HAUT_PIED),
            ],
            Stroke::new(1.0, alpha(TRAIT, 0.6)),
        );
        let manette = self.manettes.as_ref().is_some() && self.dernier_pad.connectee;
        let puces: &[(&str, &str)] = if manette {
            &[
                ("A", "Jouer"),
                ("B", "Effacer"),
                ("LB RB", "Plateforme"),
                ("Start", "Réglages"),
            ]
        } else {
            &[
                ("Entrée", "Jouer"),
                ("Flèches", "Naviguer"),
                ("Tab", "Plateforme"),
                ("F2", "Réglages"),
                ("F11", "Plein écran"),
            ]
        };
        let mut xp = ecran.left() + MARGE;
        for (t, a) in puces {
            xp += puce(&p, Pos2::new(xp, yp), t, a) + 22.0;
        }
        let g = ligne(
            &p,
            "Glissez des jeux sur la fenêtre pour les ajouter",
            regulier(13.0),
            DISCRET,
        );
        let gs = g.size();
        if xp + gs.x + 20.0 < ecran.right() - MARGE {
            p.galley(
                Pos2::new(ecran.right() - MARGE - gs.x, yp - gs.y * 0.5),
                g,
                DISCRET,
            );
        }
    }

    fn dessiner_vide(&self, p: &egui::Painter, zone: Rect) {
        let (titre, texte) = if !self.analyse_finie && self.roms.is_empty() {
            ("Analyse de la bibliothèque…".to_string(), String::new())
        } else if !self.recherche.is_empty() || self.filtre.is_some() {
            (
                "Aucun jeu ne correspond".to_string(),
                "Échap efface la recherche, Tab change de plateforme.".to_string(),
            )
        } else {
            (
                "Votre bibliothèque est vide".to_string(),
                format!(
                    "Glissez des fichiers .gb, .gbc, .gba, .zip ou .7z sur cette fenêtre,\nou placez-les dans {}",
                    self.dossier_jeux().display()
                ),
            )
        };
        let c = zone.center() - Vec2::new(0.0, 40.0);
        cartouche(
            p,
            Rect::from_center_size(c - Vec2::new(0.0, 70.0), Vec2::splat(120.0)),
            Plateforme::Gb,
            "",
        );
        let g = ligne(p, &titre, moyen(21.0), TEXTE);
        let gs = g.size();
        p.galley(Pos2::new(c.x - gs.x * 0.5, c.y + 8.0), g, TEXTE);
        if !texte.is_empty() {
            let g = p.layout(texte, regulier(14.5), DOUX, zone.width().min(620.0));
            let gs = g.size();
            p.galley(Pos2::new(c.x - gs.x * 0.5, c.y + 44.0), g, DOUX);
        }
    }
}

/// Fondu vers la couleur de fond (haut ou bas d'une zone qui défile).
fn degrade(p: &egui::Painter, r: Rect, vers_bas: bool) {
    use egui::epaint::{Mesh, Vertex};
    let mut m = Mesh::default();
    let (c_haut, c_bas) = if vers_bas {
        (FOND, alpha(FOND, 0.0))
    } else {
        (alpha(FOND, 0.0), FOND)
    };
    let v = |pos: Pos2, color: Color32| Vertex {
        pos,
        uv: egui::epaint::WHITE_UV,
        color,
    };
    m.vertices.push(v(r.left_top(), c_haut));
    m.vertices.push(v(r.right_top(), c_haut));
    m.vertices.push(v(r.right_bottom(), c_bas));
    m.vertices.push(v(r.left_bottom(), c_bas));
    m.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    p.add(egui::Shape::mesh(m));
}

/// Une carte de la grille : jaquette ou cartouche dessinée, puis titre et détails.
#[allow(clippy::too_many_arguments)]
fn carte(
    p: &egui::Painter,
    r: Rect,
    cote: f32,
    f: &Fiche,
    pl: Plateforme,
    tex: Option<&TextureHandle>,
    t: f32,
    survol: bool,
) {
    let couverture = Rect::from_min_size(r.min, Vec2::splat(cote));
    let levee = couverture.translate(Vec2::new(0.0, -5.0 * t));
    let zone = Rect::from_center_size(levee.center(), levee.size() * (1.0 + 0.025 * t));
    let rayon = CornerRadius::same(12);
    if t > 0.0 {
        p.add(
            egui::epaint::Shadow {
                offset: [0, (14.0 * t) as i8],
                blur: 28,
                spread: 0,
                color: Color32::from_black_alpha((110.0 * t) as u8),
            }
            .as_shape(zone.shrink(4.0), rayon),
        );
    }
    p.rect_filled(
        zone,
        rayon,
        if survol && t == 0.0 {
            teinte(SURFACE, 1.12)
        } else {
            SURFACE
        },
    );
    match tex {
        Some(tex) => {
            let [tl, th] = tex.size();
            let ratio = tl as f32 / th.max(1) as f32;
            let interieur = zone.shrink(zone.width() * 0.06);
            let taille = if ratio >= 1.0 {
                Vec2::new(interieur.width(), interieur.width() / ratio)
            } else {
                Vec2::new(interieur.height() * ratio, interieur.height())
            };
            let ri = Rect::from_center_size(interieur.center(), taille);
            p.rect_filled(
                ri.translate(Vec2::new(0.0, 4.0)),
                CornerRadius::same(5),
                Color32::from_black_alpha(70),
            );
            p.add(
                egui::epaint::RectShape::filled(ri, CornerRadius::same(5), Color32::WHITE)
                    .with_texture(
                        tex.id(),
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    ),
            );
        }
        None => cartouche(p, zone, pl, &f.titre),
    }
    if !pl.jouable() {
        // Plateformes pas encore émulées : visibles mais signalées.
        let g = ligne(p, "Bientôt", moyen(11.5), DOUX);
        let gs = g.size();
        let b = Rect::from_min_size(
            zone.min + Vec2::new(10.0, 10.0),
            Vec2::new(gs.x + 16.0, 22.0),
        );
        p.rect_filled(b, CornerRadius::same(11), alpha(FOND, 0.85));
        p.galley(
            Pos2::new(b.left() + 8.0, b.center().y - gs.y * 0.5),
            g,
            DOUX,
        );
    }
    if t > 0.0 {
        p.rect_stroke(
            zone.expand(4.0),
            CornerRadius::same(15),
            Stroke::new(2.5, alpha(ACCENT, t)),
            StrokeKind::Outside,
        );
    }

    let col = if t > 0.5 { TEXTE } else { teinte(TEXTE, 0.88) };
    let g = bloc(p, &f.titre, moyen(15.0), col, cote - 2.0, 2);
    let hg = g.size().y;
    let y = couverture.bottom() + 12.0;
    p.galley(Pos2::new(r.left() + 1.0, y), g, col);
    let yd = y + hg + 4.0;
    p.circle_filled(Pos2::new(r.left() + 5.0, yd + 8.0), 3.5, couleur(pl));
    let g = bloc(p, &f.details, regulier(12.5), DOUX, cote - 16.0, 1);
    p.galley(Pos2::new(r.left() + 15.0, yd), g, DOUX);
}
