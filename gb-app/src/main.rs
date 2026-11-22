//! gbemu : application de bureau (bibliothèque de jeux + lecture Game Boy).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod biblio;
mod dat;
mod dessin;
mod etats;
mod jaquettes;
mod jeu;
mod manette;
mod rembobinage;
mod sauve;

use std::path::PathBuf;
use std::time::Instant;

const AIDE: &str = "gbemu [--jeux DOSSIER] [--donnees DOSSIER] [--rom FICHIER.gb]
  Vérification visuelle : [--demo] [--capture SORTIE.png] [--images N] [--taille LxH]
  [--menu pause|reglages] [--grille] [--recherche TEXTE] [--selection N]";

fn main() -> eframe::Result {
    let debut = Instant::now();
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

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("gbemu")
            .with_app_id("gbemu")
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
