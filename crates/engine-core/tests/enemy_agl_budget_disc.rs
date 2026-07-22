//! Disc-gated: real monster records feed the enemy AGL multi-action budget.
//!
//! `monster_def_from_record` copies the AGL stat (record `+0x0E`) into
//! [`MonsterDef::agl`] and gathers the physical swing-action AGL costs
//! (`+0x74`, tag `0x0C..=0x1F`) into `action_costs`. This test decodes the real
//! monster archive (PROT 867) and asserts that (a) at least one real monster
//! carries an AGL gauge and a costed swing set, and (b) the enemy budget port
//! ([`legaia_engine_vm::battle_action::enemy_action_budget`]) turns that data
//! into a swing count of at least 1 for such a monster - i.e. the B8 plumbing +
//! B-enemy-AI consumer resolve off real disc bytes, not just the synthetic
//! catalog.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset (disc-gated convention).
use legaia_engine_core::monster_catalog::monster_def_from_record;
use legaia_patcher::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};

fn load_disc() -> Option<Vec<u8>> {
    let path = std::env::var_os("LEGAIA_DISC_BIN")?;
    std::fs::read(path).ok()
}

#[test]
fn real_monster_records_populate_agl_budget() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open disc");
    let archive = patcher
        .read_entry(MONSTER_ARCHIVE_ENTRY)
        .expect("read monster archive");
    let records = legaia_asset::monster_archive::records(&archive).expect("decode archive");
    assert!(!records.is_empty(), "archive decodes at least one monster");

    let mut with_agl = 0usize;
    let mut with_swings = 0usize;
    let mut budgeted = 0usize;
    for r in &records {
        let def = monster_def_from_record(r);
        // The AGL stat is copied verbatim from record `+0x0E`.
        assert_eq!(def.agl, r.agility(), "AGL copied from the record");
        if def.agl > 0 {
            with_agl += 1;
        }
        if def.action_costs.is_empty() {
            continue;
        }
        with_swings += 1;
        // Every gathered swing cost is a real, non-sentinel AGL cost.
        assert!(def.action_costs.iter().all(|&c| c != 0xFF));
        // A monster with an AGL gauge + at least one affordable swing lands >= 1
        // swing this turn. Use a fixed RNG (always pick candidate 0) so the
        // count is deterministic; the cheapest swing bounds the minimum.
        if def.agl > 0 {
            let mut rng = || 0u32;
            let n = legaia_engine_vm::battle_action::enemy_action_count(
                def.agl,
                &def.action_costs,
                &mut rng,
            );
            // If the picked (index-0) swing is affordable at all, the budget
            // lands at least one swing; a monster whose cheapest swing exceeds
            // its whole AGL gauge would land zero (still a valid single fallback
            // upstream), so only assert on the affordable majority.
            if def.action_costs[0] as u16 <= def.agl {
                assert!(n >= 1, "affordable swing lands at least once");
                budgeted += 1;
            }
        }
    }

    // Retail monsters overwhelmingly carry an AGL gauge and costed swings; this
    // pins that the plumbing sees real data (not an all-zero read).
    assert!(
        with_agl > 0,
        "at least one real monster carries an AGL gauge"
    );
    assert!(
        with_swings > 0,
        "at least one real monster has costed physical swings"
    );
    assert!(
        budgeted > 0,
        "the AGL budget lands >=1 swing for a real monster"
    );
    eprintln!(
        "[enemy-agl-budget] {} records: {} with AGL, {} with swing costs, {} budgeted",
        records.len(),
        with_agl,
        with_swings,
        budgeted
    );
}
