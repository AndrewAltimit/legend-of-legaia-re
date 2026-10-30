//! `asset expansion ...` over the committed flagship pack: validate it,
//! bundle it, unbundle the bundle, and get the same tree back; and refuse a
//! pack that carries a disc-format asset. Disc-free.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use legaia_expansion_format::read_dir_files;

fn asset(args: &[&Path]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_asset"))
        .arg("expansion")
        .args(args)
        .output()
        .expect("spawn the asset CLI")
}

fn rebirth() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/rebirth")
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("expansion_cli_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn ok(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn bundle_unbundle_round_trips_rebirth() {
    let dir = scratch("roundtrip");
    let lxpack = dir.join("rebirth.lxpack");
    let copy = dir.join("copy");
    // Not `--strict`: Rebirth still carries flow-lint warnings
    // (`legaia_expansion_format::flow`) its content owes.
    let text = ok(&asset(&[Path::new("validate"), &rebirth()]));
    assert!(text.contains("0 error(s)"), "{text}");
    ok(&asset(&[
        Path::new("bundle"),
        &rebirth(),
        Path::new("-o"),
        &lxpack,
    ]));
    ok(&asset(&[
        Path::new("unbundle"),
        &lxpack,
        Path::new("-o"),
        &copy,
    ]));
    assert_eq!(
        read_dir_files(&copy).unwrap(),
        read_dir_files(&rebirth()).unwrap()
    );
    let text = ok(&asset(&[Path::new("info"), &lxpack]));
    assert!(
        text.contains("Legend of Legaia: Rebirth 0.1.0 (rebirth)"),
        "{text}"
    );
    ok(&asset(&[Path::new("validate"), &lxpack]));
    ok(&asset(&[Path::new("info")]));
    // Unbundling onto a non-empty directory is refused.
    assert!(
        !asset(&[Path::new("unbundle"), &lxpack, Path::new("-o"), &copy])
            .status
            .success()
    );
}

#[test]
fn a_disc_format_asset_fails_validate_and_bundle() {
    let dir = scratch("reject");
    let pack = dir.join("pack");
    std::fs::create_dir_all(pack.join("assets")).unwrap();
    std::fs::write(
        pack.join("pack.toml"),
        "id = \"bad\"\nname = \"Bad\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let mut tim = vec![0x10, 0, 0, 0, 0x08, 0, 0, 0];
    tim.resize(64, 0);
    std::fs::write(pack.join("assets/hero.png"), tim).unwrap();
    let out = asset(&[Path::new("validate"), &pack]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("PSX TIM"));
    let lxpack = dir.join("bad.lxpack");
    assert!(
        !asset(&[Path::new("bundle"), &pack, Path::new("-o"), &lxpack])
            .status
            .success()
    );
    assert!(!lxpack.exists());
}
