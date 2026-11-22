//! Direction visuelle « la Game Boy la nuit, sous une lampe de bureau » : jetons de couleur
//! (thèmes Nuit et Jour), polices, matières (ombres, grain, lueurs, vignettage), mouvement
//! (courbes et durées uniques) et primitives (cartouches, puces de touches, icônes).
//! Aucune couleur en dur hors de ce fichier : les écrans passent par [`j`].

use crate::biblio::Plateforme;
use eframe::egui::{
    self,
    epaint::{Mesh, PathShape, Vertex, WHITE_UV},
    text::LayoutJob,
    Align, Color32, ColorImage, CornerRadius, FontFamily, FontId, Galley, Id, Painter, Pos2, Rect,
    Shape, Stroke, StrokeKind, TextureHandle, TextureOptions, Vec2,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// Jetons du thème courant (voir [`j`]).
pub struct Jetons {
    pub jour: bool,
    pub fond: Color32,
    pub fond_jeu: Color32,
    /// Lumière chaude de la lampe, posée en halo sur le fond.
    pub lampe: Color32,
    pub surface: Color32,
    pub surface_haute: Color32,
    pub trait_: Color32,
    /// Liseré de lumière sur l'arête haute des cartes.
    pub lisere: Color32,
    pub texte: Color32,
    pub doux: Color32,
    pub discret: Color32,
    pub accent: Color32,
    pub accent_clair: Color32,
    /// Encre sur fond d'accent.
    pub sur_accent: Color32,
    #[allow(dead_code)]
    pub succes: Color32,
    pub alerte: Color32,
    /// Opacité des ombres portées (0-255) et du vignettage.
    pub ombre: u8,
    pub vignette: f32,
}

pub const NUIT: Jetons = Jetons {
    jour: false,
    fond: rgb(0x16, 0x17, 0x1F),
    fond_jeu: rgb(0x11, 0x12, 0x18),
    lampe: rgb(0x3A, 0x30, 0x2C),
    surface: rgb(0x23, 0x25, 0x3A),
    surface_haute: rgb(0x2E, 0x31, 0x50),
    trait_: Color32::from_rgba_premultiplied(0x23, 0x25, 0x37, 0x99),
    lisere: Color32::from_rgba_premultiplied(0x0F, 0x0F, 0x0F, 0x0F),
    texte: rgb(0xF0, 0xEB, 0xE3),
    doux: rgb(0xA2, 0x9F, 0xB8),
    discret: rgb(0x86, 0x84, 0x9F),
    accent: rgb(0xD2, 0x4A, 0x86),
    accent_clair: rgb(0xF0, 0x7A, 0xAE),
    sur_accent: rgb(0xFF, 0xF6, 0xF9),
    succes: rgb(0x5B, 0xBF, 0x8A),
    alerte: rgb(0xE8, 0xA2, 0x3A),
    ombre: 115,
    vignette: 0.35,
};

pub const JOUR: Jetons = Jetons {
    jour: true,
    fond: rgb(0xEC, 0xE8, 0xE1),
    fond_jeu: rgb(0x1A, 0x1B, 0x23),
    lampe: rgb(0xFF, 0xFB, 0xF2),
    surface: rgb(0xF7, 0xF4, 0xEF),
    surface_haute: rgb(0xFF, 0xFF, 0xFF),
    trait_: Color32::from_rgba_premultiplied(0x8A, 0x84, 0x7C, 0x66),
    lisere: Color32::from_rgba_premultiplied(0x99, 0x99, 0x99, 0x99),
    texte: rgb(0x24, 0x22, 0x2E),
    doux: rgb(0x4E, 0x4A, 0x5E),
    discret: rgb(0x6E, 0x6A, 0x7C),
    accent: rgb(0xC2, 0x3C, 0x78),
    accent_clair: rgb(0xE0, 0x6A, 0x9E),
    sur_accent: rgb(0xFF, 0xF6, 0xF9),
    succes: rgb(0x2F, 0x8F, 0x5E),
    alerte: rgb(0xB8, 0x74, 0x12),
    ombre: 46,
    vignette: 0.10,
};

static THEME_JOUR: AtomicBool = AtomicBool::new(false);
static MOUVEMENT_REDUIT: AtomicBool = AtomicBool::new(false);

/// Jetons du thème courant.
pub fn j() -> &'static Jetons {
    if THEME_JOUR.load(Ordering::Relaxed) {
        &JOUR
    } else {
        &NUIT
    }
}

// Alias du thème Nuit, gardés pour l'écran de jeu et la console dessinée (toujours dans la
// pénombre, quel que soit le thème) ; tous ne servent pas à chaque écran.
pub const FOND: Color32 = NUIT.fond;
pub const FOND_JEU: Color32 = NUIT.fond_jeu;
#[allow(dead_code)]
pub const SURFACE: Color32 = NUIT.surface;
#[allow(dead_code)]
pub const TRAIT: Color32 = rgb(0x3A, 0x3D, 0x5C);
pub const TEXTE: Color32 = NUIT.texte;
pub const DOUX: Color32 = NUIT.doux;
#[allow(dead_code)]
pub const DISCRET: Color32 = NUIT.discret;
pub const ACCENT: Color32 = NUIT.accent;
#[allow(dead_code)]
pub const ACCENT_CLAIR: Color32 = NUIT.accent_clair;
#[allow(dead_code)]
pub const SUCCES: Color32 = NUIT.succes;
#[allow(dead_code)]
pub const ALERTE: Color32 = NUIT.alerte;
/// Plastique des consoles et cartouches dessinées (clair, ombre).
#[allow(dead_code)]
pub const PLASTIQUE: Color32 = rgb(0xC9, 0xC4, 0xBC);
#[allow(dead_code)]
pub const PLASTIQUE_OMBRE: Color32 = rgb(0x9E, 0x99, 0x8F);

// ----- Mouvement : une seule courbe « ressort doux », des durées réglées ici -----

/// Déplacement de sélection.
pub const DUREE_SELECTION: f32 = 0.14;
/// Soulèvement d'une carte.
pub const DUREE_LEVEE: f32 = 0.18;
/// Ouverture d'un menu en surimpression.
pub const DUREE_MENU: f32 = 0.22;
/// Écart entre deux tuiles qui apparaissent en cascade.
pub const CASCADE: f32 = 0.03;

/// Option « Réduire les animations » : mouvements coupés, fondus courts gardés.
pub fn reduire_mouvement(oui: bool) {
    MOUVEMENT_REDUIT.store(oui, Ordering::Relaxed);
}

pub fn mouvement_reduit() -> bool {
    MOUVEMENT_REDUIT.load(Ordering::Relaxed)
}

/// Durée d'un mouvement (déplacement, échelle) : 0 si les animations sont réduites.
pub fn duree(s: f32) -> f32 {
    if mouvement_reduit() {
        0.0
    } else {
        s
    }
}

/// Durée d'un fondu : gardé à 80 ms au plus si les animations sont réduites.
pub fn duree_fondu(s: f32) -> f32 {
    if mouvement_reduit() {
        s.min(0.08)
    } else {
        s
    }
}

/// Sortie rapide avec léger dépassement (4 %) : sélections, soulèvements.
pub fn ressort(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let c1 = 1.05;
    let c3 = c1 + 1.0;
    1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
}

/// Sortie douce, sans dépassement : fondus.
pub fn sortie(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// Booléen animé avec la durée de mouvement du projet (0 si réduit), courbe ressort.
pub fn anim(ctx: &egui::Context, id: Id, cible: bool, s: f32) -> f32 {
    let t = ctx.animate_bool_with_time(id, cible, duree(s));
    if cible {
        ressort(t)
    } else {
        sortie(t)
    }
}

pub fn couleur(p: Plateforme) -> Color32 {
    match p {
        Plateforme::Gb => rgb(0x9A, 0xB0, 0x48),
        Plateforme::Gbc => rgb(0x33, 0xA8, 0x9C),
        Plateforme::Gba => rgb(0x7A, 0x70, 0xE0),
    }
}

pub fn regulier(taille: f32) -> FontId {
    FontId::new(taille, FontFamily::Proportional)
}

pub fn moyen(taille: f32) -> FontId {
    FontId::new(taille, FontFamily::Name("moyen".into()))
}

pub fn gras(taille: f32) -> FontId {
    FontId::new(taille, FontFamily::Name("gras".into()))
}

/// Silkscreen : seulement pour les petits tampons rétro (numéro d'état, « ×4 »…), jamais en texte courant.
pub fn pixel(taille: f32) -> FontId {
    FontId::new(taille, FontFamily::Name("pixel".into()))
}

pub fn installer(ctx: &egui::Context) {
    let mut polices = egui::FontDefinitions::default();
    let ajouter = |p: &mut egui::FontDefinitions, nom: &str, octets: &'static [u8]| {
        p.font_data.insert(
            nom.to_string(),
            Arc::new(egui::FontData::from_static(octets)),
        );
    };
    ajouter(
        &mut polices,
        "rubik",
        include_bytes!("../assets/fonts/Rubik-Regular.ttf"),
    );
    ajouter(
        &mut polices,
        "rubik-moyen",
        include_bytes!("../assets/fonts/Rubik-Medium.ttf"),
    );
    ajouter(
        &mut polices,
        "rubik-gras",
        include_bytes!("../assets/fonts/Rubik-Bold.ttf"),
    );
    // Polices d'egui gardées en repli (symboles absents de Rubik).
    let repli = polices
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let famille = |principale: &str| {
        let mut v = vec![principale.to_string()];
        v.extend(repli.iter().cloned());
        v
    };
    polices
        .families
        .insert(FontFamily::Proportional, famille("rubik"));
    polices
        .families
        .insert(FontFamily::Name("moyen".into()), famille("rubik-moyen"));
    polices
        .families
        .insert(FontFamily::Name("gras".into()), famille("rubik-gras"));
    ajouter(
        &mut polices,
        "silkscreen",
        include_bytes!("../assets/fonts/Silkscreen-Regular.ttf"),
    );
    polices
        .families
        .insert(FontFamily::Name("pixel".into()), famille("silkscreen"));
    ctx.set_fonts(polices);
    appliquer_theme(ctx, false);
}

/// Bascule Nuit / Jour : les jetons rendus par [`j`] et les visuels d'egui suivent.
pub fn appliquer_theme(ctx: &egui::Context, jour: bool) {
    THEME_JOUR.store(jour, Ordering::Relaxed);
    let t = j();
    let mut v = if jour {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };
    v.panel_fill = t.fond;
    v.window_fill = t.surface;
    v.extreme_bg_color = t.surface;
    v.override_text_color = Some(t.texte);
    v.selection.bg_fill = t.accent;
    ctx.set_visuals(v);
}

/// Texte sur une ou plusieurs lignes, coupé par « … » au-delà de `lignes`.
pub fn bloc(
    painter: &Painter,
    texte: &str,
    police: FontId,
    couleur: Color32,
    largeur: f32,
    lignes: usize,
) -> Arc<Galley> {
    let mut job = LayoutJob::simple(texte.to_string(), police, couleur, largeur);
    job.wrap.max_rows = lignes;
    job.wrap.break_anywhere = false;
    job.wrap.overflow_character = Some('…');
    job.halign = Align::LEFT;
    painter.layout_job(job)
}

pub fn ligne(painter: &Painter, texte: &str, police: FontId, couleur: Color32) -> Arc<Galley> {
    painter.layout_no_wrap(texte.to_string(), police, couleur)
}

/// Assombrit (f < 1) ou éclaircit (f > 1) une couleur.
pub fn teinte(c: Color32, f: f32) -> Color32 {
    let m = |x: u8| ((x as f32 * f).round().clamp(0.0, 255.0)) as u8;
    Color32::from_rgba_unmultiplied(m(c.r()), m(c.g()), m(c.b()), c.a())
}

pub fn alpha(c: Color32, a: f32) -> Color32 {
    c.gamma_multiply(a.clamp(0.0, 1.0))
}

/// Points d'un rectangle arrondi dont le coin haut-droit est coupé en biais (cartouche GB).
fn contour_cartouche(r: Rect, rayon: f32, coupe: f32) -> Vec<Pos2> {
    let mut pts = Vec::with_capacity(32);
    let arc = |pts: &mut Vec<Pos2>, c: Pos2, a0: f32, a1: f32| {
        for i in 0..=6 {
            let a = a0 + (a1 - a0) * i as f32 / 6.0;
            pts.push(c + Vec2::new(a.cos(), a.sin()) * rayon);
        }
    };
    use std::f32::consts::{FRAC_PI_2, PI};
    // Haut-gauche, puis coin coupé, bas-droit, bas-gauche (sens horaire, y vers le bas).
    arc(
        &mut pts,
        Pos2::new(r.left() + rayon, r.top() + rayon),
        PI,
        PI + FRAC_PI_2,
    );
    if coupe > 0.0 {
        pts.push(Pos2::new(r.right() - coupe, r.top()));
        pts.push(Pos2::new(r.right(), r.top() + coupe));
    } else {
        arc(
            &mut pts,
            Pos2::new(r.right() - rayon, r.top() + rayon),
            -FRAC_PI_2,
            0.0,
        );
    }
    arc(
        &mut pts,
        Pos2::new(r.right() - rayon, r.bottom() - rayon),
        0.0,
        FRAC_PI_2,
    );
    arc(
        &mut pts,
        Pos2::new(r.left() + rayon, r.bottom() - rayon),
        FRAC_PI_2,
        PI,
    );
    pts
}

/// Carte de repli : une cartouche dessinée (tranche, stries, flèche moulée, étiquette en creux),
/// étiquette à la couleur de la plateforme.
pub fn cartouche(painter: &Painter, zone: Rect, p: Plateforme, titre: &str) {
    let cote = zone.width().min(zone.height());
    let (l, h) = match p {
        Plateforme::Gba => (cote * 0.74, cote * 0.74 * 0.62),
        _ => (cote * 0.60, cote * 0.60 * 1.14),
    };
    let epaisseur = (cote * 0.028).max(2.0);
    let corps = Rect::from_center_size(
        zone.center() - Vec2::new(0.0, epaisseur * 0.5),
        Vec2::new(l, h),
    );
    let (plastique, sombre) = match p {
        Plateforme::Gb => (rgb(0xBD, 0xB8, 0xB2), rgb(0x8E, 0x89, 0x84)),
        Plateforme::Gbc => (rgb(0x4A, 0x4C, 0x60), rgb(0x2F, 0x31, 0x40)),
        Plateforme::Gba => (rgb(0x55, 0x57, 0x6C), rgb(0x38, 0x3A, 0x4C)),
    };
    let creux = teinte(plastique, 0.84);
    let rayon = l * 0.05;
    let coupe = if p == Plateforme::Gba { 0.0 } else { l * 0.14 };

    // Ombre douce au sol, puis tranche (épaisseur du plastique), puis face avant.
    painter.add(
        egui::epaint::Shadow {
            offset: [0, (epaisseur * 2.0) as i8],
            blur: (cote * 0.09) as u8,
            spread: 0,
            color: Color32::from_black_alpha(90),
        }
        .as_shape(corps.shrink(l * 0.06), CornerRadius::same(rayon as u8)),
    );
    painter.add(Shape::Path(PathShape::convex_polygon(
        contour_cartouche(corps.translate(Vec2::new(0.0, epaisseur)), rayon, coupe),
        sombre,
        Stroke::NONE,
    )));
    painter.add(Shape::Path(PathShape::convex_polygon(
        contour_cartouche(corps, rayon, coupe),
        plastique,
        Stroke::NONE,
    )));

    // Stries de prise en main sur le haut de la face (pas sur la GBA, plus plate).
    let trait_strie = Stroke::new((h * 0.014).max(1.0), creux);
    let reflet = Stroke::new((h * 0.008).max(0.6), teinte(plastique, 1.08));
    if p != Plateforme::Gba {
        for i in 0..4 {
            let y = corps.top() + h * (0.06 + 0.04 * i as f32);
            let x0 = corps.left() + l * 0.08;
            let x1 = corps.right() - coupe - l * 0.04 + (y - corps.top()).min(coupe);
            painter.line_segment([Pos2::new(x0, y), Pos2::new(x1, y)], trait_strie);
            painter.line_segment(
                [
                    Pos2::new(x0, y + trait_strie.width),
                    Pos2::new(x1, y + trait_strie.width),
                ],
                reflet,
            );
        }
    }

    let etiquette = match p {
        Plateforme::Gba => Rect::from_min_max(
            Pos2::new(corps.left() + l * 0.10, corps.top() + h * 0.14),
            Pos2::new(corps.right() - l * 0.10, corps.bottom() - h * 0.24),
        ),
        _ => Rect::from_min_max(
            Pos2::new(corps.left() + l * 0.09, corps.top() + h * 0.30),
            Pos2::new(corps.right() - l * 0.09, corps.bottom() - h * 0.07),
        ),
    };
    // Logement en creux de l'étiquette.
    let r_et = CornerRadius::same((l * 0.03).round().clamp(2.0, 6.0) as u8);
    painter.rect_filled(etiquette.expand(l * 0.025), r_et, creux);

    // Flèche moulée (sens d'insertion), entre les stries et l'étiquette.
    let fy = if p == Plateforme::Gba {
        corps.bottom() - h * 0.12
    } else {
        corps.top() + h * 0.235
    };
    let fl = l * 0.045;
    painter.add(Shape::convex_polygon(
        vec![
            Pos2::new(corps.center().x - fl, fy - fl * 0.55),
            Pos2::new(corps.center().x + fl, fy - fl * 0.55),
            Pos2::new(corps.center().x, fy + fl * 0.55),
        ],
        creux,
        Stroke::NONE,
    ));

    let c = couleur_etiquette(p, titre);
    painter.rect_filled(etiquette, r_et, c);
    let bandeau = Rect::from_min_size(
        etiquette.min,
        Vec2::new(etiquette.width(), etiquette.height() * 0.15),
    );
    painter.rect_filled(
        bandeau,
        CornerRadius {
            nw: r_et.nw,
            ne: r_et.ne,
            sw: 0,
            se: 0,
        },
        teinte(c, 0.70),
    );

    let lum = 0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32;
    let encre = if lum < 150.0 {
        rgb(0xF4, 0xF1, 0xFF)
    } else {
        rgb(0x17, 0x19, 0x20)
    };
    let marge = etiquette.width() * 0.09;
    let taille = (etiquette.width() / 6.4)
        .min(etiquette.height() / 3.6)
        .clamp(9.0, 18.0);
    let g = bloc(
        painter,
        titre,
        moyen(taille),
        encre,
        etiquette.width() - 2.0 * marge,
        3,
    );
    let haut = bandeau.bottom() + (etiquette.bottom() - bandeau.bottom() - g.size().y) * 0.45;
    painter.galley(Pos2::new(etiquette.left() + marge, haut), g, encre);
}

/// Étiquette d'une cartouche de repli : une couleur d'époque tirée du titre (toujours la même
/// pour un jeu), pour que la grille ne soit pas un mur uniforme ; la plateforme reste lisible
/// par le plastique et la pastille sous la carte. Titre vide : couleur de la plateforme.
fn couleur_etiquette(p: Plateforme, titre: &str) -> Color32 {
    const ETIQUETTES: [Color32; 5] = [
        rgb(0xC8, 0x64, 0x3C),
        rgb(0xD9, 0xA4, 0x41),
        rgb(0x5E, 0x80, 0xA8),
        rgb(0x86, 0xA8, 0x8E),
        rgb(0x9A, 0x5C, 0x8A),
    ];
    if titre.is_empty() {
        return couleur(p);
    }
    let h = titre.bytes().fold(0x811C_9DC5u32, |h, o| {
        (h ^ o as u32).wrapping_mul(0x0100_0193)
    });
    match h % 7 {
        k @ 0..=4 => ETIQUETTES[k as usize],
        _ => couleur(p),
    }
}

/// Puce de touche (« Entrée », « F11 »…) suivie de son action. Rend la largeur occupée.
pub fn puce(painter: &Painter, pos: Pos2, touche: &str, action: &str) -> f32 {
    puce_avec(painter, pos, touche, action, j())
}

/// Largeur qu'occupera [`puce`] (pour mettre en page avant de dessiner).
pub fn largeur_puce(painter: &Painter, touche: &str, action: &str) -> f32 {
    let t = j();
    ligne(painter, touche, moyen(12.5), t.texte).size().x
        + 22.0
        + ligne(painter, action, regulier(13.5), t.doux).size().x
}

/// [`puce`] aux couleurs d'un thème donné : [`NUIT`] sur le verre sombre du jeu, quel que
/// soit le thème choisi.
pub fn puce_avec(painter: &Painter, pos: Pos2, touche: &str, action: &str, t: &Jetons) -> f32 {
    let gt = ligne(painter, touche, moyen(12.5), t.texte);
    let ga = ligne(painter, action, regulier(13.5), t.doux);
    let r = Rect::from_min_size(
        Pos2::new(pos.x, pos.y - 11.0),
        Vec2::new(gt.size().x + 14.0, 22.0),
    );
    painter.rect_filled(r, CornerRadius::same(6), t.surface_haute);
    painter.rect_stroke(
        r,
        CornerRadius::same(6),
        Stroke::new(1.0, t.trait_),
        StrokeKind::Inside,
    );
    painter.galley(
        Pos2::new(r.left() + 7.0, r.center().y - gt.size().y * 0.5),
        gt,
        t.texte,
    );
    let xa = r.right() + 8.0;
    let wa = ga.size().x;
    painter.galley(Pos2::new(xa, pos.y - ga.size().y * 0.5), ga, t.doux);
    xa + wa - pos.x
}

/// Icône de la fenêtre et de la barre des tâches (la même que celle de l'exe Windows).
pub fn icone_fenetre() -> egui::IconData {
    let mut dec = png::Decoder::new(std::io::Cursor::new(include_bytes!(
        "../assets/icone-64.png"
    )));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let lu = dec.read_info().ok().and_then(|mut l| {
        let mut rgba = vec![0; l.output_buffer_size()];
        let info = l.next_frame(&mut rgba).ok()?;
        rgba.truncate(info.buffer_size());
        (info.color_type == png::ColorType::Rgba).then_some((rgba, info.width, info.height))
    });
    let (rgba, width, height) = lu.unwrap_or_default();
    egui::IconData {
        rgba,
        width,
        height,
    }
}

// ----- Matières : ombres, panneaux, lueurs, vignettage, grain -----

/// Ombre portée douce sous un rectangle arrondi (`force` 0-1 module l'opacité du thème).
pub fn ombre(p: &Painter, r: Rect, rayon: f32, dy: f32, flou: f32, force: f32) {
    let a = (j().ombre as f32 * force.clamp(0.0, 1.5)).min(255.0) as u8;
    if a == 0 {
        return;
    }
    p.add(
        egui::epaint::Shadow {
            offset: [0, dy.clamp(-127.0, 127.0) as i8],
            blur: flou.clamp(0.0, 255.0) as u8,
            spread: 0,
            color: Color32::from_black_alpha(a),
        }
        .as_shape(r, CornerRadius::same(rayon.clamp(0.0, 255.0) as u8)),
    );
}

/// Panneau en matière : dégradé vertical léger (haut un peu plus clair), liseré de lumière
/// sur l'arête haute, bordure fine facultative.
pub fn panneau(p: &Painter, r: Rect, rayon: f32, couleur: Color32, bord: Option<Color32>) {
    let t = j();
    let rayon = rayon.min(r.width() * 0.5).min(r.height() * 0.5);
    let pts = contour_cartouche(r, rayon, 0.0);
    let haut = teinte(couleur, if t.jour { 1.0 } else { 1.07 });
    let bas = teinte(couleur, if t.jour { 0.97 } else { 0.96 });
    let mut m = Mesh::default();
    let couleur_a = |y: f32| haut.lerp_to_gamma(bas, ((y - r.top()) / r.height()).clamp(0.0, 1.0));
    m.vertices.push(Vertex {
        pos: r.center(),
        uv: WHITE_UV,
        color: couleur_a(r.center().y),
    });
    for &q in &pts {
        m.vertices.push(Vertex {
            pos: q,
            uv: WHITE_UV,
            color: couleur_a(q.y),
        });
    }
    let n = pts.len() as u32;
    for i in 0..n {
        m.indices.extend_from_slice(&[0, 1 + i, 1 + (i + 1) % n]);
    }
    p.add(Shape::mesh(m));
    // Le contour lissé (anticrénelé) recouvre le bord en escalier du maillage.
    let contour = bord.unwrap_or(couleur);
    p.add(Shape::Path(PathShape::closed_line(
        pts.clone(),
        Stroke::new(1.0, contour),
    )));
    // Liseré : les deux arcs du haut (14 premiers points du contour).
    let arete: Vec<Pos2> = contour_cartouche(r.shrink(1.0), (rayon - 1.0).max(0.0), 0.0)
        .into_iter()
        .take(14)
        .collect();
    p.add(Shape::Path(PathShape::line(
        arete,
        Stroke::new(1.0, t.lisere),
    )));
}

/// Lueur radiale (ellipse de demi-axes `rayon`) : `couleur` au centre, transparente au bord.
pub fn lueur(p: &Painter, centre: Pos2, rayon: Vec2, couleur: Color32) {
    if couleur.a() == 0 || rayon.x < 1.0 || rayon.y < 1.0 {
        return;
    }
    grille_radiale(p, Rect::from_center_size(centre, rayon * 2.0), 18, |d| {
        let f = (1.0 - d * d).max(0.0);
        couleur.gamma_multiply(f * f)
    });
}

/// Vignettage : assombrit doucement les bords de `r`.
pub fn vignette(p: &Painter, r: Rect) {
    let force = j().vignette;
    grille_radiale(p, r, 14, |d| {
        let x = ((d - 0.55) / 0.75).clamp(0.0, 1.0);
        Color32::from_black_alpha((255.0 * force * x * x * (3.0 - 2.0 * x)) as u8)
    });
}

/// Grille de sommets sur `r` colorés selon la distance elliptique normalisée au centre.
fn grille_radiale(p: &Painter, r: Rect, n: u32, couleur: impl Fn(f32) -> Color32) {
    let mut m = Mesh::default();
    let c = r.center();
    let demi = r.size() * 0.5;
    for jy in 0..=n {
        for ix in 0..=n {
            let pos = Pos2::new(
                r.left() + r.width() * ix as f32 / n as f32,
                r.top() + r.height() * jy as f32 / n as f32,
            );
            let d = ((pos - c) / demi).length();
            m.vertices.push(Vertex {
                pos,
                uv: WHITE_UV,
                color: couleur(d),
            });
        }
    }
    let l = n + 1;
    for jy in 0..n {
        for ix in 0..n {
            let a = jy * l + ix;
            m.indices
                .extend_from_slice(&[a, a + 1, a + l + 1, a, a + l + 1, a + l]);
        }
    }
    p.add(Shape::mesh(m));
}

/// Grain de pellicule (bruit 256×256 généré une fois, sans fichier) mélangé sur `r` :
/// casse l'aplat numérique du fond.
pub fn grain(ctx: &egui::Context, p: &Painter, r: Rect, force: f32) {
    let id = Id::unique("dessin-grain");
    let tex = ctx.data(|d| d.get_temp::<TextureHandle>(id));
    let tex = tex.unwrap_or_else(|| {
        let mut graine: u32 = 0x9E37_79B9;
        let mut px = Vec::with_capacity(256 * 256);
        for _ in 0..256 * 256 {
            // xorshift32 : rapide et suffisant pour du bruit visuel.
            graine ^= graine << 13;
            graine ^= graine >> 17;
            graine ^= graine << 5;
            let a = (graine & 0xFF) as u8;
            // Moitié clair, moitié sombre : le fond ne s'éclaircit pas en moyenne.
            px.push(if graine & 0x100 != 0 {
                Color32::from_rgba_premultiplied(a, a, a, a)
            } else {
                Color32::from_rgba_premultiplied(0, 0, 0, a)
            });
        }
        let img = ColorImage::new([256, 256], px);
        let t = ctx.load_texture("grain", img, TextureOptions::NEAREST_REPEAT);
        ctx.data_mut(|d| d.insert_temp(id, t.clone()));
        t
    });
    let ppp = ctx.pixels_per_point();
    let uv = Rect::from_min_max(
        Pos2::ZERO,
        Pos2::new(r.width() * ppp / 256.0, r.height() * ppp / 256.0),
    );
    p.image(tex.id(), r, uv, Color32::WHITE.gamma_multiply(force));
}

// ----- Icônes vectorielles (traits arrondis, aucune police d'icônes) -----

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Icone {
    Jouer,
    Ecran,
    Palette,
    PleinEcran,
    Maison,
    Etoile,
    EtoileVide,
    Horloge,
    /// Cartouche (emplacements d'état).
    Etats,
}

/// Icône dessinée dans un carré de côté `taille` centré sur `c`.
pub fn icone(p: &Painter, genre: Icone, c: Pos2, taille: f32, couleur: Color32) {
    let s = taille / 24.0;
    let tr = Stroke::new((2.0 * s).max(1.4), couleur);
    let q = |x: f32, y: f32| c + Vec2::new(x, y) * s;
    match genre {
        Icone::Jouer => {
            p.add(Shape::convex_polygon(
                vec![q(-5.0, -8.0), q(8.0, 0.0), q(-5.0, 8.0)],
                couleur,
                Stroke::new(1.5 * s, couleur),
            ));
        }
        Icone::Ecran => {
            // Petit écran LCD : cadre et quatre pixels allumés.
            p.rect_stroke(
                Rect::from_center_size(c, Vec2::new(20.0, 16.0) * s),
                CornerRadius::same((3.0 * s) as u8),
                tr,
                StrokeKind::Middle,
            );
            for (x, y) in [(-3.0, -2.0), (1.0, -2.0), (-3.0, 2.0), (1.0, 2.0)] {
                p.rect_filled(
                    Rect::from_min_size(q(x, y - 1.0), Vec2::splat(2.6 * s)),
                    CornerRadius::ZERO,
                    couleur,
                );
            }
        }
        Icone::Palette => {
            // Quatre teintes en dégradé, comme l'écran d'une DMG.
            for k in 0..4 {
                let r =
                    Rect::from_min_size(q(-10.0 + k as f32 * 5.2, -5.0), Vec2::new(4.2, 10.0) * s);
                p.rect_filled(
                    r,
                    CornerRadius::same((1.5 * s) as u8),
                    couleur.gamma_multiply(1.0 - k as f32 * 0.24),
                );
            }
        }
        Icone::PleinEcran => {
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let coin = q(9.0 * sx, 8.0 * sy);
                p.line_segment([coin, coin - Vec2::new(5.0 * sx, 0.0) * s], tr);
                p.line_segment([coin, coin - Vec2::new(0.0, 5.0 * sy) * s], tr);
            }
        }
        Icone::Maison => {
            p.add(Shape::Path(PathShape::line(
                vec![q(-9.0, -1.0), q(0.0, -9.0), q(9.0, -1.0)],
                tr,
            )));
            p.add(Shape::Path(PathShape::line(
                vec![q(-6.5, -3.0), q(-6.5, 8.0), q(6.5, 8.0), q(6.5, -3.0)],
                tr,
            )));
            p.line_segment([q(-1.8, 8.0), q(-1.8, 3.0)], tr);
            p.line_segment([q(-1.8, 3.0), q(1.8, 3.0)], tr);
            p.line_segment([q(1.8, 3.0), q(1.8, 8.0)], tr);
        }
        Icone::Etats => {
            p.add(Shape::Path(PathShape::closed_line(
                vec![
                    q(-8.0, -10.0),
                    q(5.0, -10.0),
                    q(8.0, -7.0),
                    q(8.0, 10.0),
                    q(-8.0, 10.0),
                ],
                tr,
            )));
            p.rect_filled(
                Rect::from_min_max(q(-4.5, -1.0), q(4.5, 6.5)),
                CornerRadius::same((1.5 * s) as u8),
                couleur,
            );
            p.line_segment([q(-4.5, -6.0), q(3.0, -6.0)], tr);
        }
        Icone::Horloge => {
            p.circle_stroke(c, 9.0 * s, tr);
            p.line_segment([c, q(0.0, -5.5)], tr);
            p.line_segment([c, q(4.0, 2.0)], tr);
        }
        Icone::Etoile | Icone::EtoileVide => {
            let pts: Vec<Pos2> = (0..10)
                .map(|k| {
                    let a = -std::f32::consts::FRAC_PI_2 + k as f32 * std::f32::consts::PI / 5.0;
                    let r = if k % 2 == 0 { 10.0 } else { 4.3 };
                    q(a.cos() * r, a.sin() * r + 0.8)
                })
                .collect();
            if genre == Icone::Etoile {
                // Étoile concave : remplie en éventail depuis le centre.
                let mut m = Mesh::default();
                m.vertices.push(Vertex {
                    pos: q(0.0, 0.8),
                    uv: WHITE_UV,
                    color: couleur,
                });
                for &pt in &pts {
                    m.vertices.push(Vertex {
                        pos: pt,
                        uv: WHITE_UV,
                        color: couleur,
                    });
                }
                for k in 0..10u32 {
                    m.indices.extend_from_slice(&[0, 1 + k, 1 + (k + 1) % 10]);
                }
                p.add(Shape::mesh(m));
            }
            p.add(Shape::Path(PathShape::closed_line(
                pts,
                Stroke::new((1.4 * s).max(1.0), couleur),
            )));
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn icone_lisible() {
        let i = super::icone_fenetre();
        assert_eq!((i.width, i.height, i.rgba.len()), (64, 64, 64 * 64 * 4));
    }
}
