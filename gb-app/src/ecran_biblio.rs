//! Écran bibliothèque : entrées, onglets (récents, favoris, plateformes), tri, grille de cartes
//! posée sous la lampe (halo teinté par la jaquette choisie), écran vide, écran de démarrage.

use super::*;
use crate::intro::{Etape, Intro};
use crate::sons_ui::{Son, SonsUi};
use crate::stats::{self, Stats};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Onglet {
    Recents,
    Favoris,
    Tous,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Tri {
    Nom,
    Recent,
    Temps,
    Annee,
    Editeur,
    Genre,
}

impl Tri {
    fn nom(self) -> &'static str {
        match self {
            Tri::Nom => "Nom",
            Tri::Recent => "Récents",
            Tri::Temps => "Temps de jeu",
            Tri::Annee => "Année",
            Tri::Editeur => "Éditeur",
            Tri::Genre => "Genre",
        }
    }

    fn suivant(self) -> Tri {
        match self {
            Tri::Nom => Tri::Recent,
            Tri::Recent => Tri::Temps,
            Tri::Temps => Tri::Annee,
            Tri::Annee => Tri::Editeur,
            Tri::Editeur => Tri::Genre,
            Tri::Genre => Tri::Nom,
        }
    }
}

/// Habillage de l'application : écran de démarrage, sons d'interface, statistiques,
/// onglet et tri de la bibliothèque, teintes des jaquettes, verre dépoli du menu pause.
pub(super) struct Habillage {
    intro: Option<Intro>,
    pub(super) sons: SonsUi,
    pub(super) stats: Stats,
    pub(super) onglet: Onglet,
    tri: Tri,
    /// Couleur moyenne de chaque jaquette chargée (halo derrière la sélection).
    pub(super) teintes: HashMap<String, Color32>,
    /// Dernière image du jeu, floutée (fond du menu pause).
    pub(super) flou: Option<TextureHandle>,
}

impl Habillage {
    pub(super) fn new(opts: &Options, chemins: &Chemins, config: &Config) -> Habillage {
        let intro = Intro::new();
        let montrer = !config.sans_intro
            && opts.rom.is_none()
            && (opts.capture.is_none() || intro.visible_en_capture());
        Habillage {
            intro: montrer.then_some(intro),
            sons: SonsUi::new(),
            stats: if opts.demo {
                Stats::demo()
            } else {
                Stats::charger(chemins.donnees.join("stats.json"))
            },
            onglet: Onglet::Tous,
            tri: Tri::Nom,
            teintes: HashMap::new(),
            flou: None,
        }
    }
}

/// Couleur moyenne d'une image RGBA, un peu ravivée (le gris moyen d'une jaquette est terne).
pub(super) fn teinte_moyenne(rgba: &[u8]) -> Color32 {
    let mut s = [0u64; 3];
    let mut n = 0u64;
    for px in rgba.as_chunks::<4>().0.iter().step_by(7) {
        if px[3] > 128 {
            for k in 0..3 {
                s[k] += px[k] as u64;
            }
            n += 1;
        }
    }
    if n == 0 {
        return j().accent;
    }
    let m = s.map(|v| v as f32 / n as f32);
    let gris = (m[0] + m[1] + m[2]) / 3.0;
    let c = m.map(|v| (gris + (v - gris) * 1.5).clamp(0.0, 255.0) as u8);
    Color32::from_rgb(c[0], c[1], c[2])
}

impl App {
    /// Son d'interface, sauf s'ils sont coupés dans les réglages.
    pub(super) fn son(&self, s: Son) {
        if !self.config.sons_coupes {
            self.hab.sons.jouer(s, self.audio.as_ref());
        }
    }

    /// Onglets dans l'ordre de l'en-tête : Récents, Favoris, Tous, GB, GBC, GBA.
    pub(super) fn changer_filtre(&mut self, sens: i32) {
        let ordre = [
            (Onglet::Recents, None),
            (Onglet::Favoris, None),
            (Onglet::Tous, None),
            (Onglet::Tous, Some(Plateforme::Gb)),
            (Onglet::Tous, Some(Plateforme::Gbc)),
            (Onglet::Tous, Some(Plateforme::Gba)),
        ];
        let i = ordre
            .iter()
            .position(|o| *o == (self.hab.onglet, self.filtre))
            .unwrap_or(2) as i32;
        (self.hab.onglet, self.filtre) = ordre[(i + sens).rem_euclid(6) as usize];
        self.changer_recherche();
        self.son(Son::Deplacement);
    }

    /// Après le filtre par plateforme et recherche : onglet Récents/Favoris, puis tri.
    pub(super) fn affiner_vue(&mut self) {
        let stats = &self.hab.stats;
        let jeu = |fi: usize| stats.jeu(self.roms[self.fiches[fi].rom].crc);
        match self.hab.onglet {
            Onglet::Recents => self.vue.retain(|&fi| jeu(fi).dernier > 0),
            Onglet::Favoris => self.vue.retain(|&fi| jeu(fi).favori),
            Onglet::Tous => {}
        }
        if let Some((champ, val)) = &self.filtre_meta {
            let fiches = &self.fiches;
            self.vue.retain(|&fi| champ.valeur(&fiches[fi]) == *val);
        }
        let tri = if self.hab.onglet == Onglet::Recents {
            Tri::Recent
        } else {
            self.hab.tri
        };
        match tri {
            Tri::Nom => {}
            Tri::Recent => self
                .vue
                .sort_by_key(|&fi| std::cmp::Reverse(jeu(fi).dernier)),
            Tri::Temps => self
                .vue
                .sort_by(|&a, &b| jeu(b).temps.total_cmp(&jeu(a).temps)),
            // Tris stables : à valeur égale, l'ordre par nom reste ; inconnus à la fin.
            Tri::Annee => {
                let f = &self.fiches;
                self.vue
                    .sort_by_key(|&fi| (f[fi].annee.is_none(), f[fi].annee));
            }
            Tri::Editeur | Tri::Genre => {
                let f = &self.fiches;
                self.vue.sort_by_cached_key(|&fi| {
                    let v = if tri == Tri::Editeur {
                        &f[fi].editeur
                    } else {
                        &f[fi].genre
                    };
                    (v.is_empty(), plier(v))
                });
            }
        }
    }

    pub(super) fn basculer_favori(&mut self) {
        let Some(&fi) = self.vue.get(self.sel) else {
            return;
        };
        let crc = self.roms[self.fiches[fi].rom].crc;
        let titre = self.fiches[fi].titre.clone();
        let oui = self.hab.stats.basculer_favori(crc);
        self.son(Son::Favori);
        self.dire(if oui {
            format!("{titre} ajouté aux favoris")
        } else {
            format!("{titre} retiré des favoris")
        });
        if self.hab.onglet == Onglet::Favoris {
            self.filtrer();
        }
    }

    pub(super) fn entrees_biblio(&mut self, evts: &[Event], pad: Etat) {
        let mut dx: i32 = 0;
        let mut dy: i32 = 0;
        let mut lancer = pad.appuis & b::SUD != 0;
        let mut recherche_changee = false;
        let mut aller = None;
        let page = self.lignes_page();
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
                    Key::PageUp => dy -= page,
                    Key::PageDown => dy += page,
                    Key::Home => aller = Some(0),
                    Key::End => aller = Some(usize::MAX),
                    Key::Enter => lancer = true,
                    Key::Backspace if modifiers.command => {
                        recherche_changee |= !self.recherche.is_empty();
                        self.recherche.clear();
                    }
                    Key::Backspace => {
                        recherche_changee |= self.recherche.pop().is_some();
                    }
                    Key::Escape => {
                        if !self.recherche.is_empty() {
                            self.recherche.clear();
                            recherche_changee = true;
                            self.son(Son::Retour);
                        } else if self.filtre_meta.is_some() {
                            self.retirer_filtre_meta();
                        } else {
                            self.recherche_active = false;
                        }
                    }
                    Key::F1 => self.ouvrir_fiche(),
                    Key::Tab => self.changer_filtre(if modifiers.shift { -1 } else { 1 }),
                    Key::F2 => self.ouvrir_menu(GenreMenu::Reglages),
                    Key::F3 => {
                        self.hab.tri = self.hab.tri.suivant();
                        self.filtrer();
                        self.son(Son::Deplacement);
                        self.dire(format!("Trier par : {}", self.hab.tri.nom()));
                    }
                    Key::F4 => self.basculer_favori(),
                    Key::F5 => {
                        self.lancer_analyse(false);
                        self.dire("Analyse de la bibliothèque…");
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        if pad.appuis & b::EST != 0 {
            if !self.recherche.is_empty() {
                self.recherche.clear();
                recherche_changee = true;
                self.son(Son::Retour);
            } else if self.filtre_meta.is_some() {
                self.retirer_filtre_meta();
            }
        }
        if pad.appuis & b::NORD != 0 {
            self.ouvrir_fiche();
        }
        if pad.appuis & b::OUEST != 0 {
            self.basculer_favori();
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
        if pad.appuis & b::L2 != 0 {
            dy -= page;
        }
        if pad.appuis & b::R2 != 0 {
            dy += page;
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
        if n > 0 && (dx != 0 || dy != 0 || aller.is_some()) {
            let c = self.colonnes.max(1);
            let avant = self.sel;
            let visible = self.vue_h <= 0.0
                || ligne_visible(self.sel / c, self.ligne_h, self.defil_cible, self.vue_h);
            self.sel = match aller {
                Some(k) => k.min(n - 1),
                // Sélection sortie de l'écran (molette, barre) : on reprend sur la 1re carte
                // visible plutôt que de remonter tout en haut.
                None if !visible => {
                    ((self.defil_cible / self.ligne_h).ceil() as usize * c).min(n - 1)
                }
                None => deplacer(self.sel, n, c, dx, dy),
            };
            if self.sel != avant {
                self.son(Son::Deplacement);
            }
            self.rendre_visible();
        }
        if lancer {
            self.lancer_avec_son();
        }
    }

    pub(super) fn lancer_avec_son(&mut self) {
        let choisi = self.vue.get(self.sel).is_some();
        self.son(if choisi { Son::Valider } else { Son::Erreur });
        self.lancer_selection();
    }

    /// Lignes entières visibles dans la grille (saut de page).
    fn lignes_page(&self) -> i32 {
        ((self.vue_h / self.ligne_h).floor() as i32).max(1)
    }

    pub(super) fn rendre_visible(&mut self) {
        let ligne = (self.sel / self.colonnes.max(1)) as f32;
        let y = ligne * self.ligne_h;
        let marge = 12.0;
        if y - marge < self.defil_cible {
            self.defil_cible = (y - marge).max(0.0);
        } else if y + self.ligne_h + marge > self.defil_cible + self.vue_h {
            self.defil_cible = y + self.ligne_h + marge - self.vue_h;
        }
    }

    /// Écran de démarrage, par-dessus la bibliothèque (une seule fois par session).
    pub(super) fn dessiner_intro(&mut self, ui: &egui::Ui, evts: &[Event], pad: Etat) {
        let Some(intro) = &mut self.hab.intro else {
            return;
        };
        if self.partie.is_some() {
            self.hab.intro = None;
            return;
        }
        let appui = pad.appuis != 0
            || evts.iter().any(|e| {
                matches!(
                    e,
                    Event::Key { pressed: true, .. } | Event::PointerButton { pressed: true, .. }
                )
            });
        match intro.dessiner(ui, appui) {
            Etape::Finie => self.hab.intro = None,
            Etape::Ding => self.son(Son::Ding),
            Etape::EnCours => {}
        }
    }

    // ----- Dessin : bibliothèque -----

    pub(super) fn dessiner_biblio(&mut self, ui: &mut egui::Ui, dt: f32) {
        let jt = j();
        let ecran = ui.max_rect();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();

        // Le bureau, la nuit : fond profond, flaque de lumière chaude de la lampe en haut à
        // gauche, bords qui s'assombrissent, grain qui casse l'aplat.
        p.rect_filled(ecran, CornerRadius::ZERO, jt.fond);
        d::lueur(
            &p,
            Pos2::new(
                ecran.left() + ecran.width() * 0.22,
                ecran.top() - ecran.height() * 0.08,
            ),
            Vec2::new(ecran.width() * 0.75, ecran.height() * 0.95),
            jt.lampe,
        );
        d::vignette(&p, ecran);
        d::grain(&ctx, &p, ecran, if jt.jour { 0.05 } else { 0.035 });

        // En-tête : marque, recherche, onglets.
        let cy = ecran.top() + HAUT_ENTETE * 0.5 + 4.0;
        let marque = ligne(&p, "gbemu", gras(25.0), jt.texte);
        let lm = marque.size().x;
        p.galley(
            Pos2::new(ecran.left() + MARGE, cy - marque.size().y * 0.52),
            marque,
            jt.texte,
        );
        // Signature : nuancier des quatre teintes de l'écran DMG, collées comme sur l'écran.
        let nuancier = Rect::from_min_size(
            Pos2::new(ecran.left() + MARGE + lm + 10.0, cy - 1.0),
            Vec2::new(28.0, 7.0),
        );
        for (k, c) in Palette::Verte.teintes().iter().enumerate() {
            p.rect_filled(
                Rect::from_min_size(
                    nuancier.min + Vec2::new(k as f32 * 7.0, 0.0),
                    Vec2::splat(7.0),
                ),
                CornerRadius::ZERO,
                Color32::from_rgb(c[0], c[1], c[2]),
            );
        }
        p.rect_stroke(
            nuancier,
            CornerRadius::same(2),
            Stroke::new(1.5, jt.trait_),
            StrokeKind::Outside,
        );

        // Compteur et onglets, alignés à droite (mesurés d'abord : la recherche prend le reste).
        let compte = self.vue.len();
        let texte_compte = match compte {
            0 => "Aucun jeu".to_string(),
            1 => "1 jeu".to_string(),
            n => format!("{n} jeux"),
        };
        let gc = ligne(&p, &texte_compte, regulier(14.0), jt.doux);
        let gcl = gc.size();
        let mut x = ecran.right() - MARGE - gcl.x;
        p.galley(Pos2::new(x, cy - gcl.y * 0.5), gc, jt.doux);
        let etat = if let Some((f, t)) = self.progres {
            Some(format!("Analyse {f} / {t}"))
        } else if !self.analyse_finie {
            Some("Analyse…".to_string())
        } else {
            None
        };
        if let Some(e) = etat {
            let g = ligne(&p, &e, regulier(12.0), jt.discret);
            let gs = g.size();
            p.galley(
                Pos2::new(ecran.right() - MARGE - gs.x, cy + 12.0),
                g,
                jt.discret,
            );
        }
        x -= 24.0;
        let onglets: [(Onglet, Option<Plateforme>, &str); 6] = [
            (Onglet::Tous, Some(Plateforme::Gba), "GBA"),
            (Onglet::Tous, Some(Plateforme::Gbc), "GBC"),
            (Onglet::Tous, Some(Plateforme::Gb), "GB"),
            (Onglet::Tous, None, "Tous"),
            (Onglet::Favoris, None, "Favoris"),
            (Onglet::Recents, None, "Récents"),
        ];
        let mut clic_onglet = None;
        // Fenêtre étroite : Récents et Favoris en icônes seules, la recherche garde sa place.
        let compact = ecran.width() < 1100.0;
        for (k, (o, f, nom)) in onglets.iter().enumerate() {
            if k == 4 {
                // Séparateur entre la vie de la bibliothèque et les plateformes.
                x -= 6.0;
                p.line_segment(
                    [Pos2::new(x, cy - 9.0), Pos2::new(x, cy + 9.0)],
                    Stroke::new(1.0, jt.trait_),
                );
                x -= 10.0;
            }
            let choisi = self.hab.onglet == *o && self.filtre == *f;
            let col = if choisi { jt.texte } else { jt.doux };
            let icone_seule = compact && *o != Onglet::Tous;
            let g = ligne(&p, if icone_seule { "" } else { nom }, moyen(14.5), col);
            let pastille = if f.is_some() || *o != Onglet::Tous {
                15.0
            } else {
                0.0
            };
            let l = if icone_seule {
                36.0
            } else {
                g.size().x + 26.0 + pastille
            };
            let r = Rect::from_min_size(Pos2::new(x - l, cy - 17.0), Vec2::new(l, 34.0));
            let mut rep = ui.interact(r, Id::unique(("onglet", k)), Sense::click());
            if icone_seule {
                rep = rep.on_hover_text(*nom);
            }
            let t = ctx.animate_bool_with_time(
                Id::unique(("onglet-t", k)),
                choisi,
                d::duree_fondu(0.15),
            );
            if t > 0.0 {
                p.rect_filled(r, CornerRadius::same(17), alpha(jt.surface_haute, t));
            } else if rep.hovered() {
                p.rect_filled(r, CornerRadius::same(17), alpha(jt.surface, 0.8));
            }
            let mut xg = r.left() + if icone_seule { 13.5 } else { 13.0 };
            if let Some(pf) = f {
                p.circle_filled(Pos2::new(xg + 4.0, r.center().y), 4.0, couleur(*pf));
                xg += pastille;
            } else if *o == Onglet::Favoris {
                let ic = if choisi {
                    Icone::Etoile
                } else {
                    Icone::EtoileVide
                };
                icone(&p, ic, Pos2::new(xg + 4.5, r.center().y), 12.0, jt.alerte);
                xg += pastille;
            } else if *o == Onglet::Recents {
                icone(
                    &p,
                    Icone::Horloge,
                    Pos2::new(xg + 4.5, r.center().y),
                    13.0,
                    col,
                );
                xg += pastille;
            }
            p.galley(Pos2::new(xg, r.center().y - g.size().y * 0.5), g, col);
            if rep.clicked() {
                clic_onglet = Some((*o, *f));
            }
            x -= l + 4.0;
        }
        if let Some(o) = clic_onglet {
            (self.hab.onglet, self.filtre) = o;
            self.changer_recherche();
            self.son(Son::Deplacement);
        }

        let x_rech = ecran.left() + MARGE + 150.0;
        let l_rech = (x - 16.0 - x_rech).min(440.0);
        if l_rech > 120.0 {
            self.dessiner_recherche(
                ui,
                &p,
                Rect::from_min_size(Pos2::new(x_rech, cy - 20.0), Vec2::new(l_rech, 40.0)),
            );
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
        let contenu_h = lignes as f32 * self.ligne_h - ECART_Y + 24.0;
        let max_defil = (contenu_h - grille.height()).max(0.0);

        // Barre de défilement dans la marge droite : poignée glissable, clic dans la piste =
        // une page vers le pointeur.
        let piste = Rect::from_min_max(
            Pos2::new(grille.right() + 8.0, grille.top() + 12.0),
            Pos2::new(ecran.right() - 6.0, grille.bottom() - 12.0),
        );
        let barre = (max_defil > 0.0)
            .then(|| ui.interact(piste, Id::unique("barre"), Sense::click_and_drag()));
        if let Some(rep) = &barre {
            let (py, ph) = poignee(piste.height(), grille.height(), contenu_h, self.defil);
            if let Some(pos) = rep.interact_pointer_pos() {
                let y = pos.y - piste.top();
                if rep.drag_started() {
                    // Prise sur la poignée : elle garde son écart au pointeur ; ailleurs,
                    // elle se centre dessus.
                    let dans = y >= py && y <= py + ph;
                    self.prise_barre = Some(if dans { y - py } else { ph * 0.5 });
                }
                if let Some(prise) = self.prise_barre.filter(|_| rep.dragged()) {
                    self.defil_cible =
                        defil_de_poignee(y - prise, piste.height(), ph, contenu_h, grille.height());
                    self.defil = self.defil_cible;
                } else if rep.clicked() {
                    let page = self.lignes_page() as f32 * self.ligne_h;
                    if y < py {
                        self.defil_cible -= page;
                    } else if y > py + ph {
                        self.defil_cible += page;
                    }
                }
            }
        }
        if !barre.as_ref().is_some_and(|r| r.dragged()) {
            self.prise_barre = None;
        }

        let molette = ui.input(|i| i.smooth_scroll_delta.y);
        if molette != 0.0 && ui.rect_contains_pointer(grille.union(piste)) {
            self.defil_cible -= molette;
        }
        self.defil_cible = self.defil_cible.clamp(0.0, max_defil);
        let k = if d::mouvement_reduit() {
            1.0
        } else {
            1.0 - (-dt * 16.0).exp()
        };
        self.defil += (self.defil_cible - self.defil) * k;
        if (self.defil_cible - self.defil).abs() < 0.3 {
            self.defil = self.defil_cible;
        } else {
            ctx.request_repaint();
        }

        let haut = grille.top() + 12.0;
        let case = |k: usize| {
            Rect::from_min_size(
                Pos2::new(
                    grille.left() + (k % colonnes) as f32 * (lc + ECART_X),
                    haut + (k / colonnes) as f32 * self.ligne_h - self.defil,
                ),
                Vec2::new(lc, hc),
            )
        };

        // Halo derrière la sélection, teinté par sa jaquette (ou sa plateforme) ; il glisse
        // d'une carte à l'autre.
        if let Some(&fi) = self.vue.get(self.sel) {
            let f = &self.fiches[fi];
            let teinte_sel = f
                .dat
                .as_ref()
                .and_then(|(_, nom)| self.hab.teintes.get(nom).copied())
                .unwrap_or_else(|| couleur(self.roms[f.rom].plateforme));
            let c = case(self.sel).min + Vec2::splat(lc * 0.5);
            let glisse = d::duree(DUREE_SELECTION * 2.0);
            let cx = ctx.animate_value_with_time(Id::unique("halo-x"), c.x, glisse);
            let cy = ctx.animate_value_with_time(Id::unique("halo-y"), c.y, glisse);
            let force = if jt.jour { 0.30 } else { 0.20 };
            d::lueur(
                &p.with_clip_rect(grille.expand(MARGE)),
                Pos2::new(cx, cy),
                Vec2::splat(lc * 1.35),
                alpha(teinte_sel, force),
            );
        }

        // Défilement rapide (barre tenue, grand saut) : indicateur de lettre, et pas de
        // jaquette demandée pour les cartes qui ne font que passer.
        let rapide = self.prise_barre.is_some()
            || (self.defil_cible - self.defil).abs() > self.ligne_h * 1.5;
        let pg = p.with_clip_rect(grille.expand2(Vec2::new(MARGE - 6.0, 0.0)));
        let premiere = ((self.defil - 12.0) / self.ligne_h).floor().max(0.0) as usize;
        let derniere = ((self.defil + grille.height()) / self.ligne_h).ceil() as usize + 1;
        let mut a_demander = Vec::new();
        let mut vues = Vec::new();
        let mut clic = None;
        let mut double = false;
        let mut fiche = false;
        for ligne_i in premiere..derniere.min(lignes) {
            for col in 0..colonnes {
                let k = ligne_i * colonnes + col;
                let Some(&fi) = self.vue.get(k) else { break };
                let r = case(k);
                let f = &self.fiches[fi];
                let rom = &self.roms[f.rom];
                let id = Id::unique(("carte", f.rom));
                let rep = ui.interact(r.intersect(grille), id, Sense::click());
                if rep.clicked() {
                    clic = Some(k);
                }
                if rep.double_clicked() {
                    clic = Some(k);
                    double = true;
                }
                if rep.secondary_clicked() {
                    clic = Some(k);
                    fiche = true;
                }
                let choisi = k == self.sel;
                let t = d::anim(&ctx, id, choisi, DUREE_LEVEE);
                let tex = f.dat.as_ref().and_then(|(_, nom)| self.textures.get(nom));
                if let Some((pl, nom)) = &f.dat {
                    if tex.is_some() {
                        vues.push(nom.clone());
                    } else if !rapide && !self.demandees.contains(nom) {
                        a_demander.push((*pl, nom.clone()));
                    }
                }
                let tex = tex.map(|(t, _)| t);
                // Les cartes s'effacent en glissant sous l'en-tête et le pied.
                let bord = 28.0;
                let a = ((r.top() + lc * 0.5 - grille.top()) / bord)
                    .min((grille.bottom() - r.top() - lc * 0.35) / bord)
                    .clamp(0.0, 1.0);
                let mut pc = pg.clone();
                pc.multiply_opacity(d::sortie(a));
                let jeu = self.hab.stats.jeu(rom.crc);
                let recent = self.hab.onglet == Onglet::Recents;
                carte(
                    &pc,
                    r,
                    lc,
                    f,
                    rom.plateforme,
                    tex,
                    t,
                    rep.hovered(),
                    jeu,
                    recent,
                );
            }
        }
        if let Some(k) = clic {
            if k != self.sel {
                self.son(Son::Deplacement);
            }
            self.sel = k;
            if double {
                self.lancer_avec_son();
            } else if fiche {
                self.ouvrir_fiche();
            }
        }
        if let Some(j) = &self.jaquettes {
            for (pl, nom) in a_demander {
                self.demandees.insert(nom.clone());
                for abandon in j.demander(pl, &nom, Vignette::Boite) {
                    self.demandees.remove(&abandon);
                }
            }
        }
        // Mémoire bornée même après avoir parcouru 5 000 jeux : les jaquettes non vues depuis
        // le plus longtemps sont libérées (relues depuis le cache disque si elles reviennent).
        for nom in vues {
            if let Some(e) = self.textures.get_mut(&nom) {
                e.1 = self.images_ui;
            }
        }
        if self.textures.len() > TEXTURES_MAX {
            let mut ages: Vec<(u64, String)> = self
                .textures
                .iter()
                .map(|(n, (_, vu))| (*vu, n.clone()))
                .collect();
            ages.sort_unstable();
            let trop = self.textures.len() - TEXTURES_MAX * 3 / 4;
            for (_, n) in ages.into_iter().take(trop) {
                self.textures.remove(&n);
                self.demandees.remove(&n);
            }
        }

        // La dernière rangée s'efface dans le bas plutôt que d'être tranchée par le pied.
        fondu_bas(
            &p,
            Rect::from_min_max(
                Pos2::new(ecran.left(), grille.bottom() - 64.0),
                Pos2::new(ecran.right(), grille.bottom()),
            ),
        );

        if self.vue.is_empty() {
            self.dessiner_vide(&p, grille);
        }

        if let Some(rep) = &barre {
            self.dessiner_defilement(&p, rep, piste, grille.height(), contenu_h, rapide);
        }

        // Pied : raccourcis.
        let yp = ecran.bottom() - HAUT_PIED * 0.5;
        p.line_segment(
            [
                Pos2::new(ecran.left() + MARGE, ecran.bottom() - HAUT_PIED),
                Pos2::new(ecran.right() - MARGE, ecran.bottom() - HAUT_PIED),
            ],
            Stroke::new(1.0, jt.trait_),
        );
        let manette = self.manettes.as_ref().is_some() && self.dernier_pad.connectee;
        let trier = format!("Trier : {}", self.hab.tri.nom());
        let puces: Vec<(&str, &str)> = if manette {
            vec![
                ("A", "Jouer"),
                ("Y", "Fiche"),
                ("X", "Favori"),
                ("B", "Effacer"),
                ("LB RB", "Onglet"),
                ("Start", "Réglages"),
            ]
        } else {
            vec![
                ("Entrée", "Jouer"),
                ("F1", "Fiche"),
                ("Tab", "Onglet"),
                ("F3", &trier),
                ("F4", "Favori"),
                ("F2", "Réglages"),
                ("F11", "Plein écran"),
            ]
        };
        let mut xp = ecran.left() + MARGE;
        for (t, a) in puces {
            xp += puce(&p, Pos2::new(xp, yp), t, a) + 22.0;
        }
        if self.filtre_meta.is_some() {
            self.dessiner_filtre_meta(ui, &p, ecran.right() - MARGE, yp);
            return;
        }
        let g = ligne(
            &p,
            "Glissez des jeux sur la fenêtre pour les ajouter",
            regulier(13.0),
            jt.discret,
        );
        let gs = g.size();
        if xp + gs.x + 20.0 < ecran.right() - MARGE {
            p.galley(
                Pos2::new(ecran.right() - MARGE - gs.x, yp - gs.y * 0.5),
                g,
                jt.discret,
            );
        }
    }

    /// Barre de défilement (piste discrète, poignée qui s'épaissit au survol) et, pendant un
    /// défilement rapide trié par nom, bulle de la lettre des cartes affichées.
    fn dessiner_defilement(
        &self,
        p: &egui::Painter,
        rep: &egui::Response,
        piste: Rect,
        vue_h: f32,
        contenu_h: f32,
        rapide: bool,
    ) {
        let jt = j();
        let ctx = &rep.ctx;
        let tenue = rep.dragged();
        let t = ctx.animate_bool_with_time(
            Id::unique("barre-t"),
            rep.hovered() || tenue,
            d::duree_fondu(0.12),
        );
        let l = 6.0 + 4.0 * t;
        let x = piste.center().x;
        let fond =
            Rect::from_center_size(Pos2::new(x, piste.center().y), Vec2::new(l, piste.height()));
        p.rect_filled(fond, CornerRadius::same(5), alpha(jt.trait_, 0.5 + 0.5 * t));
        let (py, ph) = poignee(piste.height(), vue_h, contenu_h, self.defil);
        let poig = Rect::from_min_size(Pos2::new(x - l * 0.5, piste.top() + py), Vec2::new(l, ph));
        let c = if tenue {
            jt.accent
        } else {
            jt.discret.lerp_to_gamma(jt.texte, 0.35 + 0.45 * t)
        };
        p.rect_filled(poig, CornerRadius::same(5), c);

        let tl = ctx.animate_bool_with_time(Id::unique("lettre-t"), rapide, d::duree_fondu(0.25));
        let par_nom = self.hab.tri == Tri::Nom && self.hab.onglet != Onglet::Recents;
        if tl <= 0.0 || !par_nom {
            return;
        }
        let k = (self.defil / self.ligne_h).round() as usize * self.colonnes.max(1);
        let Some(&fi) = self.vue.get(k.min(self.vue.len().saturating_sub(1))) else {
            return;
        };
        let b = Rect::from_center_size(
            Pos2::new(
                piste.left() - 36.0,
                poig.center()
                    .y
                    .clamp(piste.top() + 28.0, piste.bottom() - 28.0),
            ),
            Vec2::splat(56.0),
        );
        d::ombre(p, b.shrink(4.0), 16.0, 4.0, 14.0, 0.6 * tl);
        panneau(
            p,
            b,
            16.0,
            alpha(jt.surface_haute, tl),
            Some(alpha(jt.accent, 0.8 * tl)),
        );
        let g = ligne(
            p,
            &initiale(&self.fiches[fi].titre).to_string(),
            gras(28.0),
            alpha(jt.texte, tl),
        );
        p.galley(b.center() - g.size() * 0.5, g, alpha(jt.texte, tl));
    }

    fn dessiner_recherche(&mut self, ui: &egui::Ui, p: &egui::Painter, rech: Rect) {
        let jt = j();
        // Le champ réagit à la souris : curseur texte, clic = curseur affiché (la frappe va
        // de toute façon dans la recherche), croix pour effacer.
        let rep = ui
            .interact(rech, Id::unique("recherche"), Sense::click())
            .on_hover_cursor(egui::CursorIcon::Text);
        if rep.clicked() {
            self.recherche_active = true;
        } else if ui.input(|i| i.pointer.any_pressed()) && !rep.hovered() {
            self.recherche_active = false;
        }
        let croix = Rect::from_center_size(
            Pos2::new(rech.right() - 22.0, rech.center().y),
            Vec2::splat(26.0),
        );
        if !self.recherche.is_empty() {
            let rc = ui
                .interact(croix, Id::unique("recherche-x"), Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Effacer (Échap)");
            if rc.clicked() {
                self.recherche.clear();
                self.recherche_active = true;
                self.changer_recherche();
                self.son(Son::Retour);
            }
        }
        let actif = !self.recherche.is_empty();
        let focus = actif || self.recherche_active;
        d::ombre(p, rech.shrink(4.0), 20.0, 4.0, 12.0, 0.5);
        panneau(
            p,
            rech,
            20.0,
            jt.surface,
            Some(if focus {
                alpha(jt.accent, 0.8)
            } else if rep.hovered() {
                jt.doux
            } else {
                jt.trait_
            }),
        );
        // Loupe.
        let lc = Pos2::new(rech.left() + 21.0, rech.center().y - 1.0);
        let tr = Stroke::new(1.8, if focus { jt.texte } else { jt.doux });
        p.circle_stroke(lc, 6.0, tr);
        p.line_segment([lc + Vec2::new(4.3, 4.3), lc + Vec2::new(8.5, 8.5)], tr);
        let xt = rech.left() + 38.0;
        let mut w = 0.0;
        if actif {
            // Texte trop long : on montre sa fin, là où l'on tape.
            let g = ligne(p, &self.recherche, regulier(15.5), jt.texte);
            let place = croix.left() - 4.0 - xt;
            w = g.size().x.min(place);
            let pt = Pos2::new(xt + w - g.size().x, rech.center().y - g.size().y * 0.5);
            let clip = Rect::from_x_y_ranges(xt - 2.0..=croix.left() - 2.0, rech.y_range());
            p.with_clip_rect(clip).galley(pt, g, jt.texte);
            // Croix pour effacer.
            let survol = ui.rect_contains_pointer(croix);
            if survol {
                p.circle_filled(croix.center(), 11.0, jt.surface_haute);
            }
            let cc = croix.center();
            let tr = Stroke::new(1.6, if survol { jt.texte } else { jt.doux });
            for s in [-1.0, 1.0] {
                p.line_segment(
                    [cc + Vec2::new(-4.5, -4.5 * s), cc + Vec2::new(4.5, 4.5 * s)],
                    tr,
                );
            }
        }
        if focus {
            let t = ui.input(|i| i.time);
            if (t * 1.6).fract() < 0.6 {
                p.line_segment(
                    [
                        Pos2::new(xt + w + 2.0, rech.center().y - 9.0),
                        Pos2::new(xt + w + 2.0, rech.center().y + 9.0),
                    ],
                    Stroke::new(1.5, jt.accent),
                );
            }
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
        }
        if !actif {
            let g = bloc(
                p,
                if focus {
                    "Nom du jeu…"
                } else {
                    "Tapez pour chercher un jeu"
                },
                regulier(15.5),
                jt.discret,
                rech.width() - 52.0,
                1,
            );
            p.galley(
                Pos2::new(
                    xt + if focus { 6.0 } else { 0.0 },
                    rech.center().y - g.size().y * 0.5,
                ),
                g,
                jt.discret,
            );
        }
    }

    fn dessiner_vide(&self, p: &egui::Painter, zone: Rect) {
        let jt = j();
        let (titre, texte) = if !self.analyse_finie && self.roms.is_empty() {
            ("Analyse de la bibliothèque…".to_string(), String::new())
        } else if self.hab.onglet == Onglet::Favoris && self.recherche.is_empty() {
            (
                "Aucun favori pour l'instant".to_string(),
                "Choisissez un jeu et appuyez sur F4 (X à la manette) pour le garder ici."
                    .to_string(),
            )
        } else if self.hab.onglet == Onglet::Recents && self.recherche.is_empty() {
            (
                "Aucune partie récente".to_string(),
                "Les jeux lancés apparaîtront ici, du plus récent au plus ancien.".to_string(),
            )
        } else if !self.recherche.is_empty() || self.filtre.is_some() || self.filtre_meta.is_some()
        {
            (
                "Aucun jeu ne correspond".to_string(),
                "Échap efface la recherche, Tab change d'onglet.".to_string(),
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
        let g = ligne(p, &titre, moyen(21.0), jt.texte);
        let gs = g.size();
        p.galley(Pos2::new(c.x - gs.x * 0.5, c.y + 8.0), g, jt.texte);
        if !texte.is_empty() {
            let g = p.layout(texte, regulier(14.5), jt.doux, zone.width().min(620.0));
            let gs = g.size();
            p.galley(Pos2::new(c.x - gs.x * 0.5, c.y + 44.0), g, jt.doux);
        }
    }
}

/// Au-delà, les jaquettes les moins récemment vues sont libérées (~0,5 Mo chacune).
const TEXTURES_MAX: usize = 300;

/// Sélection après `dx` cartes puis `dy` lignes dans une grille de `n` cartes.
fn deplacer(sel: usize, n: usize, colonnes: usize, dx: i32, dy: i32) -> usize {
    let c = colonnes.max(1) as i32;
    let mut s = sel as i32 + dx;
    for _ in 0..dy.abs() {
        let suiv = s + c * dy.signum();
        if suiv >= 0 && suiv < n as i32 {
            s = suiv;
        } else if dy > 0 && (s / c) < (n as i32 - 1) / c {
            // Ligne incomplète en bas : on va au dernier jeu.
            s = n as i32 - 1;
        }
    }
    s.clamp(0, n as i32 - 1) as usize
}

/// La ligne `ligne` a-t-elle au moins la moitié de sa hauteur à l'écran ?
fn ligne_visible(ligne: usize, ligne_h: f32, defil: f32, vue_h: f32) -> bool {
    let milieu = ligne as f32 * ligne_h + ligne_h * 0.5;
    milieu >= defil && milieu <= defil + vue_h
}

/// Poignée de la barre : (haut, hauteur) dans une piste de hauteur `piste_h`.
fn poignee(piste_h: f32, vue_h: f32, contenu_h: f32, defil: f32) -> (f32, f32) {
    let h = (piste_h * vue_h / contenu_h.max(1.0)).clamp(36.0_f32.min(piste_h), piste_h);
    let max = (contenu_h - vue_h).max(0.0);
    let y = if max > 0.0 {
        (piste_h - h) * (defil / max).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (y, h)
}

/// Défilement qui place le haut de la poignée (hauteur `h`) à `y` dans la piste.
fn defil_de_poignee(y: f32, piste_h: f32, h: f32, contenu_h: f32, vue_h: f32) -> f32 {
    let course = piste_h - h;
    if course <= 0.0 {
        return 0.0;
    }
    (y / course).clamp(0.0, 1.0) * (contenu_h - vue_h).max(0.0)
}

/// Lettre de la bulle de défilement : initiale sans accent, « # » pour un chiffre ou un signe.
fn initiale(titre: &str) -> char {
    match plier(titre).chars().find(|c| c.is_alphanumeric()) {
        Some(c) if c.is_ascii_alphabetic() => c.to_ascii_uppercase(),
        _ => '#',
    }
}

/// Dégradé vers le fond (assombri comme par le vignettage) sur la hauteur de `r`.
fn fondu_bas(p: &egui::Painter, r: Rect) {
    use egui::epaint::{Mesh, Vertex, WHITE_UV};
    let jt = j();
    let bas = jt.fond.lerp_to_gamma(Color32::BLACK, jt.vignette * 0.35);
    let mut m = Mesh::default();
    for (pos, c) in [
        (r.left_top(), Color32::TRANSPARENT),
        (r.right_top(), Color32::TRANSPARENT),
        (r.right_bottom(), bas),
        (r.left_bottom(), bas),
    ] {
        m.vertices.push(Vertex {
            pos,
            uv: WHITE_UV,
            color: c,
        });
    }
    m.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    p.add(egui::Shape::mesh(m));
}

/// Une carte de la grille : panneau en matière, jaquette ou cartouche dessinée, puis titre et
/// ligne discrète (plateforme, détails ou date, temps de jeu, favori).
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
    jeu: stats::Jeu,
    recent: bool,
) {
    let jt = j();
    let couverture = Rect::from_min_size(r.min, Vec2::splat(cote));
    let zone = couverture.translate(Vec2::new(0.0, -4.0 * t));
    let rayon = 14.0;
    let ts = t.clamp(0.0, 1.0);
    // Ombre portée : courte au repos, plus longue quand la carte se soulève.
    d::ombre(
        p,
        zone.shrink(8.0),
        rayon,
        10.0 + 10.0 * ts,
        26.0 + 12.0 * ts,
        0.5 + 0.4 * ts,
    );
    if ts > 0.01 {
        p.add(
            egui::epaint::Shadow {
                offset: [0, 0],
                blur: 18,
                spread: 1,
                color: alpha(jt.accent_clair, 0.28 * ts),
            }
            .as_shape(zone.expand(3.0), CornerRadius::same(17)),
        );
    }
    let mut fond = jt.surface.lerp_to_gamma(jt.surface_haute, ts * 0.6);
    if survol && ts < 0.5 {
        fond = teinte(fond, if jt.jour { 0.98 } else { 1.08 });
    }
    panneau(p, zone, rayon, fond, Some(jt.trait_));
    match tex {
        Some(tex) => {
            let [tl, th] = tex.size();
            let ratio = tl as f32 / th.max(1) as f32;
            let interieur = zone.shrink(zone.width() * 0.07);
            let taille = if ratio >= 1.0 {
                Vec2::new(interieur.width(), interieur.width() / ratio)
            } else {
                Vec2::new(interieur.height() * ratio, interieur.height())
            };
            let ri = Rect::from_center_size(interieur.center(), taille);
            d::ombre(p, ri.shrink(2.0), 6.0, 5.0, 12.0, 0.8);
            p.add(
                egui::epaint::RectShape::filled(ri, CornerRadius::same(6), Color32::WHITE)
                    .with_texture(
                        tex.id(),
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    ),
            );
            // Reflet de la lampe sur la boîte.
            p.rect_stroke(
                ri,
                CornerRadius::same(6),
                Stroke::new(1.0, Color32::from_white_alpha(22)),
                StrokeKind::Inside,
            );
        }
        None => cartouche(p, zone, pl, &f.titre),
    }
    if ts > 0.0 {
        p.rect_stroke(
            zone.expand(3.0),
            CornerRadius::same(17),
            Stroke::new(2.0, alpha(jt.accent, ts)),
            StrokeKind::Outside,
        );
    }

    let col = if t > 0.5 {
        jt.texte
    } else {
        jt.texte.lerp_to_gamma(jt.doux, 0.25)
    };
    let g = bloc(p, &f.titre, moyen(15.0), col, cote - 2.0, 2);
    let hg = g.size().y;
    let y = couverture.bottom() + 12.0;
    p.galley(Pos2::new(r.left() + 1.0, y), g, col);
    let yd = y + hg + 4.0;

    // À droite : temps de jeu et étoile ; à gauche : pastille et détails (ou date).
    let mut droite = r.left() + cote;
    if jeu.favori {
        icone(
            p,
            Icone::Etoile,
            Pos2::new(droite - 6.0, yd + 8.0),
            12.0,
            jt.alerte,
        );
        droite -= 18.0;
    }
    let temps = stats::duree_jeu(jeu.temps);
    if !temps.is_empty() {
        let g = ligne(p, &temps, regulier(12.5), jt.doux);
        let gs = g.size();
        p.galley(Pos2::new(droite - gs.x, yd), g, jt.doux);
        droite -= gs.x + 10.0;
    }
    p.circle_filled(Pos2::new(r.left() + 5.0, yd + 8.0), 3.5, couleur(pl));
    let quand = stats::il_y_a(jeu.dernier);
    // Sans base de jeux, les détails ne sont que le nom du fichier (déjà dit par le titre) :
    // le nom de la plateforme est plus utile.
    let details = if recent && !quand.is_empty() {
        quand.as_str()
    } else if f.details.contains('.') {
        pl.nom()
    } else {
        f.details.as_str()
    };
    let g = bloc(
        p,
        details,
        regulier(12.5),
        jt.doux,
        (droite - r.left() - 15.0).max(10.0),
        1,
    );
    p.galley(Pos2::new(r.left() + 15.0, yd), g, jt.doux);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 5 200 fiches synthétiques, titres variés (accents compris).
    fn bibliotheque(n: usize) -> (Vec<Fiche>, Vec<Rom>) {
        let mots = [
            "Donjon", "Étoile", "Kobold", "Forêt", "Dragon", "Lune", "Zèbre", "Kart",
        ];
        let pl = [Plateforme::Gb, Plateforme::Gbc, Plateforme::Gba];
        let roms: Vec<Rom> = (0..n)
            .map(|i| Rom {
                fichier: PathBuf::from(format!("jeux/{i}.gb")),
                interne: None,
                taille: 0,
                crc: i as u32,
                plateforme: pl[i % 3],
                titre_entete: String::new(),
            })
            .collect();
        let fiches = (0..n)
            .map(|i| {
                let titre = format!("{} {} {i}", mots[i % 8], mots[(i / 8) % 8]);
                Fiche {
                    rom: i,
                    cle: plier(&format!("{titre} {i}.gb")),
                    titre,
                    details: String::new(),
                    dat: None,
                    annee: None,
                    editeur: String::new(),
                    genre: String::new(),
                }
            })
            .collect();
        (fiches, roms)
    }

    #[test]
    fn filtrage_mots_accents_plateforme() {
        let (fiches, roms) = bibliotheque(5200);
        assert_eq!(filtrer_fiches(&fiches, &roms, None, "").len(), 5200);
        assert_eq!(filtrer_fiches(&fiches, &roms, None, "   ").len(), 5200);
        // « foret » trouve « Forêt », « ETOILE » trouve « Étoile » ; tous les mots requis.
        let foret = filtrer_fiches(&fiches, &roms, None, "foret");
        assert!(!foret.is_empty() && foret.iter().all(|&i| fiches[i].titre.contains("Forêt")));
        let deux = filtrer_fiches(&fiches, &roms, None, "ETOILE  lune");
        assert!(!deux.is_empty());
        assert!(deux
            .iter()
            .all(|&i| fiches[i].titre.contains("Étoile") && fiches[i].titre.contains("Lune")));
        let gba = filtrer_fiches(&fiches, &roms, Some(Plateforme::Gba), "dragon");
        assert!(gba
            .iter()
            .all(|&i| roms[fiches[i].rom].plateforme == Plateforme::Gba));
        assert!(filtrer_fiches(&fiches, &roms, None, "introuvable").is_empty());
    }

    #[test]
    fn filtrage_5200_sous_16_ms() {
        let (fiches, roms) = bibliotheque(5200);
        // Une frappe = un filtrage complet ; on prend la pire de 20 frappes.
        let mut pire = std::time::Duration::ZERO;
        for t in [
            "d",
            "dr",
            "dra",
            "drag",
            "drago",
            "dragon",
            "dragon l",
            "dragon lune",
        ]
        .iter()
        .cycle()
        .take(20)
        {
            let t0 = std::time::Instant::now();
            std::hint::black_box(filtrer_fiches(&fiches, &roms, None, t));
            pire = pire.max(t0.elapsed());
        }
        assert!(pire.as_millis() < 16, "filtrage de 5 200 fiches : {pire:?}");
    }

    #[test]
    fn deplacement_dans_la_grille() {
        // 10 cartes sur 4 colonnes : lignes 0-3, 4-7, 8-9.
        assert_eq!(deplacer(0, 10, 4, 1, 0), 1);
        assert_eq!(deplacer(0, 10, 4, -1, 0), 0);
        assert_eq!(deplacer(1, 10, 4, 0, 1), 5);
        // Ligne incomplète en bas : dernier jeu.
        assert_eq!(deplacer(7, 10, 4, 0, 1), 9);
        // Saut de page de 3 lignes, borné aux extrémités.
        assert_eq!(deplacer(2, 10, 4, 0, 3), 9);
        assert_eq!(deplacer(9, 10, 4, 0, -3), 1);
        assert_eq!(deplacer(5, 10, 4, 0, -5), 1);
        assert_eq!(deplacer(9, 10, 4, 5, 0), 9);
    }

    #[test]
    fn ligne_visible_a_moitie() {
        // Lignes de 100 px, vue de 250 px.
        assert!(ligne_visible(0, 100.0, 0.0, 250.0));
        assert!(ligne_visible(2, 100.0, 0.0, 250.0));
        assert!(!ligne_visible(3, 100.0, 0.0, 250.0));
        assert!(!ligne_visible(0, 100.0, 60.0, 250.0));
        assert!(ligne_visible(1, 100.0, 60.0, 250.0));
    }

    #[test]
    fn poignee_et_retour() {
        // Contenu 10 fois plus haut que la vue : poignée au dixième de la piste.
        let (y, h) = poignee(500.0, 600.0, 6000.0, 0.0);
        assert_eq!((y, h), (0.0, 50.0));
        let (y, _) = poignee(500.0, 600.0, 6000.0, 5400.0);
        assert_eq!(y, 450.0);
        let (y, _) = poignee(500.0, 600.0, 6000.0, 2700.0);
        assert_eq!(y, 225.0);
        // Aller-retour poignée → défilement.
        assert_eq!(defil_de_poignee(225.0, 500.0, 50.0, 6000.0, 600.0), 2700.0);
        assert_eq!(defil_de_poignee(-30.0, 500.0, 50.0, 6000.0, 600.0), 0.0);
        assert_eq!(defil_de_poignee(900.0, 500.0, 50.0, 6000.0, 600.0), 5400.0);
        // 5 200 jeux : poignée jamais plus petite que 36 px.
        let (_, h) = poignee(560.0, 660.0, 870.0 * 270.0, 0.0);
        assert_eq!(h, 36.0);
        // Tout tient dans la vue : poignée pleine, immobile.
        assert_eq!(poignee(500.0, 600.0, 400.0, 0.0), (0.0, 500.0));
        assert_eq!(defil_de_poignee(10.0, 500.0, 500.0, 400.0, 600.0), 0.0);
    }

    #[test]
    fn initiale_de_bulle() {
        assert_eq!(initiale("Zelda"), 'Z');
        assert_eq!(initiale("Étoile filante"), 'E');
        assert_eq!(initiale("'Totally' Rad"), 'T');
        assert_eq!(initiale("1942"), '#');
        assert_eq!(initiale(""), '#');
    }
}
