//! Jeu d'instructions ARM (32 bits).

use super::{cycles_mul, Cpu, N, T, Z};
use crate::bus::Bus;

impl Cpu {
    /// Exécute une instruction ARM dont la condition est vraie.
    pub(super) fn arm_exec(&mut self, bus: &mut Bus, op: u32) {
        match (op >> 25) & 7 {
            0 => {
                if op & 0x0FFF_FFF0 == 0x012F_FF10 {
                    self.arm_bx(bus, op);
                } else if op & 0x0FB0_0FF0 == 0x0100_0090 {
                    self.arm_swp(bus, op);
                } else if op & 0x0FC0_00F0 == 0x0000_0090 {
                    self.arm_mul(bus, op);
                } else if op & 0x0F80_00F0 == 0x0080_0090 {
                    self.arm_mull(bus, op);
                } else if op & 0x90 == 0x90 {
                    self.arm_demi(bus, op);
                } else if op & 0x0190_0000 == 0x0100_0000 {
                    self.arm_psr(op);
                } else {
                    self.arm_dp(bus, op);
                }
            }
            1 => {
                if op & 0x0190_0000 == 0x0100_0000 {
                    self.arm_psr(op);
                } else {
                    self.arm_dp(bus, op);
                }
            }
            2 => self.arm_mot(bus, op),
            3 => {
                if op & 0x10 != 0 {
                    self.indefinie(bus);
                } else {
                    self.arm_mot(bus, op);
                }
            }
            4 => self.arm_bloc(bus, op),
            5 => {
                let off = ((op << 8) as i32 >> 6) as u32;
                if op & (1 << 24) != 0 {
                    self.r[14] = self.r[15].wrapping_sub(4);
                }
                self.r[15] = self.r[15].wrapping_add(off);
                self.flush(bus);
            }
            6 => self.indefinie(bus),
            _ => {
                if op & (1 << 24) != 0 {
                    self.swi(bus);
                } else {
                    self.indefinie(bus);
                }
            }
        }
    }

    fn arm_bx(&mut self, bus: &mut Bus, op: u32) {
        let v = self.r[(op & 15) as usize];
        if v & 1 != 0 {
            self.cpsr |= T;
        } else {
            self.cpsr &= !T;
        }
        self.r[15] = v;
        self.flush(bus);
    }

    /// Traitement de données (AND … MVN).
    fn arm_dp(&mut self, bus: &mut Bus, op: u32) {
        let s = op & (1 << 20) != 0;
        let rd = ((op >> 12) & 15) as usize;
        let rn_i = ((op >> 16) & 15) as usize;
        let mut rn = self.r[rn_i];
        let (op2, sc) = if op & (1 << 25) != 0 {
            let rot = (op >> 7) & 0x1E;
            let v = (op & 0xFF).rotate_right(rot);
            (v, if rot == 0 { self.carry() } else { v >> 31 != 0 })
        } else {
            let rm = (op & 15) as usize;
            let typ = (op >> 5) & 3;
            if op & 0x10 != 0 {
                // Décalage par registre : 1 cycle interne, PC lu à +12.
                bus.inactif(1);
                self.seq = false;
                let n = self.r[((op >> 8) & 15) as usize] & 0xFF;
                let v = if rm == 15 {
                    self.r[15].wrapping_add(4)
                } else {
                    self.r[rm]
                };
                if rn_i == 15 {
                    rn = rn.wrapping_add(4);
                }
                self.decale_reg(typ, v, n)
            } else {
                self.decale_imm(typ, self.r[rm], (op >> 7) & 31)
            }
        };
        // S avec Rd = PC : CPSR = SPSR au lieu des drapeaux.
        let fl = s && rd != 15;
        let opc = (op >> 21) & 15;
        let res = match opc {
            0 | 1 | 8 | 9 | 12 | 13 | 14 | 15 => {
                let r = match opc {
                    0 | 8 => rn & op2,
                    1 | 9 => rn ^ op2,
                    12 => rn | op2,
                    13 => op2,
                    14 => rn & !op2,
                    _ => !op2,
                };
                if fl || (8..=11).contains(&opc) {
                    self.nz(r);
                    self.set_c(sc);
                }
                r
            }
            2 | 10 => self.sub(rn, op2, true, fl || opc == 10),
            3 => self.sub(op2, rn, true, fl),
            4 | 11 => self.add(rn, op2, false, fl || opc == 11),
            5 => self.add(rn, op2, self.carry(), fl),
            6 => self.sub(rn, op2, self.carry(), fl),
            _ => self.sub(op2, rn, self.carry(), fl),
        };
        if (8..=11).contains(&opc) {
            // TST/TEQ/CMP/CMN avec Rd = PC : CPSR = SPSR, sans vider le pipeline.
            if rd == 15 {
                self.restaure_cpsr();
            }
            return;
        }
        if rd == 15 {
            if s {
                self.restaure_cpsr();
            }
            self.r[15] = res;
            self.flush(bus);
        } else {
            self.r[rd] = res;
        }
    }

    /// MRS / MSR.
    fn arm_psr(&mut self, op: u32) {
        let spsr = op & (1 << 22) != 0;
        if op & (1 << 21) == 0 {
            // MRS
            let v = if spsr && super::banque(self.cpsr) != 0 {
                self.spsr
            } else {
                self.cpsr
            };
            self.r[((op >> 12) & 15) as usize] = v;
            return;
        }
        let v = if op & (1 << 25) != 0 {
            (op & 0xFF).rotate_right((op >> 7) & 0x1E)
        } else {
            self.r[(op & 15) as usize]
        };
        let mut masque = 0;
        for (bit, m) in [(16, 0xFF), (17, 0xFF00), (18, 0xFF_0000), (19, 0xFF00_0000)] {
            if op & (1 << bit) != 0 {
                masque |= m;
            }
        }
        if spsr {
            if super::banque(self.cpsr) != 0 {
                self.spsr = (self.spsr & !masque) | (v & masque);
            }
        } else {
            if self.cpsr & 0x1F == super::MODE_USR {
                masque &= 0xFF00_0000;
            }
            // Le bit T ne change pas par MSR.
            masque &= !T;
            let nv = (self.cpsr & !masque) | (v & masque);
            self.ecrit_cpsr(nv);
        }
    }

    /// MUL / MLA.
    fn arm_mul(&mut self, bus: &mut Bus, op: u32) {
        let rd = ((op >> 16) & 15) as usize;
        let rs = self.r[((op >> 8) & 15) as usize];
        let mut r = self.r[(op & 15) as usize].wrapping_mul(rs);
        let mut cyc = cycles_mul(rs, true);
        if op & (1 << 21) != 0 {
            r = r.wrapping_add(self.r[((op >> 12) & 15) as usize]);
            cyc += 1;
        }
        bus.inactif(cyc);
        self.seq = false;
        if op & (1 << 20) != 0 {
            // raccourci : C inchangé (valeur « sans signification » sur ARMv4), à affiner si un test l'exige.
            self.nz(r);
        }
        self.r[rd] = r;
    }

    /// UMULL / UMLAL / SMULL / SMLAL.
    fn arm_mull(&mut self, bus: &mut Bus, op: u32) {
        let hi = ((op >> 16) & 15) as usize;
        let lo = ((op >> 12) & 15) as usize;
        let rs = self.r[((op >> 8) & 15) as usize];
        let rm = self.r[(op & 15) as usize];
        let signe = op & (1 << 22) != 0;
        let mut r = if signe {
            (rm as i32 as i64).wrapping_mul(rs as i32 as i64) as u64
        } else {
            rm as u64 * rs as u64
        };
        let mut cyc = cycles_mul(rs, signe) + 1;
        if op & (1 << 21) != 0 {
            r = r.wrapping_add(((self.r[hi] as u64) << 32) | self.r[lo] as u64);
            cyc += 1;
        }
        bus.inactif(cyc);
        self.seq = false;
        if op & (1 << 20) != 0 {
            self.cpsr = (self.cpsr & !(N | Z))
                | if r >> 63 != 0 { N } else { 0 }
                | if r == 0 { Z } else { 0 };
        }
        self.r[lo] = r as u32;
        self.r[hi] = (r >> 32) as u32;
    }

    /// SWP / SWPB.
    fn arm_swp(&mut self, bus: &mut Bus, op: u32) {
        let adr = self.r[((op >> 16) & 15) as usize];
        let rm = self.r[(op & 15) as usize];
        let v = if op & (1 << 22) != 0 {
            let v = bus.lit8(adr, false) as u32;
            bus.ecrit8(adr, rm as u8, false);
            v
        } else {
            let v = bus.lit32(adr, false).rotate_right((adr & 3) * 8);
            bus.ecrit32(adr, rm, false);
            v
        };
        bus.inactif(1);
        self.seq = false;
        self.r[((op >> 12) & 15) as usize] = v;
    }

    /// LDR / STR / LDRB / STRB.
    fn arm_mot(&mut self, bus: &mut Bus, op: u32) {
        let rn_i = ((op >> 16) & 15) as usize;
        let rd = ((op >> 12) & 15) as usize;
        let off = if op & (1 << 25) == 0 {
            op & 0xFFF
        } else {
            self.decale_imm((op >> 5) & 3, self.r[(op & 15) as usize], (op >> 7) & 31)
                .0
        };
        let base = self.r[rn_i];
        let decale = if op & (1 << 23) != 0 {
            base.wrapping_add(off)
        } else {
            base.wrapping_sub(off)
        };
        let pre = op & (1 << 24) != 0;
        let adr = if pre { decale } else { base };
        let wb = !pre || op & (1 << 21) != 0;
        let octet = op & (1 << 22) != 0;
        self.seq = false;
        if op & (1 << 20) != 0 {
            let v = if octet {
                bus.lit8(adr, false) as u32
            } else {
                bus.lit32(adr, false).rotate_right((adr & 3) * 8)
            };
            bus.inactif(1);
            if wb {
                self.r[rn_i] = decale;
            }
            self.r[rd] = v;
            if rd == 15 {
                self.flush(bus);
            }
        } else {
            let v = if rd == 15 {
                self.pc_stocke()
            } else {
                self.r[rd]
            };
            if octet {
                bus.ecrit8(adr, v as u8, false);
            } else {
                bus.ecrit32(adr, v, false);
            }
            if wb {
                self.r[rn_i] = decale;
            }
        }
    }

    /// LDRH / STRH / LDRSB / LDRSH.
    fn arm_demi(&mut self, bus: &mut Bus, op: u32) {
        let rn_i = ((op >> 16) & 15) as usize;
        let rd = ((op >> 12) & 15) as usize;
        let off = if op & (1 << 22) != 0 {
            ((op >> 4) & 0xF0) | (op & 0xF)
        } else {
            self.r[(op & 15) as usize]
        };
        let base = self.r[rn_i];
        let decale = if op & (1 << 23) != 0 {
            base.wrapping_add(off)
        } else {
            base.wrapping_sub(off)
        };
        let pre = op & (1 << 24) != 0;
        let adr = if pre { decale } else { base };
        let wb = !pre || op & (1 << 21) != 0;
        let sh = (op >> 5) & 3;
        self.seq = false;
        if op & (1 << 20) != 0 {
            let v = lit_demi(bus, adr, sh);
            bus.inactif(1);
            if wb {
                self.r[rn_i] = decale;
            }
            self.r[rd] = v;
            if rd == 15 {
                self.flush(bus);
            }
        } else {
            // sh = 2/3 sans L : LDRD/STRD (ARMv5), ignorés.
            if sh == 1 {
                let v = if rd == 15 {
                    self.pc_stocke()
                } else {
                    self.r[rd]
                };
                bus.ecrit16(adr, v as u16, false);
            }
            if wb {
                self.r[rn_i] = decale;
            }
        }
    }

    /// LDM / STM (liste vide : r15 transféré, base ± 0x40 ; base dans la liste : voir commentaires).
    fn arm_bloc(&mut self, bus: &mut Bus, op: u32) {
        let rn_i = ((op >> 16) & 15) as usize;
        let liste = op & 0xFFFF;
        let haut = op & (1 << 23) != 0;
        let pre = op & (1 << 24) != 0;
        let s = op & (1 << 22) != 0;
        let wb = op & (1 << 21) != 0;
        let charge = op & (1 << 20) != 0;
        self.transfert_bloc(bus, rn_i, liste, pre, haut, s, wb, charge);
    }

    /// Transfert de bloc commun à ARM et Thumb (PUSH/POP/LDMIA/STMIA).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn transfert_bloc(
        &mut self,
        bus: &mut Bus,
        rn_i: usize,
        liste: u32,
        pre: bool,
        haut: bool,
        s: bool,
        wb: bool,
        charge: bool,
    ) {
        let (n, liste) = if liste == 0 {
            (16, 0x8000)
        } else {
            (liste.count_ones(), liste)
        };
        let base = self.r[rn_i];
        let taille = n * 4;
        let mut adr = match (pre, haut) {
            (false, true) => base,
            (true, true) => base.wrapping_add(4),
            (false, false) => base.wrapping_sub(taille).wrapping_add(4),
            (true, false) => base.wrapping_sub(taille),
        };
        let fin = if haut {
            base.wrapping_add(taille)
        } else {
            base.wrapping_sub(taille)
        };
        // Bit S sans PC chargé : registres du mode utilisateur.
        let usr = s && !(charge && liste & 0x8000 != 0);
        self.seq = false;
        if charge {
            // Base dans la liste : la valeur chargée l'emporte sur la réécriture.
            if wb {
                self.r[rn_i] = fin;
            }
            let mut seq = false;
            for i in 0..16 {
                if liste & (1 << i) != 0 {
                    let v = bus.lit32(adr, seq);
                    if usr {
                        self.ecrit_reg_usr(i, v);
                    } else {
                        self.r[i] = v;
                    }
                    adr = adr.wrapping_add(4);
                    seq = true;
                }
            }
            bus.inactif(1);
            if liste & 0x8000 != 0 {
                if s {
                    self.restaure_cpsr();
                }
                self.flush(bus);
            }
        } else {
            // Base dans la liste : ancienne valeur si c'est le 1er registre, nouvelle sinon
            // (la réécriture a lieu après le 1er transfert).
            let mut premier = true;
            for i in 0..16 {
                if liste & (1 << i) != 0 {
                    let v = if i == 15 {
                        self.pc_stocke()
                    } else if usr {
                        self.reg_usr(i)
                    } else {
                        self.r[i]
                    };
                    bus.ecrit32(adr, v, !premier);
                    if premier && wb {
                        self.r[rn_i] = fin;
                    }
                    premier = false;
                    adr = adr.wrapping_add(4);
                }
            }
        }
    }
}

/// Lecture LDRH (sh = 1), LDRSB (2), LDRSH (3), avec les effets des adresses impaires.
pub(super) fn lit_demi(bus: &mut Bus, adr: u32, sh: u32) -> u32 {
    match sh {
        1 => (bus.lit16(adr, false) as u32).rotate_right((adr & 1) * 8),
        2 => bus.lit8(adr, false) as i8 as i32 as u32,
        _ => {
            if adr & 1 != 0 {
                // LDRSH impair = LDRSB.
                bus.lit8(adr, false) as i8 as i32 as u32
            } else {
                bus.lit16(adr, false) as i16 as i32 as u32
            }
        }
    }
}
