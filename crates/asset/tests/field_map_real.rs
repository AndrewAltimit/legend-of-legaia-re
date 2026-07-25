//! Disc-gated regression test for the per-scene field map
//! (`DATA\FIELD\<scene>.MAP`, the `0x12000`-byte slot 0 of every scene block)
//! and for the three sibling classes that closed the rest of `categorize`'s
//! unexplained pile: the runtime `efect.dat` 2-pack, the battle side-band
//! streaming files, and the boot `init.pak`.
//!
//! Skips silently when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.
//!
//! What this catches:
//! - The field-map size class drifting: every `0x12000` entry on the disc must
//!   be recognised, and no other entry may be.
//! - The trigger-block sub-table chain regressing - the invariant the detector
//!   rests on (`FUN_801D5AE0`'s `offset`/`count` header slots tiling the block
//!   back-to-back at strides 4/4/4/8).
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

#[test]
fn every_field_map_sized_entry_is_recognised() {
    let Some(root) = extracted_root() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let entries = prot_entries(&root);
    assert!(entries.len() > 1000, "expected the full PROT corpus");

    let mut sized = 0usize;
    let mut recognised = 0usize;
    let mut zeroed_header = Vec::new();
    let mut false_positives = Vec::new();

    for (idx, path) in &entries {
        let buf = std::fs::read(path).expect("read entry");
        let is_sized = buf.len() == FIELD_MAP_BYTES;
        let detected = field_map::detect(&buf);
        if is_sized {
            sized += 1;
            let fm = detected.unwrap_or_else(|| panic!("entry {idx}: 0x12000 but not recognised"));
            recognised += 1;
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
        } else if detected.is_some() {
            false_positives.push(*idx);
        }
    }

    assert!(
        false_positives.is_empty(),
        "field_map detector fired outside the 0x12000 size class: {false_positives:?}"
    );
    assert_eq!(sized, recognised, "every 0x12000 entry must be recognised");
    assert!(
        sized > 90,
        "expected one field map per scene block, found {sized}"
    );
    // Exactly one retail entry ships the trigger header zeroed (a scene with no
    // walkable field at all - its object table and both grids are zero too).
    assert_eq!(
        zeroed_header.len(),
        1,
        "zeroed-trigger-header entries: {zeroed_header:?}"
    );
    let only = zeroed_header[0];
    let buf = std::fs::read(&entries.iter().find(|(i, _)| *i == only).unwrap().1).unwrap();
    let fm = field_map::detect(&buf).unwrap();
    assert_eq!(
        fm.collision_fill(),
        0.0,
        "entry {only}: a zeroed trigger header should come with an empty grid"
    );
}

#[test]
fn field_maps_sit_at_scene_block_slot_zero() {
    let Some(root) = extracted_root() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let cdname = root.join("CDNAME.TXT");
    let Ok(text) = std::fs::read_to_string(&cdname) else {
        eprintln!("[skip] extracted/CDNAME.TXT missing");
        return;
    };
    // `#define <name> <raw TOC index>`; block content starts at extraction
    // index `raw - 2` (docs/formats/cdname.md, numbering space).
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
    assert!(starts.len() > 100, "expected the full CDNAME block list");

    for (idx, path) in prot_entries(&root) {
        let buf = std::fs::read(&path).expect("read entry");
        if buf.len() != FIELD_MAP_BYTES {
            continue;
        }
        assert!(
            starts.binary_search(&idx).is_ok(),
            "entry {idx} is a field map but not slot 0 of any CDNAME block"
        );
    }
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
            Class::BseBank => seen.push((idx, "bse_bank")),
            // Every statistical residual bucket must stay empty.
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
