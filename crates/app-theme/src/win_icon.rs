//! Embedding an icon into a Windows executable, called from a build script:
//!
//! ```ignore
//! fn main() {
//!     legaia_app_theme::win_icon::embed_in_bins(legaia_app_theme::icons::GAME_ICO);
//! }
//! ```
//!
//! For a Windows target it writes a one-line resource script (`1 ICON
//! "<ico>"`), compiles it with the resource compiler the toolchain carries
//! and links the result into every binary of the package; File Explorer, the
//! taskbar and the window's title bar all show that icon. For any other
//! target it does nothing, so Linux, macOS and wasm builds are unchanged.
//!
//! The compiler is `$LEGAIA_WINDRES` when set, else for a GNU target
//! `<arch>-w64-mingw32-windres` (the mingw-w64 cross toolchain the release
//! builds on Linux with), `windres` or `llvm-windres`, and for an MSVC target
//! `rc.exe` from the developer prompt's `PATH`. With none found the build
//! goes on without an icon and says so in a cargo warning; the release
//! script checks every shipped Windows executable for the icon resource, so a
//! release cannot ship without it.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::string::{String, ToString};
use std::vec::Vec;
use std::{format, println, vec};

/// The resource script naming `ico` as icon 1, the one Windows shows for the
/// executable. Backslashes become forward slashes, which both `windres` and
/// `rc.exe` accept and neither reads as an escape.
pub fn rc_source(ico: &str) -> String {
    format!("1 ICON \"{}\"\n", ico.replace('\\', "/"))
}

/// The resource compilers to try for a GNU target, in order.
pub fn windres_candidates(target_arch: &str) -> Vec<String> {
    let prefix = match target_arch {
        "x86" => "i686",
        a => a,
    };
    vec![
        format!("{prefix}-w64-mingw32-windres"),
        "windres".to_string(),
        "llvm-windres".to_string(),
    ]
}

/// Compile `ico` into a resource and link it into every binary of the
/// package whose build script calls this, when the target is Windows.
pub fn embed_in_bins(ico: &str) {
    println!("cargo:rerun-if-changed={ico}");
    println!("cargo:rerun-if-env-changed=LEGAIA_WINDRES");
    let var = |k: &str| std::env::var(k).unwrap_or_default();
    if var("CARGO_CFG_TARGET_OS") != "windows" {
        return;
    }
    let out = PathBuf::from(var("OUT_DIR"));
    let rc = out.join("app-icon.rc");
    if let Err(e) = std::fs::write(&rc, rc_source(ico)) {
        println!(
            "cargo:warning=no executable icon: writing {}: {e}",
            rc.display()
        );
        return;
    }
    let compiled = if var("CARGO_CFG_TARGET_ENV") == "msvc" {
        compile_msvc(&rc, &out.join("app-icon.res"))
    } else {
        compile_gnu(&rc, &out.join("app-icon.o"), &var("CARGO_CFG_TARGET_ARCH"))
    };
    match compiled {
        Ok(obj) => println!("cargo:rustc-link-arg-bins={}", obj.display()),
        Err(e) => println!("cargo:warning=no executable icon: {e}"),
    }
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let program = cmd.get_program().to_string_lossy().into_owned();
    match cmd.output() {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(format!(
            "{program} failed ({}): {}",
            o.status,
            String::from_utf8_lossy(&o.stderr).trim()
        )),
        Err(e) => Err(format!("{program}: {e}")),
    }
}

fn configured_compiler() -> Option<String> {
    std::env::var("LEGAIA_WINDRES")
        .ok()
        .filter(|w| !w.is_empty())
}

fn compile_gnu(rc: &Path, obj: &Path, arch: &str) -> Result<PathBuf, String> {
    let candidates = match configured_compiler() {
        Some(w) => vec![w],
        None => windres_candidates(arch),
    };
    let mut errors = Vec::new();
    for windres in &candidates {
        let r = run(Command::new(windres)
            .arg("--input")
            .arg(rc)
            .arg("--output-format=coff")
            .arg("--output")
            .arg(obj));
        match r {
            Ok(()) => return Ok(obj.to_path_buf()),
            Err(e) => errors.push(e),
        }
    }
    Err(errors.join("; "))
}

fn compile_msvc(rc: &Path, res: &Path) -> Result<PathBuf, String> {
    let rc_exe = configured_compiler().unwrap_or_else(|| "rc".to_string());
    run(Command::new(rc_exe)
        .arg("/nologo")
        .arg("/fo")
        .arg(res)
        .arg(rc))?;
    Ok(res.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rc_source_names_icon_one_with_forward_slashes() {
        assert_eq!(
            rc_source(r"C:\repo\crates\app-theme/icons/x.ico"),
            "1 ICON \"C:/repo/crates/app-theme/icons/x.ico\"\n"
        );
    }

    #[test]
    fn windres_prefers_the_mingw_cross_tool() {
        assert_eq!(
            windres_candidates("x86_64")[0],
            "x86_64-w64-mingw32-windres"
        );
        assert_eq!(windres_candidates("x86")[0], "i686-w64-mingw32-windres");
    }
}
