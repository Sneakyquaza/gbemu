//! Direction visuelle : couleurs, polices, et primitives de dessin (cartouches, puces de touches).
//! Fond « boîtier de nuit » ardoise-indigo, accent baie (les boutons A/B de la DMG),
//! une couleur par plateforme, typographie Rubik (arrondie comme le plastique des consoles).

use crate::biblio::Plateforme;
use eframe::egui::{
    self, epaint::PathShape, text::LayoutJob, Align, Color32, CornerRadius, FontFamily, FontId,
    Galley, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2,
};
use std::sync::Arc;

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub const FOND: Color32 = rgb(0x1C, 0x1E, 0x2B);
pub const FOND_JEU: Color32 = rgb(0x15, 0x16, 0x20);
pub const SURFACE: Color32 = rgb(0x26, 0x28, 0x39);
pub const SURFACE_HAUTE: Color32 = rgb(0x32, 0x34, 0x49);
pub const TRAIT: Color32 = rgb(0x3B, 0x3D, 0x54);
pub const TEXTE: Color32 = rgb(0xEC, 0xE7, 0xDF);
pub const DOUX: Color32 = rgb(0x9A, 0x97, 0xAE);
pub const DISCRET: Color32 = rgb(0x6A, 0x68, 0x80);
pub const ACCENT: Color32 = rgb(0xD0, 0x4A, 0x84);

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
    ctx.set_fonts(polices);

    let mut v = egui::Visuals::dark();
    v.panel_fill = FOND;
    v.window_fill = SURFACE;
    v.extreme_bg_color = SURFACE;
    v.override_text_color = Some(TEXTE);
    v.selection.bg_fill = ACCENT;
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

    let c = couleur(p);
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

    let encre = if p == Plateforme::Gba {
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

/// Puce de touche (« Entrée », « F11 »…) suivie de son action. Rend la largeur occupée.
pub fn puce(painter: &Painter, pos: Pos2, touche: &str, action: &str) -> f32 {
    let gt = ligne(painter, touche, moyen(12.5), TEXTE);
    let ga = ligne(painter, action, regulier(13.5), DOUX);
    let r = Rect::from_min_size(
        Pos2::new(pos.x, pos.y - 11.0),
        Vec2::new(gt.size().x + 14.0, 22.0),
    );
    painter.rect_filled(r, CornerRadius::same(6), SURFACE_HAUTE);
    painter.rect_stroke(
        r,
        CornerRadius::same(6),
        Stroke::new(1.0, TRAIT),
        StrokeKind::Inside,
    );
    painter.galley(
        Pos2::new(r.left() + 7.0, r.center().y - gt.size().y * 0.5),
        gt,
        TEXTE,
    );
    let xa = r.right() + 8.0;
    let wa = ga.size().x;
    painter.galley(Pos2::new(xa, pos.y - ga.size().y * 0.5), ga, DOUX);
    xa + wa - pos.x
}
