//! CPU SM83 : jeu d'instructions complet (+ préfixe CB), cycle par cycle machine.

use crate::bus::Bus;
use crate::etat::{Ecrit, ErreurEtat, Lecteur};

const FZ: u8 = 0x80;
const FN: u8 = 0x40;
const FH: u8 = 0x20;
const FC: u8 = 0x10;

pub struct Cpu {
    pub a: u8,
    pub f: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub sp: u16,
    pub pc: u16,
    ime: bool,
    /// EI exécuté : IME passe à 1 après l'instruction suivante.
    ei_pending: bool,
    halted: bool,
    /// Bug du HALT : le prochain octet d'opcode est lu sans avancer PC.
    halt_bug: bool,
    /// Opcode illégal exécuté : le CPU est bloqué (comme le matériel).
    locked: bool,
    /// `LD B,B` exécuté (point d'arrêt logiciel des tests mooneye).
    pub ld_b_b: bool,
}

impl Cpu {
    /// Registres après la boot ROM DMG, ou CGB (jeu CGB).
    pub fn new(cgb: bool) -> Self {
        let [a, f, b, c, d, e, h, l] = if cgb {
            [0x11, 0x80, 0x00, 0x00, 0xFF, 0x56, 0x00, 0x0D]
        } else {
            [0x01, 0xB0, 0x00, 0x13, 0x00, 0xD8, 0x01, 0x4D]
        };
        Cpu {
            a,
            f,
            b,
            c,
            d,
            e,
            h,
            l,
            sp: 0xFFFE,
            pc: 0x0100,
            ime: false,
            ei_pending: false,
            halted: false,
            halt_bug: false,
            locked: false,
            ld_b_b: false,
        }
    }

    pub fn sauve(&self, e: &mut Vec<u8>) {
        e.octets(&[
            self.a, self.f, self.b, self.c, self.d, self.e, self.h, self.l,
        ]);
        e.u16(self.sp);
        e.u16(self.pc);
        for v in [
            self.ime,
            self.ei_pending,
            self.halted,
            self.halt_bug,
            self.locked,
            self.ld_b_b,
        ] {
            e.bool(v);
        }
    }

    pub fn charge(&mut self, l: &mut Lecteur) -> Result<(), ErreurEtat> {
        let mut r = [0u8; 8];
        l.octets(&mut r)?;
        [
            self.a, self.f, self.b, self.c, self.d, self.e, self.h, self.l,
        ] = r;
        self.f &= 0xF0;
        self.sp = l.u16()?;
        self.pc = l.u16()?;
        self.ime = l.bool()?;
        self.ei_pending = l.bool()?;
        self.halted = l.bool()?;
        self.halt_bug = l.bool()?;
        self.locked = l.bool()?;
        self.ld_b_b = l.bool()?;
        Ok(())
    }

    #[inline]
    fn hl(&self) -> u16 {
        u16::from_be_bytes([self.h, self.l])
    }

    #[inline]
    fn set_hl(&mut self, v: u16) {
        [self.h, self.l] = v.to_be_bytes();
    }

    /// Paires BC, DE, HL, SP (index 0-3).
    fn rr(&self, i: u8) -> u16 {
        match i & 3 {
            0 => u16::from_be_bytes([self.b, self.c]),
            1 => u16::from_be_bytes([self.d, self.e]),
            2 => self.hl(),
            _ => self.sp,
        }
    }

    fn set_rr(&mut self, i: u8, v: u16) {
        match i & 3 {
            0 => [self.b, self.c] = v.to_be_bytes(),
            1 => [self.d, self.e] = v.to_be_bytes(),
            2 => self.set_hl(v),
            _ => self.sp = v,
        }
    }

    /// Registres 8 bits B, C, D, E, H, L, (HL), A (index 0-7).
    #[inline]
    fn r8(&mut self, i: u8, bus: &mut Bus) -> u8 {
        match i & 7 {
            0 => self.b,
            1 => self.c,
            2 => self.d,
            3 => self.e,
            4 => self.h,
            5 => self.l,
            6 => bus.read(self.hl()),
            _ => self.a,
        }
    }

    #[inline]
    fn set_r8(&mut self, i: u8, v: u8, bus: &mut Bus) {
        match i & 7 {
            0 => self.b = v,
            1 => self.c = v,
            2 => self.d = v,
            3 => self.e = v,
            4 => self.h = v,
            5 => self.l = v,
            6 => bus.write(self.hl(), v),
            _ => self.a = v,
        }
    }

    #[inline]
    fn fetch(&mut self, bus: &mut Bus) -> u8 {
        let v = bus.read(self.pc);
        if self.halt_bug {
            self.halt_bug = false;
        } else {
            self.pc = self.pc.wrapping_add(1);
        }
        v
    }

    fn fetch16(&mut self, bus: &mut Bus) -> u16 {
        let lo = self.fetch(bus);
        let hi = self.fetch(bus);
        u16::from_le_bytes([lo, hi])
    }

    fn push(&mut self, bus: &mut Bus, v: u16) {
        let [hi, lo] = v.to_be_bytes();
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, hi);
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, lo);
    }

    fn pop(&mut self, bus: &mut Bus) -> u16 {
        let lo = bus.read(self.sp);
        self.sp = self.sp.wrapping_add(1);
        let hi = bus.read(self.sp);
        self.sp = self.sp.wrapping_add(1);
        u16::from_le_bytes([lo, hi])
    }

    fn cond(&self, op: u8) -> bool {
        match (op >> 3) & 3 {
            0 => self.f & FZ == 0,
            1 => self.f & FZ != 0,
            2 => self.f & FC == 0,
            _ => self.f & FC != 0,
        }
    }

    /// Exécute une instruction (ou un cycle d'attente en HALT, ou une prise d'interruption).
    pub fn step(&mut self, bus: &mut Bus) {
        if self.locked {
            bus.tick();
            return;
        }
        if self.halted {
            if bus.pending_irq() == 0 {
                bus.tick();
                return;
            }
            self.halted = false;
        }
        if self.ime && bus.pending_irq() != 0 {
            self.interrupt(bus);
            return;
        }
        if self.ei_pending {
            self.ei_pending = false;
            self.ime = true;
        }
        let op = self.fetch(bus);
        self.execute(op, bus);
    }

    fn interrupt(&mut self, bus: &mut Bus) {
        self.ime = false;
        bus.tick();
        bus.tick();
        let [hi, lo] = self.pc.to_be_bytes();
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, hi);
        // Le vecteur est choisi après l'empilement de l'octet haut (qui peut écrire IE).
        let pending = bus.pending_irq();
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, lo);
        if pending == 0 {
            self.pc = 0;
        } else {
            let bit = pending.trailing_zeros();
            bus.if_ &= !(1 << bit);
            self.pc = 0x40 + 8 * bit as u16;
        }
        bus.tick();
    }

    fn alu(&mut self, op: u8, v: u8) {
        let a = self.a;
        let carry = (self.f & FC != 0) as u8;
        match op & 7 {
            0 | 1 => {
                let c = if op & 7 == 1 { carry } else { 0 };
                let r = a as u16 + v as u16 + c as u16;
                let h = (a & 0x0F) + (v & 0x0F) + c > 0x0F;
                self.a = r as u8;
                self.f = z(self.a) | flag(h, FH) | flag(r > 0xFF, FC);
            }
            2 | 3 | 7 => {
                let c = if op & 7 == 3 { carry } else { 0 };
                let r = (a as i16) - (v as i16) - (c as i16);
                let h = ((a & 0x0F) as i16) - ((v & 0x0F) as i16) - (c as i16) < 0;
                let res = r as u8;
                self.f = z(res) | FN | flag(h, FH) | flag(r < 0, FC);
                if op & 7 != 7 {
                    self.a = res;
                }
            }
            4 => {
                self.a = a & v;
                self.f = z(self.a) | FH;
            }
            5 => {
                self.a = a ^ v;
                self.f = z(self.a);
            }
            _ => {
                self.a = a | v;
                self.f = z(self.a);
            }
        }
    }

    fn add_sp_e(&mut self, bus: &mut Bus) -> u16 {
        let e = self.fetch(bus);
        let sp = self.sp;
        let h = (sp & 0x0F) + (e as u16 & 0x0F) > 0x0F;
        let c = (sp & 0xFF) + e as u16 > 0xFF;
        self.f = flag(h, FH) | flag(c, FC);
        sp.wrapping_add(e as i8 as u16)
    }

    fn execute(&mut self, op: u8, bus: &mut Bus) {
        match op {
            0x00 => {}
            0x01 | 0x11 | 0x21 | 0x31 => {
                let v = self.fetch16(bus);
                self.set_rr(op >> 4, v);
            }
            0x02 | 0x12 => bus.write(self.rr(op >> 4), self.a),
            0x22 => {
                let hl = self.hl();
                bus.write(hl, self.a);
                self.set_hl(hl.wrapping_add(1));
            }
            0x32 => {
                let hl = self.hl();
                bus.write(hl, self.a);
                self.set_hl(hl.wrapping_sub(1));
            }
            0x0A | 0x1A => self.a = bus.read(self.rr(op >> 4)),
            0x2A => {
                let hl = self.hl();
                self.a = bus.read(hl);
                self.set_hl(hl.wrapping_add(1));
            }
            0x3A => {
                let hl = self.hl();
                self.a = bus.read(hl);
                self.set_hl(hl.wrapping_sub(1));
            }
            0x03 | 0x13 | 0x23 | 0x33 => {
                let i = op >> 4;
                self.set_rr(i, self.rr(i).wrapping_add(1));
                bus.tick();
            }
            0x0B | 0x1B | 0x2B | 0x3B => {
                let i = op >> 4;
                self.set_rr(i, self.rr(i).wrapping_sub(1));
                bus.tick();
            }
            0x04 | 0x0C | 0x14 | 0x1C | 0x24 | 0x2C | 0x34 | 0x3C => {
                let i = op >> 3;
                let v = self.r8(i, bus).wrapping_add(1);
                self.f = (self.f & FC) | z(v) | flag(v & 0x0F == 0, FH);
                self.set_r8(i, v, bus);
            }
            0x05 | 0x0D | 0x15 | 0x1D | 0x25 | 0x2D | 0x35 | 0x3D => {
                let i = op >> 3;
                let v = self.r8(i, bus).wrapping_sub(1);
                self.f = (self.f & FC) | z(v) | FN | flag(v & 0x0F == 0x0F, FH);
                self.set_r8(i, v, bus);
            }
            0x06 | 0x0E | 0x16 | 0x1E | 0x26 | 0x2E | 0x36 | 0x3E => {
                let v = self.fetch(bus);
                self.set_r8(op >> 3, v, bus);
            }
            0x07 => {
                self.a = self.a.rotate_left(1);
                self.f = flag(self.a & 1 != 0, FC);
            }
            0x0F => {
                self.f = flag(self.a & 1 != 0, FC);
                self.a = self.a.rotate_right(1);
            }
            0x17 => {
                let c = (self.f & FC != 0) as u8;
                self.f = flag(self.a & 0x80 != 0, FC);
                self.a = (self.a << 1) | c;
            }
            0x1F => {
                let c = (self.f & FC != 0) as u8;
                self.f = flag(self.a & 1 != 0, FC);
                self.a = (self.a >> 1) | (c << 7);
            }
            0x08 => {
                let addr = self.fetch16(bus);
                let [hi, lo] = self.sp.to_be_bytes();
                bus.write(addr, lo);
                bus.write(addr.wrapping_add(1), hi);
            }
            0x09 | 0x19 | 0x29 | 0x39 => {
                let hl = self.hl();
                let v = self.rr(op >> 4);
                let r = hl as u32 + v as u32;
                let h = (hl & 0x0FFF) + (v & 0x0FFF) > 0x0FFF;
                self.f = (self.f & FZ) | flag(h, FH) | flag(r > 0xFFFF, FC);
                self.set_hl(r as u16);
                bus.tick();
            }
            0x10 => {
                self.fetch(bus);
                bus.stop();
            }
            0x18 => {
                let e = self.fetch(bus) as i8;
                self.pc = self.pc.wrapping_add(e as u16);
                bus.tick();
            }
            0x20 | 0x28 | 0x30 | 0x38 => {
                let e = self.fetch(bus) as i8;
                if self.cond(op) {
                    self.pc = self.pc.wrapping_add(e as u16);
                    bus.tick();
                }
            }
            0x27 => {
                let mut a = self.a;
                let mut carry = self.f & FC != 0;
                if self.f & FN == 0 {
                    if carry || a > 0x99 {
                        a = a.wrapping_add(0x60);
                        carry = true;
                    }
                    if self.f & FH != 0 || a & 0x0F > 0x09 {
                        a = a.wrapping_add(0x06);
                    }
                } else {
                    if carry {
                        a = a.wrapping_sub(0x60);
                    }
                    if self.f & FH != 0 {
                        a = a.wrapping_sub(0x06);
                    }
                }
                self.a = a;
                self.f = z(a) | (self.f & FN) | flag(carry, FC);
            }
            0x2F => {
                self.a = !self.a;
                self.f |= FN | FH;
            }
            0x37 => self.f = (self.f & FZ) | FC,
            0x3F => self.f = (self.f & (FZ | FC)) ^ FC,
            0x76 => {
                if !self.ime && bus.pending_irq() != 0 {
                    self.halt_bug = true;
                } else {
                    self.halted = true;
                }
            }
            0x40..=0x7F => {
                if op == 0x40 {
                    self.ld_b_b = true;
                }
                let v = self.r8(op, bus);
                self.set_r8(op >> 3, v, bus);
            }
            0x80..=0xBF => {
                let v = self.r8(op, bus);
                self.alu(op >> 3, v);
            }
            0xC0 | 0xC8 | 0xD0 | 0xD8 => {
                bus.tick();
                if self.cond(op) {
                    self.pc = self.pop(bus);
                    bus.tick();
                }
            }
            0xC1 | 0xD1 | 0xE1 => {
                let v = self.pop(bus);
                self.set_rr((op >> 4) & 3, v);
            }
            0xF1 => {
                let [a, f] = self.pop(bus).to_be_bytes();
                self.a = a;
                self.f = f & 0xF0;
            }
            0xC2 | 0xCA | 0xD2 | 0xDA => {
                let addr = self.fetch16(bus);
                if self.cond(op) {
                    self.pc = addr;
                    bus.tick();
                }
            }
            0xC3 => {
                self.pc = self.fetch16(bus);
                bus.tick();
            }
            0xC4 | 0xCC | 0xD4 | 0xDC => {
                let addr = self.fetch16(bus);
                if self.cond(op) {
                    bus.tick();
                    self.push(bus, self.pc);
                    self.pc = addr;
                }
            }
            0xC5 | 0xD5 | 0xE5 => {
                bus.tick();
                let v = self.rr((op >> 4) & 3);
                self.push(bus, v);
            }
            0xF5 => {
                bus.tick();
                let v = u16::from_be_bytes([self.a, self.f]);
                self.push(bus, v);
            }
            0xC6 | 0xCE | 0xD6 | 0xDE | 0xE6 | 0xEE | 0xF6 | 0xFE => {
                let v = self.fetch(bus);
                self.alu(op >> 3, v);
            }
            0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF => {
                bus.tick();
                self.push(bus, self.pc);
                self.pc = (op & 0x38) as u16;
            }
            0xC9 => {
                self.pc = self.pop(bus);
                bus.tick();
            }
            0xD9 => {
                self.pc = self.pop(bus);
                bus.tick();
                self.ime = true;
            }
            0xCB => {
                let cb = self.fetch(bus);
                self.execute_cb(cb, bus);
            }
            0xCD => {
                let addr = self.fetch16(bus);
                bus.tick();
                self.push(bus, self.pc);
                self.pc = addr;
            }
            0xE0 => {
                let n = self.fetch(bus);
                bus.write(0xFF00 | n as u16, self.a);
            }
            0xF0 => {
                let n = self.fetch(bus);
                self.a = bus.read(0xFF00 | n as u16);
            }
            0xE2 => bus.write(0xFF00 | self.c as u16, self.a),
            0xF2 => self.a = bus.read(0xFF00 | self.c as u16),
            0xE8 => {
                self.sp = self.add_sp_e(bus);
                bus.tick();
                bus.tick();
            }
            0xF8 => {
                let v = self.add_sp_e(bus);
                self.set_hl(v);
                bus.tick();
            }
            0xE9 => self.pc = self.hl(),
            0xF9 => {
                self.sp = self.hl();
                bus.tick();
            }
            0xEA => {
                let addr = self.fetch16(bus);
                bus.write(addr, self.a);
            }
            0xFA => {
                let addr = self.fetch16(bus);
                self.a = bus.read(addr);
            }
            0xF3 => {
                self.ime = false;
                self.ei_pending = false;
            }
            0xFB => self.ei_pending = true,
            // D3 DB DD E3 E4 EB EC ED F4 FC FD : opcodes illégaux, le CPU se bloque.
            _ => self.locked = true,
        }
    }

    fn execute_cb(&mut self, op: u8, bus: &mut Bus) {
        let i = op & 7;
        let v = self.r8(i, bus);
        let bit = (op >> 3) & 7;
        let carry = (self.f & FC != 0) as u8;
        let r = match op >> 6 {
            0 => {
                let (r, c) = match bit {
                    0 => (v.rotate_left(1), v & 0x80 != 0),
                    1 => (v.rotate_right(1), v & 1 != 0),
                    2 => ((v << 1) | carry, v & 0x80 != 0),
                    3 => ((v >> 1) | (carry << 7), v & 1 != 0),
                    4 => (v << 1, v & 0x80 != 0),
                    5 => (((v as i8) >> 1) as u8, v & 1 != 0),
                    6 => (v.rotate_left(4), false),
                    _ => (v >> 1, v & 1 != 0),
                };
                self.f = z(r) | flag(c, FC);
                r
            }
            1 => {
                self.f = (self.f & FC) | FH | z(v & (1 << bit));
                return;
            }
            2 => v & !(1 << bit),
            _ => v | (1 << bit),
        };
        self.set_r8(i, r, bus);
    }
}

#[inline]
fn z(v: u8) -> u8 {
    if v == 0 {
        FZ
    } else {
        0
    }
}

#[inline]
fn flag(cond: bool, f: u8) -> u8 {
    if cond {
        f
    } else {
        0
    }
}
