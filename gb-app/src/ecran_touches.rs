//! Écran des touches : console dessinée cliquable, profils clavier / manettes, capture de
//! la prochaine touche ou du prochain bouton, « Tout configurer », conflits, raccourcis de
//! l'appli (table [`Action`]). Fournit aussi la lecture des raccourcis pour les autres écrans.

use super::*;
use crate::console;
use crate::manette::{famille, Famille, Pad};
use crate::touches::{entree_accord, Action, Bouton, Cible, Clavier, Entree, Profil, Touches};
use gilrs::Button;

// Jetons de la palette « boîtier de nuit » (cahier §1.1) propres à cet écran.
const HALO: Color32 = Color32::from_rgb(0xF0, 0x7A, 0xAE);
const ORANGE: Color32 = Color32::from_rgb(0xE8, 0xA2, 0x3A);
const VERT: Color32 = Color32::from_rgb(0x5B, 0xBF, 0x8A);

/// Délai d'attente d'une touche à attribuer.
const DELAI_CAPTURE: f64 = 5.0;
const HAUT_LIGNE: f32 = 44.0;

#[derive(Clone, Copy, PartialEq)]
enum Section {
    Console,
    Raccourcis,
}

#[derive(Clone, PartialEq)]
enum Onglet {
    Clavier,
    ManetteDefaut,
    Manette { uuid: String, nom: String },
}

struct Capture {
    cible: Cible,
    debut: f64,
    /// Faux pendant l'image qui a lancé la capture (sa touche ne compte pas).
    armee: bool,
    /// Raccourci manette : bouton appuyé seul, en attente d'un second (accord) ou du relâché.
    candidat: Option<(usize, Button)>,
}

#[derive(Clone, Copy)]
enum Clic {
    Cible(Cible),
    Onglet(usize),
    Section(Section),
    Tout,
    Reinit,
    Effacer,
    Fermer,
}

pub(super) struct EcranTouches {
    onglet: usize,
    section: Section,
    sel: usize,
    capture: Option<Capture>,
    tout: bool,
    clignote: Option<(Cible, f64)>,
    pastille: Option<(Bouton, String, f64)>,
    /// Fin de la fenêtre de confirmation de « Réinitialiser ».
    confirmer: f64,
    defil: f32,
    retour: Option<GenreMenu>,
    /// Boutons allumés en démonstration (captures).
    demo_allumes: u8,
}

fn bits_console(gb: u8) -> u8 {
    Bouton::TOUS
        .iter()
        .filter(|b| gb & b.bit() != 0)
        .fold(0, |v, &b| v | 1 << b as usize)
}

impl App {
    // ----- Lecture des raccourcis (point d'accroche des autres écrans) -----

    fn pads(&self) -> &[Pad] {
        self.manettes.as_ref().map_or(&[], |m| m.pads())
    }

    /// Raccourci appuyé à cette image (clavier ou manette, selon les profils).
    pub(super) fn action_appuyee(&self, a: Action) -> bool {
        self.ecran_touches.is_none() && self.touches.appuyee(a, &self.clavier, self.pads())
    }

    /// Raccourci tenu (avance rapide, rembobinage…).
    pub(super) fn action_tenue(&self, a: Action) -> bool {
        self.ecran_touches.is_none() && self.touches.tenue(a, &self.clavier, self.pads())
    }

    /// Boutons Game Boy tenus (bits `gb_core::bouton`).
    pub(super) fn boutons_jeu(&self, maintenant: f64) -> u8 {
        self.touches.boutons(&self.clavier, self.pads(), maintenant)
    }

    /// À chaque image : clavier de l'image, manettes nouvellement branchées.
    pub(super) fn suivre_entrees(&mut self, ui: &egui::Ui) {
        self.clavier = ui.input(Clavier::lire);
        let Some(m) = &mut self.manettes else { return };
        let branchees = m.branchees();
        // Les manettes déjà là au lancement arrivent aussi comme « branchées » : pas de toast.
        if self.images_ui < 120 {
            return;
        }
        for (uuid, nom) in branchees {
            if !self.touches.manettes.iter().any(|p| p.uuid == uuid) {
                self.dire(format!(
                    "Nouvelle manette « {nom} » : Réglages ▸ Touches pour la configurer"
                ));
            }
        }
    }

    // ----- Ouverture, fermeture -----

    pub(super) fn ouvrir_touches(&mut self, retour: Option<GenreMenu>) {
        // La manette branchée en dernier est mise en avant.
        let onglet = match self.pads().last() {
            Some(p) => self
                .onglets()
                .iter()
                .position(|o| matches!(o, Onglet::Manette { uuid, .. } if *uuid == p.uuid))
                .unwrap_or(0),
            None => 0,
        };
        self.ecran_touches = Some(EcranTouches {
            onglet,
            section: Section::Console,
            sel: 0,
            capture: None,
            tout: false,
            clignote: None,
            pastille: None,
            confirmer: 0.0,
            defil: 0.0,
            retour,
            demo_allumes: 0,
        });
    }

    /// Ouverture pour les captures : `--menu touches[-raccourcis|-capture|-allume|-tout]`.
    pub(super) fn ouvrir_touches_demo(&mut self, variante: &str) {
        self.ouvrir_touches(None);
        let t = self.t_prec;
        let Some(e) = &mut self.ecran_touches else {
            return;
        };
        match variante {
            "-raccourcis" => {
                e.section = Section::Raccourcis;
                e.sel = 4;
            }
            "-capture" => {
                e.sel = Bouton::A as usize;
                e.capture = Some(Capture {
                    cible: Cible::Bouton(Bouton::A),
                    debut: t + 1e6,
                    armee: true,
                    candidat: None,
                });
            }
            "-allume" => {
                e.demo_allumes = bits_console(
                    gb_core::bouton::A | gb_core::bouton::GAUCHE | gb_core::bouton::START,
                );
            }
            "-conflit" => {
                e.sel = Bouton::B as usize;
                e.clignote = Some((Cible::Bouton(Bouton::A), t + 1e6));
                e.pastille = Some((Bouton::B, "X".into(), t + 1e6));
            }
            _ => {}
        }
    }

    fn fermer_touches(&mut self) {
        let retour = self.ecran_touches.take().and_then(|e| e.retour);
        if let Some(g) = retour {
            self.ouvrir_menu(g);
        }
    }

    // ----- Profils -----

    fn onglets(&self) -> Vec<Onglet> {
        let mut v = vec![Onglet::Clavier, Onglet::ManetteDefaut];
        let mut ajouter = |uuid: &str, nom: &str| {
            if !v
                .iter()
                .any(|o| matches!(o, Onglet::Manette { uuid: u, .. } if u == uuid))
            {
                v.push(Onglet::Manette {
                    uuid: uuid.into(),
                    nom: nom.into(),
                });
            }
        };
        for p in self.pads() {
            ajouter(&p.uuid, &p.nom);
        }
        for p in &self.touches.manettes {
            ajouter(&p.uuid, &p.nom);
        }
        v
    }

    fn onglet_courant(&self) -> Onglet {
        let v = self.onglets();
        let i = self.ecran_touches.as_ref().map_or(0, |e| e.onglet);
        v.get(i).cloned().unwrap_or(Onglet::Clavier)
    }

    fn profil_de(&self, o: &Onglet) -> &Profil {
        match o {
            Onglet::Clavier => &self.touches.clavier,
            Onglet::ManetteDefaut => &self.touches.manette_defaut,
            Onglet::Manette { uuid, .. } => self.touches.profil_pad(uuid),
        }
    }

    fn profil_de_mut(&mut self, o: &Onglet) -> &mut Profil {
        match o {
            Onglet::Clavier => &mut self.touches.clavier,
            Onglet::ManetteDefaut => &mut self.touches.manette_defaut,
            Onglet::Manette { uuid, nom } => self.touches.profil_pad_mut(uuid, nom),
        }
    }

    /// Manettes que lit l'onglet (celles du modèle ; sinon toutes).
    fn pads_onglet(&self, o: &Onglet) -> Vec<&Pad> {
        let tous = self.pads();
        match o {
            Onglet::Clavier => Vec::new(),
            Onglet::ManetteDefaut => tous.iter().collect(),
            Onglet::Manette { uuid, .. } => {
                let v: Vec<&Pad> = tous.iter().filter(|p| p.uuid == *uuid).collect();
                if v.is_empty() {
                    tous.iter().collect()
                } else {
                    v
                }
            }
        }
    }

    fn famille_onglet(o: &Onglet) -> Famille {
        match o {
            Onglet::Manette { nom, .. } => famille(nom),
            _ => Famille::Generique,
        }
    }

    fn enregistrer_touches(&mut self) {
        if self
            .touches
            .enregistrer(&Touches::chemin(&self.chemins.donnees))
            .is_err()
        {
            self.dire("Écriture de touches.json impossible : vérifiez le dossier gbemu-donnees");
        }
    }

    fn lignes(section: Section) -> Vec<Cible> {
        match section {
            Section::Console => Bouton::TOUS.iter().map(|&b| Cible::Bouton(b)).collect(),
            Section::Raccourcis => Action::toutes().into_iter().map(Cible::Action).collect(),
        }
    }

    // ----- Actions de l'écran -----

    fn commencer_capture(&mut self, cible: Cible) {
        let t = self.t_prec;
        if let Some(e) = &mut self.ecran_touches {
            e.capture = Some(Capture {
                cible,
                debut: t,
                armee: false,
                candidat: None,
            });
        }
    }

    fn cliquer(&mut self, c: Clic) {
        let t = self.t_prec;
        let Some(e) = &mut self.ecran_touches else {
            return;
        };
        match c {
            Clic::Cible(cible) => {
                if let Some(i) = Self::lignes(e.section).iter().position(|x| *x == cible) {
                    e.sel = i;
                } else if let Cible::Bouton(b) = cible {
                    e.section = Section::Console;
                    e.sel = b as usize;
                }
                e.tout = false;
                self.commencer_capture(cible);
            }
            Clic::Onglet(i) => {
                e.onglet = i;
                e.capture = None;
                e.tout = false;
                e.confirmer = 0.0;
            }
            Clic::Section(s) => {
                if e.section != s {
                    e.section = s;
                    e.sel = 0;
                    e.defil = 0.0;
                    e.capture = None;
                    e.tout = false;
                }
            }
            Clic::Tout => {
                e.section = Section::Console;
                e.sel = 0;
                e.tout = true;
                self.commencer_capture(Cible::Bouton(Bouton::TOUS[0]));
            }
            Clic::Reinit => {
                if t < e.confirmer {
                    e.confirmer = 0.0;
                    e.capture = None;
                    let o = self.onglet_courant();
                    *self.profil_de_mut(&o) = match o {
                        Onglet::Clavier => Profil::clavier(),
                        _ => Profil::manette(),
                    };
                    self.enregistrer_touches();
                    self.dire("Profil réinitialisé");
                } else {
                    e.confirmer = t + 3.0;
                }
            }
            Clic::Effacer => {
                let cible = Self::lignes(e.section).get(e.sel).copied();
                e.capture = None;
                e.tout = false;
                if let Some(c) = cible {
                    let o = self.onglet_courant();
                    self.profil_de_mut(&o).effacer(c);
                    self.enregistrer_touches();
                }
            }
            Clic::Fermer => self.fermer_touches(),
        }
    }

    fn attribuer(&mut self, cible: Cible, entree: Entree) {
        let t = self.t_prec;
        let o = self.onglet_courant();
        let f = Self::famille_onglet(&o);
        let perdu = self.profil_de_mut(&o).attribuer(cible, entree);
        let lib = entree.libelle(f);
        self.enregistrer_touches();
        if let Some(p) = perdu {
            self.dire(format!("{lib} retirée de {}", p.nom()));
        }
        let Some(e) = &mut self.ecran_touches else {
            return;
        };
        e.capture = None;
        if let Some(p) = perdu {
            e.clignote = Some((p, t + 1.0));
        }
        if let Cible::Bouton(b) = cible {
            e.pastille = Some((b, lib, t + 1.4));
            if e.tout {
                let i = b as usize + 1;
                if i < Bouton::TOUS.len() {
                    e.sel = i;
                    self.commencer_capture(Cible::Bouton(Bouton::TOUS[i]));
                } else {
                    e.tout = false;
                    self.dire("Toutes les touches sont configurées");
                }
            }
        }
    }

    /// Cherche l'entrée à attribuer pendant une capture.
    fn capter(&mut self) -> Option<Entree> {
        let o = self.onglet_courant();
        let e = self.ecran_touches.as_ref()?;
        let cap = e.capture.as_ref()?;
        let action = matches!(cap.cible, Cible::Action(_));
        if o == Onglet::Clavier {
            let (k, md) = *self.clavier.appuis.first()?;
            return Some(Entree::Touche {
                touche: k,
                maj: action && md.shift,
                ctrl: action && md.command,
            });
        }
        let candidat = cap.candidat;
        let mut nouveau_candidat = None;
        let mut trouve = None;
        for p in self.pads_onglet(&o) {
            if let Some((axe, positif)) = p.premier_axe() {
                trouve = Some(Entree::Axe { axe, positif });
                break;
            }
            match (action, candidat) {
                (false, _) => {
                    if let Some(b) = p.premier_appui() {
                        trouve = Some(Entree::Bouton {
                            bouton: b,
                            avec: None,
                        });
                        break;
                    }
                }
                (true, Some((id, c))) if id == p.id => {
                    if let Some(b) = p.premier_appui().filter(|&b| b != c && p.tenu(c)) {
                        trouve = Some(entree_accord(c, b));
                        break;
                    }
                    if !p.tenu(c) {
                        trouve = Some(Entree::Bouton {
                            bouton: c,
                            avec: None,
                        });
                        break;
                    }
                }
                (true, _) => {
                    if let Some(b) = p.premier_appui() {
                        match p.autres_tenus(b).next() {
                            Some(autre) => {
                                trouve = Some(entree_accord(autre, b));
                                break;
                            }
                            None => nouveau_candidat = Some((p.id, b)),
                        }
                    }
                }
            }
        }
        if let (None, Some(c)) = (trouve, nouveau_candidat) {
            if let Some(cap) = self.ecran_touches.as_mut().and_then(|e| e.capture.as_mut()) {
                cap.candidat = Some(c);
            }
        }
        trouve
    }

    // ----- Entrées -----

    pub(super) fn entrees_touches(&mut self, pad: Etat) {
        let t = self.t_prec;
        let Some(e) = &mut self.ecran_touches else {
            return;
        };
        if let Some(cap) = &mut e.capture {
            if !cap.armee {
                cap.armee = true;
                return;
            }
            let echap = self.clavier.appuis.iter().any(|(k, _)| *k == Key::Escape);
            if echap || t - cap.debut > DELAI_CAPTURE {
                e.capture = None;
                e.tout = false;
                return;
            }
            let cible = cap.cible;
            if let Some(entree) = self.capter() {
                self.attribuer(cible, entree);
            }
            return;
        }

        let n = Self::lignes(e.section).len() as i32;
        let mut dy = 0;
        let mut donglet = 0;
        let mut clic = None;
        let appui = |bit: u16| pad.appuis & bit != 0;
        if appui(b::HAUT) {
            dy -= 1;
        }
        if appui(b::BAS) {
            dy += 1;
        }
        if appui(b::GAUCHE) || appui(b::L1) {
            donglet -= 1;
        }
        if appui(b::DROITE) || appui(b::R1) {
            donglet += 1;
        }
        let autre_section = match e.section {
            Section::Console => Section::Raccourcis,
            Section::Raccourcis => Section::Console,
        };
        let cible = Self::lignes(e.section).get(e.sel).copied();
        if appui(b::NORD) {
            clic = Some(Clic::Section(autre_section));
        }
        if appui(b::SUD | b::OUEST) {
            clic = cible.map(Clic::Cible);
        }
        if appui(b::SELECT) {
            clic = Some(Clic::Effacer);
        }
        if appui(b::START) {
            clic = Some(Clic::Tout);
        }
        if appui(b::EST) {
            clic = Some(Clic::Fermer);
        }
        for (k, _) in &self.clavier.appuis {
            match k {
                Key::ArrowUp => dy -= 1,
                Key::ArrowDown => dy += 1,
                Key::ArrowLeft => donglet -= 1,
                Key::ArrowRight => donglet += 1,
                Key::Tab => clic = Some(Clic::Section(autre_section)),
                Key::Enter | Key::Space => clic = cible.map(Clic::Cible),
                Key::Delete | Key::Backspace => clic = Some(Clic::Effacer),
                Key::T => clic = Some(Clic::Tout),
                Key::Escape => clic = Some(Clic::Fermer),
                _ => {}
            }
        }
        let no = self.onglets().len() as i32;
        let Some(e) = &mut self.ecran_touches else {
            return;
        };
        if n > 0 {
            e.sel = (e.sel as i32 + dy).rem_euclid(n) as usize;
        }
        if donglet != 0 {
            e.onglet = (e.onglet as i32 + donglet).rem_euclid(no) as usize;
            e.confirmer = 0.0;
        }
        if let Some(c) = clic {
            self.cliquer(c);
        }
    }

    // ----- Dessin -----

    pub(super) fn dessiner_touches(&mut self, ui: &mut egui::Ui) {
        let ecran = ui.max_rect();
        let p = ui.painter().clone();
        let t = self.t_prec;
        let onglets = self.onglets();
        let o = self.onglet_courant();
        let fam = Self::famille_onglet(&o);
        let mut clics: Vec<Clic> = Vec::new();
        let Some(e) = &self.ecran_touches else {
            return;
        };
        if e.onglet >= onglets.len() {
            // Manette débranchée : retour au clavier.
            if let Some(e) = &mut self.ecran_touches {
                e.onglet = 0;
            }
            return;
        }
        let capture = e.capture.as_ref().map(|c| (c.cible, c.debut));
        let profil = self.profil_de(&o);

        p.rect_filled(ecran, CornerRadius::ZERO, FOND);
        let zone_clic = |r: Rect, id: &str, c: Clic, clics: &mut Vec<Clic>| {
            if ui
                .interact(r, Id::unique(("touches", id)), Sense::click())
                .clicked()
            {
                clics.push(c);
            }
        };

        // En-tête : titre et profils.
        let x0 = ecran.left() + MARGE;
        p.text(
            Pos2::new(x0, ecran.top() + 26.0),
            egui::Align2::LEFT_TOP,
            "Touches",
            gras(30.0),
            TEXTE,
        );
        p.text(
            Pos2::new(x0, ecran.top() + 66.0),
            egui::Align2::LEFT_TOP,
            "Cliquez un bouton de la console, puis appuyez sur la touche ou le bouton voulu.",
            regulier(14.0),
            DOUX,
        );
        let mut x = ecran.right() - MARGE;
        let pads_branches: Vec<String> = self.pads().iter().map(|p| p.uuid.clone()).collect();
        for (i, og) in onglets.iter().enumerate().rev() {
            let (nom, branche) = match og {
                Onglet::Clavier => ("Clavier".to_string(), false),
                Onglet::ManetteDefaut => ("Manette (défaut)".to_string(), false),
                Onglet::Manette { uuid, nom } => {
                    let court: String = nom.chars().take(22).collect();
                    (court, pads_branches.contains(uuid))
                }
            };
            let g = ligne(&p, &nom, moyen(14.0), TEXTE);
            let l = g.size().x + 28.0 + if branche { 14.0 } else { 0.0 };
            let r = Rect::from_min_size(Pos2::new(x - l, ecran.top() + 28.0), Vec2::new(l, 34.0));
            let actif = i == e.onglet;
            p.rect_filled(
                r,
                CornerRadius::same(17),
                if actif { ACCENT } else { SURFACE },
            );
            if !actif {
                p.rect_stroke(
                    r,
                    CornerRadius::same(17),
                    Stroke::new(1.0, TRAIT),
                    StrokeKind::Inside,
                );
            }
            let mut tx = r.left() + 14.0;
            if branche {
                p.circle_filled(Pos2::new(tx + 4.0, r.center().y), 4.0, VERT);
                tx += 14.0;
            }
            let couleur = if actif { TEXTE } else { DOUX };
            p.galley(Pos2::new(tx, r.center().y - g.size().y * 0.5), g, couleur);
            zone_clic(r, &format!("onglet{i}"), Clic::Onglet(i), &mut clics);
            x = r.left() - 10.0;
            if x < ecran.center().x {
                break;
            }
        }

        // Console à gauche, liste à droite.
        let haut = ecran.top() + 104.0;
        let bas = ecran.bottom() - 132.0;
        let zone_console = Rect::from_min_max(
            Pos2::new(x0, haut),
            Pos2::new(ecran.left() + ecran.width() * 0.46, bas),
        );
        let cadre = console::cadre(zone_console);
        // Halo doux derrière la console.
        for i in 0..14 {
            let f = i as f32 / 14.0;
            p.circle_filled(
                cadre.center(),
                cadre.height() * (0.75 - 0.4 * f),
                alpha(SURFACE_HAUTE, 0.05 + 0.02 * f),
            );
        }
        let mut allumes = e.demo_allumes;
        if capture.is_none() {
            let pads = self.pads_onglet(&o);
            if o == Onglet::Clavier {
                allumes |= bits_console(profil.boutons_tenus(&self.clavier, None, t));
            }
            for pad in pads {
                allumes |= bits_console(profil.boutons_tenus(&Clavier::default(), Some(pad), t));
            }
        }
        let bouton_de = |c: Option<Cible>| match c {
            Some(Cible::Bouton(b)) => Some(b),
            _ => None,
        };
        let clignote = e.clignote.filter(|c| t < c.1).map(|c| c.0);
        let vue = console::Vue {
            allumes,
            // Bouton en attente d'une touche, sinon ligne choisie (parcours au clavier, à la manette).
            choisi: bouton_de(capture.map(|c| c.0)).or(match e.section {
                Section::Console => bouton_de(Self::lignes(e.section).get(e.sel).copied()),
                Section::Raccourcis => None,
            }),
            clignote: bouton_de(clignote),
            temps: t,
            ecran: None,
        };
        let zones = console::dessiner(&p, zone_console, &vue);
        for (i, r) in zones.iter().enumerate() {
            zone_clic(
                *r,
                &format!("console{i}"),
                Clic::Cible(Cible::Bouton(Bouton::TOUS[i])),
                &mut clics,
            );
        }
        // Pastille de la touche tout juste attribuée.
        if let Some((b, lib, fin)) = &e.pastille {
            if t < *fin {
                let c = console::centre(zone_console, *b);
                let mut pp = p.clone();
                pp.multiply_opacity(((*fin - t) as f32 / 0.3).min(1.0));
                let g = ligne(&p, lib, moyen(13.0), TEXTE);
                let r = Rect::from_center_size(
                    c - Vec2::new(0.0, 30.0),
                    Vec2::new(g.size().x + 16.0, 26.0),
                );
                pp.rect_filled(r, CornerRadius::same(8), SURFACE_HAUTE);
                pp.rect_stroke(
                    r,
                    CornerRadius::same(8),
                    Stroke::new(1.0, HALO),
                    StrokeKind::Inside,
                );
                pp.galley(r.center() - g.size() * 0.5, g, TEXTE);
            }
        }
        // Bulle d'attente à côté du bouton choisi.
        if let Some((Cible::Bouton(b), debut)) = capture {
            let c = console::centre(zone_console, b);
            let texte = "Appuyez sur une touche ou un bouton…";
            let g = ligne(&p, texte, moyen(14.0), TEXTE);
            let g2 = ligne(&p, "Échap pour annuler", regulier(12.5), DOUX);
            let l = g.size().x.max(g2.size().x) + 28.0;
            let mut r = Rect::from_min_size(c + Vec2::new(-l * 0.5, -122.0), Vec2::new(l, 58.0));
            let dx = (cadre.left() + 4.0 - r.left()).max(0.0)
                + (ecran.center().x - 8.0 - r.right()).min(0.0);
            r = r.translate(Vec2::new(dx, 0.0));
            p.rect_filled(
                r.translate(Vec2::new(0.0, 6.0)),
                CornerRadius::same(12),
                alpha(Color32::BLACK, 0.3),
            );
            p.rect_filled(r, CornerRadius::same(12), SURFACE_HAUTE);
            p.rect_stroke(
                r,
                CornerRadius::same(12),
                Stroke::new(1.5, HALO),
                StrokeKind::Inside,
            );
            let pointe = Pos2::new(c.x.clamp(r.left() + 16.0, r.right() - 16.0), r.bottom());
            p.add(egui::Shape::convex_polygon(
                vec![
                    pointe + Vec2::new(-8.0, -1.0),
                    pointe + Vec2::new(8.0, -1.0),
                    pointe + Vec2::new(0.0, 9.0),
                ],
                SURFACE_HAUTE,
                Stroke::NONE,
            ));
            p.galley(Pos2::new(r.left() + 14.0, r.top() + 9.0), g, TEXTE);
            p.galley(Pos2::new(r.left() + 14.0, r.top() + 32.0), g2, DOUX);
            let reste = (1.0 - (t - debut) / DELAI_CAPTURE).clamp(0.0, 1.0) as f32;
            let barre = Rect::from_min_size(
                Pos2::new(r.left() + 12.0, r.bottom() - 5.0),
                Vec2::new((r.width() - 24.0) * reste, 2.0),
            );
            p.rect_filled(barre, CornerRadius::same(1), HALO);
        }

        // Onglets de section.
        let xd = ecran.left() + ecran.width() * 0.52;
        let xf = ecran.right() - MARGE;
        let mut xs = xd;
        for (s, nom) in [
            (Section::Console, "Console"),
            (Section::Raccourcis, "Raccourcis"),
        ] {
            let g = ligne(&p, nom, moyen(15.0), TEXTE);
            let r = Rect::from_min_size(Pos2::new(xs, haut), Vec2::new(g.size().x + 8.0, 32.0));
            let actif = e.section == s;
            p.galley(
                Pos2::new(r.left() + 4.0, r.center().y - g.size().y * 0.5),
                g,
                if actif { TEXTE } else { DISCRET },
            );
            if actif {
                p.rect_filled(
                    Rect::from_min_size(
                        Pos2::new(r.left() + 4.0, r.bottom() - 2.0),
                        Vec2::new(r.width() - 8.0, 3.0),
                    ),
                    CornerRadius::same(1),
                    ACCENT,
                );
            }
            zone_clic(r, nom, Clic::Section(s), &mut clics);
            xs = r.right() + 18.0;
        }

        // Lignes.
        let liste = Rect::from_min_max(Pos2::new(xd, haut + 46.0), Pos2::new(xf, bas));
        let lignes = Self::lignes(e.section);
        let total = lignes.len() as f32 * HAUT_LIGNE;
        let max_defil = (total - liste.height()).max(0.0);
        let mut defil = e.defil;
        let ys = e.sel as f32 * HAUT_LIGNE;
        if ys < defil {
            defil = ys;
        }
        if ys + HAUT_LIGNE > defil + liste.height() {
            defil = ys + HAUT_LIGNE - liste.height();
        }
        if ui.rect_contains_pointer(liste) {
            defil -= ui.input(|i| i.smooth_scroll_delta.y);
        }
        defil = defil.clamp(0.0, max_defil);
        let pl = p.clone().with_clip_rect(liste.expand2(Vec2::new(4.0, 0.0)));
        for (k, cible) in lignes.iter().enumerate() {
            let y = liste.top() + k as f32 * HAUT_LIGNE - defil;
            let r = Rect::from_min_size(Pos2::new(xd, y), Vec2::new(xf - xd, HAUT_LIGNE - 4.0));
            if r.bottom() < liste.top() || r.top() > liste.bottom() {
                continue;
            }
            let choisie = k == e.sel;
            let en_capture = capture.is_some_and(|c| c.0 == *cible);
            let clig = clignote == Some(*cible) && (t * 8.0).sin() > 0.0;
            if choisie || en_capture {
                pl.rect_filled(r, CornerRadius::same(10), SURFACE_HAUTE);
                pl.rect_filled(
                    Rect::from_min_size(
                        r.left_top() + Vec2::new(0.0, 8.0),
                        Vec2::new(3.0, r.height() - 16.0),
                    ),
                    CornerRadius::same(2),
                    ACCENT,
                );
            }
            if clig {
                pl.rect_stroke(
                    r,
                    CornerRadius::same(10),
                    Stroke::new(1.5, ORANGE),
                    StrokeKind::Inside,
                );
            }
            let nom = match cible {
                Cible::Bouton(b) => b.nom().to_uppercase(),
                Cible::Action(a) => a.nom(),
            };
            let police = match cible {
                Cible::Bouton(_) => gras(14.5),
                Cible::Action(_) => regulier(14.5),
            };
            let g = ligne(&p, &nom, police, TEXTE);
            pl.galley(
                Pos2::new(r.left() + 16.0, r.center().y - g.size().y * 0.5),
                g,
                if choisie { TEXTE } else { DOUX },
            );
            let mut cx = r.right() - 12.0;
            if en_capture {
                let a = 0.55 + 0.45 * (t * 5.0).sin() as f32;
                let g = ligne(&p, "Appuyez sur une touche…", moyen(13.5), HALO);
                pl.galley(
                    Pos2::new(cx - g.size().x, r.center().y - g.size().y * 0.5),
                    g,
                    alpha(HALO, a),
                );
            } else {
                let entrees = profil.entrees(*cible);
                if entrees.is_empty() {
                    let g = ligne(&p, "—", regulier(14.0), DISCRET);
                    pl.galley(
                        Pos2::new(cx - g.size().x, r.center().y - g.size().y * 0.5),
                        g,
                        DISCRET,
                    );
                }
                for en in entrees.iter().rev() {
                    let g = ligne(&p, &en.libelle(fam), moyen(13.0), TEXTE);
                    let pr = Rect::from_min_size(
                        Pos2::new(cx - g.size().x - 16.0, r.center().y - 12.0),
                        Vec2::new(g.size().x + 16.0, 24.0),
                    );
                    pl.rect_filled(pr, CornerRadius::same(6), SURFACE);
                    pl.rect_stroke(
                        pr,
                        CornerRadius::same(6),
                        Stroke::new(1.0, TRAIT),
                        StrokeKind::Inside,
                    );
                    pl.galley(
                        Pos2::new(pr.left() + 8.0, pr.center().y - g.size().y * 0.5),
                        g,
                        TEXTE,
                    );
                    cx = pr.left() - 8.0;
                }
            }
            let visible = r.intersect(liste);
            if visible.height() > 4.0 {
                zone_clic(
                    visible,
                    &format!("ligne{k}"),
                    Clic::Cible(*cible),
                    &mut clics,
                );
            }
        }
        if max_defil > 0.0 {
            // Ascenseur discret.
            let h = liste.height() * liste.height() / total;
            let y = liste.top() + (liste.height() - h) * defil / max_defil;
            p.rect_filled(
                Rect::from_min_size(Pos2::new(xf + 8.0, y), Vec2::new(3.0, h)),
                CornerRadius::same(2),
                alpha(DOUX, 0.4),
            );
        }

        // Boutons du bas.
        let reinit = if t < e.confirmer {
            "Confirmer la réinitialisation ?"
        } else {
            "Réinitialiser ce profil"
        };
        let mut xb = x0;
        let yb = ecran.bottom() - 112.0;
        for (i, (nom, c)) in [
            ("Tout configurer", Clic::Tout),
            (reinit, Clic::Reinit),
            ("Effacer la touche", Clic::Effacer),
            ("Fermer", Clic::Fermer),
        ]
        .into_iter()
        .enumerate()
        {
            let g = ligne(&p, nom, moyen(14.5), TEXTE);
            let r = Rect::from_min_size(Pos2::new(xb, yb), Vec2::new(g.size().x + 32.0, 40.0));
            let survol = ui.rect_contains_pointer(r);
            let (fond, bord) = match (i, t < e.confirmer) {
                (0, _) => (ACCENT, ACCENT),
                (1, true) => (alpha(ORANGE, 0.25), ORANGE),
                _ => (if survol { SURFACE_HAUTE } else { SURFACE }, TRAIT),
            };
            p.rect_filled(r, CornerRadius::same(20), fond);
            p.rect_stroke(
                r,
                CornerRadius::same(20),
                Stroke::new(1.0, bord),
                StrokeKind::Inside,
            );
            p.galley(r.center() - g.size() * 0.5, g, TEXTE);
            zone_clic(r, nom, c, &mut clics);
            xb = r.right() + 12.0;
        }

        // Aide en pied.
        let y = ecran.bottom() - 34.0;
        let aides: &[(&str, &str)] = if self.dernier_pad.connectee {
            &[
                ("Sud", "Attribuer"),
                ("Nord", "Raccourcis"),
                ("Start", "Tout configurer"),
                ("Select", "Effacer"),
                ("Est", "Fermer"),
            ]
        } else {
            &[
                ("Entrée", "Attribuer"),
                ("Gauche Droite", "Profil"),
                ("Tab", "Raccourcis"),
                ("T", "Tout configurer"),
                ("Suppr", "Effacer"),
                ("Échap", "Fermer"),
            ]
        };
        let mut xa = x0;
        for (k, a) in aides {
            xa += puce(&p, Pos2::new(xa, y), k, a) + 22.0;
        }

        if let Some(e) = &mut self.ecran_touches {
            e.defil = defil;
        }
        for c in clics {
            self.cliquer(c);
        }
        ui.ctx().request_repaint();
    }
}
