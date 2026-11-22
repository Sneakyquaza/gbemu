//! Jeu d'instructions Thumb (16 bits).

use super::arm::lit_demi;
use super::{cycles_mul, Cpu, T};
use crate::bus::Bus;

impl Cpu {
    pub(super) fn thumb_exec(&mut self, bus: &mut Bus, op: u32) {
        let rd = (op & 7) as usize;
        let rs = ((op >> 3) & 7) as usize;
        match op >> 11 {
            // Décalage immédiat (LSL, LSR, ASR).
            0..=2 => {
                let (v, c) = self.decale_imm(op >> 11, self.r[rs], (op >> 6) & 31);
                self.r[rd] = v;
                self.nz(v);
                self.set_c(c);
            }
            // ADD / SUB registre ou immédiat 3 bits.
            3 => {
                let n = (op >> 6) & 7;
                let v = if op & (1 << 10) != 0 {
                    n
                } else {
                    self.r[n as usize]
                };
                self.r[rd] = if op & (1 << 9) != 0 {
                    self.sub(self.r[rs], v, true, true)
                } else {
                    self.add(self.r[rs], v, false, true)
                };
            }
            // MOV / CMP / ADD / SUB immédiat 8 bits.
            4..=7 => {
                let rd = ((op >> 8) & 7) as usize;
                let imm = op & 0xFF;
                match (op >> 11) & 3 {
                    0 => {
                        self.r[rd] = imm;
                        self.nz(imm);
                    }
                    1 => {
                        self.sub(self.r[rd], imm, true, true);
                    }
                    2 => self.r[rd] = self.add(self.r[rd], imm, false, true),
                    _ => self.r[rd] = self.sub(self.r[rd], imm, true, true),
                }
            }
            8 => {
                if op & (1 << 10) == 0 {
                    self.thumb_alu(bus, op, rd, rs);
                } else {
                    self.thumb_haut(bus, op);
                }
            }
            // LDR relatif à PC.
            9 => {
                let adr = (self.r[15] & !2).wrapping_add((op & 0xFF) * 4);
                self.r[((op >> 8) & 7) as usize] = bus.lit32(adr, false);
                bus.inactif(1);
                self.seq = false;
            }
            // Chargement / rangement à décalage registre.
            10 | 11 => {
                let adr = self.r[rs].wrapping_add(self.r[((op >> 6) & 7) as usize]);
                self.seq = false;
                match (op >> 9) & 7 {
                    0 => bus.ecrit32(adr, self.r[rd], false),
                    1 => bus.ecrit16(adr, self.r[rd] as u16, false),
                    2 => bus.ecrit8(adr, self.r[rd] as u8, false),
                    k => {
                        self.r[rd] = match k {
                            3 => lit_demi(bus, adr, 2),
                            4 => bus.lit32(adr, false).rotate_right((adr & 3) * 8),
                            5 => lit_demi(bus, adr, 1),
                            6 => bus.lit8(adr, false) as u32,
                            _ => lit_demi(bus, adr, 3),
                        };
                        bus.inactif(1);
                    }
                }
            }
            // LDR / STR / LDRB / STRB immédiat 5 bits.
            12..=15 => {
                let octet = op & (1 << 12) != 0;
                let off = (op >> 6) & 31;
                let adr = self.r[rs].wrapping_add(if octet { off } else { off * 4 });
                self.seq = false;
                match (octet, op & (1 << 11) != 0) {
                    (false, false) => bus.ecrit32(adr, self.r[rd], false),
                    (true, false) => bus.ecrit8(adr, self.r[rd] as u8, false),
                    (false, true) => {
                        self.r[rd] = bus.lit32(adr, false).rotate_right((adr & 3) * 8);
                        bus.inactif(1);
                    }
                    (true, true) => {
                        self.r[rd] = bus.lit8(adr, false) as u32;
                        bus.inactif(1);
                    }
                }
            }
            // LDRH / STRH immédiat.
            16 | 17 => {
                let adr = self.r[rs].wrapping_add(((op >> 6) & 31) * 2);
                self.seq = false;
                if op & (1 << 11) != 0 {
                    self.r[rd] = lit_demi(bus, adr, 1);
                    bus.inactif(1);
                } else {
                    bus.ecrit16(adr, self.r[rd] as u16, false);
                }
            }
            // LDR / STR relatif à SP.
            18 | 19 => {
                let rd = ((op >> 8) & 7) as usize;
                let adr = self.r[13].wrapping_add((op & 0xFF) * 4);
                self.seq = false;
                if op & (1 << 11) != 0 {
                    self.r[rd] = bus.lit32(adr, false).rotate_right((adr & 3) * 8);
                    bus.inactif(1);
                } else {
                    bus.ecrit32(adr, self.r[rd], false);
                }
            }
            // ADD Rd, PC/SP, #imm.
            20 | 21 => {
                let base = if op & (1 << 11) != 0 {
                    self.r[13]
                } else {
                    self.r[15] & !2
                };
                self.r[((op >> 8) & 7) as usize] = base.wrapping_add((op & 0xFF) * 4);
            }
            22 | 23 => {
                if op & 0x0F00 == 0 {
                    // ADD SP, #±imm7×4.
                    let v = (op & 0x7F) * 4;
                    self.r[13] = if op & 0x80 != 0 {
                        self.r[13].wrapping_sub(v)
                    } else {
                        self.r[13].wrapping_add(v)
                    };
                } else if op & 0x0600 == 0x0400 {
                    // PUSH / POP (bit R : LR ou PC).
                    let pop = op & (1 << 11) != 0;
                    let mut liste = op & 0xFF;
                    if op & (1 << 8) != 0 {
                        liste |= if pop { 0x8000 } else { 0x4000 };
                    }
                    if pop {
                        self.transfert_bloc(bus, 13, liste, false, true, false, true, true);
                    } else {
                        self.transfert_bloc(bus, 13, liste, true, false, false, true, false);
                    }
                } else {
                    self.indefinie(bus);
                }
            }
            // STMIA / LDMIA Rb!.
            24 | 25 => {
                let rb = ((op >> 8) & 7) as usize;
                let liste = op & 0xFF;
                let charge = op & (1 << 11) != 0;
                // LDMIA avec la base dans la liste : pas de réécriture.
                let wb = !(charge && liste & (1 << rb) != 0);
                self.transfert_bloc(bus, rb, liste, false, true, false, wb, charge);
            }
            // Saut conditionnel / SWI.
            26 | 27 => {
                let cond = (op >> 8) & 15;
                match cond {
                    15 => self.swi(bus),
                    14 => self.indefinie(bus),
                    _ => {
                        if self.condition(cond) {
                            let off = ((op << 24) as i32 >> 23) as u32;
                            self.r[15] = self.r[15].wrapping_add(off);
                            self.flush(bus);
                        }
                    }
                }
            }
            28 => {
                let off = ((op << 21) as i32 >> 20) as u32;
                self.r[15] = self.r[15].wrapping_add(off);
                self.flush(bus);
            }
            29 => self.indefinie(bus),
            // BL : 1re moitié (poids forts dans LR).
            30 => {
                let off = ((op << 21) as i32 >> 9) as u32;
                self.r[14] = self.r[15].wrapping_add(off);
            }
            // BL : 2e moitié.
            _ => {
                let retour = self.r[15].wrapping_sub(2) | 1;
                self.r[15] = self.r[14].wrapping_add((op & 0x7FF) << 1);
                self.r[14] = retour;
                self.flush(bus);
            }
        }
    }

    /// Opérations ALU registre à registre.
    fn thumb_alu(&mut self, bus: &mut Bus, op: u32, rd: usize, rs: usize) {
        let a = self.r[rd];
        let b = self.r[rs];
        let k = (op >> 6) & 15;
        let r = match k {
            0 | 8 => a & b,
            1 => a ^ b,
            2..=4 | 7 => {
                bus.inactif(1);
                self.seq = false;
                let typ = match k {
                    2 => 0,
                    3 => 1,
                    4 => 2,
                    _ => 3,
                };
                let (v, c) = self.decale_reg(typ, a, b & 0xFF);
                self.set_c(c);
                v
            }
            5 => self.add(a, b, self.carry(), true),
            6 => self.sub(a, b, self.carry(), true),
            9 => self.sub(0, b, true, true),
            10 => self.sub(a, b, true, true),
            11 => self.add(a, b, false, true),
            12 => a | b,
            13 => {
                bus.inactif(cycles_mul(a, true));
                self.seq = false;
                // raccourci : C inchangé (comme MUL ARM).
                a.wrapping_mul(b)
            }
            14 => a & !b,
            _ => !b,
        };
        // Les opérations arithmétiques ont déjà mis leurs drapeaux.
        if !matches!(k, 5 | 6 | 9 | 10 | 11) {
            self.nz(r);
        }
        if !matches!(k, 8 | 10 | 11) {
            self.r[rd] = r;
        }
    }

    /// ADD / CMP / MOV sur registres hauts, BX.
    fn thumb_haut(&mut self, bus: &mut Bus, op: u32) {
        let rd = ((op & 7) | ((op >> 4) & 8)) as usize;
        let rs = ((op >> 3) & 15) as usize;
        let v = self.r[rs];
        match (op >> 8) & 3 {
            0 => {
                self.r[rd] = self.r[rd].wrapping_add(v);
                if rd == 15 {
                    self.flush(bus);
                }
            }
            1 => {
                self.sub(self.r[rd], v, true, true);
            }
            2 => {
                self.r[rd] = v;
                if rd == 15 {
                    self.flush(bus);
                }
            }
            _ => {
                if v & 1 != 0 {
                    self.cpsr |= T;
                } else {
                    self.cpsr &= !T;
                }
                self.r[15] = v;
                self.flush(bus);
            }
        }
    }
}
