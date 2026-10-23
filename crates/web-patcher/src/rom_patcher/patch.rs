//! The randomizer entry points: seed resolution and [`patch_rom`].
//!
//! A patch run is [`STAGES`]: an ordered table of progress stages, each a
//! list of steps over one [`PatchRun`]. The steps live in the submodule of
//! their feature family (the same families as the CLI's `randomize`
//! stages); [`run_stages`] drives the table and touches no JS value, so the
//! whole run is testable natively, and [`patch_rom`] is the `wasm_bindgen`
//! shell that gathers the page's arguments and hands back the JS result.
//!
//! The CLI keeps its own stage order in `legaia-patcher`'s `randomize`
//! module. The two orders differ, and each is what its host's seeds mean, so
//! they are not one list.

use super::*;

/// Resolve a user seed string to the numeric seed, as a decimal string (so the
/// page can display / persist it without JS `BigInt` precision loss).
#[wasm_bindgen]
pub fn resolve_seed(seed: &str) -> String {
    seed_from_str(seed).to_string()
}

mod arts;
mod battle_hooks;
mod delilas;
mod equipment;
mod language;
mod monsters;
mod starting;
mod world;

#[cfg(test)]
mod byte_pin;

/// Every option of one patch run, as the page passes it to [`patch_rom`]
/// (see there for what each one means and which values it takes).
#[derive(Clone, Copy, Default)]
pub(super) struct PatchOptions<'a> {
    pub(super) seed: &'a str,
    pub(super) lang_pack: &'a str,
    pub(super) lang_relayout: bool,
    pub(super) drops: &'a str,
    pub(super) encounters: &'a str,
    pub(super) encounter_scope: &'a str,
    pub(super) chests: &'a str,
    pub(super) shops: &'a str,
    pub(super) casino: &'a str,
    pub(super) steals: &'a str,
    pub(super) arts: &'a str,
    pub(super) doors: &'a str,
    pub(super) door_coupling: &'a str,
    pub(super) house_doors: &'a str,
    pub(super) starting_items: usize,
    pub(super) door_of_wind: u8,
    pub(super) incense: u8,
    pub(super) speed_chain: u8,
    pub(super) chicken_heart: u8,
    pub(super) good_luck_bell: u8,
    pub(super) all_warps: bool,
    pub(super) unused_enemies: bool,
    pub(super) unused_items: bool,
    pub(super) equipment_drops: bool,
    pub(super) monster_stats: &'a str,
    pub(super) move_power: &'a str,
    pub(super) element_affinity: &'a str,
    pub(super) spell_cost: &'a str,
    pub(super) equip_bonus: &'a str,
    pub(super) weapon_specialty: bool,
    pub(super) starting_level: u8,
    pub(super) solo_strong_encounters: bool,
    pub(super) flee_exp: bool,
    pub(super) seru_trade: bool,
    pub(super) enemy_ally: bool,
    pub(super) shiny_seru: bool,
    pub(super) jewel_fix: bool,
    pub(super) approach_softlock_fix: bool,
    pub(super) delilas_challenge: bool,
    pub(super) custom_items: bool,
    pub(super) fishing_prices: &'a str,
    pub(super) location_renames: &'a str,
    pub(super) earth_egg_price: &'a str,
    pub(super) arts_powers: &'a str,
    pub(super) arts_ap_grants: &'a str,
    pub(super) arts_ap_costs: &'a str,
    pub(super) spirit_ap: &'a str,
    pub(super) damage_ap: &'a str,
    pub(super) oscillating_ap: &'a str,
    pub(super) enemy_stat_scale: &'a str,
    pub(super) exp_scale: &'a str,
    pub(super) seru_catch_rate: &'a str,
    pub(super) delilas_party: &'a str,
    pub(super) delilas_arts_voice: &'a str,
    pub(super) delilas_moves: &'a str,
    pub(super) super_art_powers: &'a str,
    pub(super) show_super_arts: bool,
    pub(super) super_arts_pack: bool,
    pub(super) enemy_attack_count: &'a str,
    pub(super) swing_costs: &'a str,
    pub(super) equip_owners: &'a str,
    pub(super) enemy_hp_bar: bool,
    pub(super) fortress_encounter_fix: bool,
    pub(super) conkram_freeze_fix: bool,
}

impl PatchOptions<'_> {
    /// Whether the arts AP override has any entry.
    fn arts_ap(&self) -> bool {
        !(self.arts_ap_grants.trim().is_empty() && self.arts_ap_costs.trim().is_empty())
    }

    /// Whether oscillating AP costs are on.
    fn oscillating(&self) -> bool {
        !self.oscillating_ap.trim().is_empty()
    }
}

/// Refuse the option combinations that cannot share a disc, before anything
/// is written.
fn check_option_conflicts(o: &PatchOptions<'_>) -> StepResult {
    // The Delilas Challenge and the Delilas party swap both rewrite the AI
    // picker's Delilas arm; together the swap stops part-way through, so the
    // pair is refused before anything is written (same rule as the CLI).
    if o.delilas_challenge && !o.delilas_party.trim().is_empty() {
        return Err(
            "the Delilas Challenge and the Delilas party swap both rewrite the Delilas arm of \
             the battle AI picker and are mutually exclusive; enable only one"
                .to_string(),
        );
    }
    // Arts AP-grant, shiny-Seru, and the Delilas Challenge dome course reuse the
    // same verified-dead SCUS arena bytes (0x8007AE00). Arts AP-grant is a hard
    // conflict with either (manual-only); shiny-Seru vs the Delilas Challenge is
    // resolved softly below (the challenge wins). Refuse the hard combos here.
    let arts_ap = o.arts_ap();
    // Oscillating AP costs claims all four regions, so it is a hard conflict
    // with every other arena feature.
    let oscillating = o.oscillating();
    for (other, what) in [
        (arts_ap, "the arts AP override"),
        (o.shiny_seru, "shiny-seru"),
        (o.delilas_challenge, "the Delilas Challenge"),
        (o.show_super_arts, "showing Super Arts on the move list"),
        (o.super_arts_pack, "the Super Arts Pack"),
    ] {
        if oscillating && other {
            return Err(format!(
                "oscillating AP costs and {what} both inject into the same verified-dead SCUS \
                 regions and are mutually exclusive; enable only one"
            ));
        }
    }
    if arts_ap && o.shiny_seru {
        return Err(
            "the arts AP override and shiny-seru both inject into the same verified-dead SCUS \
             regions and are mutually exclusive; enable only one"
                .to_string(),
        );
    }
    if arts_ap && o.delilas_challenge {
        return Err(
            "the arts AP override and the Delilas Challenge both inject into the same \
             verified-dead SCUS regions and are mutually exclusive; enable only one"
                .to_string(),
        );
    }
    // The Super Arts move-list rows span all four verified-dead SCUS regions
    // (a shared unlock leaf, three hook routines and four small tables), so
    // they are a hard conflict with all three - the Delilas Challenge included,
    // which is never silently dropped in their favour.
    for (other, what) in [
        (arts_ap, "the arts AP override"),
        (o.shiny_seru, "shiny-seru"),
        (o.delilas_challenge, "the Delilas Challenge"),
    ] {
        if o.show_super_arts && other {
            return Err(format!(
                "showing Super Arts on the move list and {what} both inject into the same \
                 verified-dead SCUS regions and are mutually exclusive; enable only one"
            ));
        }
    }
    // The Super Arts Pack hosts its battle-load stub in the same arena and
    // rewrites the applier the move list detours, so it conflicts with all four.
    for (other, what) in [
        (arts_ap, "the arts AP override"),
        (o.shiny_seru, "shiny-seru"),
        (o.delilas_challenge, "the Delilas Challenge"),
        (o.show_super_arts, "showing Super Arts on the move list"),
    ] {
        if o.super_arts_pack && other {
            return Err(format!(
                "the Super Arts Pack and {what} both inject into the same verified-dead SCUS \
                 regions and are mutually exclusive; enable only one"
            ));
        }
    }
    Ok(())
}

/// What one step of a stage returns: `Err` aborts the whole patch with that
/// message; a recoverable problem is a summary line instead.
pub(super) type StepResult = Result<(), String>;

/// One step of the run: a feature block that edits the image and appends its
/// own summary lines.
type Step = fn(&mut PatchRun<'_>) -> StepResult;

/// One progress stage: the label the page shows, and the steps that run
/// under it, in order.
struct Stage {
    label: &'static str,
    steps: &'static [Step],
}

/// One patch run: the options, the resolved seed, the image being patched
/// and the report the steps accumulate.
pub(super) struct PatchRun<'a> {
    opts: &'a PatchOptions<'a>,
    seed: u64,
    patcher: DiscPatcher,
    /// The valid item pool the `random` drop / chest / steal modes draw from
    /// (empty when none of them is on).
    pool: Vec<u8>,
    /// Whether any `random` mode draws from `pool`.
    needs_pool: bool,
    /// The unused-enemy id set passed to the encounter randomizer (empty
    /// unless on).
    unused_enemy_ids: &'static [u8],
    /// The human-readable change report.
    summary: String,
    /// The parsed language pack, set by the dialog phase when one is given.
    parsed_pack: Option<LanguagePack>,
    /// Both language phases' import reports, merged.
    lang_report: ImportReport,
    /// Per-section coverage of the language pack, set by the name-table
    /// phase when a pack is applied.
    lang_sections: Option<Vec<legaia_translate::SectionCounts>>,
}

impl<'a> PatchRun<'a> {
    /// Parse the disc image and build the item pool the `random` modes share.
    fn open(image: Vec<u8>, opts: &'a PatchOptions<'a>, seed: u64) -> Result<Self, String> {
        let PatchOptions {
            drops,
            chests,
            steals,
            unused_items,
            unused_enemies,
            ..
        } = *opts;
        let drops_mode = parse_mode(drops);
        let chest_mode = parse_mode(chests);
        let steal_mode = parse_mode(steals);
        let mut patcher = DiscPatcher::open(image).map_err(|e| format!("parse disc: {e}"))?;

        // The valid item pool (from SCUS) is needed only by the `random` modes.
        // Shops build their own sellable pool internally, so they don't need the
        // general valid-item pool.
        let needs_pool = drops_mode == Some(DropMode::Random)
            || chest_mode == Some(DropMode::Random)
            || steal_mode == Some(DropMode::Random);
        let mut pool = if needs_pool {
            let scus = legaia_iso::iso9660::read_file_in_image(patcher.image(), "SCUS_942.54")
                .ok_or_else(|| {
                    "SCUS_942.54 not found in disc image (needed for a random mode)".to_string()
                })?;
            valid_item_pool(&scus).map_err(|e| format!("item pool: {e}"))?
        } else {
            Vec::new()
        };
        // `--unused-items`: widen the random-fill pool with the curated unused items
        // (the unnamed accessory in particular is otherwise excluded - no name), and
        // give that accessory the name "Seru Bell" so it doesn't show as a blank.
        if unused_items && needs_pool {
            legaia_patcher::unused::extend_pool(&mut pool, legaia_patcher::unused::UNUSED_ITEM_IDS);
            apply::inject_seru_bell_name(&mut patcher).map_err(|e| format!("name inject: {e}"))?;
        }
        // The unused-enemy id set passed to the encounter randomizer (empty unless on).
        let unused_enemy_ids: &[u8] = if unused_enemies {
            legaia_patcher::unused::UNUSED_ENEMY_IDS
        } else {
            &[]
        };
        Ok(Self {
            opts,
            seed,
            patcher,
            pool,
            needs_pool,
            unused_enemy_ids,
            summary: String::new(),
            parsed_pack: None,
            lang_report: ImportReport::default(),
            lang_sections: None,
        })
    }
}

/// The run, in order: one entry per progress stage between parsing the image
/// and assembling the result.
///
/// The order is part of a seed's meaning. A later step reads what an earlier
/// one wrote (the stat scale reads the stat shuffle's roster, several steps
/// re-pack the same scene MAN, the code hooks share one free-space ledger),
/// so a step is added at its place in the list, never moved. The language
/// pack brackets everything: dialog text before any pass relocates a MAN
/// record, name tables after every pass that classifies items by their
/// English names.
const STAGES: &[Stage] = &[
    Stage {
        label: "custom items",
        steps: &[delilas::custom_items],
    },
    Stage {
        label: "language pack: dialog text",
        steps: &[language::dialog_text],
    },
    Stage {
        label: "monster drops",
        steps: &[monsters::drops],
    },
    Stage {
        label: "equipment bonus drops",
        steps: &[battle_hooks::equipment_drops],
    },
    Stage {
        label: "random encounters",
        steps: &[world::encounters],
    },
    Stage {
        label: "run-away EXP hook",
        steps: &[battle_hooks::flee_exp],
    },
    Stage {
        label: "enemy-ally hook",
        steps: &[battle_hooks::enemy_ally],
    },
    Stage {
        label: "enemy HP bars",
        steps: &[battle_hooks::enemy_hp_bar],
    },
    Stage {
        label: "shiny Seru",
        steps: &[battle_hooks::shiny_seru],
    },
    Stage {
        label: "Super Arts move list",
        steps: &[arts::show_super_arts],
    },
    Stage {
        label: "Super Arts Pack (by ZetaPhoenix)",
        steps: &[arts::super_arts_pack],
    },
    Stage {
        label: "Seru trading",
        steps: &[world::seru_trade],
    },
    Stage {
        label: "jewel fix",
        steps: &[battle_hooks::jewel_fix],
    },
    Stage {
        label: "approach-softlock fix",
        steps: &[battle_hooks::approach_softlock_fix],
    },
    Stage {
        label: "Fortress encounter fix",
        steps: &[world::fortress_encounter_fix],
    },
    Stage {
        label: "Conkram freeze fix",
        steps: &[world::conkram_freeze_fix],
    },
    Stage {
        label: "Delilas Challenge",
        steps: &[delilas::delilas_challenge],
    },
    Stage {
        label: "prices and gauge tuning",
        steps: &[
            world::fishing_prices,
            world::earth_egg_price,
            battle_hooks::spirit_ap,
            battle_hooks::damage_ap,
            arts::oscillating_ap,
        ],
    },
    Stage {
        label: "location renames",
        steps: &[world::location_renames],
    },
    Stage {
        label: "arts tuning",
        steps: &[
            arts::arts_power,
            arts::super_art_power,
            arts::arts_ap_override,
        ],
    },
    Stage {
        label: "treasure chests",
        steps: &[world::chests],
    },
    Stage {
        label: "town shops",
        steps: &[world::shops],
    },
    Stage {
        label: "casino prizes",
        steps: &[world::casino],
    },
    Stage {
        label: "monster stats",
        steps: &[monsters::monster_stats],
    },
    Stage {
        label: "enemy tuning scales",
        steps: &[
            monsters::enemy_stat_scale,
            monsters::exp_scale,
            monsters::seru_catch_rate,
        ],
    },
    Stage {
        label: "Delilas party swap",
        steps: &[delilas::delilas_party, monsters::enemy_attack_count],
    },
    Stage {
        label: "move powers",
        steps: &[monsters::move_power],
    },
    Stage {
        label: "element affinity",
        steps: &[monsters::element_affinity],
    },
    Stage {
        label: "spell costs",
        steps: &[monsters::spell_cost],
    },
    Stage {
        label: "equipment bonuses",
        steps: &[equipment::equip_bonus],
    },
    Stage {
        label: "weapon specialty",
        steps: &[equipment::weapon_specialty],
    },
    Stage {
        label: "equipment edits",
        steps: &[equipment::equipment_edits],
    },
    Stage {
        label: "steal items",
        steps: &[monsters::steals],
    },
    Stage {
        label: "arts combos",
        steps: &[arts::arts_combos],
    },
    Stage {
        label: "doors",
        steps: &[world::doors],
    },
    Stage {
        label: "house doors",
        steps: &[world::house_doors],
    },
    Stage {
        label: "starting items",
        steps: &[starting::starting_items],
    },
    Stage {
        label: "starting level",
        steps: &[starting::starting_level],
    },
    Stage {
        label: "language pack: name tables",
        steps: &[language::name_tables],
    },
];

/// Number of progress boundaries in [`patch_rom`] (the `stage_count` every
/// progress-callback invocation carries): parsing the image, every entry of
/// [`STAGES`], assembling the result.
pub(super) const PATCH_ROM_STAGES: u32 = STAGES.len() as u32 + 2;

/// Apply every stage to `image`: refuse conflicting options, parse the disc,
/// then run [`STAGES`] in order, reporting each boundary to `prog`. Returns
/// the finished run, its image not yet taken; no JS value is touched unless
/// `prog` holds a callback.
async fn run_stages<'a>(
    image: Vec<u8>,
    opts: &'a PatchOptions<'a>,
    prog: &mut Progress,
) -> Result<PatchRun<'a>, String> {
    let seed_n = seed_from_str(opts.seed);
    check_option_conflicts(opts)?;
    prog.stage("parsing disc image").await;
    let mut run = PatchRun::open(image, opts, seed_n)?;
    for stage in STAGES {
        prog.stage(stage.label).await;
        for step in stage.steps {
            step(&mut run)?;
        }
    }
    Ok(run)
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
/// an enemy you must defeat. `enemy_hp_bar` draws a red HP gauge over every
/// living monster (the `HP` label chip + the AP meter's gauge primitive and
/// numeral, no chrome, stepping per hit; one row per monster slot along the
/// top of the screen, tracking each monster's screen X),
/// from a detour at the damage-popup renderer with the routine laid over four
/// routines retail never references - no arena bytes, composes with everything.
/// `shiny_seru` injects code hooks so that, with a
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
/// `fortress_encounter_fix` rewrites the leading encounter group of both
/// Absolute Fortress scenes (`deroa`, `chitei2`) so they stop reading the
/// Floating Castle's stale "no battles" flag - a save that left the castle
/// with under a minute on the escape clock gets its Fortress battles (and the
/// Door of Light there) back; a healthy save is unaffected (seedless).
/// `conkram_freeze_fix` makes the move-VM parts' two heap allocations
/// NULL-safe, so the Room of Mirrors scene in present-day Conkram no longer
/// locks up on an emulator that runs the CPU faster than a console: a failed
/// pose block drops the part, a failed render node gets a shared stand-in,
/// and the failure leaves the `PRG ERR` readout clear; every successful
/// allocation runs as retail (seedless).
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
/// still read "View Hyper Arts list". `super_arts_pack` installs the **Super
/// Arts Pack by ZetaPhoenix**: fifteen extra Super Arts, five per
/// character, each with its own name, hit count and animation, from his own
/// 3764-byte block (parked in the disc's `DMY.DAT` annex, streamed to
/// `0x801FD000` at battle load). Mutually exclusive with the same four features
/// as `show_super_arts`, plus `show_super_arts` itself. Mutually exclusive with
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
/// drains it) - the damage finisher's scale chain in the same overlay.
/// `oscillating_ap` (`0..=100`, empty = off) turns on oscillating AP costs:
/// every battle each Tactical Art is dealt onto the cost side (retail) or the
/// grant side (castable at any AP, gives its AP back, deals that percent of
/// its damage) - it claims the same dead regions as every other arena feature
/// and is refused alongside any of them. A
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
/// mismatch) are counted in the summary but never abort the patch.
/// `lang_relayout` lets a scene whose translated dialog no longer fits its
/// compressed footprint grow by whole sectors (the CLI's `translate import
/// --allow-relayout`): the image gets larger and the dialog lands at full
/// length instead of rolling lines back to English. It only matters with a
/// pack, and it runs in the dialog phase, before any randomizer pass, so every
/// later index-keyed edit resolves against the relaid-out disc. Returns
/// `{ data, summary, seed, lang }`.
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
    lang_relayout: bool,
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
    oscillating_ap: &str,
    enemy_stat_scale: &str,
    exp_scale: &str,
    seru_catch_rate: &str,
    delilas_party: &str,
    delilas_arts_voice: &str,
    delilas_moves: &str,
    super_art_powers: &str,
    show_super_arts: bool,
    super_arts_pack: bool,
    enemy_attack_count: &str,
    swing_costs: &str,
    equip_owners: &str,
    enemy_hp_bar: bool,
    fortress_encounter_fix: bool,
    conkram_freeze_fix: bool,
    progress: Option<js_sys::Function>,
) -> Result<JsValue, JsValue> {
    let opts = PatchOptions {
        seed,
        lang_pack,
        lang_relayout,
        drops,
        encounters,
        encounter_scope,
        chests,
        shops,
        casino,
        steals,
        arts,
        doors,
        door_coupling,
        house_doors,
        starting_items,
        door_of_wind,
        incense,
        speed_chain,
        chicken_heart,
        good_luck_bell,
        all_warps,
        unused_enemies,
        unused_items,
        equipment_drops,
        monster_stats,
        move_power,
        element_affinity,
        spell_cost,
        equip_bonus,
        weapon_specialty,
        starting_level,
        solo_strong_encounters,
        flee_exp,
        seru_trade,
        enemy_ally,
        shiny_seru,
        jewel_fix,
        approach_softlock_fix,
        delilas_challenge,
        custom_items,
        fishing_prices,
        location_renames,
        earth_egg_price,
        arts_powers,
        arts_ap_grants,
        arts_ap_costs,
        spirit_ap,
        damage_ap,
        oscillating_ap,
        enemy_stat_scale,
        exp_scale,
        seru_catch_rate,
        delilas_party,
        delilas_arts_voice,
        delilas_moves,
        super_art_powers,
        show_super_arts,
        super_arts_pack,
        enemy_attack_count,
        swing_costs,
        equip_owners,
        enemy_hp_bar,
        fortress_encounter_fix,
        conkram_freeze_fix,
    };
    let mut prog = Progress::new(progress, PATCH_ROM_STAGES);
    let run = run_stages(image, &opts, &mut prog).await.map_err(err)?;
    // Per-section rows live in the `lang` JSON object; the page renders
    // them as the coverage block, so the text summary stays one line.
    let lang_json = match (&run.parsed_pack, &run.lang_sections) {
        (Some(pack), Some(sections)) => {
            lang_report_json(&pack.language, &run.lang_report, sections)?
        }
        _ => JsValue::NULL,
    };

    prog.stage("assembling patched image").await;
    let PatchRun {
        patcher,
        summary,
        seed,
        ..
    } = run;
    let patched = patcher.into_image();
    let data = Uint8Array::new_with_length(patched.len() as u32);
    data.copy_from(&patched);

    let out = Object::new();
    Reflect::set(&out, &"data".into(), &data)?;
    Reflect::set(&out, &"summary".into(), &summary.into())?;
    Reflect::set(&out, &"seed".into(), &seed.to_string().into())?;
    Reflect::set(&out, &"lang".into(), &lang_json)?;
    Ok(out.into())
}
