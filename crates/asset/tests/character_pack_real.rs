//! Disc-gated regression for [`character_pack::parse`] against the real disc.
//!
//! Asserts the five-slot shape of PROT 0874 §0 (the player-character pack)
//! plus the equipment-conditional templates baked into the active-party slots'
//! TMDs. The numbers below come from the `docs/formats/world-map-overlay.md`
//! provenance table and are pinned by byte-equality against a settled
//! field-scene RAM snapshot at `DAT_8007C018[0..=4]`.
//!
//! Skips silently when `LEGAIA_DISC_BIN` is unset or when `PROT.DAT` isn't on
//! disk.
//!
//! What this catches:
//! - PROT 0874 stops being the character pack (extractor truncation, footprint
//!   drift, CDNAME shuffle).
//! - The LZS-then-pack chain regresses (header off, descriptor count drift).
//! - The disc-form group-count drift (we pin `nobj = 12/12/12/3/2` because the
//!   engine relies on the active-party slots carrying group 10 / 11 to drive
//!   the equipment swap; if a future extractor change rebakes the pack, the
//!   `engine_core::scene::seed_global_tmd_pool_from_befect_data` consumer
//!   would silently regress without this guard).

use std::path::PathBuf;

use legaia_asset::character_pack::{self, EQUIP_GROUP_NONZERO_OFFSET, EQUIP_GROUP_ZERO_OFFSET};
use legaia_prot::archive::Archive;

fn prot_dat() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted/PROT.DAT", "../../extracted/PROT.DAT"] {
        let f = PathBuf::from(p);
        if f.is_file() {
            return Some(f);
        }
    }
    None
}

#[test]
fn character_pack_parses_five_slots_from_prot_0874() {
    let Some(prot) = prot_dat() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };

    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let entry = archive
        .entries
        .iter()
        .find(|e| e.index == character_pack::PROT_ENTRY_INDEX)
        .expect("PROT entry 874 present")
        .clone();
    let mut buf = Vec::new();
    archive.read_entry(&entry, &mut buf).expect("read PROT 874");

    let pack = character_pack::parse(&buf).expect("parse PROT 874 §0 character pack");

    // Disc-pinned per-slot nobj (matches docs/formats/world-map-overlay.md
    // table: 12/12/12/3/2).
    let expected_nobj = [12u32, 12, 12, 3, 2];
    // Runtime-allocated TMD body sizes — what the LZS decode produces under
    // retail's compressed-size-bounded path. Slot 4's 1 048 bytes match the
    // live DAT_8007C018[4] allocation byte-for-byte.
    let expected_bytes = [13_220usize, 13_800, 11_656, 6_488, 1_048];
    for (i, slot) in pack.slots().iter().enumerate() {
        assert_eq!(slot.disc_nobj, expected_nobj[i], "slot {i} disc nobj");
        assert_eq!(slot.slot, i, "slot index matches array position");
        assert_eq!(
            slot.tmd_bytes.len(),
            expected_bytes[i],
            "slot {i} runtime body bytes"
        );
    }

    // Active-party slots must carry the equipment-conditional templates at
    // the documented offsets; auxiliary slots must not.
    for slot in pack.active_party() {
        assert!(
            slot.equipped_template().is_some(),
            "slot {} should expose the equipped (group 10) template",
            slot.slot
        );
        assert!(
            slot.unequipped_template().is_some(),
            "slot {} should expose the unequipped (group 11) template",
            slot.slot
        );
        assert_eq!(
            slot.equipped_template().unwrap().len(),
            character_pack::GROUP_DESCRIPTOR_BYTES,
            "equipped template is one full group descriptor"
        );
        // Sanity: the two templates differ on at least one byte (otherwise
        // the equipment-swap pass would be a no-op).
        let a = slot.equipment_template(EQUIP_GROUP_NONZERO_OFFSET).unwrap();
        let b = slot.equipment_template(EQUIP_GROUP_ZERO_OFFSET).unwrap();
        assert_ne!(
            a, b,
            "slot {} group 10 / group 11 templates should differ",
            slot.slot
        );
    }
    for slot in &pack.slots()[3..] {
        assert!(
            !slot.is_active_party(),
            "slot {} is not active-party",
            slot.slot
        );
        assert!(
            slot.equipped_template().is_none(),
            "slot {} should not expose the active-party templates",
            slot.slot
        );
    }

    // Every body parses as a Legaia TMD via the canonical parser.
    for slot in pack.slots() {
        let tmd = legaia_tmd::parse(&slot.tmd_bytes)
            .unwrap_or_else(|e| panic!("slot {} TMD parse: {e:#}", slot.slot));
        assert_eq!(
            tmd.header.nobj, slot.disc_nobj,
            "slot {} TMD header nobj matches +0x08",
            slot.slot
        );
    }
}

#[test]
fn equipment_swap_template_choice_matches_runtime() {
    let Some(prot) = prot_dat() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };

    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let entry = archive
        .entries
        .iter()
        .find(|e| e.index == character_pack::PROT_ENTRY_INDEX)
        .expect("PROT entry 874 present")
        .clone();
    let mut buf = Vec::new();
    archive.read_entry(&entry, &mut buf).expect("read PROT 874");
    let pack = character_pack::parse(&buf).expect("parse character pack");

    // For each active-party slot, applying the swap with equip_byte=0 should
    // copy group 11 in; with any non-zero byte it should copy group 10 in.
    for (i, patch) in character_pack::equipment_swap::ACTIVE_PARTY_SLOTS
        .iter()
        .enumerate()
    {
        let slot = pack.slot(i).expect("slot present");
        let group11 = slot.unequipped_template().unwrap().to_vec();
        let group10 = slot.equipped_template().unwrap().to_vec();
        let dst_off = character_pack::FIRST_GROUP_DESCRIPTOR_OFFSET
            + patch.patched_group_index as usize * character_pack::GROUP_DESCRIPTOR_BYTES;

        // equip_byte == 0  -> group 11
        let patched_zero = character_pack::equipment_swap::apply(&slot.tmd_bytes, *patch, 0);
        assert_eq!(
            &patched_zero[dst_off..dst_off + character_pack::GROUP_DESCRIPTOR_BYTES],
            &group11[..],
            "slot {i}: equip_byte=0 should source group 11"
        );

        // equip_byte != 0  -> group 10
        let patched_nz = character_pack::equipment_swap::apply(&slot.tmd_bytes, *patch, 1);
        assert_eq!(
            &patched_nz[dst_off..dst_off + character_pack::GROUP_DESCRIPTOR_BYTES],
            &group10[..],
            "slot {i}: equip_byte=1 should source group 10"
        );
    }
}
