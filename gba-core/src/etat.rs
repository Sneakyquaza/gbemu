//! Sauvegardes instantanées : format binaire versionné, lecture sans panique.
//! En-tête : magie `GBAE`, version (u16), CRC32 de la ROM (u32), puis chaque composant
//! dans un ordre fixe (CPU, bus avec ses modules : PPU, timers, DMA, son, cartouche),
//! petit-boutiste. Chaque module a ses méthodes `sauve` / `charge`.

use std::fmt;

pub(crate) const MAGIE: [u8; 4] = *b"GBAE";
/// Version 1 : premier format GBA.
pub(crate) const VERSION: u16 = 1;
/// Magie + version + CRC32.
pub(crate) const TAILLE_ENTETE: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErreurEtat {
    /// Pas un état gbemu (magie absente).
    Format,
    /// Version de format inconnue.
    Version(u16),
    /// État d'une autre ROM (CRC32 différent).
    AutreRom,
    /// Données tronquées ou en trop.
    Taille,
    /// Valeur impossible (données corrompues).
    Corrompu(&'static str),
}

impl fmt::Display for ErreurEtat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErreurEtat::Format => write!(f, "pas une sauvegarde d'état gbemu"),
            ErreurEtat::Version(v) => write!(f, "version d'état non prise en charge : {v}"),
            ErreurEtat::AutreRom => write!(f, "état d'un autre jeu"),
            ErreurEtat::Taille => write!(f, "état tronqué ou de taille incorrecte"),
            ErreurEtat::Corrompu(c) => write!(f, "état corrompu ({c})"),
        }
    }
}

impl std::error::Error for ErreurEtat {}

/// Écriture des champs (petit-boutiste).
pub(crate) trait Ecrit {
    fn u8(&mut self, v: u8);
    fn u16(&mut self, v: u16);
    fn u32(&mut self, v: u32);
    fn u64(&mut self, v: u64);
    fn bool(&mut self, v: bool);
    fn octets(&mut self, v: &[u8]);
}

impl Ecrit for Vec<u8> {
    fn u8(&mut self, v: u8) {
        self.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.extend_from_slice(&v.to_le_bytes());
    }
    fn bool(&mut self, v: bool) {
        self.push(v as u8);
    }
    fn octets(&mut self, v: &[u8]) {
        self.extend_from_slice(v);
    }
}

/// Lecture des champs : toute lecture hors des données renvoie `ErreurEtat::Taille`.
pub(crate) struct Lecteur<'a> {
    reste: &'a [u8],
}

impl<'a> Lecteur<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Lecteur { reste: data }
    }

    pub fn fini(&self) -> bool {
        self.reste.is_empty()
    }

    fn prend<const N: usize>(&mut self) -> Result<[u8; N], ErreurEtat> {
        let (a, b) = self
            .reste
            .split_first_chunk::<N>()
            .ok_or(ErreurEtat::Taille)?;
        self.reste = b;
        Ok(*a)
    }

    pub fn u8(&mut self) -> Result<u8, ErreurEtat> {
        Ok(self.prend::<1>()?[0])
    }
    pub fn u16(&mut self) -> Result<u16, ErreurEtat> {
        self.prend().map(u16::from_le_bytes)
    }
    pub fn u32(&mut self) -> Result<u32, ErreurEtat> {
        self.prend().map(u32::from_le_bytes)
    }
    pub fn u64(&mut self) -> Result<u64, ErreurEtat> {
        self.prend().map(u64::from_le_bytes)
    }
    pub fn bool(&mut self) -> Result<bool, ErreurEtat> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(ErreurEtat::Corrompu("booléen")),
        }
    }
    pub fn octets(&mut self, dst: &mut [u8]) -> Result<(), ErreurEtat> {
        if self.reste.len() < dst.len() {
            return Err(ErreurEtat::Taille);
        }
        let (a, b) = self.reste.split_at(dst.len());
        dst.copy_from_slice(a);
        self.reste = b;
        Ok(())
    }
    /// Octet borné à `max` (inclus).
    pub fn u8_max(&mut self, max: u8, champ: &'static str) -> Result<u8, ErreurEtat> {
        let v = self.u8()?;
        verifie(v <= max, champ)?;
        Ok(v)
    }
    /// Mot borné à `max` (inclus).
    pub fn u16_max(&mut self, max: u16, champ: &'static str) -> Result<u16, ErreurEtat> {
        let v = self.u16()?;
        verifie(v <= max, champ)?;
        Ok(v)
    }
}

pub(crate) fn verifie(ok: bool, champ: &'static str) -> Result<(), ErreurEtat> {
    if ok {
        Ok(())
    } else {
        Err(ErreurEtat::Corrompu(champ))
    }
}

/// Table du CRC32 (polynôme réfléchi 0xEDB88320, celui de zip/No-Intro).
const TABLE_CRC: [u32; 256] = {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
};

pub(crate) fn crc32(data: &[u8]) -> u32 {
    !data.iter().fold(!0u32, |c, &b| {
        TABLE_CRC[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8)
    })
}
