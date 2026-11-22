//! PPU DMG rendu par ligne : fond, fenêtre, sprites (10 par ligne), STAT/LY/LYC et modes.

use crate::etat::{verifie, Ecrit, ErreurEtat, Lecteur};

pub const LARGEUR: usize = 160;
pub const HAUTEUR: usize = 144;

pub const IRQ_VBLANK: u8 = 0x01;
pub const IRQ_STAT: u8 = 0x02;

pub struct Ppu {
    pub vram: [u8; 0x2000],
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
    /// Teintes 0 (blanc) à 3 (noir), une par pixel.
    pub frame: Box<[u8; LARGEUR * HAUTEUR]>,
    pub frame_ready: bool,
}

impl Ppu {
    pub fn new() -> Self {
        Ppu {
            vram: [0; 0x2000],
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
            frame_ready: false,
        }
    }

    pub fn sauve(&self, e: &mut Vec<u8>) {
        e.octets(&self.vram);
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
    }

    /// Relit l'état ; refuse les combinaisons qui feraient sortir le rendu de l'écran.
    pub fn charge(&mut self, l: &mut Lecteur) -> Result<(), ErreurEtat> {
        l.octets(&mut self.vram)?;
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

    /// Avance de 4 points (un cycle machine) ; renvoie les interruptions levées.
    #[inline]
    pub fn tick(&mut self) -> u8 {
        if !self.lcd_on() {
            return 0;
        }
        self.dot += 4;
        let mut irq = 0;
        match self.mode {
            2 if self.dot >= 80 => {
                self.mode = 3;
                // raccourci : durée du mode 3 sans pénalité sprites/fenêtre, à affiner si un jeu en dépend.
                self.mode3_end = 252 + (self.scx & 7) as u32;
                self.render_line();
            }
            3 if self.dot >= self.mode3_end => self.mode = 0,
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
            _ => self.wx,
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
            _ => self.wx = v,
        }
        self.refresh_stat()
    }

    #[inline]
    fn tile_row(&self, tile: u8, row: u8, signed: bool) -> (u8, u8) {
        let base = if signed {
            (0x1000i32 + (tile as i8 as i32) * 16) as usize
        } else {
            tile as usize * 16
        };
        let a = base + row as usize * 2;
        (self.vram[a], self.vram[a + 1])
    }

    fn render_line(&mut self) {
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
                let (lo, hi) = self.tile_row(tile, y & 7, signed);
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
                    let (lo, hi) = self.tile_row(tile, wl & 7, signed);
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
}
