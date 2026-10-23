//! World stages: prize prices, place names, Seru trading, encounters, chests, shops, the casino and doors.

use anyhow::{Context, Result};

use legaia_patcher::apply;
use legaia_patcher::drops::DropMode;

use super::Run;
use crate::cli::mode_str;
use crate::flag_values::parse_item_id;

/// Fishing-exchange price edits: set the point cost of one or more prizes
/// (e.g. the Buma Water Egg). Seedless targeted edits in the raw PROT 972
/// overlay; the price also gates when the prize appears.
pub(super) fn fishing_price(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    for &(item_id, price) in &args.fishing_price {
        let report = apply::set_fishing_price(patcher, item_id as u32, price)?;
        if report.edits.is_empty() {
            println!("fishing-price: item 0x{item_id:02X} already costs {price} points");
        } else {
            for (page, _row, _id, old, new) in &report.edits {
                let venue = if *page == 0 { "Buma" } else { "Vidna" };
                println!("fishing-price: {venue} item 0x{item_id:02X}: {old} -> {new} points");
            }
        }
        manifest.push(format!("fishing_price 0x{item_id:02X} = {price}"));
    }
    Ok(())
}

/// Earth Egg coin-threshold edit: rewrite the Sol Tower Prize Counter's
/// scripted coin gate + debit (koin1 MAN). Seedless targeted edit.
pub(super) fn earth_egg_price(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if let Some(price) = args.earth_egg_price {
        let report = apply::set_earth_egg_price(patcher, price)?;
        if report.changed {
            println!(
                "earth-egg-price: {} -> {} coins",
                report.old_price, report.new_price
            );
        } else {
            println!("earth-egg-price: already {} coins", report.new_price);
        }
        manifest.push(format!("earth_egg_price = {price}"));
    }
    Ok(())
}

/// Fortress encounter fix: both Absolute Fortress scene MANs stop reading the
/// Floating Castle's stale "no battles" flag. Seedless; runs ahead of the
/// randomizers that re-pack the same MANs, which then carry it forward.
pub(super) fn fortress_encounter_fix(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if args.fortress_encounter_fix {
        let report = apply::apply_fortress_encounter_fix(patcher)?;
        println!(
            "fortress-encounter-fix: {}",
            if report.changed() {
                "Absolute Fortress encounter tables patched (the Floating Castle's stale flag is ignored)"
            } else {
                "already applied"
            }
        );
        manifest.push("fortress_encounter_fix = true".to_string());
    } else {
        manifest.push("fortress_encounter_fix = false".to_string());
    }
    Ok(())
}

/// Conkram mirror freeze fix: the move-VM parts' pose-block and render-node
/// allocations survive a failure without writing through NULL or bumping
/// `PRG ERR`. Seedless - four same-size SCUS edits.
pub(super) fn conkram_freeze_fix(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if args.conkram_freeze_fix {
        let report = apply::apply_conkram_freeze_fix(patcher)?;
        println!(
            "conkram-freeze-fix: {}",
            if report.changed {
                "move VM patched (a failed effect allocation drops or stands in for the part instead of writing through NULL; PRG ERR stays clear)"
            } else {
                "already applied (no change)"
            }
        );
        manifest.push("conkram_freeze_fix = true".to_string());
    } else {
        manifest.push("conkram_freeze_fix = false".to_string());
    }
    Ok(())
}

/// Place renames: the SCUS landmark cell, the world-map label records, and
/// every scene MAN whose entry banner carries the name.
pub(super) fn rename_location(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if !args.rename_location.is_empty() {
        // Manifest the *requests*, not the per-cell outcome: a name-keyed
        // rename has no cell index, and the request is what reproduces the run.
        for (target, new) in &args.rename_location {
            let key = match target {
                apply::RenameTarget::Index(i) => i.to_string(),
                apply::RenameTarget::Name(n) => format!("{n:?}"),
            };
            manifest.push(format!("rename_location {key} = {new:?}"));
        }
        let report = apply::rename_locations_by_target(patcher, &args.rename_location)?;
        for (idx, old, new) in &report.renames {
            println!("rename-location: landmark {idx} {old:?} -> {new:?}");
        }
        println!(
            "rename-location: {} world-map label(s), {} scene banner(s), {} scene bundle(s) rewritten",
            report.world_map_records,
            report.scene_banners,
            report.entries_changed.len()
        );
        for name in &report.unmatched {
            println!("rename-location: {name:?} matched no place on this disc");
        }
        for idx in &report.skipped {
            println!(
                "rename-location: PROT entry {idx} left vanilla (re-packed MAN would not fit)"
            );
        }
        if report.is_empty() {
            println!("rename-location: nothing changed (names already match)");
        }
    }
    Ok(())
}

/// Seru trading, both halves: the retail in-shop vendor (hand-assembled
/// trade screen + seed-derived bucket schedule, all hosted in menu overlay
/// 0899 - runs on real hardware) and the SCUS config blob (enabled flag +
/// the same seed) the engine reads, so a patched disc trades
/// identically in an emulator and in the engine.
pub(super) fn seru_trade(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let seed = run.seed;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if args.seru_trade {
        apply::inject_trade_full(patcher, seed)?;
        let report = apply::enable_seru_trades(patcher, seed, args.seru_trade_offers)?;
        println!(
            "seru-trade: in-shop Seru trading vendor enabled (offer reseeds on a play-time bucket)"
        );
        manifest.push("seru_trade = true".to_string());
        manifest.push(format!("seru_trade_offers = {}", report.config.max_offers));
    } else {
        manifest.push("seru_trade = false".to_string());
    }
    Ok(())
}

/// Random-encounter formations (`--encounters`), with the solo-strong pass.
pub(super) fn encounters(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let seed = run.seed;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    let unused_enemies = run.unused_enemies;
    let enc_mode = args.encounters.mode();
    if let Some(enc_mode) = enc_mode {
        let scope = args.encounter_scope.scope();
        // Solo-strong is ON by default for any encounter randomization; opt out
        // with --no-solo-strong-encounters.
        let solo = (!args.no_solo_strong_encounters).then_some(apply::SoloStrongConfig {
            threshold_pct: args.solo_strong_threshold,
        });
        let report =
            apply::randomize_encounters_full(patcher, seed, enc_mode, scope, unused_enemies, solo)?;
        println!(
            "encounters: {} scenes rewritten, {} ids changed ({} {})",
            report.scenes_changed,
            report.ids_changed,
            args.encounter_scope.as_str(),
            mode_str(enc_mode)
        );
        if solo.is_some() {
            println!(
                "  solo-strong: {} formation(s) forced to a lone enemy (>= {}% of area average)",
                report.solo_collapsed, args.solo_strong_threshold
            );
            manifest.push("encounters_solo_strong = true".to_string());
            manifest.push(format!(
                "encounters_solo_strong_threshold = {}",
                args.solo_strong_threshold
            ));
            manifest.push(format!(
                "encounters_solo_collapsed = {}",
                report.solo_collapsed
            ));
        } else {
            println!("  solo-strong: off (over-strong packs left as randomized)");
            manifest.push("encounters_solo_strong = false".to_string());
        }
        manifest.push(format!(
            "encounters_scope = {:?}",
            args.encounter_scope.as_str()
        ));
        if report.unused_placed > 0 {
            println!(
                "  including {} unused-enemy spawn(s) injected",
                report.unused_placed
            );
            manifest.push(format!(
                "encounters_unused_placed = {}",
                report.unused_placed
            ));
        }
        if report.battle_load_capped > 0 {
            println!(
                "  battle-load cap: {} formation(s) reduced (distinct-species / heap-cost limit)",
                report.battle_load_capped
            );
            manifest.push(format!(
                "encounters_battle_load_capped = {}",
                report.battle_load_capped
            ));
        }
        manifest.push(format!("encounters = {:?}", mode_str(enc_mode)));
        manifest.push(format!(
            "encounters_scenes_changed = {}",
            report.scenes_changed
        ));
        manifest.push(format!("encounters_ids_changed = {}", report.ids_changed));
        if !report.skipped.is_empty() {
            println!(
                "  note: {} scene MAN(s) too tight to re-pack, left unchanged: {:?}",
                report.skipped.len(),
                report.skipped
            );
            manifest.push(format!("encounters_skipped = {:?}", report.skipped));
        }
    } else {
        println!("encounters: untouched");
        manifest.push("encounters = \"none\"".to_string());
    }
    Ok(())
}

/// Treasure-chest contents (`--chests`).
pub(super) fn chests(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let seed = run.seed;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    let pool = &run.pool;
    let chest_mode = args.chests.mode();
    if let Some(chest_mode) = chest_mode {
        // Resolve the keep-static set: the disc-derived quest/key/story set
        // (every unsellable quest item, so none is ever moved or randomly
        // placed), or the user's explicit (possibly empty) override.
        let keep_static: std::collections::BTreeSet<u8> = match &args.keep_static_items {
            None => {
                let scus = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
                    .context("SCUS_942.54 not found in disc image (needed for chest defaults)")?;
                legaia_patcher::items::default_static_chest_items(&scus)
            }
            Some(list) => list
                .iter()
                .filter(|s| !s.trim().is_empty())
                .map(|s| parse_item_id(s))
                .collect::<Result<_>>()?,
        };
        let report = apply::randomize_chests(patcher, pool, seed, chest_mode, &keep_static)?;
        println!(
            "chests: {} of {} sites changed across {} scenes ({:?}); {} item id(s) kept static",
            report.items_changed,
            report.sites_total,
            report.scenes_changed,
            chest_mode,
            keep_static.len()
        );
        manifest.push(format!("chests = {:?}", mode_str(chest_mode)));
        manifest.push(format!(
            "chests_keep_static = {:?}",
            keep_static
                .iter()
                .map(|id| format!("0x{id:02x}"))
                .collect::<Vec<_>>()
        ));
        manifest.push(format!("chests_sites = {}", report.sites_total));
        manifest.push(format!("chests_items_changed = {}", report.items_changed));
        if !report.skipped.is_empty() {
            println!(
                "  note: {} scene MAN(s) too tight to re-pack, left unchanged: {:?}",
                report.skipped.len(),
                report.skipped
            );
            manifest.push(format!("chests_skipped = {:?}", report.skipped));
        }
    } else {
        println!("chests: untouched");
        manifest.push("chests = \"none\"".to_string());
    }
    Ok(())
}

/// Town-merchant stock (`--shops`).
pub(super) fn shops(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let seed = run.seed;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    let shop_mode = args.shops.mode();
    if let Some(shop_mode) = shop_mode {
        let report = apply::randomize_shops(patcher, seed, shop_mode)?;
        println!(
            "shops: {} of {} town-shop item slots changed across {} scenes ({:?})",
            report.items_changed, report.slots_total, report.scenes_changed, shop_mode
        );
        manifest.push(format!("shops = {:?}", mode_str(shop_mode)));
        manifest.push(format!("shops_slots = {}", report.slots_total));
        manifest.push(format!("shops_items_changed = {}", report.items_changed));
        if !report.skipped.is_empty() {
            println!(
                "  note: {} scene MAN(s) too tight to re-pack, left unchanged: {:?}",
                report.skipped.len(),
                report.skipped
            );
            manifest.push(format!("shops_skipped = {:?}", report.skipped));
        }
    } else {
        println!("shops: untouched");
        manifest.push("shops = \"none\"".to_string());
    }
    Ok(())
}

/// Casino prize exchange (`--casino`).
pub(super) fn casino(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let seed = run.seed;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    let casino_mode = args.casino.mode();
    if let Some(casino_mode) = casino_mode {
        let changed = apply::randomize_casino(patcher, seed, casino_mode)?;
        println!("casino: {changed} prize slot(s) changed ({casino_mode:?})");
        manifest.push(format!("casino = {:?}", mode_str(casino_mode)));
        manifest.push(format!("casino_changed = {changed}"));
    } else {
        println!("casino: untouched");
        manifest.push("casino = \"none\"".to_string());
    }
    Ok(())
}

/// Scene-transition doors (`--doors`).
pub(super) fn doors(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let seed = run.seed;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    let door_mode = args.doors.mode();
    if let Some(door_mode) = door_mode {
        let coupling = args.door_coupling.coupling();
        let report = apply::randomize_doors(patcher, seed, door_mode, coupling)?;
        let coupling_str = match coupling {
            apply::DoorCoupling::Coupled => "coupled",
            apply::DoorCoupling::Decoupled => "decoupled",
        };
        println!(
            "doors: {} of {} walk-door sites changed across {} scenes ({:?}, {coupling_str}); \
             {} script/cutscene + {} world-map transitions kept vanilla",
            report.sites_changed,
            report.sites_total,
            report.scenes_changed,
            door_mode,
            report.excluded_script,
            report.excluded_world_map
        );
        manifest.push(format!("doors = {:?}", mode_str(door_mode)));
        manifest.push(format!("door_coupling = {coupling_str:?}"));
        manifest.push(format!("doors_sites = {}", report.sites_total));
        manifest.push(format!("doors_sites_changed = {}", report.sites_changed));
        manifest.push(format!(
            "doors_excluded_script = {}",
            report.excluded_script
        ));
        manifest.push(format!(
            "doors_excluded_world_map = {}",
            report.excluded_world_map
        ));
        if report.unpaired > 0 {
            manifest.push(format!("doors_unpaired = {}", report.unpaired));
        }
        if report.coupled_kept_original > 0 {
            println!(
                "  note: {} door(s) kept their original destination because a scene on \
                 their connection couldn't be grown in place (so the return trip stays correct)",
                report.coupled_kept_original
            );
            manifest.push(format!(
                "doors_coupled_kept_original = {}",
                report.coupled_kept_original
            ));
        }
        if !report.skipped.is_empty() {
            println!(
                "  note: {} scene MAN(s) overflowed on rebuild, left unchanged: {:?}",
                report.skipped.len(),
                report.skipped
            );
            manifest.push(format!("doors_skipped = {:?}", report.skipped));
        }
    } else {
        println!("doors: untouched");
        manifest.push("doors = \"none\"".to_string());
    }
    Ok(())
}

/// Intra-town doors and the `.MAP` kind-0 teleports (`--house-doors`).
pub(super) fn house_doors(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let seed = run.seed;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if let Some(hd_mode) = args.house_doors.mode() {
        let report = apply::randomize_house_doors(patcher, seed, hd_mode)?;
        if hd_mode == DropMode::Shuffle {
            println!(
                "house-doors: {} of {} door-warp targets shuffled across {} scenes",
                report.sites_changed, report.sites_total, report.scenes_changed
            );
            manifest.push("house_doors = \"shuffle\"".to_string());
            manifest.push(format!("house_doors_sites = {}", report.sites_total));
            manifest.push(format!("house_doors_changed = {}", report.sites_changed));
            if !report.skipped.is_empty() {
                println!(
                    "  note: {} scene MAN(s) too tight to re-pack, left unchanged: {:?}",
                    report.skipped.len(),
                    report.skipped
                );
                manifest.push(format!("house_doors_skipped = {:?}", report.skipped));
            }
            // The `.MAP` kind-0 intra-scene teleports (most house exits) run
            // under the same option - see `legaia_patcher::map_door`.
            let map = &report.map;
            println!(
                "map-doors: {} of {} kind-0 teleports rewired across {} scenes \
                 ({} unattributed kept vanilla)",
                map.sites_changed, map.sites_total, map.scenes_changed, map.kept_static
            );
            if map.scenes_unverified > 0 {
                println!(
                    "  note: {} scene(s) had no reachability-verified permutation, kept vanilla",
                    map.scenes_unverified
                );
            }
            manifest.push(format!("map_doors_sites = {}", map.sites_total));
            manifest.push(format!("map_doors_changed = {}", map.sites_changed));
            if map.scenes_unverified > 0 {
                manifest.push(format!(
                    "map_doors_scenes_unverified = {}",
                    map.scenes_unverified
                ));
            }
            // Spoiler log: one line per rewired teleport.
            for r in &map.rewires {
                manifest.push(format!(
                    "map_door = \"{}#{} tile ({},{}): dest ({},{}) -> ({},{})\"",
                    r.scene, r.entry_idx, r.tile.0, r.tile.1, r.from.0, r.from.1, r.to.0, r.to.1
                ));
            }
        } else {
            println!(
                "house-doors: only `shuffle` is supported (random would place the player off-map); untouched"
            );
            manifest.push("house_doors = \"none\"".to_string());
        }
    } else {
        println!("house-doors: untouched");
        manifest.push("house_doors = \"none\"".to_string());
    }
    Ok(())
}
