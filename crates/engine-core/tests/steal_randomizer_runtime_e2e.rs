//! Disc-gated end-to-end oracle for the steal-item randomizer **at runtime** -
//! the fourth member of the randomizer runtime-oracle set (chest, monster-drop,
//! encounter, and now steal).
//!
//! The randomizer's own disc-gated test (`crates/rando/tests/steal_patch_real`)
//! proves a patched steal is *written* faithfully: the item byte in the static
//! `SCUS_942.54` steal table changes, every steal chance byte is untouched, and
//! the touched SCUS sector stays EDC/ECC-valid. What it does **not** prove is
//! that a runtime actually *reads the patched byte and grants the new item* on a
//! successful steal - the question behind "is it truly randomizing, or is
//! something serving a stale value?".
//!
//! A savestate can't answer that cleanly (the same trap the other oracles
//! document): a mednafen state snapshots all of RAM, and the steal table is
//! static rodata resident in RAM the moment the executable is loaded, so a state
//! captured on a patched disc still steals the *original* item from the cached
//! RAM copy. The patched value is only observed after a fresh executable load
//! re-reads the table off the (patched) disc.
//!
//! The clean-room engine sidesteps that cache entirely: it decodes the steal
//! table straight from the disc's `SCUS_942.54` bytes
//! ([`StealTable::from_scus`]) and runs the steal-grant kernel the randomizer's
//! edit feeds - the percent roll + inventory add in [`World::apply_steal`] (the
//! steal counterpart to the `apply_battle_loot` drop grant). So this test:
//!   1. picks a stealable monster (the Skeleton, id 13 → Incense, the same
//!      monster the live `player_steal_skeleton_*` capture pinned the table
//!      with) on a scratch copy of the real disc,
//!   2. patches only its steal **item** byte in `SCUS_942.54` to a distinct id
//!      (chance untouched - the surgical single-byte edit the randomizer makes),
//!   3. re-decodes the patched steal table off the patched image (the bytes a
//!      fresh executable load would stream),
//!   4. drives `apply_steal` for that monster and asserts the runtime grants the
//!      **patched** id into the bag and never the original.
//!
//! A baseline pass over the *unpatched* table first confirms the engine grants
//! that monster's original steal at all, so the patched assertion can't pass
//! vacuously. The world RNG is seeded so the steal roll always lands
//! (`next_rng() % 100 == 0`), keeping both passes comparable. Skips without
//! `LEGAIA_DISC_BIN` (CLAUDE.md convention).

use legaia_asset::steal_table::{self, StealTable};
use legaia_engine_core::world::World;
use legaia_rando::disc::DiscPatcher;

/// World RNG seed for which the first `apply_steal` roll is `0`
/// (`next_rng() % 100 == 0`), so the steal lands for any positive chance. A
/// fresh `World` per pass reuses it, so the baseline and patched rolls are
/// identical and the only variable is the disc byte.
const ROLL_LANDS_SEED: u32 = 32937;

/// keikoku-capture anchor: the Skeleton (monster id 13) steals Incense (0x8a) at
/// 30% - the table entry the live `player_steal_skeleton_*` save pair pinned.
const SKELETON_ID: u16 = 13;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Drive the steal-grant kernel for `monster_id` against `table` and return the
/// granted item id (or `None` on a miss). A fresh `World` keeps the inventory
/// clean and the RNG seeded so the roll lands; cross-checks the bag received it.
fn steal_for(monster_id: u16, table: &StealTable) -> Option<u8> {
    let mut world = World {
        rng_state: ROLL_LANDS_SEED,
        ..World::default()
    };
    let granted = world.apply_steal(monster_id, table);
    if let Some(item) = granted {
        assert!(
            world.inventory.get(&item).copied().unwrap_or(0) >= 1,
            "stolen item 0x{item:02x} must be in the bag"
        );
    }
    granted
}

#[test]
fn patched_steal_grants_new_item_at_runtime() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };

    // Item names purely for legible output (no Sony bytes are asserted).
    let scus = legaia_iso::iso9660::read_file_in_image(&disc, "SCUS_942.54").expect("SCUS");
    let names = legaia_asset::item_names::ItemNameTable::from_scus(&scus);
    let name_of = |id: u8| {
        names
            .as_ref()
            .and_then(|t| t.name(id))
            .unwrap_or("?")
            .to_string()
    };

    // --- The unpatched table + the chosen monster's original steal. ---
    let table = StealTable::from_scus(&scus).expect("parse steal table");
    let original = table
        .steal_item(SKELETON_ID)
        .expect("Skeleton must be stealable");
    let replacement = if original.wrapping_add(0x40) == 0 || original.wrapping_add(0x40) == original
    {
        0x42
    } else {
        original.wrapping_add(0x40)
    };

    // --- Baseline: the engine grants the ORIGINAL steal on the UNPATCHED table
    //     (proves the path is reachable, non-vacuous). ---
    let baseline = steal_for(SKELETON_ID, &table);
    assert_eq!(
        baseline,
        Some(original),
        "baseline: monster {SKELETON_ID} must steal its original 0x{original:02x}"
    );
    assert_ne!(
        original, replacement,
        "replacement must differ from original"
    );

    // --- Patch only the steal item byte (chance untouched) on a scratch disc,
    //     then re-decode the table off the PATCHED image. ---
    let item_off =
        steal_table::table_file_offset(&scus).expect("table offset") + SKELETON_ID as usize * 2 + 1;
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    patcher
        .patch_named_file("SCUS_942.54", item_off as u64, &[replacement])
        .expect("patch SCUS steal item");

    let patched_scus = patcher
        .read_named_file("SCUS_942.54")
        .expect("re-read SCUS");
    let patched_table = StealTable::from_scus(&patched_scus).expect("parse patched steal table");
    let patched_entry = patched_table.entry(SKELETON_ID).unwrap();
    assert_eq!(
        patched_entry.item_id, replacement,
        "patched disc bytes must carry the new steal id"
    );
    assert_eq!(
        patched_entry.chance_pct,
        table.entry(SKELETON_ID).unwrap().chance_pct,
        "steal chance must be untouched by the item patch"
    );

    // --- Runtime: drive the SAME monster through the steal-grant kernel on the
    //     patched table. It must grant the replacement and never the original. ---
    let runtime = steal_for(SKELETON_ID, &patched_table);
    assert_eq!(
        runtime,
        Some(replacement),
        "runtime grants the patched id 0x{replacement:02x} (monster {SKELETON_ID})"
    );
    assert_ne!(
        runtime,
        Some(original),
        "runtime must NOT grant the original 0x{original:02x} after the patch \
         - a stale value here is the caching failure this test guards"
    );

    eprintln!(
        "steal runtime E2E: monster {SKELETON_ID} baseline steals 0x{original:02x} ({}), \
         patched steals 0x{replacement:02x} ({}) - 0x{original:02x} -> 0x{replacement:02x}",
        name_of(original),
        name_of(replacement),
    );
}
