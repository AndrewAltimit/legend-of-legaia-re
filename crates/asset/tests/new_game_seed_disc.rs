//! Disc-gated validation of the new-game seed ports
//! (`legaia_asset::new_game::seed_live_records` / `new_game_seed_words`).
//!
//! The record-relative offsets those functions emit were read out of the seed
//! routine's decompilation, which makes them exactly the kind of claim that
//! rots silently. This pins them against the **instruction encodings in the
//! user's own `SCUS_942.54`**: the routine stores every seeded stat with an
//! `sh rt, imm($s0)` (`$s0` = save-context base), so decoding its `$s0`-relative
//! halfword stores recovers the offset set directly from Sony's bytes.
//!
//! Asserts that:
//!
//!  - the offsets [`seed_live_records`] produces for roster slot 0 are exactly
//!    the `$s0`-relative `sh` immediates the routine executes for that slot;
//!  - the level / magic-rank byte stores agree with the offsets the module
//!    already documents for the starting-level randomizer's patch sites;
//!  - the seeded cap constant really is a literal shared by all four slots
//!    rather than a per-character template field.
//!
//! Skips and passes when `LEGAIA_DISC_BIN` / `extracted/` are absent.

use legaia_asset::new_game::{
    self, CURRENT_STATS_OFFSET, LEVEL_OFFSET, LIVE_RECORD_0_XP_OFFSET, MAGIC_RANK_OFFSET,
    MAX_STATS_OFFSET, SEEDED_CAP_CONSTANT, StartingParty,
};
use std::collections::BTreeSet;
use std::path::PathBuf;

/// Entry point + byte length of the template-expansion routine (`FUN_800560B4`).
const SEED_ROUTINE_VA: u32 = 0x8005_60B4;
const SEED_ROUTINE_LEN: usize = 0x140;

fn extracted_root() -> Option<PathBuf> {
    ["extracted", "../extracted", "../../extracted"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.join("SCUS_942.54").is_file())
}

fn gated() -> Option<Vec<u8>> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    }
    let root = extracted_root()?;
    std::fs::read(root.join("SCUS_942.54")).ok()
}

/// Collect the `$s0`-relative store immediates in a code region, split by width.
///
/// `op 0x28` = `sb`, `0x29` = `sh`; `rs == 16` selects `$s0`, the save-context
/// base register the seed loop keeps live.
fn s0_store_offsets(code: &[u8]) -> (BTreeSet<u32>, BTreeSet<u32>) {
    let mut bytes = BTreeSet::new();
    let mut halfwords = BTreeSet::new();
    for chunk in code.chunks_exact(4) {
        let word = u32::from_le_bytes(chunk.try_into().unwrap());
        let op = word >> 26;
        let rs = (word >> 21) & 0x1F;
        let imm = word & 0xFFFF;
        if rs != 16 {
            continue;
        }
        match op {
            0x28 => {
                bytes.insert(imm);
            }
            0x29 => {
                halfwords.insert(imm);
            }
            _ => {}
        }
    }
    (bytes, halfwords)
}

#[test]
fn seeded_stat_offsets_match_the_routines_store_immediates() {
    let Some(scus) = gated() else { return };
    let Some(off) = new_game::scus_file_offset(&scus, SEED_ROUTINE_VA) else {
        eprintln!("[skip] SCUS_942.54 is not a PSX-EXE we can map");
        return;
    };
    let Some(code) = scus.get(off..off + SEED_ROUTINE_LEN) else {
        eprintln!("[skip] seed routine runs past the image");
        return;
    };
    let (byte_stores, half_stores) = s0_store_offsets(code);
    assert!(
        !half_stores.is_empty(),
        "no $s0-relative halfword stores found; the routine or its base register moved"
    );

    // The routine's loop body writes slot 0's offsets literally (later slots
    // come from advancing $s0 by the record stride), so slot 0's emitted set is
    // what must appear in the encodings.
    let party = StartingParty::from_scus(&scus).expect("starting-party template");
    let slot0_end = LIVE_RECORD_0_XP_OFFSET + new_game::LIVE_RECORD_STRIDE;
    let emitted: BTreeSet<u32> = new_game::seed_live_records(&party)
        .into_iter()
        .map(|s| s.sc_offset)
        .filter(|&o| o < slot0_end)
        .collect();

    assert!(
        !emitted.is_empty(),
        "seed_live_records produced nothing for slot 0"
    );
    for offset in &emitted {
        assert!(
            half_stores.contains(offset),
            "port emits a halfword store at SC+{offset:#x} that the routine never performs; \
             routine stores: {half_stores:x?}"
        );
    }

    // The level / magic-rank pair are byte stores, and must agree with the
    // offsets the module already pins for the starting-level patch sites.
    let level = LIVE_RECORD_0_XP_OFFSET + LEVEL_OFFSET;
    let rank = LIVE_RECORD_0_XP_OFFSET + MAGIC_RANK_OFFSET;
    assert!(
        byte_stores.contains(&level) && byte_stores.contains(&rank),
        "level/magic-rank stores at SC+{level:#x}/SC+{rank:#x} not found; \
         routine byte stores: {byte_stores:x?}"
    );
}

#[test]
fn stat_blocks_are_disjoint_and_inside_the_record() {
    let Some(scus) = gated() else { return };
    let party = StartingParty::from_scus(&scus).expect("starting-party template");
    assert_eq!(party.len(), new_game::PARTY_RECORDS);

    let seeds = new_game::seed_live_records(&party);
    let unique: BTreeSet<u32> = seeds.iter().map(|s| s.sc_offset).collect();
    assert_eq!(unique.len(), seeds.len(), "a seed offset is written twice");

    for s in &seeds {
        // Every write must land inside some record's 0x414-byte extent.
        let rel = s.sc_offset - LIVE_RECORD_0_XP_OFFSET;
        let within = rel % new_game::LIVE_RECORD_STRIDE;
        assert!(
            rel / new_game::LIVE_RECORD_STRIDE < new_game::PARTY_RECORDS as u32,
            "SC+{:#x} lands past the roster",
            s.sc_offset
        );
        assert!(
            (CURRENT_STATS_OFFSET..MAX_STATS_OFFSET + 18).contains(&within),
            "SC+{:#x} (record +{within:#x}) is outside both stat blocks",
            s.sc_offset
        );
        // Halfword stores must not straddle an odd address.
        assert_eq!(s.sc_offset % 2, 0, "unaligned halfword at {:#x}", s.sc_offset);
    }
}

#[test]
fn the_cap_constant_is_a_literal_not_a_template_field() {
    let Some(scus) = gated() else { return };
    let party = StartingParty::from_scus(&scus).expect("starting-party template");
    let seeds = new_game::seed_live_records(&party);

    // Every slot's cap cells hold the same value regardless of that member's
    // agility - which is the whole point of calling it a literal.
    let mut agls = BTreeSet::new();
    for (slot, m) in party.members().iter().enumerate() {
        agls.insert(m.agl);
        let base = LIVE_RECORD_0_XP_OFFSET + slot as u32 * new_game::LIVE_RECORD_STRIDE;
        for cell in [
            base + CURRENT_STATS_OFFSET + 8,
            base + MAX_STATS_OFFSET + 4,
        ] {
            let got = seeds
                .iter()
                .find(|s| s.sc_offset == cell)
                .unwrap_or_else(|| panic!("no cap store at SC+{cell:#x}"));
            assert_eq!(
                got.value, SEEDED_CAP_CONSTANT,
                "slot {slot} cap cell SC+{cell:#x} should be the literal"
            );
        }
    }
    assert!(
        agls.len() > 1,
        "the roster's agility values are all equal, so this test cannot \
         distinguish the literal from the template field"
    );
}

#[test]
fn world_state_seed_carries_the_starting_gold() {
    let Some(_scus) = gated() else { return };
    let words = new_game::new_game_seed_words();
    let gold = words
        .iter()
        .find(|w| w.sc_offset == new_game::GOLD_SC_OFFSET)
        .expect("gold seed");
    assert_eq!(gold.value, new_game::NEW_GAME_STARTING_GOLD);
    assert_eq!(gold.width, new_game::SeedWidth::Word);

    // The story-flag clear must cover the Door-of-Wind warp bitmask, which is
    // why the warp preset has to run after it.
    let lo = new_game::STORY_FLAGS_SC_OFFSET;
    let hi = lo + new_game::STORY_FLAGS_LEN;
    assert!((lo..hi).contains(&new_game::WARP_FLAGS_SC_OFFSET));

    // No fixed world-state write may land inside a live character record.
    for w in &words {
        assert!(
            w.sc_offset < LIVE_RECORD_0_XP_OFFSET,
            "world-state seed at SC+{:#x} collides with the roster",
            w.sc_offset
        );
    }
}
