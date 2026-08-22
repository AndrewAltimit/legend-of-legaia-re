//! Disc-gated end-to-end test for the **battle-load safety cap** on randomized
//! encounters. The retail battle engine has two hard limits the encounter
//! tables must respect:
//!
//! - at most 2 **distinct** species per formation (the setup `FUN_80055B6C`
//!   species-order rebuild holds "the other species" in one register - a third
//!   distinct species is dropped-and-duplicated on half of all rolls, and on
//!   the other half streams a third decoded block);
//! - a bounded per-formation heap cost: the summed `block[+0x08]` bytes of the
//!   formation's distinct species (the battle heap's malloc is unchecked, so
//!   exceeding the workable budget is a silent hang at battle load, not an
//!   error - `docs/subsystems/battle.md`, heap-budget section).
//!
//! Retail authoring satisfies both everywhere; a kingdom/world-scope shuffle
//! can violate both. This test asserts, on a scratch copy of the disc:
//!
//! - the disc's authored tables satisfy both limits (the budget the pass
//!   enforces is well-defined and non-vacuous);
//! - a guard-free kingdom shuffle (the raw scoped pass) manufactures at least
//!   one violating formation - the defect is real, not hypothetical;
//! - the full pipeline (`randomize_encounters_full`, balanced-preset shape:
//!   kingdom scope + shuffle + solo-strong) leaves **no** violating random
//!   formation on the patched image, across BOTH MAN carriers (bundle +
//!   streaming), and reports the cap;
//! - every touched sector stays EDC/ECC-valid and a fixed seed is
//!   byte-deterministic.
//!
//! Skips + passes without `LEGAIA_DISC_BIN`.

use legaia_iso::raw::SECTOR_SIZE;
use legaia_iso::write::{is_form2, mode2_form1_sector_is_valid};
use legaia_patcher::apply::{self, EncounterScope, SoloStrongConfig};
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::drops::DropMode;
use legaia_patcher::encounter::{MAX_DISTINCT_SPECIES, MonsterCostTable, SceneEncounters};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Every random formation on the image that violates the species or heap-cost
/// limit, as `(entry_idx, formation_idx, ids, cost)` - across BOTH MAN
/// carriers (the v12-family dungeons carry theirs as a streaming chunk).
fn violations(
    patcher: &DiscPatcher,
    table: &MonsterCostTable,
    budget: u32,
) -> Vec<(usize, usize, Vec<u8>, u32)> {
    let mut out = Vec::new();
    for idx in 0..patcher.entry_count() {
        let Ok(entry) = patcher.read_entry(idx) else {
            continue;
        };
        let mut scenes = Vec::new();
        if let Some(s) = SceneEncounters::locate(&entry, idx) {
            scenes.push(s);
        }
        scenes.extend(SceneEncounters::locate_streaming_mans(&entry, idx));
        for scene in &scenes {
            for i in 0..scene.formation_count() {
                if !scene.is_random_formation(i) {
                    continue;
                }
                let ids = scene.formation_ids(i);
                let mut d: Vec<u8> = ids.iter().copied().filter(|&x| x != 0).collect();
                d.sort_unstable();
                d.dedup();
                let cost = table.formation_cost(&ids);
                if d.len() > MAX_DISTINCT_SPECIES || cost > budget {
                    out.push((idx, i, ids, cost));
                }
            }
        }
    }
    out
}

#[test]
fn battle_load_cap_holds_on_patched_image() {
    let Some(image) = load_disc() else {
        eprintln!("LEGAIA_DISC_BIN not set; skipping");
        return;
    };

    let baseline = DiscPatcher::open(image.clone()).expect("open disc");
    let table = apply::monster_cost_table(&baseline).expect("cost table");
    let budget = apply::battle_load_budget(&baseline, &table).expect("budget");

    // The authored tables define the budget, and satisfy both limits: the
    // budget is exactly the authored maximum (some formation reaches it), and
    // no authored random formation exceeds 2 distinct species.
    let authored = violations(&baseline, &table, budget);
    assert!(
        authored.is_empty(),
        "retail tables violate their own limits: {authored:?}"
    );
    assert!(
        budget > 100_000,
        "authored max implausibly small: {budget} (USA disc's is ~124 KB)"
    );

    // Non-vacuity: the raw guard-free kingdom shuffle manufactures violations.
    let seed = 0x00C0_FFEE_2026_u64;
    let mut unguarded = DiscPatcher::open(image.clone()).expect("open disc");
    apply::randomize_encounters_scoped(
        &mut unguarded,
        seed,
        DropMode::Shuffle,
        EncounterScope::Kingdom,
        &[],
    )
    .expect("raw scoped shuffle");
    let raw_violations = violations(&unguarded, &table, budget);
    assert!(
        !raw_violations.is_empty(),
        "guard-free kingdom shuffle produced no violating formation; \
         the safety pass would be untestable at this seed"
    );

    // Full pipeline (balanced-preset shape). No violation may survive.
    let mut patched = DiscPatcher::open(image.clone()).expect("open disc");
    let report = apply::randomize_encounters_full(
        &mut patched,
        seed,
        DropMode::Shuffle,
        EncounterScope::Kingdom,
        &[],
        Some(SoloStrongConfig::default()),
    )
    .expect("full pipeline");
    let after = violations(&patched, &table, budget);
    assert!(
        after.is_empty(),
        "patched image still has battle-load violations: {after:?}"
    );
    assert!(
        report.battle_load_capped > 0,
        "pass reported no capped formation although the raw shuffle violates"
    );

    // Every touched sector stays EDC/ECC-valid.
    let patched_image = patched.into_image();
    assert_eq!(patched_image.len(), image.len());
    for (i, (a, b)) in image
        .chunks(SECTOR_SIZE)
        .zip(patched_image.chunks(SECTOR_SIZE))
        .enumerate()
    {
        if a != b && !is_form2(b) {
            assert!(
                mode2_form1_sector_is_valid(b),
                "sector {i} invalid after patch"
            );
        }
    }

    // Fixed seed is byte-deterministic.
    let mut again = DiscPatcher::open(image).expect("open disc");
    apply::randomize_encounters_full(
        &mut again,
        seed,
        DropMode::Shuffle,
        EncounterScope::Kingdom,
        &[],
        Some(SoloStrongConfig::default()),
    )
    .expect("full pipeline (repeat)");
    assert!(
        again.into_image() == patched_image,
        "same seed produced different bytes"
    );
}
