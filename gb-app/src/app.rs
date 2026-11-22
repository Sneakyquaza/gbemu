//! Application : bibliothèque (grille de cartouches/jaquettes), partie en cours, menus.
//! Tout se pilote au clavier, à la manette ou à la souris ; le travail lourd (analyse,
//! métadonnées, jaquettes) tourne sur d'autres fils, l'interface ne bloque jamais.

use crate::audio::Sortie;
use crate::biblio::{self, MsgScan, Plateforme, Rom};
use crate::dat::{self, Base};
use crate::dessin::{self as d, *};
use crate::jaquettes::Jaquettes;
use crate::jeu::{Partie, AVANCE};
use crate::manette::{b, Etat, Manettes};
use crate::sauve::{Chemins, Config, Palette};
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

#[derive(Clone, Copy, PartialEq)]
enum GenreMenu {
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

struct Menu {
    genre: GenreMenu,
    sel: usize,
    /// Saisie en cours du chemin du dossier des jeux.
    edition: Option<String>,
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
            let rom = Rom {
                interne: None,
                taille: 0,
                crc: std::fs::read(&chemin).map_or(0, |d| crc32fast::hash(&d)),
                plateforme: Plateforme::Gb,
                titre_entete: String::new(),
                fichier: chemin,
            };
            let titre = rom.nom_fichier();
            let ext = rom
                .fichier
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase());
            if matches!(ext.as_deref(), Some("gbc" | "gba")) {
                app.dire("Game Boy Color et Advance : bientôt pris en charge");
            } else {
                app.lancer_partie(rom, titre);
            }
        }
        if app.opts.grille {
            app.config.grille = true;
        }
        if app.opts.menu.as_deref() == Some("reglages") {
            app.ouvrir_menu(GenreMenu::Reglages);
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

    fn dire(&mut self, texte: impl Into<String>) {
        self.toast = Some((texte.into(), self.t_prec + 3.2));
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
        if !self.vue.is_empty() {
            self.sel = self.sel.min(self.vue.len() - 1);
        }
    }

    fn changer_recherche(&mut self) {
        self.filtrer();
        self.sel = 0;
        self.defil_cible = 0.0;
    }

    fn changer_filtre(&mut self, sens: i32) {
        let ordre = [
            None,
            Some(Plateforme::Gb),
            Some(Plateforme::Gbc),
            Some(Plateforme::Gba),
        ];
        let i = ordre.iter().position(|f| *f == self.filtre).unwrap_or(0) as i32;
        self.filtre = ordre[(i + sens).rem_euclid(4) as usize];
        self.changer_recherche();
    }

    // ----- Parties -----

    fn lancer_selection(&mut self) {
        let Some(&i) = self.vue.get(self.sel) else {
            return;
        };
        let f = &self.fiches[i];
        let rom = self.roms[f.rom].clone();
        if !rom.plateforme.jouable() {
            let m = format!("{} : bientôt pris en charge", rom.plateforme.nom());
            self.dire(m);
            return;
        }
        let titre = f.titre.clone();
        self.lancer_partie(rom, titre);
    }

    fn lancer_partie(&mut self, rom: Rom, titre: String) {
        let sav = self.chemins.sauvegarde(&rom);
        match Partie::lancer(rom, titre, sav) {
            Ok(p) => {
                self.partie = Some(p);
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
        // La sauvegarde batterie est écrite par `Drop`.
        self.partie = None;
        self.menu = None;
        if let Some(a) = &self.audio {
            a.vider();
        }
    }

    // ----- Menus -----

    fn ouvrir_menu(&mut self, genre: GenreMenu) {
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

    fn basculer_plein_ecran(&self) {
        let plein = self.ctx.input(|i| i.viewport().fullscreen).unwrap_or(false);
        self.ctx
            .send_viewport_cmd(ViewportCommand::Fullscreen(!plein));
    }

    // ----- Entrées -----

    fn entrees_menu(&mut self, evts: &[Event], pad: Etat) {
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

    fn entrees_biblio(&mut self, evts: &[Event], pad: Etat) {
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

    // ----- Dessin : bibliothèque -----

    fn dessiner_biblio(&mut self, ui: &mut egui::Ui, dt: f32) {
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

    // ----- Dessin : jeu -----

    fn dessiner_jeu(&mut self, ui: &mut egui::Ui, rapide: bool) {
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

    // ----- Dessin : menus -----

    fn dessiner_menu(&mut self, ui: &mut egui::Ui) {
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

    fn dessiner_toast(&self, ui: &egui::Ui) {
        let Some((texte, fin)) = &self.toast else {
            return;
        };
        let reste = (*fin - self.t_prec) as f32;
        if reste <= 0.0 {
            return;
        }
        let a = reste.min(0.3) / 0.3;
        let ecran = ui.max_rect();
        let mut p = ui.painter().clone();
        p.multiply_opacity(a);
        let g = ligne(&p, texte, moyen(15.0), TEXTE);
        let gs = g.size();
        let b = Rect::from_center_size(
            Pos2::new(
                ecran.center().x,
                ecran.bottom() - HAUT_PIED - 34.0 + (1.0 - a) * 8.0,
            ),
            Vec2::new(gs.x + 36.0, 42.0),
        );
        p.add(
            egui::epaint::Shadow {
                offset: [0, 8],
                blur: 24,
                spread: 0,
                color: Color32::from_black_alpha(90),
            }
            .as_shape(b, CornerRadius::same(21)),
        );
        p.rect_filled(b, CornerRadius::same(21), SURFACE_HAUTE);
        p.rect_stroke(
            b,
            CornerRadius::same(21),
            Stroke::new(1.0, TRAIT),
            StrokeKind::Inside,
        );
        p.galley(
            Pos2::new(b.left() + 18.0, b.center().y - gs.y * 0.5),
            g,
            TEXTE,
        );
        ui.ctx().request_repaint();
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
            .is_none_or(|p| p.images >= self.opts.images);
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
        if ui.input(|i| i.key_pressed(Key::F11)) {
            self.basculer_plein_ecran();
        }
        if pad.appuis != 0 {
            ctx.request_repaint();
        }

        let rapide = if self.menu.is_some() {
            self.entrees_menu(&evts, pad);
            false
        } else if self.partie.is_some() {
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
        } else {
            self.entrees_biblio(&evts, pad);
            false
        };

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
        self.dessiner_toast(ui);
        self.dessiner_depot(ui);
        if self.manettes.is_some() {
            // Les manettes se lisent par sondage : on repasse régulièrement.
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        self.capture(&ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(p) = &mut self.partie {
            p.sauver();
        }
    }

    fn clear_color(&self, _v: &egui::Visuals) -> [f32; 4] {
        FOND.to_normalized_gamma_f32()
    }
}
