//! The `randomize` and `verify` subcommands: plan a full randomization from a
//! seed, diff the patched image into a PPF (+ optional patched `.bin` / `.cue` /
//! manifest), and apply-and-check an existing PPF.

use std::path::Path;

use anyhow::{Context, Result, bail};

use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::drops::DropMode;
use legaia_rando::items::valid_item_pool;
use legaia_rando::ppf;

use crate::cli::{RandomizeArgs, mode_str};
use crate::util::{
    clock_seed, cue_contents, load_image, parse_item_id, resolve_seed, with_extension,
};

pub(crate) fn cmd_randomize(args: RandomizeArgs) -> Result<()> {
    let seed = match &args.seed {
        Some(s) => resolve_seed(s),
        None => clock_seed(),
    };
    let original = load_image(&args.input)?;
    let mut patcher = DiscPatcher::open(original.clone()).context("parse disc image")?;

    let mode = args.drops.mode();
    let enc_mode = args.encounters.mode();
    let chest_mode = args.chests.mode();
    let steal_mode = args.steals.mode();
    let arts_mode = args.arts.mode().map(|m| match m {
        DropMode::Shuffle => legaia_rando::arts::ArtsMode::Shuffle,
        DropMode::Random => legaia_rando::arts::ArtsMode::Random,
    });
    let door_mode = args.doors.mode();
    let shop_mode = args.shops.mode();
    let casino_mode = args.casino.mode();
    let monster_stats_mode = args.monster_stats.mode();
    let move_power_mode = args.move_power.mode();
    let element_affinity_mode = args.element_affinity.mode();
    let spell_cost_mode = args.spell_cost.mode();
    let equip_bonus_mode = args.equip_bonus.mode();

    println!("seed: {seed} (0x{seed:016X})");
    // Manifest lines accumulate the run's options + outcome for reproducibility.
    let mut manifest = vec![
        "# legaia-rando run manifest".to_string(),
        format!("seed = {seed}  # 0x{seed:016X}"),
        format!("input = {:?}", args.input.display().to_string()),
    ];

    // The valid item pool (from SCUS) is needed only by the `random` modes.
    // Shops build their own sellable pool internally (priced items), so they
    // don't need the general valid-item pool.
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
            legaia_rando::unused::extend_pool(&mut pool, legaia_rando::unused::UNUSED_ITEM_IDS);
            // Name the otherwise-blank accessory so it shows as "Seru Bell"
            // wherever it lands.
            if let Some(name) = apply::inject_seru_bell_name(&mut patcher)? {
                println!("unused-items: named the unnamed accessory (0xFD) \"{name}\"");
                manifest.push(format!("unused_item_name = {name:?}"));
            }
        } else {
            println!("note: --unused-items has no effect without a `random` drop/chest/steal mode");
        }
        manifest.push(format!("unused_items = {}", args.unused_items));
    }
    // The unused-enemy id set (empty unless the toggle is on) is passed to the
    // encounter randomizer below.
    let unused_enemies: &[u8] = if args.unused_enemies {
        if args.encounters.mode() != Some(DropMode::Random) {
            println!("note: --unused-enemies only takes effect with `--encounters random`");
        }
        manifest.push(format!("unused_enemies = {}", args.unused_enemies));
        legaia_rando::unused::UNUSED_ENEMY_IDS
    } else {
        &[]
    };

    // Normal drop table first: reassign the monsters that already drop something.
    if let Some(mode) = mode {
        let (plan, report) = apply::randomize_drops(&mut patcher, &pool, seed, mode)?;
        println!(
            "drops: {} of {} monsters reassigned ({:?})",
            report.changed,
            plan.len(),
            mode
        );
        manifest.push(format!("drops = {:?}", mode_str(mode)));
        manifest.push(format!(
            "drops_changed = {}  # of {} dropping monsters",
            report.changed,
            plan.len()
        ));
        if !report.skipped.is_empty() {
            println!(
                "  note: {} slot(s) too full to re-pack, left unchanged: {:?}",
                report.skipped.len(),
                report.skipped
            );
            manifest.push(format!("drops_skipped = {:?}", report.skipped));
        }
    } else {
        println!("drops: untouched");
        manifest.push("drops = \"none\"".to_string());
    }

    // Equipment-as-drops layers *on top* via a code hook into the battle-end
    // reward routine: a low-chance roll grants one extra random equipment piece
    // in addition to the normal drop above, which is never disturbed.
    if args.equipment_drops {
        let report = apply::inject_equipment_bonus_drop(&mut patcher, args.equipment_drop_chance)?;
        println!(
            "equipment-drops: bonus equipment drop injected ({}% chance per battle, \
             {} gear ids in pool)",
            report.chance_pct, report.table_len
        );
        manifest.push("equipment_drops = true".to_string());
        manifest.push(format!("equipment_drop_chance = {}", report.chance_pct));
        manifest.push(format!("equipment_pool = {}", report.table_len));
    } else {
        manifest.push("equipment_drops = false".to_string());
    }

    // Run-away EXP: a code hook in the escape teardown banks a slice of the fled
    // formation's experience into the party (vanilla gives nothing for fleeing).
    if args.flee_exp {
        let report = apply::inject_flee_exp(&mut patcher, args.flee_exp_pct)?;
        println!(
            "flee-exp: {}% of a fled fight's experience banked into the party",
            report.pct
        );
        manifest.push("flee_exp = true".to_string());
        manifest.push(format!("flee_exp_pct = {}", report.pct));
    } else {
        manifest.push("flee_exp = false".to_string());
    }

    // Enemy ally ("charm"): a code hook in battle setup flags the frontmost enemy
    // with the AI-delegated bits so it fights for the party (works on bosses), and
    // a one-word widen of the victory check keeps it from being an enemy you must
    // defeat.
    if args.enemy_ally {
        let report = apply::inject_enemy_ally(&mut patcher, args.enemy_ally_pct)?;
        println!(
            "enemy-ally: {}% chance per battle a random enemy fights on your side",
            report.pct
        );
        manifest.push("enemy_ally = true".to_string());
        manifest.push(format!("enemy_ally_pct = {}", report.pct));
    } else {
        manifest.push("enemy_ally = false".to_string());
    }

    // Shiny Seru: a code hook in battle setup boosts a rare capturable enemy's
    // stats +35% and marks it; the capture/damage hooks flag the captured Seru
    // (persistent byte at record+0x1C0, kept off the level byte so the Seru still
    // levels up + displays normally) so its spell deals +35% damage forever.
    // Routines live in verified-dead SCUS arenas outside every live table.
    if args.shiny_seru {
        let report = apply::inject_shiny_seru(&mut patcher, args.shiny_pct)?;
        println!(
            "shiny-seru: {}% chance per battle a capturable enemy is shiny (+35% stats, \
             +35% damage when captured)",
            report.pct
        );
        manifest.push("shiny_seru = true".to_string());
        manifest.push(format!("shiny_pct = {}", report.pct));
    } else {
        manifest.push("shiny_seru = false".to_string());
    }

    // Seru trading: embed an enabled flag + the run's seed so the clean-room
    // engine can offer vendor seru-for-seru swaps (offers reseed every two
    // in-game hours from this seed). A plain data write; inert on real hardware.
    if args.seru_trade {
        let report = apply::enable_seru_trades(&mut patcher, seed, args.seru_trade_offers)?;
        println!(
            "seru-trade: vendor seru trading enabled (up to {} offers/vendor, reseeds every 2h)",
            report.config.max_offers
        );
        manifest.push("seru_trade = true".to_string());
        manifest.push(format!("seru_trade_offers = {}", report.config.max_offers));
    } else {
        manifest.push("seru_trade = false".to_string());
    }

    if let Some(enc_mode) = enc_mode {
        let scope = args.encounter_scope.scope();
        // Solo-strong is ON by default for any encounter randomization; opt out
        // with --no-solo-strong-encounters.
        let solo = (!args.no_solo_strong_encounters).then_some(apply::SoloStrongConfig {
            threshold_pct: args.solo_strong_threshold,
        });
        let report = apply::randomize_encounters_full(
            &mut patcher,
            seed,
            enc_mode,
            scope,
            unused_enemies,
            solo,
        )?;
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

    if let Some(chest_mode) = chest_mode {
        // Resolve the keep-static set: the disc-derived quest/key/story set
        // (every unsellable quest item, so none is ever moved or randomly
        // placed), or the user's explicit (possibly empty) override.
        let keep_static: std::collections::BTreeSet<u8> = match &args.keep_static_items {
            None => {
                let scus = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
                    .context("SCUS_942.54 not found in disc image (needed for chest defaults)")?;
                legaia_rando::items::default_static_chest_items(&scus)
            }
            Some(list) => list
                .iter()
                .filter(|s| !s.trim().is_empty())
                .map(|s| parse_item_id(s))
                .collect::<Result<_>>()?,
        };
        let report = apply::randomize_chests(&mut patcher, &pool, seed, chest_mode, &keep_static)?;
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

    if let Some(shop_mode) = shop_mode {
        let report = apply::randomize_shops(&mut patcher, seed, shop_mode)?;
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

    if let Some(casino_mode) = casino_mode {
        let changed = apply::randomize_casino(&mut patcher, seed, casino_mode)?;
        println!("casino: {changed} prize slot(s) changed ({casino_mode:?})");
        manifest.push(format!("casino = {:?}", mode_str(casino_mode)));
        manifest.push(format!("casino_changed = {changed}"));
    } else {
        println!("casino: untouched");
        manifest.push("casino = \"none\"".to_string());
    }

    if let Some(monster_stats_mode) = monster_stats_mode {
        let report = apply::randomize_monster_stats(&mut patcher, seed, monster_stats_mode)?;
        println!(
            "monster stats: {} monsters changed, {} fields ({:?})",
            report.monsters_changed, report.fields_changed, monster_stats_mode
        );
        manifest.push(format!(
            "monster_stats = {:?}",
            mode_str(monster_stats_mode)
        ));
        manifest.push(format!(
            "monster_stats_monsters_changed = {}",
            report.monsters_changed
        ));
        manifest.push(format!(
            "monster_stats_fields_changed = {}",
            report.fields_changed
        ));
        if !report.skipped.is_empty() {
            println!(
                "  note: {} monster slot(s) too tight to re-pack, left unchanged: {:?}",
                report.skipped.len(),
                report.skipped
            );
            manifest.push(format!("monster_stats_skipped = {:?}", report.skipped));
        }
    } else {
        println!("monster stats: untouched");
        manifest.push("monster_stats = \"none\"".to_string());
    }

    if let Some(move_power_mode) = move_power_mode {
        let changed = apply::randomize_move_powers(&mut patcher, seed, move_power_mode)?;
        println!("move power: {changed} special-attack power(s) changed ({move_power_mode:?})");
        manifest.push(format!("move_power = {:?}", mode_str(move_power_mode)));
        manifest.push(format!("move_power_changed = {changed}"));
    } else {
        println!("move power: untouched");
        manifest.push("move_power = \"none\"".to_string());
    }

    if let Some(element_affinity_mode) = element_affinity_mode {
        let changed = apply::randomize_element_affinity(&mut patcher, seed, element_affinity_mode)?;
        println!("element affinity: {changed} matrix cell(s) changed ({element_affinity_mode:?})");
        manifest.push(format!(
            "element_affinity = {:?}",
            mode_str(element_affinity_mode)
        ));
        manifest.push(format!("element_affinity_changed = {changed}"));
    } else {
        println!("element affinity: untouched");
        manifest.push("element_affinity = \"none\"".to_string());
    }

    if let Some(spell_cost_mode) = spell_cost_mode {
        let changed = apply::randomize_spell_costs(&mut patcher, seed, spell_cost_mode)?;
        println!("spell costs: {changed} spell MP cost(s) changed ({spell_cost_mode:?})");
        manifest.push(format!("spell_cost = {:?}", mode_str(spell_cost_mode)));
        manifest.push(format!("spell_cost_changed = {changed}"));
    } else {
        println!("spell costs: untouched");
        manifest.push("spell_cost = \"none\"".to_string());
    }

    if args.weapon_specialty {
        let report = apply::randomize_weapon_specialty(&mut patcher, seed)?;
        let map = report
            .assignments
            .iter()
            .map(|a| format!("{}->{}", a.character, a.to))
            .collect::<Vec<_>>()
            .join(", ");
        let skip_note = if report.weapons_skipped_fit > 0 {
            format!(", {} skipped (slot too tight)", report.weapons_skipped_fit)
        } else {
            String::new()
        };
        println!(
            "weapon specialty: reassigned ({map}); {} weapon(s) rewritten{skip_note}",
            report.weapons_changed
        );
        manifest.push("weapon_specialty = true".to_string());
        for a in &report.assignments {
            manifest.push(format!("weapon_specialty_{} = {}", a.character, a.to));
        }
        manifest.push(format!(
            "weapon_specialty_weapons_changed = {}",
            report.weapons_changed
        ));
        if report.weapons_skipped_fit > 0 {
            manifest.push(format!(
                "weapon_specialty_skipped_fit = {}",
                report.weapons_skipped_fit
            ));
        }
    } else {
        println!("weapon specialty: untouched");
        manifest.push("weapon_specialty = false".to_string());
    }

    if let Some(equip_bonus_mode) = equip_bonus_mode {
        let changed = apply::randomize_equip_bonuses(&mut patcher, seed, equip_bonus_mode)?;
        println!("equip bonuses: {changed} bonus row(s) changed ({equip_bonus_mode:?})");
        manifest.push(format!("equip_bonus = {:?}", mode_str(equip_bonus_mode)));
        manifest.push(format!("equip_bonus_changed = {changed}"));
    } else {
        println!("equip bonuses: untouched");
        manifest.push("equip_bonus = \"none\"".to_string());
    }

    if let Some(steal_mode) = steal_mode {
        let (plan, report) = apply::randomize_steals(&mut patcher, &pool, seed, steal_mode)?;
        println!(
            "steals: {} of {} stealable monsters reassigned ({:?})",
            report.items_changed,
            plan.len(),
            steal_mode
        );
        manifest.push(format!("steals = {:?}", mode_str(steal_mode)));
        manifest.push(format!(
            "steals_changed = {}  # of {} stealable monsters",
            report.items_changed, report.monsters
        ));
    } else {
        println!("steals: untouched");
        manifest.push("steals = \"none\"".to_string());
    }

    if let Some(arts_mode) = arts_mode {
        let (_plan, report) = apply::randomize_arts(&mut patcher, seed, arts_mode)?;
        println!(
            "arts: {} of {} arts re-combo'd ({:?})",
            report.combos_changed, report.arts, arts_mode
        );
        manifest.push(format!(
            "arts = {:?}",
            match arts_mode {
                legaia_rando::arts::ArtsMode::Shuffle => "shuffle",
                legaia_rando::arts::ArtsMode::Random => "random",
            }
        ));
        manifest.push(format!(
            "arts_changed = {}  # of {} regular arts",
            report.combos_changed, report.arts
        ));
    } else {
        println!("arts: untouched");
        manifest.push("arts = \"none\"".to_string());
    }

    if let Some(door_mode) = door_mode {
        let coupling = args.door_coupling.coupling();
        let report = apply::randomize_doors(&mut patcher, seed, door_mode, coupling)?;
        let coupling_str = match coupling {
            apply::DoorCoupling::Coupled => "coupled",
            apply::DoorCoupling::Decoupled => "decoupled",
        };
        println!(
            "doors: {} of {} sites changed across {} scenes ({:?}, {coupling_str})",
            report.sites_changed, report.sites_total, report.scenes_changed, door_mode
        );
        manifest.push(format!("doors = {:?}", mode_str(door_mode)));
        manifest.push(format!("door_coupling = {coupling_str:?}"));
        manifest.push(format!("doors_sites = {}", report.sites_total));
        manifest.push(format!("doors_sites_changed = {}", report.sites_changed));
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

    if let Some(hd_mode) = args.house_doors.mode() {
        let report = apply::randomize_house_doors(&mut patcher, seed, hd_mode)?;
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
            // under the same option - see `legaia_rando::map_door`.
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

    let seed_opts = legaia_rando::starting_items::StartingSeedOptions {
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
        let report = apply::randomize_starting_items(&mut patcher, seed, &seed_opts)?;
        let names = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
            .and_then(|scus| legaia_asset::item_names::ItemNameTable::from_scus(&scus));
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
        let overflow = legaia_rando::starting_items::overflow_bag(seed, &seed_opts);
        if !overflow.is_empty() {
            let guard = legaia_rando::starting_bag::DEFAULT_GUARD_BIT;
            let bag = apply::apply_starting_bag(&mut patcher, &overflow, guard)?;
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

    if legaia_rando::starting_level::is_active(args.starting_level) {
        let report = apply::apply_starting_level(&mut patcher, args.starting_level)?;
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

    // Diff original vs patched -> PPF.
    let patched = patcher.into_image();
    if patched.len() != original.len() {
        bail!("patched image changed size - refusing to emit (all edits must be same-size)");
    }
    let runs = ppf::diff_runs(&original, &patched);
    let changed_bytes: usize = runs.iter().map(|r| r.bytes.len()).sum();
    manifest.push(format!("ppf_records = {}", runs.len()));
    manifest.push(format!("bytes_changed = {changed_bytes}"));

    if runs.is_empty() {
        println!("note: no bytes changed (nothing to randomize for these options)");
    }

    if args.dry_run {
        println!(
            "dry run: would write a {}-record PPF ({} bytes changed); no files written",
            runs.len(),
            changed_bytes
        );
        return Ok(());
    }

    let desc = format!("Legend of Legaia randomizer seed {seed}");
    let ppf_bytes = ppf::write_ppf3(&desc, &runs);
    let patch_path = args
        .patch
        .clone()
        .unwrap_or_else(|| with_extension(&args.input, "ppf"));
    std::fs::write(&patch_path, &ppf_bytes)
        .with_context(|| format!("write patch {}", patch_path.display()))?;
    println!(
        "patch: {} ({} records, {} bytes changed)",
        patch_path.display(),
        runs.len(),
        changed_bytes
    );

    if let Some(out) = &args.output {
        std::fs::write(out, &patched).with_context(|| format!("write {}", out.display()))?;
        println!(
            "patched image: {} (contains Sony bytes - do not redistribute)",
            out.display()
        );
        // Emit a matching .cue next to the image so emulators that won't load a
        // bare BIN (e.g. mednafen rejects >64 MiB BIN) can open it directly.
        let cue_path = out.with_extension("cue");
        let bin_name = out
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "disc.bin".to_string());
        std::fs::write(&cue_path, cue_contents(&bin_name))
            .with_context(|| format!("write {}", cue_path.display()))?;
        println!("cue sheet:     {}", cue_path.display());
    }

    if let Some(mpath) = &args.manifest {
        let mut text = manifest.join("\n");
        text.push('\n');
        std::fs::write(mpath, text)
            .with_context(|| format!("write manifest {}", mpath.display()))?;
        println!("manifest: {}", mpath.display());
    }

    Ok(())
}

/// Apply a PPF to a copy of the disc and confirm the result still parses.
pub(crate) fn cmd_verify(input: &Path, patch: &Path, output: Option<&Path>) -> Result<()> {
    let mut image = load_image(input)?;
    let ppf = std::fs::read(patch).with_context(|| format!("read patch {}", patch.display()))?;
    let applied =
        legaia_rando::ppf::apply_ppf3(&mut image, &ppf).context("apply PPF to disc image")?;
    // Re-parse the patched image end to end as a sanity check.
    let patcher = DiscPatcher::open(image).context("patched image no longer parses as a disc")?;
    let drops = apply::current_drops(&patcher)
        .map(|d| d.iter().filter(|x| x.item != 0).count())
        .unwrap_or(0);
    println!(
        "verify OK: {applied} PPF records applied; disc parses ({} PROT entries, {drops} monster drops)",
        patcher.entry_count()
    );
    if let Some(out) = output {
        std::fs::write(out, patcher.image()).with_context(|| format!("write {}", out.display()))?;
        println!(
            "patched image: {} (contains Sony bytes - do not redistribute)",
            out.display()
        );
    }
    Ok(())
}
