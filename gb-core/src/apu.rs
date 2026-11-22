//! Son DMG : 2 carrés (balayage sur le 1), onde, bruit, séquenceur 512 Hz piloté par DIV.
//! Exécuté par lots (`avance`, rattrapé par le bus avant chaque accès) ; produit des échantillons stéréo f32 entrelacés
//! (moyenne sur la période d'échantillonnage, puis passe-haut façon condensateur de sortie).

use crate::etat::{verifie, Ecrit, ErreurEtat, Lecteur};

/// Bits toujours lus à 1 pour FF10..FF2F (DMG).
const MASQUES: [u8; 0x20] = [
    0x80, 0x3F, 0x00, 0xFF, 0xBF, 0xFF, 0x3F, 0x00, 0xFF, 0xBF, 0x7F, 0xFF, 0x9F, 0xFF, 0xBF, 0xFF,
    0xFF, 0x00, 0x00, 0xBF, 0x00, 0x00, 0x70, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
];

/// Motifs des rapports cycliques (bit 7 = première étape).
const RAPPORTS: [u8; 4] = [0b0000_0001, 0b1000_0001, 0b1000_0111, 0b0111_1110];

/// Diviseurs du bruit en cycles machine (8, 16, 32… points / 4).
const DIVISEURS: [u8; 8] = [2, 4, 8, 12, 16, 20, 24, 28];

/// Cycles machine par seconde.
const CYCLES_PAR_S: u32 = 1_048_576;

#[derive(Default)]
struct Enveloppe {
    initial: u8,
    monte: bool,
    periode: u8,
    vol: u8,
    compte: u8,
}

impl Enveloppe {
    fn sauve(&self, e: &mut Vec<u8>) {
        e.octets(&[self.initial, self.periode, self.vol, self.compte]);
        e.bool(self.monte);
    }

    fn charge(&mut self, l: &mut Lecteur) -> Result<(), ErreurEtat> {
        self.initial = l.u8_max(15, "enveloppe")?;
        self.periode = l.u8_max(7, "enveloppe")?;
        self.vol = l.u8_max(15, "enveloppe")?;
        self.compte = l.u8()?;
        self.monte = l.bool()?;
        Ok(())
    }

    fn ecrit(&mut self, v: u8) {
        self.initial = v >> 4;
        self.monte = v & 0x08 != 0;
        self.periode = v & 7;
    }

    // raccourci : ni mode « zombie » (écriture NRx2 canal actif) ni retard d'un pas quand l'étape 7
    // suit le déclenchement ; à faire si un jeu ou un test (SameSuite) le demande.
    fn declenche(&mut self) {
        self.vol = self.initial;
        self.compte = self.periode;
    }

    /// Pas d'enveloppe (étape 7) ; vrai si le volume change.
    fn horloge(&mut self) -> bool {
        if self.periode == 0 {
            return false;
        }
        self.compte = self.compte.saturating_sub(1);
        if self.compte > 0 {
            return false;
        }
        self.compte = self.periode;
        if self.monte && self.vol < 15 {
            self.vol += 1;
            true
        } else if !self.monte && self.vol > 0 {
            self.vol -= 1;
            true
        } else {
            false
        }
    }
}

/// Compteur de longueur commun aux 4 canaux.
#[derive(Default)]
struct Longueur {
    compte: u16,
    actif: bool,
}

impl Longueur {
    fn sauve(&self, e: &mut Vec<u8>) {
        e.u16(self.compte);
        e.bool(self.actif);
    }

    fn charge(&mut self, l: &mut Lecteur) -> Result<(), ErreurEtat> {
        self.compte = l.u16_max(256, "longueur")?;
        self.actif = l.bool()?;
        Ok(())
    }

    /// Pas de longueur (étapes paires) ; vrai si le canal doit s'éteindre.
    fn horloge(&mut self) -> bool {
        if self.actif && self.compte > 0 {
            self.compte -= 1;
            return self.compte == 0;
        }
        false
    }

    /// Écriture de NRx4 (bit 6 = longueur active, bit 7 = déclenchement).
    /// `moitie` : la prochaine étape du séquenceur ne compte pas la longueur.
    /// Renvoie faux si le canal doit s'éteindre (pas supplémentaire tombé à 0 sans déclenchement).
    fn ecrit_nrx4(&mut self, v: u8, max: u16, moitie: bool) -> bool {
        let avant = self.actif;
        self.actif = v & 0x40 != 0;
        let mut garde = true;
        // Activer la longueur pendant la première moitié de période la compte une fois de plus.
        if moitie && !avant && self.actif && self.compte > 0 {
            self.compte -= 1;
            garde = self.compte > 0 || v & 0x80 != 0;
        }
        if v & 0x80 != 0 && self.compte == 0 {
            self.compte = if self.actif && moitie { max - 1 } else { max };
        }
        garde
    }
}

#[derive(Default)]
struct Carre {
    on: bool,
    dac: bool,
    rapport: u8,
    pos: u8,
    freq: u16,
    compte: u16,
    long: Longueur,
    env: Enveloppe,
}

impl Carre {
    fn sauve(&self, e: &mut Vec<u8>) {
        e.bool(self.on);
        e.bool(self.dac);
        e.u8(self.rapport);
        e.u8(self.pos);
        e.u16(self.freq);
        e.u16(self.compte);
        self.long.sauve(e);
        self.env.sauve(e);
    }

    fn charge(&mut self, l: &mut Lecteur) -> Result<(), ErreurEtat> {
        self.on = l.bool()?;
        self.dac = l.bool()?;
        self.rapport = l.u8_max(3, "carré")?;
        self.pos = l.u8_max(7, "carré")?;
        self.freq = l.u16_max(2047, "carré")?;
        self.compte = l.u16_max(2048, "carré")?;
        verifie(!self.on || self.compte > 0, "carré")?;
        self.long.charge(l)?;
        self.env.charge(l)
    }

    fn sortie(&self) -> u8 {
        if self.on && RAPPORTS[self.rapport as usize] & (0x80 >> self.pos) != 0 {
            self.env.vol
        } else {
            0
        }
    }

    /// Un cycle machine ; vrai si la sortie peut changer.
    #[inline]
    fn tick(&mut self) -> bool {
        self.compte -= 1;
        if self.compte == 0 {
            self.compte = 2048 - self.freq;
            self.pos = (self.pos + 1) & 7;
            return true;
        }
        false
    }

    fn declenche(&mut self) {
        self.on = self.dac;
        self.compte = 2048 - self.freq;
        self.env.declenche();
    }
}

#[derive(Default)]
struct Balayage {
    periode: u8,
    negatif: bool,
    decalage: u8,
    compte: u8,
    ombre: u16,
    actif: bool,
    negatif_utilise: bool,
}

impl Balayage {
    fn sauve(&self, e: &mut Vec<u8>) {
        e.octets(&[self.periode, self.decalage, self.compte]);
        e.u16(self.ombre);
        e.bool(self.negatif);
        e.bool(self.actif);
        e.bool(self.negatif_utilise);
    }

    fn charge(&mut self, l: &mut Lecteur) -> Result<(), ErreurEtat> {
        self.periode = l.u8_max(7, "balayage")?;
        self.decalage = l.u8_max(7, "balayage")?;
        self.compte = l.u8()?;
        // Au plus 2047 : le calcul ombre + ombre >> n ne déborde pas.
        self.ombre = l.u16_max(2047, "balayage")?;
        self.negatif = l.bool()?;
        self.actif = l.bool()?;
        self.negatif_utilise = l.bool()?;
        Ok(())
    }

    fn calcul(&mut self) -> u16 {
        let d = self.ombre >> self.decalage;
        if self.negatif {
            self.negatif_utilise = true;
            self.ombre - d
        } else {
            self.ombre + d
        }
    }

    fn recharge(&mut self) {
        self.compte = if self.periode == 0 { 8 } else { self.periode };
    }
}

#[derive(Default)]
struct Onde {
    on: bool,
    dac: bool,
    volume: u8,
    freq: u16,
    /// Compte à rebours en unités de 2 points (2 MHz) ; lecture de l'échantillon suivant à 0.
    compte: u16,
    pos: u8,
    tampon: u8,
    /// La dernière unité a lu la RAM d'onde (accès CPU possible sur DMG).
    lu: bool,
    long: Longueur,
    /// GBA (déduits de NR30/NR32, hors état) : banque jouée, 64 échantillons sur les deux
    /// banques, volume forcé à 75 %.
    banque: u8,
    deux_banques: bool,
    trois_quarts: bool,
}

impl Onde {
    fn sauve(&self, e: &mut Vec<u8>) {
        e.bool(self.on);
        e.bool(self.dac);
        e.u8(self.volume);
        e.u16(self.freq);
        e.u16(self.compte);
        e.u8(self.pos);
        e.u8(self.tampon);
        e.bool(self.lu);
        self.long.sauve(e);
    }

    fn charge(&mut self, l: &mut Lecteur) -> Result<(), ErreurEtat> {
        self.on = l.bool()?;
        self.dac = l.bool()?;
        self.volume = l.u8_max(3, "onde")?;
        self.freq = l.u16_max(2047, "onde")?;
        // 2047 - freq + 3 au déclenchement.
        self.compte = l.u16_max(2050, "onde")?;
        self.pos = l.u8_max(63, "onde")?;
        self.tampon = l.u8()?;
        self.lu = l.bool()?;
        self.long.charge(l)
    }

    fn sortie(&self) -> u8 {
        if !self.on || (self.volume == 0 && !self.trois_quarts) {
            return 0;
        }
        let s = if self.pos & 1 == 0 {
            self.tampon >> 4
        } else {
            self.tampon & 0x0F
        };
        if self.trois_quarts {
            return (s * 3) >> 2;
        }
        s >> (self.volume - 1)
    }

    /// Un cycle machine (2 unités) ; vrai si un échantillon a été lu.
    #[inline]
    fn tick(&mut self, ram: &[u8; 0x20]) -> bool {
        if self.compte >= 2 {
            self.compte -= 2;
            self.lu = false;
            return false;
        }
        let mut lu = false;
        for _ in 0..2 {
            if self.compte == 0 {
                self.compte = 2047 - self.freq;
                self.pos = (self.pos + 1) & if self.deux_banques { 63 } else { 31 };
                self.tampon = ram[(((self.banque << 5) + self.pos) & 63) as usize >> 1];
                lu = true;
                self.lu = true;
            } else {
                self.compte -= 1;
                self.lu = false;
            }
        }
        lu
    }
}

#[derive(Default)]
struct Bruit {
    on: bool,
    dac: bool,
    decalage: u8,
    etroit: bool,
    diviseur: u8,
    compte: u32,
    lfsr: u16,
    long: Longueur,
    env: Enveloppe,
}

impl Bruit {
    fn sauve(&self, e: &mut Vec<u8>) {
        e.bool(self.on);
        e.bool(self.dac);
        e.octets(&[self.decalage, self.diviseur]);
        e.bool(self.etroit);
        e.u32(self.compte);
        e.u16(self.lfsr);
        self.long.sauve(e);
        self.env.sauve(e);
    }

    fn charge(&mut self, l: &mut Lecteur) -> Result<(), ErreurEtat> {
        self.on = l.bool()?;
        self.dac = l.bool()?;
        self.decalage = l.u8_max(15, "bruit")?;
        self.diviseur = l.u8_max(7, "bruit")?;
        self.etroit = l.bool()?;
        self.compte = l.u32()?;
        verifie(
            self.compte <= 28 << 15 && (!self.on || self.compte > 0),
            "bruit",
        )?;
        self.lfsr = l.u16_max(0x7FFF, "bruit")?;
        self.long.charge(l)?;
        self.env.charge(l)
    }

    fn periode(&self) -> u32 {
        (DIVISEURS[self.diviseur as usize] as u32) << self.decalage
    }

    fn sortie(&self) -> u8 {
        if self.on && self.lfsr & 1 == 0 {
            self.env.vol
        } else {
            0
        }
    }

    #[inline]
    fn tick(&mut self) -> bool {
        self.compte -= 1;
        if self.compte > 0 {
            return false;
        }
        self.compte = self.periode();
        // Décalages 14 et 15 : le registre ne reçoit plus d'horloge.
        if self.decalage >= 14 {
            return false;
        }
        let x = (self.lfsr ^ (self.lfsr >> 1)) & 1;
        self.lfsr = (self.lfsr >> 1) | (x << 14);
        if self.etroit {
            self.lfsr = (self.lfsr & !0x40) | (x << 6);
        }
        true
    }
}

/// Mixage GBA (registres du son direct tenus par gba-core).
#[derive(Clone, Copy, Default)]
struct MixGba {
    /// Volume PSG de SOUNDCNT_H (0 = 25 %, 1 = 50 %, 2 = 100 %).
    psg: u8,
    /// Contribution du son direct A/B, à l'échelle 10 bits de la sortie.
    ext_g: i32,
    ext_d: i32,
    biais: i32,
}

pub struct Apu {
    regs: [u8; 0x20],
    /// RAM d'onde ; GBA : 2 banques de 16 octets (la 2e inutilisée sur Game Boy).
    wave: [u8; 0x20],
    allume: bool,
    /// Prochaine étape du séquenceur (0-7).
    etape: u8,
    c1: Carre,
    balayage: Balayage,
    c2: Carre,
    c3: Onde,
    c4: Bruit,
    /// Sortie à recalculer (un canal, NR50/NR51 ou un DAC a changé).
    sale: bool,
    niv_g: f32,
    niv_d: f32,
    somme_g: f32,
    somme_d: f32,
    n: u32,
    /// Valeur de `n` au dernier changement de niveau.
    n_niv: u32,
    frequence: u32,
    phase: u32,
    /// Cycles jusqu'au prochain échantillon (inclus).
    attente: u32,
    /// Cycles jusqu'au prochain front descendant du bit 4 de DIV (inclus).
    avant_div: u32,
    /// Facteur de charge du condensateur par échantillon.
    charge: f32,
    cond_g: f32,
    cond_d: f32,
    echantillons: Vec<f32>,
    /// Mode CGB : RAM d'onde accessible pendant la lecture, longueurs remises à zéro hors tension.
    cgb: bool,
    /// Mode GBA (`new_gba`) : registres relus sans masque, RAM d'onde en banques, mixage 10 bits.
    gba: Option<MixGba>,
}

impl Apu {
    pub fn new(cgb: bool) -> Self {
        let mut apu = Apu {
            regs: [0; 0x20],
            wave: [0; 0x20],
            allume: true,
            etape: 0,
            c1: Carre::default(),
            balayage: Balayage::default(),
            c2: Carre::default(),
            c3: Onde::default(),
            c4: Bruit::default(),
            sale: true,
            niv_g: 0.0,
            niv_d: 0.0,
            somme_g: 0.0,
            somme_d: 0.0,
            n: 0,
            n_niv: 0,
            frequence: 0,
            phase: 0,
            attente: 1,
            avant_div: 1,
            charge: 0.0,
            cond_g: 0.0,
            cond_d: 0.0,
            echantillons: Vec::new(),
            cgb,
            gba: None,
        };
        apu.set_sample_rate(48_000);
        // État après la boot ROM DMG : canal 1 actif (fin du « ding »), volume retombé à 0.
        for (a, v) in [
            (0xFF11, 0x80),
            (0xFF12, 0xF3),
            (0xFF13, 0xC1),
            (0xFF14, 0x87),
            (0xFF24, 0x77),
            (0xFF25, 0xF3),
        ] {
            apu.write(a, v);
        }
        apu.c1.env.vol = 0;
        // Condensateur déjà chargé au niveau de repos : pas de clic au démarrage.
        apu.mixe();
        apu.sale = false;
        apu.cond_g = apu.niv_g;
        apu.cond_d = apu.niv_d;
        apu
    }

    /// Canaux Game Boy de la GBA : comportement CGB, registres à zéro, séquenceur libre (pas de
    /// DIV) ; sortie mixée avec le son direct fourni par `mix_gba`.
    pub fn new_gba() -> Self {
        let mut apu = Apu::new(true);
        apu.eteint();
        apu.allume = true;
        apu.gba = Some(MixGba::default());
        apu.mixe();
        apu.sale = false;
        apu.cond_g = apu.niv_g;
        apu.cond_d = apu.niv_d;
        apu
    }

    /// GBA : volume PSG (SOUNDCNT_H bits 0-1), son direct déjà mis à l'échelle 10 bits
    /// (gauche, droite), biais (SOUNDBIAS). À appeler une fois l'APU rattrapé.
    pub fn mix_gba(&mut self, psg: u8, ext_g: i32, ext_d: i32, biais: u16) {
        self.gba = Some(MixGba {
            psg: psg.min(2),
            ext_g,
            ext_d,
            biais: (biais & 0x3FE) as i32,
        });
        self.integre();
        self.mixe();
    }

    /// Sauvegarde d'état ; la fréquence de sortie (réglage de l'hôte) et les échantillons
    /// en attente n'en font pas partie.
    pub fn sauve(&self, e: &mut Vec<u8>) {
        e.octets(&self.regs);
        e.octets(&self.wave[..self.taille_onde()]);
        e.bool(self.allume);
        e.u8(self.etape);
        self.c1.sauve(e);
        self.balayage.sauve(e);
        self.c2.sauve(e);
        self.c3.sauve(e);
        self.c4.sauve(e);
        e.bool(self.sale);
        for v in [
            self.niv_g,
            self.niv_d,
            self.somme_g,
            self.somme_d,
            self.cond_g,
            self.cond_d,
        ] {
            e.f32(v);
        }
        e.u32(self.n);
        e.u32(self.n_niv);
        e.u32(self.phase);
        e.u32(self.avant_div);
    }

    /// Relit l'état (les échantillons en attente sont gardés).
    pub(crate) fn charge(&mut self, l: &mut Lecteur) -> Result<(), ErreurEtat> {
        l.octets(&mut self.regs)?;
        let n = self.taille_onde();
        l.octets(&mut self.wave[..n])?;
        self.allume = l.bool()?;
        self.etape = l.u8_max(7, "séquenceur")?;
        self.c1.charge(l)?;
        self.balayage.charge(l)?;
        self.c2.charge(l)?;
        self.c3.charge(l)?;
        self.c4.charge(l)?;
        self.sale = l.bool()?;
        self.niv_g = l.f32()?;
        self.niv_d = l.f32()?;
        self.somme_g = l.f32()?;
        self.somme_d = l.f32()?;
        self.cond_g = l.f32()?;
        self.cond_d = l.f32()?;
        self.n = l.u32()?;
        self.n_niv = l.u32()?;
        self.phase = l.u32()?;
        self.avant_div = l.u32()?;
        verifie(self.n_niv <= self.n && self.n <= CYCLES_PAR_S, "APU")?;
        verifie(self.phase < CYCLES_PAR_S, "APU")?;
        // Niveaux de sortie (au plus 2 en GBA) et condensateur bornés, sommes sur `n` cycles :
        // sinon les échantillons débordent en infini.
        verifie(
            [self.niv_g, self.niv_d, self.cond_g, self.cond_d]
                .iter()
                .all(|v| v.abs() <= 4.0)
                && [self.somme_g, self.somme_d]
                    .iter()
                    .all(|v| v.abs() <= 4.0 * self.n as f32),
            "APU niveaux",
        )?;
        verifie((1..=2048).contains(&self.avant_div), "APU séquenceur")?;
        verifie(self.gba.is_some() || self.c3.pos < 32, "onde")?;
        if self.gba.is_some() {
            self.regle_onde_gba();
        }
        // Même invariant que `tick` : attente = cycles restants jusqu'à l'échantillon suivant.
        self.attente = (CYCLES_PAR_S - self.phase).div_ceil(self.frequence);
        Ok(())
    }

    /// Relit un état produit par [`Apu::sauve`] occupant tout `data` (pour gba-core).
    /// En cas d'erreur, l'APU peut être à moitié rechargé : l'appelant restaure l'état d'avant.
    pub fn charge_etat(&mut self, data: &[u8]) -> Result<(), ErreurEtat> {
        let mut l = Lecteur::new(data);
        self.charge(&mut l)?;
        verifie(l.fini(), "APU")
    }

    /// Octets de RAM d'onde sauvegardés (une banque sur Game Boy, deux sur GBA).
    fn taille_onde(&self) -> usize {
        if self.gba.is_some() {
            0x20
        } else {
            0x10
        }
    }

    /// GBA : banque, dimension et volume 75 % du canal 3 depuis NR30/NR32.
    fn regle_onde_gba(&mut self) {
        let (nr30, nr32) = (self.regs[0x0A], self.regs[0x0C]);
        self.c3.banque = (nr30 >> 6) & 1;
        self.c3.deux_banques = nr30 & 0x20 != 0;
        self.c3.trois_quarts = nr32 & 0x80 != 0;
    }

    /// Fréquence de sortie (Hz, bornée à 8 000-192 000).
    pub fn set_sample_rate(&mut self, hz: u32) {
        self.frequence = hz.clamp(8_000, 192_000);
        // Condensateur DMG : 0,999958 par point.
        let points = 4.0 * CYCLES_PAR_S as f64 / self.frequence as f64;
        self.charge = 0.999958f64.powf(points) as f32;
        self.attente = (CYCLES_PAR_S - self.phase).div_ceil(self.frequence);
    }

    /// Ajoute à `out` les échantillons stéréo entrelacés (G, D) produits depuis le dernier appel.
    pub fn drain_samples(&mut self, out: &mut Vec<f32>) {
        out.append(&mut self.echantillons);
    }

    /// Avance de `k` cycles machine : saute d'un événement au suivant (pas de canal, échantillon)
    /// et n'exécute cycle par cycle que les cycles où quelque chose se passe.
    pub fn avance(&mut self, k: u64) {
        // Garde-fou : au plus 1 s rattrapée d'un coup (l'écart normal est de quelques images) ;
        // un écart absurde ne peut pas figer l'émulation.
        let mut k = k.min(CYCLES_PAR_S as u64);
        while k > 0 {
            let mut s = self
                .attente
                .min(self.avant_div)
                .min(k.min(u32::MAX as u64) as u32);
            if self.allume {
                if self.c1.on {
                    s = s.min(self.c1.compte as u32);
                }
                if self.c2.on {
                    s = s.min(self.c2.compte as u32);
                }
                if self.c3.on {
                    s = s.min(self.c3.compte as u32 / 2 + 1);
                }
                if self.c4.on {
                    s = s.min(self.c4.compte);
                }
            }
            // s - 1 cycles sans événement, puis un cycle complet.
            let m = s - 1;
            if m > 0 {
                if self.allume {
                    if self.c1.on {
                        self.c1.compte -= m as u16;
                    }
                    if self.c2.on {
                        self.c2.compte -= m as u16;
                    }
                    if self.c3.on {
                        self.c3.compte -= 2 * m as u16;
                        self.c3.lu = false;
                    }
                    if self.c4.on {
                        self.c4.compte -= m;
                    }
                }
                self.n += m;
                self.phase += self.frequence * m;
                self.attente -= m;
            }
            self.avant_div -= s;
            if self.avant_div == 0 {
                self.avant_div = 2048;
                self.frame_seq();
            }
            self.tick();
            k -= s as u64;
        }
    }

    /// Avance d'un cycle machine.
    #[inline]
    fn tick(&mut self) {
        if self.allume {
            if self.c1.on {
                self.sale |= self.c1.tick();
            }
            if self.c2.on {
                self.sale |= self.c2.tick();
            }
            if self.c3.on {
                self.sale |= self.c3.tick(&self.wave);
            }
            if self.c4.on {
                self.sale |= self.c4.tick();
            }
        }
        if self.sale {
            self.sale = false;
            self.integre();
            self.mixe();
        }
        self.n += 1;
        self.phase += self.frequence;
        self.attente -= 1;
        if self.phase >= CYCLES_PAR_S {
            self.phase -= CYCLES_PAR_S;
            self.attente = (CYCLES_PAR_S - self.phase).div_ceil(self.frequence);
            self.echantillon();
        }
    }

    /// Ajoute le niveau courant, tenu depuis le dernier changement, aux sommes de l'échantillon.
    fn integre(&mut self) {
        let c = (self.n - self.n_niv) as f32;
        self.somme_g += self.niv_g * c;
        self.somme_d += self.niv_d * c;
        self.n_niv = self.n;
    }

    fn echantillon(&mut self) {
        self.integre();
        self.n_niv = 0;
        let n = self.n as f32;
        let (g, d) = (self.somme_g / n, self.somme_d / n);
        self.somme_g = 0.0;
        self.somme_d = 0.0;
        self.n = 0;
        let sg = g - self.cond_g;
        let sd = d - self.cond_d;
        self.cond_g = g - sg * self.charge;
        self.cond_d = d - sd * self.charge;
        // Au plus 1 s en attente si personne ne vide le tampon (exécution sans son).
        if self.echantillons.len() < 2 * self.frequence as usize {
            self.echantillons.push(sg);
            self.echantillons.push(sd);
        }
    }

    fn mixe(&mut self) {
        let canaux = [
            (self.c1.dac, self.c1.sortie()),
            (self.c2.dac, self.c2.sortie()),
            (self.c3.dac, self.c3.sortie()),
            (self.c4.dac, self.c4.sortie()),
        ];
        let nr51 = self.regs[0x15];
        let nr50 = self.regs[0x14];
        if let Some(m) = self.gba {
            // GBA : canaux 0-15 non signés, ×(1-8) (max 480), volume PSG, son direct, biais,
            // saturation 10 bits, puis biais retiré.
            let (mut g, mut d) = (0i32, 0i32);
            for (i, &(_, s)) in canaux.iter().enumerate() {
                if nr51 & (0x10 << i) != 0 {
                    g += s as i32;
                }
                if nr51 & (1 << i) != 0 {
                    d += s as i32;
                }
            }
            let sh = 2 - m.psg;
            g = ((g * (((nr50 >> 4) & 7) + 1) as i32) >> sh) + m.ext_g;
            d = ((d * ((nr50 & 7) + 1) as i32) >> sh) + m.ext_d;
            let sortie = |x: i32| ((x + m.biais).clamp(0, 0x3FF) - m.biais) as f32 / 512.0;
            self.niv_g = sortie(g);
            self.niv_d = sortie(d);
            return;
        }
        let (mut g, mut d) = (0.0, 0.0);
        for (i, &(dac, s)) in canaux.iter().enumerate() {
            if !dac {
                continue;
            }
            let a = s as f32 / 7.5 - 1.0;
            if nr51 & (0x10 << i) != 0 {
                g += a;
            }
            if nr51 & (1 << i) != 0 {
                d += a;
            }
        }
        // Volume maître (1-8)/8, puis /4 canaux.
        self.niv_g = g * (((nr50 >> 4) & 7) + 1) as f32 / 32.0;
        self.niv_d = d * ((nr50 & 7) + 1) as f32 / 32.0;
    }

    /// DIV interne (points) vient de changer hors comptage normal : recale le séquenceur.
    /// En double vitesse (CGB), DIV avance 2× plus vite et le séquenceur suit son bit 5.
    pub fn recale_div(&mut self, div: u16, double: bool) {
        self.avant_div = if double {
            (0x4000 - (div & 0x3FFF) as u32).div_ceil(8)
        } else {
            (0x2000 - (div & 0x1FFF) as u32) / 4
        };
    }

    /// Front descendant du bit 4 de DIV (512 Hz).
    pub fn frame_seq(&mut self) {
        if !self.allume {
            return;
        }
        let e = self.etape;
        self.etape = (e + 1) & 7;
        if e & 1 == 0 {
            if self.c1.long.horloge() {
                self.c1.on = false;
            }
            if self.c2.long.horloge() {
                self.c2.on = false;
            }
            if self.c3.long.horloge() {
                self.c3.on = false;
            }
            if self.c4.long.horloge() {
                self.c4.on = false;
            }
        }
        if (e == 2 || e == 6) && self.c1.on {
            self.horloge_balayage();
        }
        if e == 7 {
            if self.c1.on {
                self.c1.env.horloge();
            }
            if self.c2.on {
                self.c2.env.horloge();
            }
            if self.c4.on {
                self.c4.env.horloge();
            }
        }
        self.sale = true;
    }

    fn horloge_balayage(&mut self) {
        let b = &mut self.balayage;
        b.compte = b.compte.saturating_sub(1);
        if b.compte > 0 {
            return;
        }
        b.recharge();
        if !b.actif || b.periode == 0 {
            return;
        }
        let f = b.calcul();
        if f > 2047 {
            self.c1.on = false;
        } else if b.decalage != 0 {
            b.ombre = f;
            self.c1.freq = f;
            if b.calcul() > 2047 {
                self.c1.on = false;
            }
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        if self.gba.is_some() && (0xFF30..=0xFF3F).contains(&addr) {
            return self.wave[self.index_onde_gba(addr)];
        }
        match addr {
            0xFF26 => {
                (self.allume as u8) << 7
                    | 0x70
                    | self.c1.on as u8
                    | (self.c2.on as u8) << 1
                    | (self.c3.on as u8) << 2
                    | (self.c4.on as u8) << 3
            }
            // GBA : valeurs brutes, gba-core applique ses propres masques.
            0xFF10..=0xFF2F if self.gba.is_some() => self.regs[(addr - 0xFF10) as usize],
            0xFF10..=0xFF2F => {
                let i = (addr - 0xFF10) as usize;
                self.regs[i] | MASQUES[i]
            }
            _ => match self.index_onde(addr) {
                Some(i) => self.wave[i],
                None => 0xFF,
            },
        }
    }

    /// Octet de RAM d'onde visible par le CPU : pendant la lecture par le canal 3, seul l'octet
    /// en cours est accessible (DMG : seulement au cycle où le canal le lit ; CGB : toujours).
    fn index_onde(&self, addr: u16) -> Option<usize> {
        if !self.c3.on {
            Some((addr & 0x0F) as usize)
        } else if self.c3.lu || self.cgb {
            Some((self.c3.pos >> 1) as usize)
        } else {
            None
        }
    }

    /// GBA : le CPU voit la banque qui n'est pas sélectionnée pour la lecture.
    fn index_onde_gba(&self, addr: u16) -> usize {
        ((((self.regs[0x0A] >> 6) & 1) ^ 1) as usize) << 4 | (addr & 0x0F) as usize
    }

    pub fn write(&mut self, addr: u16, v: u8) {
        if addr >= 0xFF30 && self.gba.is_some() {
            let i = self.index_onde_gba(addr);
            self.wave[i] = v;
            return;
        }
        if addr >= 0xFF30 {
            if let Some(i) = self.index_onde(addr) {
                self.wave[i] = v;
            }
            return;
        }
        if addr == 0xFF26 {
            if v & 0x80 == 0 && self.allume {
                self.eteint();
            } else if v & 0x80 != 0 && !self.allume {
                self.allume = true;
                // raccourci : pas de saut du premier pas si le bit 4 de DIV est à 1 à l'allumage.
                self.etape = 0;
            }
            return;
        }
        if !self.allume {
            // DMG : les compteurs de longueur restent accessibles hors tension (pas en CGB).
            if self.cgb {
                return;
            }
            match addr {
                0xFF11 => self.c1.long.compte = 64 - (v & 0x3F) as u16,
                0xFF16 => self.c2.long.compte = 64 - (v & 0x3F) as u16,
                0xFF1B => self.c3.long.compte = 256 - v as u16,
                0xFF20 => self.c4.long.compte = 64 - (v & 0x3F) as u16,
                _ => {}
            }
            return;
        }
        self.regs[(addr - 0xFF10) as usize] = v;
        self.sale = true;
        // Prochaine étape impaire : elle ne compte pas la longueur.
        let moitie = self.etape & 1 == 1;
        match addr {
            0xFF10 => {
                let b = &mut self.balayage;
                b.periode = (v >> 4) & 7;
                b.decalage = v & 7;
                let negatif = v & 0x08 != 0;
                // Quitter le mode soustraction après un calcul négatif coupe le canal.
                if b.negatif && !negatif && b.negatif_utilise {
                    self.c1.on = false;
                }
                b.negatif = negatif;
            }
            0xFF11 => {
                self.c1.rapport = v >> 6;
                self.c1.long.compte = 64 - (v & 0x3F) as u16;
            }
            0xFF12 => {
                self.c1.env.ecrit(v);
                self.c1.dac = v & 0xF8 != 0;
                self.c1.on &= self.c1.dac;
            }
            0xFF13 => self.c1.freq = (self.c1.freq & 0x700) | v as u16,
            0xFF14 => {
                self.c1.freq = (self.c1.freq & 0xFF) | ((v as u16 & 7) << 8);
                if !self.c1.long.ecrit_nrx4(v, 64, moitie) {
                    self.c1.on = false;
                }
                if v & 0x80 != 0 {
                    self.c1.declenche();
                    let b = &mut self.balayage;
                    b.ombre = self.c1.freq;
                    b.recharge();
                    b.actif = b.periode != 0 || b.decalage != 0;
                    b.negatif_utilise = false;
                    if b.decalage != 0 && b.calcul() > 2047 {
                        self.c1.on = false;
                    }
                }
            }
            0xFF16 => {
                self.c2.rapport = v >> 6;
                self.c2.long.compte = 64 - (v & 0x3F) as u16;
            }
            0xFF17 => {
                self.c2.env.ecrit(v);
                self.c2.dac = v & 0xF8 != 0;
                self.c2.on &= self.c2.dac;
            }
            0xFF18 => self.c2.freq = (self.c2.freq & 0x700) | v as u16,
            0xFF19 => {
                self.c2.freq = (self.c2.freq & 0xFF) | ((v as u16 & 7) << 8);
                if !self.c2.long.ecrit_nrx4(v, 64, moitie) {
                    self.c2.on = false;
                }
                if v & 0x80 != 0 {
                    self.c2.declenche();
                }
            }
            0xFF1A => {
                self.c3.dac = v & 0x80 != 0;
                self.c3.on &= self.c3.dac;
                if self.gba.is_some() {
                    self.regle_onde_gba();
                }
            }
            0xFF1B => self.c3.long.compte = 256 - v as u16,
            0xFF1C => {
                self.c3.volume = (v >> 5) & 3;
                if self.gba.is_some() {
                    self.regle_onde_gba();
                }
            }
            0xFF1D => self.c3.freq = (self.c3.freq & 0x700) | v as u16,
            0xFF1E => {
                self.c3.freq = (self.c3.freq & 0xFF) | ((v as u16 & 7) << 8);
                if !self.c3.long.ecrit_nrx4(v, 256, moitie) {
                    self.c3.on = false;
                }
                if v & 0x80 != 0 {
                    self.declenche_onde();
                }
            }
            0xFF20 => self.c4.long.compte = 64 - (v & 0x3F) as u16,
            0xFF21 => {
                self.c4.env.ecrit(v);
                self.c4.dac = v & 0xF8 != 0;
                self.c4.on &= self.c4.dac;
            }
            0xFF22 => {
                self.c4.decalage = v >> 4;
                self.c4.etroit = v & 0x08 != 0;
                self.c4.diviseur = v & 7;
            }
            0xFF23 => {
                if !self.c4.long.ecrit_nrx4(v, 64, moitie) {
                    self.c4.on = false;
                }
                if v & 0x80 != 0 {
                    let c = &mut self.c4;
                    c.on = c.dac;
                    c.lfsr = 0x7FFF;
                    c.compte = c.periode();
                    c.env.declenche();
                }
            }
            _ => {}
        }
    }

    fn declenche_onde(&mut self) {
        let c = &mut self.c3;
        // DMG : redéclencher juste avant une lecture recopie une partie de la RAM d'onde en tête
        // (pas sur CGB).
        if !self.cgb && c.on && c.compte == 0 {
            let o = (((c.pos + 1) >> 1) & 0x0F) as usize;
            if o < 4 {
                self.wave[0] = self.wave[o];
            } else {
                let b = o & !3;
                self.wave.copy_within(b..b + 4, 0);
            }
        }
        c.on = c.dac;
        c.pos = 0;
        c.lu = false;
        // Premier échantillon lu avec 3 unités de retard.
        c.compte = 2047 - c.freq + 3;
    }

    /// Mise hors tension : registres à zéro, canaux coupés ; sur DMG les longueurs restent.
    fn eteint(&mut self) {
        let longueurs = [
            self.c1.long.compte,
            self.c2.long.compte,
            self.c3.long.compte,
            self.c4.long.compte,
        ];
        self.regs[..0x16].fill(0);
        self.allume = false;
        self.c1 = Carre::default();
        self.balayage = Balayage::default();
        self.c2 = Carre::default();
        self.c3 = Onde::default();
        self.c4 = Bruit::default();
        if !self.cgb {
            self.c1.long.compte = longueurs[0];
            self.c2.long.compte = longueurs[1];
            self.c3.long.compte = longueurs[2];
            self.c4.long.compte = longueurs[3];
        }
        self.sale = true;
    }
}
