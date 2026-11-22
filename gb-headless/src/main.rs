//! Exécuteur sans écran : charge une ROM, exécute, écrit l'écran en PNG et la sortie série.

use gb_core::GameBoy;
use gb_headless::{mooneye_passed, serial_contains, write_png, write_png_rgb, write_wav};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

const AIDE: &str = "usage : gb-headless <rom> [--frames N] [--png sortie.png] \
[--until-serial TEXTE] [--until-ld-b-b] [--wav sortie.wav] [--bench] \
[--load-state état] [--save-state état] [--correction]";

/// Fréquence du WAV écrit par `--wav`.
const FREQ_WAV: u32 = 48_000;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut rom = None;
    let mut frames = 600u32;
    let mut png_out: Option<PathBuf> = None;
    let mut wav_out: Option<PathBuf> = None;
    let mut until_serial: Option<String> = None;
    let mut until_ldbb = false;
    let mut bench = false;
    let mut load_state: Option<PathBuf> = None;
    let mut save_state: Option<PathBuf> = None;
    let mut correction = false;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--frames" => match args.next().and_then(|v| v.parse().ok()) {
                Some(n) => frames = n,
                None => return erreur("--frames attend un nombre"),
            },
            "--png" => png_out = args.next().map(PathBuf::from),
            "--wav" => wav_out = args.next().map(PathBuf::from),
            "--until-serial" => until_serial = args.next(),
            "--until-ld-b-b" => until_ldbb = true,
            "--bench" => bench = true,
            "--load-state" => load_state = args.next().map(PathBuf::from),
            "--save-state" => save_state = args.next().map(PathBuf::from),
            "--correction" => correction = true,
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
    let mut gb = match GameBoy::new(&data) {
        Ok(gb) => gb,
        Err(e) => return erreur(&format!("{} : {e}", rom.display())),
    };
    if let Some(p) = &load_state {
        let etat = match std::fs::read(p) {
            Ok(d) => d,
            Err(e) => return erreur(&format!("{} : {e}", p.display())),
        };
        if let Err(e) = gb.load_state(&etat) {
            return erreur(&format!("{} : {e}", p.display()));
        }
    }

    gb.set_sample_rate(FREQ_WAV);
    gb.set_color_correction(correction);
    let mut son = Vec::new();
    let t0 = Instant::now();
    let stop = |gb: &GameBoy| {
        until_serial
            .as_deref()
            .is_some_and(|m| serial_contains(gb, m))
            || (until_ldbb && gb.hit_ld_b_b())
    };
    let mut done = 0;
    while done < frames && !stop(&gb) {
        gb.run_frame();
        if wav_out.is_some() {
            gb.drain_samples(&mut son);
        }
        done += 1;
    }
    let dt = t0.elapsed().as_secs_f64();

    let serial = String::from_utf8_lossy(gb.serial_output());
    if !serial.is_empty() {
        println!("{serial}");
    }
    if until_ldbb {
        let r = gb.registres();
        println!(
            "LD B,B : {}  B={} C={} D={} E={} H={} L={} -> {}",
            gb.hit_ld_b_b(),
            r.b,
            r.c,
            r.d,
            r.e,
            r.h,
            r.l,
            if mooneye_passed(&gb) {
                "RÉUSSI"
            } else {
                "ÉCHEC"
            }
        );
    }
    if bench {
        println!(
            "{done} images en {dt:.3} s : {:.0} images/s",
            done as f64 / dt
        );
    }
    if let Some(p) = wav_out {
        if let Err(e) = write_wav(&p, &son, FREQ_WAV) {
            return erreur(&e);
        }
    }
    if let Some(p) = save_state {
        if let Err(e) = std::fs::write(&p, gb.save_state()) {
            return erreur(&format!("{} : {e}", p.display()));
        }
    }
    if let Some(p) = png_out {
        // PNG couleur en mode CGB, niveaux de gris en DMG.
        let r = if gb.is_cgb() {
            write_png_rgb(&p, gb.frame_rgb())
        } else {
            write_png(&p, gb.frame())
        };
        if let Err(e) = r {
            return erreur(&e);
        }
    }
    ExitCode::SUCCESS
}

fn erreur(msg: &str) -> ExitCode {
    eprintln!("{msg}");
    ExitCode::from(2)
}
