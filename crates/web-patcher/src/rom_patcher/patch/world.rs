//! Scene and shop data: encounters, chests, shops, casino, doors, prices, place names, Seru trading.

use super::*;

pub(super) fn encounters(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions {
        encounters,
        encounter_scope,
        solo_strong_encounters,
        ..
    } = *run.opts;
    match parse_mode(encounters) {
        Some(m) => {
            let scope = parse_encounter_scope(encounter_scope);
            let solo = solo_strong_encounters.then(apply::SoloStrongConfig::default);
            let rep = apply::randomize_encounters_full(
                &mut run.patcher,
                run.seed,
                m,
                scope,
                run.unused_enemy_ids,
                solo,
            )
            .map_err(|e| format!("encounters: {e}"))?;
            run.summary.push_str(&format!(
                "encounters: {} scenes, {} ids changed ({} {})\n",
                rep.scenes_changed, rep.ids_changed, encounter_scope, encounters
            ));
            if rep.unused_placed > 0 {
                run.summary.push_str(&format!(
                    "  including {} unused-enemy spawn(s) injected\n",
                    rep.unused_placed
                ));
            }
            if solo.is_some() {
                run.summary.push_str(&format!(
                    "  solo-strong: {} strong fight(s) forced to a lone enemy\n",
                    rep.solo_collapsed
                ));
            }
            if rep.battle_load_capped > 0 {
                run.summary.push_str(&format!(
                    "  battle-load cap: {} formation(s) reduced to fit the battle heap\n",
                    rep.battle_load_capped
                ));
            }
        }
        None => run.summary.push_str("encounters: untouched\n"),
    }
    Ok(())
}

/// Seru trading: a vendor in shops offers to trade a party member's Seru-magic for
/// a different one (time-bucketed, deterministic from the seed). All code + data is
/// hosted in the menu overlay, so it composes with every other feature here.
pub(super) fn seru_trade(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions { seru_trade, .. } = *run.opts;
    if seru_trade {
        apply::inject_trade_full(&mut run.patcher, run.seed)
            .map_err(|e| format!("seru-trade: {e}"))?;
        // Also embed the engine-facing config blob (same seed), so the patched
        // disc trades identically when booted in the engine.
        apply::enable_seru_trades(
            &mut run.patcher,
            run.seed,
            legaia_game_tables::seru_trade::DEFAULT_MAX_OFFERS,
        )
        .map_err(|e| format!("seru-trade config: {e}"))?;
        run.summary
            .push_str("seru-trade: in-shop Seru trading vendor enabled\n");
    } else {
        run.summary.push_str("seru-trade: untouched\n");
    }
    Ok(())
}

/// Fishing-exchange price edits: a comma/semicolon/whitespace-separated list
/// of `item=points` pairs (item id decimal or 0xHH). Each sets the fishing
/// point cost of every prize row granting that item; the price also gates
/// when the prize appears. A malformed pair is reported and skipped rather
/// than aborting the whole patch.
pub(super) fn fishing_prices(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions { fishing_prices, .. } = *run.opts;
    let fishing_prices = fishing_prices.trim();
    if fishing_prices.is_empty() {
        run.summary.push_str("fishing-price: untouched\n");
    } else {
        for tok in fishing_prices
            .split([',', ';', '\n', ' '])
            .filter(|t| !t.trim().is_empty())
        {
            match parse_id_eq_u32(tok) {
                Some((item_id, price)) => {
                    match apply::set_fishing_price(&mut run.patcher, item_id as u32, price) {
                        Ok(rep) if rep.edits.is_empty() => run.summary.push_str(&format!(
                            "fishing-price: item 0x{item_id:02X} already {price} points\n"
                        )),
                        Ok(rep) => {
                            for (page, _row, _id, old, new) in &rep.edits {
                                let venue = if *page == 0 { "Buma" } else { "Vidna" };
                                run.summary.push_str(&format!(
                                    "fishing-price: {venue} item 0x{item_id:02X}: {old} -> {new} points\n"
                                ));
                            }
                        }
                        Err(e) => run.summary.push_str(&format!("fishing-price: {e}\n")),
                    }
                }
                None => run
                    .summary
                    .push_str(&format!("fishing-price: skipped malformed entry {tok:?}\n")),
            }
        }
    }
    Ok(())
}

/// Earth Egg coin threshold: the Sol Tower Prize Counter's scripted
/// coin-for-Earth-Egg exchange (koin1 MAN). A single coins-required value
/// (empty = untouched); the game debits exactly that many on purchase.
pub(super) fn earth_egg_price(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions {
        earth_egg_price, ..
    } = *run.opts;
    let earth_egg_price = earth_egg_price.trim();
    if earth_egg_price.is_empty() {
        run.summary.push_str("earth-egg-price: untouched\n");
    } else {
        match earth_egg_price.parse::<u32>() {
            Ok(price) => match apply::set_earth_egg_price(&mut run.patcher, price) {
                Ok(rep) if !rep.changed => run
                    .summary
                    .push_str(&format!("earth-egg-price: already {price} coins\n")),
                Ok(rep) => run.summary.push_str(&format!(
                    "earth-egg-price: {} -> {} coins\n",
                    rep.old_price, rep.new_price
                )),
                Err(e) => run.summary.push_str(&format!("earth-egg-price: {e}\n")),
            },
            Err(_) => run.summary.push_str(&format!(
                "earth-egg-price: skipped non-numeric value {earth_egg_price:?}\n"
            )),
        }
    }
    Ok(())
}

/// Fortress encounter fix: both Absolute Fortress scene MANs (`deroa`,
/// `chitei2`) stop reading the Floating Castle's stale "no battles" flag, so
/// a save that left the castle with under a minute on the clock rolls
/// battles there again. Seedless; a failure aborts the run, like the other
/// fixes, rather than shipping an image without the fix that was asked for.
pub(super) fn fortress_encounter_fix(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions {
        fortress_encounter_fix,
        ..
    } = *run.opts;
    if fortress_encounter_fix {
        let rep = apply::apply_fortress_encounter_fix(&mut run.patcher)
            .map_err(|e| format!("fortress-encounter-fix: {e:#}"))?;
        run.summary.push_str(if rep.changed() {
            "fortress-encounter-fix: Absolute Fortress encounter tables patched (the Floating Castle's stale flag is ignored)\n"
        } else {
            "fortress-encounter-fix: already applied\n"
        });
    } else {
        run.summary.push_str("fortress-encounter-fix: untouched\n");
    }
    Ok(())
}

/// Conkram mirror freeze fix: the move-VM parts' pose-block and render-node
/// allocations survive a failure without writing through NULL (the Room of
/// Mirrors lock-up on a fast emulated CPU) or leaving `PRG ERR` on screen.
/// Seedless; four same-size `SCUS_942.54` edits.
pub(super) fn conkram_freeze_fix(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions {
        conkram_freeze_fix, ..
    } = *run.opts;
    if conkram_freeze_fix {
        let rep = apply::apply_conkram_freeze_fix(&mut run.patcher)
            .map_err(|e| format!("conkram-freeze-fix: {e:#}"))?;
        run.summary.push_str(if rep.changed {
            "conkram-freeze-fix: move VM patched (a failed effect allocation no longer freezes the game or shows PRG ERR)\n"
        } else {
            "conkram-freeze-fix: already applied\n"
        });
    } else {
        run.summary.push_str("conkram-freeze-fix: untouched\n");
    }
    Ok(())
}

/// Place renames: newline-separated `target=name` lines (a name may contain
/// spaces, so only the newline splits entries). `target` is a landmark index
/// or the place's current name. Each rename propagates to all three carriers -
/// the SCUS quick-travel cell, the world-map labels, and the scene-entry
/// banners - so one line changes every place the game shows the name. A bad
/// entry is reported and skipped.
pub(super) fn location_renames(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions {
        location_renames, ..
    } = *run.opts;
    let location_renames = location_renames.trim();
    if location_renames.is_empty() {
        run.summary.push_str("rename-location: untouched\n");
    } else {
        let mut targets = Vec::new();
        for line in location_renames.lines().filter(|l| !l.trim().is_empty()) {
            match line.split_once('=') {
                Some((target, name)) if !target.trim().is_empty() => {
                    targets.push((apply::RenameTarget::parse(target), name.to_string()));
                }
                _ => run.summary.push_str(&format!(
                    "rename-location: skipped malformed entry {line:?}\n"
                )),
            }
        }
        match apply::rename_locations_by_target(&mut run.patcher, &targets) {
            Ok(rep) => {
                for (i, old, new) in &rep.renames {
                    run.summary.push_str(&format!(
                        "rename-location: landmark {i} {old:?} -> {new:?}\n"
                    ));
                }
                run.summary.push_str(&format!(
                    "rename-location: {} world-map label(s), {} scene banner(s)\n",
                    rep.world_map_records, rep.scene_banners
                ));
                for name in &rep.unmatched {
                    run.summary
                        .push_str(&format!("rename-location: {name:?} matched no place\n"));
                }
                for idx in &rep.skipped {
                    run.summary.push_str(&format!(
                        "rename-location: scene bundle {idx} left vanilla (would not fit)\n"
                    ));
                }
                if rep.is_empty() {
                    run.summary
                        .push_str("rename-location: nothing changed (names already match)\n");
                }
            }
            Err(e) => run.summary.push_str(&format!("rename-location: {e}\n")),
        }
    }
    Ok(())
}

pub(super) fn chests(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions { chests, .. } = *run.opts;
    match parse_mode(chests) {
        Some(m) => {
            // Protect every quest / key / story item by default (same disc-derived
            // set as the CLI), so the in-browser patcher behaves identically: no
            // quest item is moved out of its chest or dropped into another.
            let keep_static: std::collections::BTreeSet<u8> =
                match legaia_iso::iso9660::read_file_in_image(run.patcher.image(), "SCUS_942.54") {
                    Some(scus) => legaia_patcher::items::default_static_chest_items(&scus),
                    None => legaia_patcher::items::DEFAULT_STATIC_CHEST_ITEMS
                        .iter()
                        .copied()
                        .collect(),
                };
            let rep =
                apply::randomize_chests(&mut run.patcher, &run.pool, run.seed, m, &keep_static)
                    .map_err(|e| format!("chests: {e}"))?;
            run.summary.push_str(&format!(
                "chests: {} of {} sites changed across {} scenes ({}); {} kept static\n",
                rep.items_changed,
                rep.sites_total,
                rep.scenes_changed,
                chests,
                keep_static.len()
            ));
        }
        None => run.summary.push_str("chests: untouched\n"),
    }
    Ok(())
}

pub(super) fn shops(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions { shops, .. } = *run.opts;
    match parse_mode(shops) {
        Some(m) => {
            let rep = apply::randomize_shops(&mut run.patcher, run.seed, m)
                .map_err(|e| format!("shops: {e}"))?;
            run.summary.push_str(&format!(
                "shops: {} of {} town-shop slots changed across {} scenes ({})\n",
                rep.items_changed, rep.slots_total, rep.scenes_changed, shops
            ));
        }
        None => run.summary.push_str("shops: untouched\n"),
    }
    Ok(())
}

pub(super) fn casino(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions { casino, .. } = *run.opts;
    match parse_mode(casino) {
        Some(m) => {
            let changed = apply::randomize_casino(&mut run.patcher, run.seed, m)
                .map_err(|e| format!("casino: {e}"))?;
            run.summary.push_str(&format!(
                "casino: {changed} prize slot(s) changed ({casino})\n"
            ));
        }
        None => run.summary.push_str("casino: untouched\n"),
    }
    Ok(())
}

pub(super) fn doors(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions {
        doors,
        door_coupling,
        ..
    } = *run.opts;
    match parse_mode(doors) {
        Some(m) => {
            let coupling = match door_coupling {
                "decoupled" => apply::DoorCoupling::Decoupled,
                _ => apply::DoorCoupling::Coupled,
            };
            let rep = apply::randomize_doors(&mut run.patcher, run.seed, m, coupling)
                .map_err(|e| format!("doors: {e}"))?;
            run.summary.push_str(&format!(
                "doors: {} of {} sites changed across {} scenes ({}, {})\n",
                rep.sites_changed, rep.sites_total, rep.scenes_changed, doors, door_coupling
            ));
            if !rep.skipped.is_empty() {
                run.summary.push_str(&format!(
                    "  {} hub scene(s) too big to grow in place, kept original doors\n",
                    rep.skipped.len()
                ));
            }
        }
        None => run.summary.push_str("doors: untouched\n"),
    }
    Ok(())
}

pub(super) fn house_doors(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions { house_doors, .. } = *run.opts;
    match parse_mode(house_doors) {
        Some(legaia_patcher::drops::DropMode::Shuffle) => {
            let rep = apply::randomize_house_doors(
                &mut run.patcher,
                run.seed,
                legaia_patcher::drops::DropMode::Shuffle,
            )
            .map_err(|e| format!("house-doors: {e}"))?;
            run.summary.push_str(&format!(
                "house-doors: {} of {} door-warp targets shuffled across {} scenes\n",
                rep.sites_changed, rep.sites_total, rep.scenes_changed
            ));
            run.summary.push_str(&format!(
                "map-doors: {} of {} kind-0 teleports rewired across {} scenes\n",
                rep.map.sites_changed, rep.map.sites_total, rep.map.scenes_changed
            ));
        }
        Some(_) => run
            .summary
            .push_str("house-doors: only `shuffle` supported; untouched\n"),
        None => run.summary.push_str("house-doors: untouched\n"),
    }
    Ok(())
}
