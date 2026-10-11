//! The random-fill item pool and the content toggles that widen it.

use anyhow::{Context, Result};

use legaia_patcher::apply;
use legaia_patcher::drops::DropMode;
use legaia_patcher::items::valid_item_pool;

use super::Run;

/// Build the random-fill item pool and apply the content toggles that widen
/// it (`--unused-items`, `--custom-items`), and resolve the unused-enemy id
/// set the encounter stage takes. Returns `(pool, unused_enemies)`.
///
/// The valid item pool (from SCUS) is needed only by the `random` modes.
/// Shops build their own sellable pool internally (priced items), so they
/// don't need the general valid-item pool.
pub(super) fn item_pool(run: &mut Run<'_>) -> Result<(Vec<u8>, &'static [u8])> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    let mode = args.drops.mode();
    let chest_mode = args.chests.mode();
    let steal_mode = args.steals.mode();
    let needs_pool = mode == Some(DropMode::Random)
        || chest_mode == Some(DropMode::Random)
        || steal_mode == Some(DropMode::Random);
    let mut pool = if needs_pool {
        let scus = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
            .context("SCUS_942.54 not found in disc image (needed for a `random` mode)")?;
        valid_item_pool(&scus).context("build valid item pool from SCUS")?
    } else {
        Vec::new()
    };
    // `--unused-items` widens the random-fill pool with the curated unused items
    // (the unnamed accessory in particular is otherwise excluded - it has no
    // name). It only matters for the `random` modes, which are the pool's only
    // consumers; warn if it can't take effect.
    if args.unused_items {
        if needs_pool {
            legaia_patcher::unused::extend_pool(&mut pool, legaia_patcher::unused::UNUSED_ITEM_IDS);
            // Name the otherwise-blank accessory so it shows as "Seru Bell"
            // wherever it lands.
            if let Some(name) = apply::inject_seru_bell_name(patcher)? {
                println!("unused-items: named the unnamed accessory (0xFD) \"{name}\"");
                manifest.push(format!("unused_item_name = {name:?}"));
            }
        } else {
            println!("note: --unused-items has no effect without a `random` drop/chest/steal mode");
        }
        manifest.push(format!("unused_items = {}", args.unused_items));
    }
    // `--custom-items` injects the three brand-new items (Nature's Elixir /
    // Ra-Seru Tear / Fury Bloom) standalone - records, effect machinery, and
    // battle hooks; no Delilas dependency - and widens the random-fill pool
    // with them so the `random` drop/chest/steal modes can hand them out.
    // With `--delilas-challenge` they additionally become the course's
    // full-clear reward (the grant half is installed there).
    if args.custom_items {
        match apply::inject_custom_item_set(patcher) {
            Ok(true) => println!(
                "custom-items: injected Nature's Elixir (0xB9), Ra-Seru Tear (0x12), \
                 Fury Bloom (0x1A)"
            ),
            Ok(false) => println!("custom-items: already injected"),
            Err(e) => {
                anyhow::bail!("custom-items: {e:#}");
            }
        }
        if needs_pool {
            legaia_patcher::unused::extend_pool(
                &mut pool,
                legaia_delilas_challenge::custom_items::CUSTOM_ITEM_IDS,
            );
        } else if !args.delilas_challenge {
            println!(
                "note: without a `random` drop/chest/steal mode or --delilas-challenge, \
                 the custom items exist but nothing hands them out"
            );
        }
        manifest.push(format!("custom_items = {}", args.custom_items));
    }
    // The unused-enemy id set (empty unless the toggle is on) is passed to the
    // encounter randomizer below.
    let unused_enemies: &[u8] = if args.unused_enemies {
        if args.encounters.mode() != Some(DropMode::Random) {
            println!("note: --unused-enemies only takes effect with `--encounters random`");
        }
        manifest.push(format!("unused_enemies = {}", args.unused_enemies));
        legaia_patcher::unused::UNUSED_ENEMY_IDS
    } else {
        &[]
    };
    Ok((pool, unused_enemies))
}
