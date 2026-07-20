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
        assert_eq!(
            s.sc_offset % 2,
            0,
            "unaligned halfword at {:#x}",
            s.sc_offset
        );
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
        for cell in [base + CURRENT_STATS_OFFSET + 8, base + MAX_STATS_OFFSET + 4] {
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

/// Entry point + byte length of the world-state seed (`FUN_80034A6C`).
const WORLD_SEED_VA: u32 = 0x8003_4A6C;
const WORLD_SEED_LEN: usize = 0x100;

/// `$s0` value the world-state seed keeps live: the save-context base.
const SC_BASE_VA: u32 = 0x8008_4140;

/// Decode the `$s0`-relative `sb` / `sw` stores in `code`, recovering each
/// stored value by tracking the `addiu rt, $zero, imm` immediates that feed
/// them (plus `$zero` itself). Returns `(offset, value, width)` in program
/// order.
///
/// Also returns the `$s0` the routine's `lui`/`addiu` pair builds, so the test
/// can assert the base register really is the save-context base rather than
/// assuming it.
fn s0_seed_stores(code: &[u8]) -> (Option<u32>, Vec<(u32, u32, new_game::SeedWidth)>) {
    let mut regs = [0u32; 32];
    let mut s0_base = None;
    let mut out = Vec::new();

    for chunk in code.chunks_exact(4) {
        let word = u32::from_le_bytes(chunk.try_into().unwrap());
        let op = word >> 26;
        let rs = ((word >> 21) & 0x1F) as usize;
        let rt = ((word >> 16) & 0x1F) as usize;
        let imm = word & 0xFFFF;
        // Sign-extended immediate, for `addiu`.
        let simm = imm as i16 as i32 as u32;

        match op {
            // lui rt, imm
            0x0F => regs[rt] = imm << 16,
            // addiu rt, rs, imm - only the two forms this routine uses.
            0x09 => {
                if rs == 0 {
                    regs[rt] = simm;
                } else {
                    regs[rt] = regs[rs].wrapping_add(simm);
                }
                // The `lui $s0` / `addiu $s0, $s0, ...` pair that builds SC.
                if rt == 16 {
                    s0_base = Some(regs[16]);
                }
            }
            // sb / sw off $s0 ($zero always reads 0, which `regs` preserves).
            0x28 | 0x2B if rs == 16 => {
                let width = if op == 0x28 {
                    new_game::SeedWidth::Byte
                } else {
                    new_game::SeedWidth::Word
                };
                out.push((imm, regs[rt], width));
            }
            _ => {}
        }
    }
    (s0_base, out)
}

/// The world-state seed table is a list of *code literals*, so nothing about it
/// is parsed from the disc at runtime - which is exactly why it needs a disc
/// oracle. Re-derive every offset, value and store width straight from the
/// instruction encodings in the user's own `SCUS_942.54` and require the port's
/// table to match, so a wrong width cannot survive here the way a `DAT_` /
/// `_DAT_` naming guess could.
#[test]
fn world_state_seed_matches_the_routines_stores() {
    let Some(scus) = gated() else { return };
    let Some(off) = new_game::scus_file_offset(&scus, WORLD_SEED_VA) else {
        eprintln!("[skip] SCUS_942.54 is not a PSX-EXE we can map");
        return;
    };
    let Some(code) = scus.get(off..off + WORLD_SEED_LEN) else {
        eprintln!("[skip] world-state seed runs past the image");
        return;
    };

    let (s0_base, stores) = s0_seed_stores(code);
    assert_eq!(
        s0_base,
        Some(SC_BASE_VA),
        "the seed's base register is not the save-context base; the routine moved"
    );
    assert!(
        !stores.is_empty(),
        "no $s0-relative stores decoded; the routine or its base register moved"
    );

    // The port's pre-expander table must be exactly reproduced by the encodings.
    // Stores past the template expander (the starting-item seed at
    // INVENTORY_SC_OFFSET) are deliberately outside this set, so match on the
    // offsets the port claims rather than on the routine's full store list.
    for w in new_game::new_game_seed_words() {
        let found = stores
            .iter()
            .find(|(o, _, _)| *o == w.sc_offset)
            .unwrap_or_else(|| {
                panic!(
                    "port seeds SC+{:#x}, but the routine performs no $s0-relative \
                     store there; decoded offsets: {:x?}",
                    w.sc_offset,
                    stores.iter().map(|(o, _, _)| *o).collect::<Vec<_>>()
                )
            });
        assert_eq!(
            found.2, w.width,
            "SC+{:#x}: port says {:?}, the instruction is {:?}",
            w.sc_offset, w.width, found.2
        );
        assert_eq!(
            found.1, w.value,
            "SC+{:#x}: port says {:#x}, the instruction stores {:#x}",
            w.sc_offset, w.value, found.1
        );
    }

    // ...and the other direction: the port must not *omit* a store. Every
    // $s0-relative store the routine performs below the roster is part of the
    // pre-expander world-state set, so the two lists must agree exactly.
    let claimed: BTreeSet<u32> = new_game::new_game_seed_words()
        .iter()
        .map(|w| w.sc_offset)
        .collect();
    let performed: BTreeSet<u32> = stores
        .iter()
        .map(|(o, _, _)| *o)
        .filter(|&o| o < LIVE_RECORD_0_XP_OFFSET)
        .collect();
    assert_eq!(
        performed, claimed,
        "the routine's pre-roster stores and the port's table disagree"
    );

    // Gold specifically: a word store of the documented literal.
    let gold = stores
        .iter()
        .find(|(o, _, _)| *o == new_game::GOLD_SC_OFFSET)
        .expect("no gold store at GOLD_SC_OFFSET");
    assert_eq!(gold.1, new_game::NEW_GAME_STARTING_GOLD);
    assert_eq!(gold.2, new_game::SeedWidth::Word);

    // The story-flag clear must cover the Door-of-Wind warp bitmask, which is
    // why the warp preset has to run after it.
    let lo = new_game::STORY_FLAGS_SC_OFFSET;
    let hi = lo + new_game::STORY_FLAGS_LEN;
    assert!((lo..hi).contains(&new_game::WARP_FLAGS_SC_OFFSET));

    // No fixed world-state write may land inside a live character record.
    for w in &new_game::new_game_seed_words() {
        assert!(
            w.sc_offset < LIVE_RECORD_0_XP_OFFSET,
            "world-state seed at SC+{:#x} collides with the roster",
            w.sc_offset
        );
    }
}

/// The starting-item seed region the randomizer overwrites must still be the
/// ten instructions the module claims, and its first four must be the
/// `li`/`sb` pair writing Healing Leaf x5 into the inventory page. If retail
/// ever put something else there, patching the region would corrupt it.
#[test]
fn starting_item_seed_region_is_the_documented_ten_instructions() {
    let Some(scus) = gated() else { return };
    let Some(off) = new_game::scus_file_offset(&scus, new_game::STARTING_INV_SEED_VA) else {
        eprintln!("[skip] SCUS_942.54 is not a PSX-EXE we can map");
        return;
    };
    let Some(code) = scus.get(off..off + new_game::STARTING_INV_SEED_LEN) else {
        eprintln!("[skip] seed region runs past the image");
        return;
    };
    assert_eq!(new_game::STARTING_INV_SEED_LEN % 4, 0);

    let (_, stores) = s0_seed_stores(code);
    let inv = new_game::INVENTORY_SC_OFFSET;
    let ids: Vec<u32> = stores
        .iter()
        .filter(|(o, _, w)| *w == new_game::SeedWidth::Byte && (*o == inv || *o == inv + 1))
        .map(|(o, _, _)| *o)
        .collect();
    assert_eq!(
        ids,
        vec![inv, inv + 1],
        "the region no longer writes the item id / count pair at SC+{inv:#x}"
    );

    // The tail is the redundant zero-loop: a backward branch whose target is
    // inside the region. Encoding: bgez (op 0x01, rt 0x01).
    let has_back_branch = code.chunks_exact(4).enumerate().any(|(i, c)| {
        let word = u32::from_le_bytes(c.try_into().unwrap());
        let op = word >> 26;
        let rt = (word >> 16) & 0x1F;
        let disp = (word & 0xFFFF) as i16 as i32;
        op == 0x01 && rt == 0x01 && disp < 0 && i as i32 + 1 + disp >= 0
    });
    assert!(
        has_back_branch,
        "no backward bgez inside the region; the redundant zero-loop is not there, \
         so the reclaimable length is wrong"
    );
}
