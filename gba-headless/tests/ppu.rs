//! Tests PPU GBA par ROM de test publiques avec image attendue : nba-emu hw-test (BSD-3),
//! destoer gba_tests (MIT) ; et démos tonc (CC0) exécutées sans panique.
//! Ils sautent proprement si `roms/` manque : lancer `scripts/roms-test.sh`.

use gba_core::Gba;
use std::path::{Path, PathBuf};

fn chemin(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../roms")
        .join(rel)
}

/// Exécute une ROM de test `images` images, ou `None` (avec message) si absente.
fn execute(rel: &str, images: u32) -> Option<Gba> {
    let p = chemin(rel);
    let Ok(data) = std::fs::read(&p) else {
        eprintln!(
            "SAUTÉ : {} absent (lancer scripts/roms-test.sh)",
            p.display()
        );
        return None;
    };
    let mut gba = Gba::new(&data, None).expect("ROM de test illisible");
    for _ in 0..images {
        gba.run_frame();
    }
    Some(gba)
}

/// PNG en RGB8 (alpha ignoré) : (largeur, hauteur, points).
fn lit_png(rel: &str) -> (usize, usize, Vec<[u8; 3]>) {
    let f = std::fs::File::open(chemin(rel)).expect("image attendue absente");
    let mut dec = png::Decoder::new(f);
    dec.set_transformations(png::Transformations::EXPAND);
    let mut r = dec.read_info().unwrap();
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).unwrap();
    let n = info.color_type.samples();
    let px = buf[..info.buffer_size()]
        .chunks(n)
        .map(|c| [c[0], c[1], c[2]])
        .collect();
    (info.width as usize, info.height as usize, px)
}

/// destoer window_midframe : WIN0 modifiée en cours d'image ; image attendue 240×160 exacte
/// (comparée en 5 bits par composante).
#[test]
fn destoer_window_midframe() {
    let Some(gba) = execute("destoer-gba-tests/window_midframe/window_midframe.gba", 60) else {
        return;
    };
    let (l, h, att) = lit_png("destoer-gba-tests/window_midframe/expected.png");
    assert_eq!((l, h), (240, 160));
    let ecarts = gba
        .frame()
        .as_chunks::<3>()
        .0
        .iter()
        .zip(att.iter())
        .filter(|(a, b)| a.map(|v| v >> 3) != b.map(|v| v >> 3))
        .count();
    assert_eq!(ecarts, 0);
}

/// nba-emu sprite-hmosaic : mosaïque horizontale des sprites (chaînage entre sprites voisins,
/// priorités, points transparents). L'image attendue est une capture de NanoBoyAdvance agrandie
/// ×8 (écran en (68, 116)), échantillonnée au centre des points et seuillée (couleurs pures).
#[test]
fn nba_sprite_hmosaic() {
    let Some(gba) = execute("nba-hw-test/ppu/sprite-hmosaic/sprite-hmosaic.gba", 60) else {
        return;
    };
    let (l, _, att) = lit_png("nba-hw-test/ppu/sprite-hmosaic/expected.png");
    let seuil = |p: [u8; 3]| p.map(|v| v > 127);
    let ecran = gba.frame().as_chunks::<3>().0;
    let mut ecarts = 0;
    for y in 0..160 {
        for x in 0..240 {
            let a = att[(116 + 8 * y + 4) * l + 68 + 8 * x + 4];
            if seuil(ecran[y * 240 + x]) != seuil(a) {
                ecarts += 1;
            }
        }
    }
    assert_eq!(ecarts, 0);
}

/// Démos tonc (modes 0-5, sprites, affines, fenêtres, mélange, mosaïque, mode 7) : 120 images
/// sans panique et un écran non uni. Le rendu est vérifié à l'œil (captures dans
/// out/captures/gba/ppu/tonc/).
#[test]
fn tonc_demos() {
    let dossier = chemin("tonc");
    let Ok(lecture) = std::fs::read_dir(&dossier) else {
        eprintln!("SAUTÉ : {} absent", dossier.display());
        return;
    };
    let mut noms: Vec<_> = lecture
        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
        .filter(|n| n.ends_with(".gba"))
        .collect();
    noms.sort();
    assert!(noms.len() >= 30, "{noms:?}");
    for n in noms {
        let gba = execute(&format!("tonc/{n}"), 120).unwrap();
        let px = gba.frame().as_chunks::<3>().0;
        assert!(px.iter().any(|p| *p != px[0]), "{n} : écran uni");
    }
}

/// Sauvegarde d'état avec le PPU complet : rejouer un état redonne les mêmes images (mode 7
/// affine + sprites + mélange, délais DISPCNT, mosaïque des sprites).
#[test]
fn etat_rejoue_meme_ecran() {
    for rel in [
        "tonc/m7_ex.gba",
        "nba-hw-test/ppu/dispcnt-latch/dispcnt-latch.gba",
        "nba-hw-test/ppu/sprite-hmosaic/sprite-hmosaic.gba",
    ] {
        let Some(mut gba) = execute(rel, 100) else {
            return;
        };
        let etat = gba.save_state();
        let suite = |g: &mut Gba| {
            (0..30)
                .map(|_| {
                    g.run_frame();
                    g.frame().to_vec()
                })
                .collect::<Vec<_>>()
        };
        let a = suite(&mut gba);
        gba.load_state(&etat).unwrap();
        assert!(
            a == suite(&mut gba),
            "{rel} : images différentes après rechargement"
        );
    }
}
