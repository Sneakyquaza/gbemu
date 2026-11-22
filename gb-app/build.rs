//! Ressources de l'exe Windows (icône, version) compilées avec `windres` de mingw, sans
//! dépendance. Hors Windows : rien. Sans `windres` : exe sans icône, avec un avertissement.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/icone.ico");
    println!("cargo:rerun-if-changed=build.rs");
    let cible = |v: &str| std::env::var(v).unwrap_or_default();
    if cible("CARGO_CFG_TARGET_OS") != "windows" {
        return;
    }
    if cible("CARGO_CFG_TARGET_ENV") != "gnu" {
        // raccourci : seule la cible mingw est construite ; ajouter `rc.exe` si MSVC sert un jour.
        println!("cargo:warning=ressources Windows ignorées hors cible gnu");
        return;
    }
    let sortie = PathBuf::from(cible("OUT_DIR"));
    let icone = PathBuf::from(cible("CARGO_MANIFEST_DIR")).join("assets/icone.ico");
    let version = cible("CARGO_PKG_VERSION");
    let mut n: Vec<u32> = version.split('.').filter_map(|x| x.parse().ok()).collect();
    n.resize(4, 0);
    let virgules = format!("{},{},{},{}", n[0], n[1], n[2], n[3]);
    let rc = format!(
        r#"1 ICON "{icone}"
1 VERSIONINFO
FILEVERSION {virgules}
PRODUCTVERSION {virgules}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040C04B0"
    BEGIN
      VALUE "FileDescription", "gbemu"
      VALUE "ProductName", "gbemu"
      VALUE "FileVersion", "{version}"
      VALUE "ProductVersion", "{version}"
      VALUE "OriginalFilename", "gbemu.exe"
      VALUE "LegalCopyright", "Licence MIT"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x040C, 1200
  END
END
"#,
        icone = icone.display().to_string().replace('\\', "/"),
    );
    let fichier_rc = sortie.join("gbemu.rc");
    let objet = sortie.join("gbemu-ressources.o");
    if std::fs::write(&fichier_rc, rc).is_err() {
        println!("cargo:warning=gbemu.rc non écrit : exe sans icône");
        return;
    }
    let windres = if cible("HOST").contains("windows") {
        "windres".to_string()
    } else {
        format!("{}-w64-mingw32-windres", cible("CARGO_CFG_TARGET_ARCH"))
    };
    let ok = Command::new(&windres)
        .arg(&fichier_rc)
        .args(["-O", "coff", "-o"])
        .arg(&objet)
        .status()
        .is_ok_and(|s| s.success());
    if ok {
        println!("cargo:rustc-link-arg-bins={}", objet.display());
    } else {
        println!("cargo:warning={windres} indisponible : exe sans icône");
    }
}
