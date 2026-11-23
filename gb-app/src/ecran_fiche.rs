//! Fiche d'un jeu (F1, Y à la manette, clic droit) : jaquette en grand, métadonnées
//! libretro-database (et ScreenScraper si configuré), écran titre et capture, temps de jeu,
//! favori, emplacements d'état ; Jouer, favori et filtres « même éditeur / genre / année ».

use super::*;
use crate::dat::InfoJeu;
use crate::etats::{self, Cible, Etats};
use crate::jaquettes;
use crate::screenscraper::Demande;
use crate::sons_ui::Son;
use crate::stats;

/// Champ de métadonnées servant de filtre.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Champ {
    Editeur,
    Genre,
    Annee,
}

impl Champ {
    pub(super) fn valeur(self, f: &Fiche) -> String {
        match self {
            Champ::Editeur => f.editeur.clone(),
            Champ::Genre => f.genre.clone(),
            Champ::Annee => f.annee.map(|a| a.to_string()).unwrap_or_default(),
        }
    }

    fn nom(self) -> &'static str {
        match self {
            Champ::Editeur => "Éditeur",
            Champ::Genre => "Genre",
            Champ::Annee => "Année",
        }
    }
}

/// Emplacement d'état occupé, avec sa miniature.
struct EtatVu {
    libelle: String,
    date: u64,
    tex: Option<TextureHandle>,
}

pub(super) struct Panneau {
    pub(super) crc: u32,
    ouvert_a: f64,
    /// Bouton choisi (voir `boutons`).
    bouton: usize,
    etats: Vec<EtatVu>,
}

#[derive(Clone, Copy, PartialEq)]
enum ActFiche {
    Jouer,
    Favori,
    Telecharger,
    Filtre(Champ),
}

/// Base fictive de la bibliothèque de démonstration (aucun fichier derrière).
pub(super) fn base_demo(roms: &[Rom]) -> Base {
    let editeurs = [
        "Studio Lanterne",
        "Atelier Brume",
        "Kobold Soft",
        "Givre & Fils",
    ];
    let genres = ["Aventure", "Course", "Réflexion", "Plates-formes", "Sport"];
    let mut base = Base::default();
    for (i, r) in roms.iter().enumerate() {
        base.jeux.insert(
            r.crc,
            InfoJeu {
                nom: r.titre_entete.clone(),
                titre: r.titre_entete.clone(),
                region: ["Europe", "France", "USA", "Japon"][i % 4].into(),
                annee: Some(1990 + (i * 7 % 15) as u16),
                plateforme: r.plateforme,
                mois: Some((i % 12 + 1) as u8),
                developpeur: editeurs[(i + 1) % 4].into(),
                editeur: editeurs[i % 4].into(),
                genre: genres[i % 5].into(),
                joueurs: Some(if i % 3 == 0 { 2 } else { 1 }),
                esrb: if i % 2 == 0 {
                    "E".into()
                } else {
                    String::new()
                },
                resume: String::new(),
                source_resume: String::new(),
            },
        );
    }
    base
}

const MOIS: [&str; 12] = [
    "janvier",
    "février",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "août",
    "septembre",
    "octobre",
    "novembre",
    "décembre",
];

/// « août 1998 », « 1998 » ou vide.
fn date_sortie(i: &InfoJeu) -> String {
    match (i.annee, i.mois) {
        (Some(a), Some(m)) if (1..=12).contains(&m) => format!("{} {a}", MOIS[m as usize - 1]),
        (Some(a), _) => a.to_string(),
        _ => String::new(),
    }
}

fn joueurs(n: &str) -> String {
    match n.trim() {
        "" => String::new(),
        "1" => "1 joueur".into(),
        n => format!("{n} joueurs"),
    }
}

/// Taille lisible : « 32 Kio », « 4 Mio ».
fn taille(o: u64) -> String {
    if o >= 1 << 20 {
        format!(
            "{} Mio",
            (o as f64 / (1u64 << 20) as f64 * 10.0).round() / 10.0
        )
    } else {
        format!("{} Kio", o.div_ceil(1024))
    }
}

/// Écran de la console (proportions des écrans titre, captures et miniatures).
fn proportion(p: Plateforme) -> f32 {
    match p {
        Plateforme::Gba => 240.0 / 160.0,
        _ => 160.0 / 144.0,
    }
}

impl App {
    /// Ouvre la fiche du jeu sélectionné.
    pub(super) fn ouvrir_fiche(&mut self) {
        if self.vue.get(self.sel).is_none() {
            self.son(Son::Erreur);
            return;
        }
        let ouvert_a = self.panneau.as_ref().map_or(self.t_prec, |p| p.ouvert_a);
        self.charger_fiche();
        if let Some(p) = &mut self.panneau {
            p.ouvert_a = ouvert_a;
        }
        self.son(Son::Valider);
    }

    /// Prépare la fiche de la sélection : vignettes et ScreenScraper demandés, états lus.
    fn charger_fiche(&mut self) {
        let Some(&fi) = self.vue.get(self.sel) else {
            self.panneau = None;
            return;
        };
        let rom = self.roms[self.fiches[fi].rom].clone();
        // Depuis la base (les fiches de la grille peuvent dater d'avant son arrivée).
        let dat = self
            .base
            .as_ref()
            .and_then(|b| b.jeux.get(&rom.crc))
            .map(|i| (i.plateforme, i.nom.clone()));
        if let (Some(j), Some((pl, nom))) = (&self.jaquettes, &dat) {
            for v in [Vignette::Boite, Vignette::Titre, Vignette::Capture] {
                let k = jaquettes::cle(v, nom);
                if !self.textures.contains_key(&k) && self.demandees.insert(k) {
                    for abandon in j.demander(*pl, nom, v) {
                        self.demandees.remove(&abandon);
                    }
                }
            }
        }
        if let (Some(ss), false) = (&self.ss, self.ss_fiches.contains_key(&rom.crc)) {
            // Même plateforme (celle de la base) et même nom que l'import : même cache.
            let t = self.tache(&rom);
            ss.demander(Demande {
                crc: t.crc,
                taille: t.taille,
                plateforme: t.plateforme,
                nom: t.fichier,
                id: self.ss_id.clone(),
            });
        }
        let etats = if self.opts.demo && rom.fichier.starts_with("demo") {
            Vec::new()
        } else {
            self.lire_etats(&rom)
        };
        self.panneau = Some(Panneau {
            crc: rom.crc,
            ouvert_a: self.t_prec,
            bouton: 0,
            etats,
        });
    }

    /// Emplacements occupés, du plus récent au plus ancien, avec leur miniature.
    /// Fiche ouverte relue (téléchargement fini), bouton et animation gardés.
    pub(super) fn recharger_fiche(&mut self) {
        let garde = self.panneau.as_ref().map(|p| (p.ouvert_a, p.bouton));
        self.charger_fiche();
        if let (Some(p), Some((o, b))) = (&mut self.panneau, garde) {
            p.ouvert_a = o;
            p.bouton = b;
        }
    }

    fn lire_etats(&self, rom: &Rom) -> Vec<EtatVu> {
        let e = Etats::new(self.chemins.etats(rom));
        let mut v: Vec<EtatVu> = std::iter::once(Cible::Auto)
            .chain((1..=etats::NB).map(Cible::Num))
            .filter_map(|c| {
                let info = e.info(c)?;
                let tex = e
                    .lire_mini(c)
                    .and_then(|d| jaquettes::decoder(&d))
                    .map(|img| {
                        let ci = ColorImage::from_rgba_unmultiplied(
                            [img.largeur, img.hauteur],
                            &img.rgba,
                        );
                        self.ctx
                            .load_texture(format!("etat-{c:?}"), ci, TextureOptions::NEAREST)
                    });
                let libelle = match c {
                    Cible::Num(n) => format!("Emplacement {n}"),
                    _ => "Reprise".to_string(),
                };
                Some(EtatVu {
                    libelle,
                    date: info.date,
                    tex,
                })
            })
            .collect();
        v.sort_by_key(|e| std::cmp::Reverse(e.date));
        v
    }

    fn fermer_fiche(&mut self) {
        self.panneau = None;
        self.son(Son::Retour);
    }

    pub(super) fn retirer_filtre_meta(&mut self) {
        self.filtre_meta = None;
        self.changer_recherche();
        self.son(Son::Retour);
    }

    /// Réponses ScreenScraper : fiche gardée pour la session, logo et jaquette en textures.
    pub(super) fn recevoir_ss(&mut self) {
        let Some(ss) = &self.ss else { return };
        while let Ok(r) = ss.rx.try_recv() {
            for (genre, img) in [("wheel", r.wheel), ("boite", r.boite)] {
                if let Some(img) = img {
                    let ci =
                        ColorImage::from_rgba_unmultiplied([img.largeur, img.hauteur], &img.rgba);
                    let nom = format!("ss-{genre}:{:08X}", r.crc);
                    let t = self.ctx.load_texture(&nom, ci, TextureOptions::LINEAR);
                    self.textures.insert(nom, (t, self.images_ui));
                }
            }
            self.ss_fiches.insert(r.crc, r.fiche);
        }
    }

    fn boutons(&self, fi: usize) -> Vec<(String, ActFiche)> {
        let f = &self.fiches[fi];
        let mut v = vec![
            ("Jouer".to_string(), ActFiche::Jouer),
            ("Favori".to_string(), ActFiche::Favori),
        ];
        if let Some(t) = &self.tele {
            let lib = if t.en_attente(self.roms[f.rom].crc) {
                "Téléchargement…"
            } else {
                "Télécharger les infos"
            };
            v.push((lib.to_string(), ActFiche::Telecharger));
        }
        for (c, nom) in [
            (Champ::Editeur, "Même éditeur"),
            (Champ::Genre, "Même genre"),
            (Champ::Annee, "Même année"),
        ] {
            if !c.valeur(f).is_empty() {
                v.push((nom.to_string(), ActFiche::Filtre(c)));
            }
        }
        v
    }

    fn agir_fiche(&mut self, act: ActFiche) {
        match act {
            ActFiche::Jouer => self.lancer_avec_son(),
            ActFiche::Favori => self.basculer_favori(),
            ActFiche::Telecharger => {
                if let Some(&fi) = self.vue.get(self.sel) {
                    let rom = self.roms[self.fiches[fi].rom].clone();
                    self.telecharger_un(&rom);
                }
            }
            ActFiche::Filtre(c) => {
                let Some(&fi) = self.vue.get(self.sel) else {
                    return;
                };
                let val = c.valeur(&self.fiches[fi]);
                let crc = self.roms[self.fiches[fi].rom].crc;
                self.dire(format!("{} : {val}", c.nom()));
                self.filtre_meta = Some((c, val));
                self.panneau = None;
                self.son(Son::Valider);
                // Toute la bibliothèque, le jeu de départ reste sélectionné.
                self.hab.onglet = ecran_biblio::Onglet::Tous;
                self.filtre = None;
                self.recherche.clear();
                self.changer_recherche();
                if let Some(k) = self
                    .vue
                    .iter()
                    .position(|&i| self.roms[self.fiches[i].rom].crc == crc)
                {
                    self.sel = k;
                }
                self.rendre_visible();
            }
        }
    }

    /// Jeu précédent / suivant de la bibliothèque, sans fermer la fiche.
    fn fiche_voisine(&mut self, sens: i32) {
        let n = self.vue.len();
        if n < 2 {
            return;
        }
        let bouton = self.panneau.as_ref().map_or(0, |p| p.bouton);
        let ouvert_a = self.panneau.as_ref().map_or(self.t_prec, |p| p.ouvert_a);
        self.sel = (self.sel as i32 + sens).rem_euclid(n as i32) as usize;
        self.rendre_visible();
        self.charger_fiche();
        if let Some(p) = &mut self.panneau {
            p.ouvert_a = ouvert_a;
            p.bouton = bouton;
        }
        self.son(Son::Deplacement);
    }

    pub(super) fn entrees_fiche(&mut self, evts: &[Event], pad: Etat) {
        let Some(&fi) = self.vue.get(self.sel) else {
            self.panneau = None;
            return;
        };
        // Sélection changée par ailleurs (base arrivée, analyse) : la fiche suit.
        if self.panneau.as_ref().map(|p| p.crc) != Some(self.roms[self.fiches[fi].rom].crc) {
            self.charger_fiche();
        }
        let n = self.boutons(fi).len();
        let mut dx = 0i32;
        let mut voisin = 0i32;
        let mut valider = pad.appuis & b::SUD != 0;
        let mut fermer = pad.appuis & (b::EST | b::NORD | b::START) != 0;
        let mut favori = pad.appuis & b::OUEST != 0;
        if pad.appuis & b::GAUCHE != 0 {
            dx -= 1;
        }
        if pad.appuis & b::DROITE != 0 {
            dx += 1;
        }
        if pad.appuis & (b::HAUT | b::L1) != 0 {
            voisin -= 1;
        }
        if pad.appuis & (b::BAS | b::R1) != 0 {
            voisin += 1;
        }
        for e in evts {
            if let Event::Key {
                key, pressed: true, ..
            } = e
            {
                match key {
                    Key::ArrowLeft => dx -= 1,
                    Key::ArrowRight => dx += 1,
                    Key::ArrowUp | Key::PageUp => voisin -= 1,
                    Key::ArrowDown | Key::PageDown => voisin += 1,
                    Key::Tab => dx += 1,
                    Key::Enter | Key::Space => valider = true,
                    Key::Escape | Key::F1 | Key::Backspace => fermer = true,
                    Key::F4 => favori = true,
                    _ => {}
                }
            }
        }
        if fermer {
            self.fermer_fiche();
            return;
        }
        if favori {
            self.basculer_favori();
        }
        if voisin != 0 {
            self.fiche_voisine(voisin.signum());
            return;
        }
        if let Some(p) = &mut self.panneau {
            let avant = p.bouton;
            p.bouton = (p.bouton as i32 + dx).rem_euclid(n.max(1) as i32) as usize;
            if p.bouton != avant {
                self.son(Son::Deplacement);
            }
        }
        if valider {
            let b = self.panneau.as_ref().map_or(0, |p| p.bouton);
            if let Some((_, act)) = self.boutons(fi).get(b).cloned() {
                self.agir_fiche(act);
            }
        }
    }

    /// Filtre actif, dans le pied de la bibliothèque : pastille cliquable pour le retirer.
    pub(super) fn dessiner_filtre_meta(
        &mut self,
        ui: &egui::Ui,
        p: &egui::Painter,
        droite: f32,
        y: f32,
    ) {
        let Some((c, v)) = &self.filtre_meta else {
            return;
        };
        let jt = j();
        let g = ligne(p, &format!("{} : {v}", c.nom()), moyen(13.5), jt.texte);
        let gs = g.size();
        let r = Rect::from_min_max(
            Pos2::new(droite - gs.x - 50.0, y - 15.0),
            Pos2::new(droite, y + 15.0),
        );
        let rep = ui
            .interact(r, Id::unique("filtre-meta"), Sense::CLICK)
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Retirer le filtre (Échap)");
        panneau(
            p,
            r,
            15.0,
            if rep.hovered() {
                jt.surface_haute
            } else {
                jt.surface
            },
            Some(alpha(jt.accent, 0.8)),
        );
        p.galley(Pos2::new(r.left() + 14.0, y - gs.y * 0.5), g, jt.texte);
        let cc = Pos2::new(r.right() - 17.0, y);
        let tr = Stroke::new(1.6, if rep.hovered() { jt.texte } else { jt.doux });
        for s in [-1.0, 1.0] {
            p.line_segment(
                [cc + Vec2::new(-4.0, -4.0 * s), cc + Vec2::new(4.0, 4.0 * s)],
                tr,
            );
        }
        if rep.clicked() {
            self.retirer_filtre_meta();
        }
    }

    /// Texture chargée (et notée vue, pour ne pas être libérée pendant l'affichage).
    fn texture_vue(&mut self, cle: &str) -> Option<TextureHandle> {
        let images = self.images_ui;
        self.textures.get_mut(cle).map(|(t, vu)| {
            *vu = images;
            t.clone()
        })
    }

    pub(super) fn dessiner_fiche(&mut self, ui: &mut egui::Ui) {
        bloquer_dessous(ui, "fiche");
        let Some(&fi) = self.vue.get(self.sel) else {
            self.panneau = None;
            return;
        };
        let Some(pan) = &self.panneau else { return };
        let jt = j();
        let ctx = ui.ctx().clone();
        let ecran = ui.max_rect();
        let age = (self.t_prec - pan.ouvert_a) as f32;
        let bouton = pan.bouton;
        let t = d::sortie(age / d::duree_fondu(DUREE_MENU).max(0.001));
        let tm = d::ressort(age / d::duree(DUREE_MENU).max(0.001));
        if age < DUREE_MENU {
            ctx.request_repaint();
        }

        let f = &self.fiches[fi];
        let rom = self.roms[f.rom].clone();
        let pl = rom.plateforme;
        let titre = f.titre.clone();
        let dat = f.dat.clone();
        let info = self
            .base
            .as_ref()
            .and_then(|b| b.jeux.get(&rom.crc))
            .cloned();
        let ss = self
            .ss_fiches
            .get(&rom.crc)
            .and_then(|r| r.as_ref().ok())
            .cloned();
        let ss_erreur = self
            .ss_fiches
            .get(&rom.crc)
            .and_then(|r| r.as_ref().err())
            .filter(|_| self.ss_id.actif())
            .cloned();
        let jeu = self.hab.stats.jeu(rom.crc);
        let boutons = self.boutons(fi);
        let vignette = |v| dat.as_ref().map(|(_, n)| jaquettes::cle(v, n));
        let tex_boite = vignette(Vignette::Boite)
            .and_then(|k| self.texture_vue(&k))
            .or_else(|| self.texture_vue(&format!("ss-boite:{:08X}", rom.crc)));
        let tex_titre = vignette(Vignette::Titre).and_then(|k| self.texture_vue(&k));
        let tex_capture = vignette(Vignette::Capture).and_then(|k| self.texture_vue(&k));
        let tex_wheel = self.texture_vue(&format!("ss-wheel:{:08X}", rom.crc));
        let teinte_boite = dat
            .as_ref()
            .and_then(|(_, n)| self.hab.teintes.get(n).copied())
            .unwrap_or_else(|| couleur(pl));

        // Voile sur la bibliothèque ; un clic dessus ferme la fiche.
        let fond = ui.interact(ecran, Id::unique("fiche-voile"), Sense::CLICK);
        let p = ui.painter().clone();
        p.rect_filled(
            ecran,
            CornerRadius::ZERO,
            alpha(
                Color32::from_rgb(10, 11, 18),
                if jt.jour { 0.45 } else { 0.72 } * t,
            ),
        );
        let l = (ecran.width() - 64.0).min(1120.0);
        // Le pied reste libre pour les raccourcis de la fiche.
        let haut = ecran.height() - HAUT_PIED;
        let h = (haut - 48.0).min(700.0);
        let r = Rect::from_center_size(
            Pos2::new(ecran.center().x, ecran.top() + haut * 0.5),
            Vec2::new(l, h),
        )
        .translate(Vec2::new(0.0, (1.0 - tm) * 14.0));
        let mut p = p;
        p.multiply_opacity(t);
        d::ombre(&p, r, 22.0, 18.0, 48.0, 1.0);
        panneau(&p, r, 22.0, jt.surface, Some(jt.trait_));
        // Le panneau capte les clics (sinon le voile dessous fermerait la fiche).
        let dans = ui.interact(r, Id::unique("fiche-panneau"), Sense::CLICK);
        if fond.clicked() && !dans.hovered() {
            self.fermer_fiche();
            return;
        }

        let marge = 30.0;
        let pc = p.with_clip_rect(r.shrink(2.0));

        // ----- Colonne gauche : jaquette sous la lampe, vie du jeu, états -----
        let cote = (h * 0.52).min(l * 0.32);
        let gauche =
            Rect::from_min_size(r.min + Vec2::splat(marge), Vec2::new(cote, h - 2.0 * marge));
        let couv = Rect::from_min_size(gauche.min, Vec2::splat(cote));
        d::lueur(
            &pc,
            couv.center(),
            Vec2::splat(cote * 0.9),
            alpha(teinte_boite, if jt.jour { 0.32 } else { 0.24 }),
        );
        match &tex_boite {
            Some(tex) => {
                let [tl, th] = tex.size();
                let ratio = tl as f32 / th.max(1) as f32;
                let taille = if ratio >= 1.0 {
                    Vec2::new(cote, cote / ratio)
                } else {
                    Vec2::new(cote * ratio, cote)
                };
                let ri = Rect::from_center_size(couv.center(), taille);
                d::ombre(&pc, ri.shrink(4.0), 8.0, 10.0, 26.0, 1.0);
                pc.add(
                    egui::epaint::RectShape::filled(ri, CornerRadius::same(8), Color32::WHITE)
                        .with_texture(
                            tex.id(),
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        ),
                );
                pc.rect_stroke(
                    ri,
                    CornerRadius::same(8),
                    Stroke::new(1.0, Color32::from_white_alpha(26)),
                    StrokeKind::Inside,
                );
            }
            None => cartouche(&pc, couv, pl, &titre),
        }

        let mut y = couv.bottom() + 24.0;
        let fait = |p: &egui::Painter, y: f32, lib: &str, val: &str| {
            let g = ligne(p, lib, regulier(13.0), jt.discret);
            p.galley(Pos2::new(gauche.left(), y), g, jt.discret);
            let g = bloc(p, val, moyen(14.5), jt.texte, cote * 0.58, 1);
            let gs = g.size();
            p.galley(Pos2::new(gauche.right() - gs.x, y - 1.0), g, jt.texte);
        };
        let temps = stats::duree_jeu(jeu.temps);
        fait(
            &pc,
            y,
            "Temps de jeu",
            if temps.is_empty() {
                "Pas encore joué"
            } else {
                &temps
            },
        );
        y += 26.0;
        if jeu.dernier > 0 {
            fait(&pc, y, "Dernière partie", &stats::il_y_a(jeu.dernier));
            y += 26.0;
        }
        if jeu.favori {
            icone(
                &pc,
                Icone::Etoile,
                Pos2::new(gauche.left() + 6.0, y + 8.0),
                13.0,
                jt.alerte,
            );
            let g = ligne(&pc, "Dans vos favoris", regulier(13.0), jt.doux);
            pc.galley(Pos2::new(gauche.left() + 20.0, y), g, jt.doux);
            y += 26.0;
        }
        y += 10.0;
        pc.line_segment(
            [Pos2::new(gauche.left(), y), Pos2::new(gauche.right(), y)],
            Stroke::new(1.0, jt.trait_),
        );
        y += 14.0;
        let etats = &self.panneau.as_ref().map_or(&[][..], |p| &p.etats[..]);
        let g = ligne(
            &pc,
            &match etats.len() {
                0 => "Aucun état sauvegardé".to_string(),
                1 => "1 état sauvegardé".to_string(),
                n => format!("{n} états sauvegardés"),
            },
            regulier(13.0),
            jt.discret,
        );
        pc.galley(Pos2::new(gauche.left(), y), g, jt.discret);
        y += 24.0;
        let ratio = proportion(pl);
        let lm = (cote - 2.0 * 10.0) / 3.0;
        let hm = lm / ratio;
        if y + hm + 20.0 <= gauche.bottom() {
            for (k, e) in etats.iter().take(3).enumerate() {
                let m = Rect::from_min_size(
                    Pos2::new(gauche.left() + k as f32 * (lm + 10.0), y),
                    Vec2::new(lm, hm),
                );
                pc.rect_filled(m.expand(3.0), CornerRadius::same(6), jt.fond_jeu);
                if let Some(tex) = &e.tex {
                    pc.image(
                        tex.id(),
                        m,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                let lib = if e.libelle == "Reprise" {
                    "Reprise".to_string()
                } else {
                    e.libelle.replace("Emplacement ", "n° ")
                };
                let g = bloc(&pc, &lib, regulier(12.0), jt.doux, lm, 1);
                pc.galley(Pos2::new(m.left(), m.bottom() + 6.0), g, jt.doux);
                let quand = etats::date_relative(e.date, etats::maintenant());
                let g = bloc(&pc, &quand, regulier(11.5), jt.discret, lm, 1);
                pc.galley(Pos2::new(m.left(), m.bottom() + 22.0), g, jt.discret);
            }
        }

        // ----- Colonne droite : titre, faits, résumé, écrans, actions -----
        let droite = Rect::from_min_max(
            Pos2::new(gauche.right() + 40.0, r.top() + marge),
            Pos2::new(r.right() - marge, r.bottom() - marge),
        );
        let ld = droite.width();
        let mut y = droite.top();
        if let Some(w) = &tex_wheel {
            let [tl, th] = w.size();
            let hw = 58.0f32.min(ld * 0.6 * th as f32 / tl.max(1) as f32);
            let lw = hw * tl as f32 / th.max(1) as f32;
            pc.image(
                w.id(),
                Rect::from_min_size(Pos2::new(droite.left(), y), Vec2::new(lw, hw)),
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
            y += hw + 12.0;
        }
        let g = bloc(&pc, &titre, gras(30.0), jt.texte, ld, 2);
        let hg = g.size().y;
        pc.galley(Pos2::new(droite.left(), y), g, jt.texte);
        y += hg + 6.0;
        // Ligne d'identité : pastille de plateforme, plateforme, région, sortie.
        let mut morceaux = vec![pl.nom().to_string()];
        if let Some(i) = &info {
            if !i.region.is_empty() {
                morceaux.push(i.region.clone());
            }
            let ds = date_sortie(i);
            if !ds.is_empty() {
                morceaux.push(ds);
            }
        }
        pc.circle_filled(Pos2::new(droite.left() + 5.0, y + 10.0), 4.5, couleur(pl));
        let g = bloc(
            &pc,
            &morceaux.join(", "),
            regulier(15.0),
            jt.doux,
            ld - 18.0,
            1,
        );
        pc.galley(Pos2::new(droite.left() + 18.0, y), g, jt.doux);
        y += 34.0;

        // Faits sur deux colonnes : intitulé discret au-dessus, valeur dessous.
        let mut faits: Vec<(&str, String)> = Vec::new();
        let ou = |a: &str, b: Option<&String>| -> String {
            if a.is_empty() {
                b.cloned().unwrap_or_default()
            } else {
                a.to_string()
            }
        };
        match &info {
            Some(i) => {
                faits.push(("Éditeur", ou(&i.editeur, ss.as_ref().map(|s| &s.editeur))));
                faits.push((
                    "Développeur",
                    ou(&i.developpeur, ss.as_ref().map(|s| &s.developpeur)),
                ));
                faits.push(("Genre", ou(&i.genre, ss.as_ref().map(|s| &s.genre))));
                let j = i.joueurs.map(|n| n.to_string()).unwrap_or_default();
                faits.push(("Joueurs", joueurs(&ou(&j, ss.as_ref().map(|s| &s.joueurs)))));
                if !i.esrb.is_empty() {
                    faits.push(("Classement ESRB", i.esrb.clone()));
                }
            }
            None => {
                if let Some(s) = &ss {
                    faits.push(("Éditeur", s.editeur.clone()));
                    faits.push(("Développeur", s.developpeur.clone()));
                    faits.push(("Genre", s.genre.clone()));
                    faits.push(("Sortie", s.date.clone()));
                }
                let fichier = rom.interne.clone().unwrap_or_else(|| {
                    rom.fichier
                        .file_name()
                        .map(|f| f.to_string_lossy().into_owned())
                        .unwrap_or_default()
                });
                faits.push(("Fichier", fichier));
                faits.push(("Taille", taille(rom.taille)));
                faits.push(("CRC32", format!("{:08X}", rom.crc)));
                if !rom.titre_entete.is_empty() {
                    faits.push(("Titre interne", rom.titre_entete.clone()));
                }
            }
        }
        if let Some(n) = ss.as_ref().and_then(|s| s.note) {
            faits.push(("Note ScreenScraper", format!("{n} / 20")));
        }
        faits.retain(|(_, v)| !v.is_empty());
        if info.is_none() && ss.is_none() {
            let g = bloc(
                &pc,
                "Ce fichier n'est pas dans la base No-Intro : voici ce que dit la ROM elle-même.",
                regulier(14.0),
                jt.doux,
                ld,
                2,
            );
            let gs = g.size();
            pc.galley(Pos2::new(droite.left(), y), g, jt.doux);
            y += gs.y + 16.0;
        }
        let lcol = (ld - 24.0) / 2.0;
        for (k, (lib, val)) in faits.iter().enumerate() {
            let x = droite.left() + (k % 2) as f32 * (lcol + 24.0);
            let yy = y + (k / 2) as f32 * 48.0;
            let g = ligne(&pc, lib, regulier(12.5), jt.discret);
            pc.galley(Pos2::new(x, yy), g, jt.discret);
            let g = bloc(&pc, val, moyen(15.5), jt.texte, lcol, 1);
            pc.galley(Pos2::new(x, yy + 17.0), g, jt.texte);
        }
        y += faits.len().div_ceil(2) as f32 * 48.0 + 8.0;

        // Actions en bas ; écrans et résumé se partagent la place entre les deux.
        let hb = 44.0;
        let yb = droite.bottom() - hb;
        // Résumé : Wikipédia en français, sinon ScreenScraper, sinon Wikipédia en anglais.
        let wiki = info.as_ref().filter(|i| !i.resume.is_empty()).map(|i| {
            (
                i.resume.clone(),
                i.source_resume.starts_with("fr:"),
                i.url_resume(),
            )
        });
        let synopsis = ss
            .as_ref()
            .map(|s| s.synopsis.clone())
            .filter(|s| !s.is_empty());
        let (resume, source) = match (wiki, synopsis) {
            (Some((w, true, url)), _) => (Some(w), Some(("Résumé : Wikipédia, CC BY-SA", url))),
            (_, Some(s)) => (
                Some(s),
                Some((
                    "Résumé : ScreenScraper",
                    Some("https://www.screenscraper.fr/".to_string()),
                )),
            ),
            (Some((w, false, url)), None) => (
                Some(w),
                Some(("Résumé : Wikipédia (en anglais), CC BY-SA", url)),
            ),
            (None, None) => (ss_erreur.map(|e| format!("ScreenScraper : {e}.")), None),
        };
        let place = yb - 20.0 - y;
        let h_ecrans_voulu = (place * 0.62).clamp(90.0, 230.0);
        if let Some(texte) = resume {
            let lignes = (((place - h_ecrans_voulu - 16.0) / 21.0).floor() as usize).clamp(1, 6);
            let g = bloc(&pc, &texte, regulier(14.5), jt.doux, ld.min(680.0), lignes);
            let gs = g.size();
            pc.galley(Pos2::new(droite.left(), y), g, jt.doux);
            y += gs.y + 6.0;
            if let Some((lib, url)) = source {
                // Attribution (lien vers l'article ou le site) : exigée par la licence CC BY-SA.
                let g = ligne(&pc, lib, regulier(12.0), jt.discret);
                let r = Rect::from_min_size(Pos2::new(droite.left(), y), g.size());
                let rep = ui
                    .interact(r, Id::unique("fiche-source"), Sense::CLICK)
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                let c = if rep.hovered() { jt.doux } else { jt.discret };
                pc.galley(r.min, g, c);
                if rep.hovered() {
                    pc.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, c));
                }
                if let (true, Some(u)) = (rep.clicked(), url) {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(u));
                }
                y += r.height();
            }
            y += 12.0;
        }
        let h_ecrans = (yb - 34.0 - y).min(230.0);
        // Jeu non reconnu : aucune vignette possible, pas de cadres vides.
        if h_ecrans >= 70.0 && dat.is_some() {
            let ratio = proportion(pl);
            let he = h_ecrans - 22.0;
            let le = (he * ratio).min((ld - 20.0) / 2.0);
            let he = le / ratio;
            let hors_ligne = crate::sauve::hors_ligne();
            for (k, (tex, lib)) in [(&tex_titre, "Écran titre"), (&tex_capture, "En jeu")]
                .into_iter()
                .enumerate()
            {
                let e = Rect::from_min_size(
                    Pos2::new(droite.left() + k as f32 * (le + 20.0), y),
                    Vec2::new(le, he),
                );
                // Cadre d'écran : verre sombre, image en pixels nets.
                pc.rect_filled(e.expand(6.0), CornerRadius::same(10), jt.fond_jeu);
                match tex {
                    Some(t) => {
                        pc.image(
                            t.id(),
                            e,
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                    None => {
                        let vide = if hors_ligne {
                            "Hors ligne"
                        } else {
                            "Pas d'image"
                        };
                        let g = ligne(&pc, vide, regulier(13.0), DISCRET);
                        pc.galley(e.center() - g.size() * 0.5, g, DISCRET);
                    }
                }
                let g = ligne(&pc, lib, regulier(12.5), jt.discret);
                pc.galley(Pos2::new(e.left(), e.bottom() + 10.0), g, jt.discret);
            }
        }

        // Boutons : Jouer en accent, les autres en pastilles discrètes.
        let mut x = droite.left();
        let mut clic = None;
        for (k, (lib, act)) in boutons.iter().enumerate() {
            let principal = *act == ActFiche::Jouer;
            let ic = match act {
                ActFiche::Jouer => Some(Icone::Jouer),
                ActFiche::Favori => Some(if jeu.favori {
                    Icone::Etoile
                } else {
                    Icone::EtoileVide
                }),
                ActFiche::Filtre(_) | ActFiche::Telecharger => None,
            };
            let g = ligne(&pc, lib, moyen(14.5), jt.texte);
            let lb = g.size().x + 32.0 + if ic.is_some() { 22.0 } else { 0.0 };
            if x + lb > droite.right() {
                break;
            }
            let rb = Rect::from_min_size(Pos2::new(x, yb), Vec2::new(lb, hb));
            let rep = ui
                .interact(rb, Id::unique(("fiche-bouton", k)), Sense::CLICK)
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            let choisi = k == bouton;
            let (fond_b, encre) = if principal {
                (
                    if choisi || rep.hovered() {
                        jt.accent_clair
                    } else {
                        jt.accent
                    },
                    jt.sur_accent,
                )
            } else if choisi || rep.hovered() {
                (jt.surface_haute, jt.texte)
            } else {
                (jt.surface, jt.doux)
            };
            panneau(
                &pc,
                rb,
                hb * 0.5,
                fond_b,
                Some(if principal { fond_b } else { jt.trait_ }),
            );
            if choisi {
                pc.rect_stroke(
                    rb.expand(3.0),
                    CornerRadius::same((hb * 0.5 + 3.0) as u8),
                    Stroke::new(2.0, alpha(jt.accent, 0.9)),
                    StrokeKind::Outside,
                );
            }
            let mut xt = rb.left() + 16.0;
            if let Some(ic) = ic {
                let c = if *act == ActFiche::Favori && jeu.favori {
                    jt.alerte
                } else {
                    encre
                };
                icone(&pc, ic, Pos2::new(xt + 7.0, rb.center().y), 15.0, c);
                xt += 22.0;
            }
            let g = ligne(&pc, lib, moyen(14.5), encre);
            pc.galley(Pos2::new(xt, rb.center().y - g.size().y * 0.5), g, encre);
            if rep.clicked() {
                clic = Some((k, *act));
            }
            x += lb + 10.0;
        }

        // Raccourcis de la fiche à la place de ceux de la bibliothèque (pied recouvert).
        let pied = Rect::from_min_max(
            Pos2::new(ecran.left(), ecran.bottom() - HAUT_PIED + 1.0),
            ecran.max,
        );
        let yp = pied.center().y;
        {
            p.rect_filled(pied, CornerRadius::ZERO, jt.fond);
            let manette = self.manettes.is_some() && self.dernier_pad.connectee;
            let puces: &[(&str, &str)] = if manette {
                &[
                    ("A", "Valider"),
                    ("Haut Bas", "Jeu voisin"),
                    ("X", "Favori"),
                    ("B", "Fermer"),
                ]
            } else {
                &[
                    ("Entrée", "Valider"),
                    ("Gauche Droite", "Bouton"),
                    ("Haut Bas", "Jeu voisin"),
                    ("F4", "Favori"),
                    ("Échap", "Fermer"),
                ]
            };
            let total: f32 = puces
                .iter()
                .map(|(t, a)| largeur_puce(&p, t, a) + 22.0)
                .sum::<f32>()
                - 22.0;
            let mut xp = ecran.center().x - total * 0.5;
            for (tch, a) in puces {
                xp += puce(&p, Pos2::new(xp, yp), tch, a) + 22.0;
            }
        }

        if let Some((k, act)) = clic {
            if let Some(pn) = &mut self.panneau {
                pn.bouton = k;
            }
            self.agir_fiche(act);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_joueurs_tailles() {
        let mut i = base_demo(&roms_demo()).jeux[&0].clone();
        i.annee = Some(1998);
        i.mois = Some(8);
        assert_eq!(date_sortie(&i), "août 1998");
        i.mois = Some(13);
        assert_eq!(date_sortie(&i), "1998");
        i.annee = None;
        assert_eq!(date_sortie(&i), "");
        assert_eq!(joueurs("1"), "1 joueur");
        assert_eq!(joueurs("1-4"), "1-4 joueurs");
        assert_eq!(joueurs(" "), "");
        assert_eq!(taille(32 * 1024), "32 Kio");
        assert_eq!(taille(1000), "1 Kio");
        assert_eq!(taille(4 << 20), "4 Mio");
    }

    #[test]
    fn valeurs_de_filtre() {
        let f = Fiche {
            rom: 0,
            titre: "X".into(),
            details: String::new(),
            dat: None,
            cle: String::new(),
            annee: Some(1999),
            editeur: "Capcom".into(),
            genre: String::new(),
        };
        assert_eq!(Champ::Annee.valeur(&f), "1999");
        assert_eq!(Champ::Editeur.valeur(&f), "Capcom");
        assert_eq!(Champ::Genre.valeur(&f), "");
    }
}
