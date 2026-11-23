//! gbemu : application de bureau (bibliothèque de jeux + lecture Game Boy).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod biblio;
mod console;
mod dat;
mod dessin;
mod etats;
mod intro;
mod jaquettes;
mod jeu;
mod machine;
mod manette;
mod rembobinage;
mod robot;
mod sauve;
mod screenscraper;
mod sons_ui;
mod stats;
mod telechargement;
mod touches;

use std::io::Write as _;
use std::path::PathBuf;
use std::time::{Instant, SystemTime};

const AIDE: &str = "gbemu [--jeux DOSSIER] [--donnees DOSSIER] [--rom FICHIER.gb|.gbc|.gba]
  Vérification visuelle : [--demo] [--capture SORTIE.png] [--images N] [--taille LxH]
  [--menu pause|reglages|metadonnees|fiche|touches[-raccourcis|-capture|-allume|-conflit]] [--grille] [--recherche TEXTE] [--selection N]";

fn main() -> eframe::Result {
    let debut = Instant::now();
    let tous: Vec<String> = std::env::args().skip(1).collect();
    if tous.iter().any(|a| a == "--robot") {
        std::process::exit(robot::principal(&tous));
    }
    let mut opts = app::Options {
        debut,
        jeux: None,
        donnees: None,
        demo: false,
        rom: None,
        capture: None,
        images: 30,
        menu: None,
        recherche: None,
        selection: 0,
        grille: false,
        robot: false,
    };
    let mut taille = [1280.0f32, 800.0];
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = || args.next().unwrap_or_default();
        match a.as_str() {
            "--jeux" => opts.jeux = Some(PathBuf::from(val())),
            "--donnees" => opts.donnees = Some(PathBuf::from(val())),
            "--rom" => opts.rom = Some(PathBuf::from(val())),
            "--demo" => opts.demo = true,
            "--capture" => opts.capture = Some(PathBuf::from(val())),
            "--images" => opts.images = val().parse().unwrap_or(30),
            "--menu" => opts.menu = Some(val()),
            "--recherche" => opts.recherche = Some(val()),
            "--grille" => opts.grille = true,
            "--selection" => opts.selection = val().parse().unwrap_or(0),
            "--taille" => {
                let v = val();
                if let Some((l, h)) = v.split_once('x') {
                    taille = [l.parse().unwrap_or(1280.0), h.parse().unwrap_or(800.0)];
                }
            }
            "-h" | "--help" => {
                println!("{AIDE}");
                return Ok(());
            }
            // Un fichier passé seul (ouvrir avec…, glisser sur l'exe) est lancé directement.
            autre if !autre.starts_with("--") => opts.rom = Some(PathBuf::from(autre)),
            _ => {
                eprintln!("{AIDE}");
                return Ok(());
            }
        }
    }

    let donnees = sauve::Chemins::trouver(opts.donnees.clone()).donnees;
    journal_plantage(donnees.join("crash.log"));
    let _verrou = match sauve::verrou_instance(&donnees) {
        Ok(v) => v,
        Err(sauve::DejaOuvert) => return deja_ouvert(opts.capture.is_some()),
    };

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("gbemu")
            .with_app_id("gbemu")
            .with_icon(dessin::icone_fenetre())
            .with_inner_size(taille)
            .with_min_inner_size([720.0, 480.0])
            .with_drag_and_drop(true),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "gbemu",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, opts)))),
    )
}

/// Panique : date, message, emplacement et pile ajoutés à `crash.log` (sans console, l'appli ne
/// laisserait aucune trace). Sauvegarde d'urgence : la panique se déplie jusqu'à la fermeture
/// de la partie (`Partie::drop` écrit la sauvegarde batterie).
fn journal_plantage(chemin: PathBuf) {
    let defaut = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let texte = format!(
            "--- {} UTC, gbemu {}\n{info}\n{}\n",
            sauve::horodatage(SystemTime::now()),
            env!("CARGO_PKG_VERSION"),
            std::backtrace::Backtrace::force_capture()
        );
        // Journal plafonné : repart de zéro au-delà de 1 Mio.
        let trop = std::fs::metadata(&chemin).is_ok_and(|m| m.len() > 1 << 20);
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(!trop)
            .truncate(trop)
            .open(&chemin)
        {
            let _ = f.write_all(texte.as_bytes());
        }
        defaut(info);
        // Écritures en fond (sauvegardes, réglages) menées à terme avant la sortie.
        if std::thread::current().name() != Some("ecritures") {
            sauve::attendre_ecritures();
        }
    }));
}

/// Deuxième instance sur le même dossier de données : message clair puis sortie (les deux
/// écraseraient leurs sauvegardes). `sans_fenetre` : vérification visuelle, message seul.
fn deja_ouvert(sans_fenetre: bool) -> eframe::Result {
    const MESSAGE: &str = "gbemu est déjà ouvert.\nUne seule fenêtre à la fois : deux fenêtres \
                           écraseraient leurs sauvegardes.";
    eprintln!("{MESSAGE}");
    if sans_fenetre {
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("gbemu")
            .with_app_id("gbemu")
            .with_inner_size([460.0, 120.0])
            .with_resizable(false),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_ui_native("gbemu", options, |ui, _| {
        use eframe::egui;
        ui.vertical_centered(|ui| {
            ui.add_space(16.0);
            ui.label(MESSAGE);
            ui.add_space(12.0);
            let touche =
                ui.input(|i| i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Enter));
            if ui.button("Fermer").clicked() || touche {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    })
}
