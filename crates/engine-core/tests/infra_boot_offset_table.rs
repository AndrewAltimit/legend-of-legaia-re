//! `FUN_800265E8`'s boot table is the per-slot SPU RAM base table.
//!
//! The seeder's twelve words (`legaia_engine_core::scus_leaf_kernels::
//! BOOT_OFFSET_TABLE`, installed at `0x800917B0`) and the audio side's
//! `legaia_asset::sfx_table::spu_base_for_slot` are two transcriptions of one
//! retail table, reached from opposite ends: the seeder writes it, and
//! `FUN_8002630C` reads `table[slot]` out of it to hand `SsVabOpenHead`.
//!
//! They were transcribed independently, so this asserts they agree. It is the
//! guard on the identification, not on a wire: neither copy calls the other,
//! and folding them into one source of truth is the change this measures the
//! cost of.
//!
//! Disc-free - both sides are code literals.

use legaia_asset::sfx_table::spu_base_for_slot;
use legaia_engine_core::scus_leaf_kernels::{
    BOOT_OFFSET_TABLE, BOOT_OFFSET_TABLE_UNWRITTEN, seed_boot_offset_table,
};

/// Every slot the seeder writes is the base the VAB-open path reads for that
/// slot, and the one slot it skips is the one with no base.
#[test]
fn seeded_table_matches_the_audio_side_slot_bases() {
    for (slot, word) in BOOT_OFFSET_TABLE.iter().enumerate() {
        let audio = spu_base_for_slot(slot as u8);
        if slot == BOOT_OFFSET_TABLE_UNWRITTEN {
            assert_eq!(
                audio, None,
                "slot {slot} is unwritten by FUN_800265E8, so it should have no SPU base"
            );
            continue;
        }
        assert_eq!(
            audio,
            Some(*word),
            "slot {slot}: seeder writes {word:#X}, spu_base_for_slot says {audio:?}"
        );
    }
}

/// The aliased slot pairs the audio side documents are aliases *because* the
/// seeder stores one register into both cells.
#[test]
fn aliased_slots_share_one_seeded_word() {
    for (a, b) in [(0u8, 10u8), (1, 5), (2, 6), (4, 7)] {
        assert_eq!(
            BOOT_OFFSET_TABLE[a as usize], BOOT_OFFSET_TABLE[b as usize],
            "slots {a}/{b} are documented as sharing an SPU base"
        );
    }
}

/// Seeding an arbitrary buffer reproduces the table, leaving slot 9 alone -
/// the behaviour the reader depends on, since retail never initialises it.
#[test]
fn seeding_leaves_the_unwritten_slot_untouched() {
    let sentinel = 0xDEAD_BEEF;
    let mut table = [sentinel; 12];
    seed_boot_offset_table(&mut table);

    assert_eq!(table[BOOT_OFFSET_TABLE_UNWRITTEN], sentinel);
    for (slot, word) in table.iter().enumerate() {
        if slot == BOOT_OFFSET_TABLE_UNWRITTEN {
            continue;
        }
        assert_eq!(*word, BOOT_OFFSET_TABLE[slot], "slot {slot}");
    }
}
