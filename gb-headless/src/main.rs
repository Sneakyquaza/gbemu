//! Exécuteur sans écran : charge une ROM, exécute, écrit l'écran en PNG et la sortie série.

use gb_core::GameBoy;
use gb_headless::{mooneye_passed, run_until, serial_contains, write_png};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

const AIDE: &str = "usage : gb-headless <rom> [--frames N] [--png sortie.png] \
[--until-serial TEXTE] [--until-ld-b-b] [--bench]";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut rom = None;
    let mut frames = 600u32;
    let mut png_out: Option<PathBuf> = None;
    let mut until_serial: Option<String> = None;
    let mut until_ldbb = false;
    let mut bench = false;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--frames" => match args.next().and_then(|v| v.parse().ok()) {
                Some(n) => frames = n,
                None => return erreur("--frames attend un nombre"),
            },
            "--png" => png_out = args.next().map(PathBuf::from),
            "--until-serial" => until_serial = args.next(),
            "--until-ld-b-b" => until_ldbb = true,
            "--bench" => bench = true,
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

    let t0 = Instant::now();
    let done = run_until(&mut gb, frames, |gb| {
        until_serial
            .as_deref()
            .is_some_and(|m| serial_contains(gb, m))
            || (until_ldbb && gb.hit_ld_b_b())
    });
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
    if let Some(p) = png_out {
        if let Err(e) = write_png(&p, gb.frame()) {
            return erreur(&e);
        }
    }
    ExitCode::SUCCESS
}

fn erreur(msg: &str) -> ExitCode {
    eprintln!("{msg}");
    ExitCode::from(2)
}
