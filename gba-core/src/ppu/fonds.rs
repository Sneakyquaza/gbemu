//! Fonds : texte (modes 0-1), affines (modes 1-2), bitmaps (modes 3-5, transformés par BG2).

use super::{Ppu, BGCNT, LARGEUR, MOSAIC, VIDE};

impl Ppu {
    /// Ligne `y` de la mosaïque verticale du fond `b` (première ligne du bloc).
    fn ligne_mosaique(&self, b: usize, y: usize) -> usize {
        if self.regs[BGCNT + b] & 0x40 != 0 {
            y - y % (((self.regs[MOSAIC] >> 4) & 0xF) as usize + 1)
        } else {
            y
        }
    }

    /// Point de départ affine de la ligne (fond 2 ou 3) : avec mosaïque verticale, celui de la
    /// première ligne du bloc.
    fn depart_affine(&self, b: usize, y: usize) -> (i32, i32) {
        let i = b - 2;
        let recul = (y - self.ligne_mosaique(b, y)) as i32;
        let pb = self.regs[0x11 + i * 8] as i16 as i32;
        let pd = self.regs[0x13 + i * 8] as i16 as i32;
        (
            self.ref_aff[i][0] - pb * recul,
            self.ref_aff[i][1] - pd * recul,
        )
    }

    /// Fond texte `b` : carte 256/512 × 256/512, tuiles 4 bpp (16 palettes) ou 8 bpp.
    pub(super) fn ligne_texte(&self, b: usize, y: usize, out: &mut [u16; LARGEUR]) {
        let cnt = self.regs[BGCNT + b] as usize;
        let hofs = (self.regs[8 + b * 2] & 0x1FF) as usize;
        let vofs = (self.regs[9 + b * 2] & 0x1FF) as usize;
        let car = ((cnt >> 2) & 3) * 0x4000;
        let ecr = ((cnt >> 8) & 31) * 0x800;
        let c256 = cnt & 0x80 != 0;
        let (l, h) = match cnt >> 14 {
            0 => (256, 256),
            1 => (512, 256),
            2 => (256, 512),
            _ => (512, 512),
        };
        let sy = (self.ligne_mosaique(b, y) + vofs) & (h - 1);
        let ty = sy / 8;
        let base_ligne = ecr + (ty / 32) * (l / 256) * 0x800 + (ty % 32) * 64;
        let mut x = 0;
        let mut sx = hofs;
        while x < LARGEUR {
            let sxm = sx & (l - 1);
            let tx = sxm / 8;
            // Carte : blocs de 32×32 entrées ; le bloc de droite suit en largeur 512.
            let a_e = base_ligne + (tx / 32) * 0x800 + (tx % 32) * 2;
            let e = if a_e < 0x10000 {
                self.vram16(a_e) as usize
            } else {
                0
            };
            let py = if e & 0x800 != 0 { 7 - sy % 8 } else { sy % 8 };
            let tuile = e & 0x3FF;
            let mut pix = [VIDE; 8];
            if c256 {
                let a = car + tuile * 64 + py * 8;
                if a + 8 <= 0x10000 {
                    for (c, p) in pix.iter_mut().enumerate() {
                        let i = self.vram[a + c] as usize;
                        if i != 0 {
                            *p = self.couleur(i);
                        }
                    }
                }
            } else {
                let a = car + tuile * 32 + py * 4;
                if a + 4 <= 0x10000 {
                    let pal = (e >> 12) * 16;
                    for (c, p) in pix.iter_mut().enumerate() {
                        let n = (self.vram[a + c / 2] >> ((c & 1) * 4)) as usize & 15;
                        if n != 0 {
                            *p = self.couleur(pal + n);
                        }
                    }
                }
            }
            if e & 0x400 != 0 {
                pix.reverse();
            }
            let debut = sxm % 8;
            let n = (8 - debut).min(LARGEUR - x);
            out[x..x + n].copy_from_slice(&pix[debut..debut + n]);
            x += n;
            sx += n;
        }
    }

    /// Fond affine `b` (2 ou 3) : carte carrée 128 à 1024 points, entrées 8 bits, tuiles 8 bpp.
    pub(super) fn ligne_affine(&self, b: usize, y: usize, out: &mut [u16; LARGEUR]) {
        let cnt = self.regs[BGCNT + b] as usize;
        let car = ((cnt >> 2) & 3) * 0x4000;
        let ecr = ((cnt >> 8) & 31) * 0x800;
        let taille = 128i32 << (cnt >> 14);
        let tuiles_l = (taille / 8) as usize;
        let boucle = cnt & 0x2000 != 0;
        let i = b - 2;
        let pa = self.regs[0x10 + i * 8] as i16 as i32;
        let pc = self.regs[0x12 + i * 8] as i16 as i32;
        let (mut rx, mut ry) = self.depart_affine(b, y);
        for p in out.iter_mut() {
            let (mut tx, mut ty) = (rx >> 8, ry >> 8);
            rx += pa;
            ry += pc;
            if boucle {
                tx &= taille - 1;
                ty &= taille - 1;
            } else if !(0..taille).contains(&tx) || !(0..taille).contains(&ty) {
                continue;
            }
            let (tx, ty) = (tx as usize, ty as usize);
            let a_e = ecr + (ty / 8) * tuiles_l + tx / 8;
            if a_e >= 0x10000 {
                continue;
            }
            let a = car + self.vram[a_e] as usize * 64 + (ty % 8) * 8 + tx % 8;
            if a < 0x10000 {
                let c = self.vram[a] as usize;
                if c != 0 {
                    *p = self.couleur(c);
                }
            }
        }
    }

    /// Fond 2 en bitmap : mode 3 (240×160, 15 bits), 4 (240×160, palette, 2 pages),
    /// 5 (160×128, 15 bits, 2 pages). Transformé par la matrice de BG2 ; hors image transparent.
    pub(super) fn ligne_bitmap(&self, mode: u16, y: usize, out: &mut [u16; LARGEUR]) {
        let page = if self.regs[0] & 0x10 != 0 && mode != 3 {
            0xA000
        } else {
            0
        };
        let (l, h) = if mode == 5 { (160, 128) } else { (240, 160) };
        let pa = self.regs[0x10] as i16 as i32;
        let pc = self.regs[0x12] as i16 as i32;
        let (mut rx, mut ry) = self.depart_affine(2, y);
        for p in out.iter_mut() {
            let (tx, ty) = (rx >> 8, ry >> 8);
            rx += pa;
            ry += pc;
            if !(0..l).contains(&tx) || !(0..h).contains(&ty) {
                continue;
            }
            let i = (ty * l + tx) as usize;
            if mode == 4 {
                let c = self.vram[page + i] as usize;
                if c != 0 {
                    *p = self.couleur(c);
                }
            } else {
                *p = self.vram16(page + i * 2) & 0x7FFF;
            }
        }
    }
}
