//! Exécuteur GBA sans écran : charge une ROM, exécute N images, écrit l'écran en PNG, mesure.

use gb_headless::write_wav;
use gba_core::Gba;
use gba_headless::{touche_nommee, write_png_rgb};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

const AIDE: &str = "usage : gba-headless <rom> [--frames N] [--png sortie.png] [--bench] \
[--bios bios.bin] [--appui TOUCHE@IMAGE]… [--regs] [--wav sortie.wav]
  --appui : touche (A, B, SELECT, START, DROITE, GAUCHE, HAUT, BAS, R, L) tenue 6 images
  --regs  : affiche r0-r15 et CPSR à la fin";

/// Fréquence du WAV écrit par `--wav`.
const FREQ_WAV: u32 = 48_000;

/// Durée d'un appui `--appui`, en images.
const DUREE_APPUI: u32 = 6;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut rom = None;
    let mut frames = 600u32;
    let mut png_out: Option<PathBuf> = None;
    let mut wav_out: Option<PathBuf> = None;
    let mut bios: Option<PathBuf> = None;
    let mut bench = false;
    let mut regs = false;
    let mut appuis: Vec<(u16, u32)> = Vec::new();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--frames" => match args.next().and_then(|v| v.parse().ok()) {
                Some(n) => frames = n,
                None => return erreur("--frames attend un nombre"),
            },
            "--png" => png_out = args.next().map(PathBuf::from),
            "--wav" => wav_out = args.next().map(PathBuf::from),
            "--bios" => bios = args.next().map(PathBuf::from),
            "--bench" => bench = true,
            "--regs" => regs = true,
            "--appui" => {
                let v = args.next().unwrap_or_default();
                let lu = v
                    .split_once('@')
                    .and_then(|(t, i)| Some((touche_nommee(t)?, i.parse().ok()?)));
                match lu {
                    Some(x) => appuis.push(x),
                    None => return erreur("--appui attend TOUCHE@IMAGE (ex. START@60)"),
                }
            }
            "-h" | "--help" => {
                println!("{AIDE}");
                return ExitCode::SUCCESS;
            }
            _ if rom.is_none() => rom = Some(PathBuf::from(a)),
            _ => return erreur(&format!("argument inconnu : {a}")),
        }
    }
    let Some(rom) = rom else {
        return erreur(AIDE);
    };
    let data = match std::fs::read(&rom) {
        Ok(d) => d,
        Err(e) => return erreur(&format!("{} : {e}", rom.display())),
    };
    let bios = match &bios {
        Some(p) => match std::fs::read(p) {
            Ok(d) => Some(d),
            Err(e) => return erreur(&format!("{} : {e}", p.display())),
        },
        None => None,
    };
    let mut gba = match Gba::new(&data, bios.as_deref()) {
        Ok(g) => g,
        Err(e) => return erreur(&format!("{} : {e}", rom.display())),
    };

    gba.set_sample_rate(FREQ_WAV);
    let mut son = Vec::new();
    let t0 = Instant::now();
    for i in 0..frames {
        let t = appuis
            .iter()
            .filter(|&&(_, d)| (d..d + DUREE_APPUI).contains(&i))
            .fold(0, |acc, &(t, _)| acc | t);
        gba.set_keys(t);
        gba.run_frame();
        if wav_out.is_some() {
            gba.drain_samples(&mut son);
        }
    }
    let dt = t0.elapsed().as_secs_f64();

    if bench {
        println!(
            "{frames} images en {dt:.3} s : {:.0} images/s",
            frames as f64 / dt
        );
    }
    if regs {
        let r = gba.registres();
        for (i, v) in r.iter().enumerate() {
            print!("r{i}={v:08X} ");
        }
        println!("cpsr={:08X} boucle={}", gba.cpsr(), gba.en_boucle_infinie());
    }
    if let Some(p) = wav_out {
        if let Err(e) = write_wav(&p, &son, FREQ_WAV) {
            return erreur(&e);
        }
    }
    if let Some(p) = png_out {
        if let Err(e) = write_png_rgb(&p, gba.frame()) {
            return erreur(&e);
        }
    }
    ExitCode::SUCCESS
}

fn erreur(msg: &str) -> ExitCode {
    eprintln!("{msg}");
    ExitCode::from(2)
}
