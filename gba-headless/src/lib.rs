//! Outils communs à l'exécuteur GBA sans écran et aux tests ROM : PNG et lecture des résultats.

use gba_core::{touche, Gba, HAUTEUR, LARGEUR};
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

/// Écrit l'écran RGB8 240×160 en PNG.
pub fn write_png_rgb(path: &Path, rgb: &[u8]) -> Result<(), String> {
    let f = File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(BufWriter::new(f), LARGEUR as u32, HAUTEUR as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .and_then(|mut w| w.write_image_data(rgb))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Exécute jusqu'à `max_frames` images ou jusqu'à ce que `stop` soit vrai (vérifié à chaque image).
/// Renvoie le nombre d'images exécutées.
pub fn run_until(gba: &mut Gba, max_frames: u32, mut stop: impl FnMut(&Gba) -> bool) -> u32 {
    for i in 0..max_frames {
        if stop(gba) {
            return i;
        }
        gba.run_frame();
    }
    max_frames
}

/// Résultat d'une ROM jsmolka gba-tests : `Some(0)` = tout réussi, `Some(n)` = 1er test raté,
/// `None` = pas fini. Le numéro est dans r12 quand la ROM tourne sur sa boucle finale.
pub fn jsmolka_resultat(gba: &Gba) -> Option<u32> {
    gba.en_boucle_infinie().then(|| gba.registres()[12])
}

/// Nom de touche (A, B, SELECT, START, DROITE, GAUCHE, HAUT, BAS, R, L) → bit.
pub fn touche_nommee(nom: &str) -> Option<u16> {
    Some(match nom.to_ascii_uppercase().as_str() {
        "A" => touche::A,
        "B" => touche::B,
        "SELECT" => touche::SELECT,
        "START" => touche::START,
        "DROITE" => touche::DROITE,
        "GAUCHE" => touche::GAUCHE,
        "HAUT" => touche::HAUT,
        "BAS" => touche::BAS,
        "R" => touche::R,
        "L" => touche::L,
        _ => return None,
    })
}
