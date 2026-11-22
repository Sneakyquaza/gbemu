//! Écran de jeu : image de la console et entrées de jeu.

use super::*;

impl App {
    // ----- Dessin : jeu -----

    pub(super) fn dessiner_jeu(&mut self, ui: &mut egui::Ui, rapide: bool) {
        let ecran = ui.max_rect();
        let p = ui.painter().clone();
        p.rect_filled(ecran, CornerRadius::ZERO, FOND_JEU);
        let Some(partie) = &self.partie else { return };

        let teintes = self.config.palette.teintes();
        let mut px = Vec::with_capacity(gb_core::LARGEUR * gb_core::HAUTEUR);
        for &v in partie.gb.frame() {
            let c = teintes[(v & 3) as usize];
            px.push(Color32::from_rgb(c[0], c[1], c[2]));
        }
        let img = ColorImage::new([gb_core::LARGEUR, gb_core::HAUTEUR], px);
        match &mut self.tex_ecran {
            Some(t) => t.set(img, TextureOptions::NEAREST),
            None => {
                self.tex_ecran = Some(ui.ctx().load_texture("ecran", img, TextureOptions::NEAREST))
            }
        }

        // Échelle entière en pixels physiques : chaque pixel Game Boy reste net.
        let ppp = ui.ctx().pixels_per_point();
        let dispo = ecran.size() * ppp;
        let echelle = ((dispo.x / gb_core::LARGEUR as f32)
            .min(dispo.y / gb_core::HAUTEUR as f32)
            .floor())
        .max(1.0);
        let taille = Vec2::new(gb_core::LARGEUR as f32, gb_core::HAUTEUR as f32) * echelle / ppp;
        let coin = ((ecran.center() - taille * 0.5) * ppp).round() / ppp;
        let r = Rect::from_min_size(coin, taille);
        if let Some(t) = &self.tex_ecran {
            p.image(
                t.id(),
                r,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        if self.config.grille && echelle >= 3.0 {
            let pas = echelle / ppp;
            let c = teintes[0];
            let trait_lcd = Stroke::new(
                (echelle / 6.0).floor().max(1.0) / ppp,
                Color32::from_rgba_unmultiplied(c[0] / 2, c[1] / 2, c[2] / 2, 46),
            );
            for i in 1..gb_core::LARGEUR {
                let x = r.left() + i as f32 * pas;
                p.line_segment([Pos2::new(x, r.top()), Pos2::new(x, r.bottom())], trait_lcd);
            }
            for j in 1..gb_core::HAUTEUR {
                let y = r.top() + j as f32 * pas;
                p.line_segment([Pos2::new(r.left(), y), Pos2::new(r.right(), y)], trait_lcd);
            }
        }

        if rapide {
            let g = ligne(&p, &format!("Avance rapide ×{AVANCE}"), moyen(13.5), TEXTE);
            let gs = g.size();
            let b = Rect::from_min_size(
                Pos2::new(ecran.right() - 24.0 - gs.x - 24.0, ecran.top() + 20.0),
                Vec2::new(gs.x + 24.0, 30.0),
            );
            p.rect_filled(b, CornerRadius::same(15), alpha(SURFACE_HAUTE, 0.92));
            p.galley(
                Pos2::new(b.left() + 12.0, b.center().y - gs.y * 0.5),
                g,
                TEXTE,
            );
        }
        if partie.erreur_sav {
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
        // Rappel des touches au lancement, qui s'efface.
        let age = (self.t_prec - self.debut_partie) as f32;
        let a = (1.0 - (age - 3.5) / 0.8).clamp(0.0, 1.0);
        if a > 0.0 && self.menu.is_none() {
            let pa = p.clone().with_clip_rect(ecran);
            let y = ecran.bottom() - 30.0;
            let puces = [
                ("Échap", "Menu"),
                ("Tab", "Avance rapide"),
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
                x += puce(&pa, Pos2::new(x, y), t, ac) + 22.0;
            }
            ui.ctx().request_repaint();
        }
    }

    /// Entrées pendant la partie (pause, avance rapide, boutons) ; renvoie l'avance rapide.
    pub(super) fn entrees_jeu(&mut self, ui: &egui::Ui, pad: Etat, maintenant: f64) -> bool {
        let echap = ui.input(|i| i.key_pressed(Key::Escape));
        let combo = pad.tenus & (b::START | b::SELECT) == (b::START | b::SELECT)
            && pad.appuis & (b::START | b::SELECT) != 0;
        if echap || combo {
            self.ouvrir_menu(GenreMenu::Pause);
            false
        } else {
            let rapide = ui.input(|i| i.key_down(Key::Tab)) || pad.tenus & (b::R1 | b::R2) != 0;
            let boutons = ui.input(|i| {
                let k = |ks: &[Key]| ks.iter().any(|&k| i.key_down(k));
                let mut v = 0u8;
                use gb_core::bouton as gbb;
                if k(&[Key::ArrowRight]) || pad.tenus & b::DROITE != 0 {
                    v |= gbb::DROITE;
                }
                if k(&[Key::ArrowLeft]) || pad.tenus & b::GAUCHE != 0 {
                    v |= gbb::GAUCHE;
                }
                if k(&[Key::ArrowUp]) || pad.tenus & b::HAUT != 0 {
                    v |= gbb::HAUT;
                }
                if k(&[Key::ArrowDown]) || pad.tenus & b::BAS != 0 {
                    v |= gbb::BAS;
                }
                if k(&[Key::X, Key::K]) || pad.tenus & (b::EST | b::NORD) != 0 {
                    v |= gbb::A;
                }
                if k(&[Key::Z, Key::W, Key::J]) || pad.tenus & (b::SUD | b::OUEST) != 0 {
                    v |= gbb::B;
                }
                if k(&[Key::Enter]) || pad.tenus & b::START != 0 {
                    v |= gbb::START;
                }
                if k(&[Key::Backspace, Key::Space]) || pad.tenus & b::SELECT != 0 {
                    v |= gbb::SELECT;
                }
                // Directions opposées impossibles sur une vraie croix.
                if v & (gbb::GAUCHE | gbb::DROITE) == gbb::GAUCHE | gbb::DROITE {
                    v &= !(gbb::GAUCHE | gbb::DROITE);
                }
                if v & (gbb::HAUT | gbb::BAS) == gbb::HAUT | gbb::BAS {
                    v &= !(gbb::HAUT | gbb::BAS);
                }
                v
            });
            // La touche qui a lancé le jeu (Entrée, A) ne doit pas appuyer sur Start.
            if self.attendre_relache {
                self.attendre_relache = boutons != 0;
            }
            let boutons = if self.attendre_relache { 0 } else { boutons };
            if let Some(p) = &mut self.partie {
                p.gb.set_buttons(boutons);
                p.avancer(maintenant, rapide, self.audio.as_ref());
            }
            rapide
        }
    }
}
