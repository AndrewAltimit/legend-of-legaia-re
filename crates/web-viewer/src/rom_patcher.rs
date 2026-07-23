//! In-browser randomizer / disc patcher.
//!
//! Runs the Track-1 [`legaia_patcher`] randomizer entirely client-side: the user
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

use legaia_patcher::apply;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::drops::DropMode;
use legaia_patcher::items::valid_item_pool;
use legaia_patcher::rng::seed_from_str;
use legaia_patcher::translation::{
    ImportPhase, ImportReport, LanguagePack, export_pack, import_pack, import_pack_phase, lift,
};

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

/// Parse an `item=value` pair where `item` is a u8 id (decimal or `0xHH`) and
/// `value` is a u32. Returns `None` on any malformed token.
fn parse_id_eq_u32(tok: &str) -> Option<(u8, u32)> {
    let (id_str, val_str) = tok.trim().split_once('=')?;
    let id_str = id_str.trim();
    let id = if let Some(hex) = id_str
        .strip_prefix("0x")
        .or_else(|| id_str.strip_prefix("0X"))
    {
        u8::from_str_radix(hex, 16).ok()?
    } else {
        id_str.parse::<u8>().ok()?
    };
    let value = val_str.trim().parse::<u32>().ok()?;
    Some((id, value))
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
/// `"shuffle"` and covers both intra-town door classes: the scripted door
/// warps and the `.MAP` kind-0 intra-scene teleports (most house exits),
/// the latter rewired per scene only when walk-component reachability is
/// preserved. `starting_items` is the number of random starting consumables
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
/// menu readers). `jewel_fix` retargets the boss cinematic casts' damage calls
/// from the resist-ladder-bypassing wrapper to the guard-respecting one, so
/// elemental jewels / guards / All Guard apply to Xain's Bloody Horns / Terio
/// Punch, Cort's Guilty Cross, and the Delilas trio's signature moves (a fix,
/// not a randomization - it is seedless). `fishing_prices` is a
/// comma/space-separated list of `item=points` pairs that set the
/// fishing-exchange point cost of prizes (e.g. `0x6F=500` for the Water Egg).
/// `location_renames` is a newline-separated list of `index=name` lines that
/// rename world-map location slots (e.g. `3=Ancient Fire Cave`).
/// `earth_egg_price` (empty = untouched) sets the casino-coin threshold the Sol
/// Tower Prize Counter requires before it offers the Earth Ra-Seru Egg (retail
/// 100000); the game debits exactly that many coins on purchase. `arts_powers`
/// is a comma/space-separated list of `combo=value` pairs that rebalance a
/// Tactical Art's damage-power bytes (e.g. `RDLDL=0x16`; `value` a power byte
/// `0x0C..=0x1F` or `0`). `arts_ap_grants` is a comma/space-separated list of
/// `combo=amount` pairs (e.g. `RDLDL=10`; `amount` 1..=100 AP) that make an art
/// grant AP instead of costing it; mutually exclusive with `shiny_seru` (same
/// SCUS arena). These are all manual, seedless edits.
/// `starting_level`
/// begins the new game at that character level instead of 1 (`0` or `1` =
/// vanilla; range 2..=14), seeding the lead character's XP and recomputing the
/// starting stats from the disc's growth curves. `seed` is a number or
/// any string (hashed).
///
/// `lang_pack` is an **optional** `legaia-text-pack-v1` YAML document (empty
/// string = no language patch, the default). It is applied **first**, before
/// any randomizer pass, because a translation edit is keyed by a byte offset
/// into a scene's decompressed MAN and the door / starting-bag passes relocate
/// those records - translate-then-randomize composes, the reverse loses the
/// moved scenes' lines. Per-entry skips (a line over budget, a wrong-disc
/// mismatch) are counted in the summary but never abort the patch. Returns
/// `{ data, summary, seed }`.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn patch_rom(
    image: Vec<u8>,
    seed: &str,
    lang_pack: &str,
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
    jewel_fix: bool,
    fishing_prices: &str,
    location_renames: &str,
    earth_egg_price: &str,
    arts_powers: &str,
    arts_ap_grants: &str,
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
        DropMode::Shuffle => legaia_patcher::arts::ArtsMode::Shuffle,
        DropMode::Random => legaia_patcher::arts::ArtsMode::Random,
    });
    let door_mode = parse_mode(doors);
    let house_door_mode = parse_mode(house_doors);

    // Arts AP-grant and shiny-Seru reuse the same verified-dead SCUS arena bytes,
    // so they are mutually exclusive - refuse the combination before patching.
    if shiny_seru && !arts_ap_grants.trim().is_empty() {
        return Err(err(
            "arts-ap-grant and shiny-seru both inject into the same verified-dead SCUS arena \
             and are mutually exclusive; enable only one",
        ));
    }

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
        legaia_patcher::unused::extend_pool(&mut pool, legaia_patcher::unused::UNUSED_ITEM_IDS);
        apply::inject_seru_bell_name(&mut patcher).map_err(|e| err(format!("name inject: {e}")))?;
    }
    // The unused-enemy id set passed to the encounter randomizer (empty unless on).
    let unused_enemy_ids: &[u8] = if unused_enemies {
        legaia_patcher::unused::UNUSED_ENEMY_IDS
    } else {
        &[]
    };

    let mut summary = String::new();

    // Language pack, phase 1 of 2: the dialog sections (`man:` / `raw:` keys)
    // go FIRST, before any data randomization - a dialog edit is keyed by a
    // byte offset into a scene's decompressed MAN, and the door / starting-bag
    // passes relocate those records. The SCUS name sections go LAST (after
    // every randomizer pass), because passes that classify items by their
    // English names - the equipment-drop gear pool - must still see the
    // retail names; nothing in the randomizer relocates a SCUS string, so
    // translating them at the end is always safe.
    let lang_pack = lang_pack.trim();
    let parsed_pack = if lang_pack.is_empty() {
        None
    } else {
        Some(LanguagePack::from_yaml(lang_pack).map_err(|e| err(format!("language pack: {e}")))?)
    };
    let mut lang_report = ImportReport::default();
    if let Some(pack) = &parsed_pack {
        let report = import_pack_phase(&mut patcher, pack, ImportPhase::DialogOnly, false)
            .map_err(|e| err(format!("apply language pack (dialog): {e}")))?;
        lang_report.merge(report);
    }

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
            legaia_patcher::bonus_drop::DEFAULT_CHANCE_PCT,
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
        let rep = apply::inject_flee_exp(&mut patcher, legaia_patcher::flee_exp::DEFAULT_PCT)
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
        let rep = apply::inject_enemy_ally(&mut patcher, legaia_patcher::enemy_ally::DEFAULT_PCT)
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
        let rep = apply::inject_shiny_seru(&mut patcher, legaia_patcher::shiny_seru::DEFAULT_PCT)
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

    // Jewel fix: retarget the boss cinematic casts' damage calls from the
    // resist-ladder-bypassing wrapper to the guard-respecting one, so elemental
    // jewels / guards / All Guard apply to Xain's Bloody Horns / Terio Punch,
    // Cort's Guilty Cross, and the Delilas trio's signature moves. Seedless.
    if jewel_fix {
        let rep =
            apply::apply_jewel_fix(&mut patcher).map_err(|e| err(format!("jewel-fix: {e}")))?;
        summary.push_str(&format!(
            "jewel-fix: {} boss-cast damage calls now respect elemental guards\n",
            rep.sites_patched
        ));
    } else {
        summary.push_str("jewel-fix: untouched\n");
    }

    // Fishing-exchange price edits: a comma/semicolon/whitespace-separated list
    // of `item=points` pairs (item id decimal or 0xHH). Each sets the fishing
    // point cost of every prize row granting that item; the price also gates
    // when the prize appears. A malformed pair is reported and skipped rather
    // than aborting the whole patch.
    let fishing_prices = fishing_prices.trim();
    if fishing_prices.is_empty() {
        summary.push_str("fishing-price: untouched\n");
    } else {
        for tok in fishing_prices
            .split([',', ';', '\n', ' '])
            .filter(|t| !t.trim().is_empty())
        {
            match parse_id_eq_u32(tok) {
                Some((item_id, price)) => {
                    match apply::set_fishing_price(&mut patcher, item_id as u32, price) {
                        Ok(rep) if rep.edits.is_empty() => summary.push_str(&format!(
                            "fishing-price: item 0x{item_id:02X} already {price} points\n"
                        )),
                        Ok(rep) => {
                            for (page, _row, _id, old, new) in &rep.edits {
                                let venue = if *page == 0 { "Buma" } else { "Vidna" };
                                summary.push_str(&format!(
                                    "fishing-price: {venue} item 0x{item_id:02X}: {old} -> {new} points\n"
                                ));
                            }
                        }
                        Err(e) => summary.push_str(&format!("fishing-price: {e}\n")),
                    }
                }
                None => {
                    summary.push_str(&format!("fishing-price: skipped malformed entry {tok:?}\n"))
                }
            }
        }
    }

    // Earth Egg coin threshold: the Sol Tower Prize Counter's scripted
    // coin-for-Earth-Egg exchange (koin1 MAN). A single coins-required value
    // (empty = untouched); the game debits exactly that many on purchase.
    let earth_egg_price = earth_egg_price.trim();
    if earth_egg_price.is_empty() {
        summary.push_str("earth-egg-price: untouched\n");
    } else {
        match earth_egg_price.parse::<u32>() {
            Ok(price) => match apply::set_earth_egg_price(&mut patcher, price) {
                Ok(rep) if !rep.changed => {
                    summary.push_str(&format!("earth-egg-price: already {price} coins\n"))
                }
                Ok(rep) => summary.push_str(&format!(
                    "earth-egg-price: {} -> {} coins\n",
                    rep.old_price, rep.new_price
                )),
                Err(e) => summary.push_str(&format!("earth-egg-price: {e}\n")),
            },
            Err(_) => summary.push_str(&format!(
                "earth-egg-price: skipped non-numeric value {earth_egg_price:?}\n"
            )),
        }
    }

    // Location renames: newline-separated `index=name` lines (name may contain
    // spaces, so only the newline splits entries). Each is a same-size SCUS
    // slot overwrite; a bad entry is reported and skipped.
    let location_renames = location_renames.trim();
    if location_renames.is_empty() {
        summary.push_str("rename-location: untouched\n");
    } else {
        for line in location_renames.lines().filter(|l| !l.trim().is_empty()) {
            match line.split_once('=') {
                Some((idx_str, name)) => match idx_str.trim().parse::<usize>() {
                    Ok(index) => {
                        match apply::rename_locations(&mut patcher, &[(index, name.to_string())]) {
                            Ok(rep) if rep.renames.is_empty() => summary.push_str(&format!(
                                "rename-location: {index} already has that name\n"
                            )),
                            Ok(rep) => {
                                for (i, old, new) in &rep.renames {
                                    summary.push_str(&format!(
                                        "rename-location: {i} {old:?} -> {new:?}\n"
                                    ));
                                }
                            }
                            Err(e) => summary.push_str(&format!("rename-location: {e}\n")),
                        }
                    }
                    Err(_) => {
                        summary.push_str(&format!("rename-location: bad index in {line:?}\n"))
                    }
                },
                None => summary.push_str(&format!(
                    "rename-location: skipped malformed entry {line:?}\n"
                )),
            }
        }
    }

    // Arts damage-power edits: comma/space/newline-separated `COMBO=VALUE`
    // tokens (`RDLDL=0x16`). `VALUE` is a power-encoding byte (`0` disables, or
    // `0x0C..=0x1F` = a damage tier; lower = weaker). A bad entry is reported
    // and skipped.
    let arts_powers = arts_powers.trim();
    if arts_powers.is_empty() {
        summary.push_str("arts-power: untouched\n");
    } else {
        for tok in arts_powers
            .split([',', ';', '\n', ' '])
            .filter(|t| !t.trim().is_empty())
        {
            let parsed = tok.split_once('=').and_then(|(c, v)| {
                let combo = legaia_patcher::arts_power::parse_combo(c.trim())?;
                let vs = v.trim();
                let value = vs
                    .strip_prefix("0x")
                    .or_else(|| vs.strip_prefix("0X"))
                    .map(|h| u8::from_str_radix(h, 16))
                    .unwrap_or_else(|| vs.parse::<u8>())
                    .ok()?;
                (value == 0 || legaia_patcher::arts_power::is_power_byte(value))
                    .then_some((combo, value))
            });
            match parsed {
                Some((combo, value)) => {
                    match apply::set_arts_power(&mut patcher, &[(combo, value)]) {
                        Ok(rep) if rep.edits.is_empty() => {
                            summary.push_str(&format!("arts-power: {tok} unchanged\n"))
                        }
                        Ok(rep) => {
                            for e in &rep.edits {
                                let combo: String = e
                                    .combo
                                    .iter()
                                    .map(legaia_patcher::arts_power::command_glyph)
                                    .collect();
                                summary.push_str(&format!(
                                    "arts-power: {combo} ({:?}) -> {value:#04X}\n",
                                    e.character
                                ));
                            }
                        }
                        Err(e) => summary.push_str(&format!("arts-power: {e}\n")),
                    }
                }
                None => summary.push_str(&format!("arts-power: skipped malformed entry {tok:?}\n")),
            }
        }
    }

    // Arts AP-grant: comma/space/newline-separated `COMBO=AMOUNT` tokens
    // (`RDLDL=10`). `AMOUNT` (1..=100) is the AP granted per use; the art becomes
    // castable at any AP level and adds that much (clamped at 100) instead of
    // costing it. The config row is the arts-table index, shared across all three
    // characters. Mutually exclusive with shiny-seru (guarded above).
    let arts_ap_grants = arts_ap_grants.trim();
    if arts_ap_grants.is_empty() {
        summary.push_str("arts-ap-grant: untouched\n");
    } else {
        let mut grants = Vec::new();
        for tok in arts_ap_grants
            .split([',', ';', '\n', ' '])
            .filter(|t| !t.trim().is_empty())
        {
            let parsed = tok.split_once('=').and_then(|(c, v)| {
                let combo = legaia_patcher::arts_power::parse_combo(c.trim())?;
                let vs = v.trim();
                let amount = vs
                    .strip_prefix("0x")
                    .or_else(|| vs.strip_prefix("0X"))
                    .map(|h| u8::from_str_radix(h, 16))
                    .unwrap_or_else(|| vs.parse::<u8>())
                    .ok()?;
                (amount >= 1 && u16::from(amount) <= legaia_patcher::arts_ap_grant::AP_CAP)
                    .then_some((combo, amount))
            });
            match parsed {
                Some(g) => grants.push(g),
                None => {
                    summary.push_str(&format!("arts-ap-grant: skipped malformed entry {tok:?}\n"))
                }
            }
        }
        if grants.is_empty() {
            summary.push_str("arts-ap-grant: no valid entries\n");
        } else {
            match apply::inject_arts_ap_grant(&mut patcher, &grants) {
                Ok(rep) => {
                    for g in &rep.resolved {
                        let targeted = legaia_patcher::arts_ap_grant::combo_str(&g.targeted_combo);
                        let shared: Vec<String> = g
                            .shared
                            .iter()
                            .map(|(ch, name, _)| format!("{ch:?} {name:?}"))
                            .collect();
                        summary.push_str(&format!(
                            "arts-ap-grant: {targeted} -> row {} grants {} AP (shared: {})\n",
                            g.row,
                            g.amount,
                            shared.join("; ")
                        ));
                    }
                }
                Err(e) => summary.push_str(&format!("arts-ap-grant: {e}\n")),
            }
        }
    }

    match chest_mode {
        Some(m) => {
            // Protect every quest / key / story item by default (same disc-derived
            // set as the CLI), so the in-browser patcher behaves identically: no
            // quest item is moved out of its chest or dropped into another.
            let keep_static: std::collections::BTreeSet<u8> =
                match legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54") {
                    Some(scus) => legaia_patcher::items::default_static_chest_items(&scus),
                    None => legaia_patcher::items::DEFAULT_STATIC_CHEST_ITEMS
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
        Some(legaia_patcher::drops::DropMode::Shuffle) => {
            let rep = apply::randomize_house_doors(
                &mut patcher,
                seed_n,
                legaia_patcher::drops::DropMode::Shuffle,
            )
            .map_err(|e| err(format!("house-doors: {e}")))?;
            summary.push_str(&format!(
                "house-doors: {} of {} door-warp targets shuffled across {} scenes\n",
                rep.sites_changed, rep.sites_total, rep.scenes_changed
            ));
            summary.push_str(&format!(
                "map-doors: {} of {} kind-0 teleports rewired across {} scenes\n",
                rep.map.sites_changed, rep.map.sites_total, rep.map.scenes_changed
            ));
        }
        Some(_) => summary.push_str("house-doors: only `shuffle` supported; untouched\n"),
        None => summary.push_str("house-doors: untouched\n"),
    }

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
        let rep = apply::randomize_starting_items(&mut patcher, seed_n, &seed_opts)
            .map_err(|e| err(format!("starting-items: {e}")))?;
        // With a random fill requested, the seeded bag contains seed-derived
        // draws - listing their names would spoil the run before it starts.
        // Only the convenience toggles (which the user picked themselves) are
        // ever named; a randomized bag is reported count-only.
        if starting_items > 0 {
            summary.push_str(&format!(
                "starting-items: new game begins with {} item(s) (randomized - names hidden, no spoilers)\n",
                rep.items_set
            ));
        } else {
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
        }
        if rep.all_warps {
            summary.push_str("all-warps: every Door of Wind destination unlocked from the start\n");
        }
        // Items beyond the 7-slot direct-seed cap are granted on top via a silent
        // GIVE_ITEM block injected into the opening scene (see `starting_bag`), so
        // the explicit convenience items AND the full requested random fill land.
        let overflow = legaia_patcher::starting_items::overflow_bag(seed_n, &seed_opts);
        if !overflow.is_empty() {
            let bag = apply::apply_starting_bag(
                &mut patcher,
                &overflow,
                legaia_patcher::starting_bag::DEFAULT_GUARD_BIT,
            )
            .map_err(|e| err(format!("starting-items overflow: {e}")))?;
            if bag.applied {
                // Overflow slots are always part of the random fill - count
                // only, same no-spoiler rule as the direct seed above.
                summary.push_str(&format!(
                    "starting-items: + {} more via the opening scene\n",
                    overflow.len()
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

    if legaia_patcher::starting_level::is_active(starting_level) {
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

    // Language pack, phase 2 of 2: the SCUS name-table sections (see the
    // phase-1 comment above for why they come after every randomizer pass).
    let mut lang_line = String::from("language: untouched (English)\n");
    let mut lang_json = JsValue::NULL;
    if let Some(pack) = &parsed_pack {
        let report = import_pack_phase(&mut patcher, pack, ImportPhase::NamesOnly, false)
            .map_err(|e| err(format!("apply language pack (names): {e}")))?;
        lang_report.merge(report);
        let sections = lang_report.section_counts(pack);
        lang_line = format!(
            "language ({}): {} strings translated{}\n",
            pack.language,
            lang_report.applied + lang_report.already_applied,
            if lang_report.issues.is_empty() {
                String::new()
            } else {
                format!(
                    " ({} line(s) skipped - over budget, non-encodable or not on this disc)",
                    lang_report.issues.len()
                )
            }
        );
        // Per-section rows live in the `lang` JSON object; the page renders
        // them as the coverage block, so the text summary stays one line.
        lang_json = lang_report_json(&pack.language, &lang_report, &sections)?;
    }
    summary.insert_str(0, &lang_line);

    let patched = patcher.into_image();
    let data = Uint8Array::new_with_length(patched.len() as u32);
    data.copy_from(&patched);

    let out = Object::new();
    Reflect::set(&out, &"data".into(), &data)?;
    Reflect::set(&out, &"summary".into(), &summary.into())?;
    Reflect::set(&out, &"seed".into(), &seed_n.to_string().into())?;
    Reflect::set(&out, &"lang".into(), &lang_json)?;
    Ok(out.into())
}

/// Short human label for a skip diagnostic, for the per-reason breakdown the
/// page shows ("over budget", "does not recompress", ...).
fn issue_reason(msg: &str) -> &'static str {
    if msg.contains("recompresses") {
        "scene dialog does not recompress into its footprint"
    } else if msg.contains("budget") {
        "over budget"
    } else if msg.contains("not encodable") || msg.contains("doesn't encode") {
        "not encodable in the retail glyph set"
    } else if msg.contains("not built for this image")
        || msg.contains("don't match the pack source")
    {
        "not on this disc (wrong image or conflicting patch)"
    } else {
        "other (see console)"
    }
}

/// `{ language, applied, already_applied, skipped, untranslated, sections:
/// [{name, total, filled, applied, already_applied, skipped}], reasons:
/// [{reason, count}] }` - the per-section coverage report the page renders
/// after a language patch.
fn lang_report_json(
    language: &str,
    report: &ImportReport,
    sections: &[legaia_patcher::translation::SectionCounts],
) -> Result<JsValue, JsValue> {
    let out = Object::new();
    Reflect::set(&out, &"language".into(), &language.into())?;
    let num = |v: usize| JsValue::from_f64(v as f64);
    Reflect::set(&out, &"applied".into(), &num(report.applied))?;
    Reflect::set(
        &out,
        &"already_applied".into(),
        &num(report.already_applied),
    )?;
    Reflect::set(&out, &"skipped".into(), &num(report.issues.len()))?;
    Reflect::set(&out, &"untranslated".into(), &num(report.untranslated))?;
    let arr = js_sys::Array::new();
    for s in sections {
        let row = Object::new();
        Reflect::set(&row, &"name".into(), &s.name.into())?;
        Reflect::set(&row, &"total".into(), &num(s.total))?;
        Reflect::set(&row, &"filled".into(), &num(s.filled))?;
        Reflect::set(&row, &"applied".into(), &num(s.applied))?;
        Reflect::set(&row, &"already_applied".into(), &num(s.already_applied))?;
        Reflect::set(&row, &"skipped".into(), &num(s.skipped))?;
        arr.push(&row);
    }
    Reflect::set(&out, &"sections".into(), &arr)?;
    let mut reasons: std::collections::BTreeMap<&'static str, usize> =
        std::collections::BTreeMap::new();
    for (_, msg) in &report.issues {
        *reasons.entry(issue_reason(msg)).or_default() += 1;
    }
    let rarr = js_sys::Array::new();
    for (reason, count) in reasons {
        let row = Object::new();
        Reflect::set(&row, &"reason".into(), &reason.into())?;
        Reflect::set(&row, &"count".into(), &num(count))?;
        rarr.push(&row);
    }
    Reflect::set(&out, &"reasons".into(), &rarr)?;
    Ok(out.into())
}

/// Validate a `legaia-text-pack-v1` YAML document **against the user's own
/// disc**, client-side. Returns `{ ok, language, applied, skipped, message }`:
/// `applied` is how many entries would be written, `skipped` how many the disc
/// rejected (over budget or not matching this image), and `message` a short
/// human summary. This is the same dry run the CLI's `translate stats --input`
/// does - the only way to check a distributable pack's budgets, which are
/// hints until a disc is there to measure. Nothing is written.
#[wasm_bindgen]
pub fn validate_lang_pack(image: Vec<u8>, pack_yaml: &str) -> Result<JsValue, JsValue> {
    let pack = LanguagePack::from_yaml(pack_yaml).map_err(|e| err(format!("parse pack: {e}")))?;
    let mut patcher = DiscPatcher::open(image).map_err(|e| err(format!("parse disc: {e}")))?;
    let report = import_pack(&mut patcher, &pack).map_err(|e| err(format!("dry run: {e}")))?;
    let out = Object::new();
    Reflect::set(&out, &"ok".into(), &JsValue::from_bool(true))?;
    Reflect::set(&out, &"language".into(), &pack.language.as_str().into())?;
    Reflect::set(
        &out,
        &"applied".into(),
        &JsValue::from_f64(report.applied as f64),
    )?;
    Reflect::set(
        &out,
        &"skipped".into(),
        &JsValue::from_f64(report.issues.len() as f64),
    )?;
    let msg = format!(
        "{} strings would be translated, {} skipped (over budget or not on this disc)",
        report.applied,
        report.issues.len()
    );
    Reflect::set(&out, &"message".into(), &msg.into())?;
    let sections = report.section_counts(&pack);
    Reflect::set(
        &out,
        &"report".into(),
        &lang_report_json(&pack.language, &report, &sections)?,
    )?;
    Ok(out.into())
}

/// Lift the **official** French / German / Italian localization off a PAL disc
/// the user also owns, re-keyed onto their USA disc's coordinate space.
///
/// Same user-supplied-asset model as the base disc: `source_image` is the
/// user's own PAL `.bin` (`SCES_019.44` FR / `.45` DE / `.46` IT), it is read
/// in this tab, and neither image is uploaded anywhere. The result is a
/// **working** pack (`source:` = USA text, `translation:` = official text) that
/// the page feeds straight back into [`patch_rom`]'s `lang_pack` argument, so
/// the official text goes through the exact same two-phase import - and the
/// same per-section coverage report - as any community pack. Both discs are
/// consumed and dropped when this returns, so the caller can re-supply the USA
/// image for the patch run without holding two copies at once.
///
/// The pack is filled with the game's copyrighted text: it belongs in the
/// user's browser (or their own scratchpad), never in the repo.
///
/// `fold_accents` (recommended) rewrites the accented glyph cells the NTSC font
/// leaves empty onto plain ASCII - `Epee` for `Épée`. With it off the raw PAL
/// accent bytes are kept, which is byte-faithful but renders blank until the
/// font atlas is patched; either way the count is reported, never silent.
///
/// Returns `{ yaml, language, exe, summary, tables: [{name, located, pal_base,
/// valid_pct, paired}], names_filled, names_unmapped, party_filled,
/// party_total, man_total, man_paired, raw_total, raw_paired, folded,
/// unfolded }`.
#[wasm_bindgen]
pub fn lift_official_pack(
    target_image: Vec<u8>,
    source_image: Vec<u8>,
    fold_accents: bool,
) -> Result<JsValue, JsValue> {
    let target =
        DiscPatcher::open(target_image).map_err(|e| err(format!("parse USA disc: {e}")))?;
    let source =
        DiscPatcher::open(source_image).map_err(|e| err(format!("parse PAL disc: {e}")))?;
    let (mut pack, rep) =
        lift::lift_official(&target, &source).map_err(|e| err(format!("lift: {e}")))?;
    // Free the source disc as early as possible - two full images plus the pack
    // is the peak allocation of the whole page.
    drop(source);
    drop(target);

    let fold = if fold_accents {
        lift::fold_pack_accents(&mut pack)
    } else {
        Default::default()
    };
    let yaml = pack.to_yaml().map_err(|e| err(format!("emit YAML: {e}")))?;

    let num = |v: usize| JsValue::from_f64(v as f64);
    let out = Object::new();
    Reflect::set(&out, &"yaml".into(), &yaml.as_str().into())?;
    Reflect::set(&out, &"language".into(), &rep.language.as_str().into())?;
    Reflect::set(&out, &"exe".into(), &rep.exe_name.as_str().into())?;
    let tables = js_sys::Array::new();
    for t in &rep.tables {
        let row = Object::new();
        Reflect::set(&row, &"name".into(), &t.name.into())?;
        Reflect::set(&row, &"located".into(), &JsValue::from_bool(t.located))?;
        Reflect::set(
            &row,
            &"pal_base".into(),
            &format!("0x{:08x}", t.pal_base).into(),
        )?;
        Reflect::set(
            &row,
            &"valid_pct".into(),
            &JsValue::from_f64(t.valid_fraction * 100.0),
        )?;
        Reflect::set(&row, &"paired".into(), &num(t.paired))?;
        tables.push(&row);
    }
    Reflect::set(&out, &"tables".into(), &tables)?;
    Reflect::set(&out, &"names_filled".into(), &num(rep.names_filled))?;
    Reflect::set(&out, &"names_unmapped".into(), &num(rep.names_unmapped))?;
    Reflect::set(&out, &"party_filled".into(), &num(rep.party_filled))?;
    Reflect::set(&out, &"party_total".into(), &num(rep.party_total))?;
    Reflect::set(&out, &"man_total".into(), &num(rep.man_total))?;
    Reflect::set(&out, &"man_paired".into(), &num(rep.man_paired))?;
    Reflect::set(&out, &"raw_total".into(), &num(rep.raw_total))?;
    Reflect::set(&out, &"raw_paired".into(), &num(rep.raw_paired))?;
    Reflect::set(&out, &"folded".into(), &num(fold.folded))?;
    Reflect::set(&out, &"unfolded".into(), &num(fold.unmapped))?;

    // A short text block for the status panel. Counts only - no game text.
    let mut summary = format!(
        "lifted the official {} localization from {}\n",
        rep.language, rep.exe_name
    );
    for t in &rep.tables {
        summary.push_str(&if t.located {
            format!(
                "  {}: located @ 0x{:08x} ({:.0}% valid), {} names paired\n",
                t.name,
                t.pal_base,
                t.valid_fraction * 100.0,
                t.paired
            )
        } else {
            format!("  {}: NOT located - left English\n", t.name)
        });
    }
    summary.push_str(&format!(
        "  party names: {}/{} paired\n  scene dialog: {}/{} lines paired\n  \
         event-script text: {}/{} lines paired\n",
        rep.party_filled,
        rep.party_total,
        rep.man_paired,
        rep.man_total,
        rep.raw_paired,
        rep.raw_total
    ));
    summary.push_str(&if fold_accents {
        format!(
            "  accents: {} folded to ASCII ({} non-accent symbol cell(s) left as-is)\n",
            fold.folded, fold.unmapped
        )
    } else {
        "  accents: kept as PAL bytes - they render blank without a font patch\n".to_string()
    });
    summary.push_str(
        "  menu / system UI strings: not lifted - the overlay string pools sit at \
         region-specific addresses, so those labels stay English\n",
    );
    summary.push_str(
        "Lifting only re-keys the text; how much of it fits the USA disc's \
         sector-aligned scenes is the coverage report after patching.\n",
    );
    Reflect::set(&out, &"summary".into(), &summary.as_str().into())?;
    Ok(out.into())
}

/// Export a **working** language pack (source-bearing, all `translation:`
/// fields empty) from the user's own disc, as YAML text they can download and
/// fill in. This is the authoring on-ramp - the community can produce their own
/// packs without any tooling beyond the browser. The exported text is the
/// user's own disc data and never leaves the browser.
///
/// `language` stamps the pack header (`fr`, `de`, ...); pass `en` for a plain
/// source dump. Returns the YAML string.
#[wasm_bindgen]
pub fn export_lang_pack(image: Vec<u8>, language: &str) -> Result<String, JsValue> {
    let patcher = DiscPatcher::open(image).map_err(|e| err(format!("parse disc: {e}")))?;
    let pack = export_pack(&patcher).map_err(|e| err(format!("export: {e}")))?;
    let pack = if language.is_empty() || language == "en" {
        pack
    } else {
        pack.into_skeleton(language, Vec::new())
    };
    pack.to_yaml().map_err(|e| err(format!("emit YAML: {e}")))
}
