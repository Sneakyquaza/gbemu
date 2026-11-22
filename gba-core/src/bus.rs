//! Bus et carte mémoire : BIOS, EWRAM, IWRAM, E/S, palette, VRAM, OAM, ROM, SRAM ; temps d'accès
//! (WAITCNT, prélecture simple), interruptions, touches, ordonnancement PPU/timers.
//!
//! Modèle de temps : chaque accès ajoute son coût à `cycles` (16,78 MHz) ; quand `cycles` atteint
//! `prochain`, `evenements` fait avancer PPU et timers (et lance les DMA qu'ils déclenchent).

use crate::apu::Apu;
use crate::cartouche::Cartouche;
use crate::dma::{self, Dma};
use crate::ppu::Ppu;
use crate::timer::Timers;

/// Bit IRQ du clavier.
const IRQ_CLAVIER: u16 = 1 << 12;

/// Temps d'attente ROM (premier accès) selon WAITCNT.
const ATTENTE_N: [u32; 4] = [4, 3, 2, 8];

pub struct Bus {
    pub cycles: u64,
    /// Prochain évènement (min PPU / timers).
    pub prochain: u64,
    bios: Box<[u8]>,
    /// Dernier mot lu dans le BIOS (lectures du BIOS depuis l'extérieur : protection).
    bios_latch: u32,
    /// Vrai si la dernière lecture d'instruction était dans le BIOS.
    dans_bios: bool,
    ewram: Box<[u8]>,
    iwram: Box<[u8]>,
    pub ppu: Ppu,
    pub cart: Cartouche,
    timers: Timers,
    pub(crate) apu: Apu,
    pub(crate) dma: [Dma; 4],
    /// Dernières valeurs écrites dans les E/S (registres sans logique propre et base des écritures 8 bits).
    io: Box<[u8]>,
    pub ie: u16,
    pub if_: u16,
    pub ime: bool,
    /// HALTCNT écrit : le CPU attend une interruption (IE & IF).
    pub halt: bool,
    postflg: u8,
    waitcnt: u16,
    prefetch: bool,
    /// Coûts par région (adresse >> 24) : N/S en 16 et 32 bits.
    n16: [u8; 16],
    s16: [u8; 16],
    n32: [u8; 16],
    s32: [u8; 16],
    /// KEYINPUT (bit à 0 = enfoncé).
    keyinput: u16,
    keycnt: u16,
    /// Bus ouvert : dernière instruction lue (Thumb : demi-mot dupliqué).
    ouvert: u32,
}

#[inline]
fn le16(m: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([m[o], m[o + 1]])
}

#[inline]
fn le32(m: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([m[o], m[o + 1], m[o + 2], m[o + 3]])
}

#[inline]
fn ecrit_le16(m: &mut [u8], o: usize, v: u16) {
    m[o..o + 2].copy_from_slice(&v.to_le_bytes());
}

#[inline]
fn ecrit_le32(m: &mut [u8], o: usize, v: u32) {
    m[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

/// VRAM : 96 Kio, miroir tous les 128 Kio, les 32 derniers Kio répétant 0x10000-0x17FFF.
#[inline]
fn vram_off(a: u32) -> usize {
    let o = (a & 0x1FFFF) as usize;
    if o >= 0x18000 {
        o - 0x8000
    } else {
        o
    }
}

impl Bus {
    pub fn new(cart: Cartouche, bios: &[u8]) -> Self {
        let mut b = Bus {
            cycles: 0,
            prochain: 0,
            bios: bios.to_vec().into_boxed_slice(),
            // Valeur lue au démarrage d'un jeu (BIOS officiel, adresse 0xDC + 8).
            bios_latch: 0xE129_F000,
            dans_bios: false,
            ewram: vec![0; 0x40000].into_boxed_slice(),
            iwram: vec![0; 0x8000].into_boxed_slice(),
            ppu: Ppu::new(),
            cart,
            timers: Timers::new(),
            apu: Apu::new(),
            dma: [Dma::default(); 4],
            io: vec![0; 0x400].into_boxed_slice(),
            ie: 0,
            if_: 0,
            ime: false,
            halt: false,
            postflg: 0,
            waitcnt: 0,
            prefetch: false,
            n16: [1; 16],
            s16: [1; 16],
            n32: [1; 16],
            s32: [1; 16],
            keyinput: 0x3FF,
            keycnt: 0,
            ouvert: 0,
        };
        b.maj_attentes();
        b.calcule_prochain();
        b
    }

    /// État des E/S laissé par le BIOS (démarrage direct).
    pub fn apres_bios(&mut self) {
        self.postflg = 1;
        // SOUNDBIAS
        self.apu.ecrit8(0x89, 0x02);
    }

    fn maj_attentes(&mut self) {
        let w = self.waitcnt;
        // EWRAM : 2 états d'attente, bus 16 bits.
        self.n16[2] = 3;
        self.s16[2] = 3;
        self.n32[2] = 6;
        self.s32[2] = 6;
        // Palette et VRAM : bus 16 bits.
        for r in [5, 6] {
            self.n32[r] = 2;
            self.s32[r] = 2;
        }
        let ws = [
            (
                ATTENTE_N[((w >> 2) & 3) as usize],
                [2, 1][((w >> 4) & 1) as usize],
            ),
            (
                ATTENTE_N[((w >> 5) & 3) as usize],
                [4, 1][((w >> 7) & 1) as usize],
            ),
            (
                ATTENTE_N[((w >> 8) & 3) as usize],
                [8, 1][((w >> 10) & 1) as usize],
            ),
        ];
        for (i, &(n, s)) in ws.iter().enumerate() {
            for r in [8 + 2 * i, 9 + 2 * i] {
                self.n16[r] = 1 + n as u8;
                self.s16[r] = 1 + s as u8;
                self.n32[r] = 2 + n as u8 + s as u8;
                self.s32[r] = 2 + 2 * s as u8;
            }
        }
        let sram = 1 + ATTENTE_N[(w & 3) as usize] as u8;
        for r in [0xE, 0xF] {
            self.n16[r] = sram;
            self.s16[r] = sram;
            self.n32[r] = sram;
            self.s32[r] = sram;
        }
        self.prefetch = w & 0x4000 != 0;
    }

    /// Cycles internes du CPU.
    #[inline]
    pub fn inactif(&mut self, n: u32) {
        self.cycles += n as u64;
    }

    // --- Accès CPU avec temps ---

    #[inline]
    fn cout(&mut self, a: u32, seq: bool, mot: bool) {
        let r = ((a >> 24) & 0xF) as usize;
        let c = match (mot, seq) {
            (false, false) => self.n16[r],
            (false, true) => self.s16[r],
            (true, false) => self.n32[r],
            (true, true) => self.s32[r],
        };
        self.cycles += c as u64;
    }

    /// Lecture d'instruction ARM (met à jour le bus ouvert et la protection du BIOS).
    #[inline]
    pub fn fetch32(&mut self, a: u32, seq: bool) -> u32 {
        // raccourci (prélecture) : lecture séquentielle en ROM sans attente si la prélecture est
        // active, au lieu de simuler le tampon de 8 demi-mots.
        if self.prefetch && seq && (0x0800_0000..0x0E00_0000).contains(&a) {
            self.cycles += 2;
        } else {
            self.cout(a, seq, true);
        }
        self.dans_bios = a < 0x4000;
        let v = if self.dans_bios {
            let v = le32(&self.bios, a as usize & 0x3FFC);
            self.bios_latch = v;
            v
        } else {
            self.lit32_brut(a)
        };
        self.ouvert = v;
        v
    }

    /// Lecture d'instruction Thumb.
    #[inline]
    pub fn fetch16(&mut self, a: u32, seq: bool) -> u16 {
        if self.prefetch && seq && (0x0800_0000..0x0E00_0000).contains(&a) {
            self.cycles += 1;
        } else {
            self.cout(a, seq, false);
        }
        self.dans_bios = a < 0x4000;
        let v = if self.dans_bios {
            self.bios_latch = le32(&self.bios, a as usize & 0x3FFC);
            le16(&self.bios, a as usize & 0x3FFE)
        } else {
            self.lit16_brut(a)
        };
        self.ouvert = v as u32 * 0x0001_0001;
        v
    }

    #[inline]
    pub fn lit32(&mut self, a: u32, seq: bool) -> u32 {
        self.cout(a, seq, true);
        self.lit32_brut(a)
    }

    #[inline]
    pub fn lit16(&mut self, a: u32, seq: bool) -> u16 {
        self.cout(a, seq, false);
        self.lit16_brut(a)
    }

    #[inline]
    pub fn lit8(&mut self, a: u32, seq: bool) -> u8 {
        self.cout(a, seq, false);
        self.lit8_brut(a)
    }

    #[inline]
    pub fn ecrit32(&mut self, a: u32, v: u32, seq: bool) {
        self.cout(a, seq, true);
        self.ecrit32_brut(a, v);
    }

    #[inline]
    pub fn ecrit16(&mut self, a: u32, v: u16, seq: bool) {
        self.cout(a, seq, false);
        self.ecrit16_brut(a, v);
    }

    #[inline]
    pub fn ecrit8(&mut self, a: u32, v: u8, seq: bool) {
        self.cout(a, seq, false);
        self.ecrit8_brut(a, v);
    }

    // --- Accès sans temps (adresses alignées par l'appelant pour 16/32 bits) ---

    fn bios32(&self, a: u32) -> u32 {
        if a >= 0x4000 {
            self.ouvert
        } else if self.dans_bios {
            le32(&self.bios, a as usize & 0x3FFC)
        } else {
            self.bios_latch
        }
    }

    pub fn lit32_brut(&self, a: u32) -> u32 {
        match a >> 24 {
            0 => self.bios32(a),
            2 => le32(&self.ewram, a as usize & 0x3FFFC),
            3 => le32(&self.iwram, a as usize & 0x7FFC),
            4 => self.lit_io16(a & !3) as u32 | (self.lit_io16((a & !3) + 2) as u32) << 16,
            5 => le32(&self.ppu.pram, a as usize & 0x3FC),
            6 => le32(&self.ppu.vram, vram_off(a & !3)),
            7 => le32(&self.ppu.oam, a as usize & 0x3FC),
            8..=0xD => self.cart.lit32(a),
            0xE | 0xF => self.cart.lit_sram(a) as u32 * 0x0101_0101,
            _ => self.ouvert,
        }
    }

    pub fn lit16_brut(&self, a: u32) -> u16 {
        match a >> 24 {
            0 => (self.bios32(a) >> ((a & 2) * 8)) as u16,
            2 => le16(&self.ewram, a as usize & 0x3FFFE),
            3 => le16(&self.iwram, a as usize & 0x7FFE),
            4 => self.lit_io16(a & !1),
            5 => le16(&self.ppu.pram, a as usize & 0x3FE),
            6 => le16(&self.ppu.vram, vram_off(a & !1)),
            7 => le16(&self.ppu.oam, a as usize & 0x3FE),
            8..=0xD => self.cart.lit16(a),
            0xE | 0xF => self.cart.lit_sram(a) as u16 * 0x0101,
            _ => (self.ouvert >> ((a & 2) * 8)) as u16,
        }
    }

    pub fn lit8_brut(&self, a: u32) -> u8 {
        match a >> 24 {
            2 => self.ewram[a as usize & 0x3FFFF],
            3 => self.iwram[a as usize & 0x7FFF],
            6 => self.ppu.vram[vram_off(a)],
            8..=0xD => self.cart.lit8(a),
            0xE | 0xF => self.cart.lit_sram(a),
            _ => (self.lit16_brut(a & !1) >> ((a & 1) * 8)) as u8,
        }
    }

    fn ecrit32_brut(&mut self, a: u32, v: u32) {
        match a >> 24 {
            2 => ecrit_le32(&mut self.ewram, a as usize & 0x3FFFC, v),
            3 => ecrit_le32(&mut self.iwram, a as usize & 0x7FFC, v),
            4 => {
                self.ecrit_io16(a & !3, v as u16);
                self.ecrit_io16((a & !3) + 2, (v >> 16) as u16);
            }
            5 => ecrit_le32(&mut self.ppu.pram, a as usize & 0x3FC, v),
            6 => ecrit_le32(&mut self.ppu.vram, vram_off(a & !3), v),
            7 => ecrit_le32(&mut self.ppu.oam, a as usize & 0x3FC, v),
            0xE | 0xF => self.cart.ecrit_sram(a, (v >> ((a & 3) * 8)) as u8),
            _ => {}
        }
    }

    fn ecrit16_brut(&mut self, a: u32, v: u16) {
        match a >> 24 {
            2 => ecrit_le16(&mut self.ewram, a as usize & 0x3FFFE, v),
            3 => ecrit_le16(&mut self.iwram, a as usize & 0x7FFE, v),
            4 => self.ecrit_io16(a & !1, v),
            5 => ecrit_le16(&mut self.ppu.pram, a as usize & 0x3FE, v),
            6 => ecrit_le16(&mut self.ppu.vram, vram_off(a & !1), v),
            7 => ecrit_le16(&mut self.ppu.oam, a as usize & 0x3FE, v),
            0xE | 0xF => self.cart.ecrit_sram(a, (v >> ((a & 1) * 8)) as u8),
            _ => {}
        }
    }

    fn ecrit8_brut(&mut self, a: u32, v: u8) {
        match a >> 24 {
            2 => self.ewram[a as usize & 0x3FFFF] = v,
            3 => self.iwram[a as usize & 0x7FFF] = v,
            4 => self.ecrit_io8(a, v),
            // Palette et fonds en VRAM : l'octet est écrit dans les deux moitiés du demi-mot.
            5 => ecrit_le16(&mut self.ppu.pram, a as usize & 0x3FE, v as u16 * 0x0101),
            6 => {
                let o = vram_off(a & !1);
                let limite = if self.ppu.mode_bitmap() {
                    0x14000
                } else {
                    0x10000
                };
                if o < limite {
                    ecrit_le16(&mut self.ppu.vram, o, v as u16 * 0x0101);
                }
            }
            // OAM : écritures 8 bits ignorées.
            0xE | 0xF => self.cart.ecrit_sram(a, v),
            _ => {}
        }
    }

    // --- E/S ---

    fn lit_io16(&self, a: u32) -> u16 {
        if a & 0x00FF_FFFF >= 0x400 {
            return (self.ouvert >> ((a & 2) * 8)) as u16;
        }
        let o = a & 0x3FE;
        match o {
            0x000..=0x05E => self.ppu.lit_io(o),
            0x060..=0x0AE => self.apu.lit8(o) as u16 | (self.apu.lit8(o + 1) as u16) << 8,
            // DMA : seuls les CNT_H se relisent.
            0x0BA | 0x0C6 | 0x0D2 | 0x0DE => self.dma_lit_ctl(o),
            0x0B0..=0x0DE => 0,
            0x100..=0x10E => self.timers.lit(o, self.cycles),
            0x130 => self.keyinput,
            0x132 => self.keycnt,
            0x200 => self.ie,
            0x202 => self.if_,
            0x204 => self.waitcnt,
            0x208 => self.ime as u16,
            0x300 => self.postflg as u16,
            // raccourci : série et le reste relisent la dernière écriture.
            _ => le16(&self.io, o as usize),
        }
    }

    /// Valeur de base d'une écriture 8 bits (relecture du demi-mot).
    fn base_io16(&self, o: u32) -> u16 {
        match o {
            0x0BA | 0x0C6 | 0x0D2 | 0x0DE => self.dma_lit_ctl(o),
            0x100 | 0x104 | 0x108 | 0x10C => self.timers.reload(o),
            0x004 => self.ppu.lit_io(o),
            _ => le16(&self.io, o as usize),
        }
    }

    fn ecrit_io16(&mut self, a: u32, v: u16) {
        if a & 0x00FF_FFFF >= 0x400 {
            return;
        }
        let o = a & 0x3FE;
        if o == 0x300 {
            self.ecrit_io8(a, v as u8);
            self.ecrit_io8(a + 1, (v >> 8) as u8);
            return;
        }
        ecrit_le16(&mut self.io, o as usize, v);
        match o {
            0x000..=0x05E => self.ppu.ecrit_io(o, v),
            // Octet par octet : un déclenchement (NRx4) ne doit pas suivre l'écriture de NRx3.
            0x060..=0x0AE => {
                self.son_ecrit8(o, v as u8);
                self.son_ecrit8(o + 1, (v >> 8) as u8);
            }
            0x0B0..=0x0DE => self.dma_ecrit(o, v),
            0x100..=0x10E => {
                let irq = self.timers.ecrit(o, v, self.cycles);
                self.if_ |= irq;
                self.calcule_prochain();
            }
            0x132 => {
                self.keycnt = v;
                self.verifie_clavier();
            }
            0x200 => self.ie = v,
            0x202 => self.if_ &= !v,
            0x204 => {
                self.waitcnt = v & 0x5FFF;
                self.maj_attentes();
            }
            0x208 => self.ime = v & 1 != 0,
            _ => {}
        }
    }

    fn ecrit_io8(&mut self, a: u32, v: u8) {
        if a & 0x00FF_FFFF >= 0x400 {
            return;
        }
        let o = a & 0x3FF;
        match o {
            0x202 => self.if_ &= !(v as u16),
            0x203 => self.if_ &= !((v as u16) << 8),
            0x300 => {
                self.postflg = v & 1;
                self.io[0x300] = v;
            }
            0x060..=0x0AF => self.son_ecrit8(o, v),
            // HALTCNT : arrêt jusqu'à une interruption (raccourci : STOP traité comme HALT).
            0x301 => self.halt = true,
            _ => {
                let base = self.base_io16(o & !1);
                let sh = (o & 1) * 8;
                let nv = (base & !(0xFF << sh)) | (v as u16) << sh;
                self.ecrit_io16(o & !1, nv);
            }
        }
    }

    // --- Touches et interruptions ---

    /// Touches enfoncées (bit à 1, voir `crate::touche`).
    pub fn set_touches(&mut self, t: u16) {
        self.keyinput = !t & 0x3FF;
        self.verifie_clavier();
    }

    fn verifie_clavier(&mut self) {
        let c = self.keycnt;
        if c & 0x4000 == 0 {
            return;
        }
        let enfoncees = !self.keyinput & c & 0x3FF;
        let ok = if c & 0x8000 != 0 {
            enfoncees == c & 0x3FF && enfoncees != 0
        } else {
            enfoncees != 0
        };
        if ok {
            self.if_ |= IRQ_CLAVIER;
        }
    }

    /// Une interruption est due et autorisée par IME (le bit I du CPSR est vérifié par le CPU).
    #[inline]
    pub fn irq_en_attente(&self) -> bool {
        self.ime && self.ie & self.if_ & 0x3FFF != 0
    }

    /// Sortie de HALT : IE & IF non nul, même avec IME à 0.
    #[inline]
    pub fn reveil(&self) -> bool {
        self.ie & self.if_ & 0x3FFF != 0
    }

    // --- Évènements ---

    fn calcule_prochain(&mut self) {
        self.prochain = self.ppu.prochain.min(self.timers.prochain);
    }

    /// Fait avancer PPU et timers jusqu'à `cycles`.
    pub fn evenements(&mut self) {
        while self.cycles >= self.prochain {
            if self.cycles >= self.ppu.prochain {
                let ev = self.ppu.evenement();
                self.if_ |= ev.irq;
                if ev.dma_hblank {
                    self.dma_declenche(dma::HBLANK);
                }
                if ev.dma_vblank {
                    self.dma_declenche(dma::VBLANK);
                }
                if ev.dma_capture {
                    self.dma_declenche(dma::SPECIAL);
                }
                if ev.fin_capture {
                    self.dma_fin_capture();
                }
            }
            if self.cycles >= self.timers.prochain {
                let (irq, debs) = self.timers.avance(self.cycles);
                self.if_ |= irq;
                self.son_debordements(debs);
            }
            self.calcule_prochain();
        }
    }
}
