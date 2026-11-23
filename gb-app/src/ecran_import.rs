//! Jeux ajoutés : « Télécharger les infos et images de N jeux ? » (Oui / Plus tard / Ne plus
//! demander), barre de progression discrète du téléchargement groupé dans le pied de la
//! bibliothèque (annulable), bouton « Télécharger les infos » de la fiche.

use super::*;
use crate::jaquettes;
use crate::sauve::NouveauxJeux;
use crate::sons_ui::Son;
use crate::telechargement::Tache;
use std::sync::mpsc;

pub(super) struct Proposition {
    taches: Vec<Tache>,
    /// Bouton choisi : 0 Oui, 1 Plus tard, 2 Ne plus demander.
    choix: usize,
    ouvert_a: f64,
}

const CHOIX: [&str; 3] = ["Oui", "Plus tard", "Ne plus demander"];

/// Touches ignorées juste après l'ouverture de la question (frappe de recherche en cours).
const GARDE_TOUCHES: f64 = 0.3;

/// CRC des jeux de `apres` absents de `avant` (jeux ajoutés depuis la dernière analyse).
pub(super) fn nouveaux(avant: &[Rom], apres: &[Rom]) -> Vec<u32> {
    let avant: HashSet<u32> = avant.iter().map(|r| r.crc).collect();
    apres
        .iter()
        .map(|r| r.crc)
        .filter(|c| !avant.contains(c))
        .collect()
}

impl App {
    /// Tâche de téléchargement d'un jeu (nom No-Intro s'il est reconnu).
    pub(super) fn tache(&self, rom: &Rom) -> Tache {
        let info = self.base.as_ref().and_then(|b| b.jeux.get(&rom.crc));
        let fichier = rom.interne.clone().unwrap_or_else(|| {
            rom.fichier
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
        Tache {
            crc: rom.crc,
            taille: rom.taille,
            plateforme: info.map_or(rom.plateforme, |i| i.plateforme),
            nom: info.map(|i| i.nom.clone()).unwrap_or_default(),
            fichier: fichier
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(&fichier)
                .into(),
            force: false,
        }
    }

    /// Nouveaux jeux et base prêts : cherche en fond ceux à qui il manque quelque chose, puis
    /// propose (ou lance, selon le réglage) leur téléchargement.
    fn preparer_proposition(&mut self) {
        let fini = match self.calcul_import.as_ref().map(|c| c.try_recv()) {
            Some(Ok(t)) => {
                self.calcul_import = None;
                Some(t)
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.calcul_import = None;
                None
            }
            _ => None,
        };
        if let Some(taches) = fini {
            self.proposer(taches);
        }
        if self.nouveaux.is_empty()
            || self.base.is_none()
            || self.proposition.is_some()
            || self.calcul_import.is_some()
        {
            return;
        }
        let nouveaux: HashSet<u32> = std::mem::take(&mut self.nouveaux).into_iter().collect();
        if self.config.nouveaux_jeux == NouveauxJeux::Jamais {
            return;
        }
        let ss = self.ss_id.actif();
        let mut vus = HashSet::new();
        let taches: Vec<Tache> = self
            .roms
            .iter()
            .filter(|r| nouveaux.contains(&r.crc) && vus.insert(r.crc))
            .map(|r| self.tache(r))
            .filter(|t| ss || !t.nom.is_empty())
            .collect();
        if taches.is_empty() {
            return;
        }
        if ss {
            self.proposer(taches);
            return;
        }
        let dossier = self.chemins.jaquettes();
        let (tx, rx) = mpsc::channel();
        let r = reveil(&self.ctx);
        std::thread::spawn(move || {
            let taches: Vec<Tache> = taches
                .into_iter()
                .filter(|t| {
                    [Vignette::Boite, Vignette::Titre, Vignette::Capture]
                        .into_iter()
                        .any(|v| jaquettes::manquante(&dossier, t.plateforme, &t.nom, v))
                })
                .collect();
            if tx.send(taches).is_ok() {
                r();
            }
        });
        self.calcul_import = Some(rx);
    }

    fn proposer(&mut self, taches: Vec<Tache>) {
        if taches.is_empty() {
            return;
        }
        match self.config.nouveaux_jeux {
            NouveauxJeux::Jamais => {}
            NouveauxJeux::Toujours => self.lancer_telechargement(taches),
            NouveauxJeux::Demander => {
                self.proposition = Some(Proposition {
                    taches,
                    choix: 0,
                    ouvert_a: self.t_prec,
                })
            }
        }
    }

    fn lancer_telechargement(&mut self, taches: Vec<Tache>) {
        let Some(t) = &self.tele else { return };
        let n = t.ajouter(taches);
        if n > 0 {
            self.dire(if crate::sauve::hors_ligne() {
                "Téléchargement en attente : mode hors ligne".to_string()
            } else if n == 1 {
                "Téléchargement des infos de 1 jeu".to_string()
            } else {
                format!("Téléchargement des infos de {n} jeux")
            });
        }
    }

    /// Bouton de la fiche : ce jeu seul, tout de suite (images notées absentes comprises).
    pub(super) fn telecharger_un(&mut self, rom: &Rom) {
        let tache = self.tache(rom);
        let Some(t) = &self.tele else { return };
        if tache.nom.is_empty() && !self.ss_id.actif() {
            self.dire("Jeu non reconnu : rien à télécharger sans ScreenScraper");
            self.son(Son::Erreur);
            return;
        }
        t.prioriser(tache);
        self.dire(if crate::sauve::hors_ligne() {
            "En attente : mode hors ligne"
        } else {
            "Téléchargement des infos…"
        });
        self.son(Son::Valider);
    }

    /// Jeux traités : vignettes et fiche relues depuis le cache (la fiche ouverte suit).
    pub(super) fn recevoir_telechargements(&mut self) {
        self.preparer_proposition();
        let Some(t) = &self.tele else { return };
        let mut fiche = false;
        while let Ok(f) = t.rx.try_recv() {
            if !f.nom.is_empty() {
                for v in [Vignette::Boite, Vignette::Titre, Vignette::Capture] {
                    let k = jaquettes::cle(v, &f.nom);
                    if !self.textures.contains_key(&k) {
                        self.demandees.remove(&k);
                    }
                }
            }
            if self.ss_fiches.get(&f.crc).is_some_and(|r| r.is_err()) {
                self.ss_fiches.remove(&f.crc);
            }
            fiche |= self.panneau.as_ref().is_some_and(|p| p.crc == f.crc);
        }
        if fiche {
            self.recharger_fiche();
        }
    }

    /// Capture de vérification : répond Oui.
    pub(super) fn repondre_proposition_oui(&mut self) {
        self.repondre(0);
    }

    fn repondre(&mut self, choix: usize) {
        let Some(p) = self.proposition.take() else {
            return;
        };
        match choix {
            0 => {
                self.son(Son::Valider);
                self.lancer_telechargement(p.taches);
            }
            2 => {
                self.config.nouveaux_jeux = NouveauxJeux::Jamais;
                self.config.enregistrer(&self.chemins.config());
                self.dire("Réglable dans Réglages > Métadonnées");
                self.son(Son::Retour);
            }
            _ => self.son(Son::Retour),
        }
    }

    pub(super) fn entrees_proposition(&mut self, evts: &[Event], pad: Etat) {
        if self
            .proposition
            .as_ref()
            .is_some_and(|p| self.t_prec - p.ouvert_a < GARDE_TOUCHES)
        {
            return;
        }
        let mut dx = 0i32;
        let mut valider = pad.appuis & b::SUD != 0;
        let mut plus_tard = pad.appuis & b::EST != 0;
        if pad.appuis & b::GAUCHE != 0 {
            dx -= 1;
        }
        if pad.appuis & b::DROITE != 0 {
            dx += 1;
        }
        for e in evts {
            if let Event::Key {
                key, pressed: true, ..
            } = e
            {
                match key {
                    Key::ArrowLeft => dx -= 1,
                    Key::ArrowRight | Key::Tab => dx += 1,
                    Key::Enter => valider = true,
                    Key::Escape | Key::Backspace => plus_tard = true,
                    _ => {}
                }
            }
        }
        if plus_tard {
            self.repondre(1);
            return;
        }
        let Some(p) = &mut self.proposition else {
            return;
        };
        let avant = p.choix;
        p.choix = (p.choix as i32 + dx).rem_euclid(CHOIX.len() as i32) as usize;
        let c = p.choix;
        if c != avant {
            self.son(Son::Deplacement);
        }
        if valider {
            self.repondre(c);
        }
    }

    pub(super) fn dessiner_proposition(&mut self, ui: &mut egui::Ui) {
        let Some(prop) = &self.proposition else {
            return;
        };
        bloquer_dessous(ui, "proposition");
        let jt = j();
        let n = prop.taches.len();
        let choix = prop.choix;
        let age = (self.t_prec - prop.ouvert_a) as f32;
        let t = d::sortie(age / d::duree_fondu(DUREE_MENU).max(0.001));
        let tm = d::ressort(age / d::duree(DUREE_MENU).max(0.001));
        if age < DUREE_MENU {
            ui.ctx().request_repaint();
        }
        let ecran = ui.max_rect();
        // Voile : bloque les clics vers la bibliothèque.
        let _ = ui.interact(ecran, Id::unique("import-voile"), Sense::CLICK);
        let mut p = ui.painter().clone();
        p.multiply_opacity(t);
        p.rect_filled(
            ecran,
            CornerRadius::ZERO,
            alpha(
                Color32::from_rgb(10, 11, 18),
                if jt.jour { 0.45 } else { 0.72 },
            ),
        );
        let l = 520.0f32.min(ecran.width() - 48.0);
        let h = 196.0;
        let r = Rect::from_center_size(ecran.center(), Vec2::new(l, h))
            .translate(Vec2::new(0.0, (1.0 - tm) * 14.0));
        d::ombre(&p, r, 18.0, 18.0, 48.0, 1.0);
        panneau(&p, r, 18.0, jt.surface, Some(jt.trait_));
        let titre = if n == 1 {
            "Télécharger les infos et images de 1 jeu ?".to_string()
        } else {
            format!("Télécharger les infos et images de {n} jeux ?")
        };
        let g = bloc(&p, &titre, gras(20.0), jt.texte, l - 56.0, 2);
        let gh = g.size().y;
        p.galley(Pos2::new(r.left() + 28.0, r.top() + 26.0), g, jt.texte);
        let detail = if self.ss_id.actif() {
            "Jaquettes, écrans titre et captures (libretro-thumbnails), fiches ScreenScraper. \
             En arrière-plan, annulable."
        } else {
            "Jaquettes, écrans titre et captures (libretro-thumbnails). En arrière-plan, \
             annulable. Les fiches et résumés sont déjà inclus."
        };
        let g = bloc(&p, detail, regulier(13.5), jt.doux, l - 56.0, 3);
        p.galley(Pos2::new(r.left() + 28.0, r.top() + 34.0 + gh), g, jt.doux);

        let hb = 42.0;
        let yb = r.bottom() - 26.0 - hb;
        let mut x = r.left() + 28.0;
        let mut clic = None;
        for (k, lib) in CHOIX.iter().enumerate() {
            let g = ligne(&p, lib, moyen(14.5), jt.texte);
            let lb = g.size().x + 36.0;
            let rb = Rect::from_min_size(Pos2::new(x, yb), Vec2::new(lb, hb));
            let rep = ui
                .interact(rb, Id::unique(("import-bouton", k)), Sense::CLICK)
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            let choisi = k == choix;
            let (fond, encre) = if k == 0 {
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
                &p,
                rb,
                hb * 0.5,
                fond,
                Some(if k == 0 { fond } else { jt.trait_ }),
            );
            if choisi {
                p.rect_stroke(
                    rb.expand(3.0),
                    CornerRadius::same((hb * 0.5 + 3.0) as u8),
                    Stroke::new(2.0, alpha(jt.accent, 0.9)),
                    StrokeKind::Outside,
                );
            }
            let g = ligne(&p, lib, moyen(14.5), encre);
            p.galley(rb.center() - g.size() * 0.5, g, encre);
            if rep.clicked() {
                clic = Some(k);
            }
            x += lb + 10.0;
        }
        if let Some(k) = clic {
            self.repondre(k);
        }
    }

    /// Téléchargement en cours, dans le pied (à gauche de `droite`) : texte, barre fine et
    /// croix d'annulation. Rend le bord gauche de la pastille, `None` si rien n'est en cours.
    pub(super) fn dessiner_telechargement(
        &mut self,
        ui: &egui::Ui,
        p: &egui::Painter,
        droite: f32,
        y: f32,
    ) -> Option<f32> {
        let (faits, total) = self.tele.as_ref().and_then(|t| t.progres())?;
        let jt = j();
        let texte = if crate::sauve::hors_ligne() {
            format!("Infos et images : en pause (hors ligne), {faits} / {total}")
        } else {
            format!("Infos et images {faits} / {total}")
        };
        let g = ligne(p, &texte, regulier(12.5), jt.doux);
        let gs = g.size();
        let lbarre = 120.0;
        let r = Rect::from_min_max(
            Pos2::new(droite - gs.x - lbarre - 64.0, y - 14.0),
            Pos2::new(droite, y + 14.0),
        );
        panneau(p, r, 14.0, jt.surface, Some(jt.trait_));
        p.galley(Pos2::new(r.left() + 14.0, y - gs.y * 0.5), g, jt.doux);
        let b = Rect::from_min_size(
            Pos2::new(r.left() + 24.0 + gs.x, y - 2.0),
            Vec2::new(lbarre, 4.0),
        );
        p.rect_filled(b, CornerRadius::same(2), jt.surface_haute);
        let f = (faits as f32 / total.max(1) as f32).clamp(0.0, 1.0);
        p.rect_filled(
            Rect::from_min_size(b.min, Vec2::new(lbarre * f, 4.0)),
            CornerRadius::same(2),
            jt.accent,
        );
        let cc = Pos2::new(r.right() - 17.0, y);
        let rc = Rect::from_center_size(cc, Vec2::splat(24.0));
        let rep = ui
            .interact(rc, Id::unique("import-annuler"), Sense::CLICK)
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Annuler le téléchargement");
        let tr = Stroke::new(1.6, if rep.hovered() { jt.texte } else { jt.doux });
        for s in [-1.0, 1.0] {
            p.line_segment(
                [cc + Vec2::new(-4.0, -4.0 * s), cc + Vec2::new(4.0, 4.0 * s)],
                tr,
            );
        }
        if rep.clicked() {
            if let Some(t) = &self.tele {
                t.annuler();
            }
            self.dire("Téléchargement annulé");
        }
        // Pas de repeint périodique : le fil réveille l'interface à chaque jeu traité.
        Some(r.left())
    }
}
