//! CPU ARM7TDMI : états ARM et Thumb, modes et registres banqués, CPSR/SPSR, exceptions.
//! Pipeline à deux étages : `pipe[0]` = prochaine instruction à exécuter, `pipe[1]` = la suivante
//! (dernier mot lu, aussi valeur du bus ouvert). Pendant l'exécution d'une instruction d'adresse A,
//! `r[15]` vaut A + 8 (ARM) ou A + 4 (Thumb), comme sur la vraie puce.

mod arm;
mod thumb;

use crate::bus::Bus;

pub const MODE_USR: u32 = 0x10;
pub const MODE_FIQ: u32 = 0x11;
pub const MODE_IRQ: u32 = 0x12;
pub const MODE_SVC: u32 = 0x13;
pub const MODE_UND: u32 = 0x1B;
pub const MODE_SYS: u32 = 0x1F;

pub const N: u32 = 1 << 31;
pub const Z: u32 = 1 << 30;
pub const C: u32 = 1 << 29;
pub const V: u32 = 1 << 28;
pub const I: u32 = 1 << 7;
pub const F: u32 = 1 << 6;
pub const T: u32 = 1 << 5;

/// Banque de r13/r14/SPSR du mode : 0 = utilisateur/système (et modes invalides), 1 = FIQ,
/// 2 = IRQ, 3 = superviseur, 4 = abandon, 5 = indéfini.
fn banque(mode: u32) -> usize {
    match mode & 0x1F {
        0x11 => 1,
        0x12 => 2,
        0x13 => 3,
        0x17 => 4,
        0x1B => 5,
        _ => 0,
    }
}

/// Table des conditions : bit `nzcv` de `COND[cond]` à 1 si la condition est vraie.
const COND: [u16; 16] = {
    let mut t = [0u16; 16];
    let mut c = 0;
    while c < 16 {
        let mut f = 0;
        while f < 16 {
            let (n, z, cy, v) = (f & 8 != 0, f & 4 != 0, f & 2 != 0, f & 1 != 0);
            let ok = match c {
                0 => z,
                1 => !z,
                2 => cy,
                3 => !cy,
                4 => n,
                5 => !n,
                6 => v,
                7 => !v,
                8 => cy && !z,
                9 => !cy || z,
                10 => n == v,
                11 => n != v,
                12 => !z && n == v,
                13 => z || n != v,
                14 => true,
                _ => false, // 1111 : « jamais » en ARMv4
            };
            if ok {
                t[c] |= 1 << f;
            }
            f += 1;
        }
        c += 1;
    }
    t
};

pub struct Cpu {
    pub r: [u32; 16],
    pub cpsr: u32,
    /// SPSR du mode courant (inutilisé en utilisateur/système).
    pub spsr: u32,
    /// r13/r14 sauvés par banque (la banque courante est dans `r`).
    r13_14: [[u32; 2]; 6],
    spsr_b: [u32; 6],
    /// r8-r12 de l'autre jeu (utilisateur si on est en FIQ, FIQ sinon).
    r8_12_autre: [u32; 5],
    pipe: [u32; 2],
    /// Prochaine lecture d'instruction séquentielle (faux après un accès donnée ou un saut).
    seq: bool,
}

impl Cpu {
    /// État après le BIOS (démarrage direct en 0x08000000, mode système).
    pub fn new_direct(bus: &mut Bus) -> Self {
        let mut c = Self::vide();
        c.r13_14[2][0] = 0x0300_7FA0;
        c.r13_14[3][0] = 0x0300_7FE0;
        c.r[13] = 0x0300_7F00;
        c.cpsr = MODE_SYS;
        c.r[15] = 0x0800_0000;
        c.flush(bus);
        c
    }

    /// État à la mise sous tension (exécution du BIOS depuis 0).
    pub fn new_bios(bus: &mut Bus) -> Self {
        let mut c = Self::vide();
        c.cpsr = MODE_SVC | I | F;
        c.flush(bus);
        c
    }

    fn vide() -> Self {
        Cpu {
            r: [0; 16],
            cpsr: 0,
            spsr: 0,
            r13_14: [[0; 2]; 6],
            spsr_b: [0; 6],
            r8_12_autre: [0; 5],
            pipe: [0; 2],
            seq: false,
        }
    }

    pub fn thumb(&self) -> bool {
        self.cpsr & T != 0
    }

    /// Adresse de la prochaine instruction exécutée.
    pub fn pc(&self) -> u32 {
        self.r[15].wrapping_sub(if self.thumb() { 2 } else { 4 })
    }

    /// Vrai si la prochaine instruction est un saut sur elle-même (fin des ROM de test).
    pub fn boucle_infinie(&self) -> bool {
        if self.thumb() {
            self.pipe[0] & 0xFFFF == 0xE7FE
        } else {
            self.pipe[0] == 0xEAFF_FFFE
        }
    }

    /// Change de mode en échangeant les registres banqués.
    fn change_mode(&mut self, mode: u32) {
        let a = banque(self.cpsr);
        let b = banque(mode);
        if a != b {
            self.r13_14[a] = [self.r[13], self.r[14]];
            self.spsr_b[a] = self.spsr;
            if a == 1 || b == 1 {
                for i in 0..5 {
                    std::mem::swap(&mut self.r[8 + i], &mut self.r8_12_autre[i]);
                }
            }
            [self.r[13], self.r[14]] = self.r13_14[b];
            self.spsr = self.spsr_b[b];
        }
        self.cpsr = (self.cpsr & !0x1F) | (mode & 0x1F);
    }

    /// Écrit tout le CPSR (changement de mode compris).
    fn ecrit_cpsr(&mut self, v: u32) {
        self.change_mode(v);
        self.cpsr = v;
    }

    /// Copie SPSR → CPSR (retour d'exception), sans effet en utilisateur/système.
    fn restaure_cpsr(&mut self) {
        if banque(self.cpsr) != 0 {
            let s = self.spsr;
            self.ecrit_cpsr(s);
        }
    }

    /// Registre du mode utilisateur (LDM/STM avec le bit S).
    fn reg_usr(&self, i: usize) -> u32 {
        let b = banque(self.cpsr);
        match i {
            8..=12 if b == 1 => self.r8_12_autre[i - 8],
            13 | 14 if b != 0 => self.r13_14[0][i - 13],
            _ => self.r[i],
        }
    }

    fn ecrit_reg_usr(&mut self, i: usize, v: u32) {
        let b = banque(self.cpsr);
        match i {
            8..=12 if b == 1 => self.r8_12_autre[i - 8] = v,
            13 | 14 if b != 0 => self.r13_14[0][i - 13] = v,
            _ => self.r[i] = v,
        }
    }

    /// Valeur de PC stockée par STR/STM/PUSH : adresse + 12 (ARM) ou + 6 (Thumb).
    fn pc_stocke(&self) -> u32 {
        self.r[15].wrapping_add(if self.thumb() { 2 } else { 4 })
    }

    /// Vide le pipeline et relit depuis `r[15]` (après tout saut ou écriture de PC).
    pub fn flush(&mut self, bus: &mut Bus) {
        if self.thumb() {
            self.r[15] &= !1;
            self.pipe[0] = bus.fetch16(self.r[15], false) as u32;
            self.r[15] = self.r[15].wrapping_add(2);
            self.pipe[1] = bus.fetch16(self.r[15], true) as u32;
        } else {
            self.r[15] &= !3;
            self.pipe[0] = bus.fetch32(self.r[15], false);
            self.r[15] = self.r[15].wrapping_add(4);
            self.pipe[1] = bus.fetch32(self.r[15], true);
        }
        self.seq = true;
    }

    /// Exécute une instruction.
    #[inline]
    pub fn step(&mut self, bus: &mut Bus) {
        if self.cpsr & T != 0 {
            let op = self.pipe[0];
            self.pipe[0] = self.pipe[1];
            self.r[15] = self.r[15].wrapping_add(2);
            self.pipe[1] = bus.fetch16(self.r[15], self.seq) as u32;
            self.seq = true;
            self.thumb_exec(bus, op & 0xFFFF);
        } else {
            let op = self.pipe[0];
            self.pipe[0] = self.pipe[1];
            self.r[15] = self.r[15].wrapping_add(4);
            self.pipe[1] = bus.fetch32(self.r[15], self.seq);
            self.seq = true;
            if self.condition(op >> 28) {
                self.arm_exec(bus, op);
            }
        }
    }

    #[inline]
    fn condition(&self, c: u32) -> bool {
        COND[c as usize] & (1 << (self.cpsr >> 28)) != 0
    }

    /// Entrée dans une exception : SPSR = CPSR, changement de mode, LR, état ARM, IRQ masquées.
    fn exception(&mut self, bus: &mut Bus, vecteur: u32, mode: u32, lr: u32) {
        let ancien = self.cpsr;
        self.change_mode(mode);
        self.spsr = ancien;
        self.r[14] = lr;
        self.cpsr = (self.cpsr & !T) | I | if mode == MODE_FIQ { F } else { 0 };
        self.r[15] = vecteur;
        self.flush(bus);
    }

    /// Interruption matérielle (le bit I est vérifié par l'appelant).
    pub fn irq(&mut self, bus: &mut Bus) {
        // LR = prochaine instruction + 4 (retour par SUBS PC, LR, #4).
        let lr = self.r[15].wrapping_add(if self.thumb() { 2 } else { 0 });
        self.exception(bus, 0x18, MODE_IRQ, lr);
    }

    /// SWI : LR = instruction suivante.
    fn swi(&mut self, bus: &mut Bus) {
        let lr = self.r[15].wrapping_sub(if self.thumb() { 2 } else { 4 });
        self.exception(bus, 0x08, MODE_SVC, lr);
    }

    /// Instruction indéfinie (coprocesseurs, encodages réservés).
    fn indefinie(&mut self, bus: &mut Bus) {
        let lr = self.r[15].wrapping_sub(if self.thumb() { 2 } else { 4 });
        self.exception(bus, 0x04, MODE_UND, lr);
    }

    // --- Drapeaux et décalages ---

    #[inline]
    fn carry(&self) -> bool {
        self.cpsr & C != 0
    }

    #[inline]
    fn nz(&mut self, v: u32) {
        self.cpsr = (self.cpsr & !(N | Z)) | (v & N) | if v == 0 { Z } else { 0 };
    }

    #[inline]
    fn set_c(&mut self, c: bool) {
        self.cpsr = (self.cpsr & !C) | if c { C } else { 0 };
    }

    /// a + b + retenue ; met N, Z, C, V si `s`.
    #[inline]
    fn add(&mut self, a: u32, b: u32, retenue: bool, s: bool) -> u32 {
        let r64 = a as u64 + b as u64 + retenue as u64;
        let r = r64 as u32;
        if s {
            self.nz(r);
            let v = ((a ^ r) & (b ^ r)) >> 31 != 0;
            self.cpsr = (self.cpsr & !(C | V))
                | if r64 > 0xFFFF_FFFF { C } else { 0 }
                | if v { V } else { 0 };
        }
        r
    }

    /// a - b - !retenue (C = pas d'emprunt).
    #[inline]
    fn sub(&mut self, a: u32, b: u32, retenue: bool, s: bool) -> u32 {
        self.add(a, !b, retenue, s)
    }

    /// Décalage par une quantité immédiate (0 = LSR/ASR #32, RRX).
    #[inline]
    fn decale_imm(&self, typ: u32, v: u32, n: u32) -> (u32, bool) {
        match (typ, n) {
            (0, 0) => (v, self.carry()),
            (0, _) => (v << n, (v >> (32 - n)) & 1 != 0),
            (1, 0) => (0, v >> 31 != 0),
            (1, _) => (v >> n, (v >> (n - 1)) & 1 != 0),
            (2, 0) => (((v as i32) >> 31) as u32, v >> 31 != 0),
            (2, _) => (((v as i32) >> n) as u32, (v >> (n - 1)) & 1 != 0),
            (_, 0) => (((self.carry() as u32) << 31) | (v >> 1), v & 1 != 0),
            _ => (v.rotate_right(n), (v >> (n - 1)) & 1 != 0),
        }
    }

    /// Décalage par registre (quantité 0-255).
    #[inline]
    fn decale_reg(&self, typ: u32, v: u32, n: u32) -> (u32, bool) {
        if n == 0 {
            return (v, self.carry());
        }
        match typ {
            0 => match n {
                1..=31 => (v << n, (v >> (32 - n)) & 1 != 0),
                32 => (0, v & 1 != 0),
                _ => (0, false),
            },
            1 => match n {
                1..=31 => (v >> n, (v >> (n - 1)) & 1 != 0),
                32 => (0, v >> 31 != 0),
                _ => (0, false),
            },
            2 => {
                if n < 32 {
                    (((v as i32) >> n) as u32, (v >> (n - 1)) & 1 != 0)
                } else {
                    (((v as i32) >> 31) as u32, v >> 31 != 0)
                }
            }
            _ => {
                let r = n & 31;
                if r == 0 {
                    (v, v >> 31 != 0)
                } else {
                    (v.rotate_right(r), (v >> (r - 1)) & 1 != 0)
                }
            }
        }
    }
}

/// Cycles internes d'une multiplication selon l'opérande `rs` (1 à 4 ; `signe` : 0xFF… compte court).
#[inline]
fn cycles_mul(rs: u32, signe: bool) -> u32 {
    let m = |masque: u32| rs & masque == 0 || (signe && rs & masque == masque);
    if m(0xFFFF_FF00) {
        1
    } else if m(0xFFFF_0000) {
        2
    } else if m(0xFF00_0000) {
        3
    } else {
        4
    }
}
