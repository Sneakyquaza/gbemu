//! Application : bibliothèque (grille de cartouches/jaquettes), partie en cours, menus.
//! Tout se pilote au clavier, à la manette ou à la souris ; le travail lourd (analyse,
//! métadonnées, jaquettes) tourne sur d'autres fils, l'interface ne bloque jamais.

use crate::audio::Sortie;
use crate::biblio::{self, MsgScan, Plateforme, Rom};
use crate::dat::{self, Base};
use crate::dessin::{self as d, *};
use crate::jaquettes::Jaquettes;
use crate::jeu::Partie;
use crate::manette::{b, Etat, Manettes};
use crate::sauve::{Chemins, Config, Palette};
use crate::touches::{Action, Clavier, Touches};
use eframe::egui::{
    self, Color32, ColorImage, CornerRadius, Event, Id, Key, Pos2, Rect, Sense, Stroke, StrokeKind,
    TextureHandle, TextureOptions, Vec2, ViewportCommand,
};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::Instant;

#[path = "ecran_biblio.rs"]
mod ecran_biblio;
#[path = "ecran_jeu.rs"]
mod ecran_jeu;
#[path = "ecran_touches.rs"]
mod ecran_touches;
#[path = "menus.rs"]
mod menus;
#[path = "toast.rs"]
mod toast;

use menus::{GenreMenu, Menu};

pub struct Options {
    pub debut: Instant,
    pub jeux: Option<PathBuf>,
    pub donnees: Option<PathBuf>,
    pub demo: bool,
    pub rom: Option<PathBuf>,
    pub capture: Option<PathBuf>,
    pub images: u64,
    pub menu: Option<String>,
    pub recherche: Option<String>,
    pub selection: usize,
    pub grille: bool,
}

/// Ce qu'affiche une carte (titre, détails, clé de recherche), recalculé quand la base arrive.
struct Fiche {
    rom: usize,
    titre: String,
    details: String,
    dat: Option<(Plateforme, String)>,
    cle: String,
}

const MARGE: f32 = 36.0;
const HAUT_ENTETE: f32 = 84.0;
const HAUT_PIED: f32 = 52.0;
const ECART_X: f32 = 22.0;
const ECART_Y: f32 = 26.0;
const TEXTE_CARTE: f32 = 58.0;
const LARGEUR_MIN_CARTE: f32 = 172.0;

pub struct App {
    opts: Options,
    chemins: Chemins,
    config: Config,
    ctx: egui::Context,

    roms: Vec<Rom>,
    fiches: Vec<Fiche>,
    vue: Vec<usize>,
    vue_sale: bool,
    base: Option<Base>,
    rx_base: Option<Receiver<Base>>,
    rx_scan: Option<Receiver<MsgScan>>,
    progres: Option<(usize, usize)>,
    analyse_finie: bool,
    t_analyse: Instant,

    jaquettes: Option<Jaquettes>,
    textures: HashMap<String, TextureHandle>,
    demandees: HashSet<String>,

    recherche: String,
    filtre: Option<Plateforme>,
    sel: usize,
    colonnes: usize,
    defil: f32,
    defil_cible: f32,
    vue_h: f32,
    ligne_h: f32,

    partie: Option<Partie>,
    debut_partie: f64,
    tex_ecran: Option<TextureHandle>,
    menu: Option<Menu>,
    toast: Option<(String, f64)>,
    manettes: Option<Manettes>,
    audio: Option<Sortie>,

    dernier_pad: Etat,
    images_menu: u64,
    attendre_relache: bool,
    images_ui: u64,
    t_prec: f64,
    capture_demandee: bool,
    capture_faite: Arc<AtomicBool>,
    hab: ecran_biblio::Habillage,
    touches: Touches,
    clavier: Clavier,
    ecran_touches: Option<ecran_touches::EcranTouches>,
    hud: ecran_jeu::Hud,
}

fn reveil(ctx: &egui::Context) -> impl Fn() + Send + Sync + 'static {
    let c = ctx.clone();
    move || c.request_repaint()
}

/// Minuscules sans accents, pour la recherche.
fn plier(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' | 'í' => 'i',
            'ô' | 'ö' | 'ó' | 'õ' => 'o',
            'ù' | 'û' | 'ü' | 'ú' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            _ => c,
        })
        .collect()
}

/// Titre d'en-tête rendu lisible : « POKEMON_RED » → « Pokemon Red ».
fn embellir(t: &str) -> String {
    let t = t.replace('_', " ");
    if t.chars().any(|c| c.is_lowercase()) {
        return t;
    }
    t.split(' ')
        .filter(|m| !m.is_empty())
        .map(|m| {
            let mut c = m.chars();
            match c.next() {
                Some(p) => p
                    .to_uppercase()
                    .chain(c.flat_map(char::to_lowercase))
                    .collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<String>>()
        .join(" ")
}

/// Bibliothèque fictive pour juger la grille (aucun fichier derrière).
fn roms_demo() -> Vec<Rom> {
    let titres = [
        ("Lanterne des Marais", Plateforme::Gb),
        ("Kobold Kart", Plateforme::Gb),
        ("Orbite Basse", Plateforme::Gbc),
        ("Le Phare de Brume", Plateforme::Gb),
        ("Cuisine Quantique", Plateforme::Gba),
        ("Mineurs de Givre", Plateforme::Gb),
        ("Pixel Pêcheur", Plateforme::Gbc),
        ("Sentinelle d'Ambre", Plateforme::Gba),
        ("Tortues Tonnerre", Plateforme::Gb),
        ("Rails du Désert", Plateforme::Gb),
        ("Jardin de Néon", Plateforme::Gbc),
        ("Chroniques du Hibou", Plateforme::Gba),
        ("Poste Restante", Plateforme::Gb),
        ("Grand Prix Galet", Plateforme::Gb),
        ("Volcan Vert", Plateforme::Gbc),
        ("Le Dernier Tramway", Plateforme::Gba),
        ("Fourmis de Fer", Plateforme::Gb),
        ("Carillon", Plateforme::Gb),
        (
            "Mystères de la Lune Pâle : l'Archipel Oublié",
            Plateforme::Gbc,
        ),
        ("Boulanger Ninja", Plateforme::Gb),
        ("Spectre Sport", Plateforme::Gba),
        ("Cerf-volant", Plateforme::Gb),
        ("Hélice", Plateforme::Gbc),
        ("Donjon Douillet", Plateforme::Gb),
    ];
    titres
        .iter()
        .enumerate()
        .map(|(i, (t, p))| Rom {
            fichier: PathBuf::from(format!("demo/{t}.zip")),
            interne: Some(format!("{t}.{}", p.court().to_lowercase())),
            taille: 0,
            crc: i as u32,
            plateforme: *p,
            titre_entete: t.to_string(),
        })
        .collect()
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, opts: Options) -> App {
        d::installer(&cc.egui_ctx);
        let ctx = cc.egui_ctx.clone();
        let chemins = Chemins::trouver(opts.donnees.clone());
        let mut config = Config::charger(&chemins.config());
        d::appliquer_theme(&ctx, config.jour);
        d::reduire_mouvement(config.animations_reduites);
        let hab = ecran_biblio::Habillage::new(&opts, &chemins, &config);
        if let Some(j) = &opts.jeux {
            config.dossier_jeux = Some(std::path::absolute(j).unwrap_or_else(|_| j.clone()));
        }

        let mut app = App {
            recherche: opts.recherche.clone().unwrap_or_default(),
            sel: opts.selection,
            opts,
            chemins,
            config,
            ctx: ctx.clone(),
            roms: Vec::new(),
            fiches: Vec::new(),
            vue: Vec::new(),
            vue_sale: true,
            base: None,
            rx_base: None,
            rx_scan: None,
            progres: None,
            analyse_finie: false,
            t_analyse: Instant::now(),
            jaquettes: None,
            textures: HashMap::new(),
            demandees: HashSet::new(),
            filtre: None,
            colonnes: 1,
            defil: 0.0,
            defil_cible: 0.0,
            vue_h: 0.0,
            ligne_h: 1.0,
            partie: None,
            debut_partie: 0.0,
            tex_ecran: None,
            menu: None,
            toast: None,
            manettes: None,
            audio: None,
            dernier_pad: Etat::default(),
            images_menu: 0,
            attendre_relache: false,
            images_ui: 0,
            t_prec: 0.0,
            capture_demandee: false,
            capture_faite: Arc::new(AtomicBool::new(false)),
            hab,
            touches: Touches::default(),
            clavier: Clavier::default(),
            ecran_touches: None,
            hud: Default::default(),
        };

        if app.opts.demo {
            app.roms = roms_demo();
            app.analyse_finie = true;
        } else {
            let _ = std::fs::create_dir_all(app.dossier_jeux());
            app.lancer_analyse(true);
        }
        let (tx, rx) = channel();
        let dossier_dat = app.chemins.dat();
        let r = reveil(&ctx);
        std::thread::spawn(move || {
            let _ = tx.send(dat::charger(&dossier_dat));
            r();
        });
        app.rx_base = Some(rx);
        app.jaquettes = Some(Jaquettes::lancer(app.chemins.jaquettes(), reveil(&ctx)));

        if let Some(chemin) = app.opts.rom.clone() {
            let donnees = std::fs::read(&chemin).unwrap_or_default();
            let ext = chemin
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            let rom = Rom {
                interne: None,
                taille: 0,
                crc: crc32fast::hash(&donnees),
                plateforme: biblio::plateforme(&ext, &donnees),
                titre_entete: String::new(),
                fichier: chemin,
            };
            let titre = rom.nom_fichier();
            app.lancer_partie(rom, titre);
        }
        if app.opts.grille {
            app.config.grille = true;
        }
        if app.opts.menu.as_deref() == Some("reglages") {
            app.ouvrir_menu(GenreMenu::Reglages);
        }
        app.touches = Touches::charger(&Touches::chemin(&app.chemins.donnees));
        if let Some(v) = app
            .opts
            .menu
            .clone()
            .and_then(|m| m.strip_prefix("touches").map(String::from))
        {
            app.ouvrir_touches_demo(&v);
        }
        app
    }

    fn dossier_jeux(&self) -> PathBuf {
        self.config
            .dossier_jeux
            .clone()
            .unwrap_or_else(|| self.chemins.jeux_defaut.clone())
    }

    fn lancer_analyse(&mut self, envoyer_cache: bool) {
        let mut sources = vec![self.dossier_jeux()];
        sources.extend(self.config.ajouts.iter().cloned());
        let (tx, rx) = channel();
        let index = self.chemins.index();
        let r = reveil(&self.ctx);
        std::thread::spawn(move || biblio::scanner(sources, index, envoyer_cache, tx, r));
        self.rx_scan = Some(rx);
        self.t_analyse = Instant::now();
        self.analyse_finie = false;
        self.progres = None;
    }

    // ----- Réception des fils d'arrière-plan -----

    fn recevoir(&mut self) {
        if let Some(rx) = &self.rx_scan {
            while let Ok(m) = rx.try_recv() {
                match m {
                    MsgScan::Cache(r) => {
                        self.roms = r;
                        self.vue_sale = true;
                    }
                    MsgScan::Progres { faits, total } => self.progres = Some((faits, total)),
                    MsgScan::Fini(r) => {
                        eprintln!(
                            "analyse : {} ROM en {} ms",
                            r.len(),
                            self.t_analyse.elapsed().as_millis()
                        );
                        if r != self.roms {
                            self.roms = r;
                            self.vue_sale = true;
                        }
                        self.analyse_finie = true;
                        self.progres = None;
                    }
                }
            }
        }
        if let Some(rx) = &self.rx_base {
            if let Ok(base) = rx.try_recv() {
                self.base = Some(base);
                self.vue_sale = true;
                self.rx_base = None;
            }
        }
        if let Some(j) = &self.jaquettes {
            while let Ok((nom, img)) = j.rx.try_recv() {
                if let Some(img) = img {
                    self.hab
                        .teintes
                        .insert(nom.clone(), ecran_biblio::teinte_moyenne(&img.rgba));
                    let ci =
                        ColorImage::from_rgba_unmultiplied([img.largeur, img.hauteur], &img.rgba);
                    let t = self.ctx.load_texture(&nom, ci, TextureOptions::LINEAR);
                    self.textures.insert(nom, t);
                }
            }
        }
    }

    fn reconstruire(&mut self) {
        if !self.vue_sale {
            return;
        }
        self.vue_sale = false;
        let choisi = self.vue.get(self.sel).map(|&i| self.fiches[i].rom);
        let choisi = choisi.and_then(|i| self.roms.get(i).cloned());
        let mut fiches: Vec<Fiche> = self
            .roms
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let info = self.base.as_ref().and_then(|b| b.jeux.get(&r.crc));
                let fichier = match &r.interne {
                    Some(n) => n.rsplit(['/', '\\']).next().unwrap_or(n).to_string(),
                    None => r
                        .fichier
                        .file_name()
                        .map(|f| f.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                };
                let (titre, details, dat) = match info {
                    Some(j) => {
                        let mut det = j.region.clone();
                        if let Some(a) = j.annee {
                            if !det.is_empty() {
                                det.push_str(", ");
                            }
                            det.push_str(&a.to_string());
                        }
                        (j.titre.clone(), det, Some((j.plateforme, j.nom.clone())))
                    }
                    None => {
                        let t = if r.titre_entete.is_empty() {
                            r.nom_fichier()
                        } else {
                            embellir(&r.titre_entete)
                        };
                        (t, fichier.clone(), None)
                    }
                };
                // Démo : quelques cartes prennent une jaquette locale pour juger le rendu.
                let dat = if self.opts.demo && i % 6 == 4 {
                    Some((r.plateforme, "Jaquette de démonstration".to_string()))
                } else {
                    dat
                };
                let cle = plier(&format!("{titre} {fichier}"));
                Fiche {
                    rom: i,
                    titre,
                    details,
                    dat,
                    cle,
                }
            })
            .collect();
        fiches.sort_by_cached_key(|f| (plier(&f.titre), f.details.clone()));
        self.fiches = fiches;
        self.filtrer();
        if let Some(c) = choisi {
            if let Some(k) = self
                .vue
                .iter()
                .position(|&i| self.roms[self.fiches[i].rom] == c)
            {
                self.sel = k;
            }
        }
    }

    fn filtrer(&mut self) {
        let termes: Vec<String> = plier(&self.recherche)
            .split_whitespace()
            .map(str::to_string)
            .collect();
        self.vue = self
            .fiches
            .iter()
            .enumerate()
            .filter(|(_, f)| {
                self.filtre.is_none_or(|p| self.roms[f.rom].plateforme == p)
                    && termes.iter().all(|t| f.cle.contains(t.as_str()))
            })
            .map(|(i, _)| i)
            .collect();
        self.affiner_vue();
        if !self.vue.is_empty() {
            self.sel = self.sel.min(self.vue.len() - 1);
        }
    }

    fn changer_recherche(&mut self) {
        self.filtrer();
        self.sel = 0;
        self.defil_cible = 0.0;
    }

    // ----- Parties -----

    fn lancer_selection(&mut self) {
        let Some(&i) = self.vue.get(self.sel) else {
            return;
        };
        let f = &self.fiches[i];
        let rom = self.roms[f.rom].clone();
        let titre = f.titre.clone();
        self.lancer_partie(rom, titre);
    }

    fn lancer_partie(&mut self, rom: Rom, titre: String) {
        let sav = self.chemins.sauvegarde(&rom);
        let crc = rom.crc;
        let etats = self.chemins.etats(&rom);
        // Vérification visuelle : pas de carte de reprise, sauf si c'est elle qu'on capture.
        let reprise = self.opts.capture.is_none() || self.opts.menu.as_deref() == Some("reprise");
        match Partie::lancer(rom, titre, sav, etats, reprise) {
            Ok(mut p) => {
                p.gb.set_color_correction(!self.config.sans_correction);
                self.hab.stats.lancer(crc);
                self.fin_jeu();
                self.partie = Some(p);
                self.debut_jeu();
                self.debut_partie = self.t_prec;
                self.attendre_relache = true;
                self.menu = None;
                if let Some(a) = &self.audio {
                    a.vider();
                }
            }
            Err(e) => self.dire(e),
        }
    }

    fn quitter_partie(&mut self) {
        // Sauvegarde batterie et état « auto » écrits par `fin_jeu` (et `Drop` en secours).
        self.fin_jeu();
        self.partie = None;
        self.hab.stats.arreter();
        self.menu = None;
        if let Some(a) = &self.audio {
            a.vider();
        }
    }

    // ----- Entrées -----

    fn deposes(&mut self, chemins: Vec<PathBuf>) {
        let jeux = self.dossier_jeux();
        let mut n = 0;
        for c in chemins {
            let deja = c.starts_with(&jeux) || self.config.ajouts.iter().any(|a| c.starts_with(a));
            if !deja {
                self.config.ajouts.push(c);
                n += 1;
            }
        }
        if n > 0 {
            self.config.enregistrer(&self.chemins.config());
            self.lancer_analyse(false);
            self.dire(if n == 1 {
                "Ajouté à la bibliothèque".to_string()
            } else {
                format!("{n} éléments ajoutés à la bibliothèque")
            });
        } else {
            self.dire("Déjà dans la bibliothèque");
        }
    }

    fn dessiner_depot(&self, ui: &egui::Ui) {
        let survol = ui.input(|i| !i.raw.hovered_files.is_empty());
        if !survol {
            return;
        }
        let ecran = ui.max_rect();
        let p = ui.painter();
        p.rect_filled(ecran, CornerRadius::ZERO, alpha(FOND, 0.86));
        let r = ecran.shrink(28.0);
        p.rect_stroke(
            r,
            CornerRadius::same(22),
            Stroke::new(2.0, alpha(ACCENT, 0.8)),
            StrokeKind::Inside,
        );
        let g = ligne(
            p,
            "Déposez pour ajouter à la bibliothèque",
            moyen(22.0),
            TEXTE,
        );
        let gs = g.size();
        p.galley(ecran.center() - gs * 0.5, g, TEXTE);
    }

    // ----- Capture (vérification visuelle) -----

    fn capture(&mut self, ctx: &egui::Context) {
        let Some(chemin) = self.opts.capture.clone() else {
            return;
        };
        if self.capture_faite.load(Ordering::Relaxed) {
            ctx.send_viewport_cmd(ViewportCommand::Close);
            return;
        }
        ctx.request_repaint();
        let jeu_pret = self
            .partie
            .as_ref()
            .is_none_or(|p| p.images >= self.opts.images || p.reprise.is_some());
        self.preparer_capture(jeu_pret);
        // Menu pause : ouvert une fois l'image du jeu stable, capturé après son animation.
        if jeu_pret && self.partie.is_some() && self.opts.menu.as_deref() == Some("pause") {
            self.opts.menu = None;
            self.ouvrir_menu(GenreMenu::Pause);
            self.images_menu = self.images_ui;
        }
        let pret = self.images_ui >= self.opts.images
            && self.analyse_finie
            && self.opts.menu.as_deref() != Some("pause")
            && (jeu_pret || self.menu.is_some())
            && self.images_ui >= self.images_menu + 15;
        if pret && !self.capture_demandee {
            self.capture_demandee = true;
            let fait = Arc::clone(&self.capture_faite);
            let c = ctx.clone();
            ctx.request_screenshot(move |img| {
                if let Err(e) = ecrire_png(&chemin, &img) {
                    eprintln!("capture : {e}");
                }
                fait.store(true, Ordering::Relaxed);
                c.request_repaint();
            });
        }
    }
}

fn ecrire_png(chemin: &std::path::Path, img: &ColorImage) -> std::io::Result<()> {
    let [l, h] = img.size;
    let mut octets = Vec::with_capacity(l * h * 4);
    for c in &img.pixels {
        octets.extend_from_slice(&c.to_array());
    }
    if let Some(p) = chemin.parent() {
        std::fs::create_dir_all(p)?;
    }
    let f = std::io::BufWriter::new(std::fs::File::create(chemin)?);
    let mut enc = png::Encoder::new(f, l as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(std::io::Error::other)?;
    w.write_image_data(&octets).map_err(std::io::Error::other)?;
    Ok(())
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let maintenant = ui.input(|i| i.time);
        let dt = ((maintenant - self.t_prec) as f32).clamp(0.0, 0.1);
        self.t_prec = maintenant;
        if self.images_ui == 1 {
            eprintln!(
                "démarrage : {} ms jusqu'à la première image affichée",
                self.opts.debut.elapsed().as_millis()
            );
        }
        // Manettes et son ouverts après la première image : le démarrage reste instantané.
        if self.images_ui == 2 {
            self.manettes = Some(Manettes::ouvrir());
            self.audio = Sortie::ouvrir();
        }
        self.images_ui += 1;

        self.recevoir();
        self.reconstruire();

        let pad = self
            .manettes
            .as_mut()
            .map(|m| m.lire(maintenant))
            .unwrap_or_default();
        self.dernier_pad = pad;
        self.suivre_entrees(ui);
        let evts: Vec<Event> = ui.input(|i| i.events.clone());
        let deposes: Vec<PathBuf> = ui.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !deposes.is_empty() {
            self.deposes(deposes);
        }
        if self.action_appuyee(Action::PleinEcran) {
            self.basculer_plein_ecran();
        }
        if pad.appuis != 0 {
            ctx.request_repaint();
        }

        let rapide = if self.ecran_touches.is_some() {
            self.entrees_touches(pad);
            false
        } else if self.menu.is_some() {
            self.entrees_menu(&evts, pad);
            false
        } else if self.partie.is_some() {
            self.entrees_jeu(ui, pad, maintenant)
        } else {
            self.entrees_biblio(&evts, pad);
            false
        };

        if self.partie.is_some() && self.menu.is_none() {
            self.hab.stats.compter(dt, maintenant);
        }
        if self.partie.is_some() {
            self.dessiner_jeu(ui, rapide);
            if self.menu.is_none() {
                ctx.request_repaint();
            }
        } else {
            self.dessiner_biblio(ui, dt);
        }
        if self.menu.is_some() {
            self.dessiner_menu(ui);
        }
        if self.ecran_touches.is_some() {
            self.dessiner_touches(ui);
        }
        self.dessiner_toast(ui);
        self.dessiner_depot(ui);
        self.dessiner_intro(ui, &evts, pad);
        if self.manettes.is_some() {
            // Les manettes se lisent par sondage : on repasse régulièrement.
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        self.capture(&ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.fin_jeu();
        self.hab.stats.arreter();
    }

    fn clear_color(&self, _v: &egui::Visuals) -> [f32; 4] {
        FOND.to_normalized_gamma_f32()
    }
}
