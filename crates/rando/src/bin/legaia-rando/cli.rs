//! Command-line interface: the `clap` argument structs, the value-enums that
//! back the randomizer options, and the small conversions from those enums into
//! the `legaia_rando::apply` mode types.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

use legaia_rando::apply;
use legaia_rando::drops::DropMode;

use crate::util::parse_item_spec;

#[derive(Parser)]
#[command(
    name = "legaia-rando",
    about = "Legend of Legaia randomizer / disc patcher (operates on a user-supplied disc)"
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) cmd: Cmd,
}

#[derive(Subcommand)]
pub(crate) enum Cmd {
    /// Plan a randomization from a seed and write a PPF patch (and optionally a
    /// patched disc image copy).
    Randomize(RandomizeArgs),
    /// Read-only: list every monster's current item drop.
    Drops {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: list every treasure chest the randomizer would touch, grouped
    /// by scene, with the item each currently gives. Use this to audit which
    /// items would change (e.g. to spot quest items that should stay static).
    Chests {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: list every monster's current steal item (Evil God Icon),
    /// with its steal chance, from the static `SCUS_942.54` steal table.
    Steals {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: list every Tactical Art's current button combo, grouped by
    /// character, from the static `SCUS_942.54` arts-name table.
    Arts {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: list every scene-transition door/exit the randomizer can
    /// touch, grouped by the scene it lives in, with the destination each
    /// currently leads to.
    Doors {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: list the intra-town (house / interior) door-warp target
    /// tiles the house-door shuffle would touch, grouped by scene - the
    /// cross-context player MOVE_TOs in named partition-0 door records (NPC /
    /// cutscene movement is excluded by construction).
    HouseDoors {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: list the `.MAP` kind-0 intra-scene teleports (the map-data
    /// door class most house exits belong to - no script, no MAN record),
    /// grouped by scene, with each record's walk-component class. The
    /// population `--house-doors shuffle` rewires alongside the script warps.
    MapDoors {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: show the new game's current starting inventory (the
    /// `(item, count)` slots a New Game begins with - vanilla is Healing Leaf
    /// ×5).
    StartingItems {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: list every town-merchant shop and what it sells, grouped by
    /// scene, with item names.
    Shops {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: list the casino prize-exchange prizes (item, coin price,
    /// progression gate).
    Casino {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: list every monster's current combat stats (HP / MP / ATK /
    /// UDF / LDF / AGL / SPD) from the `battle_data` archive - the population
    /// the `--monster-stats` randomizer redistributes.
    MonsterStats {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: list the special-attack move-power table (the 44 power values
    /// the `--move-power` randomizer redistributes), each tagged with the
    /// spell-table name of a move id that resolves to it.
    MovePowers {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: print the 8×8 element-affinity matrix (rows = attacking
    /// element, columns = defending element; each cell a damage-scale percent)
    /// the `--element-affinity` randomizer redistributes.
    Affinity {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: list every named spell's current MP cost from the SCUS spell
    /// table - the population the `--spell-cost` randomizer redistributes.
    SpellCosts {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: list the equipment stat-bonus table (`DAT_80074F68`), grouped
    /// by slot category, with each row's stats and the items that reference it -
    /// the population the `--equip-bonus` randomizer redistributes.
    EquipBonuses {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352).
        #[arg(long)]
        input: PathBuf,
    },
    /// Read-only: show each character's current favored weapon class (read from
    /// the player battle files) - what the `--weapon-specialty` randomizer
    /// permutes.
    WeaponSpecialty {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352).
        #[arg(long)]
        input: PathBuf,
    },
    /// Apply a PPF patch to a copy of a disc and confirm it applies cleanly
    /// (records applied, the result still parses). Use this to check that a
    /// shared patch + seed match your own disc before playing.
    Verify {
        /// Path to the user's retail disc image the patch targets.
        #[arg(long)]
        input: PathBuf,
        /// The PPF 3.0 patch to apply.
        #[arg(long)]
        patch: PathBuf,
        /// Optionally write the patched image here (for local play only).
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Translation / language-pack tools: export the disc's text to an
    /// editable YAML pack, generate per-language skeletons, check coverage,
    /// and import a filled pack back onto a disc copy.
    Translate {
        #[command(subcommand)]
        cmd: TranslateCmd,
    },
}

#[derive(Subcommand)]
pub(crate) enum TranslateCmd {
    /// Export every cataloged user-facing string (item / spell / art /
    /// accessory / party names, scene dialog, event-script text) from a disc
    /// into a YAML language pack with empty `translation:` fields.
    ///
    /// The exported pack contains the game's copyrighted text - keep it
    /// local / share only filled translations per your jurisdiction's rules;
    /// never commit it to this repository.
    Export {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352).
        #[arg(long)]
        input: PathBuf,
        /// Where to write the pack (YAML).
        #[arg(long, short)]
        output: PathBuf,
    },
    /// Produce an empty per-language skeleton from an exported pack: same
    /// keys / sources / budgets, cleared translations, stamped header.
    ///
    /// `--resume` seeds it with the translations of an already-published
    /// (source-less) pack, so a translator can pick up where a shipped pack
    /// left off without anyone redistributing the source text.
    Init {
        /// Target language code (e.g. fr, de, es, it, pt-BR, ja, ru, zh, ko -
        /// note: non-Latin scripts also need a font patch, see the docs).
        #[arg(long)]
        lang: String,
        /// An existing exported pack to derive from...
        #[arg(long, conflicts_with = "input", required_unless_present = "input")]
        from: Option<PathBuf>,
        /// ...or export straight from a disc image.
        #[arg(long, required_unless_present = "from")]
        input: Option<PathBuf>,
        /// Contributor names for the pack header (repeatable).
        #[arg(long)]
        contributor: Vec<String>,
        /// Pre-fill from an existing (working or distributable) pack, matched
        /// by key - e.g. one of the shipped `site/lang/*.yaml` packs.
        #[arg(long)]
        resume: Option<PathBuf>,
        /// Also split the skeleton into chunk files of at most N entries each
        /// (`<output stem>.001.yaml`, ...) for a parallel / bulk fill pass.
        /// Recombine them with `translate merge`.
        #[arg(long, value_name = "N")]
        chunk: Option<usize>,
        /// Where to write the skeleton (YAML).
        #[arg(long, short)]
        output: PathBuf,
    },
    /// Strip a filled pack down to the **distributable** shape: the filled
    /// entries only, keys + your translations + the byte-budget hint, with
    /// every `source:` / `context:` field (the game's own text) removed.
    ///
    /// This is the shape that is safe to publish / commit.
    Strip {
        /// The filled working pack (YAML).
        #[arg(long)]
        pack: PathBuf,
        /// Where to write the distributable pack (YAML).
        #[arg(long, short)]
        output: PathBuf,
        /// Overwrite the pack's `notes:` header line.
        #[arg(long)]
        notes: Option<String>,
    },
    /// Merge the filled entries of several packs (chunks of a bulk fill, a
    /// shipped pack + your edits, ...) into the first one, matched by key.
    Merge {
        /// Base pack - defines the entry set (keys / sources / budgets).
        #[arg(long)]
        base: PathBuf,
        /// Packs whose translations are merged onto the base, in order.
        #[arg(long = "pack", required = true)]
        packs: Vec<PathBuf>,
        /// Where to write the merged pack (YAML).
        #[arg(long, short)]
        output: PathBuf,
    },
    /// Coverage + validation report for a pack: per-section translated/total
    /// counts, plus encodability and budget checks on every filled entry.
    ///
    /// Without `--input` this is an offline check against the pack's own
    /// budgets. With `--input` it is a full dry run against a real disc: every
    /// entry is planned exactly as `import` would (in memory, nothing is
    /// written), which is the only way to validate a distributable pack's
    /// budgets - they are hints until a disc is there to measure.
    Stats {
        /// The language pack (YAML).
        #[arg(long)]
        pack: PathBuf,
        /// Dry-run the pack against this disc image (`.bin`, Mode 2/2352).
        #[arg(long)]
        input: Option<PathBuf>,
    },
    /// Cross-region corpus alignment report: compare a **target** disc (the
    /// one the importer would patch, e.g. the retail NTSC/USA build) against an
    /// **official localization** disc (a PAL SCES build) and quantify how well
    /// the dialog corpus aligns id-/order-for-order and how much of the
    /// official text fits the target's same-size budget. Emits counts and byte
    /// values only - no game text - so it is safe to run and log. Use it to
    /// judge whether an official translation can be lifted into a distributable
    /// pack for the target disc.
    DiffDisc {
        /// The target disc the importer patches (`.bin`, Mode 2/2352).
        #[arg(long)]
        input: PathBuf,
        /// The other (official-localization) disc to align against.
        #[arg(long)]
        other: PathBuf,
    },
    /// Lift an **official PAL localization** (FR/DE/IT SCES disc) into a
    /// USA-keyed working pack: name tables id-for-id, dialog by positional
    /// segment pairing. Emits a filled pack (source = USA text, translation =
    /// official localized text) to `-o`.
    ///
    /// The output carries the game's copyrighted text - keep it local, never
    /// commit it. Only `translate strip`-ed distributable packs are shareable.
    LiftOfficial {
        /// The official-localization disc to lift from (`.bin`, a PAL SCES
        /// build - SCES_019.44 FR / .45 DE / .46 IT).
        #[arg(long)]
        from: PathBuf,
        /// The USA target disc whose coordinate space the pack is keyed to.
        #[arg(long)]
        target: PathBuf,
        /// Where to write the filled working pack (YAML). Scratchpad only.
        #[arg(long, short)]
        output: PathBuf,
    },
    /// Apply a filled pack to a copy of a disc. Untranslated entries are
    /// left byte-identical; every write is same-size in place and each
    /// touched sector's EDC/ECC is re-encoded.
    Import {
        /// Path to the user's retail disc image (`.bin`, Mode 2/2352).
        #[arg(long)]
        input: PathBuf,
        /// The filled language pack (YAML).
        #[arg(long)]
        pack: PathBuf,
        /// Write the patched image here (contains Sony bytes - local play
        /// only, never redistribute).
        #[arg(long)]
        output: Option<PathBuf>,
        /// Write a portable PPF 3.0 patch here (safe to share).
        #[arg(long)]
        patch: Option<PathBuf>,
    },
}

#[derive(Parser)]
pub(crate) struct RandomizeArgs {
    /// Path to the user's retail disc image (`.bin`, Mode 2/2352).
    #[arg(long)]
    pub(crate) input: PathBuf,
    /// Seed for reproducibility. Either a number (decimal or `0x`-hex) or any
    /// string (hashed to a number). The resolved numeric seed is always
    /// printed so a run can be reproduced exactly. If omitted, one is drawn
    /// from the system clock.
    #[arg(long)]
    pub(crate) seed: Option<String>,
    /// How monster item drops are reassigned.
    #[arg(long, value_enum, default_value_t = DropArg::Shuffle)]
    pub(crate) drops: DropArg,
    /// Inject an *additional* low-chance equipment drop into the battle-end
    /// reward routine (a same-size `SCUS_942.54` code hook). On a per-battle
    /// roll it grants one extra random weapon / armor / accessory **on top of**
    /// the normal drop - the regular drop table (vanilla or `--drops`) is never
    /// disturbed.
    #[arg(long, default_value_t = false)]
    pub(crate) equipment_drops: bool,
    /// Per-battle chance (percent) for the `--equipment-drops` bonus drop.
    #[arg(long, default_value_t = legaia_rando::bonus_drop::DEFAULT_CHANCE_PCT)]
    pub(crate) equipment_drop_chance: u8,
    /// Bank a slice of the formation's experience into the party whenever they
    /// **successfully run away** (a same-size code hook into the battle-action
    /// escape teardown). Vanilla awards nothing for fleeing; this credits
    /// `--flee-exp-pct`% of the fled fight's EXP to every party member.
    #[arg(long, default_value_t = false)]
    pub(crate) flee_exp: bool,
    /// Percentage of the formation's experience banked on a successful escape
    /// (only with `--flee-exp`).
    #[arg(long, default_value_t = legaia_rando::flee_exp::DEFAULT_PCT)]
    pub(crate) flee_exp_pct: u8,
    /// With `--enemy-ally-pct`% probability per battle, **charm a random enemy**
    /// onto the party's side as an uncontrolled ally (a same-size code hook into
    /// battle setup that sets the AI-delegated bits on the frontmost enemy, plus a
    /// one-word widen of the victory check so the ally isn't an enemy you must
    /// defeat). Fires only in **multi-enemy** fights - single-enemy fights (every
    /// input-gated tutorial and solo boss) are skipped, since charming the lone
    /// enemy of a scripted fight softlocks it.
    #[arg(long, default_value_t = false)]
    pub(crate) enemy_ally: bool,
    /// Per-battle percentage chance an enemy is charmed (only with `--enemy-ally`).
    #[arg(long, default_value_t = legaia_rando::enemy_ally::DEFAULT_PCT)]
    pub(crate) enemy_ally_pct: u8,
    /// With `--shiny-pct`% probability per battle, the frontmost **capturable**
    /// enemy spawns as a rare **shiny** variant: +35% stats (translucent), and the
    /// Seru you capture from it deals +35% damage forever, with a translucent
    /// summon + a "+35% DMG!" cast caption (a same-size code hook into battle
    /// setup + the capture/damage/draw paths; the persistent shiny flag is a
    /// parallel per-spell byte at `record+0x1C0`, and every injected routine lives
    /// in verified-dead SCUS space outside all live tables).
    #[arg(long, default_value_t = false)]
    pub(crate) shiny_seru: bool,
    /// Per-battle percentage chance a capturable enemy is shiny (only with
    /// `--shiny-seru`).
    #[arg(long, default_value_t = legaia_rando::shiny_seru::DEFAULT_PCT)]
    pub(crate) shiny_pct: u8,
    /// Let vendors offer to **trade** one of a character's seru for a different
    /// seru. Embeds an enabled flag + the run's seed in `SCUS_942.54`; the
    /// clean-room engine renders the trade UI and reseeds each vendor's offers
    /// every two in-game hours. (Inert on real hardware - retail has no trade UI.)
    #[arg(long, default_value_t = false)]
    pub(crate) seru_trade: bool,
    /// Maximum trades a single vendor offers at once (only with `--seru-trade`).
    #[arg(long, default_value_t = legaia_asset::seru_trade::DEFAULT_MAX_OFFERS)]
    pub(crate) seru_trade_offers: u8,
    /// How random-encounter formations are reassigned. The pool each scene draws
    /// from is set by `--encounter-scope`.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) encounters: DropArg,
    /// Pool an encounter randomization draws from: `scene` (each scene's own
    /// monsters - the default, every monster stays a local resident),
    /// `kingdom` (any monster from the same kingdom: Drake / Sebucus / Karisto),
    /// or `world` (any monster on the disc, so late-game monsters can appear at
    /// the start). Only applies when `--encounters` is set. `kingdom` needs the
    /// disc's CDNAME.TXT; the wider pools rely on the battle loader streaming a
    /// monster by id, so an out-of-area enemy still loads and renders.
    #[arg(long, value_enum, default_value_t = ScopeArg::Scene)]
    pub(crate) encounter_scope: ScopeArg,
    /// Opt out of the solo-strong pass. It is on by default whenever
    /// `--encounters` is set: a randomized fight that would pit the party against
    /// a monster much stronger than the area's natives is forced to just that one
    /// monster instead of a pack of 2+ (cut-off `--solo-strong-threshold`). Pass
    /// this to keep the over-strong packs (vanilla behaviour for the formation
    /// counts).
    #[arg(long)]
    pub(crate) no_solo_strong_encounters: bool,
    /// "Strong fight" cut-off for the solo-strong pass, as a percent of the area's
    /// native average monster power (default 200 = twice as strong). A random
    /// formation whose strongest monster clears this bar is forced solo. Ignored
    /// with `--no-solo-strong-encounters`.
    #[arg(long, default_value_t = apply::DEFAULT_SOLO_STRONG_THRESHOLD_PCT)]
    pub(crate) solo_strong_threshold: u16,
    /// How treasure-chest contents are reassigned (global; `random` draws from
    /// the valid item pool, `shuffle` redistributes the existing chest items).
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) chests: DropArg,
    /// How town-merchant shops are reassigned - what stores sell (global;
    /// `shuffle` redistributes the existing shop-item multiset across all towns,
    /// `random` draws each slot from the valid item pool). The town shop stock is
    /// inline in each scene's field-VM script (op `0x49`).
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) shops: DropArg,
    /// How the casino prize-exchange is reassigned (`shuffle` redistributes the
    /// existing prizes, `random` draws from the existing prize pool; each prize
    /// keeps its coin price + progression gate). Distinct from `--shops`: the
    /// casino spends coins, not gold.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) casino: DropArg,
    /// How monster combat stats are reassigned (HP / MP / ATK / DEF / AGL / SPD
    /// from the `battle_data` archive). `shuffle` permutes each stat column
    /// across the roster (each stat's multiset preserved, so the overall
    /// difficulty budget is kept); `random` draws each stat from that column's
    /// pool. Spirit/SP is left untouched. `legaia-rando monster-stats` lists the
    /// current stats.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) monster_stats: DropArg,
    /// How special-attack power is reassigned (the battle-action move-power
    /// table - enemy specials + Seru-magic, NOT party Tactical Arts). `shuffle`
    /// permutes the 44 power values (multiset preserved); `random` draws each
    /// from that pool. Only the power changes - each move keeps its own
    /// animation, effects, and sound. `legaia-rando move-powers` lists them.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) move_power: DropArg,
    /// How the element-affinity matrix is reassigned (which element beats which:
    /// the 8×8 damage-scale grid). `shuffle` permutes the 64 cells (the same
    /// number of weaknesses / resistances exists, between different pairs);
    /// `random` draws each cell from that pool. Per-character element assignment
    /// is left untouched. `legaia-rando affinity` shows the current grid.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) element_affinity: DropArg,
    /// How spell MP costs are reassigned (the SCUS spell table). `shuffle`
    /// permutes the MP costs of the named, costed spells (the cost multiset is
    /// preserved); `random` draws each from that pool. Free / internal-tier
    /// spells never gain a cost. `legaia-rando spell-costs` lists them.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) spell_cost: DropArg,
    /// How equipment passive stat bonuses are reassigned (the SCUS bonus table).
    /// `shuffle` permutes each slot category's stat tuples (`INT/ATK/UDF/LDF/SPD`)
    /// among that category's gear (so weapon power lands on another weapon, armor
    /// on armor; the per-category budget is kept); `random` draws each from that
    /// category's pool. The equip-character mask, accessory passive, and slot type
    /// never move. `legaia-rando equip-bonuses` lists the current table.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) equip_bonus: DropArg,
    /// Reassign which weapon class each character specializes in (Vahn blades,
    /// Noa claws, Gala clubs/axes by default). Permutes the three favored
    /// families among the characters and rewrites the per-(character, weapon)
    /// arm-cost byte in the player battle files, so an off-class weapon widens
    /// the Arms command in an arts combo. The Astral Sword stays always-wide.
    /// `legaia-rando weapon-specialty` shows the current favored class per char.
    #[arg(long, default_value_t = false)]
    pub(crate) weapon_specialty: bool,
    /// How per-monster steal items are reassigned (the Evil God Icon table;
    /// `shuffle` redistributes the existing steal items, `random` draws from the
    /// valid item pool - the steal *chance* is always preserved).
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) steals: DropArg,
    /// How each Tactical Art's button combo is reassigned. `shuffle` permutes a
    /// character's own combos among its arts; `random` draws each art a combo
    /// from the global pool of every regular art's combo. Either way every art
    /// keeps a combo that's unique within its character, and the Miracle Art is
    /// left untouched. `legaia-rando arts` lists current combos.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) arts: DropArg,
    /// How scene-transition doors/exits are reassigned (one-way / decoupled:
    /// each door's whole destination - scene + entry tile + facing - is
    /// reassigned globally; `shuffle` permutes the existing destinations across
    /// all doors, `random` draws each from the global pool). Going back through
    /// the destination's own doors is not guaranteed to return you.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) doors: DropArg,
    /// Whether door randomization is bidirectional (`coupled`: re-pair doors so
    /// you can return the way you came) or one-way (`decoupled`: each door's
    /// destination is independent, so going back leads elsewhere). Only applies
    /// when `--doors` is not `none`.
    #[arg(long, value_enum, default_value_t = CouplingArg::Coupled)]
    pub(crate) door_coupling: CouplingArg,
    /// How intra-town (house / interior) doors are reassigned. Only `shuffle`
    /// is meaningful; `random` is treated as `none`. Covers both intra-town
    /// door classes: the scripted door warps (a per-scene, class-preserving
    /// shuffle of the player door-warp target tiles: interior landings permute
    /// among house entries, exterior doorsteps among exits) and the `.MAP`
    /// kind-0 intra-scene teleports (most house exits; a per-scene shuffle
    /// accepted only when the scene's walk-component reachability is
    /// preserved, so no rewire can strand the player). `legaia-rando
    /// house-doors` / `map-doors` list the two populations.
    #[arg(long, value_enum, default_value_t = DropArg::None)]
    pub(crate) house_doors: DropArg,
    /// Number of random starting items the new game begins with (`0` = leave the
    /// vanilla Healing Leaf ×5 untouched). Each is a distinct random consumable
    /// with a small random count. The random fill shares the seed's capacity
    /// (7 slots, or 5 with `--all-warps`) with the convenience-item toggles, and
    /// takes whatever they leave - so it adds on top of them rather than being
    /// crowded out. `legaia-rando starting-items` shows the current contents.
    #[arg(long, default_value_t = 0)]
    pub(crate) starting_items: usize,
    /// Seed Door of Wind (the warp consumable) into the new game's starting bag.
    /// Pass `--door-of-wind` for the default stack (10) or `--door-of-wind N` for
    /// N (1..=99). Additive to a normal new game (the Healing Leaf is kept)
    /// unless `--starting-items` also rerolls the bag. Pairs with `--all-warps`.
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "10",
        value_name = "COUNT"
    )]
    pub(crate) door_of_wind: Option<u8>,
    /// Seed Incense (the encounter-rate consumable) into the new game's starting
    /// bag. Pass `--incense` for the default stack (10) or `--incense N` for N
    /// (1..=99). Additive to a normal new game (the Healing Leaf is kept) unless
    /// `--starting-items` also rerolls the bag.
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "10",
        value_name = "COUNT"
    )]
    pub(crate) incense: Option<u8>,
    /// Seed the Speed Chain accessory (always act first in battle) into the new
    /// game's starting bag. Pass `--speed-chain` for the default (1) or
    /// `--speed-chain N` for N (1..=99). Additive like `--door-of-wind`.
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "1",
        value_name = "COUNT"
    )]
    pub(crate) speed_chain: Option<u8>,
    /// Seed the Chicken Heart accessory (increases the successful-escape rate)
    /// into the starting bag. `--chicken-heart` for the default (1) or
    /// `--chicken-heart N`.
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "1",
        value_name = "COUNT"
    )]
    pub(crate) chicken_heart: Option<u8>,
    /// Seed the Good Luck Bell accessory (raises the item-drop rate) into the
    /// starting bag. `--good-luck-bell` for the default (1) or `--good-luck-bell N`.
    #[arg(
        long,
        num_args = 0..=1,
        default_missing_value = "1",
        value_name = "COUNT"
    )]
    pub(crate) good_luck_bell: Option<u8>,
    /// Seed explicit item(s) into the new game's starting bag, on top of the
    /// convenience toggles and the Healing Leaf base. Comma-separated
    /// `id[:count]` entries, id in decimal or `0xHH`, count defaulting to 1
    /// (e.g. `--start-with 0x89:10,0xd1,154:3`). The id space is the full item
    /// table - any consumable, weapon, armor, or accessory id works. Items
    /// beyond the 7-slot direct seed (5 with `--all-warps`) are granted via the
    /// opening scene like the random fill. `legaia-rando starting-items` shows
    /// the resulting bag.
    #[arg(long, value_name = "ID[:COUNT]", value_delimiter = ',', value_parser = parse_item_spec)]
    pub(crate) start_with: Vec<(u8, u8)>,
    /// Unlock every Door-of-Wind warp destination from the start (preset the
    /// "visited towns" story-flag bitmask). Lets Door of Wind teleport to any
    /// town immediately. It claims the warp-preset region that otherwise carries
    /// the last two starting-item slots, so the bag is capped at 5 items with it
    /// on (7 without).
    #[arg(long, default_value_t = false)]
    pub(crate) all_warps: bool,
    /// Start the new game at this character level instead of 1 (`0` or `1` =
    /// vanilla level 1). Seeds the lead character's cumulative XP and recomputes
    /// the starting stats to the level from the disc's own growth curves. Range
    /// 2..=14 (the XP seed is a single 16-bit immediate). `legaia-rando
    /// starting-items` shows the current starting level.
    #[arg(long, default_value_t = 0)]
    pub(crate) starting_level: u8,
    /// Re-introduce unused enemies (the Evil Bat duplicates that no formation
    /// references) into the random-encounter pool. Only takes effect with
    /// `--encounters random` (a `shuffle` can't introduce a new monster).
    #[arg(long, default_value_t = false)]
    pub(crate) unused_enemies: bool,
    /// Re-introduce unused items (the "Something Good" sell item and the unnamed
    /// Seru accessory) into the valid item pool, so a `random` drop / chest /
    /// steal fill can hand them out. Only affects the `random` modes.
    #[arg(long, default_value_t = false)]
    pub(crate) unused_items: bool,
    /// Comma-separated item ids (decimal or `0xHH`) to keep in their original
    /// chests, never randomized - and dropped from the random-fill pool so they
    /// can't be duplicated elsewhere. Defaults to the disc's full quest / key /
    /// story item set (every unsellable item except the chest-found equipment,
    /// so no door key, garden tool, letter, book, or one-off story item is ever
    /// moved or randomly placed). `legaia-rando chests` lists current contents
    /// to audit. Pass an empty value (`--keep-static-items ""`) to randomize
    /// everything.
    #[arg(long, value_delimiter = ',')]
    pub(crate) keep_static_items: Option<Vec<String>>,
    /// Write the portable PPF 3.0 patch here (defaults to `<input>.ppf`).
    #[arg(long)]
    pub(crate) patch: Option<PathBuf>,
    /// Also write a full patched disc-image copy here (contains Sony bytes -
    /// for local play only, never redistribute). A matching `.cue` is written
    /// beside it (single-track Mode 2/2352) so emulators that won't load a bare
    /// BIN - e.g. mednafen rejects a >64 MiB BIN - can open the image directly.
    #[arg(long)]
    pub(crate) output: Option<PathBuf>,
    /// Write a reproducibility manifest (seed + options + change summary) here.
    /// Safe to share alongside the PPF - it embeds no game bytes.
    #[arg(long)]
    pub(crate) manifest: Option<PathBuf>,
    /// Plan and report the run but write no files (patch / output / manifest).
    #[arg(long, default_value_t = false)]
    pub(crate) dry_run: bool,
}

#[derive(Copy, Clone, ValueEnum)]
pub(crate) enum CouplingArg {
    /// Bidirectional - you can return the way you came.
    Coupled,
    /// One-way - going back leads somewhere else.
    Decoupled,
}

impl CouplingArg {
    pub(crate) fn coupling(self) -> apply::DoorCoupling {
        match self {
            CouplingArg::Coupled => apply::DoorCoupling::Coupled,
            CouplingArg::Decoupled => apply::DoorCoupling::Decoupled,
        }
    }
}

#[derive(Copy, Clone, ValueEnum)]
pub(crate) enum DropArg {
    /// Redistribute the existing values (drops / encounter ids).
    Shuffle,
    /// Draw each value uniformly from the valid pool.
    Random,
    /// Leave untouched.
    None,
}

#[derive(Copy, Clone, ValueEnum)]
pub(crate) enum ScopeArg {
    /// Each scene draws only from its own monsters (the classic behaviour).
    Scene,
    /// Each scene draws from any monster in its kingdom (Drake/Sebucus/Karisto).
    Kingdom,
    /// Each scene draws from any monster on the disc (regions fully mixed).
    World,
}

impl ScopeArg {
    pub(crate) fn scope(self) -> apply::EncounterScope {
        match self {
            ScopeArg::Scene => apply::EncounterScope::Scene,
            ScopeArg::Kingdom => apply::EncounterScope::Kingdom,
            ScopeArg::World => apply::EncounterScope::World,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ScopeArg::Scene => "scene",
            ScopeArg::Kingdom => "kingdom",
            ScopeArg::World => "world",
        }
    }
}

/// Lowercase name of a mode for the manifest (valid-TOML string value).
pub(crate) fn mode_str(mode: DropMode) -> &'static str {
    match mode {
        DropMode::Shuffle => "shuffle",
        DropMode::Random => "random",
    }
}

impl DropArg {
    pub(crate) fn mode(self) -> Option<DropMode> {
        match self {
            DropArg::Shuffle => Some(DropMode::Shuffle),
            DropArg::Random => Some(DropMode::Random),
            DropArg::None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_str_is_lowercase() {
        assert_eq!(mode_str(DropMode::Shuffle), "shuffle");
        assert_eq!(mode_str(DropMode::Random), "random");
    }
}
