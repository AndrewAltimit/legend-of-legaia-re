//! New-game stages: the starting bag and the starting level.

use anyhow::Result;

use legaia_patcher::apply;

use super::Run;

/// The new game's starting bag (`--starting-items` and the convenience-item toggles).
pub(super) fn starting_items(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let seed = run.seed;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    let seed_opts = legaia_patcher::starting_items::StartingSeedOptions {
        random_items: args.starting_items,
        door_of_wind: args.door_of_wind.unwrap_or(0),
        incense: args.incense.unwrap_or(0),
        speed_chain: args.speed_chain.unwrap_or(0),
        chicken_heart: args.chicken_heart.unwrap_or(0),
        good_luck_bell: args.good_luck_bell.unwrap_or(0),
        all_warps: args.all_warps,
        extra_items: args.start_with.clone(),
    };
    if seed_opts.is_active() {
        let report = apply::randomize_starting_items(patcher, seed, &seed_opts)?;
        let names = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
            .and_then(|scus| legaia_game_tables::item_names::ItemNameTable::from_scus(&scus));
        let item_name = |id: u8| -> String {
            names
                .as_ref()
                .and_then(|t| t.name(id))
                .unwrap_or("?")
                .to_string()
        };
        let summary: Vec<String> = report
            .items
            .iter()
            .map(|(id, count)| format!("{count}x {}", item_name(*id)))
            .collect();
        println!(
            "starting-items: new game now begins with {} item(s): {}",
            report.items_set,
            summary.join(", ")
        );
        if report.all_warps {
            println!("all-warps: every Door of Wind destination unlocked from the start");
        }
        manifest.push(format!("starting_items = {}", report.items_set));
        manifest.push(format!("starting_items_set = {:?}", report.items));
        manifest.push(format!("all_warps = {}", report.all_warps));

        // Items beyond the direct seed's 7-slot cap (the bag would otherwise be
        // truncated) are granted on top via a silent GIVE_ITEM block injected into
        // the opening scene's script, so the explicit convenience items AND the full
        // requested random fill all land. See `starting_bag`.
        let overflow = legaia_patcher::starting_items::overflow_bag(seed, &seed_opts);
        if !overflow.is_empty() {
            let guard = legaia_patcher::starting_bag::DEFAULT_GUARD_BIT;
            let bag = apply::apply_starting_bag(patcher, &overflow, guard)?;
            let extra: Vec<String> = overflow
                .iter()
                .map(|(id, count)| format!("{count}x {}", item_name(*id)))
                .collect();
            if bag.applied {
                println!(
                    "starting-items: + {} more item(s) granted via the opening scene: {}",
                    overflow.len(),
                    extra.join(", ")
                );
                manifest.push(format!("starting_items_overflow = {:?}", overflow));
                manifest.push(format!("starting_items_overflow_guard_bit = {guard:#x}"));
            } else {
                println!(
                    "starting-items: WARNING - {} overflow item(s) could NOT be injected \
                     (opening scene not patchable); bag truncated to the {}-slot direct seed",
                    overflow.len(),
                    report.items_set
                );
            }
        }
    } else {
        println!("starting-items: untouched (vanilla Healing Leaf x5)");
        manifest.push("starting_items = 0".to_string());
        manifest.push("all_warps = false".to_string());
    }
    Ok(())
}

/// The new game's starting level (`--starting-level`).
pub(super) fn starting_level(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if legaia_patcher::starting_level::is_active(args.starting_level) {
        let report = apply::apply_starting_level(patcher, args.starting_level)?;
        println!(
            "starting-level: new game now begins at level {} for the starting party \
             ({} slot(s) leveled; lead HP {}, ATK {})",
            report.level, report.slots_leveled, report.stats[0], report.stats[3]
        );
        manifest.push(format!("starting_level = {}", report.level));
    } else {
        println!("starting-level: untouched (vanilla level 1)");
        manifest.push("starting_level = 0".to_string());
    }
    Ok(())
}
