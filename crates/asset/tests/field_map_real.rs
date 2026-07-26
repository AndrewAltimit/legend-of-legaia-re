//! Disc-gated regression test for the per-scene field map
//! (`DATA\FIELD\<scene>.MAP`, the `0x12000`-byte slot 0 of every scene block)
//! and for the three sibling classes that closed the rest of `categorize`'s
//! unexplained pile: the runtime `efect.dat` 2-pack, the battle side-band
//! streaming files, and the boot `init.pak`.
//!
//! Skips silently when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.
//!
//! What this catches:
//! - The field-map class drifting off **slot 0 of a scene block**. The class
//!   is *not* the `0x12000` size class: `0x12000` is 36 ordinary sectors and
//!   111 PROT entries land on it, only 101 of which are maps.
//! - The trigger-block sub-table chain regressing - the invariant the detector
//!   rests on (`FUN_801D5AE0`'s `offset`/`count` header slots tiling the block
//!   back-to-back at strides 4/4/4/8).
//! - The all-zero-trigger-header escape hatch widening back out. It admits one
//!   retail entry, and only because that entry's object table and collision
//!   grid are empty too; without the empty-field precondition it also admits
//!   any unrelated `0x12000` entry whose `+0x10000` window reads as zeros.
//! - The field map ceasing to be block slot 0 (extraction `define - 2`), which
//!   is what a resolver that walks forward from a CDNAME label gets wrong.
//! - `monster_sound_bank` reclaiming `summon.dat`: its `[u32 format = 2][u16
//!   spu_addrs[256] all >= 0x8000]` test is satisfied byte-for-byte by that
//!   file's leading `[u32 mode = 2][256-entry CLUT with STP set]`, so the
//!   detector order matters.
//! - The three single-entry classes (`efect_pack`, `init_pak`, and the
//!   `unknown_*` residue) drifting off their entries.

use legaia_asset::categorize::{Class, classify};
use legaia_asset::field_map::{
    self, COLLISION_GRID_BYTES, COLLISION_ROW_STRIDE, FIELD_MAP_BYTES, GRID_DIM, OBJECT_GRID_BYTES,
    OBJECT_GRID_ROW_STRIDE, OBJECT_RECORD_COUNT, OBJECT_RECORDS_BYTES, TRIGGER_BLOCK_BYTES,
    TRIGGER_HEADER_BYTES, TRIGGER_KIND_STRIDES, TRIGGER_SUBTABLE_GAP,
};
use std::path::{Path, PathBuf};

fn extracted_root() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    ["extracted", "../../extracted"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.join("PROT").is_dir())
}

/// Extraction indices of every CDNAME block's slot 0.
///
/// `#define <name> <raw TOC index>`; block content starts at extraction index
/// `raw - 2` (docs/formats/cdname.md, numbering space).
fn block_starts(root: &Path) -> Option<Vec<usize>> {
    let text = std::fs::read_to_string(root.join("CDNAME.TXT")).ok()?;
    let mut starts: Vec<usize> = text
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            (it.next()? == "#define").then_some(())?;
            let _name = it.next()?;
            it.next()?.parse::<usize>().ok()
        })
        .filter_map(|raw| raw.checked_sub(2))
        .collect();
    starts.sort_unstable();
    starts.dedup();
    Some(starts)
}

/// Every `NNNN_*.BIN` under `extracted/PROT`, keyed by extraction index.
fn prot_entries(root: &Path) -> Vec<(usize, PathBuf)> {
    let mut out: Vec<(usize, PathBuf)> = std::fs::read_dir(root.join("PROT"))
        .expect("read PROT dir")
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".BIN") {
                return None;
            }
            let idx = name.get(..4)?.parse::<usize>().ok()?;
            Some((idx, e.path()))
        })
        .collect();
    out.sort_by_key(|(i, _)| *i);
    out
}

#[test]
fn region_map_is_arithmetically_closed() {
    // Not disc-gated: the region sizes must sum to the footprint with nothing
    // left over. This is the load-bearing claim of the format page - if it
    // stops holding, the "four regions" reading is wrong.
    assert_eq!(
        OBJECT_RECORDS_BYTES + COLLISION_GRID_BYTES + OBJECT_GRID_BYTES + TRIGGER_BLOCK_BYTES,
        FIELD_MAP_BYTES
    );
    assert_eq!(COLLISION_ROW_STRIDE * GRID_DIM, COLLISION_GRID_BYTES);
    assert_eq!(OBJECT_GRID_ROW_STRIDE * GRID_DIM, OBJECT_GRID_BYTES);
    assert_eq!(OBJECT_RECORD_COUNT, 512);
}

/// The `0x12000`-byte entries that are **not** field maps.
///
/// `0x12000` is 36 sectors - an ordinary footprint, not a signature. Each of
/// these is a regular member of its scene block that happens to be that long,
/// and each is claimed by its own detector:
///
/// | Entry | Block slot | What it is |
/// |---|---|---|
/// | 63, 71 | dolk+5, dolk2+5 | `scene_tmd_stream` (`[u32 size][0x80000002]`) |
/// | 378, 379 | taiku+9, taiku+10 | `scene_tmd_stream` |
/// | 701 | rugi+7 | `scene_tmd_stream` |
/// | 648 | nilboa2+4 | `data_field_streaming` |
/// | 1074, 1087, 1089, 1187 | vab_01 block | `scene_vab_stream` (`VABp` at +4) |
///
/// 63 / 71 / 701 are the ones that matter: their `+0x10000..+0x10012` window
/// reads as zeros, so an all-zero-trigger-header escape hatch with no
/// empty-field precondition accepts them as field maps.
const SIZED_BUT_NOT_FIELD_MAPS: [usize; 10] = [63, 71, 378, 379, 648, 701, 1074, 1087, 1089, 1187];

/// The one retail field map with a zeroed trigger header: `rikuroa2`'s, a
/// cutscene-only scene with no walkable field.
const ZEROED_TRIGGER_HEADER_ENTRY: usize = 126;

#[test]
fn every_scene_block_slot_zero_field_map_is_recognised() {
    let Some(root) = extracted_root() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let entries = prot_entries(&root);
    assert!(entries.len() > 1000, "expected the full PROT corpus");
    let Some(starts) = block_starts(&root) else {
        eprintln!("[skip] extracted/CDNAME.TXT missing");
        return;
    };
    assert!(starts.len() > 100, "expected the full CDNAME block list");

    let mut sized = Vec::new();
    let mut recognised = Vec::new();
    let mut zeroed_header = Vec::new();
    let mut false_positives = Vec::new();

    for (idx, path) in &entries {
        let buf = std::fs::read(path).expect("read entry");
        let is_sized = buf.len() == FIELD_MAP_BYTES;
        let Some(fm) = field_map::detect(&buf) else {
            if is_sized {
                sized.push(*idx);
            }
            continue;
        };
        if !is_sized {
            false_positives.push(*idx);
            continue;
        }
        sized.push(*idx);
        recognised.push(*idx);
        match fm.trigger_block() {
            Some(block) => {
                // The chain the detector rests on, re-asserted field by field.
                let mut cursor = TRIGGER_HEADER_BYTES;
                for (kind, t) in block.tables.iter().enumerate() {
                    assert_eq!(t.offset, cursor, "entry {idx}: kind {kind} body offset");
                    assert_eq!(t.stride, TRIGGER_KIND_STRIDES[kind]);
                    cursor = t.body_range().end + TRIGGER_SUBTABLE_GAP;
                    assert!(cursor <= TRIGGER_BLOCK_BYTES, "entry {idx}: block overrun");
                    assert_eq!(fm.trigger_records(kind as u8).len(), t.count);
                }
                assert_eq!(block.end_offset, cursor, "entry {idx}: header end offset");
            }
            None => zeroed_header.push(*idx),
        }
        assert_eq!(classify(&buf).class, Class::FieldMap, "entry {idx}");
    }

    assert!(
        false_positives.is_empty(),
        "field_map detector fired outside the 0x12000 size class: {false_positives:?}"
    );

    // Every 0x12000 entry that is slot 0 of a CDNAME block is a field map, and
    // every one that is not, is not. The size class is the superset.
    let expected: Vec<usize> = sized
        .iter()
        .copied()
        .filter(|i| starts.binary_search(i).is_ok())
        .collect();
    assert_eq!(
        recognised, expected,
        "the field-map class must be exactly the block-slot-0 members of the \
         0x12000 size class"
    );
    let rejected: Vec<usize> = sized
        .iter()
        .copied()
        .filter(|i| !recognised.contains(i))
        .collect();
    assert_eq!(
        rejected,
        SIZED_BUT_NOT_FIELD_MAPS.to_vec(),
        "the 0x12000 entries that are not field maps (see the table above)"
    );
    // Disc invariants: 111 entries carry the footprint, 101 carry a map - two
    // of them (`other1`, `other7`) outside the named-scene range.
    assert_eq!(sized.len(), 111, "0x12000-byte entries: {sized:?}");
    assert_eq!(recognised.len(), 101, "field maps: {}", recognised.len());

    // Exactly one retail entry ships the trigger header zeroed - a scene with
    // no walkable field at all. Its object table and collision grid are
    // entirely zero, which is the precondition the detector now enforces.
    assert_eq!(
        zeroed_header,
        vec![ZEROED_TRIGGER_HEADER_ENTRY],
        "zeroed-trigger-header entries"
    );
    let only = zeroed_header[0];
    let buf = std::fs::read(&entries.iter().find(|(i, _)| *i == only).unwrap().1).unwrap();
    let fm = field_map::detect(&buf).unwrap();
    assert_eq!(
        fm.collision_fill(),
        0.0,
        "entry {only}: a zeroed trigger header should come with an empty grid"
    );
    assert!(
        fm.object_records().iter().all(|&b| b == 0),
        "entry {only}: and an empty object table"
    );
}

#[test]
fn field_maps_sit_at_scene_block_slot_zero() {
    let Some(root) = extracted_root() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let Some(starts) = block_starts(&root) else {
        eprintln!("[skip] extracted/CDNAME.TXT missing");
        return;
    };
    assert!(starts.len() > 100, "expected the full CDNAME block list");

    // Both directions. A resolver that walks forward from a CDNAME label to
    // "the first 0x12000 entry at or after it" picks the *next* scene's map;
    // a detector that keys on the footprint alone picks up ten strangers.
    let mut found = 0usize;
    for (idx, path) in prot_entries(&root) {
        let buf = std::fs::read(&path).expect("read entry");
        let is_slot0 = starts.binary_search(&idx).is_ok();
        match field_map::detect(&buf) {
            Some(_) => {
                assert!(
                    is_slot0,
                    "entry {idx} is a field map but not slot 0 of any CDNAME block"
                );
                found += 1;
            }
            None => assert!(
                !(is_slot0 && buf.len() == FIELD_MAP_BYTES),
                "entry {idx} is slot 0 of a CDNAME block with the 0x12000 \
                 footprint but was not recognised"
            ),
        }
    }
    assert_eq!(found, 101, "one field map per field-carrying scene block");
}

#[test]
fn sibling_classes_claim_their_entries() {
    let Some(root) = extracted_root() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let mut seen: Vec<(usize, &'static str)> = Vec::new();
    for (idx, path) in prot_entries(&root) {
        let buf = std::fs::read(&path).expect("read entry");
        let class = classify(&buf).class;
        match class {
            // summon.dat (extraction 893) and readef.DAT (extraction 894).
            Class::SummonReadef => seen.push((idx, "summon_readef")),
            // The runtime efect.dat 2-pack (extraction 873).
            Class::EfectPack => seen.push((idx, "efect_pack")),
            // Boot init.pak (extraction 895).
            Class::InitPak => seen.push((idx, "init_pak")),
            // Must be empty: summon.dat was this detector's only match.
            Class::MonsterSoundBank => seen.push((idx, "monster_sound_bank")),
            // `bse.dat` (extraction 888) and its uncalled sibling (1195).
            // 1195 is also `other1 + 2`, the slot every scene block seats a
            // prescript in, and `[u16 count = 1][u16 offsets[0] = 4]` is
            // byte-identical to `[u16 tag][u16 body_offset = 4]` - see
            // `docs/formats/bse-dat.md`. `bse_bank` runs first and keeps it.
            Class::BseBank => seen.push((idx, "bse_bank")),
            // Every statistical residual bucket must stay empty. Two detector
            // tiers keep it that way and both are load-bearing here:
            // `scene_event_scripts::detect_structural` claims the 23 scene
            // prescripts whose records carry no transform-node sentinel (a
            // frame-opener rate of 0 is a fact about `geremi`'s scripts, not
            // about the format), and `is_overlay_data_image`'s prologue arm
            // claims `0974_other_game.BIN` - a debug-string pool followed by
            // MIPS code, whose ASCII share (12 %) sits under the ratio gate.
            Class::MostlyZeros => seen.push((idx, "residual")),
            Class::UnknownOther => seen.push((idx, "residual")),
            Class::UnknownLowEntropy => seen.push((idx, "residual")),
            Class::UnknownHighEntropy => seen.push((idx, "residual")),
            _ => {}
        }
    }
    let of = |name: &str| -> Vec<usize> {
        seen.iter()
            .filter(|(_, n)| *n == name)
            .map(|(i, _)| *i)
            .collect()
    };
    assert_eq!(of("summon_readef"), vec![893, 894]);
    assert_eq!(of("efect_pack"), vec![873]);
    assert_eq!(of("init_pak"), vec![895]);
    assert_eq!(of("bse_bank"), vec![888, 1195]);
    assert!(
        of("residual").is_empty(),
        "entries fell through to a class named after their byte histogram \
         rather than their format: {:?}",
        of("residual")
    );
    assert!(
        of("monster_sound_bank").is_empty(),
        "monster_sound_bank matched {:?}; its only historical match was \
         summon.dat's leading CLUT, and monster.snd is extraction 891 \
         (FUN_8003E104's `li v0,0x37d`)",
        of("monster_sound_bank")
    );
}
