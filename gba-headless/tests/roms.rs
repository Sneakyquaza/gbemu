//! Tests GBA par ROM de test publiques : jsmolka gba-tests, FuzzARM, armwrestler.
//! Ils sautent proprement si `roms/` manque : lancer `scripts/roms-test.sh`.

use gba_core::{touche, Gba};
use gba_headless::{jsmolka_resultat, run_until};
use std::path::Path;

/// Charge une ROM de test (BIOS libre, démarrage direct), ou `None` (avec message) si absente.
fn charge(rel: &str) -> Option<Gba> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../roms")
        .join(rel);
    let Ok(data) = std::fs::read(&p) else {
        eprintln!(
            "SAUTÉ : {} absent (lancer scripts/roms-test.sh)",
            p.display()
        );
        return None;
    };
    Some(Gba::new(&data, None).expect("ROM de test illisible"))
}

/// jsmolka : (ROM, résultat attendu). 0 = tout réussi ; sinon n° du 1er test raté connu.
const JSMOLKA: [(&str, u32); 7] = [
    ("arm/arm.gba", 0),
    ("thumb/thumb.gba", 0),
    ("memory/memory.gba", 0),
    ("save/none.gba", 0),
    ("save/sram.gba", 0),
    ("nes/nes.gba", 0),
    // Lecture du BIOS après SWI : valeur propre au BIOS de Nintendo (0x188 + 8), jamais embarqué.
    ("bios/bios.gba", 2),
];
// Hors liste : save/flash64 et flash128 (lot « sauvegardes »), unsafe (comportements non
// vérifiés sur console), ppu/* (vérifiés à l'œil : pas d'image de référence).

#[test]
fn jsmolka_gba_tests() {
    let mut ecarts = Vec::new();
    for (rel, attendu) in JSMOLKA {
        let Some(mut gba) = charge(&format!("gba-tests/{rel}")) else {
            return;
        };
        run_until(&mut gba, 300, |g| jsmolka_resultat(g).is_some());
        let r = jsmolka_resultat(&gba);
        eprintln!("{rel} : {r:?}");
        if r != Some(attendu) {
            ecarts.push(format!("{rel} : {r:?} au lieu de Some({attendu})"));
        }
    }
    assert!(ecarts.is_empty(), "{ecarts:#?}");
}

/// Démarrage par le BIOS (exécuté depuis 0, écran de démarrage compris) puis arm.gba.
#[test]
fn jsmolka_arm_apres_demarrage_bios() {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../roms/gba-tests/arm/arm.gba");
    let Ok(data) = std::fs::read(&p) else {
        eprintln!("SAUTÉ : {} absent", p.display());
        return;
    };
    let mut gba = Gba::new(&data, Some(gba_core::BIOS_LIBRE)).unwrap();
    run_until(&mut gba, 600, |g| jsmolka_resultat(g).is_some());
    assert_eq!(jsmolka_resultat(&gba), Some(0));
}

/// FuzzARM : fin de la ROM (boucle finale) sans rapport d'erreur au début de l'EWRAM.
#[test]
fn fuzzarm() {
    for nom in [
        "ARM_Any",
        "ARM_DataProcessing",
        "THUMB_Any",
        "THUMB_DataProcessing",
        "FuzzARM",
    ] {
        let Some(mut gba) = charge(&format!("fuzzarm/{nom}.gba")) else {
            return;
        };
        run_until(&mut gba, 6000, |g| g.en_boucle_infinie());
        assert!(gba.en_boucle_infinie(), "{nom} : pas fini");
        let rapport: Vec<u8> = (0..4).map(|i| gba.peek(0x0200_0000 + i)).collect();
        assert_eq!(rapport, [0; 4], "{nom} : test raté (rapport en EWRAM)");
    }
}

/// Pixels « BAD » (couleur 0x0018) et « OK » (0x0300) d'une page armwrestler.
fn armwrestler_page(gba: &Gba) -> (usize, usize) {
    let px = gba.frame().as_chunks::<3>().0;
    let bad = px.iter().filter(|p| **p == [198, 0, 0]).count();
    let ok = px.iter().filter(|p| **p == [0, 198, 0]).count();
    (bad, ok)
}

fn appui(gba: &mut Gba, t: u16) {
    gba.set_keys(t);
    for _ in 0..6 {
        gba.run_frame();
    }
    gba.set_keys(0);
    for _ in 0..30 {
        gba.run_frame();
    }
}

/// armwrestler : les 5 pages ARM puis les 3 pages Thumb, aucune ligne « BAD ».
#[test]
fn armwrestler() {
    let Some(mut gba) = charge("armwrestler/armwrestler-gba-fixed.gba") else {
        return;
    };
    for _ in 0..60 {
        gba.run_frame();
    }
    let mut pages = Vec::new();
    for _ in 0..5 {
        appui(&mut gba, touche::START);
        pages.push(armwrestler_page(&gba));
    }
    appui(&mut gba, touche::START); // retour au menu
    for _ in 0..3 {
        appui(&mut gba, touche::BAS);
    }
    for _ in 0..3 {
        appui(&mut gba, touche::START);
        pages.push(armwrestler_page(&gba));
    }
    for (i, &(bad, ok)) in pages.iter().enumerate() {
        assert_eq!(bad, 0, "page {i} : lignes BAD");
        assert!(ok > 0, "page {i} : aucun OK (page non affichée ?)");
    }
}
