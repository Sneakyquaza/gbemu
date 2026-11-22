//! PPU minimal : synchronisation (HDraw/HBlank/VBlank, VCOUNT, IRQ, déclencheurs DMA) et rendu par
//! ligne des modes 3 et 4 et du mode 0 (fonds texte, priorités, défilement).
//! raccourci : pas de sprites, fenêtres, mélange, mosaïque, modes 1/2/5 ni transformation affine
//! du fond 2 en bitmap ; c'est le lot « PPU complet », qui remplace `dessine_ligne` sans toucher
//! à l'interface (`lit_io`, `ecrit_io`, `evenement`, `frame`, mémoires publiques).

pub const LARGEUR: usize = 240;
pub const HAUTEUR: usize = 160;

const CYCLES_HDRAW: u64 = 960;
const CYCLES_HBLANK: u64 = 272;
const LIGNES: u16 = 228;

/// Ce que l'évènement PPU demande au bus.
#[derive(Default)]
pub struct EvenementPpu {
    /// Bits IRQ (0 = VBlank, 1 = HBlank, 2 = VCount).
    pub irq: u16,
    pub dma_hblank: bool,
    pub dma_vblank: bool,
    /// Ligne de capture vidéo (DMA3 spécial, lignes 2 à 161).
    pub dma_capture: bool,
    /// Fin de la capture vidéo (ligne 162).
    pub fin_capture: bool,
}

pub struct Ppu {
    pub vram: Box<[u8]>,
    pub pram: Box<[u8]>,
    pub oam: Box<[u8]>,
    /// Registres 0x000-0x05F tels qu'écrits (DISPSTAT et VCOUNT à part).
    regs: [u16; 0x30],
    dispstat: u16,
    vcount: u16,
    en_hblank: bool,
    /// Cycle du prochain évènement.
    pub prochain: u64,
    pub frame_ready: bool,
    ecran: Box<[u8]>,
}

impl Ppu {
    pub fn new() -> Self {
        let mut p = Ppu {
            vram: vec![0; 0x18000].into_boxed_slice(),
            pram: vec![0; 0x400].into_boxed_slice(),
            oam: vec![0; 0x400].into_boxed_slice(),
            regs: [0; 0x30],
            dispstat: 0,
            vcount: 0,
            en_hblank: false,
            prochain: CYCLES_HDRAW,
            frame_ready: false,
            ecran: vec![0; LARGEUR * HAUTEUR * 3].into_boxed_slice(),
        };
        // Matrices affines identité (BG2PA/PD, BG3PA/PD), comme après le BIOS.
        p.regs[0x20 / 2] = 0x100;
        p.regs[0x26 / 2] = 0x100;
        p.regs[0x30 / 2] = 0x100;
        p.regs[0x36 / 2] = 0x100;
        p
    }

    /// Écran RGB8 (240×160×3).
    pub fn frame(&self) -> &[u8] {
        &self.ecran
    }

    /// Modes bitmap 3-5 : la VRAM des fonds va jusqu'à 0x14000 (sinon 0x10000).
    pub fn mode_bitmap(&self) -> bool {
        self.regs[0] & 7 >= 3
    }

    pub fn lit_io(&self, o: u32) -> u16 {
        match o {
            0x04 => self.dispstat,
            0x06 => self.vcount,
            // Registres lisibles : DISPCNT, vert inversé, BGxCNT, fenêtres, BLDCNT, BLDALPHA.
            0x00 | 0x02 | 0x08..=0x0E | 0x48 | 0x4A | 0x50 | 0x52 => self.regs[(o / 2) as usize],
            _ => 0,
        }
    }

    pub fn ecrit_io(&mut self, o: u32, v: u16) {
        match o {
            0x04 => {
                self.dispstat = (self.dispstat & 7) | (v & 0xFF38);
                self.compare_vcount();
            }
            0x06 => {}
            _ => {
                if let Some(r) = self.regs.get_mut((o / 2) as usize) {
                    *r = v;
                }
            }
        }
    }

    fn compare_vcount(&mut self) -> bool {
        if self.vcount == self.dispstat >> 8 {
            self.dispstat |= 4;
            true
        } else {
            self.dispstat &= !4;
            false
        }
    }

    /// Début de HBlank ou fin de ligne (appelé quand `prochain` est atteint).
    pub fn evenement(&mut self) -> EvenementPpu {
        let mut ev = EvenementPpu::default();
        if !self.en_hblank {
            self.en_hblank = true;
            self.dispstat |= 2;
            self.prochain += CYCLES_HBLANK;
            if self.vcount < HAUTEUR as u16 {
                self.dessine_ligne();
                ev.dma_hblank = true;
            }
            if self.dispstat & 0x10 != 0 {
                ev.irq |= 2;
            }
            ev.dma_capture = (2..162).contains(&self.vcount);
            ev.fin_capture = self.vcount == 162;
        } else {
            self.en_hblank = false;
            self.dispstat &= !2;
            self.prochain += CYCLES_HDRAW;
            self.vcount = (self.vcount + 1) % LIGNES;
            if self.vcount == HAUTEUR as u16 {
                self.dispstat |= 1;
                self.frame_ready = true;
                ev.dma_vblank = true;
                if self.dispstat & 8 != 0 {
                    ev.irq |= 1;
                }
            } else if self.vcount == LIGNES - 1 {
                self.dispstat &= !1;
            }
            if self.compare_vcount() && self.dispstat & 0x20 != 0 {
                ev.irq |= 4;
            }
        }
        ev
    }

    #[inline]
    fn couleur(&self, i: usize) -> u16 {
        u16::from_le_bytes([self.pram[i * 2], self.pram[i * 2 + 1]])
    }

    #[inline]
    fn vram16(&self, o: usize) -> u16 {
        u16::from_le_bytes([self.vram[o], self.vram[o + 1]])
    }

    fn dessine_ligne(&mut self) {
        let y = self.vcount as usize;
        let dispcnt = self.regs[0];
        let mut ligne = [0u16; LARGEUR];
        if dispcnt & 0x80 != 0 {
            ligne = [0x7FFF; LARGEUR]; // écran forcé blanc
        } else {
            let fond = self.couleur(0);
            ligne.fill(fond);
            let bg2 = dispcnt & 0x400 != 0;
            match dispcnt & 7 {
                0 => self.ligne_texte(y, dispcnt, &mut ligne),
                3 if bg2 => {
                    for (x, p) in ligne.iter_mut().enumerate() {
                        *p = self.vram16((y * LARGEUR + x) * 2);
                    }
                }
                4 if bg2 => {
                    let base = if dispcnt & 0x10 != 0 { 0xA000 } else { 0 };
                    for (x, p) in ligne.iter_mut().enumerate() {
                        let i = self.vram[base + y * LARGEUR + x] as usize;
                        if i != 0 {
                            *p = self.couleur(i);
                        }
                    }
                }
                _ => {}
            }
        }
        let out = &mut self.ecran[y * LARGEUR * 3..(y + 1) * LARGEUR * 3];
        for (px, &c) in out.as_chunks_mut::<3>().0.iter_mut().zip(ligne.iter()) {
            let e = |v: u16| ((v << 3) | (v >> 2)) as u8;
            px[0] = e(c & 0x1F);
            px[1] = e((c >> 5) & 0x1F);
            px[2] = e((c >> 10) & 0x1F);
        }
    }

    /// Mode 0 : 4 fonds texte, du moins prioritaire au plus prioritaire.
    fn ligne_texte(&self, y: usize, dispcnt: u16, ligne: &mut [u16; LARGEUR]) {
        let mut ordre: Vec<usize> = (0..4).filter(|&b| dispcnt & (0x100 << b) != 0).collect();
        // Priorité 0 = devant ; à égalité, le fond de plus petit numéro est devant.
        ordre.sort_by_key(|&b| std::cmp::Reverse((self.regs[4 + b] & 3, b)));
        for b in ordre {
            let cnt = self.regs[4 + b] as usize;
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
            let sy = (y + vofs) & (h - 1);
            let ty = sy / 8;
            for (x, p) in ligne.iter_mut().enumerate() {
                let sx = (x + hofs) & (l - 1);
                let tx = sx / 8;
                let bloc = tx / 32 + (ty / 32) * (l / 256);
                let e = self.vram16(ecr + bloc * 0x800 + ((ty % 32) * 32 + tx % 32) * 2) as usize;
                let px = if e & 0x400 != 0 { 7 - sx % 8 } else { sx % 8 };
                let py = if e & 0x800 != 0 { 7 - sy % 8 } else { sy % 8 };
                let tuile = e & 0x3FF;
                let idx = if c256 {
                    let a = car + tuile * 64 + py * 8 + px;
                    if a >= 0x10000 {
                        continue;
                    }
                    self.vram[a] as usize
                } else {
                    let a = car + tuile * 32 + py * 4 + px / 2;
                    if a >= 0x10000 {
                        continue;
                    }
                    let n = (self.vram[a] >> ((px & 1) * 4)) as usize & 15;
                    if n == 0 {
                        continue;
                    }
                    (e >> 12) * 16 + n
                };
                if idx != 0 {
                    *p = self.couleur(idx);
                }
            }
        }
    }
}

// --- Sauvegarde d'état (lot sauvegardes ; le lot PPU y ajoute ses champs) ---

use crate::etat::{verifie, Ecrit, ErreurEtat, Lecteur};

impl Ppu {
    pub(crate) fn sauve(&self, e: &mut Vec<u8>) {
        e.octets(&self.vram);
        e.octets(&self.pram);
        e.octets(&self.oam);
        self.regs.iter().for_each(|&v| e.u16(v));
        e.u16(self.dispstat);
        e.u16(self.vcount);
        e.bool(self.en_hblank);
        e.u64(self.prochain);
        e.bool(self.frame_ready);
        e.octets(&self.ecran);
    }

    pub(crate) fn charge(&mut self, l: &mut Lecteur) -> Result<(), ErreurEtat> {
        let mut p = Ppu::new();
        l.octets(&mut p.vram)?;
        l.octets(&mut p.pram)?;
        l.octets(&mut p.oam)?;
        for v in &mut p.regs {
            *v = l.u16()?;
        }
        p.dispstat = l.u16()?;
        p.vcount = l.u16_max(227, "vcount")?;
        p.en_hblank = l.bool()?;
        p.prochain = l.u64()?;
        verifie(p.prochain < 1 << 60, "ppu")?;
        p.frame_ready = l.bool()?;
        l.octets(&mut p.ecran)?;
        *self = p;
        Ok(())
    }
}
