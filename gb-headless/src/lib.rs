//! Outils communs à l'exécutable sans écran et aux tests ROM : PNG et conditions d'arrêt.

use gb_core::{GameBoy, HAUTEUR, LARGEUR};
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

/// Gris DMG (teinte 0 = blanc).
const GRIS: [u8; 4] = [0xFF, 0xAA, 0x55, 0x00];

/// Écrit l'écran en PNG niveaux de gris.
pub fn write_png(path: &Path, frame: &[u8]) -> Result<(), String> {
    let f = File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(BufWriter::new(f), LARGEUR as u32, HAUTEUR as u32);
    enc.set_color(png::ColorType::Grayscale);
    enc.set_depth(png::BitDepth::Eight);
    let px: Vec<u8> = frame.iter().map(|&t| GRIS[(t & 3) as usize]).collect();
    enc.write_header()
        .and_then(|mut w| w.write_image_data(&px))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Lit un PNG 160×144 et le convertit en teintes 0-3 (par luminance).
pub fn read_png_shades(path: &Path) -> Result<Vec<u8>, String> {
    let f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut dec = png::Decoder::new(f);
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = dec.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).map_err(|e| e.to_string())?;
    if info.width as usize != LARGEUR || info.height as usize != HAUTEUR {
        return Err(format!(
            "{} : taille {}x{}",
            path.display(),
            info.width,
            info.height
        ));
    }
    let ch = info.color_type.samples();
    Ok(buf[..info.buffer_size()]
        .chunks(ch)
        .map(|p| {
            let lum = if ch >= 3 {
                (p[0] as u32 * 299 + p[1] as u32 * 587 + p[2] as u32 * 114) / 1000
            } else {
                p[0] as u32
            };
            3 - ((lum + 42) / 85).min(3) as u8
        })
        .collect())
}

/// Exécute jusqu'à `max_frames` images ou jusqu'à ce que `stop` soit vrai (vérifié à chaque image).
/// Renvoie le nombre d'images exécutées.
pub fn run_until(gb: &mut GameBoy, max_frames: u32, mut stop: impl FnMut(&GameBoy) -> bool) -> u32 {
    for i in 0..max_frames {
        if stop(gb) {
            return i;
        }
        gb.run_frame();
    }
    max_frames
}

/// Vrai si la sortie série contient `motif`.
pub fn serial_contains(gb: &GameBoy, motif: &str) -> bool {
    gb.serial_output()
        .windows(motif.len())
        .any(|w| w == motif.as_bytes())
}

/// Registres de réussite mooneye (Fibonacci).
pub fn mooneye_passed(gb: &GameBoy) -> bool {
    let r = gb.registres();
    (r.b, r.c, r.d, r.e, r.h, r.l) == (3, 5, 8, 13, 21, 34)
}

/// Résultat blargg écrit en mémoire cartouche : `Some(code)` une fois le test fini (0 = réussi).
pub fn blargg_memory_status(gb: &GameBoy) -> Option<u8> {
    let sig = [gb.peek(0xA001), gb.peek(0xA002), gb.peek(0xA003)];
    let st = gb.peek(0xA000);
    (sig == [0xDE, 0xB0, 0x61] && st != 0x80).then_some(st)
}

/// Texte blargg écrit en mémoire cartouche à partir de $A004.
pub fn blargg_memory_text(gb: &GameBoy) -> String {
    (0xA004..0xC000u16)
        .map(|a| gb.peek(a))
        .take_while(|&b| b != 0)
        .map(|b| b as char)
        .collect()
}
