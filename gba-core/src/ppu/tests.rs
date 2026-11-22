//! Tests de rendu par ligne : chaque fonctionnalité sur une petite scène construite à la main,
//! ligne générée comparée à la ligne attendue (couleurs BGR555).

use super::*;

const ROUGE: u16 = 0x001F;
const VERT: u16 = 0x03E0;
const BLEU: u16 = 0x7C00;
const BLANC: u16 = 0x7FFF;
const NOIR: u16 = 0;

fn pal(p: &mut Ppu, i: usize, c: u16) {
    p.pram[i * 2..i * 2 + 2].copy_from_slice(&c.to_le_bytes());
}

fn r16(p: &mut Ppu, o: u32, v: u16) {
    p.ecrit_io(o, v);
}

/// DISPCNT sans délai d'activation des calques (testé à part).
fn dispcnt(p: &mut Ppu, v: u16) {
    p.ecrit_io(0, v);
    p.delai_actif = [0; 6];
}

/// Tuile 4 bpp unie (index `n`) à l'adresse VRAM `a`.
fn tuile4(p: &mut Ppu, a: usize, n: u8) {
    p.vram[a..a + 32].fill(n | n << 4);
}

/// Tuile 8 bpp unie.
fn tuile8(p: &mut Ppu, a: usize, n: u8) {
    p.vram[a..a + 64].fill(n);
}

fn entree(p: &mut Ppu, a: usize, v: u16) {
    p.vram[a..a + 2].copy_from_slice(&v.to_le_bytes());
}

fn oam(p: &mut Ppu, i: usize, a0: u16, a1: u16, a2: u16) {
    for (k, v) in [a0, a1, a2].into_iter().enumerate() {
        p.oam[i * 8 + k * 2..i * 8 + k * 2 + 2].copy_from_slice(&v.to_le_bytes());
    }
}

/// Paramètre affine `g` des sprites (PA, PB, PC, PD).
fn oam_affine(p: &mut Ppu, g: usize, m: [i16; 4]) {
    for (k, v) in m.into_iter().enumerate() {
        let o = g * 32 + 6 + k * 8;
        p.oam[o..o + 2].copy_from_slice(&v.to_le_bytes());
    }
}

/// Sprites tous désactivés (l'OAM à zéro = 128 sprites 8×8 en (0, 0)).
fn oam_vide(p: &mut Ppu) {
    for i in 0..128 {
        oam(p, i, 0x200, 0, 0);
    }
}

/// Fond 0 texte 4 bpp : carte en 0x800 (bloc 1), tuile 1 unie index 1 (palette 0) partout
/// sauf la colonne de tuiles 0 (tuile 0 transparente).
fn scene_fond0(p: &mut Ppu) {
    oam_vide(p);
    pal(p, 0, NOIR);
    pal(p, 1, ROUGE);
    tuile4(p, 32, 1);
    for ty in 0..32 {
        for tx in 1..32 {
            entree(p, 0x800 + (ty * 32 + tx) * 2, 1);
        }
    }
    r16(p, 0x08, 1 << 8);
    dispcnt(p, 0x100);
}

#[test]
fn texte_4bpp_defilement_et_miroir() {
    let mut p = Ppu::new();
    scene_fond0(&mut p);
    let l = p.compose_ligne(0);
    assert_eq!(&l[..8], &[NOIR; 8]);
    assert_eq!(&l[8..], &[ROUGE; 232]);
    // Défilement horizontal de 3 : la colonne transparente finit en x = 4 ; la carte boucle à 256.
    r16(&mut p, 0x10, 3);
    let l = p.compose_ligne(0);
    assert_eq!(&l[..5], &[NOIR; 5]);
    assert_eq!(l[5], ROUGE);
    assert_eq!(l[239], ROUGE);
    // Tuile 2 : moitié gauche index 2 (vert), droite transparente ; retournée en x par le bit 10.
    pal(&mut p, 2, VERT);
    p.vram[64..96].copy_from_slice(&[0x22, 0x22, 0, 0].repeat(8));
    r16(&mut p, 0x10, 0);
    entree(&mut p, 0x800 + 2, 2);
    entree(&mut p, 0x800 + 4, 2 | 0x400);
    let l = p.compose_ligne(0);
    assert_eq!(&l[8..16], &[VERT, VERT, VERT, VERT, NOIR, NOIR, NOIR, NOIR]);
    assert_eq!(
        &l[16..24],
        &[NOIR, NOIR, NOIR, NOIR, VERT, VERT, VERT, VERT]
    );
    // Palette 4 bpp : bloc 3 de la palette.
    pal(&mut p, 0x31, BLEU);
    entree(&mut p, 0x800 + 6, 1 | 0x3000);
    assert_eq!(p.compose_ligne(0)[24], BLEU);
}

#[test]
fn texte_8bpp_et_priorites() {
    let mut p = Ppu::new();
    scene_fond0(&mut p);
    // Fond 1 : 8 bpp, tuile 1 (index 5 = bleu), carte en 0x1000, sur toute la ligne.
    pal(&mut p, 5, BLEU);
    tuile8(&mut p, 0x8000 + 64, 5);
    for tx in 0..32 {
        entree(&mut p, 0x1000 + tx * 2, 1);
    }
    r16(&mut p, 0x0A, 2 << 8 | 2 << 2 | 0x80);
    dispcnt(&mut p, 0x300);
    // Priorités égales : le fond 0 devant.
    let l = p.compose_ligne(0);
    assert_eq!(l[0], BLEU);
    assert_eq!(l[8], ROUGE);
    // Fond 0 en priorité 1 : le fond 1 (priorité 0) passe devant.
    r16(&mut p, 0x08, 1 << 8 | 1);
    assert_eq!(p.compose_ligne(0)[8], BLEU);
}

#[test]
fn texte_carte_512() {
    let mut p = Ppu::new();
    scene_fond0(&mut p);
    // Largeur 512 : le bloc suivant (0x1000) donne les colonnes 32-63 ; on le laisse vide.
    r16(&mut p, 0x08, 1 << 8 | 1 << 14);
    r16(&mut p, 0x10, 256 - 16);
    let l = p.compose_ligne(0);
    assert_eq!(&l[..16], &[ROUGE; 16]);
    assert_eq!(l[16], NOIR);
    // Hauteur 512 : la ligne 256 vient du bloc 0x1000 (vide).
    r16(&mut p, 0x08, 1 << 8 | 2 << 14);
    r16(&mut p, 0x10, 0);
    r16(&mut p, 0x12, 256);
    assert_eq!(p.compose_ligne(0)[8], NOIR);
}

/// Mode 1, fond 2 affine 128×128 : carte en 0x800, tuile 1 unie index 3 (vert) sur la colonne
/// de tuiles 0, transparent ailleurs.
fn scene_affine(p: &mut Ppu) {
    oam_vide(p);
    pal(p, 3, VERT);
    tuile8(p, 64, 3);
    for ty in 0..16 {
        p.vram[0x800 + ty * 16] = 1;
    }
    r16(p, 0x0C, 1 << 8);
    dispcnt(p, 1 | 0x400);
}

#[test]
fn affine_zoom_boucle_et_reference() {
    let mut p = Ppu::new();
    scene_affine(&mut p);
    let l = p.compose_ligne(0);
    assert_eq!(&l[..8], &[VERT; 8]);
    assert_eq!(&l[8..], &[NOIR; 232]);
    // Zoom ×2 (PA = 0,5) : 16 points verts.
    r16(&mut p, 0x20, 0x80);
    let l = p.compose_ligne(0);
    assert_eq!(&l[..16], &[VERT; 16]);
    assert_eq!(l[16], NOIR);
    // Sans débordement, rien après 128 texels ; avec, la colonne 0 revient en x = 256 (hors
    // écran à ×1, donc PA = 2 : texel 128 en x = 64).
    r16(&mut p, 0x20, 0x200);
    assert_eq!(p.compose_ligne(0)[64], NOIR);
    r16(&mut p, 0x0C, 1 << 8 | 0x2000);
    let l = p.compose_ligne(0);
    assert_eq!(&l[64..68], &[VERT; 4]);
    // Point de référence BG2X = -4 (28 bits signés) : 4 points de plus avant la colonne 0.
    r16(&mut p, 0x20, 0x100);
    r16(&mut p, 0x0C, 1 << 8);
    r16(&mut p, 0x28, (-4i32 << 8) as u16);
    r16(&mut p, 0x2A, ((-4i32 << 8) >> 16) as u16 & 0x0FFF);
    let l = p.compose_ligne(0);
    assert_eq!(&l[..4], &[NOIR; 4]);
    assert_eq!(&l[4..12], &[VERT; 8]);
}

#[test]
fn affine_avance_par_ligne_et_rechargement_vblank() {
    let mut p = Ppu::new();
    scene_affine(&mut p);
    // PB = 1 : chaque ligne décale x d'un texel (cisaillement).
    r16(&mut p, 0x22, 0x100);
    p.dessine_ligne();
    p.dessine_ligne();
    let l = p.compose_ligne(2);
    assert_eq!(&l[..6], &[VERT; 6]);
    assert_eq!(l[6], NOIR);
    // Le VBlank recharge BG2X : on reprend à 0 dès la ligne suivante.
    while p.vcount != HAUTEUR as u16 {
        p.evenement();
    }
    assert_eq!(p.ref_aff[0], [0, 0]);
    // Une écriture de BG2X en cours d'image recharge aussi.
    p.ref_aff[0][0] = 1234;
    r16(&mut p, 0x2A, 0);
    assert_eq!(p.ref_aff[0][0], 0);
}

#[test]
fn bitmaps_modes_3_4_5() {
    let mut p = Ppu::new();
    oam_vide(&mut p);
    pal(&mut p, 0, NOIR);
    // Mode 3 : ligne 1, x = 2 en bleu.
    let o = (240 + 2) * 2;
    p.vram[o..o + 2].copy_from_slice(&BLEU.to_le_bytes());
    dispcnt(&mut p, 3 | 0x400);
    p.dessine_ligne(); // ligne 0 → point de référence y = 1
    assert_eq!(p.compose_ligne(1)[2], BLEU);
    // Mode 4 : page 2 (0xA000), index 7.
    pal(&mut p, 7, ROUGE);
    p.vram[0xA000 + 5] = 7;
    r16(&mut p, 0x28, 0);
    r16(&mut p, 0x2C, 0);
    dispcnt(&mut p, 4 | 0x400 | 0x10);
    let l = p.compose_ligne(0);
    assert_eq!(l[5], ROUGE);
    assert_eq!(l[4], NOIR); // index 0 transparent
    dispcnt(&mut p, 4 | 0x400);
    assert_eq!(p.compose_ligne(0)[5], NOIR);
    // Mode 5 : 160×128, hors image = fond.
    pal(&mut p, 0, VERT);
    p.vram[0..320].copy_from_slice(&BLANC.to_le_bytes().repeat(160));
    dispcnt(&mut p, 5 | 0x400);
    let l = p.compose_ligne(0);
    assert_eq!(l[159], BLANC);
    assert_eq!(l[160], VERT);
}

/// Sprite 0 : 8×8 4 bpp, tuile 1 unie index 1 (palette OBJ 0 = rouge).
fn scene_sprite(p: &mut Ppu) {
    oam_vide(p);
    pal(p, 0, NOIR);
    pal(p, 0x101, ROUGE);
    tuile4(p, 0x10000 + 32, 1);
    oam(p, 0, 10, 20, 1);
    dispcnt(p, 0x1000 | 0x40);
}

#[test]
fn sprite_simple_position_et_lignes() {
    let mut p = Ppu::new();
    scene_sprite(&mut p);
    let l = p.compose_ligne(10);
    assert_eq!(l[19], NOIR);
    assert_eq!(&l[20..28], &[ROUGE; 8]);
    assert_eq!(l[28], NOIR);
    assert_eq!(p.compose_ligne(9)[20], NOIR);
    assert_eq!(p.compose_ligne(17)[20], ROUGE);
    assert_eq!(p.compose_ligne(18)[20], NOIR);
    // X = 508 (-4) : 4 points visibles à gauche ; Y = 252 : lignes 0 à 3.
    oam(&mut p, 0, 252, 508, 1);
    let l = p.compose_ligne(3);
    assert_eq!(&l[..4], &[ROUGE; 4]);
    assert_eq!(l[4], NOIR);
    assert_eq!(p.compose_ligne(4)[0], NOIR);
    // Désactivé (bit 9 sans affine).
    oam(&mut p, 0, 0x200 | 10, 20, 1);
    assert_eq!(p.compose_ligne(10)[20], NOIR);
}

#[test]
fn sprite_correspondance_1d_2d_et_miroirs() {
    let mut p = Ppu::new();
    scene_sprite(&mut p);
    // 16×16 tuile de base 1 : en 1D la tuile de la 2e rangée est 1 + 2 = 3, en 2D 1 + 32 = 33.
    pal(&mut p, 0x102, VERT);
    pal(&mut p, 0x103, BLEU);
    tuile4(&mut p, 0x10000 + 3 * 32, 2);
    tuile4(&mut p, 0x10000 + 33 * 32, 3);
    oam(&mut p, 0, 0, 1 << 14, 1);
    assert_eq!(p.compose_ligne(8)[0], VERT);
    dispcnt(&mut p, 0x1000);
    assert_eq!(p.compose_ligne(8)[0], BLEU);
    // Miroir vertical : la 1re ligne affichée vient de la 2e rangée.
    oam(&mut p, 0, 0, 1 << 14 | 0x2000, 1);
    assert_eq!(p.compose_ligne(0)[0], BLEU);
    // Miroir horizontal sur une tuile mi-pleine.
    dispcnt(&mut p, 0x1000 | 0x40);
    p.vram[0x10000 + 32..0x10000 + 64].copy_from_slice(&[0x11, 0x11, 0, 0].repeat(8));
    oam(&mut p, 0, 0, 0, 1);
    assert_eq!(
        &p.compose_ligne(0)[..8],
        &[ROUGE, ROUGE, ROUGE, ROUGE, NOIR, NOIR, NOIR, NOIR]
    );
    oam(&mut p, 0, 0, 0x1000, 1);
    assert_eq!(
        &p.compose_ligne(0)[..8],
        &[NOIR, NOIR, NOIR, NOIR, ROUGE, ROUGE, ROUGE, ROUGE]
    );
    // 8 bpp : tuile 2 unie index 9.
    pal(&mut p, 0x109, BLANC);
    tuile8(&mut p, 0x10000 + 64, 9);
    oam(&mut p, 0, 0x2000, 0, 2);
    assert_eq!(p.compose_ligne(0)[0], BLANC);
}

#[test]
fn sprite_priorites_ordre_et_mode_bitmap() {
    let mut p = Ppu::new();
    scene_fond0(&mut p);
    pal(&mut p, 0x101, VERT);
    pal(&mut p, 0x102, BLEU);
    tuile4(&mut p, 0x10000 + 32, 1);
    tuile4(&mut p, 0x10000 + 64, 2);
    dispcnt(&mut p, 0x1100 | 0x40);
    // Sprite priorité 0 devant le fond (priorité 0).
    oam(&mut p, 0, 0, 8, 1);
    assert_eq!(p.compose_ligne(0)[8], VERT);
    // Fond en priorité 0, sprite en priorité 1 : derrière.
    oam(&mut p, 0, 0, 8, 1 | 1 << 10);
    assert_eq!(p.compose_ligne(0)[8], ROUGE);
    // Deux sprites même priorité : le plus petit numéro devant.
    oam(&mut p, 0, 0, 8, 1);
    oam(&mut p, 1, 0, 8, 2);
    assert_eq!(p.compose_ligne(0)[8], VERT);
    // Le n° 1 de priorité plus forte passe devant.
    oam(&mut p, 0, 0, 8, 1 | 1 << 10);
    assert_eq!(p.compose_ligne(0)[8], BLEU);
    // Mode bitmap : tuiles < 512 invisibles, 512 et plus visibles.
    oam_vide(&mut p);
    pal(&mut p, 0, NOIR);
    dispcnt(&mut p, 3 | 0x1000 | 0x40);
    oam(&mut p, 0, 0, 0, 1);
    assert_eq!(p.compose_ligne(0)[0], NOIR);
    tuile4(&mut p, 0x14000, 2);
    oam(&mut p, 0, 0, 0, 512);
    assert_eq!(p.compose_ligne(0)[0], BLEU);
}

#[test]
fn sprite_affine_et_double_taille() {
    let mut p = Ppu::new();
    scene_sprite(&mut p);
    // Affine identité : 8 points.
    oam_affine(&mut p, 0, [0x100, 0, 0, 0x100]);
    oam(&mut p, 0, 0x100, 20, 1);
    let l = p.compose_ligne(0);
    assert_eq!(&l[20..28], &[ROUGE; 8]);
    assert_eq!(l[28], NOIR);
    // Double taille : cadre 16×16, sprite centré (x 24-31, lignes 4-11).
    oam(&mut p, 0, 0x300, 20, 1);
    let l = p.compose_ligne(4);
    assert_eq!(l[23], NOIR);
    assert_eq!(&l[24..32], &[ROUGE; 8]);
    assert_eq!(p.compose_ligne(3)[24], NOIR);
    // Agrandi ×2 (PA = PD = 0,5) dans le cadre double : 16 points.
    oam_affine(&mut p, 0, [0x80, 0, 0, 0x80]);
    let l = p.compose_ligne(0);
    assert_eq!(&l[20..36], &[ROUGE; 16]);
    // Groupe de paramètres 1 (attr1 bits 9-13) : miroir horizontal PA = -1.
    p.vram[0x10000 + 32..0x10000 + 64].copy_from_slice(&[0x11, 0x11, 0, 0].repeat(8));
    oam_affine(&mut p, 1, [-0x100, 0, 0, 0x100]);
    oam(&mut p, 0, 0x100, 20 | 1 << 9, 1);
    let l = p.compose_ligne(0);
    // Texels 8 (hors sprite), 7 … 1 : il manque le décalage d'un texel d'un vrai miroir.
    assert_eq!(
        &l[20..28],
        &[NOIR, NOIR, NOIR, NOIR, NOIR, ROUGE, ROUGE, ROUGE]
    );
}

#[test]
fn sprite_limite_de_cycles() {
    let mut p = Ppu::new();
    scene_sprite(&mut p);
    // 64×64 coûte 64 cycles : 1210 / 64 = 18 sprites dessinés, le 19e non.
    tuile4(&mut p, 0x10000 + 32, 1);
    for i in 0..19 {
        oam(&mut p, i, 0, 3 << 14 | (i as u16 * 8), 1);
    }
    let l = p.compose_ligne(0);
    // Le 18e (n° 17) commence en x = 136 ; ses tuiles 1+k sont vides sauf la 1re.
    assert_eq!(l[136], ROUGE);
    assert_eq!(l[144], NOIR);
    // HBlank libre (DISPCNT bit 5) : 954 cycles, 14 sprites.
    dispcnt(&mut p, 0x1000 | 0x40 | 0x20);
    let l = p.compose_ligne(0);
    assert_eq!(l[13 * 8], ROUGE);
    assert_eq!(l[14 * 8], NOIR);
}

#[test]
fn fenetres_0_1_obj_et_exterieur() {
    let mut p = Ppu::new();
    scene_fond0(&mut p);
    // WIN0 x 16-31 sans fond ; extérieur avec fond 0.
    r16(&mut p, 0x40, 16 << 8 | 32);
    r16(&mut p, 0x44, 160);
    p.fen_v = [true, false];
    r16(&mut p, 0x48, 0);
    r16(&mut p, 0x4A, 1);
    dispcnt(&mut p, 0x100 | 0x2000);
    let l = p.compose_ligne(0);
    assert_eq!(l[15], ROUGE);
    assert_eq!(&l[16..32], &[NOIR; 16]);
    assert_eq!(l[32], ROUGE);
    // X1 > X2 : la fenêtre boucle (x ≥ 200 ou x < 16).
    r16(&mut p, 0x40, 200 << 8 | 16);
    let l = p.compose_ligne(0);
    assert_eq!(l[8], NOIR);
    assert_eq!(l[100], ROUGE);
    assert_eq!(l[220], NOIR);
    // WIN1 sur x 8-23 avec fond, WIN0 (prioritaire) sur 16-31 sans.
    r16(&mut p, 0x40, 16 << 8 | 32);
    r16(&mut p, 0x42, 8 << 8 | 24);
    r16(&mut p, 0x48, 1 << 8);
    r16(&mut p, 0x4A, 0);
    p.fen_v = [true, true];
    dispcnt(&mut p, 0x100 | 0x6000);
    let l = p.compose_ligne(0);
    assert_eq!(l[100], NOIR);
    assert_eq!(&l[8..16], &[ROUGE; 8]);
    assert_eq!(l[16], NOIR);
    // Fenêtre OBJ : un sprite en mode 2 (invisible) ouvre le fond.
    tuile4(&mut p, 0x10000 + 32, 1);
    oam(&mut p, 0, 2 << 10, 100, 1);
    r16(&mut p, 0x4A, 1 << 8);
    dispcnt(&mut p, 0x100 | 0x1000 | 0x8000 | 0x40);
    let l = p.compose_ligne(0);
    assert_eq!(l[99], NOIR);
    assert_eq!(&l[100..108], &[ROUGE; 8]);
    assert_eq!(l[108], NOIR);
}

#[test]
fn fenetres_bascule_verticale() {
    let mut p = Ppu::new();
    r16(&mut p, 0x44, 10 << 8 | 20);
    let mut actives = Vec::new();
    for _ in 0..LIGNES as usize * 2 {
        let ev = p.vcount;
        p.evenement();
        if p.vcount != ev && p.fen_v[0] && p.vcount < HAUTEUR as u16 {
            actives.push(p.vcount);
        }
    }
    assert_eq!(actives, (10..20).collect::<Vec<_>>());
}

#[test]
fn effets_alpha_eclaircir_assombrir() {
    let mut p = Ppu::new();
    scene_fond0(&mut p);
    // Fond 1 vert derrière (priorité 1), sur toute la ligne.
    pal(&mut p, 2, VERT);
    tuile4(&mut p, 64, 2);
    for tx in 0..32 {
        entree(&mut p, 0x1000 + tx * 2, 2);
    }
    r16(&mut p, 0x0A, 2 << 8 | 1);
    dispcnt(&mut p, 0x300);
    // Alpha : fond 0 (1re cible) sur fond 1 (2e cible), EVA = EVB = 8 → moitié de chaque.
    r16(&mut p, 0x50, 1 | 1 << 6 | 2 << 8);
    r16(&mut p, 0x52, 8 | 8 << 8);
    let l = p.compose_ligne(0);
    assert_eq!(l[8], 15 | 15 << 5);
    assert_eq!(l[0], VERT); // fond 0 transparent : fond 1 seul, pas 1re cible
                            // 2e cible absente (fond d'écran non coché sous un pixel sans fond 1) : pas de mélange.
    r16(&mut p, 0x50, 1 | 1 << 6 | 0x20 << 8);
    assert_eq!(p.compose_ligne(0)[8], ROUGE);
    // Saturation : EVA = EVB = 16.
    pal(&mut p, 1, 0x0010);
    pal(&mut p, 2, 0x0010);
    r16(&mut p, 0x50, 1 | 1 << 6 | 2 << 8);
    r16(&mut p, 0x52, 16 | 16 << 8);
    assert_eq!(p.compose_ligne(0)[8], 31);
    // Éclaircir / assombrir avec EVY = 8.
    pal(&mut p, 1, 10 | 20 << 5);
    r16(&mut p, 0x54, 8);
    r16(&mut p, 0x50, 1 | 2 << 6);
    assert_eq!(p.compose_ligne(0)[8], (10 + 10) | (20 + 5) << 5 | 15 << 10);
    r16(&mut p, 0x50, 1 | 3 << 6);
    assert_eq!(p.compose_ligne(0)[8], 5 | 10 << 5);
    // Fond d'écran (bit 5) en 1re cible.
    dispcnt(&mut p, 0x100);
    r16(&mut p, 0x50, 0x20 | 2 << 6);
    r16(&mut p, 0x54, 16);
    assert_eq!(p.compose_ligne(0)[0], BLANC);
    // Une fenêtre sans le bit 5 coupe l'effet.
    r16(&mut p, 0x4A, 0x1F);
    dispcnt(&mut p, 0x100 | 0x2000);
    assert_eq!(p.compose_ligne(0)[0], NOIR);
}

#[test]
fn sprite_semi_transparent() {
    let mut p = Ppu::new();
    scene_fond0(&mut p);
    pal(&mut p, 0x101, BLEU);
    tuile4(&mut p, 0x10000 + 32, 1);
    oam(&mut p, 0, 1 << 10, 8, 1);
    dispcnt(&mut p, 0x1100 | 0x40);
    r16(&mut p, 0x52, 8 | 8 << 8);
    // Sans 2e cible : opaque.
    assert_eq!(p.compose_ligne(0)[8], BLEU);
    // Fond 0 en 2e cible : mélange forcé même sans mode alpha ni sprite en 1re cible.
    r16(&mut p, 0x50, 1 << 8);
    assert_eq!(p.compose_ligne(0)[8], 15 | 15 << 10);
    // Au-dessus du fond d'écran non coché : mode éclaircir s'il est 1re cible.
    r16(&mut p, 0x50, 0x10 | 2 << 6 | 1 << 8);
    r16(&mut p, 0x54, 16);
    oam(&mut p, 0, 1 << 10, 0, 1);
    assert_eq!(p.compose_ligne(0)[0], BLANC);
}

#[test]
fn mosaique_fonds_et_sprites() {
    let mut p = Ppu::new();
    scene_fond0(&mut p);
    // Fond 0 mosaïque H = 4, HOFS 6 : x 0-1 transparents, 2-3 rouges sans mosaïque ; le bloc
    // 0-3 reprend x = 0.
    r16(&mut p, 0x10, 6);
    r16(&mut p, 0x08, 1 << 8 | 0x40);
    r16(&mut p, 0x4C, 3);
    let l = p.compose_ligne(0);
    assert_eq!(&l[..4], &[NOIR; 4]);
    assert_eq!(&l[4..8], &[ROUGE; 4]);
    r16(&mut p, 0x10, 2);
    let l = p.compose_ligne(0);
    assert_eq!(&l[4..8], &[NOIR; 4]); // x = 4 → texel 6 transparent
    assert_eq!(&l[8..12], &[ROUGE; 4]);
    // Mosaïque V = 3 : la ligne 5 reprend la ligne 3 (VOFS 253 : ligne 3 = rangée de tuiles 0,
    // ligne 5 = rangée 32 → 0 aussi ; on marque la rangée 0 colonne 1 en vert).
    pal(&mut p, 2, VERT);
    tuile4(&mut p, 64, 2);
    entree(&mut p, 0x800 + 2, 2);
    r16(&mut p, 0x10, 0);
    r16(&mut p, 0x12, 253);
    r16(&mut p, 0x4C, 2 << 4);
    assert_eq!(p.compose_ligne(4)[8], VERT); // ligne 3 → 256 → rangée 0
    assert_eq!(p.compose_ligne(2)[8], ROUGE); // ligne 0 → 253 → rangée 31
                                              // Sprite mosaïque H = 4 (texels 0-1 et 6-7 pleins) en x = 2 : grille de l'écran ; x 2-3 =
                                              // début du sprite, 4-7 = texel 2 (vide) répété, 8-9 = texel 6.
    oam_vide(&mut p);
    dispcnt(&mut p, 0x1000 | 0x40);
    pal(&mut p, 0x101, BLEU);
    p.vram[0x10000 + 32..0x10000 + 64].copy_from_slice(&[0x11, 0, 0, 0x11].repeat(8));
    oam(&mut p, 0, 0x1000, 2, 1);
    r16(&mut p, 0x4C, 3 << 8);
    let l = p.compose_ligne(0);
    assert_eq!(&l[2..10], &[BLEU, BLEU, NOIR, NOIR, NOIR, NOIR, BLEU, BLEU]);
}

#[test]
fn ecran_blanc_vert_inverse_et_delai_dispcnt() {
    let mut p = Ppu::new();
    scene_fond0(&mut p);
    dispcnt(&mut p, 0x180);
    assert_eq!(p.compose_ligne(0), [BLANC; LARGEUR]);
    // Vert inversé : échange des composantes vertes par paires de points.
    pal(&mut p, 0, BLEU);
    pal(&mut p, 1, VERT);
    dispcnt(&mut p, 0x100);
    r16(&mut p, 0x02, 1);
    let l = p.compose_ligne(0);
    assert_eq!(&l[6..10], &[BLEU, BLEU, VERT, VERT]);
    r16(&mut p, 0x10, 1);
    let l = p.compose_ligne(0);
    assert_eq!(&l[6..10], &[BLEU | VERT, 0, VERT, VERT]);
    // Fond activé : visible 3 lignes plus tard ; désactivé : immédiat.
    r16(&mut p, 0x02, 0);
    r16(&mut p, 0x10, 0);
    r16(&mut p, 0x00, 0);
    r16(&mut p, 0x00, 0x100);
    let mut lignes = Vec::new();
    for _ in 0..8 {
        lignes.push(p.compose_ligne(0)[8] == VERT);
        p.evenement();
        p.evenement();
    }
    assert_eq!(lignes, [false, false, false, true, true, true, true, true]);
    r16(&mut p, 0x00, 0);
    assert_eq!(p.compose_ligne(0)[8], BLEU);
}
