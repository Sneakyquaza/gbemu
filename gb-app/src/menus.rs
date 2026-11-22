//! Menus (pause, réglages) : éléments, actions, entrées et dessin.

use super::*;

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
    Fermer,
}

pub(super) struct Menu {
    genre: GenreMenu,
    sel: usize,
    /// Saisie en cours du chemin du dossier des jeux.
    edition: Option<String>,
}

impl App {
    // ----- Menus -----

    pub(super) fn ouvrir_menu(&mut self, genre: GenreMenu) {
        self.menu = Some(Menu {
            genre,
            sel: 0,
            edition: None,
        });
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
                self.config.palette = match self.config.palette {
                    Palette::Verte => Palette::Grise,
                    Palette::Grise => Palette::Verte,
                };
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
        let mut basculer = pad.appuis & (b::GAUCHE | b::DROITE) != 0;
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
                    Key::ArrowLeft | Key::ArrowRight => basculer = true,
                    Key::Enter | Key::Space => valider = true,
                    Key::Escape | Key::F2 => retour = true,
                    _ => {}
                }
            }
        }
        if retour {
            self.agir(Act::Fermer);
            return;
        }
        let Some(m) = &mut self.menu else { return };
        if n > 0 {
            m.sel = (m.sel as i32 + dy).rem_euclid(n as i32) as usize;
        }
        let sel = m.sel.min(n.saturating_sub(1));
        let act = self.elements(genre).get(sel).map(|e| e.2);
        if let Some(act) = act {
            let bascule = matches!(act, Act::Grille | Act::Palette | Act::PleinEcran);
            if valider || (basculer && bascule) {
                self.agir(act);
            }
        }
    }

    // ----- Dessin : menus -----

    pub(super) fn dessiner_menu(&mut self, ui: &mut egui::Ui) {
        let Some(m) = &self.menu else { return };
        let genre = m.genre;
        let sel = m.sel;
        let edition = m.edition.clone();
        let elements = self.elements(genre);
        let ecran = ui.max_rect();
        let p = ui.painter().clone();
        let t = ui
            .ctx()
            .animate_bool_with_time(Id::unique("menu-ouvert"), true, 0.16);
        p.rect_filled(
            ecran,
            CornerRadius::ZERO,
            alpha(Color32::from_rgb(10, 11, 18), 0.72 * t),
        );

        let l = 560.0f32.min(ecran.width() - 48.0);
        let h_el = 48.0;
        let h = 96.0 + elements.len() as f32 * h_el + 20.0;
        let mut r = Rect::from_center_size(ecran.center(), Vec2::new(l, h));
        r = r.translate(Vec2::new(0.0, (1.0 - t) * 14.0));
        let mut p = p;
        p.multiply_opacity(t);
        p.add(
            egui::epaint::Shadow {
                offset: [0, 18],
                blur: 48,
                spread: 0,
                color: Color32::from_black_alpha(110),
            }
            .as_shape(r, CornerRadius::same(18)),
        );
        p.rect_filled(r, CornerRadius::same(18), SURFACE);
        p.rect_stroke(
            r,
            CornerRadius::same(18),
            Stroke::new(1.0, TRAIT),
            StrokeKind::Inside,
        );

        let (titre, sous) = match genre {
            GenreMenu::Pause => (
                "Pause".to_string(),
                self.partie
                    .as_ref()
                    .map(|p| p.titre.clone())
                    .unwrap_or_default(),
            ),
            GenreMenu::Reglages => (
                "Réglages".to_string(),
                "Bibliothèque et affichage".to_string(),
            ),
        };
        let g = ligne(&p, &titre, gras(22.0), TEXTE);
        p.galley(Pos2::new(r.left() + 28.0, r.top() + 24.0), g, TEXTE);
        let g = bloc(&p, &sous, regulier(14.0), DOUX, l - 56.0, 1);
        p.galley(Pos2::new(r.left() + 28.0, r.top() + 56.0), g, DOUX);

        let mut y = r.top() + 96.0;
        let mut clic = None;
        for (k, (lib, val, act)) in elements.iter().enumerate() {
            let re = Rect::from_min_size(
                Pos2::new(r.left() + 12.0, y),
                Vec2::new(l - 24.0, h_el - 4.0),
            );
            let rep = ui.interact(re, Id::unique(("menu", k)), Sense::click());
            let choisi = k == sel;
            if choisi {
                p.rect_filled(re, CornerRadius::same(11), SURFACE_HAUTE);
                p.rect_filled(
                    Rect::from_min_size(
                        Pos2::new(re.left(), re.top() + 12.0),
                        Vec2::new(3.0, re.height() - 24.0),
                    ),
                    CornerRadius::same(2),
                    ACCENT,
                );
            } else if rep.hovered() {
                p.rect_filled(re, CornerRadius::same(11), alpha(SURFACE_HAUTE, 0.5));
            }
            let g = ligne(&p, lib, moyen(16.0), if choisi { TEXTE } else { DOUX });
            let gl = g.size().x;
            p.galley(
                Pos2::new(re.left() + 18.0, re.center().y - g.size().y * 0.5),
                g,
                TEXTE,
            );
            let edite = choisi && *act == Act::DossierJeux && edition.is_some();
            let valeur = if edite { edition.clone() } else { val.clone() };
            if let Some(v) = valeur {
                let max = re.width() - gl - 54.0;
                let col = if edite { TEXTE } else { DOUX };
                let g = bloc(&p, &v, regulier(14.5), col, max, 1);
                let gs = g.size();
                let x = re.right() - 18.0 - gs.x;
                if edite {
                    let champ = Rect::from_min_max(
                        Pos2::new(re.right() - 18.0 - max - 10.0, re.top() + 7.0),
                        Pos2::new(re.right() - 8.0, re.bottom() - 7.0),
                    );
                    p.rect_filled(champ, CornerRadius::same(8), FOND);
                    p.rect_stroke(
                        champ,
                        CornerRadius::same(8),
                        Stroke::new(1.0, alpha(ACCENT, 0.8)),
                        StrokeKind::Inside,
                    );
                    p.line_segment(
                        [
                            Pos2::new(re.right() - 16.0, re.center().y - 9.0),
                            Pos2::new(re.right() - 16.0, re.center().y + 9.0),
                        ],
                        Stroke::new(1.5, ACCENT),
                    );
                }
                p.galley(Pos2::new(x, re.center().y - gs.y * 0.5), g, col);
                if matches!(act, Act::Grille | Act::Palette | Act::PleinEcran) && choisi {
                    // Chevrons : la valeur se change avec gauche/droite.
                    let cx = x - 14.0;
                    let cyy = re.center().y;
                    p.line_segment(
                        [Pos2::new(cx + 3.0, cyy - 4.0), Pos2::new(cx - 1.0, cyy)],
                        Stroke::new(1.5, DOUX),
                    );
                    p.line_segment(
                        [Pos2::new(cx - 1.0, cyy), Pos2::new(cx + 3.0, cyy + 4.0)],
                        Stroke::new(1.5, DOUX),
                    );
                }
            }
            if rep.clicked() {
                clic = Some((k, *act));
            }
            y += h_el;
        }
        if let Some((k, act)) = clic {
            if let Some(m) = &mut self.menu {
                m.sel = k;
            }
            self.agir(act);
        }
        if t < 1.0 {
            ui.ctx().request_repaint();
        }
    }
}
