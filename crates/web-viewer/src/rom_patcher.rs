//! In-browser randomizer / disc patcher.
//!
//! Runs the Track-1 [`legaia_rando`] randomizer entirely client-side: the user
//! supplies their own disc image, the patcher edits it in WASM memory, and the
//! page downloads the patched image locally. No bytes leave the browser and
//! nothing is uploaded - the same "user supplies the disc" model as the CLI, so
//! the site still ships only code.
//!
//! [`patch_rom`] returns a JS object `{ data: Uint8Array, summary: String,
//! seed: String }`: `data` is the patched image (the download), `summary` is a
//! human-readable change report, `seed` is the resolved numeric seed (so a run
//! reproduces from a memorable string seed).

use js_sys::{Object, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;

use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::drops::DropMode;
use legaia_rando::items::valid_item_pool;
use legaia_rando::rng::seed_from_str;

fn parse_mode(s: &str) -> Option<DropMode> {
    match s {
        "shuffle" => Some(DropMode::Shuffle),
        "random" => Some(DropMode::Random),
        _ => None, // "none" or anything else
    }
}

fn parse_encounter_scope(s: &str) -> apply::EncounterScope {
    match s {
        "kingdom" => apply::EncounterScope::Kingdom,
        "world" => apply::EncounterScope::World,
        _ => apply::EncounterScope::Scene, // "scene" or anything else
    }
}

fn err(msg: impl AsRef<str>) -> JsValue {
    JsValue::from_str(msg.as_ref())
}

/// Resolve a user seed string to the numeric seed, as a decimal string (so the
/// page can display / persist it without JS `BigInt` precision loss).
#[wasm_bindgen]
pub fn resolve_seed(seed: &str) -> String {
    seed_from_str(seed).to_string()
}

/// Patch a user-supplied disc image with the chosen randomizer settings.
///
/// `drops` / `encounters` / `chests` / `shops` / `casino` / `steals` / `arts` /
/// `doors` / `house_doors` are each `"shuffle"`, `"random"`, or `"none"`.
/// `arts` reassigns Tactical-Arts button combos (same-length, unique within
/// character; Miracle Arts untouched). `shops`
/// randomizes what town stores sell; `casino` the casino prize exchange. `door_coupling` is `"coupled"`
/// (bidirectional) or `"decoupled"` (one-way). `house_doors` honours only
/// `"shuffle"`. `starting_items` is the number of random starting consumables
/// the new game begins with (`0` = leave the vanilla Healing Leaf ×5). The
/// random fill shares the seed's capacity (7 slots, or 5 with `all_warps`) with
/// the convenience-item toggles below and takes whatever they leave, so it adds
/// on top of them. `door_of_wind` is how many Door of Wind (the warp consumable) to seed
/// into the starting bag (`0` = none); `incense` is how many Incense (the
/// encounter-rate consumable) to seed likewise (`0` = none); `speed_chain` /
/// `chicken_heart` / `good_luck_bell` seed those accessories the same way
/// (`0` = none each); `all_warps` presets the visited-towns
/// bitmask so Door of Wind can teleport to any town from the start (its own code
/// region, so it doesn't reduce the item count). `unused_enemies` adds the unused Evil Bat ids to the random-encounter
/// pool (only with `encounters = "random"`); `unused_items` adds the unused
/// "Something Good" / unnamed-accessory items to the random-fill pool (only the
/// `random` drop / chest / steal modes use it). `equipment_drops` injects a code
/// hook into the battle-end reward routine that, on a low per-battle chance,
/// grants one *extra* random weapon / armor / accessory on top of the normal
/// drop - additive, so `drops` is never disturbed. `monster_stats` / `move_power` /
/// `element_affinity` / `spell_cost` / `equip_bonus` are the battle-tuning +
/// equipment-bonus passes, each `"shuffle"` / `"random"` / `"none"`: monster
/// combat stats, special-attack power, the element-affinity matrix, spell MP
/// costs, and the equipment passive stat tuples (redistributed within each slot
/// category). `encounter_scope` widens the monster pool an
/// encounter roll draws from: `"scene"` (default - each scene's own monsters),
/// `"kingdom"` (any monster in the scene's Drake/Sebucus/Karisto kingdom), or
/// `"world"` (any monster on the disc, so late-game monsters can appear at the
/// start). Only matters when `encounters` is not `"none"`.
/// `solo_strong_encounters` (only with `encounters` set) forces any randomized
/// formation holding a monster much stronger than the area's natives down to that
/// lone enemy, so an over-strong monster is faced solo instead of in a pack.
/// `flee_exp` injects a code hook into the battle-action escape teardown so that
/// successfully running away banks a small slice of the fled fight's experience
/// into the party (vanilla awards nothing for fleeing). `seru_trade` adds an
/// in-shop trading vendor (a fourth Buy/Sell/Trade/Quit row) that swaps a party
/// member's learned Seru-magic for a different one at a fixed level, on a
/// time-bucketed schedule derived from the seed; all of it is hosted in the menu
/// overlay, so it composes with every other option here. `enemy_ally` injects a
/// code hook into battle setup so that, with a per-battle chance, a random enemy
/// is charmed onto the party's side as an uncontrolled ally (works in any fight,
/// bosses included), plus a one-word widen of the victory check so the ally isn't
/// an enemy you must defeat. `shiny_seru` injects code hooks so that, with a
/// per-battle chance, the frontmost *capturable* enemy spawns as a rare shiny
/// variant (+35% stats) whose captured Seru deals +35% damage on every future
/// cast (the flag rides the spell's level byte and is masked from the level-up +
/// menu readers).
/// `starting_level`
/// begins the new game at that character level instead of 1 (`0` or `1` =
/// vanilla; range 2..=14), seeding the lead character's XP and recomputing the
/// starting stats from the disc's growth curves. `seed` is a number or
/// any string (hashed). Returns `{ data, summary, seed }`.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn patch_rom(
    image: Vec<u8>,
    seed: &str,
    drops: &str,
    encounters: &str,
    encounter_scope: &str,
    chests: &str,
    shops: &str,
    casino: &str,
    steals: &str,
    arts: &str,
    doors: &str,
    door_coupling: &str,
    house_doors: &str,
    starting_items: usize,
    door_of_wind: u8,
    incense: u8,
    speed_chain: u8,
    chicken_heart: u8,
    good_luck_bell: u8,
    all_warps: bool,
    unused_enemies: bool,
    unused_items: bool,
    equipment_drops: bool,
    monster_stats: &str,
    move_power: &str,
    element_affinity: &str,
    spell_cost: &str,
    equip_bonus: &str,
    weapon_specialty: bool,
    starting_level: u8,
    solo_strong_encounters: bool,
    flee_exp: bool,
    seru_trade: bool,
    enemy_ally: bool,
    shiny_seru: bool,
) -> Result<JsValue, JsValue> {
    let seed_n = seed_from_str(seed);
    let drops_mode = parse_mode(drops);
    let enc_mode = parse_mode(encounters);
    let chest_mode = parse_mode(chests);
    let monster_stats_mode = parse_mode(monster_stats);
    let move_power_mode = parse_mode(move_power);
    let element_affinity_mode = parse_mode(element_affinity);
    let spell_cost_mode = parse_mode(spell_cost);
    let equip_bonus_mode = parse_mode(equip_bonus);
    let shop_mode = parse_mode(shops);
    let casino_mode = parse_mode(casino);
    let steal_mode = parse_mode(steals);
    let arts_mode = parse_mode(arts).map(|m| match m {
        DropMode::Shuffle => legaia_rando::arts::ArtsMode::Shuffle,
        DropMode::Random => legaia_rando::arts::ArtsMode::Random,
    });
    let door_mode = parse_mode(doors);
    let house_door_mode = parse_mode(house_doors);

    let mut patcher = DiscPatcher::open(image).map_err(|e| err(format!("parse disc: {e}")))?;

    // The valid item pool (from SCUS) is needed only by the `random` modes.
    // Shops build their own sellable pool internally, so they don't need the
    // general valid-item pool.
    let needs_pool = drops_mode == Some(DropMode::Random)
        || chest_mode == Some(DropMode::Random)
        || steal_mode == Some(DropMode::Random);
    let mut pool = if needs_pool {
        let scus = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
            .ok_or_else(|| err("SCUS_942.54 not found in disc image (needed for a random mode)"))?;
        valid_item_pool(&scus).map_err(|e| err(format!("item pool: {e}")))?
    } else {
        Vec::new()
    };
    // `--unused-items`: widen the random-fill pool with the curated unused items
    // (the unnamed accessory in particular is otherwise excluded - no name), and
    // give that accessory the name "Seru Bell" so it doesn't show as a blank.
    if unused_items && needs_pool {
        legaia_rando::unused::extend_pool(&mut pool, legaia_rando::unused::UNUSED_ITEM_IDS);
        apply::inject_seru_bell_name(&mut patcher).map_err(|e| err(format!("name inject: {e}")))?;
    }
    // The unused-enemy id set passed to the encounter randomizer (empty unless on).
    let unused_enemy_ids: &[u8] = if unused_enemies {
        legaia_rando::unused::UNUSED_ENEMY_IDS
    } else {
        &[]
    };

    let mut summary = String::new();

    // Normal drop table first: reassign the monsters that already drop something.
    match drops_mode {
        Some(m) => {
            let (plan, rep) = apply::randomize_drops(&mut patcher, &pool, seed_n, m)
                .map_err(|e| err(format!("drops: {e}")))?;
            summary.push_str(&format!(
                "drops: {} of {} reassigned ({})\n",
                rep.changed,
                plan.len(),
                drops
            ));
            if !rep.skipped.is_empty() {
                summary.push_str(&format!(
                    "  {} slot(s) too full to re-pack\n",
                    rep.skipped.len()
                ));
            }
        }
        None => summary.push_str("drops: untouched\n"),
    }

    // Equipment-as-drops layers on top via a code hook into the battle-end
    // reward routine: a low-chance roll grants one extra random equipment piece
    // in addition to the normal drop, which is never disturbed.
    if equipment_drops {
        let rep = apply::inject_equipment_bonus_drop(
            &mut patcher,
            legaia_rando::bonus_drop::DEFAULT_CHANCE_PCT,
        )
        .map_err(|e| err(format!("equipment drops: {e}")))?;
        summary.push_str(&format!(
            "equipment-drops: bonus drop injected ({}% per battle, {} gear ids in pool)\n",
            rep.chance_pct, rep.table_len
        ));
    }

    match enc_mode {
        Some(m) => {
            let scope = parse_encounter_scope(encounter_scope);
            let solo = solo_strong_encounters.then(apply::SoloStrongConfig::default);
            let rep = apply::randomize_encounters_full(
                &mut patcher,
                seed_n,
                m,
                scope,
                unused_enemy_ids,
                solo,
            )
            .map_err(|e| err(format!("encounters: {e}")))?;
            summary.push_str(&format!(
                "encounters: {} scenes, {} ids changed ({} {})\n",
                rep.scenes_changed, rep.ids_changed, encounter_scope, encounters
            ));
            if rep.unused_placed > 0 {
                summary.push_str(&format!(
                    "  including {} unused-enemy spawn(s) injected\n",
                    rep.unused_placed
                ));
            }
            if solo.is_some() {
                summary.push_str(&format!(
                    "  solo-strong: {} strong fight(s) forced to a lone enemy\n",
                    rep.solo_collapsed
                ));
            }
        }
        None => summary.push_str("encounters: untouched\n"),
    }

    // Run-away EXP: a code hook in the escape teardown banks a slice of a fled
    // fight's experience into the party (vanilla gives nothing for fleeing).
    if flee_exp {
        let rep = apply::inject_flee_exp(&mut patcher, legaia_rando::flee_exp::DEFAULT_PCT)
            .map_err(|e| err(format!("flee-exp: {e}")))?;
        summary.push_str(&format!(
            "flee-exp: {}% of a fled fight's experience banked into the party\n",
            rep.pct
        ));
    } else {
        summary.push_str("flee-exp: untouched\n");
    }

    // Enemy ally ("charm"): a code hook in battle setup flags the frontmost enemy
    // so it fights on the player's side (works on bosses); a one-word widen of the
    // victory check keeps the charmed enemy from being one you must defeat.
    if enemy_ally {
        let rep = apply::inject_enemy_ally(&mut patcher, legaia_rando::enemy_ally::DEFAULT_PCT)
            .map_err(|e| err(format!("enemy-ally: {e}")))?;
        summary.push_str(&format!(
            "enemy-ally: {}% chance per battle a random enemy fights on your side\n",
            rep.pct
        ));
    } else {
        summary.push_str("enemy-ally: untouched\n");
    }

    // Shiny Seru: a code hook boosts a rare capturable enemy's stats +35%; the
    // capture/damage hooks make its captured Seru deal +35% damage forever.
    if shiny_seru {
        let rep = apply::inject_shiny_seru(&mut patcher, legaia_rando::shiny_seru::DEFAULT_PCT)
            .map_err(|e| err(format!("shiny-seru: {e}")))?;
        summary.push_str(&format!(
            "shiny-seru: {}% chance per battle a capturable enemy is shiny (+35% stats / damage)\n",
            rep.pct
        ));
    } else {
        summary.push_str("shiny-seru: untouched\n");
    }

    // Seru trading: a vendor in shops offers to trade a party member's Seru-magic for
    // a different one (time-bucketed, deterministic from the seed). All code + data is
    // hosted in the menu overlay, so it composes with every other feature here.
    if seru_trade {
        apply::inject_trade_full(&mut patcher, seed_n)
            .map_err(|e| err(format!("seru-trade: {e}")))?;
        summary.push_str("seru-trade: in-shop Seru trading vendor enabled\n");
    } else {
        summary.push_str("seru-trade: untouched\n");
    }

    match chest_mode {
        Some(m) => {
            // Protect every quest / key / story item by default (same disc-derived
            // set as the CLI), so the in-browser patcher behaves identically: no
            // quest item is moved out of its chest or dropped into another.
            let keep_static: std::collections::BTreeSet<u8> =
                match legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54") {
                    Some(scus) => legaia_rando::items::default_static_chest_items(&scus),
                    None => legaia_rando::items::DEFAULT_STATIC_CHEST_ITEMS
                        .iter()
                        .copied()
                        .collect(),
                };
            let rep = apply::randomize_chests(&mut patcher, &pool, seed_n, m, &keep_static)
                .map_err(|e| err(format!("chests: {e}")))?;
            summary.push_str(&format!(
                "chests: {} of {} sites changed across {} scenes ({}); {} kept static\n",
                rep.items_changed,
                rep.sites_total,
                rep.scenes_changed,
                chests,
                keep_static.len()
            ));
        }
        None => summary.push_str("chests: untouched\n"),
    }

    match shop_mode {
        Some(m) => {
            let rep = apply::randomize_shops(&mut patcher, seed_n, m)
                .map_err(|e| err(format!("shops: {e}")))?;
            summary.push_str(&format!(
                "shops: {} of {} town-shop slots changed across {} scenes ({})\n",
                rep.items_changed, rep.slots_total, rep.scenes_changed, shops
            ));
        }
        None => summary.push_str("shops: untouched\n"),
    }

    match casino_mode {
        Some(m) => {
            let changed = apply::randomize_casino(&mut patcher, seed_n, m)
                .map_err(|e| err(format!("casino: {e}")))?;
            summary.push_str(&format!(
                "casino: {changed} prize slot(s) changed ({casino})\n"
            ));
        }
        None => summary.push_str("casino: untouched\n"),
    }

    match monster_stats_mode {
        Some(m) => {
            let rep = apply::randomize_monster_stats(&mut patcher, seed_n, m)
                .map_err(|e| err(format!("monster-stats: {e}")))?;
            summary.push_str(&format!(
                "monster-stats: {} monsters changed, {} fields ({})\n",
                rep.monsters_changed, rep.fields_changed, monster_stats
            ));
        }
        None => summary.push_str("monster-stats: untouched\n"),
    }

    match move_power_mode {
        Some(m) => {
            let changed = apply::randomize_move_powers(&mut patcher, seed_n, m)
                .map_err(|e| err(format!("move-power: {e}")))?;
            summary.push_str(&format!(
                "move-power: {changed} special-attack power(s) changed ({move_power})\n"
            ));
        }
        None => summary.push_str("move-power: untouched\n"),
    }

    match element_affinity_mode {
        Some(m) => {
            let changed = apply::randomize_element_affinity(&mut patcher, seed_n, m)
                .map_err(|e| err(format!("element-affinity: {e}")))?;
            summary.push_str(&format!(
                "element-affinity: {changed} matrix cell(s) changed ({element_affinity})\n"
            ));
        }
        None => summary.push_str("element-affinity: untouched\n"),
    }

    match spell_cost_mode {
        Some(m) => {
            let changed = apply::randomize_spell_costs(&mut patcher, seed_n, m)
                .map_err(|e| err(format!("spell-cost: {e}")))?;
            summary.push_str(&format!(
                "spell-cost: {changed} spell MP cost(s) changed ({spell_cost})\n"
            ));
        }
        None => summary.push_str("spell-cost: untouched\n"),
    }

    match equip_bonus_mode {
        Some(m) => {
            let changed = apply::randomize_equip_bonuses(&mut patcher, seed_n, m)
                .map_err(|e| err(format!("equip-bonus: {e}")))?;
            summary.push_str(&format!(
                "equip-bonus: {changed} bonus row(s) changed ({equip_bonus})\n"
            ));
        }
        None => summary.push_str("equip-bonus: untouched\n"),
    }

    if weapon_specialty {
        let rep = apply::randomize_weapon_specialty(&mut patcher, seed_n)
            .map_err(|e| err(format!("weapon-specialty: {e}")))?;
        let map = rep
            .assignments
            .iter()
            .map(|a| format!("{}->{}", a.character, a.to))
            .collect::<Vec<_>>()
            .join(", ");
        summary.push_str(&format!(
            "weapon-specialty: reassigned ({map}); {} weapon(s) rewritten\n",
            rep.weapons_changed
        ));
    } else {
        summary.push_str("weapon-specialty: untouched\n");
    }

    match steal_mode {
        Some(m) => {
            let (plan, rep) = apply::randomize_steals(&mut patcher, &pool, seed_n, m)
                .map_err(|e| err(format!("steals: {e}")))?;
            summary.push_str(&format!(
                "steals: {} of {} stealable monsters reassigned ({})\n",
                rep.items_changed,
                plan.len(),
                steals
            ));
        }
        None => summary.push_str("steals: untouched\n"),
    }

    match arts_mode {
        Some(m) => {
            let (_plan, rep) = apply::randomize_arts(&mut patcher, seed_n, m)
                .map_err(|e| err(format!("arts: {e}")))?;
            summary.push_str(&format!(
                "arts: {} of {} arts re-combo'd ({})\n",
                rep.combos_changed, rep.arts, arts
            ));
        }
        None => summary.push_str("arts: untouched\n"),
    }

    match door_mode {
        Some(m) => {
            let coupling = match door_coupling {
                "decoupled" => apply::DoorCoupling::Decoupled,
                _ => apply::DoorCoupling::Coupled,
            };
            let rep = apply::randomize_doors(&mut patcher, seed_n, m, coupling)
                .map_err(|e| err(format!("doors: {e}")))?;
            summary.push_str(&format!(
                "doors: {} of {} sites changed across {} scenes ({}, {})\n",
                rep.sites_changed, rep.sites_total, rep.scenes_changed, doors, door_coupling
            ));
            if !rep.skipped.is_empty() {
                summary.push_str(&format!(
                    "  {} hub scene(s) too big to grow in place, kept original doors\n",
                    rep.skipped.len()
                ));
            }
        }
        None => summary.push_str("doors: untouched\n"),
    }

    match house_door_mode {
        Some(legaia_rando::drops::DropMode::Shuffle) => {
            let rep = apply::randomize_house_doors(
                &mut patcher,
                seed_n,
                legaia_rando::drops::DropMode::Shuffle,
            )
            .map_err(|e| err(format!("house-doors: {e}")))?;
            summary.push_str(&format!(
                "house-doors: {} of {} door-warp targets shuffled across {} scenes\n",
                rep.sites_changed, rep.sites_total, rep.scenes_changed
            ));
        }
        Some(_) => summary.push_str("house-doors: only `shuffle` supported; untouched\n"),
        None => summary.push_str("house-doors: untouched\n"),
    }

    let seed_opts = legaia_rando::starting_items::StartingSeedOptions {
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
        let rep = apply::randomize_starting_items(&mut patcher, seed_n, &seed_opts)
            .map_err(|e| err(format!("starting-items: {e}")))?;
        let names = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
            .and_then(|scus| legaia_asset::item_names::ItemNameTable::from_scus(&scus));
        let list: Vec<String> = rep
            .items
            .iter()
            .map(|(id, count)| {
                let nm = names.as_ref().and_then(|t| t.name(*id)).unwrap_or("?");
                format!("{count}x {nm}")
            })
            .collect();
        summary.push_str(&format!(
            "starting-items: new game begins with {} item(s): {}\n",
            rep.items_set,
            list.join(", ")
        ));
        if rep.all_warps {
            summary.push_str("all-warps: every Door of Wind destination unlocked from the start\n");
        }
        // Items beyond the 7-slot direct-seed cap are granted on top via a silent
        // GIVE_ITEM block injected into the opening scene (see `starting_bag`), so
        // the explicit convenience items AND the full requested random fill land.
        let overflow = legaia_rando::starting_items::overflow_bag(seed_n, &seed_opts);
        if !overflow.is_empty() {
            let bag = apply::apply_starting_bag(
                &mut patcher,
                &overflow,
                legaia_rando::starting_bag::DEFAULT_GUARD_BIT,
            )
            .map_err(|e| err(format!("starting-items overflow: {e}")))?;
            let extra: Vec<String> = overflow
                .iter()
                .map(|(id, count)| {
                    let nm = names.as_ref().and_then(|t| t.name(*id)).unwrap_or("?");
                    format!("{count}x {nm}")
                })
                .collect();
            if bag.applied {
                summary.push_str(&format!(
                    "starting-items: + {} more via the opening scene: {}\n",
                    overflow.len(),
                    extra.join(", ")
                ));
            } else {
                summary.push_str(&format!(
                    "starting-items: WARNING - {} overflow item(s) could not be injected; \
                     bag truncated to the direct seed\n",
                    overflow.len()
                ));
            }
        }
    } else {
        summary.push_str("starting-items: untouched (vanilla Healing Leaf x5)\n");
    }

    if legaia_rando::starting_level::is_active(starting_level) {
        let rep = apply::apply_starting_level(&mut patcher, starting_level)
            .map_err(|e| err(format!("starting-level: {e}")))?;
        summary.push_str(&format!(
            "starting-level: starting party begins at level {} ({} slot(s) leveled; \
             lead HP {}, MP {}, ATK {})\n",
            rep.level, rep.slots_leveled, rep.stats[0], rep.stats[1], rep.stats[3]
        ));
    } else {
        summary.push_str("starting-level: untouched (vanilla level 1)\n");
    }

    let patched = patcher.into_image();
    let data = Uint8Array::new_with_length(patched.len() as u32);
    data.copy_from(&patched);

    let out = Object::new();
    Reflect::set(&out, &"data".into(), &data)?;
    Reflect::set(&out, &"summary".into(), &summary.into())?;
    Reflect::set(&out, &"seed".into(), &seed_n.to_string().into())?;
    Ok(out.into())
}
