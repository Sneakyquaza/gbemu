//! PPU rendu par ligne : fond, fenêtre, sprites (10 par ligne), STAT/LY/LYC et modes.
//! En mode CGB : 2 banques de VRAM, attributs de tuiles, palettes couleur BGR555.

use crate::etat::{verifie, Ecrit, ErreurEtat, Lecteur};

pub const LARGEUR: usize = 160;
pub const HAUTEUR: usize = 144;

pub const IRQ_VBLANK: u8 = 0x01;
pub const IRQ_STAT: u8 = 0x02;
/// Bit renvoyé par `tick` au début d'un HBlank visible (HDMA CGB), hors registre IF.
pub const DEBUT_HBLANK: u8 = 0x80;

/// Gris DMG de `frame_rgb` (teinte 0 = blanc).
const GRIS_RGB: [[u8; 3]; 4] = [[0xFF; 3], [0xAA; 3], [0x55; 3], [0x00; 3]];

pub struct Ppu {
    pub cgb: bool,
    /// Banque 0 puis banque 1 (CGB seulement).
    pub vram: Box<[u8; 0x4000]>,
    /// Décalage de la banque VRAM vue par le CPU (VBK × 0x2000).
    pub vram_banque: usize,
    pub oam: [u8; 0xA0],
    pub lcdc: u8,
    stat: u8,
    pub scy: u8,
    pub scx: u8,
    pub ly: u8,
    pub lyc: u8,
    pub bgp: u8,
    pub obp0: u8,
    pub obp1: u8,
    pub wy: u8,
    pub wx: u8,
    dot: u32,
    mode: u8,
    mode3_end: u32,
    window_line: u8,
    wy_hit: bool,
    stat_line: bool,
    /// Teintes 0 (blanc) à 3 (noir), une par pixel. En CGB : index de couleur 0-3 avant palette.
    pub frame: Box<[u8; LARGEUR * HAUTEUR]>,
    /// Écran RGB8 (CGB : couleurs converties ; DMG : gris des teintes).
    pub rgb: Box<[u8; LARGEUR * HAUTEUR * 3]>,
    pub frame_ready: bool,
    bcps: u8,
    ocps: u8,
    /// RAM des palettes CGB (8 palettes × 4 couleurs BGR555 petit-boutistes).
    bgpal: [u8; 64],
    obpal: [u8; 64],
    /// Couleurs RGB des palettes, recalculées à chaque écriture (ou changement de correction).
    bg_rgb: [[u8; 3]; 32],
    ob_rgb: [[u8; 3]; 32],
    /// OPRI bit 0 : 0 = priorité des sprites par index OAM (CGB), 1 = par X (DMG).
    opri: u8,
    correction: bool,
}

impl Ppu {
    pub fn new(cgb: bool) -> Self {
        let mut ppu = Ppu {
            cgb,
            vram: Box::new([0; 0x4000]),
            vram_banque: 0,
            oam: [0; 0xA0],
            lcdc: 0x91,
            stat: 0,
            scy: 0,
            scx: 0,
            ly: 0,
            lyc: 0,
            bgp: 0xFC,
            obp0: 0xFF,
            obp1: 0xFF,
            wy: 0,
            wx: 0,
            dot: 0,
            mode: 2,
            mode3_end: 252,
            window_line: 0,
            wy_hit: false,
            stat_line: false,
            frame: Box::new([0; LARGEUR * HAUTEUR]),
            rgb: Box::new([0xFF; LARGEUR * HAUTEUR * 3]),
            frame_ready: false,
            bcps: 0,
            ocps: 0,
            // Boot CGB : palettes blanches.
            bgpal: [0xFF; 64],
            obpal: [0xFF; 64],
            bg_rgb: [[0; 3]; 32],
            ob_rgb: [[0; 3]; 32],
            opri: if cgb { 0 } else { 1 },
            correction: false,
        };
        ppu.recalcule_palettes();
        ppu
    }

    /// Taille de VRAM sauvée : une banque en DMG.
    fn taille_vram(&self) -> usize {
        if self.cgb {
            0x4000
        } else {
            0x2000
        }
    }

    /// Couleur BGR555 → RGB8 : linéaire `(c << 3) | (c >> 2)`, ou avec correction
    /// façon écran CGB (mixage des canaux de Gambatte, sortie 0-248).
    fn couleur(pal: &[u8; 64], i: usize, correction: bool) -> [u8; 3] {
        let c = u16::from_le_bytes([pal[i * 2], pal[i * 2 + 1]]);
        let (r, g, b) = (
            (c & 0x1F) as u32,
            ((c >> 5) & 0x1F) as u32,
            ((c >> 10) & 0x1F) as u32,
        );
        if correction {
            [
                ((r * 13 + g * 2 + b) >> 1) as u8,
                ((g * 3 + b) << 1) as u8,
                ((r * 3 + g * 2 + b * 11) >> 1) as u8,
            ]
        } else {
            [r, g, b].map(|c| ((c << 3) | (c >> 2)) as u8)
        }
    }

    fn recalcule_palettes(&mut self) {
        for i in 0..32 {
            self.bg_rgb[i] = Self::couleur(&self.bgpal, i, self.correction);
            self.ob_rgb[i] = Self::couleur(&self.obpal, i, self.correction);
        }
    }

    /// Correction des couleurs CGB (réglage de sortie, hors état) : visible à la prochaine ligne rendue.
    pub fn set_correction(&mut self, on: bool) {
        self.correction = on;
        self.recalcule_palettes();
    }

    pub fn sauve(&self, e: &mut Vec<u8>) {
        e.octets(&self.vram[..self.taille_vram()]);
        e.octets(&self.oam);
        e.octets(&[
            self.lcdc,
            self.stat,
            self.scy,
            self.scx,
            self.ly,
            self.lyc,
            self.bgp,
            self.obp0,
            self.obp1,
            self.wy,
            self.wx,
            self.mode,
            self.window_line,
        ]);
        e.u32(self.dot);
        e.u32(self.mode3_end);
        e.bool(self.wy_hit);
        e.bool(self.stat_line);
        e.bool(self.frame_ready);
        e.octets(&self.frame[..]);
        e.octets(&[
            (self.vram_banque / 0x2000) as u8,
            self.bcps,
            self.ocps,
            self.opri,
        ]);
        e.octets(&self.bgpal);
        e.octets(&self.obpal);
        if self.cgb {
            e.octets(&self.rgb[..]);
        }
    }

    /// Relit l'état ; refuse les combinaisons qui feraient sortir le rendu de l'écran.
    pub fn charge(&mut self, l: &mut Lecteur) -> Result<(), ErreurEtat> {
        let n = self.taille_vram();
        l.octets(&mut self.vram[..n])?;
        l.octets(&mut self.oam)?;
        let mut r = [0u8; 13];
        l.octets(&mut r)?;
        [
            self.lcdc,
            self.stat,
            self.scy,
            self.scx,
            self.ly,
            self.lyc,
            self.bgp,
            self.obp0,
            self.obp1,
            self.wy,
            self.wx,
            self.mode,
            self.window_line,
        ] = r;
        self.stat &= 0x78;
        self.dot = l.u32()?;
        self.mode3_end = l.u32()?;
        self.wy_hit = l.bool()?;
        self.stat_line = l.bool()?;
        self.frame_ready = l.bool()?;
        l.octets(&mut self.frame[..])?;
        let banque = l.u8_max(self.cgb as u8, "VBK")?;
        self.vram_banque = banque as usize * 0x2000;
        self.bcps = l.u8()? & 0xBF;
        self.ocps = l.u8()? & 0xBF;
        self.opri = l.u8_max(1, "OPRI")?;
        l.octets(&mut self.bgpal)?;
        l.octets(&mut self.obpal)?;
        if self.cgb {
            l.octets(&mut self.rgb[..])?;
        } else {
            for (p, &t) in self
                .rgb
                .as_chunks_mut::<3>()
                .0
                .iter_mut()
                .zip(self.frame.iter())
            {
                *p = GRIS_RGB[(t & 3) as usize];
            }
        }
        self.recalcule_palettes();
        verifie(self.ly <= 153 && self.dot < 456 && self.mode <= 3, "PPU")?;
        // Une ligne n'est rendue qu'en mode 2 visible (ou à l'allumage, LY = 0).
        verifie(self.mode != 2 || self.ly < 144, "PPU mode")?;
        verifie(self.lcd_on() || self.ly == 0, "PPU éteint")?;
        verifie(self.window_line as u16 <= self.ly as u16 + 1, "PPU fenêtre")?;
        verifie(self.frame.iter().all(|&t| t <= 3), "écran")
    }

    #[inline]
    pub fn lcd_on(&self) -> bool {
        self.lcdc & 0x80 != 0
    }

    /// Ligne d'interruption STAT ; l'interruption part sur le front montant.
    fn refresh_stat(&mut self) -> u8 {
        let s = self.stat;
        let line = self.lcd_on()
            && ((self.mode == 0 && s & 0x08 != 0)
                || (self.mode == 1 && s & 0x10 != 0)
                || (self.mode == 2 && s & 0x20 != 0)
                || (self.ly == self.lyc && s & 0x40 != 0));
        let rise = line && !self.stat_line;
        self.stat_line = line;
        if rise {
            IRQ_STAT
        } else {
            0
        }
    }

    /// Avance de `points` points (4 par cycle machine, 2 en double vitesse CGB) ;
    /// renvoie les interruptions levées (+ `DEBUT_HBLANK`).
    #[inline]
    pub fn tick(&mut self, points: u32) -> u8 {
        if !self.lcd_on() {
            return 0;
        }
        self.dot += points;
        let mut irq = 0;
        match self.mode {
            2 if self.dot >= 80 => {
                self.mode = 3;
                // raccourci : durée du mode 3 sans pénalité sprites/fenêtre, à affiner si un jeu en dépend.
                self.mode3_end = 252 + (self.scx & 7) as u32;
                if self.cgb {
                    self.render_line_cgb();
                } else {
                    self.render_line();
                }
            }
            3 if self.dot >= self.mode3_end => {
                self.mode = 0;
                irq |= DEBUT_HBLANK;
            }
            _ => {}
        }
        if self.dot >= 456 {
            self.dot -= 456;
            self.ly += 1;
            if self.ly == 144 {
                self.mode = 1;
                irq |= IRQ_VBLANK;
                self.frame_ready = true;
            } else if self.ly == 154 {
                self.ly = 0;
                self.window_line = 0;
                self.wy_hit = false;
                self.mode = 2;
            } else if self.ly < 144 {
                self.mode = 2;
            }
        }
        irq | self.refresh_stat()
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF40 => self.lcdc,
            0xFF41 => {
                let coinc = if self.ly == self.lyc { 0x04 } else { 0 };
                let mode = if self.lcd_on() { self.mode } else { 0 };
                0x80 | self.stat | coinc | mode
            }
            0xFF42 => self.scy,
            0xFF43 => self.scx,
            0xFF44 => self.ly,
            0xFF45 => self.lyc,
            0xFF47 => self.bgp,
            0xFF48 => self.obp0,
            0xFF49 => self.obp1,
            0xFF4A => self.wy,
            0xFF4B => self.wx,
            0xFF4F => 0xFE | (self.vram_banque / 0x2000) as u8,
            0xFF68 => self.bcps | 0x40,
            0xFF69 => self.bgpal[(self.bcps & 0x3F) as usize],
            0xFF6A => self.ocps | 0x40,
            0xFF6B => self.obpal[(self.ocps & 0x3F) as usize],
            _ => 0xFE | self.opri,
        }
    }

    /// Écriture BCPD/OCPD : octet de palette puis auto-incrément de l'index (bit 7).
    // raccourci : accès aux palettes non bloqué en mode 3 (durée du mode 3 approchée, rendu par ligne).
    fn ecrit_palette(&mut self, obj: bool, v: u8) {
        let (idx, pal) = if obj {
            (&mut self.ocps, &mut self.obpal)
        } else {
            (&mut self.bcps, &mut self.bgpal)
        };
        let i = (*idx & 0x3F) as usize;
        pal[i] = v;
        if *idx & 0x80 != 0 {
            *idx = 0x80 | ((*idx + 1) & 0x3F);
        }
        let c = Self::couleur(pal, i / 2, self.correction);
        if obj {
            self.ob_rgb[i / 2] = c;
        } else {
            self.bg_rgb[i / 2] = c;
        }
    }

    /// Écriture d'un registre ; renvoie les interruptions STAT éventuelles.
    pub fn write(&mut self, addr: u16, v: u8) -> u8 {
        match addr {
            0xFF40 => {
                let was_on = self.lcd_on();
                self.lcdc = v;
                if was_on && !self.lcd_on() {
                    self.ly = 0;
                    self.dot = 0;
                    self.mode = 0;
                    self.window_line = 0;
                    self.wy_hit = false;
                } else if !was_on && self.lcd_on() {
                    self.dot = 0;
                    self.mode = 2;
                }
            }
            0xFF41 => self.stat = v & 0x78,
            0xFF42 => self.scy = v,
            0xFF43 => self.scx = v,
            0xFF44 => {}
            0xFF45 => self.lyc = v,
            0xFF47 => self.bgp = v,
            0xFF48 => self.obp0 = v,
            0xFF49 => self.obp1 = v,
            0xFF4A => self.wy = v,
            0xFF4B => self.wx = v,
            0xFF4F => self.vram_banque = (v as usize & 1) * 0x2000,
            0xFF68 => self.bcps = v & 0xBF,
            0xFF69 => self.ecrit_palette(false, v),
            0xFF6A => self.ocps = v & 0xBF,
            0xFF6B => self.ecrit_palette(true, v),
            _ => self.opri = v & 1,
        }
        self.refresh_stat()
    }

    /// Octets d'une ligne de tuile ; `banque` = décalage VRAM (0 ou 0x2000).
    #[inline]
    fn tile_row(&self, tile: u8, row: u8, signed: bool, banque: usize) -> (u8, u8) {
        let base = banque
            + if signed {
                (0x1000i32 + (tile as i8 as i32) * 16) as usize
            } else {
                tile as usize * 16
            };
        let a = base + row as usize * 2;
        (self.vram[a], self.vram[a + 1])
    }

    #[inline(never)]
    fn render_line(&mut self) {
        self.render_line_dmg();
        let ly = self.ly as usize;
        let teintes = &self.frame[ly * LARGEUR..(ly + 1) * LARGEUR];
        let rgb = &mut self.rgb[ly * LARGEUR * 3..(ly + 1) * LARGEUR * 3];
        for (p, &t) in rgb.as_chunks_mut::<3>().0.iter_mut().zip(teintes) {
            *p = GRIS_RGB[t as usize & 3];
        }
    }

    fn render_line_dmg(&mut self) {
        let ly = self.ly;
        if ly == self.wy {
            self.wy_hit = true;
        }
        let mut bg = [0u8; LARGEUR];
        let signed = self.lcdc & 0x10 == 0;

        if self.lcdc & 0x01 != 0 {
            // Fond.
            let map = if self.lcdc & 0x08 != 0 {
                0x1C00
            } else {
                0x1800
            };
            let y = ly.wrapping_add(self.scy);
            let row_base = map + (y as usize / 8) * 32;
            let mut x = 0usize;
            let mut sx = self.scx;
            while x < LARGEUR {
                let tile = self.vram[row_base + (sx as usize / 8)];
                let (lo, hi) = self.tile_row(tile, y & 7, signed, 0);
                let mut b = sx & 7;
                while b < 8 && x < LARGEUR {
                    let s = 7 - b;
                    bg[x] = ((lo >> s) & 1) | (((hi >> s) & 1) << 1);
                    x += 1;
                    b += 1;
                    sx = sx.wrapping_add(1);
                }
            }

            // Fenêtre.
            let wx = self.wx as i32 - 7;
            if self.lcdc & 0x20 != 0 && self.wy_hit && wx < LARGEUR as i32 && self.wx <= 166 {
                let map = if self.lcdc & 0x40 != 0 {
                    0x1C00
                } else {
                    0x1800
                };
                let wl = self.window_line;
                let row_base = map + (wl as usize / 8) * 32;
                for x in wx.max(0)..LARGEUR as i32 {
                    let px = (x - wx) as usize;
                    let tile = self.vram[row_base + px / 8];
                    let (lo, hi) = self.tile_row(tile, wl & 7, signed, 0);
                    let s = 7 - (px & 7);
                    bg[x as usize] = ((lo >> s) & 1) | (((hi >> s) & 1) << 1);
                }
                self.window_line += 1;
            }
        }

        let line = &mut self.frame[ly as usize * LARGEUR..(ly as usize + 1) * LARGEUR];
        for (o, &c) in line.iter_mut().zip(bg.iter()) {
            *o = (self.bgp >> (c * 2)) & 3;
        }

        if self.lcdc & 0x02 == 0 {
            return;
        }
        // Sélection OAM : 10 premiers sprites qui couvrent la ligne.
        let height: i32 = if self.lcdc & 0x04 != 0 { 16 } else { 8 };
        let mut sel = [0usize; 10];
        let mut n = 0;
        for i in 0..40 {
            let y = self.oam[i * 4] as i32 - 16;
            if (ly as i32) >= y && (ly as i32) < y + height {
                sel[n] = i;
                n += 1;
                if n == 10 {
                    break;
                }
            }
        }
        // Priorité DMG : X le plus petit, puis index OAM le plus petit (tri stable).
        let sel = &mut sel[..n];
        sel.sort_by_key(|&i| self.oam[i * 4 + 1]);

        let mut drawn = [false; LARGEUR];
        for &i in sel.iter() {
            let o = &self.oam[i * 4..i * 4 + 4];
            let y = o[0] as i32 - 16;
            let x = o[1] as i32 - 8;
            let flags = o[3];
            let mut row = ly as i32 - y;
            if flags & 0x40 != 0 {
                row = height - 1 - row;
            }
            let tile = if height == 16 { o[2] & 0xFE } else { o[2] } as usize;
            let a = tile * 16 + row as usize * 2;
            let (lo, hi) = (self.vram[a], self.vram[a + 1]);
            let pal = if flags & 0x10 != 0 {
                self.obp1
            } else {
                self.obp0
            };
            for px in 0..8 {
                let sx = x + px;
                if !(0..LARGEUR as i32).contains(&sx) {
                    continue;
                }
                let sx = sx as usize;
                if drawn[sx] {
                    continue;
                }
                let s = if flags & 0x20 != 0 { px } else { 7 - px };
                let c = ((lo >> s) & 1) | (((hi >> s) & 1) << 1);
                if c == 0 {
                    continue;
                }
                drawn[sx] = true;
                if flags & 0x80 != 0 && bg[sx] != 0 {
                    continue;
                }
                line[sx] = (pal >> (c * 2)) & 3;
            }
        }
    }

    /// Ligne en mode CGB. Par pixel de fond : index de couleur (bits 0-1), palette (bits 2-4),
    /// priorité BG de l'attribut (bit 7).
    #[inline(never)]
    fn render_line_cgb(&mut self) {
        let ly = self.ly;
        if ly == self.wy {
            self.wy_hit = true;
        }
        let mut bg = [0u8; LARGEUR];
        let signed = self.lcdc & 0x10 == 0;

        // Fond (LCDC bit 0 = priorité maître en CGB, le fond reste affiché).
        let map = if self.lcdc & 0x08 != 0 {
            0x1C00
        } else {
            0x1800
        };
        let y = ly.wrapping_add(self.scy);
        let row_base = map + (y as usize / 8) * 32;
        let mut x = 0usize;
        let mut sx = self.scx;
        while x < LARGEUR {
            let m = row_base + (sx as usize / 8);
            let (tile, attr) = (self.vram[m], self.vram[0x2000 + m]);
            let row = if attr & 0x40 != 0 { 7 - (y & 7) } else { y & 7 };
            let banque = (attr as usize & 0x08) << 10;
            let (lo, hi) = self.tile_row(tile, row, signed, banque);
            let haut = ((attr & 7) << 2) | (attr & 0x80);
            let mut b = sx & 7;
            while b < 8 && x < LARGEUR {
                let s = if attr & 0x20 != 0 { b } else { 7 - b };
                bg[x] = ((lo >> s) & 1) | (((hi >> s) & 1) << 1) | haut;
                x += 1;
                b += 1;
                sx = sx.wrapping_add(1);
            }
        }

        // Fenêtre.
        let wx = self.wx as i32 - 7;
        if self.lcdc & 0x20 != 0 && self.wy_hit && wx < LARGEUR as i32 && self.wx <= 166 {
            let map = if self.lcdc & 0x40 != 0 {
                0x1C00
            } else {
                0x1800
            };
            let wl = self.window_line;
            let row_base = map + (wl as usize / 8) * 32;
            for x in wx.max(0)..LARGEUR as i32 {
                let px = (x - wx) as usize;
                let m = row_base + px / 8;
                let (tile, attr) = (self.vram[m], self.vram[0x2000 + m]);
                let row = if attr & 0x40 != 0 {
                    7 - (wl & 7)
                } else {
                    wl & 7
                };
                let banque = (attr as usize & 0x08) << 10;
                let (lo, hi) = self.tile_row(tile, row, signed, banque);
                let s = if attr & 0x20 != 0 {
                    px & 7
                } else {
                    7 - (px & 7)
                };
                bg[x as usize] =
                    ((lo >> s) & 1) | (((hi >> s) & 1) << 1) | ((attr & 7) << 2) | (attr & 0x80);
            }
            self.window_line += 1;
        }

        let l = ly as usize;
        let teintes = &mut self.frame[l * LARGEUR..(l + 1) * LARGEUR];
        let rgb = &mut self.rgb[l * LARGEUR * 3..(l + 1) * LARGEUR * 3];
        let pixels = rgb.as_chunks_mut::<3>().0;
        for ((p, t), &c) in pixels.iter_mut().zip(teintes.iter_mut()).zip(&bg) {
            *t = c & 3;
            *p = self.bg_rgb[(c & 0x1F) as usize];
        }

        if self.lcdc & 0x02 == 0 {
            return;
        }
        let height: i32 = if self.lcdc & 0x04 != 0 { 16 } else { 8 };
        let mut sel = [0usize; 10];
        let mut n = 0;
        for i in 0..40 {
            let y = self.oam[i * 4] as i32 - 16;
            if (ly as i32) >= y && (ly as i32) < y + height {
                sel[n] = i;
                n += 1;
                if n == 10 {
                    break;
                }
            }
        }
        // Priorité CGB : index OAM le plus petit ; OPRI bit 0 = 1 → par X comme le DMG.
        let sel = &mut sel[..n];
        if self.opri & 1 != 0 {
            sel.sort_by_key(|&i| self.oam[i * 4 + 1]);
        }
        let maitre = self.lcdc & 0x01 != 0;
        let mut drawn = [false; LARGEUR];
        for &i in sel.iter() {
            let o = &self.oam[i * 4..i * 4 + 4];
            let y = o[0] as i32 - 16;
            let x = o[1] as i32 - 8;
            let flags = o[3];
            let mut row = ly as i32 - y;
            if flags & 0x40 != 0 {
                row = height - 1 - row;
            }
            let tile = if height == 16 { o[2] & 0xFE } else { o[2] } as usize;
            let a = ((flags as usize & 0x08) << 10) + tile * 16 + row as usize * 2;
            let (lo, hi) = (self.vram[a], self.vram[a + 1]);
            let pal = (flags as usize & 7) * 4;
            for px in 0..8 {
                let sx = x + px;
                if !(0..LARGEUR as i32).contains(&sx) {
                    continue;
                }
                let sx = sx as usize;
                if drawn[sx] {
                    continue;
                }
                let s = if flags & 0x20 != 0 { px } else { 7 - px };
                let c = ((lo >> s) & 1) | (((hi >> s) & 1) << 1);
                if c == 0 {
                    continue;
                }
                drawn[sx] = true;
                let f = bg[sx];
                if maitre && f & 3 != 0 && (f & 0x80 != 0 || flags & 0x80 != 0) {
                    continue;
                }
                teintes[sx] = c;
                pixels[sx] = self.ob_rgb[pal + c as usize];
            }
        }
    }
}
