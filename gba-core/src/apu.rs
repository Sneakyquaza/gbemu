//! Son GBA (E/S 0x060-0x0AE) : les 4 canaux Game Boy (moteur de gb-core en mode GBA : RAM d'onde
//! en 2 banques, volume 75 %, séquenceur libre), son direct A/B (FIFO de 32 octets vidées par les
//! débordements des timers 0/1 et remplies par DMA1/2), SOUNDCNT_H/X, SOUNDBIAS.
//! Le moteur gb-core fait le mixage 10 bits (biais, saturation), la moyenne sur chaque période
//! d'échantillonnage et le passe-haut de sortie ; il tourne à 1 MiHz (16 cycles GBA par pas).
//! raccourci : la résolution PWM de SOUNDBIAS (bits 14-15) n'est pas émulée (sortie plus propre
//! que la console), et SOUNDCNT_X relu sans rattraper le PSG (état des canaux au dernier accès).

use crate::bus::Bus;
use crate::etat::{verifie, Ecrit, ErreurEtat, Lecteur};
use gb_core::apu::Apu as Psg;

/// Cycles GBA par cycle du moteur Game Boy.
const CYCLES_PAR_PAS: u64 = 16;

/// Adresses FIFO A et B (destination des DMA son).
const FIFO_ADR: [u32; 2] = [0x0400_00A0, 0x0400_00A4];

/// FIFO de son direct : 32 échantillons signés 8 bits.
#[derive(Default)]
struct Fifo {
    buf: [i8; 32],
    lecture: usize,
    n: usize,
    /// Échantillon joué (dernier tiré).
    echantillon: i8,
}

impl Fifo {
    fn pousse(&mut self, v: u8) {
        // FIFO pleine : l'octet est perdu.
        if self.n < 32 {
            self.buf[(self.lecture + self.n) & 31] = v as i8;
            self.n += 1;
        }
    }

    fn tire(&mut self) {
        if self.n > 0 {
            self.echantillon = self.buf[self.lecture];
            self.lecture = (self.lecture + 1) & 31;
            self.n -= 1;
        }
    }

    fn vide(&mut self) {
        self.lecture = 0;
        self.n = 0;
    }
}

pub struct Apu {
    psg: Psg,
    /// Cycle GBA jusqu'où le moteur Game Boy a été exécuté.
    fait: u64,
    fifo: [Fifo; 2],
    /// SOUNDCNT_H sans les bits de remise à zéro (11, 15).
    cnt_h: u16,
    biais: u16,
}

/// Adresse Game Boy (FF10-FF3F) d'un octet d'E/S son GBA.
fn adresse_gb(o: u32) -> Option<u16> {
    let a = match o {
        0x60 => 0xFF10,
        0x62..=0x65 => 0xFF11 + (o - 0x62) as u16,
        0x68 | 0x69 => 0xFF16 + (o - 0x68) as u16,
        0x6C | 0x6D => 0xFF18 + (o - 0x6C) as u16,
        0x70 => 0xFF1A,
        0x72..=0x75 => 0xFF1B + (o - 0x72) as u16,
        0x78 | 0x79 => 0xFF20 + (o - 0x78) as u16,
        0x7C | 0x7D => 0xFF22 + (o - 0x7C) as u16,
        0x80 | 0x81 => 0xFF24 + (o - 0x80) as u16,
        0x84 => 0xFF26,
        0x90..=0x9F => 0xFF30 + (o - 0x90) as u16,
        _ => return None,
    };
    Some(a)
}

/// Bits relisibles d'un octet PSG (longueurs et fréquences en écriture seule).
fn masque_lecture(o: u32) -> u8 {
    match o {
        0x60 => 0x7F,
        0x62 | 0x68 => 0xC0,
        0x63 | 0x69 | 0x79 | 0x7C | 0x81 => 0xFF,
        0x65 | 0x6D | 0x75 | 0x7D => 0x40,
        0x70 | 0x73 => 0xE0,
        0x80 => 0x77,
        0x84 => 0x8F,
        0x90..=0x9F => 0xFF,
        _ => 0,
    }
}

impl Apu {
    pub fn new() -> Self {
        Apu {
            psg: Psg::new_gba(),
            fait: 0,
            fifo: [Fifo::default(), Fifo::default()],
            cnt_h: 0,
            biais: 0,
        }
    }

    /// Rattrape le moteur Game Boy jusqu'au cycle `cycles` (un retour en arrière, après chargement
    /// d'un état, repart simplement de `cycles`).
    pub fn sync(&mut self, cycles: u64) {
        self.psg
            .avance((cycles / CYCLES_PAR_PAS).saturating_sub(self.fait / CYCLES_PAR_PAS));
        self.fait = cycles;
    }

    pub fn set_sample_rate(&mut self, hz: u32) {
        self.psg.set_sample_rate(hz);
    }

    /// Ajoute à `out` les échantillons stéréo entrelacés (G, D) produits jusqu'au dernier `sync`.
    pub fn drain_samples(&mut self, out: &mut Vec<f32>) {
        self.psg.drain_samples(out);
    }

    /// Lecture d'un octet (décalage 0x060-0x0AF).
    pub fn lit8(&self, o: u32) -> u8 {
        match o {
            0x82 => (self.cnt_h & 0x0F) as u8,
            0x83 => (self.cnt_h >> 8) as u8 & 0x77,
            0x88 => self.biais as u8,
            0x89 => (self.biais >> 8) as u8,
            _ => adresse_gb(o).map_or(0, |a| self.psg.read(a) & masque_lecture(o)),
        }
    }

    /// Écriture d'un octet (décalage 0x060-0x0AF) ; l'APU doit être rattrapé (`sync`).
    pub fn ecrit8(&mut self, o: u32, v: u8) {
        match o {
            0x82 => self.cnt_h = (self.cnt_h & 0xFF00) | (v & 0x0F) as u16,
            0x83 => {
                if v & 0x08 != 0 {
                    self.fifo[0].vide();
                }
                if v & 0x80 != 0 {
                    self.fifo[1].vide();
                }
                self.cnt_h = (self.cnt_h & 0xFF) | ((v & 0x77) as u16) << 8;
            }
            0x88 => self.biais = (self.biais & 0xFF00) | (v & 0xFE) as u16,
            0x89 => self.biais = (self.biais & 0xFF) | ((v & 0xC3) as u16) << 8,
            0xA0..=0xA3 => return self.fifo[0].pousse(v),
            0xA4..=0xA7 => return self.fifo[1].pousse(v),
            _ => match adresse_gb(o) {
                Some(a) => self.psg.write(a, v),
                None => return,
            },
        }
        self.maj_mix();
    }

    /// Débordement du timer `t` : chaque FIFO réglée sur ce timer passe à l'échantillon suivant.
    /// Renvoie les FIFO (bit 0 = A, bit 1 = B) à 16 octets ou moins, qui demandent un DMA.
    pub fn deborde(&mut self, t: usize) -> u8 {
        let mut dma = 0;
        for i in 0..2 {
            if (self.cnt_h >> (10 + 4 * i)) as usize & 1 == t {
                self.fifo[i].tire();
                if self.fifo[i].n <= 16 {
                    dma |= 1 << i;
                }
            }
        }
        dma
    }

    /// Transmet SOUNDCNT_H, SOUNDBIAS et les échantillons FIFO au mixage du moteur.
    pub fn maj_mix(&mut self) {
        let (mut g, mut d) = (0, 0);
        // Interrupteur général (SOUNDCNT_X bit 7) éteint : son direct muet aussi.
        if self.psg.read(0xFF26) & 0x80 != 0 {
            for (i, f) in self.fifo.iter().enumerate() {
                let c = self.cnt_h >> (4 * i);
                // 8 bits signés ×4 (100 %) ou ×2 (50 %).
                let s = f.echantillon as i32 * if self.cnt_h & (4 << i) != 0 { 4 } else { 2 };
                if c & 0x200 != 0 {
                    g += s;
                }
                if c & 0x100 != 0 {
                    d += s;
                }
            }
        }
        self.psg.mix_gba((self.cnt_h & 3) as u8, g, d, self.biais);
    }
}

// --- Sauvegarde d'état (lot sauvegardes) ---

impl Apu {
    /// Moteur Game Boy (état préfixé par sa longueur), cycle atteint, FIFO, SOUNDCNT_H, SOUNDBIAS.
    pub(crate) fn sauve(&self, e: &mut Vec<u8>) {
        let mut psg = Vec::new();
        self.psg.sauve(&mut psg);
        e.u32(psg.len() as u32);
        e.octets(&psg);
        e.u64(self.fait);
        for f in &self.fifo {
            e.octets(&f.buf.map(|v| v as u8));
            e.u8(f.lecture as u8);
            e.u8(f.n as u8);
            e.u8(f.echantillon as u8);
        }
        e.u16(self.cnt_h);
        e.u16(self.biais);
    }

    pub(crate) fn charge(&mut self, l: &mut Lecteur) -> Result<(), ErreurEtat> {
        let n = l.u32()? as usize;
        verifie(n <= 4096, "son")?;
        let mut psg = vec![0; n];
        l.octets(&mut psg)?;
        self.fait = l.u64()?;
        for f in &mut self.fifo {
            let mut buf = [0u8; 32];
            l.octets(&mut buf)?;
            f.buf = buf.map(|v| v as i8);
            f.lecture = l.u8_max(31, "fifo")? as usize;
            f.n = l.u8_max(32, "fifo")? as usize;
            f.echantillon = l.u8()? as i8;
        }
        self.cnt_h = l.u16()?;
        self.biais = l.u16()?;
        verifie(
            self.cnt_h & !0x770F == 0 && self.biais & !0xC3FE == 0,
            "son",
        )?;
        // Réglages du mixage (hors état du moteur) d'abord : le mixage qu'ils déclenchent est
        // ensuite écrasé par l'état relu du moteur.
        self.maj_mix();
        self.psg
            .charge_etat(&psg)
            .map_err(|_| ErreurEtat::Corrompu("son"))
    }

    /// Cycle GBA jusqu'où le son a été calculé (doit rester ≤ cycles du bus).
    pub(crate) fn fait(&self) -> u64 {
        self.fait
    }
}

impl Bus {
    /// Écriture d'un octet d'E/S son (0x060-0x0AF).
    pub(crate) fn son_ecrit8(&mut self, o: u32, v: u8) {
        self.apu.sync(self.cycles);
        self.apu.ecrit8(o, v);
    }

    /// Débordements des timers 0/1 : échantillons FIFO suivants et DMA de remplissage.
    pub(crate) fn son_debordements(&mut self, debs: [u32; 2]) {
        if debs == [0, 0] {
            return;
        }
        self.apu.sync(self.cycles);
        for (t, &n) in debs.iter().enumerate() {
            // Au-delà de 32 débordements d'un coup, la FIFO est de toute façon vidée.
            for _ in 0..n.min(32) {
                let dma = self.apu.deborde(t);
                for (i, &adr) in FIFO_ADR.iter().enumerate() {
                    if dma & (1 << i) != 0 {
                        for c in [1, 2] {
                            if self.dma[c].dad & !3 == adr {
                                self.dma_fifo(c);
                            }
                        }
                    }
                }
            }
        }
        self.apu.maj_mix();
    }
}

#[cfg(test)]
mod tests {
    use super::Apu;

    /// Échantillons produits jusqu'au cycle `c` (moyenne des deux voies).
    fn echantillons(apu: &mut Apu, c: u64) -> Vec<f32> {
        apu.sync(c);
        let mut v = Vec::new();
        apu.drain_samples(&mut v);
        v
    }

    #[test]
    fn fifo_pleine_et_remise_a_zero() {
        let mut apu = Apu::new();
        apu.ecrit8(0x83, 0x03); // A : 2 côtés, timer 0
        for i in 0..40 {
            apu.ecrit8(0xA0 + (i & 3), i as u8);
        }
        assert_eq!(apu.fifo[0].n, 32);
        // B (vide, aussi sur le timer 0) demande lui aussi un DMA.
        assert_eq!(apu.deborde(0), 2);
        assert_eq!(apu.fifo[0].echantillon, 0);
        assert_eq!(apu.fifo[0].n, 31);
        for _ in 0..15 {
            apu.deborde(0);
        }
        // 16 octets restants : demande de DMA pour A seulement.
        assert_eq!(apu.deborde(1), 0);
        assert_eq!(apu.fifo[0].n, 16);
        assert_eq!(apu.deborde(0) & 1, 1);
        apu.ecrit8(0x83, 0x08); // remise à zéro de A
        assert_eq!(apu.fifo[0].n, 0);
        assert_eq!(apu.lit8(0x83), 0);
    }

    #[test]
    fn son_direct_volume_et_panoramique() {
        let mut apu = Apu::new();
        apu.ecrit8(0x84, 0x80);
        apu.ecrit8(0x89, 0x02); // biais 0x200
        apu.ecrit8(0x82, 0x04); // A à 100 %
        apu.ecrit8(0x83, 0x02); // A à gauche seulement, timer 0
        apu.ecrit8(0xA0, 100);
        apu.deborde(0);
        apu.maj_mix();
        let v = echantillons(&mut apu, 16 * 1024);
        // Premier échantillon : saut de 400/512 à gauche, rien à droite (passe-haut ensuite).
        assert!((v[0] - 400.0 / 512.0).abs() < 0.02, "{}", v[0]);
        assert!(v[1].abs() < 1e-6);
        // 50 % : moitié (w[0] à cheval sur le changement).
        apu.ecrit8(0x82, 0x00);
        let w = echantillons(&mut apu, 16 * 2048);
        assert!(w[2] < v[0] * 0.5 && w[2] > 0.0, "{} {}", w[2], v[0]);
    }

    #[test]
    fn biais_sature_la_sortie() {
        let mut apu = Apu::new();
        apu.ecrit8(0x84, 0x80);
        apu.ecrit8(0x89, 0x00); // biais 0 : moitié négative coupée
        apu.ecrit8(0x82, 0x04);
        apu.ecrit8(0x83, 0x03);
        apu.ecrit8(0xA0, (-100i8) as u8);
        apu.deborde(0);
        apu.maj_mix();
        let v = echantillons(&mut apu, 16 * 1024);
        assert!(v[0].abs() < 1e-6, "{}", v[0]);
    }

    #[test]
    fn carre_du_canal_1() {
        let mut apu = Apu::new();
        apu.ecrit8(0x84, 0x80);
        apu.ecrit8(0x89, 0x02);
        apu.ecrit8(0x82, 0x02); // PSG 100 %
        apu.ecrit8(0x80, 0x77);
        apu.ecrit8(0x81, 0x11); // canal 1 des deux côtés
        apu.ecrit8(0x62, 0x80); // rapport 50 %
        apu.ecrit8(0x63, 0xF0); // volume 15
        apu.ecrit8(0x64, 0x00);
        apu.ecrit8(0x65, 0x87); // 131072 / (2048 - 0x700) = 512 Hz, déclenché
        assert_eq!(apu.lit8(0x84) & 0x0F, 1);
        // Relecture : longueur et fréquence en écriture seule.
        assert_eq!(
            (apu.lit8(0x62), apu.lit8(0x64), apu.lit8(0x65)),
            (0x80, 0, 0)
        );
        let v = echantillons(&mut apu, 16_777_216 / 10);
        let g: Vec<f32> = v.iter().step_by(2).copied().collect();
        // ~51 périodes en 0,1 s : compter les passages par zéro montants.
        let montees = g.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        assert!((48..=54).contains(&montees), "{montees}");
        let max = g.iter().fold(0f32, |m, &x| m.max(x.abs()));
        // 15 × 8 = 120 / 512, crête du créneau centré par le passe-haut.
        assert!(max > 0.15 && max < 0.3, "{max}");
    }

    #[test]
    fn ram_d_onde_en_deux_banques() {
        let mut apu = Apu::new();
        apu.ecrit8(0x84, 0x80);
        apu.ecrit8(0x70, 0x00); // banque 0 jouée : le CPU écrit la banque 1
        apu.ecrit8(0x90, 0x12);
        apu.ecrit8(0x70, 0x40); // banque 1 jouée : le CPU voit la banque 0
        assert_eq!(apu.lit8(0x90), 0);
        apu.ecrit8(0x90, 0x34);
        apu.ecrit8(0x70, 0x00);
        assert_eq!(apu.lit8(0x90), 0x12);
        assert_eq!(apu.lit8(0x70), 0x00);
        apu.ecrit8(0x70, 0xE0);
        assert_eq!(apu.lit8(0x70), 0xE0);
        apu.ecrit8(0x73, 0x80); // 75 %
        assert_eq!(apu.lit8(0x73), 0x80);
    }

    #[test]
    fn timer_et_dma_alimentent_la_fifo() {
        use crate::bus::Bus;
        use crate::cartouche::Cartouche;
        let mut b = Bus::new(Cartouche::new(&[0; 4]).unwrap(), crate::BIOS_LIBRE);
        // Créneau en IWRAM : 8 échantillons à +64 puis 8 à -64 (2 048 Hz à 32 768 Hz).
        for i in 0..1024u32 {
            let v = if i & 2 == 0 { 0x4040_4040 } else { 0xC0C0_C0C0 };
            b.ecrit32(0x0300_0000 + 4 * i, v, false);
        }
        b.ecrit16(0x0400_0084, 0x80, false);
        b.ecrit16(0x0400_0088, 0x200, false);
        // A : 100 %, deux côtés, timer 0, FIFO remise à zéro.
        b.ecrit16(0x0400_0082, 0x0B04, false);
        b.ecrit32(0x0400_00BC, 0x0300_0000, false);
        b.ecrit32(0x0400_00C0, 0x0400_00A0, false);
        // DMA1 : spécial, 32 bits, répété, destination fixe.
        b.ecrit16(0x0400_00C6, 0xB640, false);
        b.ecrit16(0x0400_0100, 0u16.wrapping_sub(512), false);
        b.ecrit16(0x0400_0102, 0x80, false);
        let fin = 16_777_216 / 10;
        while b.cycles < fin {
            b.cycles += 100;
            if b.cycles >= b.prochain {
                b.evenements();
            }
        }
        let mut v = Vec::new();
        b.apu.sync(b.cycles);
        b.apu.drain_samples(&mut v);
        let g: Vec<f32> = v.iter().step_by(2).copied().collect();
        let montees = g.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        assert!((200..=210).contains(&montees), "{montees}");
        let max = g.iter().fold(0f32, |m, &x| m.max(x));
        assert!(max > 0.4 && max < 0.6, "{max}");
        // Remplie par blocs de 16 octets dès qu'il en reste 16 ou moins.
        assert!((16..=32).contains(&b.apu.fifo[0].n), "{}", b.apu.fifo[0].n);
    }
}
