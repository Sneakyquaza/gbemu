//! Sprites (OBJ) : 128 entrées OAM, tailles 8 à 64, 4/8 bpp, correspondance 1D/2D, affines
//! (double taille), semi-transparents, fenêtre OBJ, mosaïque, limite de cycles par ligne.

use super::{LigneObj, Ppu, DISPCNT, LARGEUR, MOSAIC, VIDE};

/// (largeur, hauteur) par forme (carré, large, haut) et taille.
const TAILLES: [[(i32, i32); 4]; 3] = [
    [(8, 8), (16, 16), (32, 32), (64, 64)],
    [(16, 8), (32, 8), (32, 16), (64, 32)],
    [(8, 16), (8, 32), (16, 32), (32, 64)],
];

impl Ppu {
    #[inline]
    fn oam16(&self, o: usize) -> u16 {
        u16::from_le_bytes([self.oam[o], self.oam[o + 1]])
    }

    /// Sprites de la ligne `y`, de l'entrée 0 (devant à priorité égale) à 127.
    pub(super) fn ligne_objets(&self, y: usize, out: &mut LigneObj) {
        let dispcnt = self.regs[DISPCNT];
        let map1d = dispcnt & 0x40 != 0;
        // En mode bitmap, les 16 premiers Kio de la VRAM des sprites servent aux fonds.
        let debut_vram = if dispcnt & 7 >= 3 { 0x14000 } else { 0x10000 };
        let mos = self.regs[MOSAIC];
        let mos_h = ((mos >> 8) & 0xF) as usize + 1;
        let mos_v = ((mos >> 12) & 0xF) as i32 + 1;
        // raccourci : budget de cycles décompté par sprite entier (le sprite qui dépasse n'est
        // pas dessiné en partie) et sans coût des entrées hors ligne.
        let mut budget: i32 = if dispcnt & 0x20 != 0 { 954 } else { 1210 };
        let y = y as i32;
        for i in 0..128 {
            let a0 = self.oam16(i * 8);
            let affine = a0 & 0x100 != 0;
            if !affine && a0 & 0x200 != 0 {
                continue; // désactivé
            }
            let mode = (a0 >> 10) & 3;
            let forme = (a0 >> 14) as usize;
            if mode == 3 || forme == 3 {
                continue;
            }
            let a1 = self.oam16(i * 8 + 2);
            let a2 = self.oam16(i * 8 + 4) as usize;
            let (w, h) = TAILLES[forme][(a1 >> 14) as usize];
            let double = affine && a0 & 0x200 != 0;
            let (bw, bh) = if double { (w * 2, h * 2) } else { (w, h) };
            let mut iy = (y - (a0 & 0xFF) as i32) & 0xFF;
            if iy >= bh {
                continue;
            }
            let cout = if affine { 10 + 2 * bw } else { bw };
            if cout > budget {
                break;
            }
            budget -= cout;
            let mosaique = a0 & 0x1000 != 0;
            if mosaique {
                iy = (iy - y % mos_v).max(0);
            }
            let ox = (((a1 & 0x1FF) << 7) as i16 >> 7) as i32;
            let c256 = a0 & 0x2000 != 0;
            let tuile = a2 & 0x3FF;
            let prio = ((a2 >> 10) & 3) as u8;
            let pal = if c256 { 0x100 } else { 0x100 + (a2 >> 12) * 16 };
            // Pas en tuiles entre deux rangées de 8 lignes.
            let rangee = if map1d {
                (w as usize / 8) * if c256 { 2 } else { 1 }
            } else {
                32
            };
            let texel = |tx: i32, ty: i32| -> usize {
                let (tx, ty) = (tx as usize, ty as usize);
                let t = tuile + (ty / 8) * rangee + (tx / 8) * if c256 { 2 } else { 1 };
                let a = 0x10000 + ((t * 32) & 0x7FFF);
                if a < debut_vram {
                    return 0;
                }
                if c256 {
                    self.vram[a + (ty % 8) * 8 + tx % 8] as usize
                } else {
                    let o = self.vram[a + (ty % 8) * 4 + (tx % 8) / 2];
                    ((o >> ((tx & 1) * 4)) & 15) as usize
                }
            };
            let x0 = (-ox).max(0);
            let x1 = (LARGEUR as i32 - ox).min(bw);
            if affine {
                let g = ((a1 >> 9) & 31) as usize * 32;
                let pa = self.oam16(g + 6) as i16 as i32;
                let pb = self.oam16(g + 14) as i16 as i32;
                let pc = self.oam16(g + 22) as i16 as i32;
                let pd = self.oam16(g + 30) as i16 as i32;
                let ry = iy - bh / 2;
                for ix in x0..x1 {
                    let rx = ix - bw / 2;
                    let tx = ((pa * rx + pb * ry) >> 8) + w / 2;
                    let ty = ((pc * rx + pd * ry) >> 8) + h / 2;
                    if (0..w).contains(&tx) && (0..h).contains(&ty) {
                        let c = texel(tx, ty);
                        self.pose(out, (ox + ix) as usize, c, pal, prio, mode, mosaique);
                    }
                }
            } else {
                let ty = if a1 & 0x2000 != 0 { h - 1 - iy } else { iy };
                let hflip = a1 & 0x1000 != 0;
                // Rangée de texels décodée tuile par tuile.
                let mut rang = [0u8; 64];
                for k in 0..w as usize / 8 {
                    let t = tuile + (ty as usize / 8) * rangee + k * if c256 { 2 } else { 1 };
                    let a = 0x10000 + ((t * 32) & 0x7FFF);
                    if a < debut_vram {
                        continue;
                    }
                    let r = &mut rang[k * 8..k * 8 + 8];
                    if c256 {
                        let a = a + (ty as usize % 8) * 8;
                        r.copy_from_slice(&self.vram[a..a + 8]);
                    } else {
                        let a = a + (ty as usize % 8) * 4;
                        for (j, &o) in self.vram[a..a + 4].iter().enumerate() {
                            r[j * 2] = o & 15;
                            r[j * 2 + 1] = o >> 4;
                        }
                    }
                }
                for ix in x0..x1 {
                    let tx = if hflip { w - 1 - ix } else { ix };
                    let c = rang[tx as usize] as usize;
                    if c != 0 {
                        self.pose(out, (ox + ix) as usize, c, pal, prio, mode, mosaique);
                    } else if mosaique {
                        self.pose(out, (ox + ix) as usize, 0, pal, prio, mode, mosaique);
                    }
                }
            }
        }
        // Mosaïque horizontale sur la ligne des sprites, grille de l'écran : un point marqué
        // reprend le point précédent hors début de bloc.
        if mos_h > 1 {
            for x in 1..LARGEUR {
                let (m, mp) = (out.mosaique[x], out.mosaique[x - 1]);
                if m != 0 && mp != 0 && mp <= m && x % mos_h != 0 {
                    out.mosaique[x] = mp;
                    out.coul[x] = out.coul[x - 1];
                    out.prio[x] = out.prio[x - 1];
                    out.semi[x] = out.semi[x - 1];
                }
            }
        }
    }

    /// Pose un point de sprite (index de couleur `c`, 0 = transparent).
    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn pose(
        &self,
        out: &mut LigneObj,
        x: usize,
        c: usize,
        pal: usize,
        prio: u8,
        mode: u16,
        mosaique: bool,
    ) {
        if c == 0 {
            if out.coul[x] == VIDE && mode != 2 {
                out.mosaique[x] = if mosaique { prio + 1 } else { 0 };
            }
            return;
        }
        if mode == 2 {
            out.fenetre[x] = true;
        } else if prio < out.prio[x] {
            out.coul[x] = self.couleur(pal + c);
            out.prio[x] = prio;
            out.semi[x] = mode == 1;
            out.mosaique[x] = if mosaique { prio + 1 } else { 0 };
        }
    }
}
