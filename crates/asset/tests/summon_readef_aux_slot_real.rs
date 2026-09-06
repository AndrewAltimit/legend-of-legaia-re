//! Disc-gated regression test for the applier's **`base + 1`** slot gate in
//! `readef.DAT` / `summon.dat` (extraction PROT 894 / 893).
//!
//! Skips silently when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.
//!
//! What this catches:
//! - [`summon_readef::aux_slot_is_texture_upload`] drifting off
//!   `FUN_801F12D0` stage 4's own gate (`sltiu v0,v1,0x42` at `0x801F1500`
//!   and `sltiu v0,v0,0x2b` at `0x801F150C`). The gate is a *base-byte*
//!   predicate the applier evaluates before it looks at the slot, so it can
//!   only be right if it agrees with the file's own content - which is what
//!   this test measures.
//! - The three readef groups whose aux slot is an actor record rather than a
//!   texture (`base` = `0x39` / `0x3C` / `0x3F`) changing membership, and
//!   their `base+2` twin ceasing to be a byte-identical copy. Nothing stages
//!   `base+2` for a readef group - the applier stops at stage 4 - so the twin
//!   is the reason the aux slot is where the record has to live.
//!
//! Format: `docs/formats/summon-readef.md` § "The higher readef groups'
//! `base+1` slot".

use legaia_asset::summon_readef::{
    self, READEF_PROT_INDEX, READEF_SLOT_COUNT, SLOT_BYTES, SUMMON_PROT_INDEX, SlotKind,
};
use std::path::{Path, PathBuf};

/// Readef groups whose aux slot the gate excludes from the second texture
/// upload while still being past the character band.
const ACTOR_RECORD_BASES: [u8; 3] = [0x39, 0x3C, 0x3F];
/// Below this base the group is one of the four characters' art banks.
const CHARACTER_BAND_END: u8 = 0x0C;

fn extracted_root() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    ["extracted", "../../extracted"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.join("PROT").is_dir())
}

fn entry_bytes(root: &Path, index: u32) -> Option<Vec<u8>> {
    let dir = root.join("PROT");
    let prefix = format!("{index:04}_");
    let name = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|n| n.starts_with(&prefix))?;
    std::fs::read(dir.join(name)).ok()
}

#[test]
fn gate_matches_the_readef_aux_slot_kinds_or_skips() {
    let Some(root) = extracted_root() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let Some(buf) = entry_bytes(&root, READEF_PROT_INDEX.into()) else {
        eprintln!("[skip] extraction entry 0894 missing");
        return;
    };
    let file = summon_readef::parse(&buf).expect("readef.DAT parses");
    assert_eq!(file.slots.len(), READEF_SLOT_COUNT);

    let mut textures = 0usize;
    let mut me_archives = 0usize;
    let mut actor_records = Vec::new();
    for group in 0..(READEF_SLOT_COUNT / 3) {
        let base = (group * 3) as u8;
        let aux = &file.slots[group * 3 + 1];
        match &aux.kind {
            SlotKind::Texture(_) => {
                assert!(
                    summon_readef::aux_slot_is_texture_upload(base),
                    "group {group} (base {base:#04x}) holds a texture the gate would skip"
                );
                textures += 1;
            }
            SlotKind::MeArchive { .. } => {
                assert!(
                    base < CHARACTER_BAND_END,
                    "an ME archive outside the character band (base {base:#04x})"
                );
                assert!(!summon_readef::aux_slot_is_texture_upload(base));
                me_archives += 1;
            }
            SlotKind::ActorRecord(_) => {
                assert!(
                    !summon_readef::aux_slot_is_texture_upload(base),
                    "group {group} (base {base:#04x}) holds an actor record the gate would upload"
                );
                actor_records.push(base);
            }
            SlotKind::Payload => panic!("group {group} aux slot is unclassified"),
        }
    }
    assert_eq!(me_archives, 4, "one art bank per playable character");
    assert_eq!(
        actor_records, ACTOR_RECORD_BASES,
        "the three excluded groups"
    );
    assert!(textures > 15, "the rest of the file is texture pairs");

    // Each excluded group's aux slot is a byte-identical copy of its base+2
    // slot, which the applier never stages for a readef group.
    for base in ACTOR_RECORD_BASES {
        let group = base as usize / 3;
        let a = &buf[(group * 3 + 1) * SLOT_BYTES..(group * 3 + 2) * SLOT_BYTES];
        let b = &buf[(group * 3 + 2) * SLOT_BYTES..(group * 3 + 3) * SLOT_BYTES];
        assert_eq!(a, b, "base {base:#04x}: aux slot == base+2 slot");
    }
}

#[test]
fn every_summon_group_passes_the_gate_or_skips() {
    let Some(root) = extracted_root() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let Some(buf) = entry_bytes(&root, SUMMON_PROT_INDEX.into()) else {
        eprintln!("[skip] extraction entry 0893 missing");
        return;
    };
    let file = summon_readef::parse(&buf).expect("summon.dat parses");
    // Every summon base has bit 7 set, so `base >= 0x42` is always true and
    // the aux slot is always the group's second texture page.
    for spell in 0x81u8..=0xA0 {
        let base = summon_readef::base_byte_for_action(spell);
        assert!(base & 0x80 != 0, "spell {spell:#04x} selects summon.dat");
        assert!(
            summon_readef::aux_slot_is_texture_upload(base),
            "spell {spell:#04x} (base {base:#04x}) must upload its aux slot"
        );
        let slot = (base & 0x7F) as usize + 1;
        assert!(
            matches!(file.slots[slot].kind, SlotKind::Texture(_)),
            "spell {spell:#04x} aux slot {slot} is a texture"
        );
    }
}
