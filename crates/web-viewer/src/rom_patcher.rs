//! In-browser randomizer / disc patcher.
//!
//! Runs the Track-1 [`legaia_rando`] randomizer entirely client-side: the user
//! supplies their own disc image, the patcher edits it in WASM memory, and the
//! page downloads the patched image locally. No bytes leave the browser and
//! nothing is uploaded — the same "user supplies the disc" model as the CLI, so
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
/// the new game begins with (`0` = leave the vanilla Healing Leaf ×5; capped at
/// 5). `door_of_wind` is how many Door of Wind (the warp consumable) to seed
/// into the starting bag (`0` = none); `incense` is how many Incense (the
/// encounter-rate consumable) to seed likewise (`0` = none); `all_warps` presets the visited-towns
/// bitmask so Door of Wind can teleport to any town from the start (its own code
/// region, so it doesn't reduce the item count). `unused_enemies` adds the unused Evil Bat ids to the random-encounter
/// pool (only with `encounters = "random"`); `unused_items` adds the unused
/// "Something Good" / unnamed-accessory items to the random-fill pool (only the
/// `random` drop / chest / steal modes use it). `equipment_drops` turns every
/// monster's drop into a rare random weapon / armor / accessory at a tiered
/// chance (overrides `drops`). `monster_stats` / `move_power` /
/// `element_affinity` / `spell_cost` / `equip_bonus` are the battle-tuning +
/// equipment-bonus passes, each `"shuffle"` / `"random"` / `"none"`: monster
/// combat stats, special-attack power, the element-affinity matrix, spell MP
/// costs, and the equipment passive stat tuples (redistributed within each slot
/// category). `encounter_scope` widens the monster pool an
/// encounter roll draws from: `"scene"` (default — each scene's own monsters),
/// `"kingdom"` (any monster in the scene's Drake/Sebucus/Karisto kingdom), or
/// `"world"` (any monster on the disc, so late-game monsters can appear at the
/// start). Only matters when `encounters` is not `"none"`. `seed` is a number or
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
    // (the unnamed accessory in particular is otherwise excluded — no name), and
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

    if equipment_drops {
        // Equipment drops own the single drop slot, so they replace the normal
        // drops pass (same as the CLI's `--equipment-drops`).
        let scus = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
            .ok_or_else(|| err("SCUS_942.54 not found (needed for equipment drops)"))?;
        let equip_pool = legaia_rando::equipment::equipment_pool(&scus)
            .map_err(|e| err(format!("equipment: {e}")))?;
        let (plan, rep) = apply::randomize_equipment_drops(&mut patcher, &equip_pool, seed_n)
            .map_err(|e| err(format!("equipment drops: {e}")))?;
        summary.push_str(&format!(
            "equipment-drops: {} of {} monsters drop rare gear ({} ids in pool)\n",
            rep.changed,
            plan.len(),
            equip_pool.len()
        ));
        if !rep.skipped.is_empty() {
            summary.push_str(&format!(
                "  {} slot(s) too full to re-pack\n",
                rep.skipped.len()
            ));
        }
    } else {
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
    }

    match enc_mode {
        Some(m) => {
            let scope = parse_encounter_scope(encounter_scope);
            let rep = apply::randomize_encounters_scoped(
                &mut patcher,
                seed_n,
                m,
                scope,
                unused_enemy_ids,
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
        }
        None => summary.push_str("encounters: untouched\n"),
    }

    match chest_mode {
        Some(m) => {
            // Protect the curated quest / key-item chests by default (same set as
            // the CLI's default), so the in-browser patcher behaves identically.
            let keep_static: std::collections::BTreeSet<u8> =
                legaia_rando::items::DEFAULT_STATIC_CHEST_ITEMS
                    .iter()
                    .copied()
                    .collect();
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
        all_warps,
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
    } else {
        summary.push_str("starting-items: untouched (vanilla Healing Leaf x5)\n");
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
