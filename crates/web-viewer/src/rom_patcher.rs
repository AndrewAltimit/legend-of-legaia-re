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
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

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

/// Resolve after one `setTimeout(..., 0)` macrotask, so the browser can
/// repaint between the synchronous patch stages. A microtask
/// (`Promise.resolve()`) is not enough - the renderer only paints at a
/// macrotask boundary. Looked up via the global object so it works in both
/// window and worker scopes; a scope without `setTimeout` resolves
/// immediately (no paint, but the patch still completes).
async fn macrotask_yield() {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let global = js_sys::global();
        let scheduled = Reflect::get(&global, &JsValue::from_str("setTimeout"))
            .ok()
            .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
            .and_then(|f| f.call2(&global, &resolve, &JsValue::from(0)).ok());
        if scheduled.is_none() {
            let _ = resolve.call0(&JsValue::NULL);
        }
    });
    let _ = JsFuture::from(promise).await;
}

/// Stage-progress reporter for the async patch entry points. Holds the
/// page-supplied JS callback (when one is passed); each [`Progress::stage`]
/// invokes it with `(stage_index, stage_count, label)` and then yields one
/// macrotask so the page's bar actually paints. Stages are the feature
/// blocks the patch applies sequentially - a couple dozen yields per run,
/// never one per inner-loop item. Without a callback nothing is reported
/// and nothing yields.
struct Progress {
    callback: Option<js_sys::Function>,
    index: u32,
    count: u32,
}

impl Progress {
    fn new(callback: Option<js_sys::Function>, count: u32) -> Self {
        Self {
            callback,
            index: 0,
            count,
        }
    }

    async fn stage(&mut self, label: &str) {
        let idx = self.index;
        self.index += 1;
        let Some(cb) = &self.callback else { return };
        let _ = cb.call3(
            &JsValue::NULL,
            &JsValue::from(idx),
            &JsValue::from(self.count),
            &JsValue::from_str(label),
        );
        macrotask_yield().await;
    }
}

/// Parse one arts-AP token: `[CHARACTER:]COMBO=AMOUNT` (`Vahn:RDLDL=10`,
/// `RDLDL=10`). `grant` picks the mode. Returns `None` on any malformed token
/// (the caller reports it in the summary and carries on).
fn parse_art_ap_token(tok: &str, grant: bool) -> Option<legaia_patcher::arts_ap_grant::ArtApSpec> {
    use legaia_art::queue::Character;
    use legaia_patcher::arts_ap_grant::{AP_CAP, ApMode, ArtApSpec};
    let (lhs, val_str) = tok.trim().split_once('=')?;
    let (character, combo_str) = match lhs.split_once(':') {
        Some((c, rest)) => {
            let ch = match c.trim().to_ascii_lowercase().as_str() {
                "vahn" => Character::Vahn,
                "noa" => Character::Noa,
                "gala" => Character::Gala,
                _ => return None,
            };
            (Some(ch), rest)
        }
        None => (None, lhs),
    };
    let combo = legaia_patcher::arts_power::parse_combo(combo_str.trim())?;
    let vs = val_str.trim();
    let amount = vs
        .strip_prefix("0x")
        .or_else(|| vs.strip_prefix("0X"))
        .map(|h| u8::from_str_radix(h, 16))
        .unwrap_or_else(|| vs.parse::<u8>())
        .ok()?;
    if amount < 1 || u16::from(amount) > AP_CAP {
        return None;
    }
    Some(ArtApSpec {
        character,
        combo,
        mode: if grant {
            ApMode::Grant(amount)
        } else {
            ApMode::Cost(amount)
        },
    })
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

/// Number of `prog.stage(..)` boundaries in [`patch_rom`] (the `stage_count`
/// every progress-callback invocation carries).
const PATCH_ROM_STAGES: u32 = 36;

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
/// not a randomization - it is seedless). `delilas_challenge` adds a fourth
/// Muscle Dome enrollment option: a new 2-round dome course - Che & Lu
/// Delilas double-team, then Gi - unlocked by the Koru event; losing a round
/// returns to the venue by the dome's design - no game over (seedless).
/// `custom_items` injects three brand-new items into cut item slots
/// (Nature's Elixir / Ra-Seru Tear / Fury Bloom) - standalone: with a
/// `random` drop / chest / steal mode they join the fill pool, and with
/// `delilas_challenge` they replace the Honey as the course's full-clear
/// reward. `approach_softlock_fix` re-stages a
/// monster's approach animation when it dies mid-approach (the summon-then-
/// melee clip death that parks the battle in an infinite range poll - the
/// "endless camera orbit" softlock), so the monster resumes walking instead
/// of wedging the fight; healthy fights are byte-identical (also seedless).
/// `fishing_prices` is a
/// comma/space-separated list of `item=points` pairs that set the
/// fishing-exchange point cost of prizes (e.g. `0x6F=500` for the Water Egg).
/// `location_renames` is a newline-separated list of `index=name` lines that
/// rename world-map location slots (e.g. `3=Ancient Fire Cave`).
/// `earth_egg_price` (empty = untouched) sets the casino-coin threshold the Sol
/// Tower Prize Counter requires before it offers the Earth Ra-Seru Egg (retail
/// 100000); the game debits exactly that many coins on purchase. `arts_powers`
/// is a comma/space-separated list of `combo=value` pairs that rebalance a
/// Tactical Art's damage-power bytes (e.g. `RDLDL=0x16`; `value` a power byte
/// `0x0C..=0x1F` or `0`). `super_art_powers` is the Super Art sibling - a
/// comma/newline-separated (never space-separated, the names contain spaces)
/// list of `name=value` pairs (e.g. `Tri-Somersault=0x1A`), rebalancing a Super
/// Art's own `record0` power bytes; Super Arts carry no combo, no
/// arts-name-table row and no AP cost of their own, so name is the only key
/// they have. `show_super_arts` lists a character's Super Arts on the
/// Tactical-Arts list the Triangle button opens in battle, which retail never
/// draws at all: a row appears once the player has **performed** that Super Art
/// (a per-character byte the Super applier's detour records, saved with the
/// character), sits among the regular arts **by AP**, and shows the Super Art's
/// name, the chain's summed AP cost and the arrows the player types; the pause
/// menu's Status screen (Left = Moves) lists them the same way. The
/// Triangle caption's own page thresholds stay retail, so on a later page it can
/// still read "View Hyper Arts list". Mutually exclusive with
/// `shiny_seru`, the arts AP override and `delilas_challenge` (same SCUS
/// regions). `arts_ap_grants` and `arts_ap_costs` are
/// comma/space-separated lists of `[character:]combo=amount` pairs (e.g.
/// `Vahn:RDLDL=10`; `amount` 1..=100 AP): a grant makes the art castable at any
/// AP level and *add* that much, a cost charges exactly that much instead of
/// retail's computed value, and both rewrite the art's menu AP number (a grant
/// shows `0`). Each entry keys on `(character, arts row)`, so one character's
/// art never moves another's. Mutually exclusive with `shiny_seru` (same SCUS
/// regions). `spirit_ap` (empty = untouched) sets how much AP the Spirit
/// command charges into the battle gauge (retail 32; `0` = defence boost
/// only, `100` = one press fills the gauge, negative = Spirit drains the
/// gauge) - four immediate words in the battle overlay (the accrual plus the
/// gauge-widget ramp targets that mirror it). `damage_ap` (empty = untouched)
/// sets how much AP taking damage grants, as AP per 100% of max HP lost
/// (retail 100; `0` = damage never feeds the gauge, negative = being hit
/// drains it) - the damage finisher's scale chain in the same overlay. A
/// negative value on either knob also neutralizes the AP-Boost accessory
/// arms, which read the accrual unsigned. `enemy_stat_scale` (empty or `1` =
/// untouched) multiplies every monster's combat stats by a difficulty factor
/// (`0.1`..`5`), story bosses included; it moves nothing between monsters, so
/// each keeps its own profile while the whole roster shifts together, and it is
/// applied after `monster_stats` so the two compose. It takes either spelling of
/// the knob - a bare multiplier (`"2.5"`) scales every stat, and a `key=value`
/// list (`"hp=2,attack=1.5"`) scales only the stats it names, and a
/// `|`-separated per-group split (`"regular:0.75|boss:2"`, each half itself
/// either of the first two spellings) gives random encounters and scripted boss
/// fights their own scale - which is what the page's simple and advanced slider
/// panes send. `exp_scale` (empty or `1` = untouched) multiplies every
/// monster's base EXP reward (`0.1`..`5`) - the victory spoils, the party
/// split, and the flee-EXP grant all read that record field, so one edit
/// scales them together; gold and drops stay retail, a scaled reward floors
/// at 1 EXP and saturates at 65535. `seru_catch_rate` (empty = untouched)
/// overrides every capturable Seru monster's catch chance with one flat
/// percent (`0`..`100`) - the odds that a killing blow absorbs its magic;
/// only the 63 capturable records are touched, so a non-Seru monster can
/// never become capturable. `enemy_attack_count` (empty or `1` = untouched)
/// scales how many hits enemies land with their standard physical attacks
/// (`0.1`..`5`): retail prices each attack in AGL against a per-round AGL
/// gauge, so this divides each attack entry's AGL-cost byte by the
/// multiplier while leaving AGL itself (and every spell cast) alone; costs
/// round half up and clamp so a retail attacker always lands at least one
/// hit per attack turn, never zero, and the engine's own 15-action queue
/// bounds the top end. These are all manual, seedless edits.
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
///
/// Async: the optional trailing `progress` callback is invoked with
/// `(stage_index, stage_count, label)` at each feature-stage boundary, and
/// the function yields one macrotask after each call so the page can paint
/// a progress bar instead of looking hanged for the whole run. Without the
/// callback no yields happen and the patch runs straight through.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub async fn patch_rom(
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
    approach_softlock_fix: bool,
    delilas_challenge: bool,
    custom_items: bool,
    fishing_prices: &str,
    location_renames: &str,
    earth_egg_price: &str,
    arts_powers: &str,
    arts_ap_grants: &str,
    arts_ap_costs: &str,
    spirit_ap: &str,
    damage_ap: &str,
    enemy_stat_scale: &str,
    exp_scale: &str,
    seru_catch_rate: &str,
    delilas_party: &str,
    delilas_arts_voice: &str,
    delilas_moves: &str,
    super_art_powers: &str,
    show_super_arts: bool,
    enemy_attack_count: &str,
    progress: Option<js_sys::Function>,
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

    // Arts AP-grant, shiny-Seru, and the Delilas Challenge dome course reuse the
    // same verified-dead SCUS arena bytes (0x8007AE00). Arts AP-grant is a hard
    // conflict with either (manual-only); shiny-Seru vs the Delilas Challenge is
    // resolved softly below (the challenge wins). Refuse the hard combos here.
    let arts_ap = !(arts_ap_grants.trim().is_empty() && arts_ap_costs.trim().is_empty());
    if arts_ap && shiny_seru {
        return Err(err(
            "the arts AP override and shiny-seru both inject into the same verified-dead SCUS \
             regions and are mutually exclusive; enable only one",
        ));
    }
    if arts_ap && delilas_challenge {
        return Err(err(
            "the arts AP override and the Delilas Challenge both inject into the same \
             verified-dead SCUS regions and are mutually exclusive; enable only one",
        ));
    }
    // The Super Arts move-list rows span all four verified-dead SCUS regions
    // (a shared unlock leaf, three hook routines and four small tables), so
    // they are a hard conflict with all three - the Delilas Challenge included,
    // which is never silently dropped in their favour.
    for (other, what) in [
        (arts_ap, "the arts AP override"),
        (shiny_seru, "shiny-seru"),
        (delilas_challenge, "the Delilas Challenge"),
    ] {
        if show_super_arts && other {
            return Err(err(format!(
                "showing Super Arts on the move list and {what} both inject into the same \
                 verified-dead SCUS regions and are mutually exclusive; enable only one"
            )));
        }
    }

    let mut prog = Progress::new(progress, PATCH_ROM_STAGES);
    prog.stage("parsing disc image").await;
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

    // Custom items: inject the standalone item set (records, effect machinery,
    // battle hooks - no Delilas dependency) and widen the random-fill pool so
    // the `random` drop/chest/steal modes can hand them out. With the Delilas
    // Challenge also on, they become the course's full-clear reward (the grant
    // half is installed with the challenge below).
    prog.stage("custom items").await;
    if custom_items {
        match apply::inject_custom_item_set(&mut patcher) {
            Ok(true) => summary
                .push_str("custom-items: injected Nature's Elixir, Ra-Seru Tear, Fury Bloom\n"),
            Ok(false) => summary.push_str("custom-items: already injected\n"),
            Err(e) => return Err(err(format!("custom-items: {e:#}"))),
        }
        if needs_pool {
            legaia_patcher::unused::extend_pool(
                &mut pool,
                legaia_patcher::custom_items::CUSTOM_ITEM_IDS,
            );
        } else if !delilas_challenge {
            summary.push_str(
                "  note: without a random drop/chest/steal mode or the Delilas Challenge, \
                 the custom items exist but nothing hands them out\n",
            );
        }
    }

    // Language pack, phase 1 of 2: the dialog sections (`man:` / `raw:` keys)
    // go FIRST, before any data randomization - a dialog edit is keyed by a
    // byte offset into a scene's decompressed MAN, and the door / starting-bag
    // passes relocate those records. The SCUS name sections go LAST (after
    // every randomizer pass), because passes that classify items by their
    // English names - the equipment-drop gear pool - must still see the
    // retail names; nothing in the randomizer relocates a SCUS string, so
    // translating them at the end is always safe.
    prog.stage("language pack: dialog text").await;
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

    prog.stage("monster drops").await;
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
    prog.stage("equipment bonus drops").await;
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

    prog.stage("random encounters").await;
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
            if rep.battle_load_capped > 0 {
                summary.push_str(&format!(
                    "  battle-load cap: {} formation(s) reduced to fit the battle heap\n",
                    rep.battle_load_capped
                ));
            }
        }
        None => summary.push_str("encounters: untouched\n"),
    }

    // Run-away EXP: a code hook in the escape teardown banks a slice of a fled
    // fight's experience into the party (vanilla gives nothing for fleeing).
    prog.stage("run-away EXP hook").await;
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
    prog.stage("enemy-ally hook").await;
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
    // It shares the verified-dead SCUS arena bytes (0x8007AE00) with the
    // Delilas Challenge's dome-course cave, so the two cannot coexist; the
    // Delilas Challenge takes precedence and shiny-Seru yields with a note.
    prog.stage("shiny Seru").await;
    if shiny_seru && delilas_challenge {
        summary.push_str(
            "shiny-seru: skipped (shares SCUS arena bytes with the Delilas Challenge, which wins)\n",
        );
    } else if shiny_seru {
        let rep = apply::inject_shiny_seru(&mut patcher, legaia_patcher::shiny_seru::DEFAULT_PCT)
            .map_err(|e| err(format!("shiny-seru: {e}")))?;
        summary.push_str(&format!(
            "shiny-seru: {}% chance per battle a capturable enemy is shiny (+35% stats / damage)\n",
            rep.pct
        ));
    } else {
        summary.push_str("shiny-seru: untouched\n");
    }

    // Show Super Arts: the in-battle Tactical-Arts list gains, sorted in by AP,
    // the Super Arts the character has performed - name, chain AP cost and the
    // arrows the player types per row (two detours into the SCUS list renderer
    // plus their routines and tables in dead space, a replaced list pager in
    // PROT 0898 whose tail records a performed Super Art, and a detour from the
    // Super applier into it). Spoiler-safe: the count, never the roster.
    prog.stage("Super Arts move list").await;
    if show_super_arts {
        let rep = apply::inject_super_art_list(&mut patcher)
            .map_err(|e| err(format!("show-super-arts: {e}")))?;
        summary.push_str(&format!(
            "show-super-arts: {} Super Arts join the in-battle move list and the status \
             screen's Moves page once performed\n",
            rep.rows.len()
        ));
    } else {
        summary.push_str("show-super-arts: untouched\n");
    }

    // Seru trading: a vendor in shops offers to trade a party member's Seru-magic for
    // a different one (time-bucketed, deterministic from the seed). All code + data is
    // hosted in the menu overlay, so it composes with every other feature here.
    prog.stage("Seru trading").await;
    if seru_trade {
        apply::inject_trade_full(&mut patcher, seed_n)
            .map_err(|e| err(format!("seru-trade: {e}")))?;
        // Also embed the engine-facing config blob (same seed), so the patched
        // disc trades identically when booted in the clean-room engine.
        apply::enable_seru_trades(
            &mut patcher,
            seed_n,
            legaia_asset::seru_trade::DEFAULT_MAX_OFFERS,
        )
        .map_err(|e| err(format!("seru-trade config: {e}")))?;
        summary.push_str("seru-trade: in-shop Seru trading vendor enabled\n");
    } else {
        summary.push_str("seru-trade: untouched\n");
    }

    // Jewel fix: retarget the boss cinematic casts' damage calls from the
    // resist-ladder-bypassing wrapper to the guard-respecting one, so elemental
    // jewels / guards / All Guard apply to Xain's Bloody Horns / Terio Punch,
    // Cort's Guilty Cross, and the Delilas trio's signature moves. Seedless.
    prog.stage("jewel fix").await;
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

    // Attack-approach softlock fix: nine words in the battle overlay so a
    // monster whose approach animation dies mid-approach is re-staged and
    // resumes walking instead of parking the battle in the state-0x19 range
    // poll forever. Seedless.
    prog.stage("approach-softlock fix").await;
    if approach_softlock_fix {
        let rep = apply::apply_approach_fix(&mut patcher)
            .map_err(|e| err(format!("approach-softlock-fix: {e}")))?;
        summary.push_str(if rep.changed {
            "approach-softlock-fix: battle overlay patched (dead approach animations are re-staged)\n"
        } else {
            "approach-softlock-fix: already applied\n"
        });
    } else {
        summary.push_str("approach-softlock-fix: untouched\n");
    }

    // Delilas Challenge: a fourth Muscle Dome enrollment option that runs a
    // brand-new 2-round dome course - Che & Lu double-team (1v2), then Gi -
    // routed through the real arena (magic-off), gated on the Koru event in
    // Nivora Ravine. The 1v2 fits the battle heap via slim clones at
    // unreachable archive slots; the originals are untouched. Losing a round
    // returns to the venue by the dome's design; a full clear pays 5000
    // coins. koin1 script edit + a companion arena/SCUS code injection;
    // seedless. A koin1 MAN another edit has already grown past its
    // zero-slack footprint skips with a note instead of failing the run.
    prog.stage("Delilas Challenge").await;
    if delilas_challenge {
        match apply::apply_delilas_challenge(&mut patcher, custom_items) {
            Ok(rep) if rep.changed => summary.push_str(&format!(
                "delilas-challenge: Muscle Dome enrollment offers the Delilas Challenge \
                 (a 2-round dome course: Che & Lu double-team, then Gi; a full clear pays \
                 5000 coins + {}; unlocks after Nivora Ravine)\n",
                if custom_items {
                    "the three custom items"
                } else {
                    "a Honey"
                }
            )),
            Ok(_) => summary.push_str("delilas-challenge: already applied\n"),
            Err(e) => summary.push_str(&format!("delilas-challenge: skipped ({e:#})\n")),
        }
    } else {
        summary.push_str("delilas-challenge: untouched\n");
    }

    // Fishing-exchange price edits: a comma/semicolon/whitespace-separated list
    // of `item=points` pairs (item id decimal or 0xHH). Each sets the fishing
    // point cost of every prize row granting that item; the price also gates
    // when the prize appears. A malformed pair is reported and skipped rather
    // than aborting the whole patch.
    prog.stage("prices and gauge tuning").await;
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

    // Spirit AP: the AP the Spirit command charges into the battle gauge
    // (retail 32; 0 = defence-only, 100 = full gauge, negative = Spirit drains
    // the gauge). A single value (empty = untouched); four immediate words in
    // the battle overlay, plus the signed accrual tail when negative.
    let spirit_ap = spirit_ap.trim();
    if spirit_ap.is_empty() {
        summary.push_str("spirit-ap: untouched\n");
    } else {
        match spirit_ap.parse::<i16>() {
            Ok(ap) if (-100..=100).contains(&ap) => {
                match apply::apply_spirit_ap(&mut patcher, ap) {
                    Ok(rep) if !rep.changed => {
                        summary.push_str(&format!("spirit-ap: already {ap} AP per Spirit\n"))
                    }
                    Ok(rep) => summary.push_str(&format!(
                        "spirit-ap: {} -> {ap} AP per Spirit (retail 32)\n",
                        rep.previous
                    )),
                    Err(e) => summary.push_str(&format!("spirit-ap: {e}\n")),
                }
            }
            _ => summary.push_str(&format!(
                "spirit-ap: skipped out-of-range value {spirit_ap:?} (want -100..=100)\n"
            )),
        }
    }

    // Enemy-damage AP: AP granted per 100% of max HP lost (retail 100; 0 =
    // damage never feeds the gauge, negative = being hit drains it). A single
    // value (empty = untouched); the damage finisher's scale chain in the
    // battle overlay, plus its accrual tail when negative.
    let damage_ap = damage_ap.trim();
    if damage_ap.is_empty() {
        summary.push_str("damage-ap: untouched\n");
    } else {
        match damage_ap.parse::<i16>() {
            Ok(v) if (-200..=200).contains(&v) => match apply::apply_damage_ap(&mut patcher, v) {
                Ok(rep) if !rep.changed => summary.push_str(&format!(
                    "damage-ap: already {v} AP per 100% max-HP damage\n"
                )),
                Ok(rep) => summary.push_str(&format!(
                    "damage-ap: {} -> {v} AP per 100% max-HP damage (retail 100)\n",
                    rep.previous
                )),
                Err(e) => summary.push_str(&format!("damage-ap: {e}\n")),
            },
            _ => summary.push_str(&format!(
                "damage-ap: skipped out-of-range value {damage_ap:?} (want -200..=200)\n"
            )),
        }
    }

    // Place renames: newline-separated `target=name` lines (a name may contain
    // spaces, so only the newline splits entries). `target` is a landmark index
    // or the place's current name. Each rename propagates to all three carriers
    // - the SCUS quick-travel cell, the world-map labels, and the scene-entry
    // banners - so one line changes every place the game shows the name. A bad
    // entry is reported and skipped.
    prog.stage("location renames").await;
    let location_renames = location_renames.trim();
    if location_renames.is_empty() {
        summary.push_str("rename-location: untouched\n");
    } else {
        let mut targets = Vec::new();
        for line in location_renames.lines().filter(|l| !l.trim().is_empty()) {
            match line.split_once('=') {
                Some((target, name)) if !target.trim().is_empty() => {
                    targets.push((apply::RenameTarget::parse(target), name.to_string()));
                }
                _ => summary.push_str(&format!(
                    "rename-location: skipped malformed entry {line:?}\n"
                )),
            }
        }
        match apply::rename_locations_by_target(&mut patcher, &targets) {
            Ok(rep) => {
                for (i, old, new) in &rep.renames {
                    summary.push_str(&format!(
                        "rename-location: landmark {i} {old:?} -> {new:?}\n"
                    ));
                }
                summary.push_str(&format!(
                    "rename-location: {} world-map label(s), {} scene banner(s)\n",
                    rep.world_map_records, rep.scene_banners
                ));
                for name in &rep.unmatched {
                    summary.push_str(&format!("rename-location: {name:?} matched no place\n"));
                }
                for idx in &rep.skipped {
                    summary.push_str(&format!(
                        "rename-location: scene bundle {idx} left vanilla (would not fit)\n"
                    ));
                }
                if rep.is_empty() {
                    summary.push_str("rename-location: nothing changed (names already match)\n");
                }
            }
            Err(e) => summary.push_str(&format!("rename-location: {e}\n")),
        }
    }

    // Arts damage-power edits: comma/space/newline-separated `COMBO=VALUE`
    // tokens (`RDLDL=0x16`). `VALUE` is a power-encoding byte (`0` disables, or
    // `0x0C..=0x1F` = a damage tier; lower = weaker). A bad entry is reported
    // and skipped.
    prog.stage("arts tuning").await;
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

    // Super Art damage-power edits: `NAME=VALUE` tokens
    // (`Tri-Somersault=0x1A`). Super Art names contain spaces, so this field
    // splits on commas / semicolons / newlines only - never on a space. A Super
    // Art has no combo and no arts-name-table row, so it is addressed by name
    // and located through its finisher action constant in the character's own
    // `record0` art block. A bad entry is reported and skipped.
    let super_art_powers = super_art_powers.trim();
    if super_art_powers.is_empty() {
        summary.push_str("super-art-power: untouched\n");
    } else {
        for tok in super_art_powers
            .split([',', ';', '\n'])
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            let parsed = tok.split_once('=').and_then(|(n, v)| {
                let hits = legaia_patcher::super_art_power::find_super_art(n.trim(), None);
                let art = (hits.len() == 1).then_some(hits[0])?;
                let vs = v.trim();
                let value = vs
                    .strip_prefix("0x")
                    .or_else(|| vs.strip_prefix("0X"))
                    .map(|h| u8::from_str_radix(h, 16))
                    .unwrap_or_else(|| vs.parse::<u8>())
                    .ok()?;
                legaia_patcher::super_art_power::is_accepted_power(value).then_some((art, value))
            });
            match parsed {
                Some((art, value)) => {
                    match apply::set_super_art_power(&mut patcher, &[(art, value)]) {
                        Ok(rep) if rep.edits.is_empty() => {
                            summary.push_str(&format!("super-art-power: {} unchanged\n", art.name))
                        }
                        Ok(rep) => {
                            for e in &rep.edits {
                                summary.push_str(&format!(
                                    "super-art-power: {} ({:?}) -> {value:#04X}\n",
                                    e.name, e.character
                                ));
                            }
                        }
                        Err(e) => summary.push_str(&format!("super-art-power: {e}\n")),
                    }
                }
                None => summary.push_str(&format!(
                    "super-art-power: skipped malformed entry {tok:?}\n"
                )),
            }
        }
    }

    // Arts AP override: comma/space/newline-separated `[CHARACTER:]COMBO=AMOUNT`
    // tokens (`Vahn:RDLDL=10`). In `arts_ap_grants` the amount is AP granted per
    // use (the art becomes castable at any AP level and adds that much, clamped
    // at 100); in `arts_ap_costs` it is the flat AP the art charges, replacing
    // retail's computed `multiplier x command count`. Each entry lands in its own
    // per-(character, row) config cell, so one character's art never moves
    // another's. Mutually exclusive with shiny-seru (guarded above).
    let arts_ap_grants = arts_ap_grants.trim();
    let arts_ap_costs = arts_ap_costs.trim();
    if arts_ap_grants.is_empty() && arts_ap_costs.is_empty() {
        summary.push_str("arts-ap: untouched\n");
    } else {
        let mut specs: Vec<legaia_patcher::arts_ap_grant::ArtApSpec> = Vec::new();
        for (src, grant) in [(arts_ap_grants, true), (arts_ap_costs, false)] {
            for tok in src
                .split([',', ';', '\n', ' '])
                .filter(|t| !t.trim().is_empty())
            {
                match parse_art_ap_token(tok, grant) {
                    Some(s) => specs.push(s),
                    None => {
                        summary.push_str(&format!("arts-ap: skipped malformed entry {tok:?}\n"))
                    }
                }
            }
        }
        if specs.is_empty() {
            summary.push_str("arts-ap: no valid entries\n");
        } else {
            match apply::inject_arts_ap_grant(&mut patcher, &specs) {
                Ok(rep) => {
                    for g in &rep.resolved {
                        let combo = legaia_patcher::arts_ap_grant::combo_str(&g.combo);
                        let what = if g.mode.is_grant() {
                            format!("grants {} AP", g.mode.amount())
                        } else {
                            format!("costs {} AP", g.mode.amount())
                        };
                        summary.push_str(&format!(
                            "arts-ap: {:?} {combo} {:?} {what} (menu list now reads {}, was {})\n",
                            g.character, g.name, g.display_ap, g.previous_display_ap
                        ));
                    }
                }
                Err(e) => summary.push_str(&format!("arts-ap: {e}\n")),
            }
        }
    }

    prog.stage("treasure chests").await;
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

    prog.stage("town shops").await;
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

    prog.stage("casino prizes").await;
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

    prog.stage("monster stats").await;
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

    // Enemy difficulty scale: a multiplier over every monster's combat stats,
    // with its own value for random encounters and for bosses (empty or `1` =
    // retail). Sequenced after the stat randomizer so it scales whatever that
    // pass dealt out. The per-group split rides inside this same string - the
    // page emits `regular:...|boss:...` when the two halves differ - so widening
    // the knob needed no new argument on this boundary.
    prog.stage("enemy tuning scales").await;
    let enemy_stat_scale = enemy_stat_scale.trim();
    if enemy_stat_scale.is_empty() {
        summary.push_str("enemy-stat-scale: 1x (retail)\n");
    } else {
        match legaia_patcher::monster_stats::ScaleProfile::parse(enemy_stat_scale) {
            Ok(scale) if scale.is_retail() => {
                summary.push_str("enemy-stat-scale: 1x (retail)\n");
            }
            Ok(scale) => match apply::scale_monster_stats_profile(&mut patcher, scale) {
                Ok(rep) if scale.is_uniform() => summary.push_str(&format!(
                    "enemy-stat-scale: {scale} ({} monsters changed, {} stats)\n",
                    rep.monsters_changed, rep.fields_changed
                )),
                Ok(rep) => summary.push_str(&format!(
                    "enemy-stat-scale: {scale} ({} monsters changed incl. {} bosses, {} stats)\n",
                    rep.monsters_changed, rep.bosses_changed, rep.fields_changed
                )),
                Err(e) => summary.push_str(&format!("enemy-stat-scale: {e}\n")),
            },
            Err(e) => summary.push_str(&format!("enemy-stat-scale: skipped - {e}\n")),
        }
    }

    // EXP multiplier: scales every monster's base-EXP reward halfword (empty
    // or 1 = retail). Seedless, same shape as the difficulty scale above.
    let exp_scale = exp_scale.trim();
    if exp_scale.is_empty() {
        summary.push_str("exp-scale: 1x (retail)\n");
    } else {
        match legaia_patcher::monster_stats::ScalePermille::parse(exp_scale) {
            Ok(scale) if scale.is_retail() => {
                summary.push_str("exp-scale: 1x (retail)\n");
            }
            Ok(scale) => match apply::scale_monster_exp(&mut patcher, scale) {
                Ok(rep) => summary.push_str(&format!(
                    "exp-scale: {scale} ({} monsters changed)\n",
                    rep.monsters_changed
                )),
                Err(e) => summary.push_str(&format!("exp-scale: {e}\n")),
            },
            Err(e) => summary.push_str(&format!("exp-scale: skipped - {e}\n")),
        }
    }

    // Seru catch-rate override: one flat percent into every capturable
    // record's catch-chance byte (empty = retail per-monster rates).
    let seru_catch_rate = seru_catch_rate.trim();
    if seru_catch_rate.is_empty() {
        summary.push_str("seru-catch-rate: retail\n");
    } else {
        match legaia_patcher::rewards::parse_catch_rate(seru_catch_rate) {
            Ok(pct) => match apply::set_seru_catch_rate(&mut patcher, pct) {
                Ok(rep) => summary.push_str(&format!(
                    "seru-catch-rate: {pct}% ({} monsters changed)\n",
                    rep.monsters_changed
                )),
                Err(e) => summary.push_str(&format!("seru-catch-rate: {e}\n")),
            },
            Err(e) => summary.push_str(&format!("seru-catch-rate: skipped - {e}\n")),
        }
    }

    // Delilas party swap (empty = off): play as the mapped siblings, the
    // ravine duels field Vahn / Noa / Gala models. Runs after
    // --delilas-challenge (same ordering as the CLI - the challenge cuts
    // its slim dome clones from the pre-swap blocks). The single heaviest
    // stage in the whole run (player files + monster blocks + field forms
    // + XA banks), so it gets its own progress label.
    prog.stage("Delilas party swap").await;
    let delilas_party = delilas_party.trim();
    if delilas_party.is_empty() {
        summary.push_str("delilas-party: off\n");
    } else {
        // Arts-voice mode for the swapped shout banks; an empty or
        // unknown value falls back to the default (original).
        let arts_voice = delilas_arts_voice
            .trim()
            .parse::<legaia_patcher::delilas_voice_fx::ArtsVoiceMode>()
            .unwrap_or_default();
        // Move mode for the swapped kit; an empty or unknown value falls
        // back to the retail-preserving default (hybrid).
        let move_mode = delilas_moves
            .trim()
            .parse::<legaia_patcher::delilas_party::DelilasMoveMode>()
            .unwrap_or_default();
        match legaia_patcher::delilas_party::PartyMapping::parse(delilas_party) {
            Ok(mapping) => {
                let cast_route = if shiny_seru
                    || show_super_arts
                    || !arts_ap_grants.trim().is_empty()
                    || !arts_ap_costs.trim().is_empty()
                {
                    legaia_patcher::delilas_party::CastRoutePolicy::ArenaTaken
                } else {
                    legaia_patcher::delilas_party::CastRoutePolicy::Install
                };
                match legaia_patcher::delilas_party::apply_delilas_party(
                    &mut patcher,
                    &mapping,
                    arts_voice,
                    move_mode,
                    cast_route,
                ) {
                    Ok(rep) if rep.changed => {
                        summary.push_str(&format!(
                            "delilas-party: playing as {} / {} / {} (duels field the heroes); \
                             moves: {move_mode}; arts voices: {arts_voice}\n",
                            mapping.vahn.display_name(),
                            mapping.noa.display_name(),
                            mapping.gala.display_name()
                        ));
                        for note in rep.notes.iter().filter(|n| n.contains("cast route")) {
                            summary.push_str(&format!("  {note}\n"));
                        }
                    }
                    Ok(_) => summary.push_str("delilas-party: already applied\n"),
                    // A mid-apply error leaves the image partially
                    // swapped (the swap touches many entries before the
                    // failing one) - shipping that as a "skipped" note
                    // would hand the user a broken hybrid ROM. Fail the
                    // whole patch instead, like the CLI does.
                    Err(e) => {
                        return Err(JsValue::from_str(&format!(
                            "delilas-party failed mid-apply ({e:#}); no ROM was produced - \
                             the image would have been a partial hybrid"
                        )));
                    }
                }
            }
            Err(e) => summary.push_str(&format!("delilas-party: skipped - {e}\n")),
        }
    }

    // Enemy attack-count multiplier: divides each attack entry's AGL-cost
    // byte so the per-round AGL budget affords more (or fewer) strikes
    // (empty or 1 = retail). Seedless, same shape as the scales above.
    let enemy_attack_count = enemy_attack_count.trim();
    if enemy_attack_count.is_empty() {
        summary.push_str("enemy-attack-count: 1x (retail)\n");
    } else {
        match legaia_patcher::monster_stats::ScalePermille::parse(enemy_attack_count) {
            Ok(scale) if scale.is_retail() => {
                summary.push_str("enemy-attack-count: 1x (retail)\n");
            }
            Ok(scale) => match apply::scale_enemy_attack_count(&mut patcher, scale) {
                Ok(rep) => summary.push_str(&format!(
                    "enemy-attack-count: {scale} ({} monsters changed, {} attack entries)\n",
                    rep.monsters_changed, rep.entries_changed
                )),
                Err(e) => summary.push_str(&format!("enemy-attack-count: {e}\n")),
            },
            Err(e) => summary.push_str(&format!("enemy-attack-count: skipped - {e}\n")),
        }
    }

    prog.stage("move powers").await;
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

    prog.stage("element affinity").await;
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

    prog.stage("spell costs").await;
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

    prog.stage("equipment bonuses").await;
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

    prog.stage("weapon specialty").await;
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

    prog.stage("steal items").await;
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

    prog.stage("arts combos").await;
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

    prog.stage("doors").await;
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

    prog.stage("house doors").await;
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

    prog.stage("starting items").await;
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

    prog.stage("starting level").await;
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
    prog.stage("language pack: name tables").await;
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

    prog.stage("assembling patched image").await;
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

/// Read the disc-resident tables behind the ROM-patcher page's structured
/// value editors, so those controls can show the disc's own current values
/// instead of asking the user to type raw ids: the 16 world-map location-name
/// slots (the SCUS table [`legaia_patcher::location_name`] renames in place)
/// and the fishing point-exchange prize rows (PROT 972, the table
/// [`legaia_patcher::fishing_price`] reprices), each prize's item id resolved
/// to its display name through the SCUS item-name table.
///
/// Returns `{ max_name_len, locations: [name; 16], fishing: [{ page, row,
/// item, name, price, one_time }] }`. Everything is decoded from the image the
/// user supplied, in this call, in this tab - the site ships no game text and
/// nothing is uploaded. The patcher itself can only *reprice* a fishing prize
/// (the 12 rows and their item ids are fixed on the disc) and only *rename* a
/// location slot, which is exactly the shape this listing exposes.
#[wasm_bindgen]
pub fn read_manual_edit_tables(image: Vec<u8>) -> Result<JsValue, JsValue> {
    let scus = legaia_iso::iso9660::read_file_in_image(&image, "SCUS_942.54")
        .ok_or_else(|| err("SCUS_942.54 not found in disc image"))?;
    let locations = legaia_patcher::location_name::list_names(&scus)
        .map_err(|e| err(format!("location-name table: {e}")))?;
    let item_names = legaia_asset::item_names::ItemNameTable::from_scus(&scus);
    drop(scus);
    let patcher = DiscPatcher::open(image).map_err(|e| err(format!("open disc image: {e}")))?;
    let overlay = patcher
        .read_entry(legaia_patcher::fishing_price::OVERLAY_PROT_INDEX)
        .map_err(|e| err(format!("read fishing overlay: {e}")))?;
    let prizes = legaia_patcher::fishing_price::list_prizes(&overlay)
        .map_err(|e| err(format!("fishing prize table: {e}")))?;
    // The world-map label table names 14 places the 16 quick-travel cells have
    // no room for; those are renamed by name, not by cell index.
    let world_map_only: Vec<String> = legaia_patcher::apply::list_world_map_labels(&patcher)
        .into_iter()
        .map(|(_, _, _, name)| name)
        .filter(|name| !locations.iter().any(|(_, cell)| cell == name))
        .collect();
    drop(patcher);

    let num = JsValue::from_f64;
    let out = Object::new();
    Reflect::set(
        &out,
        &"max_name_len".into(),
        &num(legaia_patcher::location_name::MAX_NAME_LEN as f64),
    )?;
    let locs = js_sys::Array::new();
    for (_idx, name) in &locations {
        locs.push(&JsValue::from_str(name));
    }
    Reflect::set(&out, &"locations".into(), &locs)?;
    // The 14 places that have a world-map label + an entry banner but no
    // quick-travel cell ("Hunter's Spring", "Sol Tower", ...). They are keyed
    // by their current name, so the editor sends `Old=New` rather than
    // `index=New`.
    let extra = js_sys::Array::new();
    for name in world_map_only {
        extra.push(&JsValue::from_str(&name));
    }
    Reflect::set(&out, &"world_map_only".into(), &extra)?;
    let fish = js_sys::Array::new();
    for p in &prizes {
        // All-zero rows are structural padding in the 6-row page, not prizes.
        if p.item_id == 0 && p.price == 0 {
            continue;
        }
        let o = Object::new();
        Reflect::set(&o, &"page".into(), &num(p.page as f64))?;
        Reflect::set(&o, &"row".into(), &num(p.row as f64))?;
        Reflect::set(&o, &"item".into(), &num(p.item_id as f64))?;
        let name = u8::try_from(p.item_id)
            .ok()
            .and_then(|id| item_names.as_ref().and_then(|t| t.name(id)))
            .unwrap_or("");
        Reflect::set(&o, &"name".into(), &name.into())?;
        Reflect::set(&o, &"price".into(), &num(p.price as f64))?;
        Reflect::set(&o, &"one_time".into(), &JsValue::from_bool(p.one_time))?;
        fish.push(&o.into());
    }
    Reflect::set(&out, &"fishing".into(), &fish)?;
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

// --- Texture replacement --------------------------------------------------
//
// The same client-side model as the randomizer: the user's disc bytes are
// scanned in WASM memory, the edited PNG is validated + encoded here, and the
// patched image is downloaded locally. Nothing is uploaded.
//
// Every family-specific rule lives in [`crate::texture_registry`] - which
// families exist, how each enumerates, decodes and writes. The bindings below
// are a translation layer to JS values and nothing else, so adding a texture
// family does not touch this file.

use legaia_patcher::texture::replace_texture;
use legaia_patcher::{battle_texture, monster_texture, save_icon};
use legaia_tim::encode::{EncodeOptions, decode_png_rgba};

use crate::texture_pack::{self, PackEntry, PackMeta};
use crate::texture_registry::{self as reg, ReplaceOp, Rgba, ScanCtx, TexCoord, TexRow};

/// Nearest-neighbour downscale of an RGBA8 image to fit in `max` on the long
/// side (thumbnails for the texture browser).
fn downscale_rgba(rgba: &[u8], w: usize, h: usize, max: usize) -> (usize, usize, Vec<u8>) {
    if w <= max && h <= max {
        return (w, h, rgba.to_vec());
    }
    let step = w.max(h).div_ceil(max).max(1);
    let tw = (w / step).max(1);
    let th = (h / step).max(1);
    let mut out = Vec::with_capacity(tw * th * 4);
    for y in 0..th {
        for x in 0..tw {
            let i = (y * step * w + x * step) * 4;
            out.extend_from_slice(&rgba[i..i + 4]);
        }
    }
    (tw, th, out)
}

/// `{ w, h, rgba: Uint8Array }` for a decoded image.
fn rgba_js(w: usize, h: usize, rgba: &[u8]) -> Result<JsValue, JsValue> {
    let o = Object::new();
    Reflect::set(&o, &"w".into(), &JsValue::from_f64(w as f64))?;
    Reflect::set(&o, &"h".into(), &JsValue::from_f64(h as f64))?;
    let arr = Uint8Array::new_with_length(rgba.len() as u32);
    arr.copy_from(rgba);
    Reflect::set(&o, &"rgba".into(), &arr)?;
    Ok(o.into())
}

/// A registry coordinate from the page's `(tier, entry, section, offset)`
/// quad. The tier string is resolved against the registry so an unknown
/// family is refused here rather than silently taking some other family's
/// writer.
fn coord_of(tier: &str, entry: i32, section: i32, offset: f64) -> Result<TexCoord, JsValue> {
    let id = reg::tier(tier)
        .ok_or_else(|| err(format!("unknown texture family {tier:?}")))?
        .id;
    Ok(TexCoord {
        tier: id,
        entry: entry as i64,
        section: section as i64,
        offset: offset as u64,
    })
}

/// What the scan needs out of a disc image, read before the image is
/// dropped: the `PROT.DAT` payload, the CDNAME block map, and the executable.
///
/// Peak-memory discipline: the scan only needs these, and a full image plus
/// payload plus scan state would not fit comfortably in 32-bit WASM memory.
///
/// The executable is here because a texture family can need data that is not
/// in `PROT.DAT` at all - the battle-equipment tier names its rows after the
/// equipment they belong to, and that name table lives in `SCUS_942.54`. It
/// is the disc that holds both, so the disc is where both get read.
struct DiscScanInput {
    prot: Vec<u8>,
    blocks: Option<legaia_prot::cdname::IndexMap>,
    scus: Option<Vec<u8>>,
}

fn disc_scan_input(image: Vec<u8>) -> Result<DiscScanInput, JsValue> {
    let prot = legaia_iso::iso9660::read_file_in_image(&image, "PROT.DAT")
        .ok_or_else(|| err("PROT.DAT not found in disc image"))?;
    let blocks = crate::disc::extract_cdname_txt(&image)
        .and_then(|t| legaia_prot::cdname::parse_str(&t).ok());
    let scus = crate::disc::extract_scus(&image);
    drop(image);
    Ok(DiscScanInput { prot, blocks, scus })
}

/// Every TOC entry's `(byte_offset, size_bytes, index)`.
fn entry_spans(prot: &[u8]) -> Result<Vec<(u64, u64, u32)>, JsValue> {
    let archive = legaia_prot::archive::Archive::from_bytes(prot.to_vec())
        .map_err(|e| err(format!("parse PROT.DAT TOC: {e}")))?;
    Ok(archive
        .entries
        .iter()
        .map(|e| (e.byte_offset, e.size_bytes, e.index))
        .collect())
}

/// The CDNAME block a PROT entry belongs to.
///
/// `Archive` entry indices are extraction-frame indices, and CDNAME `#define`
/// numbers are raw in-RAM TOC indices, so the lookup must go through the +2
/// shift - reading the define numbers as extraction indices names the wrong
/// block near every block boundary.
fn block_name(blocks: Option<&legaia_prot::cdname::IndexMap>, entry: i64) -> String {
    if entry < 0 {
        return "unindexed gap (boot UI)".to_string();
    }
    blocks
        .and_then(|m| legaia_prot::cdname::block_for_extraction_index(m, entry as u32))
        .unwrap_or("")
        .to_string()
}

/// One scan row as a JS object.
fn row_js(
    row: &TexRow,
    replaceable: bool,
    block: &str,
    thumb: JsValue,
) -> Result<JsValue, JsValue> {
    let o = Object::new();
    let num = JsValue::from_f64;
    Reflect::set(&o, &"tier".into(), &row.coord.tier.into())?;
    Reflect::set(&o, &"entry".into(), &num(row.coord.entry as f64))?;
    Reflect::set(&o, &"section".into(), &num(row.coord.section as f64))?;
    Reflect::set(&o, &"offset".into(), &num(row.coord.offset as f64))?;
    Reflect::set(&o, &"width".into(), &num(row.width as f64))?;
    Reflect::set(&o, &"height".into(), &num(row.height as f64))?;
    Reflect::set(&o, &"bpp".into(), &num(row.bpp as f64))?;
    Reflect::set(&o, &"cluts".into(), &num(row.cluts as f64))?;
    Reflect::set(&o, &"bytes".into(), &num(row.bytes as f64))?;
    Reflect::set(
        &o,
        &"label".into(),
        &row.label.as_deref().unwrap_or("").into(),
    )?;
    // A 64-bit fingerprint does not survive a JS number, and a pack compares
    // it for equality - so it crosses as hex text, never as a float.
    Reflect::set(
        &o,
        &"fnv1a".into(),
        &format!("{:016x}", row.fnv1a).as_str().into(),
    )?;
    Reflect::set(&o, &"replaceable".into(), &JsValue::from_bool(replaceable))?;
    Reflect::set(&o, &"block".into(), &block.into())?;
    match row.vram {
        Some((x, y, w, h)) => {
            let v = Object::new();
            Reflect::set(&v, &"x".into(), &num(x as f64))?;
            Reflect::set(&v, &"y".into(), &num(y as f64))?;
            Reflect::set(&v, &"w".into(), &num(w as f64))?;
            Reflect::set(&v, &"h".into(), &num(h as f64))?;
            Reflect::set(&o, &"vram".into(), &v)?;
        }
        None => {
            Reflect::set(&o, &"vram".into(), &JsValue::NULL)?;
        }
    };
    match row.clut_vram {
        Some((x, y)) => {
            let v = Object::new();
            Reflect::set(&v, &"x".into(), &num(x as f64))?;
            Reflect::set(&v, &"y".into(), &num(y as f64))?;
            Reflect::set(&o, &"clut_vram".into(), &v)?;
        }
        None => {
            Reflect::set(&o, &"clut_vram".into(), &JsValue::NULL)?;
        }
    };
    Reflect::set(&o, &"thumb".into(), &thumb)?;
    Ok(o.into())
}

/// Scan a user-supplied disc image for every texture the registry can reach,
/// with thumbnails.
///
/// Returns `{ tiers: [{ id, title, about, replaceable, count }], textures:
/// [{ tier, entry, section, offset, width, height, bpp, cluts, bytes, label,
/// fnv1a, replaceable, block, vram, clut_vram, thumb }] }` plus `raw_count` /
/// `lzs_count` / `save_icon_count` for the page's headline note.
///
/// `entry` is `-1` for the unindexed gap before entry 0; `section` is `-1`
/// where the family does not use it. `thumb_max` caps the thumbnail's long
/// side (0 = no thumbnails). `fnv1a` is 16 hex digits.
#[wasm_bindgen]
pub fn scan_textures(image: Vec<u8>, thumb_max: u32) -> Result<JsValue, JsValue> {
    let DiscScanInput { prot, blocks, scus } = disc_scan_input(image)?;
    let spans = entry_spans(&prot)?;
    let ctx = ScanCtx::with_scus(&prot, &spans, scus.as_deref());

    let textures = js_sys::Array::new();
    let mut counts: Vec<(&'static str, usize)> =
        reg::tiers().iter().map(|t| (t.id, 0usize)).collect();

    // The sink thumbnails and drops each decode as it arrives - full-size
    // pixels for every texture on the disc would not fit in WASM memory.
    let mut sink_err: Option<JsValue> = None;
    {
        let mut sink = |row: TexRow, rgba: Option<Rgba>| -> Result<(), String> {
            let thumb = match (thumb_max, rgba) {
                (0, _) | (_, None) => JsValue::NULL,
                (max, Some(img)) => {
                    let (tw, th, small) = downscale_rgba(&img.data, img.w, img.h, max as usize);
                    rgba_js(tw, th, &small).unwrap_or(JsValue::NULL)
                }
            };
            let replaceable = reg::tier(row.coord.tier).is_some_and(|t| t.replaceable);
            let block = block_name(blocks.as_ref(), row.coord.entry);
            match row_js(&row, replaceable, &block, thumb) {
                Ok(js) => {
                    textures.push(&js);
                    if let Some(c) = counts.iter_mut().find(|(id, _)| *id == row.coord.tier) {
                        c.1 += 1;
                    }
                    Ok(())
                }
                Err(e) => {
                    sink_err = Some(e);
                    Err("could not build a row object".to_string())
                }
            }
        };
        if let Err(msg) = reg::scan_all(&ctx, thumb_max > 0, &mut sink) {
            return Err(sink_err.unwrap_or_else(|| err(msg)));
        }
    }

    let count_of = |id: &str| counts.iter().find(|(i, _)| *i == id).map_or(0, |c| c.1);
    let tiers = js_sys::Array::new();
    for t in reg::tiers() {
        let o = Object::new();
        Reflect::set(&o, &"id".into(), &t.id.into())?;
        Reflect::set(&o, &"title".into(), &t.title.into())?;
        Reflect::set(&o, &"about".into(), &t.about.into())?;
        Reflect::set(
            &o,
            &"replaceable".into(),
            &JsValue::from_bool(t.replaceable),
        )?;
        Reflect::set(
            &o,
            &"count".into(),
            &JsValue::from_f64(count_of(t.id) as f64),
        )?;
        tiers.push(&o);
    }

    let out = Object::new();
    let num = JsValue::from_f64;
    // Kept for the page's headline note. These are emitted-row counts (what
    // the grid actually offers), not catalog lengths.
    Reflect::set(
        &out,
        &"raw_count".into(),
        &num(count_of(reg::TIER_RAW) as f64),
    )?;
    Reflect::set(
        &out,
        &"lzs_count".into(),
        &num(count_of(reg::TIER_LZS) as f64),
    )?;
    Reflect::set(
        &out,
        &"save_icon_count".into(),
        &num(count_of(reg::TIER_SAVE_ICON) as f64),
    )?;
    Reflect::set(&out, &"tiers".into(), &tiers)?;
    Reflect::set(&out, &"textures".into(), &textures)?;
    Ok(out.into())
}

/// Decode one texture full-size straight from the disc, without going near
/// the writer. This is how a read-only family previews and exports.
/// Returns `{ w, h, rgba }`.
#[wasm_bindgen]
pub fn decode_texture(
    image: Vec<u8>,
    tier: &str,
    entry: i32,
    section: i32,
    offset: f64,
) -> Result<JsValue, JsValue> {
    let coord = coord_of(tier, entry, section, offset)?;
    let input = disc_scan_input(image)?;
    let spans = entry_spans(&input.prot)?;
    let ctx = ScanCtx::new(&input.prot, &spans);
    let img = reg::read_row(&ctx, &coord).map_err(err)?;
    rgba_js(img.w, img.h, &img.data)
}

/// Validate one texture replacement against the user's disc and build the
/// side-by-side preview. Never writes.
///
/// Returns `{ ok, error, original: { w, h, rgba }, preview: { w, h, rgba } |
/// null, width, height, bpp, cluts, new_palette_entries, quantized_pixels,
/// fit: { capacity, recompressed } | null }`. `preview` is the replacement as
/// it will *display* on disc (15-bit rounding + any quantization applied), so
/// what the user sees is what the game gets.
///
/// One entry point for every family: the registry decides which writer a
/// coordinate resolves to.
#[wasm_bindgen]
pub fn preview_texture_replace(
    image: Vec<u8>,
    tier: &str,
    entry: i32,
    section: i32,
    offset: f64,
    png: &[u8],
    quantize: bool,
) -> Result<JsValue, JsValue> {
    let coord = coord_of(tier, entry, section, offset)?;
    let mut patcher = DiscPatcher::open(image).map_err(|e| err(format!("parse disc: {e}")))?;
    let op = reg::replace_op(&coord).map_err(err)?;

    let out = Object::new();
    let num = JsValue::from_f64;
    let fail = |out: &Object, msg: String| -> Result<JsValue, JsValue> {
        Reflect::set(out, &"ok".into(), &JsValue::from_bool(false))?;
        Reflect::set(out, &"error".into(), &msg.as_str().into())?;
        Ok(out.clone().into())
    };

    match op {
        ReplaceOp::SaveIconSlot(slot) => {
            use legaia_asset::save_icon as si;
            let size = si::TILE_SIZE;
            Reflect::set(&out, &"width".into(), &num(size as f64))?;
            Reflect::set(&out, &"height".into(), &num(size as f64))?;
            Reflect::set(&out, &"bpp".into(), &num(4.0))?;
            Reflect::set(&out, &"cluts".into(), &num(1.0))?;
            let sheet = save_icon::read_sheet(&patcher)
                .map_err(|e| err(format!("read save-icon sheet: {e:#}")))?;
            let original = match save_icon::export_slot(&sheet, slot) {
                Ok(rgba) => rgba,
                Err(e) => return fail(&out, format!("{e:#}")),
            };
            Reflect::set(&out, &"original".into(), &rgba_js(size, size, &original)?)?;
            let (w, h, rgba) = match decode_png_rgba(png) {
                Ok(v) => v,
                Err(e) => return fail(&out, format!("read PNG: {e}")),
            };
            if (w, h) != (size, size) {
                return fail(
                    &out,
                    format!("a save-slot portrait must be {size}x{size}, got {w}x{h}"),
                );
            }
            match save_icon::preview_slot(&sheet, slot, &rgba, quantize) {
                Ok(p) => {
                    Reflect::set(&out, &"preview".into(), &rgba_js(size, size, &p.rgba)?)?;
                    Reflect::set(
                        &out,
                        &"new_palette_entries".into(),
                        &num(p.palette_entries_changed as f64),
                    )?;
                    Reflect::set(
                        &out,
                        &"quantized_pixels".into(),
                        &num(p.quantized_pixels as f64),
                    )?;
                    Reflect::set(&out, &"ok".into(), &JsValue::from_bool(true))?;
                    Reflect::set(&out, &"error".into(), &"".into())?;
                }
                Err(e) => return fail(&out, format!("{e:#}")),
            }
        }
        ReplaceOp::Tim(target) => {
            let orig = legaia_patcher::texture::read_texture(&patcher, &target)
                .map_err(|e| err(format!("read texture: {e:#}")))?;
            let (ow, oh) = (orig.tim.pixel_width(), orig.tim.pixel_height());
            let orig_rgba = legaia_tim::decode_rgba8(&orig.tim, 0)
                .map_err(|e| err(format!("decode original: {e}")))?;
            Reflect::set(&out, &"original".into(), &rgba_js(ow, oh, &orig_rgba)?)?;
            Reflect::set(&out, &"width".into(), &num(ow as f64))?;
            Reflect::set(&out, &"height".into(), &num(oh as f64))?;
            Reflect::set(&out, &"cluts".into(), &num(orig.tim.palette_count() as f64))?;

            let (pw, ph, rgba) = match decode_png_rgba(png) {
                Ok(v) => v,
                Err(e) => return fail(&out, format!("read PNG: {e}")),
            };
            let opts = EncodeOptions { quantize };
            // Encode first (for the preview), then dry-run the full
            // replacement so the LZS fit is measured exactly as apply would.
            match legaia_tim::encode::encode_replacement(&orig.tim, &rgba, pw, ph, &opts) {
                Ok(enc) => {
                    Reflect::set(
                        &out,
                        &"new_palette_entries".into(),
                        &num(enc.new_palette_entries as f64),
                    )?;
                    Reflect::set(
                        &out,
                        &"quantized_pixels".into(),
                        &num(enc.quantized_pixels as f64),
                    )?;
                    let ptim = legaia_tim::parse(&enc.bytes)
                        .map_err(|e| err(format!("re-parse encoded TIM: {e}")))?;
                    let prgba = legaia_tim::decode_rgba8(&ptim, 0)
                        .map_err(|e| err(format!("decode encoded TIM: {e}")))?;
                    Reflect::set(&out, &"preview".into(), &rgba_js(pw, ph, &prgba)?)?;
                }
                Err(e) => return fail(&out, e.to_string()),
            }
            match replace_texture(&mut patcher, &target, &rgba, pw, ph, &opts, true) {
                Ok(outcome) => {
                    Reflect::set(&out, &"ok".into(), &JsValue::from_bool(true))?;
                    Reflect::set(&out, &"error".into(), &"".into())?;
                    Reflect::set(&out, &"bpp".into(), &num(outcome.bpp as f64))?;
                    if let Some(fit) = outcome.lzs {
                        let f = Object::new();
                        Reflect::set(&f, &"capacity".into(), &num(fit.capacity as f64))?;
                        Reflect::set(&f, &"recompressed".into(), &num(fit.recompressed as f64))?;
                        Reflect::set(&out, &"fit".into(), &f)?;
                    }
                }
                Err(e) => return fail(&out, format!("{e:#}")),
            }
        }
        ReplaceOp::BattleEquip(target) => {
            let orig = battle_texture::export_block(&patcher, &target, reg::BATTLE_PREVIEW_PALETTE)
                .map_err(|e| err(format!("read battle texture: {e:#}")))?;
            Reflect::set(
                &out,
                &"original".into(),
                &rgba_js(orig.width, orig.height, &orig.rgba)?,
            )?;
            Reflect::set(&out, &"width".into(), &num(orig.width as f64))?;
            Reflect::set(&out, &"height".into(), &num(orig.height as f64))?;
            Reflect::set(&out, &"bpp".into(), &num(4.0))?;
            Reflect::set(&out, &"cluts".into(), &num(orig.palette_count as f64))?;

            let (pw, ph, rgba) = match decode_png_rgba(png) {
                Ok(v) => v,
                Err(e) => return fail(&out, format!("read PNG: {e}")),
            };
            // One call: the same encode and the same recompression the write
            // performs, stopped before the patch. A separate "preview" encode
            // could disagree with the writer about a folded colour.
            match battle_texture::preview_block(
                &patcher,
                &target,
                &rgba,
                pw,
                ph,
                reg::BATTLE_PREVIEW_PALETTE,
                quantize,
            ) {
                Ok(p) => {
                    Reflect::set(&out, &"preview".into(), &rgba_js(pw, ph, &p.rgba)?)?;
                    Reflect::set(
                        &out,
                        &"new_palette_entries".into(),
                        &num(p.palette_entries_changed as f64),
                    )?;
                    Reflect::set(
                        &out,
                        &"quantized_pixels".into(),
                        &num(p.quantized_pixels as f64),
                    )?;
                    let f = Object::new();
                    Reflect::set(&f, &"capacity".into(), &num(p.fit.capacity as f64))?;
                    Reflect::set(&f, &"recompressed".into(), &num(p.fit.recompressed as f64))?;
                    Reflect::set(&out, &"fit".into(), &f)?;
                    Reflect::set(&out, &"ok".into(), &JsValue::from_bool(true))?;
                    Reflect::set(&out, &"error".into(), &"".into())?;
                }
                Err(e) => return fail(&out, format!("{e:#}")),
            }
        }
        ReplaceOp::MonsterPage(target) => {
            let orig = monster_texture::export_page(&patcher, &target)
                .map_err(|e| err(format!("read monster texture: {e:#}")))?;
            Reflect::set(
                &out,
                &"original".into(),
                &rgba_js(orig.width, orig.height, &orig.rgba)?,
            )?;
            Reflect::set(&out, &"width".into(), &num(orig.width as f64))?;
            Reflect::set(&out, &"height".into(), &num(orig.height as f64))?;
            Reflect::set(&out, &"bpp".into(), &num(4.0))?;
            Reflect::set(&out, &"cluts".into(), &num(orig.palettes_populated as f64))?;

            let (pw, ph, rgba) = match decode_png_rgba(png) {
                Ok(v) => v,
                Err(e) => return fail(&out, format!("read PNG: {e}")),
            };
            match monster_texture::preview_page(&patcher, &target, &rgba, pw, ph, quantize) {
                Ok(p) => {
                    Reflect::set(&out, &"preview".into(), &rgba_js(pw, ph, &p.rgba)?)?;
                    // This family never rewrites a palette (a monster's CLUTs
                    // upload verbatim, so their blend bits are live state), so
                    // the page's own counter is texels re-indexed instead.
                    Reflect::set(&out, &"new_palette_entries".into(), &num(0.0))?;
                    Reflect::set(
                        &out,
                        &"quantized_pixels".into(),
                        &num(p.quantized_texels as f64),
                    )?;
                    Reflect::set(
                        &out,
                        &"texels_changed".into(),
                        &num(p.texels_changed as f64),
                    )?;
                    Reflect::set(
                        &out,
                        &"dead_texels_ignored".into(),
                        &num(p.dead_texels_ignored as f64),
                    )?;
                    let f = Object::new();
                    Reflect::set(&f, &"capacity".into(), &num(p.fit.capacity as f64))?;
                    Reflect::set(&f, &"recompressed".into(), &num(p.fit.recompressed as f64))?;
                    Reflect::set(&out, &"fit".into(), &f)?;
                    Reflect::set(&out, &"ok".into(), &JsValue::from_bool(true))?;
                    Reflect::set(&out, &"error".into(), &"".into())?;
                }
                Err(e) => return fail(&out, format!("{e:#}")),
            }
        }
    }
    Ok(out.into())
}

/// One queued replacement, read off a JS spec object.
struct Spec {
    coord: TexCoord,
    png: Vec<u8>,
    quantize: bool,
}

fn read_spec(spec: &JsValue) -> Result<Spec, JsValue> {
    let get_num = |k: &str| -> Result<f64, JsValue> {
        Reflect::get(spec, &k.into())?
            .as_f64()
            .ok_or_else(|| err(format!("texture spec missing numeric {k}")))
    };
    let tier = Reflect::get(spec, &"tier".into())?
        .as_string()
        .ok_or_else(|| err("texture spec missing tier"))?;
    Ok(Spec {
        coord: coord_of(
            &tier,
            get_num("entry")? as i32,
            get_num("section")? as i32,
            get_num("offset")?,
        )?,
        png: Uint8Array::from(Reflect::get(spec, &"png".into())?).to_vec(),
        quantize: Reflect::get(spec, &"quantize".into())?
            .as_bool()
            .unwrap_or(false),
    })
}

/// Apply a queue of validated texture replacements to a disc image. `specs`
/// is an array of `{ tier, entry, section, offset, png: Uint8Array, quantize
/// }` (same coordinate conventions as [`preview_texture_replace`]). Applied
/// in order; a failing spec aborts with its error (nothing partial is
/// returned). Returns `{ data, summary }` - the same shape the page consumes
/// from [`patch_rom`], so texture patches chain after a randomizer run.
///
/// Async with the same optional trailing `progress` callback as [`patch_rom`]:
/// one stage to parse the disc, one per replacement spec, one to assemble the
/// output image.
#[wasm_bindgen]
pub async fn apply_texture_replacements(
    image: Vec<u8>,
    specs: JsValue,
    progress: Option<js_sys::Function>,
) -> Result<JsValue, JsValue> {
    let list = js_sys::Array::from(&specs);
    let mut prog = Progress::new(progress, list.length() + 2);
    prog.stage("parsing disc image").await;
    let mut patcher = DiscPatcher::open(image).map_err(|e| err(format!("parse disc: {e}")))?;
    let mut summary = String::new();
    for (i, raw) in list.iter().enumerate() {
        prog.stage(&format!("texture {} of {}", i + 1, list.length()))
            .await;
        let spec = read_spec(&raw)?;
        let at = format!(
            "{} entry {} section {} +0x{:X}",
            spec.coord.tier, spec.coord.entry, spec.coord.section, spec.coord.offset
        );
        let op =
            reg::replace_op(&spec.coord).map_err(|e| err(format!("texture {i} ({at}): {e}")))?;
        let (w, h, rgba) =
            decode_png_rgba(&spec.png).map_err(|e| err(format!("texture {i} ({at}): {e}")))?;
        match op {
            ReplaceOp::SaveIconSlot(slot) => {
                let size = legaia_asset::save_icon::TILE_SIZE;
                if (w, h) != (size, size) {
                    return Err(err(format!(
                        "save-icon {i} (slot {slot}): portrait must be {size}x{size}, got {w}x{h}"
                    )));
                }
                let outcome = save_icon::replace_slot(&mut patcher, slot, &rgba, spec.quantize)
                    .map_err(|e| err(format!("save-icon {i} (slot {slot}): {e:#}")))?;
                summary.push_str(&format!(
                    "save icon: slot {} (save number {}) replaced{}\n",
                    outcome.slot,
                    outcome.slot + 1,
                    if outcome.quantized_pixels > 0 {
                        format!(", {} pixel(s) quantized", outcome.quantized_pixels)
                    } else {
                        String::new()
                    },
                ));
            }
            ReplaceOp::Tim(target) => {
                let outcome = replace_texture(
                    &mut patcher,
                    &target,
                    &rgba,
                    w,
                    h,
                    &EncodeOptions {
                        quantize: spec.quantize,
                    },
                    false,
                )
                .map_err(|e| err(format!("texture {i} ({target}): {e:#}")))?;
                summary.push_str(&format!(
                    "texture: {target} replaced ({}x{} {} bpp{}{}{})\n",
                    outcome.width,
                    outcome.height,
                    outcome.bpp,
                    if outcome.new_palette_entries > 0 {
                        format!(", {} new palette color(s)", outcome.new_palette_entries)
                    } else {
                        String::new()
                    },
                    if outcome.quantized_pixels > 0 {
                        format!(", {} pixel(s) quantized", outcome.quantized_pixels)
                    } else {
                        String::new()
                    },
                    match outcome.lzs {
                        Some(f) => format!(
                            ", recompressed {}B into the {}B stream",
                            f.recompressed, f.capacity
                        ),
                        None => String::new(),
                    },
                ));
            }
            ReplaceOp::BattleEquip(target) => {
                let outcome = battle_texture::replace_block(
                    &mut patcher,
                    &target,
                    &rgba,
                    w,
                    h,
                    reg::BATTLE_PREVIEW_PALETTE,
                    spec.quantize,
                    false,
                )
                .map_err(|e| err(format!("battle texture {i} ({target}): {e:#}")))?;
                summary.push_str(&format!(
                    "battle art: {target} replaced ({}x{} 4 bpp, {}{}{})\n",
                    outcome.width,
                    outcome.height,
                    outcome.palette,
                    if outcome.quantized_pixels > 0 {
                        format!(", {} pixel(s) quantized", outcome.quantized_pixels)
                    } else {
                        String::new()
                    },
                    if outcome.unchanged {
                        " - identical to retail, nothing written".to_string()
                    } else {
                        format!(
                            ", recompressed {}B into the {}B slot",
                            outcome.fit.recompressed, outcome.fit.capacity
                        )
                    },
                ));
            }
            ReplaceOp::MonsterPage(target) => {
                let outcome = monster_texture::replace_page(
                    &mut patcher,
                    &target,
                    &rgba,
                    w,
                    h,
                    spec.quantize,
                    false,
                )
                .map_err(|e| err(format!("monster texture {i} ({target}): {e:#}")))?;
                summary.push_str(&format!(
                    "monster skin: {} #{} repainted ({}x{} 4 bpp, {} texel(s) changed{}{}{})\n",
                    outcome.name,
                    outcome.id,
                    outcome.width,
                    outcome.height,
                    outcome.texels_changed,
                    if outcome.quantized_texels > 0 {
                        format!(", {} folded onto a nearer colour", outcome.quantized_texels)
                    } else {
                        String::new()
                    },
                    if outcome.dead_texels_ignored > 0 {
                        format!(
                            ", {} painted where nothing samples the page (ignored)",
                            outcome.dead_texels_ignored
                        )
                    } else {
                        String::new()
                    },
                    if outcome.unchanged {
                        " - identical to retail, nothing written".to_string()
                    } else {
                        format!(
                            ", recompressed {}B into the {}B slot",
                            outcome.fit.recompressed, outcome.fit.capacity
                        )
                    },
                ));
            }
        }
    }
    if list.length() == 0 {
        summary.push_str("textures: untouched\n");
    }

    prog.stage("assembling patched image").await;
    let patched = patcher.into_image();
    let data = Uint8Array::new_with_length(patched.len() as u32);
    data.copy_from(&patched);
    let out = Object::new();
    Reflect::set(&out, &"data".into(), &data)?;
    Reflect::set(&out, &"summary".into(), &summary.into())?;
    Ok(out.into())
}

// --- Change packs -----------------------------------------------------------

/// Serialize a queue of replacements into a shareable texture change pack.
///
/// `specs` adds `fnv1a` (16 hex digits), `width`, `height`, `bpp` and `label`
/// to the shape [`apply_texture_replacements`] takes - the fingerprint of the
/// *retail* texture the edit was authored against, which is what lets an
/// import verify it landed on the right disc.
///
/// A pack carries the user's own images plus those fingerprints. It never
/// carries retail pixels, so it is shareable; that is enforced by the pack
/// module, not by this binding.
#[wasm_bindgen]
pub fn export_texture_pack(
    specs: JsValue,
    name: &str,
    author: &str,
    note: &str,
) -> Result<String, JsValue> {
    let list = js_sys::Array::from(&specs);
    let mut entries = Vec::with_capacity(list.length() as usize);
    for raw in list.iter() {
        let spec = read_spec(&raw)?;
        let hex = Reflect::get(&raw, &"fnv1a".into())?
            .as_string()
            .ok_or_else(|| err("texture spec missing fnv1a"))?;
        let original_fnv1a = u64::from_str_radix(&hex, 16)
            .map_err(|_| err(format!("fnv1a is not 16 hex digits: {hex:?}")))?;
        let num = |k: &str| -> u32 {
            Reflect::get(&raw, &k.into())
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as u32
        };
        entries.push(PackEntry {
            coord: spec.coord,
            original_fnv1a,
            original_width: num("width"),
            original_height: num("height"),
            original_bpp: num("bpp"),
            label: Reflect::get(&raw, &"label".into())?
                .as_string()
                .unwrap_or_default(),
            quantize: spec.quantize,
            png: spec.png,
        });
    }
    let meta = PackMeta {
        name: name.to_string(),
        author: author.to_string(),
        note: note.to_string(),
    };
    Ok(texture_pack::to_json(&meta, &entries))
}

/// Read a texture change pack and grade every entry against the user's own
/// disc.
///
/// Returns `{ name, author, note, version, entries: [{ tier, entry, section,
/// offset, label, quantize, width, height, fnv1a, status, detail, usable, png
/// }] }`. `status` is one of `ok` / `unknown-family` / `not-found` /
/// `hash-mismatch` / `size-mismatch`; `detail` is a sentence to show; `usable`
/// says whether the page may queue it.
///
/// Verification reads the *current* image, so a texture already patched on
/// this disc reports `hash-mismatch` rather than being replaced twice.
/// `accept_hash_mismatch` marks those usable anyway - the deliberate
/// "re-apply on top of my own edit" case.
#[wasm_bindgen]
pub fn import_texture_pack(
    image: Vec<u8>,
    json: &str,
    accept_hash_mismatch: bool,
) -> Result<JsValue, JsValue> {
    let pack = texture_pack::from_json(json).map_err(err)?;
    let patcher = DiscPatcher::open(image).map_err(|e| err(format!("parse disc: {e}")))?;

    let entries = js_sys::Array::new();
    for e in &pack.entries {
        let status = texture_pack::verify(&patcher, e);
        let usable = match &status {
            texture_pack::EntryStatus::Ok => true,
            texture_pack::EntryStatus::HashMismatch { .. } => accept_hash_mismatch,
            _ => false,
        };
        let o = Object::new();
        let num = JsValue::from_f64;
        Reflect::set(&o, &"tier".into(), &e.coord.tier.into())?;
        Reflect::set(&o, &"entry".into(), &num(e.coord.entry as f64))?;
        Reflect::set(&o, &"section".into(), &num(e.coord.section as f64))?;
        Reflect::set(&o, &"offset".into(), &num(e.coord.offset as f64))?;
        Reflect::set(&o, &"width".into(), &num(e.original_width as f64))?;
        Reflect::set(&o, &"height".into(), &num(e.original_height as f64))?;
        Reflect::set(&o, &"bpp".into(), &num(e.original_bpp as f64))?;
        Reflect::set(
            &o,
            &"fnv1a".into(),
            &format!("{:016x}", e.original_fnv1a).as_str().into(),
        )?;
        Reflect::set(&o, &"label".into(), &e.label.as_str().into())?;
        Reflect::set(&o, &"quantize".into(), &JsValue::from_bool(e.quantize))?;
        Reflect::set(&o, &"status".into(), &status.tag().into())?;
        Reflect::set(&o, &"detail".into(), &status.detail().as_str().into())?;
        Reflect::set(&o, &"usable".into(), &JsValue::from_bool(usable))?;
        let png = Uint8Array::new_with_length(e.png.len() as u32);
        png.copy_from(&e.png);
        Reflect::set(&o, &"png".into(), &png)?;
        entries.push(&o);
    }

    let out = Object::new();
    Reflect::set(&out, &"name".into(), &pack.meta.name.as_str().into())?;
    Reflect::set(&out, &"author".into(), &pack.meta.author.as_str().into())?;
    Reflect::set(&out, &"note".into(), &pack.meta.note.as_str().into())?;
    Reflect::set(
        &out,
        &"version".into(),
        &JsValue::from_f64(pack.version as f64),
    )?;
    Reflect::set(&out, &"entries".into(), &entries)?;
    Ok(out.into())
}
