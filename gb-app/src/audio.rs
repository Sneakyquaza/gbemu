//! Sortie son (cpal) : stéréo 48 kHz, échantillons f32 entrelacés poussés par l'émulation.
//! Sans périphérique audio, la partie tourne sans son.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

pub const FREQUENCE: u32 = 48_000;
/// Au-delà (≈ 120 ms), les échantillons en trop sont jetés pour garder une latence faible.
const TAMPON_MAX: usize = FREQUENCE as usize * 2 * 120 / 1000;

pub struct Sortie {
    _flux: cpal::Stream,
    tampon: Arc<Mutex<VecDeque<f32>>>,
}

impl Sortie {
    /// `None` sans périphérique de sortie.
    pub fn ouvrir() -> Option<Sortie> {
        let tampon: Arc<Mutex<VecDeque<f32>>> = Arc::default();
        let flux = ouvrir_flux(Arc::clone(&tampon))?;
        Some(Sortie {
            _flux: flux,
            tampon,
        })
    }

    /// Échantillons stéréo entrelacés (G, D, G, D…).
    pub fn pousser(&self, ech: &[f32]) {
        if let Ok(mut t) = self.tampon.lock() {
            t.extend(ech.iter().copied());
            let trop = t.len().saturating_sub(TAMPON_MAX);
            t.drain(..trop & !1);
        }
    }

    /// Mélange des échantillons (sons d'interface) à ce qui est déjà en attente.
    pub fn melanger(&self, ech: &[f32]) {
        if let Ok(mut t) = self.tampon.lock() {
            let n = t.len().min(ech.len()) & !1;
            for (s, e) in t.iter_mut().zip(&ech[..n]) {
                *s += e;
            }
            t.extend(ech[n..].iter().copied());
            let trop = t.len().saturating_sub(TAMPON_MAX);
            t.drain(..trop & !1);
        }
    }

    /// Échantillons en attente (G et D comptés séparément).
    pub fn remplissage(&self) -> usize {
        self.tampon.lock().map_or(0, |t| t.len())
    }

    pub fn vider(&self) {
        if let Ok(mut t) = self.tampon.lock() {
            t.clear();
        }
    }
}

fn ouvrir_flux(tampon: Arc<Mutex<VecDeque<f32>>>) -> Option<cpal::Stream> {
    let hote = cpal::default_host();
    let peripherique = hote.default_output_device()?;
    // raccourci : 48 kHz stéréo demandés tels quels, sans rééchantillonnage si le périphérique
    // refuse ; à ajouter si des sorties ne l'acceptent pas.
    let config = cpal::StreamConfig {
        channels: 2,
        sample_rate: FREQUENCE,
        buffer_size: cpal::BufferSize::Default,
    };
    let flux = peripherique
        .build_output_stream(
            config,
            move |sortie: &mut [f32], _| {
                let mut t = match tampon.lock() {
                    Ok(t) => t,
                    Err(_) => {
                        sortie.fill(0.0);
                        return;
                    }
                };
                for s in sortie.iter_mut() {
                    *s = t.pop_front().unwrap_or(0.0);
                }
            },
            |_| {},
            None,
        )
        .ok()?;
    flux.play().ok()?;
    Some(flux)
}
