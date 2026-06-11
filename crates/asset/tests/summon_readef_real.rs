//! Disc-gated regression test for the battle side-band streaming files
//! `\data\battle\summon.dat` (extraction PROT 893) and
//! `\data\battle\readef.DAT` (extraction PROT 894).
//!
//! Skips silently when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.
//!
//! What this catches:
//! - The retail-TOC-index ↔ extraction-entry resolution drifting: the raw
//!   PROT.DAT TOC word at `(0x37F + 2)` / `(0x380 + 2)` must equal the
//!   extraction entries' start LBAs (the `FUN_8003E8A8` arithmetic over the
//!   header-included in-RAM TOC copy).
//! - The `0x10800` slot framing regressing (exact 103 / 78 slot counts).
//! - The slot classification (texture-slot mode header, actor-record TMD
//!   magic) or the action-id → base-slot formula drifting from the
//!   `FUN_801E295C` case-`0x32` banding.

use legaia_asset::summon_readef::{
    self, READEF_PROT_INDEX, READEF_RETAIL_TOC_INDEX, READEF_SLOT_COUNT, SLOT_BYTES,
    SUMMON_PROT_INDEX, SUMMON_RETAIL_TOC_INDEX, SUMMON_SLOT_COUNT, SlotKind, StreamFile,
    stream_target,
};
use std::path::{Path, PathBuf};

fn extracted_root() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    ["extracted", "../../extracted"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.join("PROT").is_dir())
}

fn entry_bytes(root: &Path, prefix: &str) -> Option<Vec<u8>> {
    let dir = root.join("PROT");
    let name = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|n| n.starts_with(prefix))?;
    std::fs::read(dir.join(name)).ok()
}

#[test]
fn retail_toc_index_resolution_and_slot_framing() {
    let Some(root) = extracted_root() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let Ok(toc_head) = std::fs::read(root.join("PROT.DAT")).map(|d| d[..0x1800].to_vec()) else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    let word =
        |i: usize| u32::from_le_bytes(toc_head[i * 4..i * 4 + 4].try_into().expect("toc word"));

    let summon = entry_bytes(&root, &format!("{SUMMON_PROT_INDEX:04}_")).expect("entry 893");
    let readef = entry_bytes(&root, &format!("{READEF_PROT_INDEX:04}_")).expect("entry 894");

    // FUN_8003E8A8: start = raw_toc_word[idx + 2]; size = word[idx+3] - word[idx+2].
    // The retail indices 0x37F / 0x380 must land on the extraction entries'
    // exact footprints (extraction index = retail index - 2).
    for (label, retail_idx, bytes, slot_count) in [
        (
            "summon.dat",
            SUMMON_RETAIL_TOC_INDEX,
            &summon,
            SUMMON_SLOT_COUNT,
        ),
        (
            "readef.DAT",
            READEF_RETAIL_TOC_INDEX,
            &readef,
            READEF_SLOT_COUNT,
        ),
    ] {
        let idx = retail_idx as usize;
        let footprint_sectors = word(idx + 3) - word(idx + 2);
        assert_eq!(
            footprint_sectors as usize * 0x800,
            bytes.len(),
            "{label}: retail TOC footprint vs extraction entry size"
        );
        assert_eq!(
            bytes.len(),
            slot_count * SLOT_BYTES,
            "{label}: exact 0x10800-slot multiple"
        );
    }

    let summon = summon_readef::parse(&summon).expect("parse summon.dat");
    let readef = summon_readef::parse(&readef).expect("parse readef.DAT");

    // Every group's base slot must be a texture slot in the right file.
    // readef ids 1..=26, summon 3-slot ids 0x81..=0x99, big-summon 0x9A..=0xA0.
    let mut actor_records = 0usize;
    for id in (0x01..=0x1A).chain(0x81..=0xA0u8) {
        let (file, slot) = stream_target(id);
        let f = match file {
            StreamFile::Summon => {
                assert!(id >= 0x81, "id {id:#x} must stream from summon.dat");
                &summon
            }
            StreamFile::Readef => {
                assert!(id < 0x81, "id {id:#x} must stream from readef.DAT");
                &readef
            }
        };
        let base = &f.slots[slot as usize];
        assert!(
            matches!(base.kind, SlotKind::Texture(_)),
            "id {id:#x}: base slot {slot} must be a texture slot"
        );
        // Big summons (base >= 0xCB): 2nd slot is also a texture page, the
        // 4th is the actor record FUN_801F19EC installs.
        if id >= 0x9A {
            assert!(
                matches!(f.slots[slot as usize + 1].kind, SlotKind::Texture(_)),
                "big summon id {id:#x}: second texture slot"
            );
            let rec = &f.slots[slot as usize + 3];
            assert!(
                matches!(rec.kind, SlotKind::ActorRecord(_)),
                "big summon id {id:#x}: 4th slot must be an actor record"
            );
        }
    }

    // Actor records (TMD-magic gated) appear across both files; every one
    // carries a sane part table and an in-slot texture pool after the TMD.
    for f in [&summon, &readef] {
        for slot in &f.slots {
            if let SlotKind::ActorRecord(rec) = &slot.kind {
                actor_records += 1;
                assert!(rec.tmd_offset > rec.name_offset, "slot {}", slot.index);
                assert!(
                    rec.texture_pool_offset > rec.tmd_offset,
                    "slot {}",
                    slot.index
                );
                assert!(
                    rec.part_count > 0 && rec.part_count <= 16,
                    "slot {}",
                    slot.index
                );
            }
        }
    }
    assert!(
        actor_records >= 30,
        "expected a healthy actor-record population, got {actor_records}"
    );

    // Group 0 of summon.dat is spell id 0x81 (Gimard); its actor record's
    // name string starts with the known player-summon attack name prefix.
    let SlotKind::ActorRecord(gimard) = &summon.slots[2].kind else {
        panic!("summon slot 2 must be Gimard's actor record");
    };
    assert!(
        gimard
            .name
            .as_deref()
            .is_some_and(|n| n.starts_with("Burn")),
        "summon group 0 actor record must carry the Burning Attack name"
    );
}
