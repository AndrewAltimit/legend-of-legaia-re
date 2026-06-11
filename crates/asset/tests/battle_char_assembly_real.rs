//! Disc-gated test for [`battle_char_assembly`]: assemble each character's
//! battle TMD from the retail player files and pin the shapes the live
//! runtime exhibits (the full-party battle save's assembled Vahn blob:
//! `nobj = 17`, bone tags `[0..14, 200, 201]`, attach bones `[5, 8]` with
//! the Hunter Clothes / Survival Knife / Ra-Seru Meta loadout).
//! Skips silently when `LEGAIA_DISC_BIN` is unset.

use std::path::{Path, PathBuf};

use legaia_asset::{battle_char_assembly, battle_data_pack};

fn extracted_prot_dir() -> Option<PathBuf> {
    let cands = [
        PathBuf::from("extracted/PROT"),
        PathBuf::from("../../extracted/PROT"),
    ];
    cands.into_iter().find(|p| p.is_dir())
}

fn load(prot_dir: &Path, file: &str) -> Option<(Vec<u8>, battle_data_pack::BattleDataPack)> {
    let path = prot_dir.join(file);
    if !path.exists() {
        eprintln!("[skip] {} missing", path.display());
        return None;
    }
    let raw = std::fs::read(&path).expect("read player file");
    let pack = battle_data_pack::parse(&raw).expect("parse pack");
    Some((raw, pack))
}

/// The live full-party save's Vahn loadout (char record `+0x196..+0x19A`):
/// Hunter Clothes body, bare head, Survival Knife, Ra-Seru Meta, bare feet.
const VAHN_SAVE_LOADOUT: [u8; 5] = [0x43, 0x00, 0x22, 0x01, 0x00];

#[test]
fn assembles_vahn_to_the_live_save_shape() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(prot_dir) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let Some((raw, pack)) = load(&prot_dir, "0863_edstati3.BIN") else {
        return;
    };
    let asm = battle_char_assembly::assemble_character(&raw, &pack, &VAHN_SAVE_LOADOUT)
        .expect("assemble Vahn");

    // The shape the live runtime blob exhibits (byte-read from the
    // full-party battle save's DAT_8007C018[0] registration).
    let mut expected_tags: Vec<u8> = (0..=14).collect();
    expected_tags.extend([200, 201]);
    assert_eq!(asm.bone_tags, expected_tags, "post-sort bone tags");
    assert_eq!(asm.attach_bones, vec![5, 8], "equipment attach bones");
    assert_eq!(
        [asm.sections[0].id, asm.sections[2].id, asm.sections[3].id],
        [0x43, 0x22, 0x01],
        "equipped sections selected"
    );
    assert_eq!(asm.sections[1].id, 0, "bare head takes the default");

    // The merged TMD must parse as a Legaia TMD with 17 objects.
    let tmd = legaia_tmd::parse(&asm.tmd).expect("parse assembled TMD");
    assert_eq!(tmd.objects.len(), 17, "assembled nobj");
}

/// Every character assembles with all-default equipment, and the merged
/// TMD parses with `nobj` = skeleton bones + the default sections' extras.
#[test]
fn assembles_all_four_characters_with_default_equipment() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(prot_dir) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    for file in [
        "0863_edstati3.BIN",
        "0864_edstati3.BIN",
        "0865_battle_data.BIN",
        "0866_battle_data.BIN",
    ] {
        let Some((raw, pack)) = load(&prot_dir, file) else {
            return;
        };
        let asm = battle_char_assembly::assemble_character(&raw, &pack, &[0; 5])
            .unwrap_or_else(|e| panic!("{file}: assemble defaults: {e:#}"));
        let tmd = legaia_tmd::parse(&asm.tmd)
            .unwrap_or_else(|e| panic!("{file}: parse assembled TMD: {e:#}"));
        assert_eq!(tmd.objects.len(), asm.bone_tags.len(), "{file}: tag count");
        // Tags are sorted ascending with the skeleton bones first; every
        // 200+ extra has a recorded attach bone.
        assert!(
            asm.bone_tags.windows(2).all(|w| w[0] <= w[1]),
            "{file}: tags sorted"
        );
        let extras = asm.bone_tags.iter().filter(|&&t| t >= 200).count();
        assert_eq!(extras, asm.attach_bones.len(), "{file}: attach per extra");
        eprintln!(
            "{file}: nobj={} extras={} attach={:?}",
            tmd.objects.len(),
            extras,
            asm.attach_bones
        );
    }
}
