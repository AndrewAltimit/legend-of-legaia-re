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
/// `drops` / `encounters` / `chests` / `steals` / `doors` / `house_doors` are
/// each `"shuffle"`, `"random"`, or `"none"`. `door_coupling` is `"coupled"`
/// (bidirectional) or `"decoupled"` (one-way). `house_doors` honours only
/// `"shuffle"`. `starting_items` is the number of random starting consumables
/// the new game begins with (`0` = leave the vanilla Healing Leaf ×5; capped at
/// 5). `unused_enemies` adds the unused Evil Bat ids to the random-encounter
/// pool (only with `encounters = "random"`); `unused_items` adds the unused
/// "Something Good" / unnamed-accessory items to the random-fill pool (only the
/// `random` drop / chest / steal modes use it). `seed` is a number or any string
/// (hashed). Returns `{ data, summary, seed }`.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn patch_rom(
    image: Vec<u8>,
    seed: &str,
    drops: &str,
    encounters: &str,
    chests: &str,
    steals: &str,
    doors: &str,
    door_coupling: &str,
    house_doors: &str,
    starting_items: usize,
    unused_enemies: bool,
    unused_items: bool,
) -> Result<JsValue, JsValue> {
    let seed_n = seed_from_str(seed);
    let drops_mode = parse_mode(drops);
    let enc_mode = parse_mode(encounters);
    let chest_mode = parse_mode(chests);
    let steal_mode = parse_mode(steals);
    let door_mode = parse_mode(doors);
    let house_door_mode = parse_mode(house_doors);

    let mut patcher = DiscPatcher::open(image).map_err(|e| err(format!("parse disc: {e}")))?;

    // The valid item pool (from SCUS) is needed only by the `random` modes.
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

    match enc_mode {
        Some(m) => {
            let rep = apply::randomize_encounters(&mut patcher, seed_n, m, unused_enemy_ids)
                .map_err(|e| err(format!("encounters: {e}")))?;
            summary.push_str(&format!(
                "encounters: {} scenes, {} ids changed ({})\n",
                rep.scenes_changed, rep.ids_changed, encounters
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
                "house-doors: {} of {} MOVE_TO targets shuffled across {} scenes\n",
                rep.sites_changed, rep.sites_total, rep.scenes_changed
            ));
        }
        Some(_) => summary.push_str("house-doors: only `shuffle` supported; untouched\n"),
        None => summary.push_str("house-doors: untouched\n"),
    }

    if starting_items > 0 {
        let rep = apply::randomize_starting_items(&mut patcher, seed_n, starting_items)
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
            "starting-items: new game begins with {} random item(s): {}\n",
            rep.items_set,
            list.join(", ")
        ));
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
