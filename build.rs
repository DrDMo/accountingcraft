// Puts the abacus logo into AccountingCraft.exe so Explorer, the taskbar and shortcuts show it,
// and finds Inter (the UI and PDF font) in a craft-fonts checkout (CRAFT_FONTS_DIR), if there is one.
#[path = "src/icon.rs"]
mod icon;

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=src/icon.rs");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default());
    pdf_fonts(&out);
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        windows_icon(&out);
    }
}

fn windows_icon(out: &Path) {
    let ico = out.join("app.ico");
    if let Err(e) = std::fs::write(&ico, icon::ico(&[16, 24, 32, 48, 64, 128, 256])) {
        println!("cargo::error=writing {}: {e}", ico.display());
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon(&ico.to_string_lossy());
    res.set("ProductName", "AccountingCraft");
    res.set("FileDescription", "AccountingCraft");
    if let Err(e) = res.compile() {
        println!("cargo::error=embedding the icon (needs the Windows SDK's rc.exe): {e}");
    }
}

/// Writes `pdf_fonts.rs` defining INTER: Some((regular, semibold)) or None. Fonts live in storytold/craft-fonts, not in
/// this repo (Craft house rule); releases set CRAFT_FONTS_REQUIRED so they can't ship without them.
fn pdf_fonts(out: &Path) {
    println!("cargo::rerun-if-env-changed=CRAFT_FONTS_DIR");
    println!("cargo::rerun-if-env-changed=CRAFT_FONTS_REQUIRED");
    let required = std::env::var_os("CRAFT_FONTS_REQUIRED").is_some();
    let src = match std::env::var_os("CRAFT_FONTS_DIR").map(PathBuf::from).map(|d| inter(&d)) {
        Some(Ok((reg, bold))) => format!("pub const INTER: Option<(&[u8], &[u8])> = Some((include_bytes!({reg:?}), include_bytes!({bold:?})));\n"),
        found => {
            match found {
                Some(Err(e)) if required => println!("cargo::error=craft-fonts: {e}"),
                Some(Err(e)) => println!("cargo::warning=building without Inter: craft-fonts: {e}"),
                _ if required => println!("cargo::error=CRAFT_FONTS_REQUIRED is set but CRAFT_FONTS_DIR is missing"),
                _ => {}
            }
            "pub const INTER: Option<(&[u8], &[u8])> = None;\n".into()
        }
    };
    let path = out.join("pdf_fonts.rs");
    if let Err(e) = std::fs::write(&path, src) {
        println!("cargo::error=writing {}: {e}", path.display());
    }
}

/// Inter Regular and SemiBold from a craft-fonts checkout's Latin manifest.
fn inter(dir: &Path) -> Result<(String, String), String> {
    let manifest = dir.join("fonts/latin-manifest.txt");
    println!("cargo::rerun-if-changed={}", manifest.display());
    let text = std::fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let find = |style: &str| -> Result<String, String> {
        let file = text
            .lines()
            .map(|l| l.split(" | ").map(str::trim).collect::<Vec<_>>())
            .find(|f| f.first() == Some(&"Inter") && f.get(1) == Some(&style))
            .and_then(|f| f.get(2).map(|s| s.to_string()))
            .ok_or(format!("no Inter {style} in {}", manifest.display()))?;
        let path = dir.join(&file).canonicalize().map_err(|e| format!("{file}: {e}"))?;
        println!("cargo::rerun-if-changed={}", path.display());
        Ok(path.to_string_lossy().into_owned())
    };
    Ok((find("Regular")?, find("SemiBold")?))
}
