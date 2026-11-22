//! Message bref en bas de l'écran.

use super::*;

impl App {
    pub(super) fn dire(&mut self, texte: impl Into<String>) {
        self.toast = Some((texte.into(), self.t_prec + 3.2));
    }

    pub(super) fn dessiner_toast(&self, ui: &egui::Ui) {
        let Some((texte, fin)) = &self.toast else {
            return;
        };
        let reste = (*fin - self.t_prec) as f32;
        if reste <= 0.0 {
            return;
        }
        let jt = j();
        let a = reste.min(0.3) / 0.3;
        let ecran = ui.max_rect();
        let mut p = ui.painter().clone();
        p.multiply_opacity(a);
        let g = ligne(&p, texte, moyen(15.0), jt.texte);
        let gs = g.size();
        let b = Rect::from_center_size(
            Pos2::new(
                ecran.center().x,
                ecran.bottom() - HAUT_PIED - 34.0 + (1.0 - a) * 8.0,
            ),
            Vec2::new(gs.x + 36.0, 42.0),
        );
        p.add(
            egui::epaint::Shadow {
                offset: [0, 8],
                blur: 24,
                spread: 0,
                color: Color32::from_black_alpha(jt.ombre),
            }
            .as_shape(b, CornerRadius::same(21)),
        );
        p.rect_filled(b, CornerRadius::same(21), jt.surface_haute);
        p.rect_stroke(
            b,
            CornerRadius::same(21),
            Stroke::new(1.0, jt.trait_),
            StrokeKind::Inside,
        );
        p.galley(
            Pos2::new(b.left() + 18.0, b.center().y - gs.y * 0.5),
            g,
            jt.texte,
        );
        ui.ctx().request_repaint();
    }
}
