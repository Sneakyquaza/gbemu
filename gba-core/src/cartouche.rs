//! Cartouche : ROM (jusqu'à 32 Mio) et mémoire de sauvegarde (SRAM, Flash 64/128 Kio, EEPROM
//! 512 o / 8 Kio). Type détecté par les chaînes des bibliothèques Nintendo dans la ROM
//! (`SRAM_V`, `FLASH_V`/`FLASH512_V`/`FLASH1M_V`, `EEPROM_V`) ; sans chaîne, SRAM 64 Kio, et le
//! type est repris de la taille du fichier importé (table de repli).

use crate::etat::{verifie, Ecrit, ErreurEtat, Lecteur};
use std::fmt;

/// Taille maximale d'une ROM GBA.
pub const ROM_MAX: usize = 32 * 1024 * 1024;
const SRAM_TAILLE: usize = 0x10000;
const EEPROM_MAX: usize = 0x2000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErreurChargement {
    RomVide,
    RomTropGrande(usize),
    /// Le BIOS fourni ne fait pas 16 Kio.
    BiosTaille(usize),
}

impl fmt::Display for ErreurChargement {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ErreurChargement::RomVide => write!(f, "ROM vide"),
            ErreurChargement::RomTropGrande(n) => write!(f, "ROM trop grande ({n} octets)"),
            ErreurChargement::BiosTaille(n) => {
                write!(f, "BIOS de {n} octets (16 384 attendus)")
            }
        }
    }
}

impl std::error::Error for ErreurChargement {}

/// Type de mémoire de sauvegarde.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeSauvegarde {
    /// Aucune chaîne trouvée : se comporte comme une SRAM 64 Kio.
    Inconnu,
    Sram,
    Flash64,
    Flash128,
    Eeprom,
}

impl TypeSauvegarde {
    fn code(self) -> u8 {
        self as u8
    }

    fn depuis(c: u8) -> Option<Self> {
        use TypeSauvegarde::*;
        [Inconnu, Sram, Flash64, Flash128, Eeprom]
            .get(c as usize)
            .copied()
    }
}

/// Recherche des chaînes de type. Les bibliothèques les rangent alignées sur 4 octets.
/// raccourci : recherche alignée seulement, à élargir si un jeu non détecté le demande.
fn detecte(rom: &[u8]) -> TypeSauvegarde {
    let mut t = TypeSauvegarde::Inconnu;
    for (i, m) in rom.as_chunks::<4>().0.iter().enumerate() {
        let suite = |s: &[u8]| rom.get(i * 4..i * 4 + s.len()) == Some(s);
        let trouve = match m {
            b"EEPR" if suite(b"EEPROM_V") => TypeSauvegarde::Eeprom,
            b"SRAM" if suite(b"SRAM_V") || suite(b"SRAM_F_V") => TypeSauvegarde::Sram,
            b"FLAS" if suite(b"FLASH1M_V") => TypeSauvegarde::Flash128,
            b"FLAS" if suite(b"FLASH_V") || suite(b"FLASH512_V") => TypeSauvegarde::Flash64,
            _ => continue,
        };
        t = trouve;
        break;
    }
    t
}

/// Étape de la séquence de commande Flash (0xAA en 0x5555, 0x55 en 0x2AAA, commande en 0x5555).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Etape {
    Attente,
    Aa,
    Commande,
}

/// Puce Flash (Panasonic 64 Kio, Sanyo 128 Kio : ID des émulateurs courants).
#[derive(Clone)]
struct Flash {
    etape: Etape,
    /// Mode identification (0x90) : lecture du fabricant et du modèle en 0 et 1.
    id: bool,
    /// 0x80 reçu : effacement de puce (0x10) ou de secteur (0x30) attendu.
    effacement: bool,
    /// 0xA0 reçu : la prochaine écriture programme un octet.
    programme: bool,
    /// 0xB0 reçu (128 Kio) : la prochaine écriture en 0 choisit la banque.
    choix_banque: bool,
    banque: u8,
}

impl Flash {
    fn new() -> Self {
        Flash {
            etape: Etape::Attente,
            id: false,
            effacement: false,
            programme: false,
            choix_banque: false,
            banque: 0,
        }
    }
}

/// EEPROM série (bit 0 des accès 16 bits, normalement par DMA 3).
#[derive(Clone)]
struct Eeprom {
    /// Taille connue : `Some(true)` = 8 Kio (adresses 14 bits), `Some(false)` = 512 o (6 bits).
    grande: Option<bool>,
    /// Bits reçus (le plus récent en bit 0) et leur nombre.
    bits: u128,
    n: u8,
    /// Lecture en cours : adresse d'octet et bit suivant (0-67 : 4 bits nuls puis 64 bits).
    lecture: Option<(u16, u8)>,
}

impl Eeprom {
    fn largeur(&self) -> u8 {
        if self.grande.unwrap_or(true) {
            14
        } else {
            6
        }
    }
}

pub struct Cartouche {
    /// ROM complétée par des zéros jusqu'à un multiple de 4.
    rom: Vec<u8>,
    /// Vrai si la ROM dépasse 16 Mio (EEPROM seulement en 0x0DFFFF00-0x0DFFFFFF).
    grande_rom: bool,
    type_: TypeSauvegarde,
    /// SRAM (64 Kio), Flash (64 ou 128 Kio) ou EEPROM (8 Kio).
    mem: Vec<u8>,
    flash: Flash,
    eeprom: Eeprom,
}

impl Cartouche {
    pub fn new(rom: &[u8]) -> Result<Self, ErreurChargement> {
        if rom.is_empty() {
            return Err(ErreurChargement::RomVide);
        }
        if rom.len() > ROM_MAX {
            return Err(ErreurChargement::RomTropGrande(rom.len()));
        }
        let mut r = rom.to_vec();
        r.resize(rom.len().div_ceil(4) * 4, 0);
        let mut c = Cartouche {
            grande_rom: r.len() > 16 * 1024 * 1024,
            type_: TypeSauvegarde::Inconnu,
            rom: r,
            mem: Vec::new(),
            flash: Flash::new(),
            eeprom: Eeprom {
                grande: None,
                bits: 0,
                n: 0,
                lecture: None,
            },
        };
        c.change_type(detecte(&c.rom));
        Ok(c)
    }

    fn change_type(&mut self, t: TypeSauvegarde) {
        self.type_ = t;
        let taille = match t {
            TypeSauvegarde::Flash128 => 0x20000,
            TypeSauvegarde::Eeprom => EEPROM_MAX,
            _ => SRAM_TAILLE,
        };
        self.mem = vec![0xFF; taille];
        self.flash = Flash::new();
    }

    pub fn type_sauvegarde(&self) -> TypeSauvegarde {
        self.type_
    }

    /// Lecture 16 bits (adresse paire) ; au-delà de la ROM : bus ouvert de la cartouche (adresse / 2).
    #[inline]
    pub fn lit16(&self, a: u32) -> u16 {
        let o = (a & 0x01FF_FFFE) as usize;
        match self.rom.get(o..o + 2) {
            Some(b) => u16::from_le_bytes([b[0], b[1]]),
            None => (a >> 1) as u16,
        }
    }

    #[inline]
    pub fn lit32(&self, a: u32) -> u32 {
        let o = (a & 0x01FF_FFFC) as usize;
        match self.rom.get(o..o + 4) {
            Some(b) => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            None => {
                let a = a & !3;
                self.lit16(a) as u32 | (self.lit16(a + 2) as u32) << 16
            }
        }
    }

    #[inline]
    pub fn lit8(&self, a: u32) -> u8 {
        (self.lit16(a & !1) >> ((a & 1) * 8)) as u8
    }

    /// Vrai si l'adresse (région 0x0D) est celle de l'EEPROM.
    #[inline]
    pub fn est_eeprom(&self, a: u32) -> bool {
        self.type_ == TypeSauvegarde::Eeprom
            && a >> 24 == 0xD
            && (!self.grande_rom || a & 0x00FF_FF00 == 0x00FF_FF00)
    }

    /// Lecture série de l'EEPROM : bit de donnée, ou 1 (prête) hors lecture.
    pub fn lit_eeprom(&mut self) -> u16 {
        let e = &mut self.eeprom;
        let Some((adr, i)) = e.lecture else {
            return 1;
        };
        e.lecture = if i + 1 < 68 { Some((adr, i + 1)) } else { None };
        if i < 4 {
            return 0;
        }
        let k = (i - 4) as usize;
        (self.mem[adr as usize + k / 8] >> (7 - k % 8)) as u16 & 1
    }

    /// Écriture 16 bits en ROM : seul le bit 0 compte, et seulement vers l'EEPROM.
    pub fn ecrit_rom16(&mut self, a: u32, v: u16) {
        if !self.est_eeprom(a) {
            return;
        }
        let w = self.eeprom.largeur();
        let e = &mut self.eeprom;
        e.bits = (e.bits << 1) | (v & 1) as u128;
        e.n += 1;
        let masque = (1u128 << w) - 1;
        let n = e.n;
        if n < 3 {
            if n == 2 && e.bits < 2 {
                // Ni lecture (11) ni écriture (10) : bits ignorés.
                e.n = 0;
                e.bits = 0;
            }
            return;
        }
        let lecture = e.bits >> (n - 2) == 0b11;
        if lecture && n == 3 + w {
            let bloc = ((e.bits >> 1) & masque) as usize & 0x3FF;
            e.lecture = Some((((bloc * 8) % self.mem.len()) as u16, 0));
            e.n = 0;
            e.bits = 0;
        } else if !lecture && n == 3 + w + 64 {
            let bloc = ((e.bits >> 65) & masque) as usize & 0x3FF;
            let o = (bloc * 8) % self.mem.len();
            let d = ((e.bits >> 1) as u64).to_be_bytes();
            e.n = 0;
            e.bits = 0;
            e.lecture = None;
            self.mem[o..o + 8].copy_from_slice(&d);
        } else if n >= 3 + 14 + 64 {
            e.n = 0;
            e.bits = 0;
        }
    }

    /// Début d'un DMA 3 : sa longueur donne la taille de l'EEPROM (9 ou 73 bits = 512 o,
    /// 17 ou 81 = 8 Kio), retenue au premier accès.
    pub fn dma_eeprom(&mut self, src: u32, dst: u32, compte: u32) {
        if self.eeprom.grande.is_some() || !(self.est_eeprom(src) || self.est_eeprom(dst)) {
            return;
        }
        self.eeprom.grande = match compte {
            9 | 73 => Some(false),
            17 | 81 => Some(true),
            _ => None,
        };
    }

    /// SRAM / Flash (bus 8 bits, miroir tous les 64 Kio).
    #[inline]
    pub fn lit_sram(&self, a: u32) -> u8 {
        let o = a as usize & 0xFFFF;
        match self.type_ {
            TypeSauvegarde::Flash64 | TypeSauvegarde::Flash128 => {
                if self.flash.id && o < 2 {
                    let id = if self.type_ == TypeSauvegarde::Flash64 {
                        [0x32, 0x1B]
                    } else {
                        [0x62, 0x13]
                    };
                    id[o]
                } else {
                    self.mem[self.flash.banque as usize * 0x10000 + o]
                }
            }
            TypeSauvegarde::Eeprom => 0xFF,
            _ => self.mem[o],
        }
    }

    #[inline]
    pub fn ecrit_sram(&mut self, a: u32, v: u8) {
        let o = a as usize & 0xFFFF;
        match self.type_ {
            TypeSauvegarde::Flash64 | TypeSauvegarde::Flash128 => self.ecrit_flash(o, v),
            TypeSauvegarde::Eeprom => {}
            _ => self.mem[o] = v,
        }
    }

    fn ecrit_flash(&mut self, o: usize, v: u8) {
        let f = &mut self.flash;
        if f.programme {
            f.programme = false;
            let i = f.banque as usize * 0x10000 + o;
            self.mem[i] = v;
            return;
        }
        if f.choix_banque {
            f.choix_banque = false;
            if o == 0 {
                f.banque = v & 1;
                return;
            }
        }
        match (f.etape, o, v) {
            (Etape::Attente, 0x5555, 0xAA) => f.etape = Etape::Aa,
            (Etape::Attente, 0x5555, 0xF0) => f.id = false,
            (Etape::Aa, 0x2AAA, 0x55) => f.etape = Etape::Commande,
            (Etape::Commande, _, 0x30) if f.effacement => {
                f.etape = Etape::Attente;
                f.effacement = false;
                let d = f.banque as usize * 0x10000 + (o & 0xF000);
                self.mem[d..d + 0x1000].fill(0xFF);
            }
            (Etape::Commande, 0x5555, c) => {
                f.etape = Etape::Attente;
                match c {
                    0x90 => f.id = true,
                    0xF0 => f.id = false,
                    0x80 => f.effacement = true,
                    0x10 if f.effacement => {
                        f.effacement = false;
                        self.mem.fill(0xFF);
                    }
                    0xA0 => f.programme = true,
                    0xB0 if self.type_ == TypeSauvegarde::Flash128 => f.choix_banque = true,
                    _ => {}
                }
            }
            _ => f.etape = Etape::Attente,
        }
    }

    /// Mémoire de sauvegarde à écrire sur disque. `None` tant qu'il n'y a rien à garder : type
    /// inconnu jamais écrit (tout à 0xFF), ou EEPROM dont la taille n'est pas encore connue.
    /// raccourci : un jeu sans chaîne qui efface toute sa SRAM à 0xFF n'est pas réécrit.
    pub fn sauvegarde(&self) -> Option<Vec<u8>> {
        match self.type_ {
            TypeSauvegarde::Inconnu if self.mem.iter().all(|&b| b == 0xFF) => None,
            TypeSauvegarde::Eeprom => match self.eeprom.grande? {
                true => Some(self.mem.clone()),
                false => Some(self.mem[..512].to_vec()),
            },
            _ => Some(self.mem.clone()),
        }
    }

    /// Recharge une sauvegarde (.sav). Sans chaîne de type, la taille choisit le type :
    /// 512 o / 8 Kio = EEPROM, 128 Kio = Flash 128 Kio, sinon SRAM. Données plus courtes que la
    /// mémoire : le reste reste à 0xFF ; plus longues : tronquées. Jamais de panique.
    pub fn charger_sauvegarde(&mut self, data: &[u8]) {
        if self.type_ == TypeSauvegarde::Inconnu {
            match data.len() {
                512 | EEPROM_MAX => self.change_type(TypeSauvegarde::Eeprom),
                0x20000 => self.change_type(TypeSauvegarde::Flash128),
                _ => {}
            }
        }
        if self.type_ == TypeSauvegarde::Eeprom {
            match data.len() {
                512 => self.eeprom.grande = Some(false),
                EEPROM_MAX => self.eeprom.grande = Some(true),
                _ => {}
            }
        }
        self.mem.fill(0xFF);
        let n = data.len().min(self.mem.len());
        self.mem[..n].copy_from_slice(&data[..n]);
    }

    // --- Sauvegarde d'état ---

    pub(crate) fn sauve(&self, e: &mut Vec<u8>) {
        e.u8(self.type_.code());
        e.u32(self.mem.len() as u32);
        e.octets(&self.mem);
        let f = &self.flash;
        e.u8(f.etape as u8);
        e.bool(f.id);
        e.bool(f.effacement);
        e.bool(f.programme);
        e.bool(f.choix_banque);
        e.u8(f.banque);
        let p = &self.eeprom;
        e.u8(match p.grande {
            None => 0,
            Some(false) => 1,
            Some(true) => 2,
        });
        e.u64(p.bits as u64);
        e.u64((p.bits >> 64) as u64);
        e.u8(p.n);
        match p.lecture {
            Some((a, i)) => {
                e.bool(true);
                e.u16(a);
                e.u8(i);
            }
            None => e.bool(false),
        }
    }

    pub(crate) fn charge(&mut self, l: &mut Lecteur) -> Result<(), ErreurEtat> {
        let t = TypeSauvegarde::depuis(l.u8()?).ok_or(ErreurEtat::Corrompu("type sauvegarde"))?;
        let attendu = match t {
            TypeSauvegarde::Flash128 => 0x20000,
            TypeSauvegarde::Eeprom => EEPROM_MAX,
            _ => SRAM_TAILLE,
        };
        verifie(l.u32()? as usize == attendu, "taille sauvegarde")?;
        let mut mem = vec![0; attendu];
        l.octets(&mut mem)?;
        let etape = match l.u8()? {
            0 => Etape::Attente,
            1 => Etape::Aa,
            2 => Etape::Commande,
            _ => return Err(ErreurEtat::Corrompu("flash")),
        };
        let flash = Flash {
            etape,
            id: l.bool()?,
            effacement: l.bool()?,
            programme: l.bool()?,
            choix_banque: l.bool()?,
            banque: l.u8_max((t == TypeSauvegarde::Flash128) as u8, "banque flash")?,
        };
        let grande = match l.u8()? {
            0 => None,
            1 => Some(false),
            2 => Some(true),
            _ => return Err(ErreurEtat::Corrompu("eeprom")),
        };
        let bits = l.u64()? as u128 | (l.u64()? as u128) << 64;
        let n = l.u8_max(81, "eeprom")?;
        let lecture = if l.bool()? {
            let a = l.u16()?;
            let i = l.u8_max(67, "eeprom")?;
            verifie(a as usize + 8 <= EEPROM_MAX && a % 8 == 0, "eeprom")?;
            Some((a, i))
        } else {
            None
        };
        // Même ROM (CRC vérifié) : le type peut venir d'un .sav importé (repli par taille).
        self.type_ = t;
        self.mem = mem;
        self.flash = flash;
        self.eeprom = Eeprom {
            grande,
            bits,
            n,
            lecture,
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rom_avec(chaine: &[u8]) -> Vec<u8> {
        let mut r = vec![0u8; 0x400];
        r[0x100..0x100 + chaine.len()].copy_from_slice(chaine);
        r
    }

    /// Envoie des bits à l'EEPROM (bit de poids fort d'abord).
    fn envoie(c: &mut Cartouche, v: u128, n: u32) {
        for i in (0..n).rev() {
            c.ecrit_rom16(0x0D00_0000, (v >> i) as u16 & 1);
        }
    }

    fn lit_bloc(c: &mut Cartouche, bloc: u128, w: u32) -> u64 {
        envoie(c, (0b11 << (w + 1)) | (bloc << 1), 3 + w);
        let mut v = 0u64;
        for i in 0..68 {
            let b = c.lit_eeprom();
            if i >= 4 {
                v = (v << 1) | b as u64;
            } else {
                assert_eq!(b, 0);
            }
        }
        assert_eq!(c.lit_eeprom(), 1, "prête après la lecture");
        v
    }

    fn ecrit_bloc(c: &mut Cartouche, bloc: u128, w: u32, d: u64) {
        envoie(
            c,
            (0b10 << (w + 65)) | (bloc << 65) | ((d as u128) << 1),
            3 + w + 64,
        );
    }

    #[test]
    fn detection_par_chaines() {
        let t = |s: &[u8]| Cartouche::new(&rom_avec(s)).unwrap().type_sauvegarde();
        assert_eq!(t(b"SRAM_V113"), TypeSauvegarde::Sram);
        assert_eq!(t(b"FLASH_V126"), TypeSauvegarde::Flash64);
        assert_eq!(t(b"FLASH512_V131"), TypeSauvegarde::Flash64);
        assert_eq!(t(b"FLASH1M_V103"), TypeSauvegarde::Flash128);
        assert_eq!(t(b"EEPROM_V124"), TypeSauvegarde::Eeprom);
        assert_eq!(t(b"rien"), TypeSauvegarde::Inconnu);
    }

    #[test]
    fn eeprom_512_detectee_par_dma() {
        let mut c = Cartouche::new(&rom_avec(b"EEPROM_V124")).unwrap();
        assert_eq!(c.sauvegarde(), None, "taille encore inconnue");
        c.dma_eeprom(0x0200_0000, 0x0D00_0000, 73);
        ecrit_bloc(&mut c, 5, 6, 0x0123_4567_89AB_CDEF);
        assert_eq!(c.lit_eeprom(), 1);
        c.dma_eeprom(0x0200_0000, 0x0D00_0000, 9);
        assert_eq!(lit_bloc(&mut c, 5, 6), 0x0123_4567_89AB_CDEF);
        assert_eq!(lit_bloc(&mut c, 6, 6), u64::MAX, "bloc vierge");
        let s = c.sauvegarde().unwrap();
        assert_eq!(s.len(), 512);
        assert_eq!(
            &s[40..48],
            &[0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF]
        );
        // Bloc 64 = bloc 0 en 512 o (adresse sur 6 bits seulement).
    }

    #[test]
    fn eeprom_8k_et_aller_retour() {
        let mut c = Cartouche::new(&rom_avec(b"EEPROM_V124")).unwrap();
        c.dma_eeprom(0x0D00_0000, 0x0200_0000, 17);
        ecrit_bloc(&mut c, 1000, 14, 0xDEAD_BEEF_0000_0001);
        assert_eq!(lit_bloc(&mut c, 1000, 14), 0xDEAD_BEEF_0000_0001);
        let s = c.sauvegarde().unwrap();
        assert_eq!(s.len(), 8192);
        let mut d = Cartouche::new(&rom_avec(b"EEPROM_V124")).unwrap();
        d.charger_sauvegarde(&s);
        assert_eq!(lit_bloc(&mut d, 1000, 14), 0xDEAD_BEEF_0000_0001);
        // Commande invalide (00) ignorée, puis lecture normale.
        envoie(&mut d, 0, 2);
        assert_eq!(lit_bloc(&mut d, 1000, 14), 0xDEAD_BEEF_0000_0001);
    }

    #[test]
    fn eeprom_grande_rom_fenetre_haute() {
        let mut r = vec![0u8; 17 * 1024 * 1024];
        r[0x100..0x108].copy_from_slice(b"EEPROM_V");
        let c = Cartouche::new(&r).unwrap();
        assert!(!c.est_eeprom(0x0D00_0000));
        assert!(c.est_eeprom(0x0DFF_FF00));
    }

    #[test]
    fn repli_par_taille_et_donnees_invalides() {
        let mut c = Cartouche::new(&rom_avec(b"rien")).unwrap();
        assert_eq!(c.sauvegarde(), None, "rien écrit : pas de fichier");
        c.charger_sauvegarde(&[0x12; 0x20000]);
        assert_eq!(c.type_sauvegarde(), TypeSauvegarde::Flash128);
        assert_eq!(c.lit_sram(0), 0x12);
        let mut c = Cartouche::new(&rom_avec(b"rien")).unwrap();
        c.charger_sauvegarde(&[7; 512]);
        assert_eq!(c.type_sauvegarde(), TypeSauvegarde::Eeprom);
        assert_eq!(c.sauvegarde().unwrap(), vec![7; 512]);
        // Tailles quelconques : ni panique ni changement de type.
        let mut c = Cartouche::new(&rom_avec(b"SRAM_V")).unwrap();
        for n in [0, 1, 3, 513, 70_000, 300_000] {
            c.charger_sauvegarde(&vec![1; n]);
        }
        assert_eq!(c.type_sauvegarde(), TypeSauvegarde::Sram);
        assert_eq!(c.sauvegarde().unwrap().len(), SRAM_TAILLE);
    }

    #[test]
    fn flash_id_et_effacement() {
        let mut c = Cartouche::new(&rom_avec(b"FLASH1M_V103")).unwrap();
        let cmd = |c: &mut Cartouche, v: u8| {
            c.ecrit_sram(0x0E00_5555, 0xAA);
            c.ecrit_sram(0x0E00_2AAA, 0x55);
            c.ecrit_sram(0x0E00_5555, v);
        };
        cmd(&mut c, 0x90);
        assert_eq!(
            (c.lit_sram(0x0E00_0000), c.lit_sram(0x0E00_0001)),
            (0x62, 0x13)
        );
        cmd(&mut c, 0xF0);
        cmd(&mut c, 0xB0);
        c.ecrit_sram(0x0E00_0000, 1);
        cmd(&mut c, 0xA0);
        c.ecrit_sram(0x0E00_1234, 0x42);
        assert_eq!(c.sauvegarde().unwrap()[0x11234], 0x42, "banque 1");
        cmd(&mut c, 0x80);
        cmd(&mut c, 0x10);
        assert!(c.sauvegarde().unwrap().iter().all(|&b| b == 0xFF));
    }
}
