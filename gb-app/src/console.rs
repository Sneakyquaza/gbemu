//! Console de poche dessinée en vectoriel (forme DMG, sans aucun logo ni texte de marque),
//! paramétrique : nette à toute taille. Boutons cliquables (zones renvoyées par
//! [`dessiner`]), enfoncés et illuminés à l'appui, halo pulsant sur le bouton choisi.
//! Les mesures sont en millimètres d'une console de 90 × 148 mm.

use crate::touches::Bouton;
use eframe::egui::{
    epaint::{Mesh, PathShape, PathStroke, TextShape},
    pos2, vec2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, TextureId, Vec2,
};
use std::f32::consts::PI;

const L: f32 = 90.0;
const H: f32 = 148.0;

const PLASTIQUE: Color32 = Color32::from_rgb(0xC9, 0xC4, 0xBC);
const PLASTIQUE_OMBRE: Color32 = Color32::from_rgb(0x9E, 0x99, 0x8F);
const PLASTIQUE_CLAIR: Color32 = Color32::from_rgb(0xD8, 0xD4, 0xCC);
const CADRE_HAUT: Color32 = Color32::from_rgb(0x5E, 0x5F, 0x73);
const CADRE_BAS: Color32 = Color32::from_rgb(0x46, 0x47, 0x58);
const CROIX_HAUT: Color32 = Color32::from_rgb(0x34, 0x34, 0x3A);
const CROIX_BAS: Color32 = Color32::from_rgb(0x17, 0x17, 0x1B);
const BAIE_HAUT: Color32 = Color32::from_rgb(0xB0, 0x2E, 0x6A);
const BAIE_BAS: Color32 = Color32::from_rgb(0x74, 0x17, 0x44);
const BAIE_ALLUMEE: Color32 = Color32::from_rgb(0xE8, 0x5A, 0x98);
const PASTILLE_HAUT: Color32 = Color32::from_rgb(0x7A, 0x78, 0x80);
const PASTILLE_BAS: Color32 = Color32::from_rgb(0x56, 0x54, 0x5C);
const ENCRE: Color32 = Color32::from_rgb(0x3C, 0x48, 0x9C);
const HALO: Color32 = Color32::from_rgb(0xF0, 0x7A, 0xAE);
const ORANGE: Color32 = Color32::from_rgb(0xE8, 0xA2, 0x3A);
const LED_ETEINTE: Color32 = Color32::from_rgb(0x5A, 0x22, 0x22);
const LED: Color32 = Color32::from_rgb(0xFF, 0x3B, 0x3B);
const ECRAN: [Color32; 4] = [
    Color32::from_rgb(0xC4, 0xCF, 0xA1),
    Color32::from_rgb(0x8B, 0x95, 0x6D),
    Color32::from_rgb(0x4D, 0x53, 0x3C),
    Color32::from_rgb(0x1F, 0x1F, 0x1F),
];

/// Ce que la console montre.
#[derive(Default, Clone, Copy)]
pub struct Vue {
    /// Boutons enfoncés (bit `1 << Bouton as usize`).
    pub allumes: u16,
    /// Bouton en attente d'une touche : halo pulsant.
    pub choisi: Option<Bouton>,
    /// Bouton qui vient de perdre sa touche : clignote orange.
    pub clignote: Option<Bouton>,
    /// Temps (s) pour les pulsations.
    pub temps: f64,
    /// Image du jeu à poser dans l'écran (sinon motif).
    pub ecran: Option<TextureId>,
}

/// Repère : millimètres → points.
#[derive(Clone, Copy)]
struct Geo {
    o: Pos2,
    s: f32,
}

impl Geo {
    fn p(&self, x: f32, y: f32) -> Pos2 {
        self.o + vec2(x, y) * self.s
    }
    fn r(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(self.p(x0, y0), self.p(x1, y1))
    }
}

/// Rectangle occupé par la console dans `zone` (centrée, proportions gardées).
pub fn cadre(zone: Rect) -> Rect {
    let s = (zone.width() / L).min(zone.height() / H);
    Rect::from_center_size(zone.center(), vec2(L, H) * s)
}

fn geo(zone: Rect) -> Geo {
    let c = cadre(zone);
    Geo {
        o: c.min,
        s: c.width() / L,
    }
}

// ----- Placement des commandes (mm) -----

const CROIX: (f32, f32) = (20.5, 102.0);
const CROIX_DEMI: f32 = 13.0;
const CROIX_BRAS: f32 = 4.4;
const BTN_B: (f32, f32) = (58.0, 104.5);
const BTN_A: (f32, f32) = (74.0, 96.5);
const BTN_R: f32 = 5.6;
const PASTILLE_SELECT: (f32, f32) = (35.5, 127.5);
const PASTILLE_START: (f32, f32) = (49.5, 127.5);
const ANGLE: f32 = -25.0 * PI / 180.0;
/// Gâchettes L/R (GBA) : languettes dans les coins du haut, au-dessus de la rainure.
const GACHETTE_L: (f32, f32, f32, f32) = (4.0, 1.4, 21.0, 5.8);
const GACHETTE_R: (f32, f32, f32, f32) = (L - 21.0, 1.4, L - 4.0, 5.8);

/// Centre d'un bouton à l'écran (pour poser une bulle à côté).
pub fn centre(zone: Rect, b: Bouton) -> Pos2 {
    let g = geo(zone);
    let (x, y) = match b {
        Bouton::Haut => (CROIX.0, CROIX.1 - CROIX_DEMI * 0.6),
        Bouton::Bas => (CROIX.0, CROIX.1 + CROIX_DEMI * 0.6),
        Bouton::Gauche => (CROIX.0 - CROIX_DEMI * 0.6, CROIX.1),
        Bouton::Droite => (CROIX.0 + CROIX_DEMI * 0.6, CROIX.1),
        Bouton::A => BTN_A,
        Bouton::B => BTN_B,
        Bouton::Select => PASTILLE_SELECT,
        Bouton::Start => PASTILLE_START,
        Bouton::L => (12.5, 3.6),
        Bouton::R => (L - 12.5, 3.6),
    };
    g.p(x, y)
}

/// Zones cliquables, dans l'ordre de [`Bouton::TOUS`].
pub fn zones(zone: Rect) -> [Rect; 10] {
    let g = geo(zone);
    let (cx, cy) = CROIX;
    // Branches seules (le centre de la croix n'est à personne), un peu débordantes au bout.
    let (d, w, e) = (CROIX_DEMI, CROIX_BRAS, CROIX_BRAS + 0.1);
    let rond = |(x, y): (f32, f32)| {
        g.r(
            x - BTN_R - 1.5,
            y - BTN_R - 1.5,
            x + BTN_R + 1.5,
            y + BTN_R + 1.5,
        )
    };
    let pastille = |(x, y): (f32, f32)| g.r(x - 6.5, y - 4.0, x + 6.5, y + 4.5);
    [
        g.r(cx - w, cy - d - 1.0, cx + w, cy - e),
        g.r(cx - w, cy + e, cx + w, cy + d + 1.0),
        g.r(cx - d - 1.0, cy - w, cx - e, cy + w),
        g.r(cx + e, cy - w, cx + d + 1.0, cy + w),
        rond(BTN_A),
        rond(BTN_B),
        pastille(PASTILLE_SELECT),
        pastille(PASTILLE_START),
        g.r(
            GACHETTE_L.0,
            GACHETTE_L.1 - 0.6,
            GACHETTE_L.2,
            GACHETTE_L.3 + 0.6,
        ),
        g.r(
            GACHETTE_R.0,
            GACHETTE_R.1 - 0.6,
            GACHETTE_R.2,
            GACHETTE_R.3 + 0.6,
        ),
    ]
}

// ----- Primitives -----

fn melange(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        m(a.r(), b.r()),
        m(a.g(), b.g()),
        m(a.b(), b.b()),
        m(a.a(), b.a()),
    )
}

fn voile(c: Color32, a: f32) -> Color32 {
    c.gamma_multiply(a.clamp(0.0, 1.0))
}

/// Contour d'un rectangle à coins arrondis (haut-gauche, haut-droit, bas-droit, bas-gauche).
fn arrondi(r: Rect, rayons: [f32; 4]) -> Vec<Pos2> {
    let coins = [
        (pos2(r.left(), r.top()), pos2(1.0, 1.0), PI),
        (pos2(r.right(), r.top()), pos2(-1.0, 1.0), 1.5 * PI),
        (pos2(r.right(), r.bottom()), pos2(-1.0, -1.0), 0.0),
        (pos2(r.left(), r.bottom()), pos2(1.0, -1.0), 0.5 * PI),
    ];
    let max = r.width().min(r.height()) * 0.5;
    let mut v = Vec::new();
    for (i, (coin, sens, a0)) in coins.into_iter().enumerate() {
        let ra = rayons[i].clamp(0.0, max);
        let c = pos2(coin.x + sens.x * ra, coin.y + sens.y * ra);
        let n = ((ra * 0.7) as usize).clamp(2, 28);
        for k in 0..=n {
            let a = a0 + 0.5 * PI * k as f32 / n as f32;
            v.push(c + vec2(a.cos(), a.sin()) * ra);
        }
    }
    v
}

/// Gélule de demi-longueur `l`, rayon `r`, inclinée de `ang`.
fn gelule(c: Pos2, l: f32, r: f32, ang: f32) -> Vec<Pos2> {
    let (ax, ay) = (vec2(ang.cos(), ang.sin()), vec2(-ang.sin(), ang.cos()));
    let n = ((r * 0.8) as usize).clamp(6, 20);
    let mut v = Vec::new();
    for (bout, a0) in [(l, -0.5 * PI), (-l, 0.5 * PI)] {
        let centre = c + ax * bout;
        for k in 0..=n {
            let a = a0 + PI * k as f32 / n as f32;
            v.push(centre + ax * (a.cos() * r) + ay * (a.sin() * r));
        }
    }
    v
}

/// Polygone convexe rempli d'un dégradé linéaire (haut-gauche `c0` → bas-droit `c1`),
/// bord adouci par un trait fin.
fn degrade(p: &Painter, pts: &[Pos2], c0: Color32, c1: Color32, sens: Vec2) {
    if pts.len() < 3 {
        return;
    }
    let sens = sens.normalized();
    let proj: Vec<f32> = pts.iter().map(|q| q.to_vec2().dot(sens)).collect();
    let (mn, mx) = proj
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
    let t = |q: Pos2| (q.to_vec2().dot(sens) - mn) / (mx - mn).max(1e-3);
    let centre = pts.iter().fold(Vec2::ZERO, |a, q| a + q.to_vec2()) / pts.len() as f32;
    let centre = centre.to_pos2();
    let mut mesh = Mesh::default();
    mesh.colored_vertex(centre, melange(c0, c1, t(centre)));
    for &q in pts {
        mesh.colored_vertex(q, melange(c0, c1, t(q)));
    }
    let n = pts.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    // Trait d'abord : sa moitié extérieure lisse le bord du maillage.
    p.add(PathShape::closed_line(
        pts.to_vec(),
        PathStroke::new(1.2, melange(c0, c1, 0.5)),
    ));
    p.add(Shape::mesh(mesh));
}

/// Ombre douce par couches empilées.
fn ombre(p: &Painter, r: Rect, rayons: [f32; 4], decalage: Vec2, flou: f32, force: f32) {
    let n = 10;
    for i in 0..n {
        let e = flou * (1.0 - i as f32 / n as f32);
        let ra = rayons.map(|x| x + e);
        let a = force / n as f32 * (0.4 + 0.6 * i as f32 / n as f32);
        p.add(Shape::convex_polygon(
            arrondi(r.translate(decalage).expand(e), ra),
            voile(Color32::BLACK, a),
            Stroke::NONE,
        ));
    }
}

/// Lueur ronde (anneaux concentriques transparents).
fn lueur(p: &Painter, c: Pos2, r: f32, largeur: f32, couleur: Color32, force: f32) {
    let n = 8;
    for i in 0..n {
        let f = i as f32 / n as f32;
        p.circle_stroke(
            c,
            r + largeur * f,
            Stroke::new(largeur / n as f32 + 0.5, voile(couleur, force * (1.0 - f))),
        );
    }
}

fn texte(p: &Painter, pos: Pos2, s: &str, taille: f32, angle: f32, couleur: Color32) {
    let g = p.layout_no_wrap(s.into(), FontId::proportional(taille), couleur);
    let ts = g.size();
    // Centré sur `pos`, puis tourné autour de ce centre.
    let (ax, ay) = (
        vec2(angle.cos(), angle.sin()),
        vec2(-angle.sin(), angle.cos()),
    );
    let coin = pos - ax * (ts.x * 0.5) - ay * (ts.y * 0.5);
    p.add(TextShape::new(coin, g, couleur).with_angle(angle));
}

// ----- Dessin -----

/// Dessine la console dans `zone` ; renvoie les zones cliquables (ordre de [`Bouton::TOUS`]).
pub fn dessiner(p: &Painter, zone: Rect, v: &Vue) -> [Rect; 10] {
    let g = geo(zone);
    let s = g.s;
    let allume = |b: Bouton| v.allumes & (1 << b as usize) != 0;
    let pulse = (0.5 + 0.5 * (v.temps * 5.0).sin()) as f32;
    let clignote = |b: Bouton| v.clignote == Some(b) && (v.temps * 8.0).sin() > 0.0;
    let sens = vec2(0.45, 1.0);

    // Boîtier : coin bas droit très arrondi, ombre portée longue.
    let corps = g.r(0.0, 0.0, L, H);
    let rayons = [4.0 * s, 4.0 * s, 22.0 * s, 4.0 * s];
    ombre(p, corps, rayons, vec2(0.0, 5.0 * s), 9.0 * s, 0.55);
    degrade(
        p,
        &arrondi(corps, rayons),
        PLASTIQUE_CLAIR,
        melange(PLASTIQUE, PLASTIQUE_OMBRE, 0.7),
        sens,
    );
    // Liseré de lumière en haut, tranche plus sombre en bas.
    let interieur = arrondi(corps.shrink(0.6 * s), rayons.map(|r| r - 0.6 * s));
    p.add(PathShape::closed_line(
        interieur,
        PathStroke::new(0.5 * s, voile(Color32::WHITE, 0.18)),
    ));

    // Gâchettes L et R : languettes sombres, baie et lueur à l'appui, halo si choisies.
    for (b, (x0, y0, x1, y1), nom) in [(Bouton::L, GACHETTE_L, "L"), (Bouton::R, GACHETTE_R, "R")] {
        let enfonce = allume(b);
        let r = g.r(x0, y0, x1, y1);
        let rr = [1.6 * s; 4];
        if v.choisi == Some(b) || clignote(b) {
            let coul = if clignote(b) { ORANGE } else { HALO };
            for i in 0..6 {
                let f = i as f32 / 6.0;
                p.add(PathShape::closed_line(
                    arrondi(r.expand((0.4 + f * 1.8) * s), rr.map(|x| x + f * 1.8 * s)),
                    PathStroke::new(0.5 * s, voile(coul, (0.35 + 0.55 * pulse) * (1.0 - f))),
                ));
            }
        }
        let r2 = r.translate(vec2(0.0, if enfonce { 0.35 } else { 0.0 } * s));
        if !enfonce {
            ombre(p, r, rr, vec2(0.0, 0.7 * s), 0.9 * s, 0.45);
        }
        let (h, bas) = if enfonce {
            (BAIE_HAUT, BAIE_BAS)
        } else {
            (CROIX_HAUT, CROIX_BAS)
        };
        degrade(p, &arrondi(r2, rr), h, bas, sens);
        if enfonce {
            lueur(p, r2.center(), 2.0 * s, 3.0 * s, BAIE_ALLUMEE, 0.35);
        }
        p.line_segment(
            [
                r2.left_top() + vec2(1.6, 0.5) * s,
                r2.right_top() + vec2(-1.6, 0.5) * s,
            ],
            Stroke::new(0.35 * s, voile(Color32::WHITE, 0.2)),
        );
        texte(
            p,
            r2.center(),
            nom,
            3.0 * s,
            0.0,
            voile(Color32::WHITE, 0.75),
        );
    }

    // Rainure du haut.
    for (dy, c) in [
        (0.0, voile(PLASTIQUE_OMBRE, 0.9)),
        (0.35, voile(Color32::WHITE, 0.35)),
    ] {
        let y = 7.0 + dy;
        p.line_segment([g.p(4.0, y), g.p(L - 4.0, y)], Stroke::new(0.3 * s, c));
    }

    // Cadre de l'écran.
    let cadre_ecran = g.r(7.5, 12.0, L - 7.5, 70.5);
    let rc = [3.0 * s, 3.0 * s, 13.0 * s, 3.0 * s];
    let creux = arrondi(cadre_ecran.expand(0.5 * s), rc.map(|r| r + 0.5 * s));
    p.add(Shape::convex_polygon(
        creux,
        voile(Color32::BLACK, 0.25),
        Stroke::NONE,
    ));
    degrade(p, &arrondi(cadre_ecran, rc), CADRE_HAUT, CADRE_BAS, sens);
    // Deux filets fins (sans texte) en haut du cadre.
    for (y, c) in [
        (15.8, Color32::from_rgb(0x8A, 0x2E, 0x5C)),
        (16.9, Color32::from_rgb(0x2E, 0x34, 0x72)),
    ] {
        p.line_segment([g.p(11.0, y), g.p(54.0, y)], Stroke::new(0.55 * s, c));
    }
    // Voyant d'alimentation.
    let led = g.p(14.2, 39.0);
    p.circle_filled(led, 1.6 * s, voile(Color32::BLACK, 0.35));
    p.circle_filled(
        led,
        1.25 * s,
        if v.allumes != 0 { LED } else { LED_ETEINTE },
    );
    lueur(
        p,
        led,
        1.2 * s,
        2.6 * s,
        LED,
        if v.allumes != 0 { 0.45 } else { 0.0 },
    );
    p.circle_filled(
        led + vec2(-0.4, -0.4) * s,
        0.4 * s,
        voile(Color32::WHITE, 0.45),
    );

    // Écran.
    let ecran = g.r(20.5, 21.0, 69.5, 65.1);
    p.rect_filled(
        ecran.expand(0.7 * s),
        0.6 * s,
        Color32::from_rgb(0x2A, 0x2B, 0x33),
    );
    match v.ecran {
        Some(t) => {
            p.image(
                t,
                ecran,
                Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        None => motif(p, ecran),
    }
    // Reflet en biais sur la vitre.
    let reflet = vec![
        ecran.left_top(),
        ecran.left_top() + vec2(ecran.width() * 0.42, 0.0),
        ecran.left_top() + vec2(ecran.width() * 0.18, ecran.height()),
        ecran.left_bottom(),
    ];
    p.add(Shape::convex_polygon(
        reflet,
        voile(Color32::WHITE, 0.05),
        Stroke::NONE,
    ));
    p.rect_stroke(
        ecran,
        0.0,
        Stroke::new(0.4 * s, voile(Color32::BLACK, 0.45)),
        eframe::egui::StrokeKind::Inside,
    );

    // Croix directionnelle : creux, deux barres, relief.
    let (cx, cy) = CROIX;
    let (d, w) = (CROIX_DEMI, CROIX_BRAS);
    let barre_h = g.r(cx - d, cy - w, cx + d, cy + w);
    let barre_v = g.r(cx - w, cy - d, cx + w, cy + d);
    let rb = [0.9 * s; 4];
    p.circle_filled(
        g.p(cx, cy + 0.6),
        (d + 2.6) * s,
        voile(PLASTIQUE_OMBRE, 0.35),
    );
    p.circle_filled(g.p(cx, cy), (d + 2.1) * s, voile(Color32::BLACK, 0.06));
    for b in [barre_h, barre_v] {
        ombre(p, b, rb, vec2(0.0, 1.4 * s), 1.6 * s, 0.6);
    }
    let pts_h = arrondi(barre_h, rb);
    let pts_v = arrondi(barre_v, rb);
    for pts in [&pts_h, &pts_v] {
        p.add(PathShape::closed_line(
            pts.clone(),
            PathStroke::new(1.4, CROIX_BAS),
        ));
    }
    for pts in [&pts_h, &pts_v] {
        degrade(p, pts, CROIX_HAUT, CROIX_BAS, sens);
    }
    // Branches enfoncées : plus sombres en haut, lueur d'accent.
    let branches = [
        (
            Bouton::Haut,
            g.r(cx - w, cy - d, cx + w, cy - w),
            vec2(0.0, -1.0),
        ),
        (
            Bouton::Bas,
            g.r(cx - w, cy + w, cx + w, cy + d),
            vec2(0.0, 1.0),
        ),
        (
            Bouton::Gauche,
            g.r(cx - d, cy - w, cx - w, cy + w),
            vec2(-1.0, 0.0),
        ),
        (
            Bouton::Droite,
            g.r(cx + w, cy - w, cx + d, cy + w),
            vec2(1.0, 0.0),
        ),
    ];
    for (b, r, dir) in branches {
        let fleche = r.center() + dir * (0.9 * s);
        let perp = vec2(-dir.y, dir.x);
        let tri = vec![
            fleche + dir * (1.4 * s),
            fleche - dir * (0.5 * s) + perp * (1.4 * s),
            fleche - dir * (0.5 * s) - perp * (1.4 * s),
        ];
        let actif = allume(b) || v.choisi == Some(b) || clignote(b);
        let teinte = if clignote(b) {
            ORANGE
        } else if allume(b) {
            BAIE_ALLUMEE
        } else {
            voile(Color32::WHITE, 0.10)
        };
        if actif {
            let force = if allume(b) { 0.85 } else { 0.35 + 0.5 * pulse };
            let coul = if clignote(b) {
                ORANGE
            } else if allume(b) {
                BAIE_ALLUMEE
            } else {
                HALO
            };
            for i in 0..6 {
                let f = i as f32 / 6.0;
                p.add(PathShape::closed_line(
                    arrondi(r.expand((0.6 + 2.4 * f) * s), [(1.2 + 2.4 * f) * s; 4]),
                    PathStroke::new(0.5 * s, voile(coul, 0.6 * force * (1.0 - f))),
                ));
            }
            p.rect_filled(r.shrink(0.25 * s), 0.6 * s, voile(teinte, 0.45 * force));
            p.add(PathShape::closed_line(
                arrondi(r.expand(0.6 * s), [1.2 * s; 4]),
                PathStroke::new(
                    0.5 * s,
                    voile(if clignote(b) { ORANGE } else { HALO }, force),
                ),
            ));
        }
        p.add(Shape::convex_polygon(
            tri,
            if allume(b) {
                voile(BAIE_ALLUMEE, 0.9)
            } else {
                teinte
            },
            Stroke::NONE,
        ));
    }
    // Cuvette centrale et liseré de lumière.
    p.circle_filled(g.p(cx, cy), 2.3 * s, voile(Color32::BLACK, 0.35));
    p.circle_stroke(
        g.p(cx, cy),
        2.3 * s,
        Stroke::new(0.25 * s, voile(Color32::WHITE, 0.10)),
    );
    p.line_segment(
        [g.p(cx - d + 0.8, cy - w + 0.35), g.p(cx - w, cy - w + 0.35)],
        Stroke::new(0.3 * s, voile(Color32::WHITE, 0.16)),
    );
    p.line_segment(
        [g.p(cx - w + 0.35, cy - d + 0.8), g.p(cx - w + 0.35, cy - w)],
        Stroke::new(0.3 * s, voile(Color32::WHITE, 0.16)),
    );

    // Boutons A et B : ronds baie dans une cuvette inclinée commune.
    let ca = g.p(BTN_A.0, BTN_A.1);
    let cb = g.p(BTN_B.0, BTN_B.1);
    let milieu = ca + (cb - ca) * 0.5;
    let demi = (ca - cb).length() * 0.5;
    let ang_ab = (ca - cb).angle();
    p.add(Shape::convex_polygon(
        gelule(milieu + vec2(0.0, 0.5 * s), demi, (BTN_R + 1.7) * s, ang_ab),
        voile(PLASTIQUE_OMBRE, 0.28),
        Stroke::NONE,
    ));
    p.add(Shape::convex_polygon(
        gelule(milieu, demi, (BTN_R + 1.9) * s, ang_ab),
        voile(Color32::BLACK, 0.07),
        Stroke::NONE,
    ));
    for (b, c, nom) in [(Bouton::A, ca, "A"), (Bouton::B, cb, "B")] {
        let enfonce = allume(b);
        let c2 = c + if enfonce {
            vec2(0.0, 0.7 * s)
        } else {
            Vec2::ZERO
        };
        let r = BTN_R * s;
        // Ombre (réduite quand le bouton est enfoncé).
        let po = if enfonce { 0.4 } else { 1.3 };
        for i in 0..6 {
            let f = i as f32 / 6.0;
            p.circle_filled(
                c + vec2(0.0, po * s),
                r + (1.4 - f * 1.4) * s,
                voile(Color32::BLACK, 0.06 + 0.05 * f),
            );
        }
        if v.choisi == Some(b) || clignote(b) {
            let coul = if clignote(b) { ORANGE } else { HALO };
            lueur(p, c2, r + 0.6 * s, 3.4 * s, coul, 0.35 + 0.55 * pulse);
        }
        if enfonce {
            lueur(p, c2, r, 3.0 * s, BAIE_ALLUMEE, 0.75);
        }
        let (haut, bas) = if enfonce {
            (BAIE_ALLUMEE, BAIE_HAUT)
        } else {
            (BAIE_HAUT, BAIE_BAS)
        };
        p.circle_filled(c2, r + 0.6, bas);
        // Dégradé radial décalé vers le haut-gauche.
        let n = 10;
        for i in 0..n {
            let f = i as f32 / n as f32;
            p.circle_filled(
                c2 + vec2(-0.8, -0.9) * s * f,
                r * (1.0 - 0.55 * f),
                melange(bas, haut, f),
            );
        }
        p.add(Shape::ellipse_filled(
            c2 + vec2(-1.7, -2.2) * s,
            vec2(2.0, 1.1) * s,
            voile(Color32::WHITE, if enfonce { 0.10 } else { 0.20 }),
        ));
        p.circle_stroke(c2, r, Stroke::new(0.3 * s, voile(Color32::BLACK, 0.35)));
        texte(p, c + vec2(1.6, 9.6) * s, nom, 4.0 * s, 0.0, ENCRE);
    }

    // Select et Start : gélules grises inclinées dans leur creux, libellés dessous.
    for (b, (x, y), nom) in [
        (Bouton::Select, PASTILLE_SELECT, "SELECT"),
        (Bouton::Start, PASTILLE_START, "START"),
    ] {
        let c = g.p(x, y);
        let enfonce = allume(b);
        p.add(Shape::convex_polygon(
            gelule(c + vec2(0.0, 0.3 * s), 5.0 * s, 2.35 * s, ANGLE),
            voile(PLASTIQUE_OMBRE, 0.85),
            Stroke::NONE,
        ));
        if v.choisi == Some(b) || clignote(b) {
            let coul = if clignote(b) { ORANGE } else { HALO };
            for i in 0..6 {
                let f = i as f32 / 6.0;
                p.add(PathShape::closed_line(
                    gelule(c, 4.2 * s, (1.9 + 0.5 + f * 2.4) * s, ANGLE),
                    PathStroke::new(0.5 * s, voile(coul, (0.35 + 0.55 * pulse) * (1.0 - f))),
                ));
            }
        }
        let c2 = c + if enfonce {
            vec2(0.0, 0.35 * s)
        } else {
            vec2(0.0, -0.2 * s)
        };
        if !enfonce {
            p.add(Shape::convex_polygon(
                gelule(c + vec2(0.0, 0.5 * s), 4.2 * s, 1.75 * s, ANGLE),
                voile(Color32::BLACK, 0.3),
                Stroke::NONE,
            ));
        } else {
            for i in 0..5 {
                let f = i as f32 / 5.0;
                p.add(PathShape::closed_line(
                    gelule(c2, 4.2 * s, (1.9 + f * 2.2) * s, ANGLE),
                    PathStroke::new(0.5 * s, voile(BAIE_ALLUMEE, 0.6 * (1.0 - f))),
                ));
            }
        }
        let (h, bas) = if enfonce {
            (
                Color32::from_rgb(0xC0, 0x6A, 0x92),
                Color32::from_rgb(0x8E, 0x45, 0x6A),
            )
        } else {
            (PASTILLE_HAUT, PASTILLE_BAS)
        };
        degrade(p, &gelule(c2, 4.2 * s, 1.75 * s, ANGLE), h, bas, sens);
        p.add(Shape::convex_polygon(
            gelule(c2 + vec2(-0.2, -0.55) * s, 3.2 * s, 0.45 * s, ANGLE),
            voile(Color32::WHITE, 0.18),
            Stroke::NONE,
        ));
        texte(p, c + vec2(0.8, 4.6) * s, nom, 2.9 * s, ANGLE, ENCRE);
    }

    // Grille du haut-parleur : six fentes en biais.
    let ang_hp = -62.0 * PI / 180.0;
    let ay = vec2(-ang_hp.sin(), ang_hp.cos());
    let centre_hp = g.p(71.0, 127.5);
    for i in 0..6 {
        let decal = (i as f32 - 2.5) * 4.2 * s;
        let c = centre_hp + ay * decal;
        let creux = gelule(c, 7.6 * s, 1.05 * s, ang_hp);
        p.add(Shape::convex_polygon(
            gelule(c + vec2(0.0, 0.35 * s), 7.6 * s, 1.15 * s, ang_hp),
            voile(Color32::WHITE, 0.22),
            Stroke::NONE,
        ));
        p.add(Shape::convex_polygon(
            creux,
            Color32::from_rgb(0x6E, 0x6A, 0x63),
            Stroke::NONE,
        ));
        p.add(Shape::convex_polygon(
            gelule(c + vec2(0.0, -0.25 * s), 7.2 * s, 0.6 * s, ang_hp),
            voile(Color32::from_rgb(0x3E, 0x3B, 0x37), 0.8),
            Stroke::NONE,
        ));
    }
    zones(zone)
}

/// Motif d'attente dans l'écran éteint du jeu : petit paysage en 4 teintes, pixels nets.
fn motif(p: &Painter, r: Rect) {
    let (nx, ny) = (40usize, 36usize);
    let px = r.width() / nx as f32;
    let py = r.height() / ny as f32;
    p.rect_filled(r, 0.0, ECRAN[0]);
    let carre = |x: usize, y: usize, c: usize| {
        let q = Rect::from_min_size(
            pos2(r.left() + x as f32 * px, r.top() + y as f32 * py),
            vec2(px + 0.3, py + 0.3),
        );
        p.rect_filled(q, 0.0, ECRAN[c]);
    };
    // Soleil.
    for y in 0..ny {
        for x in 0..nx {
            let (dx, dy) = (x as f32 - 29.5, y as f32 - 9.5);
            if dx * dx + dy * dy < 16.0 {
                carre(x, y, 1);
            }
        }
    }
    // Collines lointaines puis proches.
    for x in 0..nx {
        let xf = x as f32;
        let loin = (22.0 + 3.0 * (xf * 0.22).sin() + 1.5 * (xf * 0.53).cos()) as usize;
        let pres = (27.0 + 2.5 * (xf * 0.15 + 1.7).sin()) as usize;
        for y in loin.min(ny)..ny {
            carre(x, y, if y >= pres { 3 } else { 2 });
        }
    }
    // Deux nuages.
    for (x0, y0) in [(6usize, 7usize), (15, 12)] {
        for (dx, dy) in [(1, 0), (2, 0), (0, 1), (1, 1), (2, 1), (3, 1), (4, 1)] {
            carre(x0 + dx, y0 + dy, 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zones_dans_la_console_et_disjointes() {
        let zone = Rect::from_min_size(Pos2::ZERO, vec2(600.0, 700.0));
        let c = cadre(zone);
        let z = zones(zone);
        for (i, a) in z.iter().enumerate() {
            assert!(c.contains_rect(*a), "{:?}", Bouton::TOUS[i]);
            for b in &z[i + 1..] {
                assert!(!a.intersects(*b), "{:?}", Bouton::TOUS[i]);
            }
            assert!(a.contains(centre(zone, Bouton::TOUS[i])));
        }
    }
}
