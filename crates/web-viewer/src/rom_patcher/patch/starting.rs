//! The new game's starting bag and level.

use super::*;

pub(super) fn starting_items(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions {
        starting_items,
        door_of_wind,
        incense,
        speed_chain,
        chicken_heart,
        good_luck_bell,
        all_warps,
        ..
    } = *run.opts;
    let seed_opts = legaia_patcher::starting_items::StartingSeedOptions {
        random_items: starting_items,
        door_of_wind,
        incense,
        speed_chain,
        chicken_heart,
        good_luck_bell,
        all_warps,
        // The in-browser patcher doesn't surface explicit item picks yet; the CLI
        // `--start-with` flag does. Leave it empty so web behaviour is unchanged.
        extra_items: Vec::new(),
    };
    if seed_opts.is_active() {
        let rep = apply::randomize_starting_items(&mut run.patcher, run.seed, &seed_opts)
            .map_err(|e| format!("starting-items: {e}"))?;
        // With a random fill requested, the seeded bag contains seed-derived
        // draws - listing their names would spoil the run before it starts.
        // Only the convenience toggles (which the user picked themselves) are
        // ever named; a randomized bag is reported count-only.
        if starting_items > 0 {
            run.summary.push_str(&format!(
                "starting-items: new game begins with {} item(s) (randomized - names hidden, no spoilers)\n",
                rep.items_set
            ));
        } else {
            let names = legaia_iso::iso9660::read_file_in_image(run.patcher.image(), "SCUS_942.54")
                .and_then(|scus| legaia_game_tables::item_names::ItemNameTable::from_scus(&scus));
            let list: Vec<String> = rep
                .items
                .iter()
                .map(|(id, count)| {
                    let nm = names.as_ref().and_then(|t| t.name(*id)).unwrap_or("?");
                    format!("{count}x {nm}")
                })
                .collect();
            run.summary.push_str(&format!(
                "starting-items: new game begins with {} item(s): {}\n",
                rep.items_set,
                list.join(", ")
            ));
        }
        if rep.all_warps {
            run.summary
                .push_str("all-warps: every Door of Wind destination unlocked from the start\n");
        }
        // Items beyond the 7-slot direct-seed cap are granted on top via a silent
        // GIVE_ITEM block injected into the opening scene (see `starting_bag`), so
        // the explicit convenience items AND the full requested random fill land.
        let overflow = legaia_patcher::starting_items::overflow_bag(run.seed, &seed_opts);
        if !overflow.is_empty() {
            let bag = apply::apply_starting_bag(
                &mut run.patcher,
                &overflow,
                legaia_patcher::starting_bag::DEFAULT_GUARD_BIT,
            )
            .map_err(|e| format!("starting-items overflow: {e}"))?;
            if bag.applied {
                // Overflow slots are always part of the random fill - count
                // only, same no-spoiler rule as the direct seed above.
                run.summary.push_str(&format!(
                    "starting-items: + {} more via the opening scene\n",
                    overflow.len()
                ));
            } else {
                run.summary.push_str(&format!(
                    "starting-items: WARNING - {} overflow item(s) could not be injected; \
                     bag truncated to the direct seed\n",
                    overflow.len()
                ));
            }
        }
    } else {
        run.summary
            .push_str("starting-items: untouched (vanilla Healing Leaf x5)\n");
    }
    Ok(())
}

pub(super) fn starting_level(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions { starting_level, .. } = *run.opts;
    if legaia_patcher::starting_level::is_active(starting_level) {
        let rep = apply::apply_starting_level(&mut run.patcher, starting_level)
            .map_err(|e| format!("starting-level: {e}"))?;
        run.summary.push_str(&format!(
            "starting-level: starting party begins at level {} ({} slot(s) leveled; \
             lead HP {}, MP {}, ATK {})\n",
            rep.level, rep.slots_leveled, rep.stats[0], rep.stats[1], rep.stats[3]
        ));
    } else {
        run.summary
            .push_str("starting-level: untouched (vanilla level 1)\n");
    }
    Ok(())
}
