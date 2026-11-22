//! PPU : synchronisation (HDraw/HBlank/VBlank, VCOUNT, IRQ, déclencheurs DMA) et rendu par ligne
//! (au début du HBlank) : modes 0-5, sprites, fenêtres, effets de couleur, mosaïque.
//! Fonds dans `fonds.rs`, sprites dans `objets.rs`, composition ici.
//! raccourci : rendu de la ligne entière au début du HBlank (pas au point près) : les
//! changements de registres en cours de ligne et les retards de prise en compte de DISPCNT ne
//! sont pas émulés ; à revoir si un jeu en dépend.

mod fonds;
mod objets;
#[cfg(test)]
mod tests;

pub const LARGEUR: usize = 240;
pub const HAUTEUR: usize = 160;

const CYCLES_HDRAW: u64 = 960;
const CYCLES_HBLANK: u64 = 272;
const LIGNES: u16 = 228;

/// Pixel transparent dans les lignes de calque (les couleurs n'ont que 15 bits).
const VIDE: u16 = 0x8000;

// Indices dans `regs` (adresse / 2).
const DISPCNT: usize = 0;
const GREENSWAP: usize = 1;
const BGCNT: usize = 4;
const WIN0H: usize = 0x20;
const WIN0V: usize = 0x22;
const WININ: usize = 0x24;
const WINOUT: usize = 0x25;
const MOSAIC: usize = 0x26;
const BLDCNT: usize = 0x28;
const BLDALPHA: usize = 0x29;
const BLDY: usize = 0x2A;

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
    /// Points de référence internes des fonds affines 2 et 3 (x, y en 20.8 signé), rechargés
    /// depuis BGxX/BGxY au VBlank et à chaque écriture, avancés de PB/PD à chaque ligne.
    ref_aff: [[i32; 2]; 2],
    /// Lignes restantes avant l'affichage d'un fond (0-3) ou des sprites (4) qui vient d'être
    /// activé dans DISPCNT (la désactivation est immédiate), et (5) avant la fin d'un écran forcé
    /// blanc (le début est immédiat). Délais vérifiés par nba-emu hw-test dispcnt-latch.
    delai_actif: [u8; 6],
    /// Fenêtres 0 et 1 actives sur la ligne (bascule à Y1, coupure à Y2).
    fen_v: [bool; 2],
    /// Cycle du prochain évènement.
    pub prochain: u64,
    pub frame_ready: bool,
    ecran: Box<[u8]>,
}

/// Ligne des sprites : couleur (VIDE si rien), priorité, semi-transparence, fenêtre OBJ.
struct LigneObj {
    coul: [u16; LARGEUR],
    prio: [u8; LARGEUR],
    semi: [bool; LARGEUR],
    fenetre: [bool; LARGEUR],
    /// Mosaïque : 0 = non, sinon 1 + priorité du sprite qui a couvert le point (même transparent).
    mosaique: [u8; LARGEUR],
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
            ref_aff: [[0; 2]; 2],
            delai_actif: [0; 6],
            fen_v: [false; 2],
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
                if o == 0 {
                    let allumes = v & !self.regs[DISPCNT];
                    for (i, d) in self.delai_actif[..5].iter_mut().enumerate() {
                        if allumes & (0x100 << i) != 0 {
                            *d = 3;
                        }
                    }
                    if self.regs[DISPCNT] & !v & 0x80 != 0 {
                        self.delai_actif[5] = 3;
                    }
                }
                if let Some(r) = self.regs.get_mut((o / 2) as usize) {
                    *r = v;
                }
                // BG2X/BG2Y (0x28-0x2E), BG3X/BG3Y (0x38-0x3E) : rechargement immédiat.
                if matches!(o, 0x28..=0x2E | 0x38..=0x3E) {
                    let b = (o as usize - 0x28) / 0x10;
                    let c = (o as usize >> 2) & 1;
                    self.ref_aff[b][c] = self.ref_registre(b, c);
                }
            }
        }
    }

    /// Valeur 28 bits signée de BG2X/BG2Y/BG3X/BG3Y (`b` = fond 2 ou 3 moins 2, `c` = 0 x, 1 y).
    fn ref_registre(&self, b: usize, c: usize) -> i32 {
        let i = 0x14 + b * 8 + c * 2;
        let v = self.regs[i] as u32 | (self.regs[i + 1] as u32) << 16;
        ((v << 4) as i32) >> 4
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
            self.maj_fenetres_v();
            for d in self.delai_actif.iter_mut() {
                *d = d.saturating_sub(1);
            }
            if self.vcount == HAUTEUR as u16 {
                self.dispstat |= 1;
                self.frame_ready = true;
                ev.dma_vblank = true;
                for b in 0..2 {
                    for c in 0..2 {
                        self.ref_aff[b][c] = self.ref_registre(b, c);
                    }
                }
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

    /// Bascule verticale des fenêtres en début de ligne : active à Y1, coupée à Y2.
    fn maj_fenetres_v(&mut self) {
        let y = self.vcount;
        for w in 0..2 {
            let v = self.regs[WIN0V + w];
            if y == v >> 8 {
                self.fen_v[w] = true;
            }
            if y == v & 0xFF {
                self.fen_v[w] = false;
            }
        }
    }

    #[inline]
    fn couleur(&self, i: usize) -> u16 {
        u16::from_le_bytes([self.pram[i * 2], self.pram[i * 2 + 1]]) & 0x7FFF
    }

    #[inline]
    fn vram16(&self, o: usize) -> u16 {
        u16::from_le_bytes([self.vram[o], self.vram[o + 1]])
    }

    fn dessine_ligne(&mut self) {
        let y = self.vcount as usize;
        let ligne = self.compose_ligne(y);
        // Avance des points de référence affines (BG2PB/PD, BG3PB/PD).
        for b in 0..2 {
            self.ref_aff[b][0] += self.regs[0x11 + b * 8] as i16 as i32;
            self.ref_aff[b][1] += self.regs[0x13 + b * 8] as i16 as i32;
        }
        let out = &mut self.ecran[y * LARGEUR * 3..(y + 1) * LARGEUR * 3];
        for (px, &c) in out.as_chunks_mut::<3>().0.iter_mut().zip(ligne.iter()) {
            let e = |v: u16| ((v << 3) | (v >> 2)) as u8;
            px[0] = e(c & 0x1F);
            px[1] = e((c >> 5) & 0x1F);
            px[2] = e((c >> 10) & 0x1F);
        }
    }

    /// Couleurs BGR555 de la ligne `y`.
    fn compose_ligne(&self, y: usize) -> [u16; LARGEUR] {
        let dispcnt = self.regs[DISPCNT];
        if dispcnt & 0x80 != 0 || self.delai_actif[5] != 0 {
            return [0x7FFF; LARGEUR]; // écran forcé blanc
        }
        let mode = dispcnt & 7;
        // Fonds présents selon le mode, puis activés dans DISPCNT.
        let possibles: u16 = match mode {
            0 => 0xF,
            1 => 0x7,
            2 => 0xC,
            3..=5 => 0x4,
            _ => 0,
        };
        let mut prets = 0;
        for (i, &d) in self.delai_actif[..5].iter().enumerate() {
            if d == 0 {
                prets |= 1 << i;
            }
        }
        let actifs = (dispcnt >> 8) & possibles & prets;
        let mut calques = [[VIDE; LARGEUR]; 4];
        for (b, calque) in calques.iter_mut().enumerate() {
            if actifs & (1 << b) == 0 {
                continue;
            }
            match (mode, b) {
                (0, _) | (1, 0 | 1) => self.ligne_texte(b, y, calque),
                (1 | 2, _) => self.ligne_affine(b, y, calque),
                _ => self.ligne_bitmap(mode, y, calque),
            }
            if self.regs[BGCNT + b] & 0x40 != 0 {
                mosaique_h(calque, (self.regs[MOSAIC] & 0xF) as usize + 1);
            }
        }
        let mut obj = LigneObj {
            coul: [VIDE; LARGEUR],
            prio: [4; LARGEUR],
            semi: [false; LARGEUR],
            fenetre: [false; LARGEUR],
            mosaique: [0; LARGEUR],
        };
        let obj_actif = dispcnt & 0x1000 != 0 && prets & 0x10 != 0;
        if obj_actif || dispcnt & 0x8000 != 0 {
            self.ligne_objets(y, &mut obj);
        }

        // Fonds actifs triés : priorité puis numéro.
        let mut ordre = [(0u16, 0usize); 4];
        let mut n = 0;
        for b in 0..4 {
            if actifs & (1 << b) != 0 {
                ordre[n] = (self.regs[BGCNT + b] & 3, b);
                n += 1;
            }
        }
        let ordre = &mut ordre[..n];
        ordre.sort_unstable();

        let bldcnt = self.regs[BLDCNT];
        let mode_eff = (bldcnt >> 6) & 3;
        let fond = self.couleur(0);
        let mut ligne = [fond; LARGEUR];
        // Cas courant sans fenêtre ni effet : calques peints de l'arrière vers l'avant.
        if dispcnt & 0xE000 == 0
            && (mode_eff == 0 || bldcnt & 0x3F == 0)
            && !obj.semi.iter().any(|&s| s)
        {
            for p in (0..4).rev() {
                for &(_, b) in ordre.iter().rev().filter(|&&(pb, _)| pb == p) {
                    for (l, &c) in ligne.iter_mut().zip(calques[b].iter()) {
                        if c != VIDE {
                            *l = c;
                        }
                    }
                }
                if obj_actif {
                    for ((l, &c), &po) in ligne.iter_mut().zip(&obj.coul).zip(&obj.prio) {
                        if po == p as u8 {
                            *l = c;
                        }
                    }
                }
            }
            self.vert_inverse(&mut ligne);
            return ligne;
        }

        let masques = self.masques_fenetres(dispcnt, &obj.fenetre);
        let alpha = self.regs[BLDALPHA];
        let eva = (alpha & 0x1F).min(16) as u32;
        let evb = ((alpha >> 8) & 0x1F).min(16) as u32;
        let evy = (self.regs[BLDY] & 0x1F).min(16) as u32;

        for x in 0..LARGEUR {
            let m = masques[x];
            // Les deux calques visibles du dessus : (couleur, bit de cible, sprite semi-transparent).
            let mut couches = [(fond, 0x20u16, false); 2];
            let mut k = 0;
            let o = obj.coul[x];
            let mut obj_place = !obj_actif || o == VIDE || m & 0x10 == 0;
            for &(p, b) in ordre.iter() {
                if !obj_place && obj.prio[x] as u16 <= p {
                    obj_place = true;
                    couches[k] = (o, 0x10, obj.semi[x]);
                    k += 1;
                    if k == 2 {
                        break;
                    }
                }
                let c = calques[b][x];
                if c != VIDE && m & (1 << b) != 0 {
                    couches[k] = (c, 1 << b, false);
                    k += 1;
                    if k == 2 {
                        break;
                    }
                }
            }
            if !obj_place && k < 2 {
                couches[k] = (o, 0x10, obj.semi[x]);
            }
            let (a, ta, semi) = couches[0];
            let (b, tb, _) = couches[1];
            ligne[x] = if m & 0x20 == 0 {
                a
            } else if semi && bldcnt & (tb << 8) != 0 {
                melange(a, b, eva, evb)
            } else if bldcnt & ta == 0 {
                a
            } else {
                match mode_eff {
                    1 if bldcnt & (tb << 8) != 0 => melange(a, b, eva, evb),
                    2 => eclaircit(a, evy),
                    3 => assombrit(a, evy),
                    _ => a,
                }
            };
        }
        self.vert_inverse(&mut ligne);
        ligne
    }

    /// GREENSWAP : composantes vertes échangées entre points pairs et impairs.
    fn vert_inverse(&self, ligne: &mut [u16; LARGEUR]) {
        if self.regs[GREENSWAP] & 1 != 0 {
            for paire in ligne.as_chunks_mut::<2>().0 {
                let (g0, g1) = (paire[0] & 0x3E0, paire[1] & 0x3E0);
                paire[0] = (paire[0] & !0x3E0) | g1;
                paire[1] = (paire[1] & !0x3E0) | g0;
            }
        }
    }

    /// Calques autorisés par pixel (bits 0-3 fonds, 4 sprites, 5 effets).
    fn masques_fenetres(&self, dispcnt: u16, fen_obj: &[bool; LARGEUR]) -> [u8; LARGEUR] {
        if dispcnt & 0xE000 == 0 {
            return [0x3F; LARGEUR];
        }
        let winout = self.regs[WINOUT];
        let mut m = [(winout & 0x3F) as u8; LARGEUR];
        if dispcnt & 0x8000 != 0 {
            let dans = ((winout >> 8) & 0x3F) as u8;
            for (mx, &f) in m.iter_mut().zip(fen_obj.iter()) {
                if f {
                    *mx = dans;
                }
            }
        }
        // Fenêtre 1 puis 0 : la 0 est prioritaire.
        for w in (0..2).rev() {
            if dispcnt & (0x2000 << w) == 0 || !self.fen_v[w] {
                continue;
            }
            let h = self.regs[WIN0H + w];
            let (x1, x2) = ((h >> 8) as usize, (h & 0xFF) as usize);
            let dans = ((self.regs[WININ] >> (w * 8)) & 0x3F) as u8;
            if x1 <= x2 {
                m[x1.min(LARGEUR)..x2.min(LARGEUR)].fill(dans);
            } else {
                m[..x2.min(LARGEUR)].fill(dans);
                m[x1.min(LARGEUR)..].fill(dans);
            }
        }
        m
    }
}

/// Mosaïque horizontale : chaque bloc de `n` pixels reprend son premier pixel.
fn mosaique_h(l: &mut [u16; LARGEUR], n: usize) {
    if n > 1 {
        for x in 0..LARGEUR {
            l[x] = l[x - x % n];
        }
    }
}

#[inline]
fn composantes(c: u16) -> [u32; 3] {
    [
        (c & 0x1F) as u32,
        ((c >> 5) & 0x1F) as u32,
        ((c >> 10) & 0x1F) as u32,
    ]
}

#[inline]
fn assemble(c: [u32; 3]) -> u16 {
    (c[0] | c[1] << 5 | c[2] << 10) as u16
}

/// Mélange alpha : min(31, (a×EVA + b×EVB) / 16) par composante.
fn melange(a: u16, b: u16, eva: u32, evb: u32) -> u16 {
    let (a, b) = (composantes(a), composantes(b));
    assemble([0, 1, 2].map(|i| ((a[i] * eva + b[i] * evb) >> 4).min(31)))
}

fn eclaircit(a: u16, evy: u32) -> u16 {
    assemble(composantes(a).map(|c| c + (((31 - c) * evy) >> 4)))
}

fn assombrit(a: u16, evy: u32) -> u16 {
    assemble(composantes(a).map(|c| c - ((c * evy) >> 4)))
}

// --- Sauvegarde d'état ---

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
        self.ref_aff.iter().flatten().for_each(|&v| e.u32(v as u32));
        self.delai_actif.iter().for_each(|&v| e.u8(v));
        self.fen_v.iter().for_each(|&v| e.bool(v));
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
        for v in p.ref_aff.iter_mut().flatten() {
            *v = l.u32()? as i32;
        }
        for v in &mut p.delai_actif {
            *v = l.u8_max(3, "delai_actif")?;
        }
        for v in &mut p.fen_v {
            *v = l.bool()?;
        }
        *self = p;
        Ok(())
    }
}
