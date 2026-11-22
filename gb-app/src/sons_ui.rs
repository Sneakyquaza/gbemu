//! Sons d'interface synthétisés au démarrage (aucun échantillon tiers) : clic plastique,
//! bips carrés façon APU, « ding » à deux notes. Discrets (volume 40 %), désactivables,
//! jamais joués pendant la partie. Chacun dure moins de 120 ms (taille du tampon de sortie).

use crate::audio::{Sortie, FREQUENCE};

#[derive(Clone, Copy)]
pub enum Son {
    /// Déplacement de la sélection.
    Deplacement,
    Valider,
    Retour,
    Erreur,
    Favori,
    /// Écran de démarrage.
    Ding,
}

/// Volume général des sons d'interface.
const VOLUME: f32 = 0.4;

pub struct SonsUi {
    sons: [Vec<f32>; 6],
}

/// Une note : fréquence (Hz), début et durée (s), amplitude, dureté du carré (0 = sinus).
struct Note(f32, f32, f32, f32, f32);

/// Mono → stéréo entrelacé, avec attaque et extinction exponentielle par note (pas de clic).
fn synth(notes: &[Note], bruit: f32) -> Vec<f32> {
    let fin = notes.iter().map(|n| n.1 + n.2).fold(0.0f32, f32::max);
    let n = (fin * FREQUENCE as f32) as usize;
    let mut v = Vec::with_capacity(n * 2);
    let mut graine: u32 = 0x1234_5677;
    for i in 0..n {
        let t = i as f32 / FREQUENCE as f32;
        let mut s = 0.0;
        for Note(f, debut, duree, amp, durete) in notes {
            let u = t - debut;
            if u < 0.0 || u > *duree {
                continue;
            }
            let attaque = (u / 0.002).min(1.0);
            let queue = ((duree - u) / 0.004).min(1.0);
            let env = attaque * queue * (-u * 4.0 / duree).exp();
            let x = (std::f32::consts::TAU * f * u).sin();
            // Carré adouci (tanh) : la couleur de l'APU sans ses harmoniques agressives.
            let onde = if *durete > 0.0 {
                (x * durete).tanh() / durete.tanh()
            } else {
                x
            };
            s += onde * amp * env;
        }
        if bruit > 0.0 {
            graine ^= graine << 13;
            graine ^= graine >> 17;
            graine ^= graine << 5;
            let b = (graine as f32 / u32::MAX as f32) * 2.0 - 1.0;
            s += b * bruit * (-t * 400.0).exp();
        }
        let s = s * VOLUME;
        v.push(s);
        v.push(s);
    }
    v
}

impl SonsUi {
    pub fn new() -> SonsUi {
        SonsUi {
            sons: [
                // Clic plastique : craquement de bruit très court + tic aigu.
                synth(&[Note(2400.0, 0.0, 0.015, 0.05, 0.0)], 0.05),
                synth(
                    &[
                        Note(1046.5, 0.0, 0.028, 0.10, 3.0),
                        Note(1568.0, 0.03, 0.035, 0.10, 3.0),
                    ],
                    0.0,
                ),
                synth(
                    &[
                        Note(880.0, 0.0, 0.025, 0.08, 3.0),
                        Note(587.3, 0.026, 0.035, 0.08, 3.0),
                    ],
                    0.0,
                ),
                synth(
                    &[
                        Note(110.0, 0.0, 0.09, 0.16, 4.0),
                        Note(116.5, 0.0, 0.09, 0.10, 4.0),
                    ],
                    0.0,
                ),
                synth(
                    &[
                        Note(1318.5, 0.0, 0.03, 0.07, 1.5),
                        Note(1760.0, 0.03, 0.05, 0.07, 1.5),
                    ],
                    0.0,
                ),
                // Deux notes à l'octave, en clin d'œil au démarrage de la console.
                synth(
                    &[
                        Note(1046.5, 0.0, 0.04, 0.09, 2.0),
                        Note(2093.0, 0.04, 0.07, 0.08, 2.0),
                    ],
                    0.0,
                ),
            ],
        }
    }

    pub fn jouer(&self, son: Son, sortie: Option<&Sortie>) {
        if let Some(s) = sortie {
            s.melanger(&self.sons[son as usize]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sons_courts_et_sans_saturation() {
        let s = SonsUi::new();
        for v in &s.sons {
            assert!(!v.is_empty() && v.len() % 2 == 0);
            // Moins de 120 ms : tient dans le tampon de sortie sans être tronqué.
            assert!(v.len() < (FREQUENCE as usize * 2 * 120 / 1000));
            assert!(v.iter().all(|x| x.abs() < 0.5));
        }
    }
}
