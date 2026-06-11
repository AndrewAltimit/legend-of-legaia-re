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

/// The registration-time TSB/CBA relocation moves each party character's
/// assembled mesh into the documented runtime VRAM band
/// (`docs/formats/character-mesh.md` § Battle render): after
/// `relocate_tsb_cba(slot)` every textured prim's CLUT row is exactly
/// `481 + slot` and its texpage is one of the slot's two runtime pages
/// (Vahn `(512,256)+(576,256)`, Noa `(640,256)+(704,256)`,
/// Gala `(768,256)+(832,256)`), with the CLUT column preserved from the
/// authoring layout.
#[test]
fn relocates_each_character_into_its_runtime_band() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(prot_dir) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let files = [
        ("0863_edstati3.BIN", 0usize),
        ("0864_edstati3.BIN", 1),
        ("0865_battle_data.BIN", 2),
    ];
    for (file, slot) in files {
        let Some((raw, pack)) = load(&prot_dir, file) else {
            return;
        };
        let mut asm = battle_char_assembly::assemble_character(&raw, &pack, &[0; 5])
            .unwrap_or_else(|e| panic!("{file}: assemble defaults: {e:#}"));

        // Authoring layout: all textured prims sample pages 0x15/0x16 and
        // CLUT row 480, with per-prim columns.
        let authoring_cols = textured_prim_addresses(&asm.tmd)
            .into_iter()
            .map(|(cba, tsb)| {
                assert_eq!((cba >> 6) & 0x1FF, 480, "{file}: authoring CLUT row");
                assert!(
                    matches!(tsb & 0x1F, 0x15 | 0x16),
                    "{file}: authoring texpage {:#x}",
                    tsb & 0x1F
                );
                cba & 0x3F
            })
            .collect::<Vec<_>>();

        let n = battle_char_assembly::relocate_tsb_cba(&mut asm.tmd, slot as u8)
            .unwrap_or_else(|e| panic!("{file}: relocate: {e:#}"));
        assert!(n > 0, "{file}: no textured prims relocated");

        // The slot's two runtime texpage indices; both sit on the y = 256
        // page row (bit 4 set), decoded x = 512 + 64 * (page - 0x18).
        let runtime_pages = [0x18 + 2 * slot as u16, 0x19 + 2 * slot as u16];
        let relocated = textured_prim_addresses(&asm.tmd);
        assert_eq!(relocated.len(), authoring_cols.len(), "{file}: prim count");
        for ((cba, tsb), authoring_col) in relocated.into_iter().zip(authoring_cols) {
            assert_eq!(
                (cba >> 6) & 0x1FF,
                481 + slot as u16,
                "{file}: runtime CLUT row"
            );
            assert_eq!(cba & 0x3F, authoring_col, "{file}: CLUT column preserved");
            let page = tsb & 0x1F;
            assert!(
                runtime_pages.contains(&page),
                "{file}: runtime texpage {page:#x} outside the slot band"
            );
            assert_eq!((tsb >> 4) & 1, 1, "{file}: runtime page y");
        }
        eprintln!("{file}: relocated {n} textured prims into slot {slot}'s band");
    }
}

/// Collect `(cba, tsb)` of every textured prim in a relative-offset Legaia
/// TMD, in walk order.
fn textured_prim_addresses(tmd_bytes: &[u8]) -> Vec<(u16, u16)> {
    let tmd = legaia_tmd::parse(tmd_bytes).expect("parse TMD");
    let mut out = Vec::new();
    for obj in &tmd.objects {
        let groups = legaia_tmd::legaia_prims::iter_groups(
            tmd_bytes,
            obj.primitives_byte_offset,
            obj.primitives_byte_size,
        )
        .expect("walk groups");
        for group in groups {
            if group.header.mode & 0x04 == 0 {
                continue;
            }
            for prim in &group.prims {
                out.push((prim.cba, prim.tsb));
            }
        }
    }
    out
}

/// Every player file carries the character's own battle animations in
/// record[0] (the retail pose source for the assembled mesh - see
/// `docs/formats/battle-data-pack.md` § Battle animations), and the
/// assembler's per-object channel map (`anm_bones`) is consistent with the
/// stream: the idle's `parts` equals the skeleton bone count, skeleton
/// objects ride their own bone channel, and every `200+` equipment extra
/// rides its recorded attach bone - which is what makes the duplicate
/// equipment pieces land exactly on their attach piece instead of
/// rendering as a second floating copy.
#[test]
fn idle_stream_drives_every_assembled_object() {
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
        let anims = battle_char_assembly::battle_animations(&raw)
            .unwrap_or_else(|e| panic!("{file}: battle animations: {e:#}"));
        assert!(!anims.is_empty(), "{file}: no battle animations decoded");
        let idle = battle_char_assembly::idle_battle_animation(&raw)
            .unwrap_or_else(|e| panic!("{file}: idle decode: {e:#}"))
            .unwrap_or_else(|| panic!("{file}: no idle stream"));
        assert!(idle.frame_count >= 2, "{file}: idle is not a clip");

        let asm = battle_char_assembly::assemble_character(&raw, &pack, &[0; 5])
            .unwrap_or_else(|e| panic!("{file}: assemble defaults: {e:#}"));
        let skeleton = asm.bone_tags.iter().filter(|&&t| t < 100).count();
        assert_eq!(
            idle.part_count, skeleton,
            "{file}: idle parts == skeleton bone count"
        );
        assert_eq!(asm.anm_bones.len(), asm.bone_tags.len(), "{file}: map len");
        for (i, (&tag, &ch)) in asm.bone_tags.iter().zip(&asm.anm_bones).enumerate() {
            assert!(
                (ch as usize) < idle.part_count,
                "{file}: object {i} channel {ch} out of the {} parts",
                idle.part_count
            );
            if tag < 100 {
                assert_eq!(ch, tag, "{file}: skeleton object {i} rides its own bone");
            } else if tag >= 200 {
                assert_eq!(
                    ch,
                    asm.attach_bones[(tag - 200) as usize],
                    "{file}: extra {i} rides its attach bone"
                );
            }
        }
        // Expansion: channel i drives object i, extras duplicating their
        // attach channel.
        let expanded = battle_char_assembly::expand_animation_for_objects(&idle, &asm.anm_bones);
        assert_eq!(expanded.part_count, asm.bone_tags.len());
        assert_eq!(expanded.frame_count, idle.frame_count);
        for (i, &ch) in asm.anm_bones.iter().enumerate() {
            assert_eq!(
                expanded.frames[0][i], idle.frames[0][ch as usize],
                "{file}: expanded channel {i}"
            );
        }
        eprintln!(
            "{file}: {} actions, idle parts={} frames={}, nobj={}",
            anims.len(),
            idle.part_count,
            idle.frame_count,
            asm.bone_tags.len()
        );
    }
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
